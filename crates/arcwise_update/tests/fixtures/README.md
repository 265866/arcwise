# Test fixtures

`gh-2.102.0-msi.sigstore.json` is public data, kept as recorded. It is the build provenance that
GitHub CLI's own release workflow, in the public `cli/cli` repository, published for its 2.102.0
release. Seven tests in [`src/builder.rs`](../../src/builder.rs) check Arcwise's attestation check
against it.

It stays as recorded because it is the published record: the tests verify its real Sigstore
signature, so a changed value in its statement, signature, certificate or log entry fails them. It
names no person, personal account, email or postal address. It holds:

- the `cli/cli` repository, its visibility, and its owner, the `cli` organization, by URL and
  numeric ID;
- the release workflow, its name, branch and commit, the run, and that it was started by hand
  (`workflow_dispatch`, without who started it), in the `production` environment, on a
  GitHub-hosted runner;
- the names and SHA-256 digests of the 21 release files it covers;
- the certificate Sigstore issued the workflow, the workflow's signature, and Sigstore's
  transparency-log entry, with when it was signed: 2026-09-30, 02:38 UTC.
