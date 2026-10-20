//! What Arcwise remembers between runs, in `%APPDATA%\Arcwise\settings.json`.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use arcwise_core::{AgentId, MapId, Side};
use serde::{Deserialize, Serialize};

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
    /// How much of the blur shows, from 0 to 1; see [`theme::strength`].
    pub blur_strength: f32,
    /// Whether Arcwise updates itself; see [`crate::update`].
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library: None,
            last: None,
            blur: true,
            blur_strength: theme::DEFAULT_STRENGTH,
            auto_update: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Last {
    pub map: MapId,
    pub agent: AgentId,
    pub side: Side,
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
            blur_strength: 0.3,
            auto_update: false,
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
    }

    #[test]
    fn settings_saved_before_updates_existed_turn_them_on() {
        let settings: Settings =
            serde_json::from_str(r#"{"blur": false, "blur_strength": 0.3}"#).unwrap();
        assert!(!settings.blur);
        assert!(settings.auto_update);
    }
}
