//! Keeps Arcwise up to date. When Arcwise opens, it asks GitHub for the latest release, downloads
//! a newer installer, and checks the installer's attestation: that this repository's release
//! workflow built it on GitHub's own runners. The installer then runs silently when Arcwise
//! closes, or straight away when asked to restart.

use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, bail, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use sigstore_verify::trust_root::{SigstoreInstance, TrustedRoot};
use sigstore_verify::types::{Bundle, Sha256Hash, SignatureContent, Statement};
use sigstore_verify::{VerificationPolicy, Verifier};

/// The page people can download releases from themselves.
pub const RELEASES_PAGE: &str = "https://github.com/265866/arcwise/releases/latest";
const LATEST_RELEASE: &str = "https://api.github.com/repos/265866/arcwise/releases/latest";
/// The release workflow attaches it to every release.
const ATTESTATION: &str = "attestation.json";

/// Who must have built an installer for Arcwise to run it.
pub struct Builder {
    pub repository: &'static str,
    /// The certificate's identity: the workflow file and the ref it ran from.
    pub workflow: &'static str,
}

/// The release workflow, which runs on pushes to main (see .github/workflows/release.yml).
pub const RELEASE_BUILDER: Builder = Builder {
    repository: "https://github.com/265866/arcwise",
    workflow: "https://github.com/265866/arcwise/.github/workflows/release.yml@refs/heads/main",
};

const GITHUB_ACTIONS: &str = "https://token.actions.githubusercontent.com";

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Whether Arcwise looks for updates when it opens, and installs one it found when it closes.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is semver")
}

/// A newer installer, downloaded and verified.
#[derive(Clone, Debug)]
pub struct Update {
    pub version: Version,
    pub installer: PathBuf,
}

#[derive(Debug)]
pub enum Outcome {
    UpToDate,
    Ready(Update),
    /// A newer release exists, but its installer could not be shown to come from the release
    /// workflow, so it was deleted.
    Unverified {
        version: Version,
        reason: String,
    },
}

/// Where downloaded installers wait to run: `%LOCALAPPDATA%\Arcwise\updates`.
pub fn directory() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| Path::new(&dir).join("Arcwise").join("updates"))
}

/// Looks for a release newer than `current` and readies its installer in `dir`. Blocks on the
/// network, so it runs off the UI thread. An error means the check could not finish, such as
/// when offline, and is not worth showing.
pub fn check(current: &Version, dir: &Path) -> anyhow::Result<Outcome> {
    let agent = agent(current);
    let latest: Release = match agent.get(LATEST_RELEASE).call() {
        Ok(mut response) => serde_json::from_slice(&response.body_mut().read_to_vec()?)
            .context("reading the latest release")?,
        // No release has been published yet.
        Err(ureq::Error::StatusCode(404)) => return Ok(Outcome::UpToDate),
        Err(e) => return Err(e).context("asking GitHub for the latest release"),
    };
    let Some(found) = newer(current, &latest) else {
        clear(dir, None);
        return Ok(Outcome::UpToDate);
    };
    clear(dir, Some(&found.installer_name));
    let installer = dir.join(&found.installer_name);
    let digest = if installer.exists() {
        hash(&mut File::open(&installer)?)?
    } else {
        download(&agent, &found.installer_url, &installer)?
    };
    let attestation = agent
        .get(&found.attestation_url)
        .call()?
        .body_mut()
        .read_to_string()
        .context("downloading the attestation")?;
    match verify(
        &attestation,
        digest,
        &found.installer_name,
        &RELEASE_BUILDER,
    ) {
        Ok(()) => Ok(Outcome::Ready(Update {
            version: found.version,
            installer,
        })),
        Err(e) => {
            let _ = std::fs::remove_file(&installer);
            Ok(Outcome::Unverified {
                version: found.version,
                reason: format!("{e:#}"),
            })
        }
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

/// The parts of GitHub's release JSON the check reads.
#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, PartialEq)]
struct Found {
    version: Version,
    installer_name: String,
    installer_url: String,
    attestation_url: String,
}

/// The release's installer and attestation, when it is a newer, final version that has both.
/// A release still getting its files attached lacks them, and is picked up next time.
fn newer(current: &Version, release: &Release) -> Option<Found> {
    let version = Version::parse(release.tag_name.strip_prefix('v')?).ok()?;
    if !version.pre.is_empty() || version <= *current {
        return None;
    }
    let installer_name = format!("arcwise-{version}-x64-setup.exe");
    let url = |name: &str| {
        release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .map(|asset| asset.browser_download_url.clone())
    };
    Some(Found {
        installer_url: url(&installer_name)?,
        attestation_url: url(ATTESTATION)?,
        installer_name,
        version,
    })
}

/// Checks that `attestation` shows `builder` built the file named `file_name` whose SHA-256 is
/// `digest`.
fn verify(
    attestation: &str,
    digest: [u8; 32],
    file_name: &str,
    builder: &Builder,
) -> anyhow::Result<()> {
    let bundle = Bundle::from_json(attestation).context("reading the attestation")?;
    let root = TrustedRoot::from_embedded(SigstoreInstance::PublicGood)?;
    let digest = Sha256Hash::new(digest);
    let policy = VerificationPolicy::new(builder.workflow, GITHUB_ACTIONS);
    let result = Verifier::new(&root)?
        .verify(digest, &bundle, &policy)
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
        claims.source_repository_uri.as_deref() == Some(builder.repository),
        "the attestation is for {:?}, not {}",
        claims.source_repository_uri,
        builder.repository
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
            .any(|s| s.name == file_name && s.digest.sha256.as_ref() == Some(&digest)),
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

/// Downloads `url` to `path`, through a `.part` file so an interrupted download is never taken
/// for a whole one, and returns its SHA-256.
fn download(agent: &ureq::Agent, url: &str, path: &Path) -> anyhow::Result<[u8; 32]> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let part = path.with_extension("part");
    let mut body = agent.get(url).call()?.into_body().into_reader();
    let mut file = BufWriter::new(File::create(&part)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = body.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])?;
    }
    file.into_inner()
        .map_err(io::IntoInnerError::into_error)?
        .sync_all()?;
    std::fs::rename(&part, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(hasher.finalize().into())
}

fn hash(reader: &mut impl Read) -> io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    io::copy(reader, &mut hasher)?;
    Ok(hasher.finalize().into())
}

/// Deletes everything in `dir` but the file named `keep`, such as the installer of an update
/// already installed.
fn clear(dir: &Path, keep: Option<&str>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if keep.is_none_or(|keep| entry.file_name() != keep) {
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
    const GH_MSI_SHA256: &str = "7944b579d76359e5b859a263bf63583e271ae3fbd427ae46e133c1f348b46371";
    const GH_BUILDER: Builder = Builder {
        repository: "https://github.com/cli/cli",
        workflow: "https://github.com/cli/cli/.github/workflows/deployment.yml@refs/heads/trunk",
    };

    fn sha256(hex: &str) -> [u8; 32] {
        let mut bytes = [0; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        bytes
    }

    /// Checks verification failed, and for the reason expected.
    fn refused(result: anyhow::Result<()>, reason: &str) {
        let got = format!("{:#}", result.expect_err("verification should fail"));
        assert!(got.contains(reason), "{got}");
    }

    #[test]
    fn accepts_the_installer_its_builder_attested() {
        verify(GH_ATTESTATION, sha256(GH_MSI_SHA256), GH_MSI, &GH_BUILDER).unwrap();
    }

    #[test]
    fn refuses_other_content() {
        let mut digest = sha256(GH_MSI_SHA256);
        digest[0] ^= 1;
        refused(
            verify(GH_ATTESTATION, digest, GH_MSI, &GH_BUILDER),
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
            verify(GH_ATTESTATION, sha256(GH_MSI_SHA256), GH_MSI, &builder),
            "identity mismatch",
        );
    }

    #[test]
    fn refuses_another_repository() {
        refused(
            verify(
                GH_ATTESTATION,
                sha256(GH_MSI_SHA256),
                GH_MSI,
                &RELEASE_BUILDER,
            ),
            "identity mismatch",
        );
    }

    #[test]
    fn refuses_the_right_content_under_another_name() {
        refused(
            verify(
                GH_ATTESTATION,
                sha256(GH_MSI_SHA256),
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
            verify(&tampered, sha256(GH_MSI_SHA256), GH_MSI, &GH_BUILDER),
            "signature invalid",
        );
    }

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            assets: assets
                .iter()
                .map(|name| Asset {
                    name: (*name).into(),
                    browser_download_url: format!("https://example.com/{name}"),
                })
                .collect(),
        }
    }

    const FILES: &[&str] = &[
        "arcwise-0.3.0-x64-setup.exe",
        "arcwise-0.3.0-x64.zip",
        ATTESTATION,
    ];

    #[test]
    fn finds_a_newer_release() {
        let found = newer(&Version::new(0, 2, 0), &release("v0.3.0", FILES)).unwrap();
        assert_eq!(found.version, Version::new(0, 3, 0));
        assert_eq!(found.installer_name, "arcwise-0.3.0-x64-setup.exe");
        assert_eq!(
            found.installer_url,
            "https://example.com/arcwise-0.3.0-x64-setup.exe"
        );
        assert_eq!(
            found.attestation_url,
            "https://example.com/attestation.json"
        );
    }

    #[test]
    fn ignores_the_same_or_an_older_release() {
        assert_eq!(
            newer(&Version::new(0, 3, 0), &release("v0.3.0", FILES)),
            None
        );
        assert_eq!(
            newer(&Version::new(0, 4, 0), &release("v0.3.0", FILES)),
            None
        );
    }

    #[test]
    fn ignores_pre_releases_and_odd_tags() {
        let files = &["arcwise-0.3.0-beta.1-x64-setup.exe", ATTESTATION];
        assert_eq!(
            newer(&Version::new(0, 2, 0), &release("v0.3.0-beta.1", files)),
            None
        );
        assert_eq!(
            newer(&Version::new(0, 2, 0), &release("0.3.0", FILES)),
            None
        );
        assert_eq!(
            newer(&Version::new(0, 2, 0), &release("nightly", FILES)),
            None
        );
    }

    #[test]
    fn waits_for_a_release_to_have_its_files() {
        let current = Version::new(0, 2, 0);
        assert_eq!(newer(&current, &release("v0.3.0", &[ATTESTATION])), None);
        assert_eq!(
            newer(
                &current,
                &release("v0.3.0", &["arcwise-0.3.0-x64-setup.exe"])
            ),
            None
        );
    }

    #[test]
    fn clears_all_but_the_installer_to_keep() {
        let dir = std::env::temp_dir().join(format!("arcwise-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "arcwise-0.2.0-x64-setup.exe",
            "arcwise-0.3.0-x64-setup.exe",
            "x.part",
        ] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        clear(&dir, Some("arcwise-0.3.0-x64-setup.exe"));
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(left, ["arcwise-0.3.0-x64-setup.exe"]);
    }

    #[test]
    fn hashes_what_it_reads() {
        assert_eq!(
            hash(&mut &b"abc"[..]).unwrap(),
            sha256("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }
}
