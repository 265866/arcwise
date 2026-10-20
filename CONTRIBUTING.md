# Contributing to Arcwise

Arcwise is written in Rust with [GPUI](https://www.gpui.rs/), the UI framework behind the Zed
editor, and runs only on Windows.

## Build and run

You need Windows 10 or 11, [rustup](https://rustup.rs/), and the
[Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the
C++ workload. `rust-toolchain.toml` pins the Rust version. Install it once with
`rustup toolchain install`.

The commands below are for PowerShell.

```powershell
cargo run --release
```

The first build compiles GPUI and takes a few minutes. Command-line flags open a library or a
lineup directly, which helps while working on the UI:

```powershell
cargo run --release -- --library D:\lineups --map Ascent --agent Sova --side attack
cargo run --release -- --lineup 211 --view aim
```

`.cargo/config.toml` links the C runtime statically, so every build, debug ones too, runs without
the Visual C++ redistributable.

The workspace has three crates:

| Crate | Holds |
| --- | --- |
| `arcwise-core` | The library format, landing clusters and browsing state. No UI; most tests live here. |
| `arcwise-video` | The Media Foundation decode thread. |
| `arcwise` | The GPUI app, including the updater in `src/update.rs`. |

## Test

`cargo test` runs everything, including decoding a small H.264 clip through Media Foundation and
checking a real GitHub attestation. Two tests need files of your own and are ignored by default:

```powershell
$env:ARCWISE_LIBRARY = 'D:\lineups'; cargo test -p arcwise-core -- --ignored
$env:ARCWISE_CLIP = 'D:\lineups\lineups\211.mp4'; cargo test -p arcwise-video -- --ignored
```

To see where time goes, name a file in `ARCWISE_TRACE`. The app writes a CSV line for every
frame (how long it took to draw), every key and click (how long until the first frame after
it, and until nothing on screen is still loading), and every picture or clip it loads:

```powershell
$env:ARCWISE_TRACE = 'trace.csv'; cargo run --release
```

A variable set with `$env:` lasts until you close the PowerShell window. To stop tracing sooner,
run `Remove-Item Env:ARCWISE_TRACE`.

Only a release build with the installer's `unins000.exe` beside it checks for updates. Builds you
run with `cargo run` never do.

## Send a change

Every change lands on `main` through a pull request. `main` accepts squash merges only, and the
PR title becomes the commit, so the title must be a [conventional commit](https://www.conventionalcommits.org/):

- `feat(board): show counts above every marker` for a new feature.
- `fix(video): recover from a lost GPU` for a bug fix.
- `perf`, `refactor`, `docs`, `build`, `ci`, `test`, `style`, `chore` or `revert` for the rest.
- A `!` before the colon, as in `feat(library)!: read library format 2`, marks a change that
  breaks something for people using Arcwise, such as a library older versions can't read.

A PR merges once these checks pass:

| Check | Runs |
| --- | --- |
| PR title | The title format above. |
| Quick checks | On Ubuntu: `cargo fmt --check`, typos, actionlint and `cargo deny check`. |
| Windows | `cargo clippy` with warnings as errors, then the tests. |

PRs that change the build, such as `Cargo.lock`, the installer script or the toolchain, also run
**Release build**, which builds the installer as a downloadable artifact. It isn't required, but
check that it passed before merging such a PR. You can also run it by hand from the **Actions**
tab.

`cargo deny check` fails on a dependency with a known vulnerability, or under a license outside
the list in `deny.toml`. Release builds ship every dependency's license notice, generated from
`about.toml`, and the build fails if a license file pinned there changes.

Dependabot opens one grouped PR a week for Cargo dependencies and one for GitHub Actions.

## Ship a release

[release-plz](https://release-plz.dev/) keeps a release PR open, titled `chore: release vX.Y.Z`.
Every merge to `main` updates it with the next version and the new `CHANGELOG.md` entries. Before
1.0, a `feat` moves 0.1.0 to 0.2.0, a `fix` moves 0.2.0 to 0.2.1, and a breaking change also
moves the minor version.

To ship:

1. Open the release PR and read the changelog. If an entry needs work, edit `CHANGELOG.md` on the
   PR's branch. Your push starts the PR's checks.
2. If you made no edit, approve the waiting workflow runs on the PR's page. A workflow opened the
   PR, so GitHub holds its checks until someone approves them, and again after each update
   release-plz pushes.
3. Merge the release PR once its checks pass. Don't merge another PR until the **Release**
   workflow's **Tag** job has finished, or release-plz may propose the same version again.

The **Release** workflow then tags `vX.Y.Z`, builds the installer and zip from the tag, attests
them, and publishes the GitHub release with the files and the release notes. If building or
publishing fails, re-run the failed jobs. They pick up from the same tag.

Release notes are GitHub's generated notes, grouped by the labels the **PR labels** workflow
applies from each PR title. `.github/release.yml` sets the groups.

## Keep the updater working

Installed copies of Arcwise update from the latest release of `265866/arcwise`. They find and
accept an update only when all of these hold:

- The release's tag is `v` followed by a version with no pre-release part, such as `v0.3.0`.
- The release has the files `arcwise-<version>-x64-setup.exe` and `attestation.json`.
- `attestation.json` shows that `.github/workflows/release.yml` built that installer on `main`,
  on a GitHub-hosted runner, in this repository.
- The installer keeps its `AppId` in `installer/arcwise.iss` and accepts `/VERYSILENT`,
  `/SUPPRESSMSGBOXES`, `/NORESTART` and `/RELAUNCH`.
- The installer puts its uninstaller, `unins000.exe`, beside `arcwise.exe`. Arcwise checks for
  updates only when it finds that file there.

Changing any of these, such as renaming the workflow, the repository or the installer, makes
every installed copy stop updating without telling anyone.

`sigstore-verify` is pinned to an exact version, and its trusted Sigstore keys are built into the
app. Update it when a new version comes out, so releases keep up with any change Sigstore makes
to its keys.
