//! What the browser tells the app about: the choice on screen, and every setting changed in the
//! settings window, for the app to save or act on.

use std::collections::BTreeMap;

use arcwise_core::{AgentId, MapId, Side};

use crate::pacing::Speed;
use crate::settings::LibrarySource;

/// Emitted when the map, agent or side on screen changes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chosen {
    pub(crate) map: MapId,
    pub(crate) agent: AgentId,
    pub(crate) side: Side,
}

/// Emitted when the user asks to open a different library.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AddLibraryFolder;

/// The list of libraries changed in the settings; `reload` when which lineups show changes, not
/// only which are kept offline.
#[derive(Clone, Debug)]
pub(crate) struct SetLibraries {
    pub(crate) list: Vec<LibrarySource>,
    pub(crate) reload: bool,
}

/// Emitted when the user turns background blur on or off, which [`crate::theme::blur`] already
/// shows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetBlur(pub(crate) bool);

/// Emitted when the user lets go of the blur strength track, with where it stopped, from 0 to
/// 100.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetBlurStrength(pub(crate) u8);

/// Emitted when the user turns automatic updates on or off, which the
/// [`Updater`](crate::updater::Updater) already knows.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetAutoUpdate(pub(crate) bool);

/// Emitted when the user picks the dark or light theme, or Windows', which [`crate::theme::mode`]
/// and [`crate::theme::follows_system`] already show.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetTheme;

/// Emitted when the user turns the custom title bar on or off, which
/// [`crate::titlebar::chosen`] already shows. It takes effect when Arcwise relaunches.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetCustomTitleBar(pub(crate) bool);

/// Emitted when the user changes a keyboard shortcut, with every one that differs from the
/// defaults, as [`Shortcuts::changes`](crate::shortcuts::Shortcuts::changes) gives them.
#[derive(Clone, Debug)]
pub(crate) struct SetShortcuts(pub(crate) BTreeMap<String, Vec<String>>);

/// Emitted when the user turns opening where they left off on or off.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetReopenLast(pub(crate) bool);

/// Emitted when the user picks the speed clips start at.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetStartSpeed(pub(crate) Speed);

/// Emitted when the user asks Arcwise to relaunch, to apply a setting that needs a new window.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Relaunch;
