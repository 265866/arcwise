# Install arcwise

Read this page to run arcwise on your machine. It shows how to install a release or
build from source, and how to update.

Each [release](https://github.com/265866/arcwise/releases) has one archive per
platform:

- Windows on x86-64: `arcwise-<version>-x86_64-pc-windows-msvc.zip`

## Install a release

1. Download the archive for your platform and `SHA256SUMS` from the release.
2. Check the download as [Verify a release](verifying-releases.md) shows.
3. Unpack the archive and move `arcwise` to where you keep programs, such as a directory
   on your `PATH`.

To update, install the new release the same way.

## Build from source

You need [Rust](https://rustup.rs/).

```sh
cargo install --locked --git https://github.com/265866/arcwise arcwise
```
