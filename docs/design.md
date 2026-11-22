# Design

How arcwise works and why. Update this page in the same change as the code it
describes.

## Goals

Arcwise shows Valorant lineups: where each piece of utility lands on a map, where to stand, where
to aim and how to throw. It is a Windows desktop app in a window of its own, for a second monitor
beside the game, never an overlay on it. It must:

- play each lineup's clip at full frame rate while the game has focus, without taking much CPU
  from it;
- read lineup libraries from folders and from the web, and keep chosen maps on disk to play
  offline;
- reopen where it was left, and keep its settings across updates;
- update itself only to an installer this repository's release workflow built.

This repository replaced an earlier one of the same name (GitHub's number for it is 1426430645),
from version 0.11.0. Its settings file, data folders, installer
and release file names stay those of 0.10.0, so an existing install upgrades in place.

## Architecture

| Crate | Owns |
| --- | --- |
| `arcwise` | The program: `--version`, and otherwise the command line handed to `arcwise_ui::run`. |
| `arcwise_ui` | Everything on GPUI: startup, the window, the choosers, the map board, the lineup panel and player, the settings window and file, themes, pictures, keyboard shortcuts, offline-download progress and the update schedule. |
| `arcwise_core` | The `library.json` model, checking and merging libraries, landings, the browse state, and the command-line parser. No UI and no network. |
| `arcwise_remote` | Libraries on the web, mirrored under `%LOCALAPPDATA%\Arcwise\libraries`, offline copies map by map, and reading the list of libraries shown into parts for `arcwise_core` to merge. |
| `arcwise_update` | Asking GitHub for the latest release, downloading its installer, and checking its attestation. |
| `arcwise_video` | Hardware video decoding through Windows Media Foundation, on a thread of its own. |
| `arcwise_ffi` | Safe wrappers over the Win32 window, theme, keyboard and mutex calls and libwebp's decoder. |

`arcwise_video` and `arcwise_ffi` hold all the unsafe code; every other crate denies it. Data moves
one way: `arcwise_remote` and folders yield library parts, `arcwise_core` merges them into one
`Library` and keeps the browse state over it, and `arcwise_ui` draws that state and turns input
into calls on it. Clips go from `arcwise_remote`'s mirror (or its web address, while the clip is
fetched) to `arcwise_video`, whose frames the player paces against the clip's clock.

## Decisions

- **GPUI from Zed's repository.** The window, its text and its GPU drawing come from GPUI, pinned
  to one commit of Zed's repository, since the crates.io release lags that tree. `deny.toml`
  allows that source and the crates gpui itself depends on.
- **No code before `main()`.** gpui's `actions!` writes an `inventory::submit!` into the calling
  crate. Arcwise declares its actions with `#[derive(Action)]` and `#[action(no_register)]`
  instead, and binds keys to the typed actions, which need no registration.
- **Hardware video over a software decoder.** Media Foundation decodes on the GPU and converts to
  BGRA there, so a clip cost about an eighth of one CPU core while the game ran, as measured for
  0.10.0, which decodes the same way. Frames are read
  back into memory, since GPUI can't sample a texture from another Direct3D device.
- **Pictures through libwebp.** The library's stills decode about three times faster with libwebp
  than with the `image` crate's decoder. libwebp fills its decoding tables on first use without a
  lock, so Arcwise decodes one tiny picture of each kind on one thread before any other.
- **A mirror per web library.** A library on the web is a folder on disk to the rest of Arcwise,
  filled lazily: a picture when first shown, a clip while it streams. A changed file is dropped by
  its SHA-256 in the manifest. The same code serves offline copies.
- **Updates verified against this repository.** The updater checks the installer's Sigstore
  attestation, that the release workflow on `main` of this repository, by GitHub's number for it,
  built that file on a GitHub-hosted runner. A new repository under the same name has another
  number, so it can't ship Arcwise an update.
- **Errors nothing shows go to stderr.** Arcwise keeps no log file. A failure the window doesn't
  show is reported on stderr with the user's folders written as `%LOCALAPPDATA%`, `%APPDATA%` and
  `%USERPROFILE%`.
