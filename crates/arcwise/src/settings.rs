//! What Arcwise remembers between runs, in `%APPDATA%\Arcwise\settings.json`.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use arcwise_core::{AgentId, MapId, Side};
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize};

use crate::placement::Placement;
use crate::theme;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The library folder chosen last.
    pub library: Option<PathBuf>,
    /// What was on screen last, to open on next time.
    pub last: Option<Last>,
    /// Whether the window shows what is behind it, blurred.
    pub blur: bool,
    /// How much of the blur shows, from 0 to 100; see [`theme::strength`].
    #[serde(deserialize_with = "strength")]
    pub blur_strength: u8,
    /// Whether Arcwise updates itself; see [`crate::updater`].
    pub auto_update: bool,
    /// The dark or light theme; see [`theme::mode`].
    pub theme: theme::Mode,
    /// Whether the header stands in for Windows' title bar; see [`crate::titlebar`].
    pub custom_title_bar: bool,
    /// Where the window was when it last moved, to open there again.
    pub window: Option<Placement>,
    /// The version that ran last, to say so when an update has replaced it.
    pub version: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library: None,
            last: None,
            blur: true,
            blur_strength: theme::DEFAULT_STRENGTH,
            auto_update: true,
            theme: theme::Mode::Dark,
            custom_title_bar: true,
            window: None,
            version: None,
        }
    }
}

/// Whether `current` is newer than the version that ran `last`. Without one, as on a first run or
/// after a version that did not note it, nothing says so.
fn updated(last: Option<&str>, current: &Version) -> bool {
    last.and_then(|last| Version::parse(last).ok())
        .is_some_and(|last| last < *current)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Last {
    pub map: MapId,
    pub agent: AgentId,
    pub side: Side,
}

/// Reads the blur strength. Arcwise 0.2 and earlier saved it from 0 to 1, as a decimal such as
/// 0.55, which serde_json keeps apart from a whole number such as 55. A decimal above 1, such as
/// a hand-edited 55.0, is already from 0 to 100.
fn strength<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let number = serde_json::Number::deserialize(deserializer)?;
    let value = number.as_f64().unwrap_or(0.);
    let percent = if number.is_f64() && value <= 1. {
        value * 100.
    } else {
        value
    };
    Ok(percent.clamp(0., 100.).round() as u8)
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|dir| Path::new(&dir).join("Arcwise").join("settings.json"))
    }

    /// Reads the settings, or the defaults when there are none yet or they cannot be read.
    pub fn load() -> Settings {
        Self::path()
            .and_then(|path| Self::read(&path).ok())
            .unwrap_or_default()
    }

    pub fn read(path: &Path) -> anyhow::Result<Settings> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write(&path)
    }

    /// Saves only where the window is, over what is in the file, so another Arcwise's settings
    /// saved since are kept.
    pub fn save_window(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write_over(&path, |saved| saved.window = self.window.clone())
    }

    /// Notes `current` as the version that ran last, and returns it when it replaced an older one
    /// since Arcwise last ran.
    pub fn note_version(&mut self, current: &Version) -> Option<Version> {
        let updated = updated(self.version.as_deref(), current);
        let running = current.to_string();
        if self.version.as_ref() != Some(&running) {
            self.version = Some(running);
            if let Err(e) = self.save_version() {
                eprintln!("arcwise: could not save the version: {e:#}");
            }
        }
        updated.then(|| current.clone())
    }

    /// Saves only the version that ran last, the same way.
    fn save_version(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write_over(&path, |saved| saved.version = self.version.clone())
    }

    /// Changes the settings in the file with `change`, or these if there is no file yet. A file
    /// that does not parse, as after a mistaken hand edit, is left for its owner to fix.
    fn write_over(&self, path: &Path, change: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
        let mut saved = if path.exists() {
            Self::read(path)?
        } else {
            self.clone()
        };
        change(&mut saved);
        saved.write(path)
    }

    /// Writes beside the target and renames over it, so a crash never leaves half a file.
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        // Named for this process, so two Arcwise windows saving at once never write one file.
        let part = path.with_extension(format!("json.{}.part", std::process::id()));
        std::fs::write(&part, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", part.display()))?;
        std::fs::rename(&part, path).with_context(|| format!("replacing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_a_file() {
        let dir = std::env::temp_dir().join(format!("arcwise-settings-{}", std::process::id()));
        let path = dir.join("nested").join("settings.json");
        let settings = Settings {
            library: Some(PathBuf::from(r"D:\lineups")),
            last: Some(Last {
                map: MapId::new("m-ascent"),
                agent: AgentId::new("a-sova"),
                side: Side::Defense,
            }),
            blur: false,
            blur_strength: 30,
            auto_update: false,
            theme: theme::Mode::Light,
            custom_title_bar: false,
            window: Some(Placement {
                x: -1200.,
                y: 40.5,
                width: 1300.,
                height: 820.,
                maximized: true,
                monitor: Some("a1b2c3d4-0000-0000-0000-000000000002".into()),
            }),
            version: Some("0.6.0".into()),
        };
        settings.write(&path).unwrap();
        assert_eq!(Settings::read(&path).unwrap(), settings);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_the_window_keeps_what_another_arcwise_saved() {
        let dir = std::env::temp_dir().join(format!("arcwise-window-{}", std::process::id()));
        let path = dir.join("settings.json");
        let other = Settings {
            theme: theme::Mode::Light,
            ..Settings::default()
        };
        other.write(&path).unwrap();
        let this = Settings {
            window: Some(Placement {
                x: 10.,
                y: 20.,
                width: 1200.,
                height: 800.,
                maximized: false,
                monitor: None,
            }),
            ..Settings::default()
        };
        this.write_over(&path, |saved| saved.window = this.window.clone())
            .unwrap();
        let read = Settings::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(read.theme, theme::Mode::Light, "the other's theme is kept");
        assert_eq!(read.window, this.window);
    }

    #[test]
    fn saving_one_setting_leaves_a_file_that_does_not_parse() {
        let dir = std::env::temp_dir().join(format!("arcwise-broken-{}", std::process::id()));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ \"theme\": \"dark\", oops }").unwrap();
        let this = Settings {
            version: Some("0.7.0".into()),
            ..Settings::default()
        };
        let saved = this.write_over(&path, |saved| saved.version = this.version.clone());
        let left = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(saved.is_err());
        assert_eq!(left, "{ \"theme\": \"dark\", oops }");
    }

    #[test]
    fn saving_one_setting_starts_a_file_that_is_not_there() {
        let dir = std::env::temp_dir().join(format!("arcwise-fresh-{}", std::process::id()));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        let this = Settings {
            version: Some("0.7.0".into()),
            ..Settings::default()
        };
        this.write_over(&path, |saved| saved.version = this.version.clone())
            .unwrap();
        let read = Settings::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(read, this);
    }

    #[test]
    fn an_update_is_a_newer_version_than_the_last_run() {
        let current = Version::new(0, 7, 0);
        assert!(updated(Some("0.6.0"), &current));
        assert!(!updated(Some("0.7.0"), &current), "the same version again");
        assert!(
            !updated(Some("0.8.0"), &current),
            "an older version installed over it"
        );
        assert!(
            !updated(None, &current),
            "a first run, or one after 0.6.0 and earlier"
        );
        assert!(!updated(Some("not a version"), &current));
    }

    #[test]
    fn missing_fields_take_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert!(settings.blur, "blur is on until turned off");
        assert_eq!(settings.blur_strength, theme::DEFAULT_STRENGTH);
        assert!(settings.auto_update, "updates are on until turned off");
        assert_eq!(settings.theme, theme::Mode::Dark, "dark until changed");
        assert!(
            settings.custom_title_bar,
            "the header is the title bar until changed"
        );
    }

    #[test]
    fn reads_the_strength_older_versions_saved_from_0_to_1() {
        let read = |json: &str| {
            serde_json::from_str::<Settings>(json)
                .unwrap()
                .blur_strength
        };
        assert_eq!(read(r#"{"blur_strength": 0.55}"#), 55);
        assert_eq!(read(r#"{"blur_strength": 1.0}"#), 100);
        assert_eq!(read(r#"{"blur_strength": 0.0}"#), 0);
        assert_eq!(
            read(r#"{"blur_strength": 1}"#),
            1,
            "a whole number is already 0 to 100"
        );
        assert_eq!(read(r#"{"blur_strength": 250}"#), 100);
        assert_eq!(
            read(r#"{"blur_strength": 55.0}"#),
            55,
            "a decimal above 1 is already 0 to 100"
        );
        assert_eq!(read(r#"{"blur_strength": -0.5}"#), 0);
    }

    #[test]
    fn settings_saved_before_updates_existed_turn_them_on() {
        let settings: Settings =
            serde_json::from_str(r#"{"blur": false, "blur_strength": 30}"#).unwrap();
        assert!(!settings.blur);
        assert!(settings.auto_update);
    }

    #[test]
    fn saves_the_theme_by_name() {
        let settings = Settings {
            theme: theme::Mode::Light,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["theme"], "light");
        let read: Settings = serde_json::from_str(r#"{"theme": "dark"}"#).unwrap();
        assert_eq!(read.theme, theme::Mode::Dark);
    }
}
