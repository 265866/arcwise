# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.1](https://github.com/265866/arcwise/compare/v0.8.0...v0.8.1) - 2026-10-22

### Fixed

- *(app)* only say an update is out when Arcwise opens with automatic updates off ([#24](https://github.com/265866/arcwise/pull/24))

## [0.8.0](https://github.com/265866/arcwise/compare/v0.7.0...v0.8.0) - 2026-10-22

### Added

- *(app)* theme can follow Windows, starting speed, open where left off, and check for updates now ([#21](https://github.com/265866/arcwise/pull/21))
- *(app)* change keyboard shortcuts in the settings, with Ctrl, Alt and Shift and two per command ([#20](https://github.com/265866/arcwise/pull/20))
- *(app)* settings open in a window of their own, with a tab for each kind ([#19](https://github.com/265866/arcwise/pull/19))

### Fixed

- *(app)* Arcwise fits a 1080p laptop at 150% scaling ([#22](https://github.com/265866/arcwise/pull/22))

## [0.7.0](https://github.com/265866/arcwise/compare/v0.6.0...v0.7.0) - 2026-10-21

### Added

- *(app)* say when an update is ready or installed, and look for one hourly ([#17](https://github.com/265866/arcwise/pull/17))

## [0.6.0](https://github.com/265866/arcwise/compare/v0.5.0...v0.6.0) - 2026-10-21

### Added

- *(app)* drag along the progress bar to scrub the clip ([#15](https://github.com/265866/arcwise/pull/15))

## [0.5.0](https://github.com/265866/arcwise/compare/v0.4.0...v0.5.0) - 2026-10-21

### Added

- *(app)* Backspace replays the clip and its progress bar seeks ([#13](https://github.com/265866/arcwise/pull/13))

## [0.4.0](https://github.com/265866/arcwise/compare/v0.3.0...v0.4.0) - 2026-10-21

### Added

- *(app)* light theme, a title bar in the header, and the window reopens where it was ([#11](https://github.com/265866/arcwise/pull/11))

## [0.3.0](https://github.com/265866/arcwise/compare/v0.2.0...v0.3.0) - 2026-10-20

### Added

- *(app)* updates install quietly, with a badge and relaunch button when off ([#9](https://github.com/265866/arcwise/pull/9))

## [0.2.0](https://github.com/265866/arcwise/compare/v0.1.0...v0.2.0) - 2026-10-20

### Added

- *(app)* update to new releases verified by their GitHub attestation ([#6](https://github.com/265866/arcwise/pull/6))

## [0.1.0](https://github.com/265866/arcwise/releases/tag/v0.1.0) - 2026-10-20

### Added

- *(header)* reset settings to defaults
- *(header)* blur strength slider
- *(header)* settings menu behind a gear
- *(ui)* blur what is behind the window
- *(board)* click empty board to close the lineup
- *(board)* show the map behind the agent step
- *(ui)* drop the brand mark from the header
- *(video)* open a clip ahead of time
- *(app)* time frames, inputs and loads with ARCWISE_TRACE
- *(app)* open another library from the header
- *(app)* reopen on the map, agent and side from last time
- *(app)* slide the lineup panel in beside the map
- *(app)* open aim and landing pictures full size
- *(app)* keyboard shortcuts for landings, stand spots, views and playback
- *(app)* play the selected lineup's clip, with pause and slow motion
- *(video)* decode clips with Media Foundation on a thread of their own
- *(app)* lineup panel with stills, a stand close-up and the throw's steps
- *(app)* stand markers and throw paths for the selected landing
- *(app)* landing markers on the map, with an ability filter and hover cards
- *(app)* map and agent choosers in the header
- *(app)* load a library from --library, the last folder, or a folder picker
- *(app)* open the main window
- *(core)* browsing state for choosers, filters and selection
- *(core)* group lineups into landings
- *(core)* read lineup libraries from library.json

### Fixed

- *(board)* draw lineup counts above every marker
- *(app)* keep the readied clip when the panel closes
- *(app)* keep work done ahead in step with what changed
- *(video)* prepare a clip even while the one playing is paused
- *(app)* set up libwebp on one thread before decoding
- *(video)* recover from a lost GPU and measure buffer offsets by address
- *(app)* let only the latest library request win, and keep notice clicks off the map
- *(core)* refuse a library that lists a map, agent or ability twice
- *(app)* keep the open library when another fails, and free its images
- *(app)* keep clicks on a dropdown's backdrop from reaching the map
- *(app)* show the landings when a side's map picture is missing
- *(app)* hold the clip still while the viewer shows a picture
- *(video)* bound frame copies by the real texture and buffer
- *(app)* say a clip is missing instead of blaming the codec

### Other

- windows installer and release build ([#3](https://github.com/265866/arcwise/pull/3))
- *(app)* draw maps at the size the board shows them
- *(app)* draw the clip over the panel, not inside it
- *(app)* draw shapes without GPU paths
- *(app)* keep 128 MB of decoded pictures, not 512
- *(app)* keep aim pictures at the size the viewer shows them
- *(video)* ask Media Foundation for low-latency decoding
- *(app)* read the library while the window opens
- *(app)* fetch the pictures of the landings the keys reach next
- *(video)* send a clip's first frame before preparing the next
- *(app)* ready the clip likely to play next
- *(app)* decode WebP pictures with libwebp
- *(app)* reuse the map and header between clip frames
- *(app)* draw maps and icons before they are asked for
- *(core)* move the path into the read error
- *(app)* anchor dropdowns under their pickers
- *(core)* derive the selected landing from the selected lineup
- give the app an icon, version information and a minimum window size
- *(app)* fetch the pictures of nearby lineups ahead of time
- decode clips at the size the viewer shows them
- pin Rust 1.99 and declare it the minimum version
