# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.2](https://github.com/265866/arcwise/compare/v0.11.1...v0.11.2) - 2026-11-23

### Fixed

- *(ui)* keep a settings file Arcwise couldn't read, and read one with a byte order mark ([#27](https://github.com/265866/arcwise/pull/27))

## [0.11.1](https://github.com/265866/arcwise/compare/v0.11.0...v0.11.1) - 2026-11-23

### Other

- the installer is now built by trellis's pinned Inno Setup 7.1.1

## [0.11.0](https://github.com/265866/arcwise/releases/tag/v0.11.0) - 2026-11-22

Arcwise is rebuilt, and this is its first release from this repository. The app works as 0.10.0 did. It keeps your settings, your downloaded lineups and your library folders where they are.

### Upgrading from 0.10.0 and earlier

Install this version by hand once: download `arcwise-0.11.0-x64-setup.exe` below and run it. It installs over the old version.

Arcwise 0.10.0 and earlier trust only the repository they were released from, which this one replaces, so they can't update themselves to this release. When one finds it, it says "Its download could not be verified. Get it from the releases page instead." That is expected, and no sign of a tampered download. From 0.11.0 on, the app updates itself as before.

### Changes you might notice

- `arcwise --version` prints the version when its output goes to a file or a pipe. Typed in a console window, it prints nothing, because Arcwise is a windowed app.
- Arcwise is licensed under MIT OR Apache-2.0. `LICENSE-MIT.txt` and `LICENSE-APACHE.txt` replace `LICENSE.txt` in the install folder.

### Added

- *(ui)* port the window, the browser and the app's startup ([#15](https://github.com/265866/arcwise/pull/15))
- *(app)* add the app icon ([#12](https://github.com/265866/arcwise/pull/12))
- *(video)* port the Media Foundation decoder crate ([#11](https://github.com/265866/arcwise/pull/11))
- *(ffi)* safe wrappers over the Win32 and libwebp calls ([#10](https://github.com/265866/arcwise/pull/10))
- *(remote)* port libraries on the web, offline downloads and library sources ([#8](https://github.com/265866/arcwise/pull/8))
- *(update)* port the update check, download and attestation check ([#6](https://github.com/265866/arcwise/pull/6))
- *(core)* port the command-line parser ([#7](https://github.com/265866/arcwise/pull/7))
- *(core)* port the lineup library and browsing logic ([#3](https://github.com/265866/arcwise/pull/3))

### Other

- *(app)* embed the icon and version information in arcwise.exe ([#13](https://github.com/265866/arcwise/pull/13))
- generate from trellis
