//! Who must have built an installer for Arcwise to run it, and the check of a release's
//! attestation against it.

use anyhow::{bail, ensure};
use sigstore_verify::trust_root::{SigstoreInstance, TrustedRoot};
use sigstore_verify::types::{Bundle, Sha256Hash, SignatureContent, Statement};
use sigstore_verify::{VerificationPolicy, Verifier};

/// The issuer of the certificates GitHub Actions workflows sign with.
const GITHUB_ACTIONS: &str = "https://token.actions.githubusercontent.com";

/// Who must have built an installer for Arcwise to run it.
#[derive(Clone, Copy, Debug)]
pub struct Builder {
    /// The repository's address.
    pub repository: &'static str,
    /// GitHub's number for the repository, which a new repository under the same name would not
    /// have.
    pub repository_id: &'static str,
    /// The certificate's identity: the workflow file and the ref it ran from.
    pub workflow: &'static str,
}

/// This repository's release workflow, which runs on pushes to `main`: see
/// `.github/workflows/release.yml`.
pub const RELEASE_BUILDER: Builder = Builder {
    repository: "https://github.com/265866/arcwise",
    repository_id: "1467322936",
    workflow: "https://github.com/265866/arcwise/.github/workflows/release.yml@refs/heads/main",
};

/// Checks that `attestation` shows `builder` built the file named `file_name` whose SHA-256 is
/// `digest`, on a GitHub-hosted runner. The signature covers every file the release built, so the
/// statement must also name this file with this content: not another file, and not an older
/// release's installer.
pub(crate) fn verify(
    attestation: &str,
    digest: &Sha256Hash,
    file_name: &str,
    builder: &Builder,
) -> anyhow::Result<()> {
    use anyhow::Context as _;

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
            .expect("the digest is hex")
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
        assert_ne!(tampered, GH_ATTESTATION, "the signature changed");
        refused(
            verify(&tampered, &gh_msi_sha256(), GH_MSI, &GH_BUILDER),
            "signature invalid",
        );
    }

    #[test]
    fn pins_this_repository_and_its_release_workflow() {
        assert_eq!(
            RELEASE_BUILDER.repository_id, "1467322936",
            "GitHub's number for 265866/arcwise"
        );
        assert_eq!(
            RELEASE_BUILDER.workflow,
            "https://github.com/265866/arcwise/.github/workflows/release.yml@refs/heads/main",
            "the release workflow on main"
        );
    }
}
