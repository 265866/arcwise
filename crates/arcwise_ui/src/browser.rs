//! The lineup browser: the header, the map, the lineup panel beside it, the choosers and the
//! settings window over them, and the keys that drive them, over one library.

mod board;
mod browser_view;
mod chooser;
mod events;
mod follow;
mod header;
mod keyboard;
mod keys;
mod libraries;
mod panel;
mod part;
mod settings_window;
mod slide;
mod tab;

pub(crate) use browser_view::{BrowserView, first_pictures};
pub(crate) use events::{
    AddLibraryFolder, Chosen, Relaunch, SetAutoUpdate, SetBlur, SetBlurStrength, SetCustomTitleBar,
    SetLibraries, SetReopenLast, SetShortcuts, SetStartSpeed, SetTheme,
};
pub(crate) use libraries::Libraries;
pub(crate) use tab::Tab;
