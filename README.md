# Arcwise

A desktop lineup browser for Valorant. Pick a map and an agent, see where each piece of utility
lands, and watch exactly where to stand, where to aim and how to throw.

Arcwise is a separate window, not an overlay: keep it on a second monitor while you play.

![Choosing a map](docs/screenshots/choose-map.png)

## What it does

- **Map first, then agent.** The first run asks for a map, then an agent who has lineups on it.
  After that the header's pickers open as dropdowns, and Arcwise reopens where you left off.
- **Every landing on the map.** Lineups of one ability that land in the same place share a marker,
  with a count. Point at one to see what lands there and where it is thrown from.
- **Each stand spot for a landing.** Select a landing to see its stand spots, numbered, with the
  throw drawn from each.
- **The throw, step by step.** The panel plays the clip and has four views: the full clip, the map
  around the stand spot, the crosshair at the throw, and where it lands. A note over the clip says
  when the throw is more than a plain click, such as a jump throw.
- **Hardware video.** Clips decode on the GPU through Windows Media Foundation and play at full
  frame rate even while another window has focus, at about an eighth of one CPU core.

![Choosing an agent](docs/screenshots/choose-agent.png)

## Keyboard

| Keys | Does |
| --- | --- |
| <kbd>↑</kbd> <kbd>↓</kbd> | Previous or next landing |
| <kbd>←</kbd> <kbd>→</kbd> | Previous or next stand spot for the landing |
| <kbd>1</kbd> to <kbd>9</kbd> | All abilities, then one ability at a time |
| <kbd>Q</kbd> <kbd>W</kbd> <kbd>E</kbd> <kbd>R</kbd> | Clip, stand spot, aim, landing |
| <kbd>Space</kbd> | Pause or play the clip |
| <kbd>S</kbd> | Slow motion: ½×, then ¼×, then back to full speed |
| <kbd>Tab</kbd> | Attack or defense |
| <kbd>Esc</kbd> | Close the picture, chooser or lineup |

## Lineups

Arcwise does not come with lineups or download any. It reads a library folder on your computer;
[docs/library-format.md](docs/library-format.md) describes the layout. On first run it asks for
the folder, and the header's library button opens another one.

## Requirements

- Windows 10 or 11 with a Direct3D 11 GPU.
- For AV1 clips, the [AV1 Video Extension](https://apps.microsoft.com/detail/9mvzqvxjbq9v) from
  the Microsoft Store. Windows 11 usually has it already.

## Building

Arcwise is written in Rust with [GPUI](https://www.gpui.rs/), the UI framework behind the Zed
editor. The toolchain version is pinned in `rust-toolchain.toml`.

```sh
cargo run --release
```

The first build compiles GPUI and takes a few minutes. Command-line flags open a library or a
lineup directly, which helps while working on the UI:

```sh
cargo run --release -- --library D:\lineups --map Ascent --agent Sova --side attack
cargo run --release -- --lineup 211 --view aim
```

The workspace has three crates:

| Crate | Holds |
| --- | --- |
| `arcwise-core` | The library format, landing clusters and browsing state. No UI; most tests live here. |
| `arcwise-video` | The Media Foundation decode thread. |
| `arcwise` | The GPUI app. |

`cargo test` runs everything, including decoding a small H.264 clip through Media Foundation.
Two tests need files of your own and are ignored by default:

```sh
ARCWISE_LIBRARY=D:\lineups cargo test -p arcwise-core -- --ignored
ARCWISE_CLIP=D:\lineups\lineups\211.mp4 cargo test -p arcwise-video -- --ignored
```

To see where time goes, name a file in `ARCWISE_TRACE`. The app writes a CSV line for every
frame (how long it took to draw), every key and click (how long until the first frame after
it, and until nothing on screen is still loading), and every picture or clip it loads:

```sh
ARCWISE_TRACE=trace.csv cargo run --release
```

## License

[MIT](LICENSE).

Arcwise isn't endorsed by Riot Games and doesn't reflect the views or opinions of Riot Games or
anyone officially involved in producing or managing Riot Games properties. Riot Games, and all
associated properties are trademarks or registered trademarks of Riot Games, Inc.
