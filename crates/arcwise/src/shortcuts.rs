//! Keyboard shortcuts people can change: what each command does, the keys that do it, and the
//! key bindings GPUI matches them with. Esc, which closes things, and the number keys, which pick
//! abilities, stay as they are.

use std::collections::BTreeMap;

use gpui::{App, Global, KeyBinding, Keystroke, SharedString, actions};

actions!(
    arcwise,
    [
        NextLanding,
        PreviousLanding,
        NextStand,
        PreviousStand,
        WatchClip,
        ShowStand,
        ShowAim,
        ShowLanding,
        TogglePause,
        RestartClip,
        SlowMotion,
        SwitchSide,
        ToggleSettings,
    ]
);

/// The key context the browser's bindings apply in.
pub const CONTEXT: &str = "Browser";

/// The most shortcuts one command can have.
pub const MOST: usize = 2;

/// Something a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Command {
    NextLanding,
    PreviousLanding,
    NextStand,
    PreviousStand,
    SwitchSide,
    WatchClip,
    ShowStand,
    ShowAim,
    ShowLanding,
    TogglePause,
    RestartClip,
    SlowMotion,
    ToggleSettings,
}

/// The heading a command is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Browse,
    Views,
    Clip,
    Window,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Browse, Group::Views, Group::Clip, Group::Window];

    pub fn label(self) -> &'static str {
        match self {
            Group::Browse => "Browse",
            Group::Views => "Views",
            Group::Clip => "Clip",
            Group::Window => "Window",
        }
    }
}

impl Command {
    pub const ALL: [Command; 13] = [
        Command::NextLanding,
        Command::PreviousLanding,
        Command::NextStand,
        Command::PreviousStand,
        Command::SwitchSide,
        Command::WatchClip,
        Command::ShowStand,
        Command::ShowAim,
        Command::ShowLanding,
        Command::TogglePause,
        Command::RestartClip,
        Command::SlowMotion,
        Command::ToggleSettings,
    ];

    /// What the settings file calls it.
    fn name(self) -> &'static str {
        match self {
            Command::NextLanding => "next_landing",
            Command::PreviousLanding => "previous_landing",
            Command::NextStand => "next_stand",
            Command::PreviousStand => "previous_stand",
            Command::SwitchSide => "switch_side",
            Command::WatchClip => "watch_clip",
            Command::ShowStand => "show_stand",
            Command::ShowAim => "show_aim",
            Command::ShowLanding => "show_landing",
            Command::TogglePause => "toggle_pause",
            Command::RestartClip => "restart_clip",
            Command::SlowMotion => "slow_motion",
            Command::ToggleSettings => "toggle_settings",
        }
    }

    fn named(name: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.name() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Command::NextLanding => "Next landing",
            Command::PreviousLanding => "Previous landing",
            Command::NextStand => "Next stand spot",
            Command::PreviousStand => "Previous stand spot",
            Command::SwitchSide => "Attack or defense",
            Command::WatchClip => "Clip",
            Command::ShowStand => "Stand spot",
            Command::ShowAim => "Aim",
            Command::ShowLanding => "Landing",
            Command::TogglePause => "Pause or play",
            Command::RestartClip => "Restart",
            Command::SlowMotion => "Slow motion",
            Command::ToggleSettings => "Settings",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Command::NextLanding
            | Command::PreviousLanding
            | Command::NextStand
            | Command::PreviousStand
            | Command::SwitchSide => Group::Browse,
            Command::WatchClip | Command::ShowStand | Command::ShowAim | Command::ShowLanding => {
                Group::Views
            }
            Command::TogglePause | Command::RestartClip | Command::SlowMotion => Group::Clip,
            Command::ToggleSettings => Group::Window,
        }
    }

    fn defaults(self) -> &'static [&'static str] {
        match self {
            Command::NextLanding => &["down"],
            Command::PreviousLanding => &["up"],
            Command::NextStand => &["right"],
            Command::PreviousStand => &["left"],
            Command::SwitchSide => &["tab"],
            Command::WatchClip => &["q"],
            Command::ShowStand => &["w"],
            Command::ShowAim => &["e"],
            Command::ShowLanding => &["r"],
            Command::TogglePause => &["space"],
            Command::RestartClip => &["backspace"],
            Command::SlowMotion => &["s"],
            Command::ToggleSettings => &["ctrl-,"],
        }
    }

    fn default_keys(self) -> Vec<Shortcut> {
        self.defaults()
            .iter()
            .map(|k| Shortcut::parse(k).expect("the default shortcuts parse"))
            .collect()
    }

    fn binding(self, keys: &str) -> KeyBinding {
        let context = Some(CONTEXT);
        match self {
            Command::NextLanding => KeyBinding::new(keys, NextLanding, context),
            Command::PreviousLanding => KeyBinding::new(keys, PreviousLanding, context),
            Command::NextStand => KeyBinding::new(keys, NextStand, context),
            Command::PreviousStand => KeyBinding::new(keys, PreviousStand, context),
            Command::SwitchSide => KeyBinding::new(keys, SwitchSide, context),
            Command::WatchClip => KeyBinding::new(keys, WatchClip, context),
            Command::ShowStand => KeyBinding::new(keys, ShowStand, context),
            Command::ShowAim => KeyBinding::new(keys, ShowAim, context),
            Command::ShowLanding => KeyBinding::new(keys, ShowLanding, context),
            Command::TogglePause => KeyBinding::new(keys, TogglePause, context),
            Command::RestartClip => KeyBinding::new(keys, RestartClip, context),
            Command::SlowMotion => KeyBinding::new(keys, SlowMotion, context),
            Command::ToggleSettings => KeyBinding::new(keys, ToggleSettings, context),
        }
    }
}

/// A key, and the modifiers held with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    ctrl: bool,
    alt: bool,
    shift: bool,
    /// GPUI's name for the key, such as `r`, `space` or `down`.
    key: String,
}

/// Why a key cannot become a shortcut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Esc closes things, and cancels choosing a shortcut.
    Escape,
    /// The number keys pick abilities.
    Number,
    /// Windows keeps it for itself.
    Windows,
}

impl Refused {
    pub fn why(self) -> &'static str {
        match self {
            Refused::Escape => "Esc stays as it is: it closes things.",
            Refused::Number => "1 to 9 stay as they are: they pick abilities.",
            Refused::Windows => "Windows keeps that one for itself.",
        }
    }
}

/// Shortcuts with these keys and modifiers belong to Windows.
const WINDOWS_KEEPS: [&str; 6] = [
    "alt-tab",
    "alt-f4",
    "alt-escape",
    "alt-space",
    "ctrl-escape",
    "ctrl-shift-escape",
];

impl Shortcut {
    /// Reads GPUI's notation, such as `ctrl-alt-r`, as long as it makes a shortcut that can be
    /// used.
    pub fn parse(text: &str) -> Option<Shortcut> {
        let keystroke = Keystroke::parse(text).ok()?;
        // GPUI takes any word for a key, which would never be pressed.
        if keystroke.key.chars().any(char::is_whitespace) {
            return None;
        }
        Shortcut::pressed(&keystroke)?.ok()
    }

    /// The shortcut a key press makes, or why it cannot be one. `None` for a modifier pressed on
    /// its own, which only starts one.
    pub fn pressed(keystroke: &Keystroke) -> Option<Result<Shortcut, Refused>> {
        let key = keystroke.key.as_str();
        if key.is_empty() || matches!(key, "shift" | "control" | "alt" | "platform" | "function") {
            return None;
        }
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.function {
            return Some(Err(Refused::Windows));
        }
        let shortcut = Shortcut {
            ctrl: modifiers.control,
            alt: modifiers.alt,
            shift: modifiers.shift,
            key: key.to_owned(),
        };
        let bare = !shortcut.ctrl && !shortcut.alt && !shortcut.shift;
        Some(if key == "escape" && bare {
            Err(Refused::Escape)
        } else if bare && matches!(key, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9") {
            Err(Refused::Number)
        } else if WINDOWS_KEEPS.contains(&shortcut.unparse().as_str()) {
            Err(Refused::Windows)
        } else {
            Ok(shortcut)
        })
    }

    /// GPUI's notation for it, which the settings file keeps.
    pub fn unparse(&self) -> String {
        let mut text = String::new();
        for (held, name) in [
            (self.ctrl, "ctrl-"),
            (self.alt, "alt-"),
            (self.shift, "shift-"),
        ] {
            if held {
                text.push_str(name);
            }
        }
        text.push_str(&self.key);
        text
    }

    /// The key caps to show for it, modifiers first: `["Ctrl", "Alt", "R"]`. A symbol typed with
    /// Shift, which Windows reports as the symbol alone, shows as Shift and its key: Shift+1, not
    /// "!", on a US keyboard.
    pub fn caps(&self) -> Vec<SharedString> {
        let (shift, key) = match shifted_key(&self.key) {
            Some(base) if !self.shift => (true, base.to_string()),
            _ => (self.shift, self.key.clone()),
        };
        let modifiers = [(self.ctrl, "Ctrl"), (self.alt, "Alt"), (shift, "Shift")];
        modifiers
            .into_iter()
            .filter(|(held, _)| *held)
            .map(|(_, name)| SharedString::from(name))
            .chain([key_name(&key)])
            .collect()
    }

    /// One label for it, such as `Ctrl+R`.
    pub fn label(&self) -> SharedString {
        self.caps().join("+").into()
    }
}

/// The key that types `key` with Shift, when `key` is one such symbol in the keyboard layout in
/// use.
pub fn shifted_key(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let (Some(symbol), None) = (chars.next(), chars.next()) else {
        return None;
    };
    // The numpad's symbols reach Arcwise as the same keys as the ones typed with Shift, such as *
    // for Shift+8, so a shortcut with one cannot say which was pressed; its symbol is right
    // for both.
    if symbol.is_alphanumeric() || matches!(symbol, '*' | '+' | '-' | '/' | '.') {
        return None;
    }
    #[cfg(windows)]
    return crate::win::shifted(symbol);
    #[cfg(not(windows))]
    None
}

/// What a key cap says for one of GPUI's key names.
fn key_name(key: &str) -> SharedString {
    let named = match key {
        "space" => "Space",
        "backspace" => "Backspace",
        "tab" => "Tab",
        "enter" => "Enter",
        "escape" => "Esc",
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        "home" => "Home",
        "end" => "End",
        "insert" => "Ins",
        "delete" => "Del",
        _ => {
            let mut chars = key.chars();
            return match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>().into(),
                None => SharedString::default(),
            };
        }
    };
    named.into()
}

/// Every command's shortcuts, as an app-wide global.
#[derive(Clone, Debug, PartialEq)]
pub struct Shortcuts {
    keys: Vec<Vec<Shortcut>>,
}

impl Global for Shortcuts {}

impl Default for Shortcuts {
    fn default() -> Self {
        Shortcuts {
            keys: Command::ALL.map(Command::default_keys).to_vec(),
        }
    }
}

impl Shortcuts {
    /// The defaults with the changes the settings file keeps, from [`Shortcuts::changes`]. Names
    /// and keys it does not know, such as a newer version's, are skipped. Each key is given as
    /// it would be in the Keyboard tab, taken from any command that had it, so a key a hand edit
    /// lists twice ends up with one command.
    pub fn with_changes(changes: &BTreeMap<String, Vec<String>>) -> Shortcuts {
        let changed: Vec<(Command, &Vec<String>)> = changes
            .iter()
            .filter_map(|(name, keys)| Some((Command::named(name)?, keys)))
            .collect();
        let mut shortcuts = Shortcuts::default();
        for (command, _) in &changed {
            shortcuts.keys[*command as usize].clear();
        }
        for (command, keys) in changed {
            for key in keys.iter().filter_map(|key| Shortcut::parse(key)) {
                shortcuts.set(command, MOST, key);
            }
        }
        shortcuts
    }

    /// The commands whose keys differ from the defaults, for the settings file.
    pub fn changes(&self) -> BTreeMap<String, Vec<String>> {
        Command::ALL
            .into_iter()
            .filter(|&c| !self.is_default(c))
            .map(|c| {
                let keys = self.keys(c).iter().map(Shortcut::unparse).collect();
                (c.name().to_owned(), keys)
            })
            .collect()
    }

    pub fn keys(&self, command: Command) -> &[Shortcut] {
        &self.keys[command as usize]
    }

    pub fn first(&self, command: Command) -> Option<&Shortcut> {
        self.keys(command).first()
    }

    pub fn is_default(&self, command: Command) -> bool {
        self.keys(command) == command.default_keys()
    }

    pub fn all_default(&self) -> bool {
        Command::ALL.into_iter().all(|c| self.is_default(c))
    }

    /// The command that has `shortcut`, if any.
    pub fn holder(&self, shortcut: &Shortcut) -> Option<Command> {
        Command::ALL
            .into_iter()
            .find(|&c| self.keys(c).contains(shortcut))
    }

    /// Gives `command` the shortcut at `slot`, adding it after the last when `slot` is past them,
    /// and takes it from any other command that had it.
    pub fn set(&mut self, command: Command, slot: usize, shortcut: Shortcut) {
        for keys in &mut self.keys {
            keys.retain(|s| *s != shortcut);
        }
        let keys = &mut self.keys[command as usize];
        if slot < keys.len() {
            keys[slot] = shortcut;
        } else if keys.len() < MOST {
            keys.push(shortcut);
        }
    }

    pub fn remove(&mut self, command: Command, slot: usize) {
        let keys = &mut self.keys[command as usize];
        if slot < keys.len() {
            keys.remove(slot);
        }
    }

    /// Puts a command's shortcuts back, taking its default keys from any command that has them.
    pub fn reset(&mut self, command: Command) {
        for shortcut in command.default_keys() {
            for keys in &mut self.keys {
                keys.retain(|s| *s != shortcut);
            }
        }
        self.keys[command as usize] = command.default_keys();
    }

    /// The key bindings for these shortcuts. Esc and the number keys, which stay as they are, are
    /// the browser's own key handling.
    fn bindings(&self) -> Vec<KeyBinding> {
        Command::ALL
            .into_iter()
            .flat_map(|command| {
                self.keys(command)
                    .iter()
                    .map(move |shortcut| command.binding(&shortcut.unparse()))
            })
            .collect()
    }
}

/// Makes `shortcuts` the ones in use, and binds their keys in place of the last ones.
pub fn apply(shortcuts: Shortcuts, cx: &mut App) {
    cx.clear_key_bindings();
    cx.bind_keys(shortcuts.bindings());
    cx.set_global(shortcuts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Shortcut {
        Shortcut::parse(text).unwrap()
    }

    fn pressed(text: &str) -> Option<Result<Shortcut, Refused>> {
        Shortcut::pressed(&Keystroke::parse(text).unwrap())
    }

    #[test]
    fn shortcuts_read_and_write_gpui_notation() {
        for text in [
            "r",
            "ctrl-alt-r",
            "shift-/",
            "ctrl--",
            "space",
            "ctrl-shift-f5",
            "ctrl-,",
        ] {
            assert_eq!(key(text).unparse(), text);
        }
        assert_eq!(
            key("CTRL-Alt-r").unparse(),
            "ctrl-alt-r",
            "modifiers in any case"
        );
        assert_eq!(
            key("R").unparse(),
            "shift-r",
            "a capital is Shift, as GPUI reads it"
        );
        assert_eq!(Shortcut::parse("ctrl-"), None, "a modifier alone");
        assert_eq!(Shortcut::parse("ctrl-+-r"), None, "not a keystroke");
    }

    #[test]
    fn caps_spell_out_modifiers_then_the_key() {
        assert_eq!(key("ctrl-alt-r").caps(), ["Ctrl", "Alt", "R"]);
        assert_eq!(key("down").caps(), ["↓"]);
        assert_eq!(key("shift-space").label(), "Shift+Space");
        assert_eq!(key("f5").label(), "F5");
        assert_eq!(key("backspace").label(), "Backspace");
        // The numpad's symbols are the same keys to Arcwise as the ones typed with Shift, so they
        // show as themselves on any layout.
        assert_eq!(key("*").label(), "*");
        assert_eq!(key("+").label(), "+");
        assert_eq!(key("shift-*").label(), "Shift+*");
    }

    #[test]
    fn some_keys_stay_as_they_are() {
        assert_eq!(pressed("escape"), Some(Err(Refused::Escape)));
        assert_eq!(pressed("3"), Some(Err(Refused::Number)));
        assert_eq!(pressed("alt-f4"), Some(Err(Refused::Windows)));
        assert_eq!(pressed("alt-tab"), Some(Err(Refused::Windows)));
        assert_eq!(pressed("win-r"), Some(Err(Refused::Windows)));
        assert_eq!(
            pressed("ctrl-3"),
            Some(Ok(key("ctrl-3"))),
            "a number with Ctrl is free"
        );
        assert_eq!(pressed("shift-escape"), Some(Ok(key("shift-escape"))));
        assert_eq!(pressed("shift"), None, "a modifier only starts one");
    }

    #[test]
    fn the_defaults_are_all_usable_and_distinct() {
        for (i, command) in Command::ALL.into_iter().enumerate() {
            assert_eq!(command as usize, i, "Command::ALL is in declaration order");
        }
        let shortcuts = Shortcuts::default();
        let mut seen = Vec::new();
        for command in Command::ALL {
            assert_eq!(shortcuts.keys(command).len(), command.defaults().len());
            for shortcut in shortcuts.keys(command) {
                assert!(!seen.contains(shortcut), "{shortcut:?} twice");
                seen.push(shortcut.clone());
            }
        }
        assert!(shortcuts.all_default());
        assert!(shortcuts.changes().is_empty());
    }

    #[test]
    fn only_changes_are_saved_and_they_load_back() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set(Command::RestartClip, 1, key("ctrl-r"));
        shortcuts.remove(Command::SlowMotion, 0);
        let changes = shortcuts.changes();
        assert_eq!(
            changes,
            BTreeMap::from([
                (
                    "restart_clip".to_owned(),
                    vec!["backspace".into(), "ctrl-r".into()]
                ),
                ("slow_motion".to_owned(), vec![]),
            ])
        );
        assert_eq!(Shortcuts::with_changes(&changes), shortcuts);
    }

    #[test]
    fn a_key_moves_from_the_command_that_had_it() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set(Command::RestartClip, 0, key("r"));
        assert_eq!(shortcuts.keys(Command::RestartClip), [key("r")]);
        assert!(shortcuts.keys(Command::ShowLanding).is_empty());
        assert_eq!(shortcuts.holder(&key("r")), Some(Command::RestartClip));
        // Its default comes back, and takes r from the restart.
        shortcuts.reset(Command::ShowLanding);
        assert_eq!(shortcuts.keys(Command::ShowLanding), [key("r")]);
        assert!(shortcuts.keys(Command::RestartClip).is_empty());
    }

    #[test]
    fn a_command_has_at_most_two() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set(Command::ShowAim, 1, key("ctrl-e"));
        shortcuts.set(Command::ShowAim, 2, key("alt-e"));
        assert_eq!(shortcuts.keys(Command::ShowAim), [key("e"), key("ctrl-e")]);
    }

    #[test]
    fn a_settings_file_it_does_not_understand_still_loads() {
        let changes = BTreeMap::from([
            ("from_a_newer_version".to_owned(), vec!["ctrl-n".into()]),
            (
                "show_aim".to_owned(),
                vec!["not a key".into(), "escape".into(), "a".into()],
            ),
            (
                "watch_clip".to_owned(),
                vec!["1".into(), "2".into(), "z".into(), "x".into()],
            ),
        ]);
        let shortcuts = Shortcuts::with_changes(&changes);
        assert_eq!(shortcuts.keys(Command::ShowAim), [key("a")]);
        assert_eq!(shortcuts.keys(Command::WatchClip), [key("z"), key("x")]);
        assert_eq!(shortcuts.holder(&key("ctrl-n")), None);
    }

    #[test]
    fn a_key_a_hand_edit_lists_twice_goes_to_one_command() {
        let changes = BTreeMap::from([
            ("restart_clip".to_owned(), vec!["r".into(), "r".into()]),
            ("show_landing".to_owned(), vec!["r".into()]),
        ]);
        let shortcuts = Shortcuts::with_changes(&changes);
        let holders: Vec<_> = Command::ALL
            .into_iter()
            .filter(|&c| shortcuts.keys(c).contains(&key("r")))
            .collect();
        assert_eq!(holders.len(), 1, "{holders:?}");
        assert_eq!(shortcuts.keys(holders[0]), [key("r")], "and only once");
    }

    #[test]
    fn a_changed_command_keeps_a_key_it_took_from_one_left_as_it_was() {
        // Restart moved to r, which landing still had by default.
        let changes = BTreeMap::from([("restart_clip".to_owned(), vec!["r".into()])]);
        let shortcuts = Shortcuts::with_changes(&changes);
        assert_eq!(shortcuts.holder(&key("r")), Some(Command::RestartClip));
        assert!(shortcuts.keys(Command::ShowLanding).is_empty());
    }
}
