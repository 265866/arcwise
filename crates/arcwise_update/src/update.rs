//! Finding, downloading and running a newer release's installer.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::Arc;
use std::time::Duration;
use std::{env, io};

use anyhow::Context as _;
use rustls::crypto::aws_lc_rs;
use semver::Version;
use serde::Deserialize;
use sigstore_verify::crypto::hash;
use ureq::Agent;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

use crate::builder::{self, RELEASE_BUILDER};

/// The page people can download releases from themselves.
pub const RELEASES_PAGE: &str = "https://github.com/265866/arcwise/releases/latest";
/// Every release, with what each changed.
pub const RELEASES: &str = "https://github.com/265866/arcwise/releases";
const LATEST_RELEASE: &str = "https://api.github.com/repos/265866/arcwise/releases/latest";
/// Where GitHub serves a release's files, by tag and file name.
const DOWNLOADS: &str = "https://github.com/265866/arcwise/releases/download";
/// The release workflow attaches it to every release.
const ATTESTATION: &str = "attestation.json";
/// How long a downloaded installer for a newer version is kept for another Arcwise to install.
const KEEP_DOWNLOADS: Duration = Duration::from_hours(24);

/// The version of Arcwise that is running.
///
/// # Panics
///
/// Never in practice: Cargo refuses a package version that isn't semver.
#[must_use]
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is semver")
}

/// Whether this exe is the one the installer put in place, beside its uninstaller. An update
/// installs there, so updating any other copy, such as one run from the zip or a development
/// build, would install a second Arcwise and leave this one behind.
#[must_use]
pub fn installed() -> bool {
    env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("unins000.exe")))
        .is_some_and(|uninstaller| uninstaller.is_file())
}

/// A newer installer, downloaded and verified.
#[derive(Clone, Debug)]
pub struct Update {
    /// The version it installs.
    pub version: Version,
    /// Where it was saved.
    pub installer: PathBuf,
}

/// What a download came to.
#[derive(Debug)]
pub enum Outcome {
    /// The installer is downloaded and verified.
    Ready(Update),
    /// The installer could not be shown to come from the release workflow, so it was deleted, or
    /// the reason says why it couldn't be.
    Unverified {
        /// Why, and whether deleting it failed too.
        reason: String,
    },
}

/// What asking GitHub for the latest release found.
#[derive(Debug)]
pub struct Check {
    /// The latest release's version, when it is a newer final version.
    pub newer: Option<Version>,
    /// Files in the updates folder that should have gone but couldn't be deleted, such as an
    /// installer that is still running.
    pub kept: Vec<Kept>,
}

/// A file that couldn't be deleted.
#[derive(Debug)]
pub struct Kept {
    /// The file.
    pub path: PathBuf,
    /// Why it stayed.
    pub error: io::Error,
}

/// Where downloaded installers wait to run: `%LOCALAPPDATA%\Arcwise\updates`.
#[must_use]
pub fn directory() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(|dir| Path::new(&dir).join("Arcwise").join("updates"))
}

/// Asks GitHub for the latest release and returns its version when it is newer than `current`.
/// Installers left in `dir` by earlier runs, such as one already installed, are deleted. Blocks
/// on the network, so it runs off the UI thread.
///
/// # Errors
///
/// When GitHub can't be reached or answers with something other than a release.
pub fn latest(current: &Version, dir: &Path) -> anyhow::Result<Check> {
    let response = agent(current).get(LATEST_RELEASE).call();
    if matches!(response, Err(ureq::Error::StatusCode(404))) {
        return Ok(Check {
            newer: None,
            kept: Vec::new(),
        });
    }
    let mut response = response.context("asking GitHub for the latest release")?;
    let release: Release = serde_json::from_slice(&response.body_mut().read_to_vec()?)
        .context("reading the latest release")?;
    let kept = clear(dir, current);
    Ok(Check {
        newer: newer(current, &release),
        kept,
    })
}

/// Downloads `version`'s installer into `dir` and checks its attestation. Blocks on the network,
/// so it runs off the UI thread. Anything but a verified installer is deleted, or the outcome
/// says why it couldn't be.
///
/// # Errors
///
/// When the download can't finish, such as when offline or while the release is still getting
/// its files.
pub fn fetch(version: &Version, dir: &Path) -> anyhow::Result<Outcome> {
    use std::fmt::Write as _;

    let agent = agent(&current_version());
    let name = installer_name(version);
    let url = |file: &str| format!("{DOWNLOADS}/v{version}/{file}");
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let installer = dir.join(download_name(version, process::id()));
    let checked = (|| {
        download(&agent, &url(&name), &installer)?;
        let attestation = agent
            .get(&url(ATTESTATION))
            .call()?
            .body_mut()
            .read_to_string()
            .context("downloading the attestation")?;
        let digest = hash::sha256_reader(File::open(&installer)?)?;
        anyhow::Ok(builder::verify(
            &attestation,
            &digest,
            &name,
            &RELEASE_BUILDER,
        ))
    })();
    match checked {
        Ok(Ok(())) => Ok(Outcome::Ready(Update {
            version: version.clone(),
            installer,
        })),
        Ok(Err(unverified)) => {
            let mut reason = format!("{unverified:#}");
            if let Err(e) = fs::remove_file(&installer) {
                write!(reason, "; deleting {} failed too: {e}", installer.display())?;
            }
            Ok(Outcome::Unverified { reason })
        }
        Err(failed) => match fs::remove_file(&installer) {
            Ok(()) => Err(failed),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(failed),
            Err(e) => Err(anyhow::anyhow!(
                "{failed:#}; deleting the partial download {} failed too: {e}",
                installer.display()
            )),
        },
    }
}

/// How the installer runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relaunch {
    /// Open the new version once it is installed.
    Yes,
    /// Leave Arcwise closed.
    No,
}

/// Runs the installer silently. It closes Arcwise if it is still running, and with
/// [`Relaunch::Yes`] opens the new version when it is done.
///
/// # Errors
///
/// When the installer can't be started.
pub fn install(update: &Update, relaunch: Relaunch) -> io::Result<()> {
    let mut command = Command::new(&update.installer);
    command.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]);
    if relaunch == Relaunch::Yes {
        command.arg("/RELAUNCH");
    }
    command.spawn().map(drop)
}

/// Deletes the installer of an update that will not be installed.
///
/// # Errors
///
/// When the file can't be deleted.
pub fn discard(update: Update) -> io::Result<()> {
    fs::remove_file(update.installer)
}

/// The part of GitHub's release JSON the check reads.
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

/// The release's version, when it is a newer final version.
fn newer(current: &Version, release: &Release) -> Option<Version> {
    let version = Version::parse(release.tag_name.strip_prefix('v')?).ok()?;
    (version.pre.is_empty() && version > *current).then_some(version)
}

/// The installer's name in the release, which its attestation names too.
fn installer_name(version: &Version) -> String {
    format!("arcwise-{version}-x64-setup.exe")
}

/// The name [`fetch`] saves `version`'s installer under, for process `pid`, so a second Arcwise
/// downloading at the same time has its own file.
fn download_name(version: &Version, pid: u32) -> String {
    format!("arcwise-{version}-{pid}-setup.exe")
}

/// The version in a name from [`download_name`].
fn downloaded_version(name: &str) -> Option<Version> {
    let rest = name.strip_prefix("arcwise-")?.strip_suffix("-setup.exe")?;
    Version::parse(rest.rsplit_once('-')?.0).ok()
}

/// An HTTPS client that names Arcwise and its version as the user agent and trusts the
/// certificates Windows trusts.
fn agent(current: &Version) -> Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(aws_lc_rs::default_provider()))
        .build();
    Agent::config_builder()
        .tls_config(tls)
        .user_agent(format!("Arcwise/{current}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .new_agent()
}

fn download(agent: &Agent, url: &str, path: &Path) -> anyhow::Result<()> {
    let mut body = agent.get(url).call()?.into_body().into_reader();
    let mut file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    io::copy(&mut body, &mut file).with_context(|| format!("downloading {url}"))?;
    Ok(())
}

/// Deletes the files in `dir` except installers newer than `current` from the last
/// [`KEEP_DOWNLOADS`], which another Arcwise running at the same time may be downloading or
/// about to install. Ones downloaded earlier are left by installs that never finished. Returns
/// the files that should have gone but couldn't be deleted.
fn clear(dir: &Path, current: &Version) -> Vec<Kept> {
    let mut kept = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return kept;
    };
    for entry in entries.flatten() {
        let newer = entry
            .file_name()
            .to_str()
            .and_then(downloaded_version)
            .is_some_and(|version| version > *current);
        let recent = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| modified.elapsed().is_ok_and(|age| age < KEEP_DOWNLOADS));
        if !(newer && recent) {
            let path = entry.path();
            if let Err(error) = fs::remove_file(&path) {
                kept.push(Kept { path, error });
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
        }
    }

    #[test]
    fn finds_a_newer_release() {
        assert_eq!(
            newer(&Version::new(0, 2, 0), &release("v0.3.0")),
            Some(Version::new(0, 3, 0)),
            "a newer final release"
        );
        assert_eq!(
            installer_name(&Version::new(0, 3, 0)),
            "arcwise-0.3.0-x64-setup.exe",
            "the installer's name in the release"
        );
    }

    #[test]
    fn ignores_the_same_or_an_older_release() {
        assert_eq!(
            newer(&Version::new(0, 3, 0), &release("v0.3.0")),
            None,
            "the same version"
        );
        assert_eq!(
            newer(&Version::new(0, 4, 0), &release("v0.3.0")),
            None,
            "an older version"
        );
    }

    #[test]
    fn ignores_pre_releases_and_odd_tags() {
        let current = Version::new(0, 2, 0);
        assert_eq!(
            newer(&current, &release("v0.3.0-beta.1")),
            None,
            "a pre-release"
        );
        assert_eq!(
            newer(&current, &release("0.3.0")),
            None,
            "a tag without its v"
        );
        assert_eq!(
            newer(&current, &release("nightly")),
            None,
            "a tag that isn't a version"
        );
    }

    #[test]
    fn clears_all_but_recent_installers_newer_than_the_running_version() {
        let dir = env::temp_dir().join(format!("arcwise-update-test-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let version = |v: &str| Version::parse(v).unwrap();
        let older = download_name(&version("0.1.0"), 1);
        let running = download_name(&version("0.2.0"), 2);
        let newer = download_name(&version("0.3.0"), 3);
        let prerelease = download_name(&version("0.4.0-rc.1"), 4);
        let abandoned = download_name(&version("0.3.0"), 5);
        for name in [
            &older,
            &running,
            &newer,
            &prerelease,
            &abandoned,
            "notes.txt",
        ] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        let two_days_ago = SystemTime::now() - 2 * KEEP_DOWNLOADS;
        File::options()
            .write(true)
            .open(dir.join(&abandoned))
            .unwrap()
            .set_modified(two_days_ago)
            .unwrap();
        let kept = clear(&dir, &version("0.2.0"));
        let mut left: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        fs::remove_dir_all(&dir).unwrap();
        assert!(kept.is_empty(), "nothing failed to delete: {kept:?}");
        assert_eq!(
            left,
            [newer, prerelease],
            "only recent installers newer than 0.2.0 stay"
        );
    }

    #[test]
    fn reads_the_version_back_from_a_download_name() {
        assert_eq!(
            downloaded_version(&download_name(&Version::new(1, 2, 3), 42)),
            Some(Version::new(1, 2, 3)),
            "the pid is not part of the version"
        );
        assert_eq!(downloaded_version("notes.txt"), None, "not a download");
    }
}
