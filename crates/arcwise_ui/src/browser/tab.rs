//! The settings window's pages, down its left side.

/// A page of the settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    General,
    Appearance,
    Playback,
    Keyboard,
    Updates,
}

impl Tab {
    /// Every page, in the order the sidebar lists them.
    pub(super) const ALL: [Tab; 5] = [
        Tab::General,
        Tab::Appearance,
        Tab::Playback,
        Tab::Keyboard,
        Tab::Updates,
    ];

    /// The page's name, in the sidebar and over the page.
    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Appearance => "Appearance",
            Tab::Playback => "Playback",
            Tab::Keyboard => "Keyboard",
            Tab::Updates => "Updates",
        }
    }

    /// The page's icon in the sidebar: a folder, a half-filled circle, a screen with a play mark,
    /// a keyboard, and a ring around an arrow pointing up.
    pub(super) fn icon(self) -> &'static [u8] {
        match self {
            Tab::General => FOLDER,
            Tab::Appearance => CONTRAST,
            Tab::Playback => PLAYBACK,
            Tab::Keyboard => KEYBOARD,
            Tab::Updates => UPDATES,
        }
    }
}

const FOLDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" fill="none" stroke="#000" stroke-width="2" stroke-linejoin="round"/></svg>"##;
const CONTRAST: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" fill="none" stroke="#000" stroke-width="2"/><path d="M12 3.5a8.5 8.5 0 0 0 0 17z"/></svg>"##;
const UPDATES: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" fill="none" stroke="#000" stroke-width="2"/><path d="M12 16V8.5M8.75 11.5 12 8.25l3.25 3.25" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const PLAYBACK: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="2" fill="none" stroke="#000" stroke-width="2"/><path d="M10 9.2v5.6l4.6-2.8z"/></svg>"##;
const KEYBOARD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="2.5" y="6" width="19" height="12" rx="2" fill="none" stroke="#000" stroke-width="2"/><path d="M6.5 10h.01M10.5 10h.01M14.5 10h.01M18 10h.01M7.5 14h9" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##;
