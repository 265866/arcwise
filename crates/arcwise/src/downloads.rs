//! Keeping a library on the web on disk to use offline: the maps chosen in the settings, or all of
//! them, fetched into the folder that mirrors it ([`crate::remote`]). Each file resumes where an
//! attempt stopped and is checked against the library's manifest. Reading the libraries again
//! after the library changes fetches only what changed, since [`crate::remote::refresh`] has
//! dropped the copies of changed files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use arcwise_core::{MapId, Part};
use gpui::{AppContext as _, AsyncApp, Context, WeakEntity};

use crate::remote;

/// How many files download at once.
const AT_ONCE: usize = 4;

/// One library's download, shared with the threads doing it.
#[derive(Default)]
struct Job {
    total: AtomicU64,
    done: AtomicU64,
    failed: AtomicUsize,
    finished: AtomicBool,
    /// Asks the threads to stop after the file each is on.
    cancel: AtomicBool,
}

/// What the settings show of a download.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
    /// Files that could not be fetched; the next download tries them again.
    pub failed: usize,
    pub finished: bool,
}

/// How much of one map's lineups is on disk, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OnDisk {
    pub have: u64,
    pub total: u64,
}

#[derive(Default)]
pub struct Downloads {
    jobs: HashMap<String, Arc<Job>>,
    /// Per library on the web, per map: how much is on disk.
    on_disk: HashMap<String, HashMap<MapId, OnDisk>>,
}

impl Downloads {
    pub fn progress(&self, location: &str) -> Option<Progress> {
        self.jobs.get(location).map(|job| Progress {
            done: job.done.load(Ordering::Relaxed),
            total: job.total.load(Ordering::Relaxed),
            failed: job.failed.load(Ordering::Relaxed),
            finished: job.finished.load(Ordering::Relaxed),
        })
    }

    pub fn on_disk(&self, location: &str) -> Option<&HashMap<MapId, OnDisk>> {
        self.on_disk.get(location)
    }

    /// Fetches what the maps chosen (every map for `None`) need and is not on disk yet. A
    /// download already running for the library stops first.
    pub fn start(&mut self, location: &str, maps: Option<Vec<MapId>>, cx: &mut Context<Self>) {
        if let Some(old) = self.jobs.get(location) {
            old.cancel.store(true, Ordering::Relaxed);
        }
        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        let job = Arc::new(Job::default());
        self.jobs.insert(location.to_owned(), job.clone());
        let location = location.to_owned();
        let work = cx.background_spawn({
            let job = job.clone();
            async move { download(&mirror, maps.as_deref(), &job) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            // Redraws the settings while it runs.
            let ticking = cx.spawn({
                let this = this.clone();
                let job = job.clone();
                async move |cx: &mut AsyncApp| {
                    while !job.finished.load(Ordering::Relaxed) {
                        cx.background_executor()
                            .timer(Duration::from_millis(250))
                            .await;
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }
            });
            work.await;
            drop(ticking);
            this.update(cx, |this, cx| this.measure(&location, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    /// Stops a download; what it fetched stays.
    pub fn stop(&mut self, location: &str, cx: &mut Context<Self>) {
        if let Some(job) = self.jobs.get(location) {
            job.cancel.store(true, Ordering::Relaxed);
        }
        cx.notify();
    }

    /// Deletes the clips and pictures of the lineups on `maps` from disk, to make room. Maps,
    /// agents and abilities stay, as every map shows them.
    pub fn remove(&mut self, location: &str, maps: Vec<MapId>, cx: &mut Context<Self>) {
        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        let location = location.to_owned();
        let work = cx.background_spawn(async move {
            if let Some(part) = part(&mirror) {
                for rel in part
                    .files(Some(&maps))
                    .iter()
                    .filter(|f| f.starts_with("lineups/"))
                {
                    let _ = std::fs::remove_file(mirror.join(rel));
                }
            }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            work.await;
            this.update(cx, |this, cx| this.measure(&location, cx)).ok();
        })
        .detach();
    }

    /// Works out how much of each map of a library on the web is on disk, for the settings.
    pub fn measure(&mut self, location: &str, cx: &mut Context<Self>) {
        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        let location = location.to_owned();
        let work = cx.background_spawn(async move { measure(&mirror) });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let sizes = work.await;
            this.update(cx, |this, cx| {
                this.on_disk.insert(location, sizes);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn part(mirror: &Path) -> Option<Part> {
    let bytes = std::fs::read(mirror.join("library.json")).ok()?;
    Part::parse(mirror, &bytes).ok()
}

fn measure(mirror: &Path) -> HashMap<MapId, OnDisk> {
    let (Some(part), Some(manifest)) = (part(mirror), remote::manifest(mirror)) else {
        return HashMap::new();
    };
    part.maps()
        .into_iter()
        .map(|(map, _)| {
            let mut sizes = OnDisk::default();
            for rel in part.files(Some(std::slice::from_ref(&map.id))) {
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

/// Fetches the files `maps` need that are not on disk, a few at once.
fn download(mirror: &Path, maps: Option<&[MapId]>, job: &Job) {
    let finish = || job.finished.store(true, Ordering::Relaxed);
    let (Some(part), Some(remote)) = (part(mirror), remote::remote(mirror)) else {
        job.failed.store(1, Ordering::Relaxed);
        return finish();
    };
    let manifest = remote::manifest(mirror).unwrap_or_default();
    let wanted: Vec<(String, PathBuf, u64)> = part
        .files(maps)
        .into_iter()
        .filter_map(|rel| {
            let size = manifest.files.get(&rel)?.size;
            let path = mirror.join(&rel);
            (!path.is_file()).then_some((rel, path, size))
        })
        .collect();
    job.total.store(
        wanted.iter().map(|(_, _, size)| size).sum(),
        Ordering::Relaxed,
    );
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..AT_ONCE {
            scope.spawn(|| {
                while !job.cancel.load(Ordering::Relaxed) {
                    let Some((rel, path, size)) = wanted.get(next.fetch_add(1, Ordering::Relaxed))
                    else {
                        break;
                    };
                    let mut got = 0;
                    let fetched = remote::fetch_with(&remote, rel, path, |n| {
                        got += n;
                        job.done.fetch_add(n, Ordering::Relaxed);
                    });
                    if let Err(e) = fetched {
                        eprintln!("arcwise: {e:#}");
                        job.failed.fetch_add(1, Ordering::Relaxed);
                        // Counted as done, so the bar still reaches its end.
                        job.done
                            .fetch_add(size.saturating_sub(got), Ordering::Relaxed);
                    }
                }
            });
        }
    });
    finish();
}
