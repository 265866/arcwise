# arcwise

[![CI](https://github.com/265866/arcwise/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/265866/arcwise/actions/workflows/ci.yml?query=branch%3Amain)
[![Latest release](https://img.shields.io/github/v/release/265866/arcwise)](https://github.com/265866/arcwise/releases/latest)

Arcwise is a Windows app for learning Valorant lineups: the throws that land a smoke, a dart or a
molly on one exact spot. Pick a map and an agent, and it marks every spot where that agent's
utility lands. Select one, and it shows where to stand, where to aim and how to throw, with a clip
of the throw. It is for players who learn lineups between matches or keep them open on a second
monitor while they play.

Lineups usually come as videos and web pages, one throw at a time, found by searching. Arcwise puts
them on the map instead: all of a map's lineups for an agent at once, ready to walk through with
the keyboard. The lineups come from libraries, each a web address or a folder of clips and
pictures; Arcwise starts with [library.arcwise.lol](https://library.arcwise.lol/) and has no editor
for making new ones. Arcwise never touches the game: nothing is drawn over it, nothing is read from
it, and there is no Riot sign-in, so you tell it the map. It runs on 64-bit Windows 10 and 11 only.

- **Every landing on the map.** Lineups of one ability that land in the same place share a marker,
  with a count. Point at one to see what lands there and where it is thrown from. See
  [What it does](docs/using.md#what-it-does).
- **The throw, four ways.** The clip, the stand spot on the map, the crosshair at the throw, and
  where it lands, with the steps written out and a note for a jump throw or the like. See
  [What it does](docs/using.md#what-it-does).
- **Keys for browsing.** Arrow keys walk the landings and stand spots, Q, W, E and R switch the
  view, and Space, Backspace and S pause, restart and slow the clip. Every shortcut but Esc and the
  number keys can be changed. See [Keyboard](docs/using.md#keyboard).
- **More libraries, and offline maps.** Add a library folder or another library on the web. Keep
  a web library's maps on disk, and they play without a connection. See
  [Lineups](docs/using.md#lineups) and [Libraries](docs/library-format.md).
- **Hardware video.** Clips decode on the GPU and keep their full frame rate while the game has
  focus. See [What it does](docs/using.md#what-it-does).
- **Updates itself** once installed. Before it runs a new installer, it checks the installer's
  GitHub attestation, a signed record that this repository's release workflow built that exact
  file. See [Updates](docs/using.md#updates).

[![Arcwise with a lineup open, in its dark theme. On the left, the map Saltmarsh with a marker for each landing and the selected throw drawn from stand spot 1. On the right, the lineup "Dart A Default from Lobby": its three stand spots, the clip with the dart in flight, tiles for the clip, stand spot, aim and landing views, the bow's charge bar, and the steps. The map, the agent Quill and the clip are invented for the screenshots.](docs/screenshots/lineup.png)](docs/using.md#screenshots)

Select the picture, or see [Screenshots](docs/using.md#screenshots), for every screen from choosing
a map to the settings. The maps, agents and clips in them are made up for the screenshots.

## Quick start

You need 64-bit Windows 10 or 11 with a graphics card that runs Direct3D 11, and an internet
connection for the lineups of [library.arcwise.lol](https://library.arcwise.lol/). Clips in AV1
also need the [AV1 Video Extension](https://apps.microsoft.com/detail/9mvzqvxjbq9v) from the
Microsoft Store, which Windows 11 usually has already.

1. Download `arcwise-<version>-x64-setup.exe` from the
   [latest release](https://github.com/265866/arcwise/releases/latest). To check the file first,
   follow [Verify a release](docs/verifying-releases.md).

2. Open the file. The installer has no code signature yet, so SmartScreen may stop it with
   "Windows protected your PC". Select **More info**, then **Run anyway**.

3. Follow the installer. It installs Arcwise for your Windows account only, so it never asks for
   administrator rights. It adds Arcwise to the Start menu, and to the desktop if you tick
   **Create a desktop shortcut**. Leave **Launch Arcwise** ticked and select **Finish**.

4. Arcwise says "Loading lineups…" while it fetches the library, then opens on **Choose a map**.
   Pick a map, then an agent. Without a connection it says "No lineups to show" instead: connect
   and select **Try again**, or select **Add a folder…** for a library folder you already have.

5. Select a marker on the map. The panel on the right plays the clip of the throw. Press
   <kbd>W</kbd>, <kbd>E</kbd> and <kbd>R</kbd> for where to stand, where to aim and where it lands,
   and <kbd>Q</kbd> for the clip, which the panel calls **Watch**. <kbd>←</kbd> and <kbd>→</kbd>
   move to another stand spot for the same landing, <kbd>↓</kbd> and <kbd>↑</kbd> to the next or
   previous landing, and <kbd>Tab</kbd> between attack and defense. <kbd>Esc</kbd> goes back to
   the map.

Arcwise reopens where you left it, on the same map, agent and monitor. When a new version comes
out, it downloads it in the background and installs it when you close Arcwise, as long as
**Automatic updates** stays on, as it is at first.
[Use Arcwise](docs/using.md) has the settings, the command line, and how updates work. To run
Arcwise without installing it, the release also has `arcwise-<version>-x64.zip`, with the same
`arcwise.exe`; a copy run from the zip doesn't update itself.

If you have Arcwise 0.10.0 or earlier, it can't reach releases from this repository by itself:
run the new installer once over it, and your settings carry over.

**Where its files are.** Arcwise installs in `%LOCALAPPDATA%\Programs\Arcwise` by default. Its
settings are in `%APPDATA%\Arcwise\settings.json`, and its copies of libraries on the web in
`%LOCALAPPDATA%\Arcwise\libraries`. To remove Arcwise, uninstall it from **Settings** > **Apps** >
**Installed apps** (**Apps & features** on Windows 10). The uninstaller asks whether to delete
your settings too. It leaves the copies of web libraries, so delete `%LOCALAPPDATA%\Arcwise` as
well to remove everything.

## Documentation

- [Use Arcwise](docs/using.md): install it on Windows, what it does, its keys and settings, and how it updates itself.
- [Screenshots](docs/using.md#screenshots): every screen, from choosing a map to the settings.
- [Install](docs/install.md): every way to install and update it.
- [Libraries](docs/library-format.md): how Arcwise reads, merges and mirrors lineup libraries.
- [Verify a release](docs/verifying-releases.md): check that a download came from this repository.
- [Design](docs/design.md): the goals, the architecture, and the decisions behind them.
- [Stability](docs/stability.md): what a release can change, and what to read before you upgrade.
- [Code style](docs/style.md): the style rules a tool checks, and the review checklist for the rest.
- [Contributing](CONTRIBUTING.md): set up, run the checks, and send a change.
- [Maintain](docs/maintaining.md): the owner's repository settings, releases and trellis updates.
- [Security policy](SECURITY.md): how to report a vulnerability.
- [Changelog](CHANGELOG.md): what changed in each release.

## License

Released under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE), at
your option.
Release builds include `THIRD-PARTY-LICENSES.html`, the licenses of the open-source crates
Arcwise is built with.

For a build from source, `just notices` writes the licenses of the crates it is built with to
`target/THIRD-PARTY-LICENSES.html`.

Arcwise isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or
anyone officially involved in producing or managing Riot Games properties. Riot Games, and all
associated properties are trademarks or registered trademarks of Riot Games, Inc.
