//! Keeping a library on the web on disk to use offline: the maps chosen in the settings, or all of
//! them, fetched into the folder that mirrors it ([`crate::remote`]). Each file resumes where an
//! attempt stopped and is checked against the library's manifest. Reading the libraries again
//! after the library changes fetches only what changed, since [`crate::remote::refresh`] has
//! dropped the copies of changed files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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
    /// Per library on the web: the maps kept on disk as last asked, every map for `None`.
    kept: HashMap<String, Arc<Mutex<Kept>>>,
}

type Kept = Option<Vec<MapId>>;

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
        *lock(&self.kept(location)) = maps.clone();
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
    /// agents and abilities stay, as every map shows them. A download running starts again for
    /// the maps still kept, `keep`, and the files go once its threads have finished the files
    /// they are on, so it does not fetch them again. A map kept again meanwhile keeps its files.
    pub fn remove(
        &mut self,
        location: &str,
        maps: Vec<MapId>,
        keep: Vec<MapId>,
        cx: &mut Context<Self>,
    ) {
        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        let kept = self.kept(location);
        *lock(&kept) = Some(keep.clone());
        let running = self.running(location);
        if running.is_some() {
            if keep.is_empty() {
                self.stop(location, cx);
            } else {
                self.start(location, Some(keep), cx);
            }
        }
        let location = location.to_owned();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            if let Some(job) = running {
                stopped(&job, cx).await;
            }
            cx.background_spawn(async move { delete_unkept(&mirror, &maps, &kept) })
                .await;
            this.update(cx, |this, cx| this.measure(&location, cx)).ok();
        })
        .detach();
    }

    /// Stops the download of a library on the web that has left the list, and deletes the folder
    /// mirroring it unless it is `shared` with another library in the list. A file being fetched
    /// into the folder then is not saved, as the library is gone.
    pub fn forget(&mut self, location: &str, shared: bool, cx: &mut Context<Self>) {
        if !remote::is_url(location) {
            return;
        }
        self.stop(location, cx);
        self.jobs.remove(location);
        self.on_disk.remove(location);
        self.kept.remove(location);
        if shared {
            return;
        }
        let Some(deleting) = remote::forget(location) else {
            return;
        };
        let location = location.to_owned();
        cx.background_spawn(async move {
            if let Err(e) = deleting.finish() {
                eprintln!("arcwise: deleting the copy of {location}: {e}");
            }
        })
        .detach();
    }

    fn kept(&mut self, location: &str) -> Arc<Mutex<Kept>> {
        self.kept.entry(location.to_owned()).or_default().clone()
    }

    fn running(&self, location: &str) -> Option<Arc<Job>> {
        self.jobs
            .get(location)
            .filter(|job| !job.finished.load(Ordering::Relaxed))
            .cloned()
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

/// Once a download asked to stop has.
async fn stopped(job: &Job, cx: &mut AsyncApp) {
    while !job.finished.load(Ordering::Relaxed) {
        cx.background_executor()
            .timer(Duration::from_millis(50))
            .await;
    }
}

fn lock(kept: &Mutex<Kept>) -> std::sync::MutexGuard<'_, Kept> {
    kept.lock().unwrap_or_else(|e| e.into_inner())
}

/// Deletes the clips and pictures of the lineups on `maps`, each while its map is not `kept`.
/// `kept` is held while a file is deleted, so a download for a map kept again meanwhile, which
/// updates `kept` first, finds each file either gone or there to stay.
fn delete_unkept(mirror: &Path, maps: &[MapId], kept: &Mutex<Kept>) {
    let Some(part) = part(mirror) else {
        return;
    };
    for map in maps {
        for rel in part
            .files(Some(std::slice::from_ref(map)))
            .iter()
            .filter(|f| f.starts_with("lineups/"))
        {
            let kept = lock(kept);
            if kept.as_ref().is_none_or(|kept| kept.contains(map)) {
                break;
            }
            let _ = std::fs::remove_file(mirror.join(rel));
        }
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
                    // A file another Arcwise is fetching is not a failure; this one fetches it
                    // next time if it is still missing.
                    if !matches!(fetched, Ok(remote::Fetched::Saved)) {
                        if let Err(e) = fetched {
                            eprintln!("arcwise: {e:#}");
                            job.failed.fetch_add(1, Ordering::Relaxed);
                        }
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

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &[u8] = include_bytes!("../../arcwise-core/tests/fixtures/library.json");

    /// A mirror of the test library with the clip of lineup 1, on Ascent, and of lineup 7, on Bind.
    fn mirror(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("arcwise-downloads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("lineups")).unwrap();
        std::fs::write(dir.join("library.json"), LIBRARY).unwrap();
        for clip in ["lineups/1.mp4", "lineups/7.mp4"] {
            std::fs::write(dir.join(clip), "clip").unwrap();
        }
        dir
    }

    #[test]
    fn the_files_of_a_map_no_longer_kept_are_deleted() {
        let dir = mirror("unkept");
        let ascent = MapId::new("m-ascent");
        let bind = MapId::new("m-bind");
        delete_unkept(&dir, &[ascent], &Mutex::new(Some(vec![bind])));
        assert!(!dir.join("lineups/1.mp4").exists());
        assert!(dir.join("lineups/7.mp4").exists(), "on a map not removed");
        assert!(dir.join("library.json").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_map_kept_again_before_its_files_go_keeps_them() {
        let dir = mirror("kept-again");
        let ascent = MapId::new("m-ascent");
        delete_unkept(
            &dir,
            std::slice::from_ref(&ascent),
            &Mutex::new(Some(vec![ascent.clone()])),
        );
        assert!(dir.join("lineups/1.mp4").exists());
        delete_unkept(&dir, &[ascent], &Mutex::new(None));
        assert!(dir.join("lineups/1.mp4").exists(), "every map kept again");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
