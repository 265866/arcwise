//! What Arcwise remembers between runs, in `%APPDATA%\Arcwise\settings.json`.
//!
//! The file's format is the one every earlier Arcwise wrote and read: the field names, defaults
//! and the fields left out when empty stay as they are, so a file from any version loads, and a
//! file this version writes still loads in an older one. Fields the file has that this version
//! doesn't know are dropped when it is written again, as they always were.

use std::collections::BTreeMap;
use std::ops::Not;
use std::path::{Path, PathBuf};
use std::{env, fs, io, iter, process};

use anyhow::Context as _;
use arcwise_core::{AgentId, MapId, Side};
use arcwise_remote::remote;
use semver::Version;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Number, Value};

use crate::pacing::Speed;
use crate::placement::Placement;
use crate::theme::{self, Mode};

/// The UTF-8 byte order mark, which some editors save at the start of a file.
const BYTE_ORDER_MARK: &[u8] = b"\xEF\xBB\xBF";

/// Everything the settings file holds. A field missing from the file takes its default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool but `file_unread`, which is never saved, is a separate on/off setting the \
              file has always stored as true or false under its own name, and an enum per setting \
              would change the file's format"
)]
pub(crate) struct Settings {
    /// The library folder chosen last, before Arcwise read libraries from the web. Kept for an
    /// older Arcwise; [`Settings::libraries`] carries it over.
    pub(crate) library: Option<PathBuf>,
    /// The libraries lineups come from, in order: web addresses and folders. Unset until the list
    /// is first changed, while [`Settings::libraries`] gives the default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) libraries: Option<Vec<LibrarySource>>,
    /// What was on screen last, to open on next time.
    pub(crate) last: Option<Last>,
    /// Whether the window shows what is behind it, blurred.
    pub(crate) blur: bool,
    /// How much of the blur shows, from 0 to 100; see [`theme::strength`].
    #[serde(deserialize_with = "strength")]
    pub(crate) blur_strength: u8,
    /// Whether Arcwise updates itself; see [`crate::updater`].
    pub(crate) auto_update: bool,
    /// The dark or light theme; see [`theme::mode`]. While the theme follows Windows', the one it
    /// showed last.
    pub(crate) theme: Mode,
    /// Whether the theme follows Windows' light or dark mode, the settings' System choice.
    pub(crate) follow_system_theme: bool,
    /// Whether Arcwise opens on the map, agent and side shown last.
    pub(crate) reopen_last: bool,
    /// The speed clips play at when Arcwise opens.
    #[serde(deserialize_with = "or_default")]
    pub(crate) start_speed: Speed,
    /// Whether the header stands in for Windows' title bar; see [`crate::titlebar`].
    pub(crate) custom_title_bar: bool,
    /// Where the window was when it last moved, to open there again.
    pub(crate) window: Option<Placement>,
    /// The version that ran last, to say so when an update has replaced it.
    pub(crate) version: Option<String>,
    /// Keyboard shortcuts changed from the defaults; see
    /// [`crate::shortcuts::Shortcuts::changes`].
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) shortcuts: BTreeMap<String, Vec<String>>,
    /// Whether the file could not be read when Arcwise opened, so these settings started from the
    /// defaults. [`Settings::save`] then leaves it alone until Arcwise opens again, even if it is
    /// fixed meanwhile, so these never replace what its owner fixed.
    #[serde(skip)]
    pub(crate) file_unread: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library: None,
            libraries: None,
            last: None,
            blur: true,
            blur_strength: theme::DEFAULT_STRENGTH,
            auto_update: true,
            theme: Mode::Dark,
            follow_system_theme: false,
            reopen_last: true,
            start_speed: Speed::Full,
            custom_title_bar: true,
            window: None,
            version: None,
            shortcuts: BTreeMap::new(),
            file_unread: false,
        }
    }
}

/// Whether `current` is newer than the version that ran `last`. Without one, as on a first run or
/// after a version that did not note it, nothing says so.
fn updated(last: Option<&str>, current: &Version) -> bool {
    last.and_then(|last| Version::parse(last).ok())
        .is_some_and(|last| last < *current)
}

/// A library in the settings' list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct LibrarySource {
    /// A web address, or a folder on disk.
    pub(crate) location: String,
    /// Whether its lineups are shown.
    #[serde(default = "shown")]
    pub(crate) on: bool,
    /// Maps whose lineups are kept on disk to use offline; only for a library on the web.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) keep: Vec<MapId>,
    /// Whether every map is kept on disk, including ones the library adds later.
    #[serde(default, skip_serializing_if = "Not::not")]
    pub(crate) keep_all: bool,
}

/// A library in a file that doesn't say whether it is shown is shown.
fn shown() -> bool {
    true
}

impl LibrarySource {
    /// The library at `location`, shown, with no maps kept on disk.
    #[must_use]
    pub(crate) fn new(location: String) -> Self {
        LibrarySource {
            location,
            on: true,
            keep: Vec::new(),
            keep_all: false,
        }
    }
}

/// The map, agent and side on screen when Arcwise last closed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Last {
    pub(crate) map: MapId,
    pub(crate) agent: AgentId,
    pub(crate) side: Side,
}

/// Reads a value, or takes its default when it is one this version does not know, such as a
/// newer version's, rather than failing the whole file.
fn or_default<'de, D: Deserializer<'de>, T: DeserializeOwned + Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}

/// Reads the blur strength. Arcwise 0.2 and earlier saved it from 0 to 1, as a decimal such as
/// 0.55, which `serde_json` keeps apart from a whole number such as 55. A decimal above 1, such
/// as a hand-edited 55.0, is already from 0 to 100.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the percentage is clamped to 0 to 100 and rounded before the cast, so it fits a u8"
)]
fn strength<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let number = Number::deserialize(deserializer)?;
    let value = number.as_f64().unwrap_or(0.);
    let percent = if number.is_f64() && value <= 1. {
        value * 100.
    } else {
        value
    };
    Ok(percent.clamp(0., 100.).round() as u8)
}

impl Settings {
    /// The libraries to show: the list as last changed, or Arcwise's own library on the web,
    /// followed by the folder chosen before there was a list.
    #[must_use]
    pub(crate) fn libraries(&self) -> Vec<LibrarySource> {
        self.libraries.clone().unwrap_or_else(|| {
            iter::once(LibrarySource::new(remote::DEFAULT.to_owned()))
                .chain(
                    self.library
                        .as_ref()
                        .map(|path| LibrarySource::new(path.to_string_lossy().into_owned())),
                )
                .collect()
        })
    }

    /// Where the settings file is: `%APPDATA%\Arcwise\settings.json`, or nowhere without
    /// `APPDATA`.
    #[must_use]
    pub(crate) fn path() -> Option<PathBuf> {
        env::var_os("APPDATA").map(|dir| Path::new(&dir).join("Arcwise").join("settings.json"))
    }

    /// Reads the settings, or the defaults when there are none yet or they cannot be read. A
    /// file that is there but cannot be read is reported, and [`Settings::save`] leaves it alone
    /// for the rest of this run; see [`Settings::file_unread`].
    #[must_use]
    pub(crate) fn load() -> Settings {
        Self::path().map_or_else(Settings::default, |path| Self::load_from(&path))
    }

    /// [`Settings::load`], from the file at `path`.
    fn load_from(path: &Path) -> Settings {
        match Self::read(path) {
            Ok(Some(settings)) => settings,
            Ok(None) => Settings::default(),
            Err(e) => {
                remote::report(format_args!("{e:#}, so Arcwise leaves it as it is"));
                Settings {
                    file_unread: true,
                    ..Settings::default()
                }
            }
        }
    }

    /// Reads the settings file at `path`: `None` when there is none, or it is empty, as a write cut
    /// short can leave it, or holds only whitespace or NUL bytes, as a power cut can leave it. A
    /// byte order mark at its start, which some editors save, is skipped.
    ///
    /// # Errors
    ///
    /// When the file is there but cannot be read, or is not a settings file.
    fn read(path: &Path) -> anyhow::Result<Option<Settings>> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let json = bytes.strip_prefix(BYTE_ORDER_MARK).unwrap_or(&bytes);
        if json.iter().all(|b| b.is_ascii_whitespace() || *b == 0) {
            return Ok(None);
        }
        serde_json::from_slice(json)
            .map(Some)
            .with_context(|| format!("parsing {}", path.display()))
    }

    /// Saves every setting over the file, unless it could not be read when Arcwise opened (see
    /// [`Settings::file_unread`]) or cannot be read now.
    ///
    /// # Errors
    ///
    /// When `APPDATA` is not set, and as [`Settings::write_whole`] says.
    pub(crate) fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write_whole(&path)
    }

    /// Writes every setting to `path`, unless a file is there that does not parse, as after a
    /// mistaken hand edit, or did not when Arcwise opened. The file is left for its owner to fix
    /// rather than replaced by these settings.
    ///
    /// # Errors
    ///
    /// When the file could not be read when Arcwise opened, is there but cannot be read or does
    /// not parse, or cannot be written.
    fn write_whole(&self, path: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.file_unread,
            "{} could not be read when Arcwise opened, so Arcwise leaves it as it is until it \
             opens again",
            path.display()
        );
        Self::read(path)?;
        self.write(path)
    }

    /// Saves only where the window is, over what is in the file, so another Arcwise's settings
    /// saved since are kept.
    ///
    /// # Errors
    ///
    /// When `APPDATA` is not set, or the file cannot be read or written.
    pub(crate) fn save_window(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write_over(&path, |saved| saved.window.clone_from(&self.window))
    }

    /// Notes `current` as the version that ran last, and returns it when it replaced an older one
    /// since Arcwise last ran.
    pub(crate) fn note_version(&mut self, current: &Version) -> Option<Version> {
        let updated = updated(self.version.as_deref(), current);
        let running = current.to_string();
        if self.version.as_ref() != Some(&running) {
            self.version = Some(running);
            if let Err(e) = self.save_version() {
                remote::report(format_args!("could not save the version: {e:#}"));
            }
        }
        updated.then(|| current.clone())
    }

    /// Saves only the version that ran last, the same way.
    fn save_version(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write_over(&path, |saved| saved.version.clone_from(&self.version))
    }

    /// Saves only the theme shown, the same way, as when it followed Windows' without anyone
    /// changing a setting.
    ///
    /// # Errors
    ///
    /// When `APPDATA` is not set, or the file cannot be read or written.
    pub(crate) fn save_theme(&self) -> anyhow::Result<()> {
        let path = Self::path().context("APPDATA is not set")?;
        self.write_theme(&path)
    }

    /// Only while the file still follows Windows' theme: another Arcwise may have had a theme
    /// chosen since, which this must not undo.
    fn write_theme(&self, path: &Path) -> anyhow::Result<()> {
        self.write_over(path, |saved| {
            if saved.follow_system_theme {
                saved.theme = self.theme;
            }
        })
    }

    /// Changes the settings in the file with `change`, or these if there is no file yet. A file
    /// that does not parse, as after a mistaken hand edit, is left for its owner to fix.
    fn write_over(&self, path: &Path, change: impl FnOnce(&mut Settings)) -> anyhow::Result<()> {
        let mut saved = Self::read(path)?.unwrap_or_else(|| self.clone());
        change(&mut saved);
        saved.write(path)
    }

    /// Writes the whole file beside the target and renames it over the target, so a crash never
    /// leaves half a file. The file beside it is named for this process, so two Arcwise windows
    /// saving at once never write one file.
    ///
    /// # Errors
    ///
    /// When the folder cannot be created, or the file cannot be written or renamed.
    fn write(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let part = path.with_extension(format!("json.{}.part", process::id()));
        fs::write(&part, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", part.display()))?;
        fs::rename(&part, path).with_context(|| format!("replacing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file the previous Arcwise wrote with every setting changed from its default.
    const OLD_FULL: &str = include_str!("../tests/fixtures/settings-full.json");
    /// The file the previous Arcwise wrote for the default settings.
    const OLD_DEFAULT: &str = include_str!("../tests/fixtures/settings-default.json");
    /// A file as Arcwise 0.2 left it: a library folder and no list, the blur strength from 0 to
    /// 1, no `auto_update`, a speed this version does not know, and a field from a newer Arcwise.
    const OLD_INPUT: &str = include_str!("../tests/fixtures/settings-old-input.json");
    /// What the previous Arcwise wrote after it read [`OLD_INPUT`].
    const OLD_MIGRATED: &str = include_str!("../tests/fixtures/settings-migrated.json");

    /// A folder of its own for each test, under the temporary folder.
    fn dir(name: &str) -> PathBuf {
        env::temp_dir().join(format!("arcwise-settings-{name}-{}", process::id()))
    }

    /// `settings` as [`Settings::write`] writes it.
    fn written(settings: &Settings, name: &str) -> String {
        let dir = dir(name);
        let path = dir.join("settings.json");
        settings.write(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        text
    }

    #[test]
    fn reads_and_writes_the_previous_versions_file_unchanged() {
        let dir = dir("previous");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, OLD_FULL).unwrap();
        let read = Settings::read(&path).unwrap().unwrap();
        fs::remove_dir_all(&dir).unwrap();
        let expected = Settings {
            library: Some(PathBuf::from(r"D:\lineups")),
            libraries: Some(vec![
                LibrarySource {
                    location: "https://library.example.invalid/".into(),
                    on: false,
                    keep: vec![MapId::new("m-ascent")],
                    keep_all: false,
                },
                LibrarySource {
                    location: "https://other.example.invalid/".into(),
                    on: true,
                    keep: Vec::new(),
                    keep_all: true,
                },
                LibrarySource::new(r"D:\lineups".into()),
            ]),
            last: Some(Last {
                map: MapId::new("m-ascent"),
                agent: AgentId::new("a-sova"),
                side: Side::Defense,
            }),
            blur: false,
            blur_strength: 30,
            auto_update: false,
            theme: Mode::Light,
            follow_system_theme: true,
            reopen_last: false,
            start_speed: Speed::Quarter,
            custom_title_bar: false,
            window: Some(Placement {
                x: -1200.,
                y: 40.5,
                width: 1300.,
                height: 820.25,
                maximized: true,
                monitor: Some("a1b2c3d4-0000-0000-0000-000000000002".into()),
            }),
            version: Some("0.6.0".into()),
            shortcuts: BTreeMap::from([
                ("next_agent".into(), vec!["alt-n".into(), "f2".into()]),
                ("restart_clip".into(), vec!["ctrl-r".into()]),
            ]),
            file_unread: false,
        };
        assert_eq!(
            read, expected,
            "every field reads as the previous Arcwise wrote it"
        );
        assert_eq!(
            written(&read, "previous-again"),
            OLD_FULL,
            "written back byte for byte as the previous Arcwise wrote it"
        );
    }

    #[test]
    fn writes_the_defaults_as_the_previous_version_did() {
        assert_eq!(written(&Settings::default(), "defaults"), OLD_DEFAULT);
        assert_eq!(
            serde_json::from_str::<Settings>(OLD_DEFAULT).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn carries_over_an_old_file_as_the_previous_version_did() {
        let read: Settings = serde_json::from_str(OLD_INPUT).unwrap();
        assert_eq!(
            read.blur_strength, 30,
            "0.3 from 0 to 1 is 30 from 0 to 100"
        );
        assert!(
            read.auto_update,
            "updates are on in a file from before them"
        );
        assert_eq!(
            read.start_speed,
            Speed::Full,
            "an unknown speed takes the default"
        );
        assert_eq!(
            read.libraries(),
            [
                LibrarySource::new(remote::DEFAULT.into()),
                LibrarySource::new(r"D:\lineups".into()),
            ],
            "the folder chosen before there was a list follows Arcwise's own library"
        );
        assert_eq!(
            written(&read, "migrated"),
            OLD_MIGRATED,
            "written back as the previous Arcwise wrote it, the newer Arcwise's field dropped"
        );
    }

    #[test]
    fn round_trips_through_a_file() {
        let dir = dir("round-trip");
        let path = dir.join("nested").join("settings.json");
        let settings = Settings {
            library: Some(PathBuf::from(r"D:\lineups")),
            libraries: Some(vec![LibrarySource {
                location: "https://library.arcwise.lol/".into(),
                on: false,
                keep: vec![MapId::new("m-ascent")],
                keep_all: false,
            }]),
            last: Some(Last {
                map: MapId::new("m-ascent"),
                agent: AgentId::new("a-sova"),
                side: Side::Defense,
            }),
            blur: false,
            blur_strength: 30,
            auto_update: false,
            theme: Mode::Light,
            follow_system_theme: true,
            reopen_last: false,
            start_speed: Speed::Quarter,
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
            shortcuts: BTreeMap::from([("restart_clip".into(), vec!["ctrl-r".into()])]),
            file_unread: false,
        };
        settings.write(&path).unwrap();
        assert_eq!(Settings::read(&path).unwrap().unwrap(), settings);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_the_window_keeps_what_another_arcwise_saved() {
        let dir = dir("window");
        let path = dir.join("settings.json");
        let other = Settings {
            theme: Mode::Light,
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
        this.write_over(&path, |saved| saved.window.clone_from(&this.window))
            .unwrap();
        let read = Settings::read(&path).unwrap().unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(read.theme, Mode::Light, "the other's theme is kept");
        assert_eq!(read.window, this.window);
    }

    #[test]
    fn saving_one_setting_leaves_a_file_that_does_not_parse() {
        let dir = dir("broken");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ \"theme\": \"dark\", oops }").unwrap();
        let this = Settings {
            version: Some("0.7.0".into()),
            ..Settings::default()
        };
        let saved = this.write_over(&path, |saved| saved.version.clone_from(&this.version));
        let left = fs::read_to_string(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(saved.is_err());
        assert_eq!(left, "{ \"theme\": \"dark\", oops }");
    }

    #[test]
    fn saving_every_setting_leaves_a_file_that_does_not_parse() {
        let dir = dir("broken-whole");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ \"theme\": \"light\", oops }").unwrap();
        let saved = Settings::default().write_whole(&path);
        let left = fs::read_to_string(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(saved.is_err());
        assert_eq!(left, "{ \"theme\": \"light\", oops }");
    }

    #[test]
    fn saving_every_setting_replaces_a_file_that_parses() {
        let dir = dir("whole");
        let path = dir.join("settings.json");
        Settings::default().write(&path).unwrap();
        let this = Settings {
            theme: Mode::Light,
            ..Settings::default()
        };
        let written = this.write_whole(&path);
        let read = Settings::read(&path);
        fs::remove_dir_all(&dir).unwrap();
        written.unwrap();
        assert_eq!(read.unwrap(), Some(this));
    }

    #[test]
    fn a_file_with_a_byte_order_mark_is_read_and_saved_over() {
        let dir = dir("bom");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &path,
            [BYTE_ORDER_MARK, br#"{"theme": "light", "blur": false}"#].concat(),
        )
        .unwrap();
        let read = Settings::read(&path);
        let this = Settings {
            reopen_last: false,
            ..Settings::default()
        };
        let written = this.write_whole(&path);
        let saved = fs::read(&path);
        fs::remove_dir_all(&dir).unwrap();
        let read = read.unwrap().unwrap();
        assert_eq!(read.theme, Mode::Light);
        assert!(!read.blur, "the rest of the file counts too");
        written.unwrap();
        let saved = saved.unwrap();
        assert!(!saved.starts_with(BYTE_ORDER_MARK), "saved without one");
        assert_eq!(serde_json::from_slice::<Settings>(&saved).unwrap(), this);
    }

    #[test]
    fn a_file_that_holds_nothing_is_read_as_none_and_saved_over() {
        for (name, contents) in [
            ("empty", Vec::new()),
            ("blank", b" \r\n".to_vec()),
            ("nul", vec![0; 64]),
            ("mark", BYTE_ORDER_MARK.to_vec()),
        ] {
            let dir = dir(name);
            let path = dir.join("settings.json");
            fs::create_dir_all(&dir).unwrap();
            fs::write(&path, contents).unwrap();
            let loaded = Settings::load_from(&path);
            let written = Settings::default().write_whole(&path);
            let saved = Settings::read(&path);
            fs::remove_dir_all(&dir).unwrap();
            assert_eq!(
                loaded,
                Settings::default(),
                "{name}: the defaults, not marked unread"
            );
            written.unwrap();
            assert_eq!(saved.unwrap(), Some(Settings::default()), "{name}");
        }
    }

    #[test]
    fn a_file_that_does_not_parse_at_launch_is_marked_unread() {
        let dir = dir("load-broken");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ oops }").unwrap();
        let loaded = Settings::load_from(&path);
        let missing = Settings::load_from(&dir.join("none.json"));
        fs::remove_dir_all(&dir).unwrap();
        assert!(loaded.file_unread);
        assert_eq!(
            Settings {
                file_unread: false,
                ..loaded
            },
            Settings::default()
        );
        assert!(!missing.file_unread, "no file yet is not an unread one");
    }

    #[test]
    fn a_file_that_cannot_be_read_is_an_error_and_left() {
        let dir = dir("unreadable");
        let path = dir.join("settings.json");
        fs::create_dir_all(&path).unwrap();
        let read = Settings::read(&path);
        let loaded = Settings::load_from(&path);
        let written = Settings::default().write_whole(&path);
        let left = path.is_dir();
        fs::remove_dir_all(&dir).unwrap();
        assert!(read.is_err(), "an error, not no file");
        assert!(loaded.file_unread);
        assert!(written.is_err());
        assert!(left);
    }

    #[test]
    fn saving_every_setting_starts_a_file_that_is_not_there() {
        let dir = dir("whole-fresh");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        let read = Settings::read(&path);
        let written = Settings::default().write_whole(&path);
        let saved = Settings::read(&path);
        fs::remove_dir_all(&dir).unwrap();
        assert!(
            read.unwrap().is_none(),
            "no file is no settings, not an error"
        );
        written.unwrap();
        assert_eq!(saved.unwrap(), Some(Settings::default()));
    }

    #[test]
    fn a_file_unread_at_launch_is_left_even_once_fixed() {
        let dir = dir("unread");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        let fixed = r#"{"theme": "light"}"#;
        fs::write(&path, fixed).unwrap();
        let this = Settings {
            file_unread: true,
            ..Settings::default()
        };
        let written = this.write_whole(&path);
        let left = fs::read_to_string(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(written.is_err());
        assert_eq!(left, fixed);
    }

    #[test]
    fn saving_one_setting_starts_a_file_that_is_not_there() {
        let dir = dir("fresh");
        let path = dir.join("settings.json");
        fs::create_dir_all(&dir).unwrap();
        let this = Settings {
            version: Some("0.7.0".into()),
            ..Settings::default()
        };
        this.write_over(&path, |saved| saved.version.clone_from(&this.version))
            .unwrap();
        let read = Settings::read(&path).unwrap().unwrap();
        fs::remove_dir_all(&dir).unwrap();
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
    fn a_speed_it_does_not_know_keeps_the_rest() {
        let settings: Settings =
            serde_json::from_str(r#"{"start_speed": "eighth", "blur": false}"#).unwrap();
        assert_eq!(settings.start_speed, Speed::Full);
        assert!(!settings.blur, "the rest of the file still counts");
    }

    #[test]
    fn following_windows_does_not_undo_a_theme_chosen_in_another_arcwise() {
        let dir = dir("theme");
        let path = dir.join("settings.json");
        let following = Settings {
            theme: Mode::Light,
            follow_system_theme: true,
            ..Settings::default()
        };
        let chosen_elsewhere = Settings {
            theme: Mode::Dark,
            ..Settings::default()
        };
        chosen_elsewhere.write(&path).unwrap();
        following.write_theme(&path).unwrap();
        assert_eq!(
            Settings::read(&path).unwrap().unwrap().theme,
            Mode::Dark,
            "the other Arcwise chose Dark, which stops following Windows"
        );
        following.write(&path).unwrap();
        let dark = Settings {
            theme: Mode::Dark,
            ..following.clone()
        };
        dark.write_theme(&path).unwrap();
        let read = Settings::read(&path).unwrap().unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            read.theme,
            Mode::Dark,
            "while the file still follows Windows, the theme taken from it is saved"
        );
    }

    #[test]
    fn missing_fields_take_defaults() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        assert!(settings.blur, "blur is on until turned off");
        assert_eq!(settings.blur_strength, theme::DEFAULT_STRENGTH);
        assert!(settings.auto_update, "updates are on until turned off");
        assert_eq!(settings.theme, Mode::Dark, "dark until changed");
        assert!(!settings.follow_system_theme);
        assert!(
            settings.reopen_last,
            "opens where it was left until turned off"
        );
        assert_eq!(settings.start_speed, Speed::Full);
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
            theme: Mode::Light,
            ..Settings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["theme"], "light");
        let read: Settings = serde_json::from_str(r#"{"theme": "dark"}"#).unwrap();
        assert_eq!(read.theme, Mode::Dark);
    }
}
