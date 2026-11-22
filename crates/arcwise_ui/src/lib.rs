//! Arcwise's window: the theme, the pieces of UI and drawing its screens share, the clip player,
//! the keyboard shortcuts, the settings file, offline downloads, updates, and the image store
//! behind the lineup browser. [`run()`] starts it.

mod app;
mod browser;
mod downloads;
mod drawn;
mod images;
mod pacing;
mod placement;
mod player;
mod run;
mod settings;
mod shortcuts;
mod theme;
mod titlebar;
mod trace;
mod ui;
mod updater;

pub use run::run;
