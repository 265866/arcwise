//! Keeping a library on the web on disk to use offline, as the settings ask: which downloads run,
//! how far each has got, and how much of each map is on disk. The files themselves are fetched,
//! measured and deleted by [`arcwise_remote::downloads`], off the UI thread.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arcwise_core::MapId;
use arcwise_remote::downloads::{self, Job, KeptMaps, OnDisk, Progress};
use arcwise_remote::remote;
use gpui::{AsyncApp, Context, WeakEntity};

/// How often the settings redraw while a download runs.
const REDRAW_EVERY: Duration = Duration::from_millis(250);
/// How often a download asked to stop is checked on until it has.
const STOPPED_EVERY: Duration = Duration::from_millis(50);

/// Every library's download, and what is on disk, by the library's web address.
#[derive(Debug, Default)]
pub(crate) struct Downloads {
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the threads fetching the files update the job while the settings read its \
                      progress, and either side can finish first"
        )
    )]
    jobs: HashMap<String, Arc<Job>>,
    /// Per library on the web, per map: how much is on disk.
    on_disk: HashMap<String, HashMap<MapId, OnDisk>>,
    /// Per library on the web: the maps kept on disk as last asked.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "a deletion running off the UI thread reads the maps kept while the settings \
                      change them, and outlives a library forgotten meanwhile"
        )
    )]
    kept: HashMap<String, Arc<KeptMaps>>,
}

impl Downloads {
    /// How far the library's download has got, if one has run since Arcwise opened.
    #[must_use]
    pub(crate) fn progress(&self, location: &str) -> Option<Progress> {
        self.jobs.get(location).map(|job| job.progress())
    }

    /// How much of each map of the library is on disk, once measured.
    #[must_use]
    pub(crate) fn on_disk(&self, location: &str) -> Option<&HashMap<MapId, OnDisk>> {
        self.on_disk.get(location)
    }

    /// Fetches what the maps chosen (every map for `None`) need and is not on disk yet. A
    /// download already running for the library stops first.
    pub(crate) fn start(
        &mut self,
        location: &str,
        maps: Option<Vec<MapId>>,
        cx: &mut Context<'_, Self>,
    ) {
        use gpui::AppContext as _;

        if let Some(old) = self.jobs.get(location) {
            old.cancel();
        }
        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        self.kept(location).set(maps.clone());
        let job = Arc::new(Job::default());
        self.jobs.insert(location.to_owned(), job.clone());
        let location = location.to_owned();
        let work = cx.background_spawn({
            let job = job.clone();
            async move { downloads::download(&mirror, maps.as_deref(), &job) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let redrawing = cx.spawn({
                let this = this.clone();
                let job = job.clone();
                async move |cx: &mut AsyncApp| {
                    while !job.is_finished() {
                        cx.background_executor().timer(REDRAW_EVERY).await;
                        let Some(this) = this.upgrade() else { break };
                        this.update(cx, |_, cx| cx.notify());
                    }
                }
            });
            work.await;
            drop(redrawing);
            if let Some(this) = this.upgrade() {
                this.update(cx, |_, cx| Downloads::measure(&location, cx));
            }
        })
        .detach();
        cx.notify();
    }

    /// Stops a download; what it fetched stays.
    pub(crate) fn stop(&mut self, location: &str, cx: &mut Context<'_, Self>) {
        if let Some(job) = self.jobs.get(location) {
            job.cancel();
        }
        cx.notify();
    }

    /// Deletes the clips and pictures of the lineups on `maps` from disk, to make room. Maps,
    /// agents and abilities stay, as every map shows them. A download running starts again for
    /// the maps still kept, `keep`, and the files go once its threads have finished the files
    /// they are on, so it does not fetch them again. A map kept again meanwhile keeps its files.
    pub(crate) fn remove(
        &mut self,
        location: &str,
        maps: Vec<MapId>,
        keep: Vec<MapId>,
        cx: &mut Context<'_, Self>,
    ) {
        use gpui::AppContext as _;

        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        let kept = self.kept(location);
        kept.set(Some(keep.clone()));
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
            cx.background_spawn(async move { downloads::delete_unkept(&mirror, &maps, &kept) })
                .await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |_, cx| Downloads::measure(&location, cx));
            }
        })
        .detach();
    }

    /// Stops the download of a library on the web that has left the list, and deletes the folder
    /// mirroring it unless it is `shared` with another library in the list. A file being fetched
    /// into the folder then is not saved, as the library is gone.
    pub(crate) fn forget(&mut self, location: &str, shared: bool, cx: &mut Context<'_, Self>) {
        use gpui::AppContext as _;

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
                remote::report(format_args!("deleting the copy of {location}: {e}"));
            }
        })
        .detach();
    }

    /// The maps of the library kept on disk, shared with the work that downloads and deletes.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "a deletion running off the UI thread reads the maps kept while the settings \
                      change them, and outlives a library forgotten meanwhile"
        )
    )]
    fn kept(&mut self, location: &str) -> Arc<KeptMaps> {
        self.kept.entry(location.to_owned()).or_default().clone()
    }

    /// The library's download, while it runs.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the threads fetching the files update the job while the caller waits for \
                      them to stop"
        )
    )]
    fn running(&self, location: &str) -> Option<Arc<Job>> {
        self.jobs
            .get(location)
            .filter(|job| !job.is_finished())
            .cloned()
    }

    /// Works out how much of each map of a library on the web is on disk, for the settings.
    pub(crate) fn measure(location: &str, cx: &mut Context<'_, Self>) {
        use gpui::AppContext as _;

        let Some(mirror) = remote::mirror(location) else {
            return;
        };
        let location = location.to_owned();
        let work = cx.background_spawn(async move { downloads::measure(&mirror) });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let sizes = work.await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    this.on_disk.insert(location, sizes);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

/// Once a download asked to stop has.
async fn stopped(job: &Job, cx: &mut AsyncApp) {
    while !job.is_finished() {
        cx.background_executor().timer(STOPPED_EVERY).await;
    }
}
