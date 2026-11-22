//! Keeping a library on the web on disk to use offline: the maps chosen in the settings, or all of
//! them, fetched into the folder that mirrors it ([`crate::remote`]). Each file resumes where an
//! attempt stopped and is checked against the library's manifest. Reading the libraries again
//! after the library changes fetches only what changed, since [`crate::remote::refresh`] has
//! dropped the copies of changed files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::{fs, slice, thread};

use arcwise_core::{MapId, MapLineups, Part};

use crate::remote::{self, Fetched};

/// How many files download at once.
const AT_ONCE: usize = 4;

/// One library's download, shared with the threads doing it.
#[derive(Debug, Default)]
pub struct Job {
    total: AtomicU64,
    done: AtomicU64,
    failed: AtomicUsize,
    finished: AtomicBool,
    /// Asks the threads to stop after the file each is on.
    cancel: AtomicBool,
}

impl Job {
    /// How far the download has got.
    #[must_use]
    pub fn progress(&self) -> Progress {
        Progress {
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            finished: self.is_finished(),
        }
    }

    /// Asks the download to stop after the file each of its threads is on; what it fetched stays.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Whether the download has ended, stopped or not.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
}

/// What the settings show of a download.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Bytes fetched, or counted as done since they failed or another Arcwise fetches them.
    pub done: u64,
    /// Bytes the download has to fetch.
    pub total: u64,
    /// Files that could not be fetched; the next download tries them again.
    pub failed: usize,
    /// Whether the download has ended.
    pub finished: bool,
}

/// How much of one map's lineups is on disk, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OnDisk {
    /// Bytes on disk.
    pub have: u64,
    /// Bytes the map's lineups take in all.
    pub total: u64,
}

/// The maps of one library kept on disk, as last asked: some, or every map.
#[derive(Debug, Default)]
pub struct KeptMaps(Mutex<Option<Vec<MapId>>>);

impl KeptMaps {
    /// Maps kept as `maps` says: those listed, or every map for `None`.
    #[must_use]
    pub fn new(maps: Option<Vec<MapId>>) -> Self {
        KeptMaps(Mutex::new(maps))
    }

    /// Keeps the maps `maps` lists, or every map for `None`, from now on.
    ///
    /// # Panics
    ///
    /// When a thread panicked while holding the list, which a release build, aborting on a
    /// panic, never sees.
    pub fn set(&self, maps: Option<Vec<MapId>>) {
        *self.lock() = maps;
    }

    fn lock(&self) -> MutexGuard<'_, Option<Vec<MapId>>> {
        self.0
            .lock()
            .expect("a thread panicked while holding the maps kept")
    }
}

/// Deletes the clips and pictures of the lineups on `maps`, each while its map is not `kept`.
/// Maps, agents and abilities stay, as every map shows them. `kept` is held while a file is
/// deleted, so a download for a map kept again meanwhile, which updates `kept` first, finds each
/// file either gone or there to stay. A file not there, as most never fetched are, is gone
/// already.
///
/// # Panics
///
/// When a thread panicked while holding `kept`, which a release build, aborting on a panic, never
/// sees.
pub fn delete_unkept(mirror: &Path, maps: &[MapId], kept: &KeptMaps) {
    let Some(part) = part(mirror) else {
        return;
    };
    for map in maps {
        for rel in part.files(Some(slice::from_ref(map))) {
            if !rel.starts_with("lineups/") {
                continue;
            }
            let kept = kept.lock();
            if kept.as_ref().is_none_or(|kept| kept.contains(map)) {
                break;
            }
            let path = mirror.join(rel);
            if let Err(e) = remote::remove_if_there(&path) {
                remote::report(format_args!("deleting {}: {e}", path.display()));
            }
        }
    }
}

/// The library last fetched into `mirror`.
fn part(mirror: &Path) -> Option<Part> {
    let bytes = fs::read(mirror.join("library.json")).ok()?;
    Part::parse(mirror, &bytes).ok()
}

/// How much of each map of the library mirrored in `mirror` is on disk, for the settings. Empty
/// while the library has not been read, or has no manifest to give the files' sizes.
#[must_use]
pub fn measure(mirror: &Path) -> HashMap<MapId, OnDisk> {
    let (Some(part), Some(manifest)) = (part(mirror), remote::manifest(mirror)) else {
        return HashMap::new();
    };
    part.maps()
        .into_iter()
        .map(|MapLineups { map, .. }| {
            let mut sizes = OnDisk::default();
            for rel in part.files(Some(slice::from_ref(&map.id))) {
                let Some(entry) = rel
                    .starts_with("lineups/")
                    .then(|| manifest.files.get(&rel))
                    .flatten()
                else {
                    continue;
                };
                sizes.total += entry.size;
                if mirror.join(&rel).is_file() {
                    sizes.have += entry.size;
                }
            }
            (map.id.clone(), sizes)
        })
        .collect()
}

/// A file a download fetches.
struct Wanted {
    /// Its path in the library.
    rel: String,
    /// Where it goes on disk.
    path: PathBuf,
    /// Its size, as the manifest gives it.
    size: u64,
}

/// Fetches the files the maps chosen (every map for `None`) need that are not on disk, a few at
/// once, telling `job` how far it has got. Without a library read into `mirror`, it fails at once
/// with one file failed. A file another Arcwise is fetching is not a failure; this one fetches it
/// next time if it is still missing. A file not fetched, failed or not, counts as done, so the
/// progress still reaches its end.
pub fn download(mirror: &Path, maps: Option<&[MapId]>, job: &Job) {
    let (Some(part), Some(remote)) = (part(mirror), remote::remote(mirror)) else {
        job.failed.store(1, Ordering::Relaxed);
        job.finished.store(true, Ordering::Relaxed);
        return;
    };
    let manifest = remote::manifest(mirror).unwrap_or_default();
    let wanted: Vec<Wanted> = part
        .files(maps)
        .into_iter()
        .filter_map(|rel| {
            let size = manifest.files.get(&rel)?.size;
            let path = mirror.join(&rel);
            (!path.is_file()).then_some(Wanted { rel, path, size })
        })
        .collect();
    job.total
        .store(wanted.iter().map(|file| file.size).sum(), Ordering::Relaxed);
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..AT_ONCE {
            scope.spawn(|| {
                while !job.cancel.load(Ordering::Relaxed) {
                    let Some(Wanted { rel, path, size }) =
                        wanted.get(next.fetch_add(1, Ordering::Relaxed))
                    else {
                        break;
                    };
                    let mut got = 0;
                    let fetched = remote::fetch_with(&remote, rel, path, |n| {
                        got += n;
                        job.done.fetch_add(n, Ordering::Relaxed);
                    });
                    if !matches!(fetched, Ok(Fetched::Saved)) {
                        if let Err(e) = fetched {
                            remote::report(format_args!("{e:#}"));
                            job.failed.fetch_add(1, Ordering::Relaxed);
                        }
                        job.done
                            .fetch_add(size.saturating_sub(got), Ordering::Relaxed);
                    }
                }
            });
        }
    });
    job.finished.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use std::{env, io, process};

    use super::*;

    const LIBRARY: &[u8] = include_bytes!("../tests/fixtures/library.json");

    /// A mirror of the test library with the clip of lineup 1, on Ascent, and of lineup 7, on Bind.
    fn mirror(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("arcwise-downloads-{name}-{}", process::id()));
        match fs::remove_dir_all(&dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            result => result.unwrap(),
        }
        fs::create_dir_all(dir.join("lineups")).unwrap();
        fs::write(dir.join("library.json"), LIBRARY).unwrap();
        for clip in ["lineups/1.mp4", "lineups/7.mp4"] {
            fs::write(dir.join(clip), "clip").unwrap();
        }
        dir
    }

    #[test]
    fn the_files_of_a_map_no_longer_kept_are_deleted() {
        let dir = mirror("unkept");
        let ascent = MapId::new("m-ascent");
        let bind = MapId::new("m-bind");
        delete_unkept(&dir, &[ascent], &KeptMaps::new(Some(vec![bind])));
        assert!(!dir.join("lineups/1.mp4").exists());
        assert!(dir.join("lineups/7.mp4").exists(), "on a map not removed");
        assert!(dir.join("library.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_map_kept_again_before_its_files_go_keeps_them() {
        let dir = mirror("kept-again");
        let ascent = MapId::new("m-ascent");
        delete_unkept(
            &dir,
            slice::from_ref(&ascent),
            &KeptMaps::new(Some(vec![ascent.clone()])),
        );
        assert!(dir.join("lineups/1.mp4").exists());
        delete_unkept(&dir, &[ascent], &KeptMaps::new(None));
        assert!(dir.join("lineups/1.mp4").exists(), "every map kept again");
        fs::remove_dir_all(&dir).unwrap();
    }
}
