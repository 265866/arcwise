# Test fixtures

`gh-2.102.0-msi.sigstore.json` is public data, kept as recorded. It is the build provenance that GitHub CLI's own release workflow, in the public `cli/cli` repository, published for the GitHub CLI 2.102.0 release. Seven tests in `src/builder.rs` check Arcwise's attestation check against it.

It stays byte for byte as recorded because those tests verify its real Sigstore signature, and a changed byte fails them. It names no person, account or address. It holds:

- the `cli/cli` repository and its owner, by URL and numeric ID;
- the release workflow, its commit and run, and the GitHub-hosted runner it ran on;
- the 2.102.0 release's file names and their SHA-256 digests;
- Sigstore's certificate, signature and transparency-log entry.
