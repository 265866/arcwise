//! Reading the libraries in the settings' list: a folder as it is, a web address through the
//! folder that mirrors it ([`crate::remote`]).

use std::fs;
use std::path::PathBuf;

use arcwise_core::{LoadError, MapId, Part};

use crate::remote;

/// How one library in the list fared the last time the libraries were read.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Turned off in the list.
    Off,
    /// Read.
    Loaded {
        /// What the library calls itself, when it says.
        name: Option<String>,
        /// How many lineups it lists.
        lineups: usize,
        /// Its lineups not shown, because a library above it in the list has the same ids.
        hidden: usize,
        /// The maps with lineups, with how many each has.
        maps: Vec<MapCount>,
        /// For a library on the web, the copy fetched last time, since it could not be reached.
        stale: bool,
    },
    /// Not read, and why.
    Failed(String),
}

/// A map a library has lineups on, and how many.
#[derive(Clone, Debug, PartialEq)]
pub struct MapCount {
    /// The map's id.
    pub id: MapId,
    /// The map's name.
    pub name: String,
    /// How many lineups the library has on it.
    pub lineups: usize,
}

/// A library from the list and how reading it went.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    /// Where the library is: a folder or a web address.
    pub location: String,
    /// How reading it went.
    pub state: State,
}

impl Status {
    /// What to call the library: its own name, else its folder's name or its web address's host.
    #[must_use]
    pub fn title(&self) -> String {
        if let State::Loaded {
            name: Some(name), ..
        } = &self.state
        {
            return name.clone();
        }
        title(&self.location)
    }
}

/// A short name for a library location.
#[must_use]
pub fn title(location: &str) -> String {
    if remote::is_url(location) {
        let rest = location.split_once("://").map_or(location, |(_, r)| r);
        return rest.trim_end_matches('/').to_owned();
    }
    PathBuf::from(location)
        .file_name()
        .map_or_else(|| location.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// The folder a library's files are read from: its own, or the one mirroring it from the web.
#[must_use]
pub fn folder(location: &str) -> Option<PathBuf> {
    if remote::is_url(location) {
        remote::mirror(location)
    } else {
        Some(PathBuf::from(location))
    }
}

/// What reading one library gave.
#[derive(Debug)]
pub struct ReadOutcome {
    /// The library, or why it could not be read.
    pub part: Result<Part, String>,
    /// For a library on the web, whether it is the copy fetched last time, since it could not be
    /// reached.
    pub stale: bool,
}

/// Reads one library. From the web, this fetches its `library.json` first, falling back on the
/// copy from last time.
#[must_use]
pub fn read(location: &str) -> ReadOutcome {
    let Some(root) = folder(location) else {
        return ReadOutcome {
            part: Err("there is no folder to keep it in".into()),
            stale: false,
        };
    };
    let mut stale = false;
    if remote::is_url(location) {
        match remote::refresh(location, &root) {
            Ok(fresh) => stale = !fresh,
            Err(e) => {
                return ReadOutcome {
                    part: Err(problem(&e)),
                    stale: false,
                };
            }
        }
    }
    let path = root.join("library.json");
    let part = fs::read(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
        .and_then(|bytes| {
            Part::parse(&root, &bytes).map_err(|e| format!("{:#}", anyhow::Error::from(e)))
        });
    ReadOutcome { part, stale }
}

/// Why a library on the web could not be read: what it sent was not a library, or it could not be
/// reached.
fn problem(e: &anyhow::Error) -> String {
    if e.chain()
        .any(|c| c.is::<LoadError>() || c.is::<serde_json::Error>())
    {
        format!("{e:#}")
    } else {
        format!("could not reach it: {}", reason(e))
    }
}

/// The innermost cause of an error, as a person would say it: without the error number Windows adds.
fn reason(e: &anyhow::Error) -> String {
    let cause = e.root_cause().to_string();
    let cause = cause.strip_prefix("io: ").unwrap_or(&cause);
    let cause = match cause.find(" (os error") {
        Some(at) => &cause[..at],
        None => cause,
    };
    cause.trim_end_matches('.').to_owned()
}

/// What reading a library says about it, for the settings; how many of its lineups are hidden is
/// known once the libraries are merged.
#[must_use]
pub fn loaded(part: &Part, stale: bool) -> State {
    State::Loaded {
        name: part.name().map(str::to_owned),
        lineups: part.lineup_count(),
        hidden: 0,
        maps: part
            .maps()
            .into_iter()
            .map(|counted| MapCount {
                id: counted.map.id.clone(),
                name: counted.map.name.clone(),
                lineups: counted.lineup_count,
            })
            .collect(),
        stale,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::{env, io, process};

    use super::*;

    #[test]
    fn titles_name_the_host_or_folder() {
        assert_eq!(title("https://library.arcwise.lol/"), "library.arcwise.lol");
        assert_eq!(title(r"D:\lineups\library"), "library");
        let status = Status {
            location: "https://x.y/".into(),
            state: State::Loaded {
                name: Some("Mine".into()),
                lineups: 1,
                hidden: 0,
                maps: vec![],
                stale: false,
            },
        };
        assert_eq!(status.title(), "Mine");
    }

    #[test]
    fn reasons_leave_out_windows_error_numbers() {
        let e = anyhow::Error::new(io::Error::other(
            "io: No such host is known. (os error 11001)",
        ))
        .context("fetching https://x.y/library.json");
        assert_eq!(reason(&e), "No such host is known");
        assert_eq!(problem(&e), "could not reach it: No such host is known");
        let html = Part::parse(Path::new("https://x.y/"), b"<html>")
            .err()
            .unwrap();
        assert!(
            problem(&anyhow::Error::from(html))
                .starts_with("https://x.y/library.json is not a lineup library: "),
        );
    }

    #[test]
    fn a_folder_reads_as_it_is() {
        let dir = env::temp_dir().join(format!("arcwise-sources-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("library.json"),
            include_bytes!("../tests/fixtures/library.json"),
        )
        .unwrap();
        let ReadOutcome { part, stale } = read(&dir.to_string_lossy());
        let part = part.unwrap();
        assert!(!stale);
        assert_eq!(part.lineup_count(), 9);
        let State::Loaded { maps, .. } = loaded(&part, stale) else {
            panic!()
        };
        assert!(
            maps.iter()
                .any(|map| map.name == "Ascent" && map.lineups == 7)
        );
        let ReadOutcome { part: missing, .. } = read(&dir.join("nothing").to_string_lossy());
        let Err(problem) = missing else {
            panic!("read a library that is not there")
        };
        assert!(problem.starts_with("cannot read"), "{problem}");
        fs::remove_dir_all(&dir).unwrap();
    }
}
