# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/265866/arcwise/releases/tag/v0.1.0) - 2026-10-20

### Added

- *(header)* reset settings to defaults
- *(header)* blur strength slider
- *(header)* settings menu behind a gear
- *(ui)* blur what is behind the window
- *(board)* click empty board to close the lineup
- *(board)* show the map behind the agent step
- *(ui)* drop the brand mark from the header
- *(app)* time frames, inputs and loads with ARCWISE_TRACE
- *(app)* open another library from the header
- *(app)* reopen on the map, agent and side from last time
- *(app)* slide the lineup panel in beside the map
- *(app)* open aim and landing pictures full size
- *(app)* keyboard shortcuts for landings, stand spots, views and playback
- *(app)* play the selected lineup's clip, with pause and slow motion
- *(app)* lineup panel with stills, a stand close-up and the throw's steps
- *(app)* stand markers and throw paths for the selected landing
- *(app)* landing markers on the map, with an ability filter and hover cards
- *(app)* map and agent choosers in the header
- *(app)* load a library from --library, the last folder, or a folder picker
- *(app)* open the main window

### Fixed

- *(board)* draw lineup counts above every marker
- *(app)* keep the readied clip when the panel closes
- *(app)* keep work done ahead in step with what changed
- *(app)* set up libwebp on one thread before decoding
- *(app)* let only the latest library request win, and keep notice clicks off the map
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
- *(app)* read the library while the window opens
- *(app)* fetch the pictures of the landings the keys reach next
- *(app)* ready the clip likely to play next
- *(app)* decode WebP pictures with libwebp
- *(app)* reuse the map and header between clip frames
- *(app)* draw maps and icons before they are asked for
- *(app)* anchor dropdowns under their pickers
- *(core)* derive the selected landing from the selected lineup
- give the app an icon, version information and a minimum window size
- *(app)* fetch the pictures of nearby lineups ahead of time
- decode clips at the size the viewer shows them
- pin Rust 1.99 and declare it the minimum version
