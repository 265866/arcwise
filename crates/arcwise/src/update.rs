//! Finds, downloads and checks updates. [`latest`] asks GitHub for the latest release. [`fetch`]
//! downloads a release's installer and checks its attestation: that this repository's release
//! workflow built it on GitHub's own runners. [`crate::updater`] decides when each runs.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, bail, ensure};
use semver::Version;
use serde::Deserialize;
use sigstore_verify::crypto::hash::sha256_reader;
use sigstore_verify::trust_root::{SigstoreInstance, TrustedRoot};
use sigstore_verify::types::{Bundle, Sha256Hash, SignatureContent, Statement};
use sigstore_verify::{VerificationPolicy, Verifier};

/// The page people can download releases from themselves.
pub const RELEASES_PAGE: &str = "https://github.com/265866/arcwise/releases/latest";
const LATEST_RELEASE: &str = "https://api.github.com/repos/265866/arcwise/releases/latest";
/// Where GitHub serves a release's files, by tag and file name.
const DOWNLOADS: &str = "https://github.com/265866/arcwise/releases/download";
/// The release workflow attaches it to every release.
const ATTESTATION: &str = "attestation.json";
/// How long a downloaded installer for a newer version is kept for another Arcwise to install.
const KEEP_DOWNLOADS: Duration = Duration::from_secs(24 * 60 * 60);

/// Who must have built an installer for Arcwise to run it.
pub struct Builder {
    pub repository: &'static str,
    /// GitHub's number for the repository, which a new repository under the same name would not
    /// have.
    pub repository_id: &'static str,
    /// The certificate's identity: the workflow file and the ref it ran from.
    pub workflow: &'static str,
}

/// The release workflow, which runs on pushes to main (see .github/workflows/release.yml).
pub const RELEASE_BUILDER: Builder = Builder {
    repository: "https://github.com/265866/arcwise",
    repository_id: "1426430645",
    workflow: "https://github.com/265866/arcwise/.github/workflows/release.yml@refs/heads/main",
};

const GITHUB_ACTIONS: &str = "https://token.actions.githubusercontent.com";

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is semver")
}

/// Whether this exe is the one the installer put in place, beside its uninstaller. An update
/// installs there, so updating any other copy, such as one run from the zip or a development
/// build, would install a second Arcwise and leave this one behind.
pub fn installed() -> bool {
    !cfg!(debug_assertions)
        && std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("unins000.exe")))
            .is_some_and(|uninstaller| uninstaller.is_file())
}

/// A newer installer, downloaded and verified.
#[derive(Clone, Debug)]
pub struct Update {
    pub version: Version,
    pub installer: PathBuf,
}

#[derive(Debug)]
pub enum Outcome {
    Ready(Update),
    /// The installer could not be shown to come from the release workflow, so it was deleted.
    Unverified {
        reason: String,
    },
}

/// Where downloaded installers wait to run: `%LOCALAPPDATA%\Arcwise\updates`.
pub fn directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| Path::new(&dir).join("Arcwise").join("updates"))
}

/// Asks GitHub for the latest release and returns its version when it is newer than `current`.
/// Installers left in `dir` by earlier runs, such as one already installed, are deleted. Blocks
/// on the network, so it runs off the UI thread.
pub fn latest(current: &Version, dir: &Path) -> anyhow::Result<Option<Version>> {
    let release: Release = match agent(current).get(LATEST_RELEASE).call() {
        Ok(mut response) => serde_json::from_slice(&response.body_mut().read_to_vec()?)
            .context("reading the latest release")?,
        // No release has been published yet.
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(e).context("asking GitHub for the latest release"),
    };
    clear(dir, current);
    Ok(newer(current, &release))
}

/// Downloads `version`'s installer into `dir` and checks its attestation. Blocks on the network,
/// so it runs off the UI thread. An error means the download could not finish, such as when
/// offline or while the release is still getting its files.
pub fn fetch(version: &Version, dir: &Path) -> anyhow::Result<Outcome> {
    let agent = agent(&current_version());
    let name = installer_name(version);
    let url = |file: &str| format!("{DOWNLOADS}/v{version}/{file}");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let installer = dir.join(download_name(version, std::process::id()));
    let checked = (|| {
        download(&agent, &url(&name), &installer)?;
        let attestation = agent
            .get(&url(ATTESTATION))
            .call()?
            .body_mut()
            .read_to_string()
            .context("downloading the attestation")?;
        let digest = sha256_reader(File::open(&installer)?)?;
        anyhow::Ok(verify(&attestation, &digest, &name, &RELEASE_BUILDER))
    })();
    // Anything but a verified installer is deleted.
    if !matches!(checked, Ok(Ok(()))) {
        let _ = std::fs::remove_file(&installer);
    }
    match checked? {
        Ok(()) => Ok(Outcome::Ready(Update {
            version: version.clone(),
            installer,
        })),
        Err(e) => Ok(Outcome::Unverified {
            reason: format!("{e:#}"),
        }),
    }
}

/// Runs the installer silently. It closes Arcwise if it is still running, and with `relaunch`
/// opens the new version when it is done.
pub fn install(update: &Update, relaunch: bool) -> io::Result<()> {
    let mut command = Command::new(&update.installer);
    command.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"]);
    if relaunch {
        command.arg("/RELAUNCH");
    }
    command.spawn().map(drop)
}

/// Deletes the installer of an update that will not be installed.
pub fn discard(update: Update) {
    let _ = std::fs::remove_file(update.installer);
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

/// Checks that `attestation` shows `builder` built the file named `file_name` whose SHA-256 is
/// `digest`.
fn verify(
    attestation: &str,
    digest: &Sha256Hash,
    file_name: &str,
    builder: &Builder,
) -> anyhow::Result<()> {
    let bundle = Bundle::from_json(attestation).context("reading the attestation")?;
    let root = TrustedRoot::from_embedded(SigstoreInstance::PublicGood)?;
    let policy = VerificationPolicy::new(builder.workflow, GITHUB_ACTIONS);
    let result = Verifier::new(&root)?
        .verify(*digest, &bundle, &policy)
        .context("checking the attestation's signature")?;
    ensure!(
        result.tlog_verified(),
        "the attestation is not in Sigstore's transparency log"
    );
    let claims = &result
        .certificate()
        .context("the attestation has no certificate")?
        .ci_claims;
    ensure!(
        claims.source_repository_uri.as_deref() == Some(builder.repository)
            && claims.source_repository_identifier.as_deref() == Some(builder.repository_id),
        "the attestation is for {:?} ({:?}), not {} ({})",
        claims.source_repository_uri,
        claims.source_repository_identifier,
        builder.repository,
        builder.repository_id
    );
    ensure!(
        claims.runner_environment.as_deref() == Some("github-hosted"),
        "the installer was not built on a GitHub-hosted runner"
    );
    // The signature covers every file the release built. The download must be the installer of
    // the version asked for, not another file or an older release's installer.
    let SignatureContent::DsseEnvelope(envelope) = &bundle.content else {
        bail!("the attestation holds no statement about files");
    };
    let statement: Statement = serde_json::from_slice(envelope.payload.as_bytes())
        .context("reading the attestation's statement")?;
    ensure!(
        statement
            .subject
            .iter()
            .any(|s| s.name == file_name && s.digest.sha256.as_ref() == Some(digest)),
        "the attestation does not name {file_name} with this content"
    );
    Ok(())
}

fn agent(current: &Version) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(format!("Arcwise/{current}"))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .new_agent()
}

fn download(agent: &ureq::Agent, url: &str, path: &Path) -> anyhow::Result<()> {
    let mut body = agent.get(url).call()?.into_body().into_reader();
    let mut file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    io::copy(&mut body, &mut file).with_context(|| format!("downloading {url}"))?;
    Ok(())
}

/// Deletes the files in `dir` except installers newer than `current` from the last
/// [`KEEP_DOWNLOADS`], which another Arcwise running at the same time may be downloading or
/// about to install. Ones downloaded earlier are left by installs that never finished.
fn clear(dir: &Path, current: &Version) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
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
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The build provenance GitHub CLI 2.102.0's release workflow made for its Windows
    /// installer: a real attestation from the same action and Sigstore instance as Arcwise's.
    const GH_ATTESTATION: &str = include_str!("../tests/fixtures/gh-2.102.0-msi.sigstore.json");
    const GH_MSI: &str = "gh_2.102.0_windows_amd64.msi";
    const GH_BUILDER: Builder = Builder {
        repository: "https://github.com/cli/cli",
        repository_id: "212613049",
        workflow: "https://github.com/cli/cli/.github/workflows/deployment.yml@refs/heads/trunk",
    };

    fn gh_msi_sha256() -> Sha256Hash {
        Sha256Hash::from_hex("7944b579d76359e5b859a263bf63583e271ae3fbd427ae46e133c1f348b46371")
            .unwrap()
    }

    /// Checks verification failed, and for the reason expected.
    fn refused(result: anyhow::Result<()>, reason: &str) {
        let got = format!("{:#}", result.expect_err("verification should fail"));
        assert!(got.contains(reason), "{got}");
    }

    #[test]
    fn accepts_the_installer_its_builder_attested() {
        verify(GH_ATTESTATION, &gh_msi_sha256(), GH_MSI, &GH_BUILDER).unwrap();
    }

    #[test]
    fn refuses_other_content() {
        let other = Sha256Hash::from_hex(
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .unwrap();
        refused(
            verify(GH_ATTESTATION, &other, GH_MSI, &GH_BUILDER),
            "does not match any subject",
        );
    }

    #[test]
    fn refuses_another_workflow() {
        let builder = Builder {
            workflow: "https://github.com/cli/cli/.github/workflows/other.yml@refs/heads/trunk",
            ..GH_BUILDER
        };
        refused(
            verify(GH_ATTESTATION, &gh_msi_sha256(), GH_MSI, &builder),
            "identity mismatch",
        );
    }

    #[test]
    fn refuses_arcwise_for_another_repository() {
        refused(
            verify(GH_ATTESTATION, &gh_msi_sha256(), GH_MSI, &RELEASE_BUILDER),
            "identity mismatch",
        );
    }

    #[test]
    fn refuses_a_repository_with_the_same_name_but_another_number() {
        let builder = Builder {
            repository_id: "1",
            ..GH_BUILDER
        };
        refused(
            verify(GH_ATTESTATION, &gh_msi_sha256(), GH_MSI, &builder),
            "the attestation is for",
        );
    }

    #[test]
    fn refuses_the_right_content_under_another_name() {
        refused(
            verify(
                GH_ATTESTATION,
                &gh_msi_sha256(),
                "gh_2.101.0_windows_amd64.msi",
                &GH_BUILDER,
            ),
            "does not name",
        );
    }

    #[test]
    fn refuses_a_tampered_attestation() {
        let tampered = GH_ATTESTATION.replacen("\"sig\":\"M", "\"sig\":\"N", 1);
        assert_ne!(tampered, GH_ATTESTATION);
        refused(
            verify(&tampered, &gh_msi_sha256(), GH_MSI, &GH_BUILDER),
            "signature invalid",
        );
    }

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
        }
    }

    #[test]
    fn finds_a_newer_release() {
        assert_eq!(
            newer(&Version::new(0, 2, 0), &release("v0.3.0")),
            Some(Version::new(0, 3, 0))
        );
        assert_eq!(
            installer_name(&Version::new(0, 3, 0)),
            "arcwise-0.3.0-x64-setup.exe"
        );
    }

    #[test]
    fn ignores_the_same_or_an_older_release() {
        assert_eq!(newer(&Version::new(0, 3, 0), &release("v0.3.0")), None);
        assert_eq!(newer(&Version::new(0, 4, 0), &release("v0.3.0")), None);
    }

    #[test]
    fn ignores_pre_releases_and_odd_tags() {
        let current = Version::new(0, 2, 0);
        assert_eq!(newer(&current, &release("v0.3.0-beta.1")), None);
        assert_eq!(newer(&current, &release("0.3.0")), None);
        assert_eq!(newer(&current, &release("nightly")), None);
    }

    #[test]
    fn clears_all_but_recent_installers_newer_than_the_running_version() {
        let dir = std::env::temp_dir().join(format!("arcwise-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
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
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let two_days_ago = std::time::SystemTime::now() - 2 * KEEP_DOWNLOADS;
        File::options()
            .write(true)
            .open(dir.join(&abandoned))
            .unwrap()
            .set_modified(two_days_ago)
            .unwrap();
        clear(&dir, &version("0.2.0"));
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left, [newer, prerelease]);
    }
}
