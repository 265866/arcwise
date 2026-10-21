//! What Arcwise remembers between runs, in `%APPDATA%\Arcwise\settings.json`.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use arcwise_core::{AgentId, MapId, Side};
use serde::{Deserialize, Deserializer, Serialize};

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
        }
    }
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

    /// Writes beside the target and renames over it, so a crash never leaves half a file.
    pub fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let part = path.with_extension("json.part");
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
        };
        settings.write(&path).unwrap();
        assert_eq!(Settings::read(&path).unwrap(), settings);
        std::fs::remove_dir_all(&dir).unwrap();
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
