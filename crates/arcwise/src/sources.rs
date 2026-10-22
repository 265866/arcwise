//! Reading the libraries in the settings' list: a folder as it is, a web address through the
//! folder that mirrors it ([`crate::remote`]).

use std::path::PathBuf;

use arcwise_core::{LoadError, MapId, Part};

use crate::remote;

/// How one library in the list fared the last time the libraries were read.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Turned off in the list.
    Off,
    Loaded {
        /// What the library calls itself, when it says.
        name: Option<String>,
        lineups: usize,
        /// Its lineups not shown, because a library above it in the list has the same ids.
        hidden: usize,
        /// The maps with lineups, with how many each has.
        maps: Vec<(MapId, String, usize)>,
        /// For a library on the web, the copy fetched last time, since it could not be reached.
        stale: bool,
    },
    Failed(String),
}

/// A library from the list and how reading it went.
#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub location: String,
    pub state: State,
}

impl Status {
    /// What to call the library: its own name, else its folder's name or its web address's host.
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
pub fn folder(location: &str) -> Option<PathBuf> {
    if remote::is_url(location) {
        remote::mirror(location)
    } else {
        Some(PathBuf::from(location))
    }
}

/// Reads one library. From the web, this fetches its `library.json` first, falling back on the
/// copy from last time.
pub fn read(location: &str) -> (Result<Part, String>, bool) {
    let Some(root) = folder(location) else {
        return (Err("there is no folder to keep it in".into()), false);
    };
    let mut stale = false;
    if remote::is_url(location) {
        match remote::refresh(location, &root) {
            Ok(fresh) => stale = !fresh,
            Err(e) => return (Err(problem(&e)), false),
        }
    }
    let path = root.join("library.json");
    let part = std::fs::read(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))
        .and_then(|bytes| {
            Part::parse(root, &bytes).map_err(|e| format!("{:#}", anyhow::Error::from(e)))
        });
    (part, stale)
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
pub fn loaded(part: &Part, stale: bool) -> State {
    State::Loaded {
        name: part.name().map(str::to_owned),
        lineups: part.lineup_count(),
        hidden: 0,
        maps: part
            .maps()
            .into_iter()
            .map(|(map, n)| (map.id.clone(), map.name.clone(), n))
            .collect(),
        stale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_name_the_host_or_folder() {
        assert_eq!(title("https://library.arcwise.lol/"), "library.arcwise.lol");
        assert_eq!(title(r"D:\snapline\library"), "library");
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
        let e = anyhow::Error::new(std::io::Error::other(
            "io: No such host is known. (os error 11001)",
        ))
        .context("fetching https://x.y/library.json");
        assert_eq!(reason(&e), "No such host is known");
        assert_eq!(problem(&e), "could not reach it: No such host is known");
        let html = Part::parse("https://x.y/", b"<html>").err().unwrap();
        assert!(
            problem(&anyhow::Error::from(html))
                .starts_with("https://x.y/library.json is not a lineup library: "),
        );
    }

    #[test]
    fn a_folder_reads_as_it_is() {
        let dir = std::env::temp_dir().join(format!("arcwise-sources-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("library.json"),
            include_bytes!("../../arcwise-core/tests/fixtures/library.json"),
        )
        .unwrap();
        let (part, stale) = read(&dir.to_string_lossy());
        let part = part.unwrap();
        assert!(!stale);
        assert_eq!(part.lineup_count(), 9);
        let State::Loaded { maps, .. } = loaded(&part, false) else {
            panic!()
        };
        assert!(maps.iter().any(|(_, name, n)| name == "Ascent" && *n == 7));
        let (missing, _) = read(&dir.join("nothing").to_string_lossy());
        let Err(problem) = missing else {
            panic!("read a library that is not there")
        };
        assert!(problem.starts_with("cannot read"), "{problem}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
