//! What Arcwise knows about updates, shared by the window, which shows toasts, and the settings
//! window, which shows the version and an install button. Arcwise looks for a newer version when
//! it opens, every [`CHECK_EVERY`] while it stays open, and when asked to. With automatic updates
//! on, a newer version downloads in the background and installs when Arcwise closes, without a
//! toast. With them off, a toast says it exists when Arcwise opens, and the settings window offers
//! to install it. Either way the settings gear wears a badge until it is installed.
//!
//! [`arcwise_update`] does the asking, downloading and checking; this decides when.

use std::mem;
use std::time::{Duration, Instant};

use arcwise_remote::remote;
use arcwise_update::{Check, Outcome, Relaunch, Update};
use gpui::{AsyncApp, Context, WeakEntity};
use semver::Version;

/// How long a toast about a version shows before it is gone, fade included.
pub(crate) const TOAST_LIFETIME: Duration = Duration::from_secs(6);
/// How often an Arcwise left open looks for a newer version again.
const CHECK_EVERY: Duration = Duration::from_hours(1);

/// When to look for, download and install a newer version, and what to tell the user.
#[derive(Debug)]
pub(crate) struct Updater {
    /// Whether updates download and install without asking.
    automatic: bool,
    state: State,
    toast: Option<Toast>,
    /// Toasts shown so far, so a fading toast's timer leaves a newer toast alone.
    toasts: u64,
    /// A version whose download could not be verified, not tried again until a newer one is out.
    refused: Option<Version>,
    /// Whether GitHub is being asked now.
    checking: bool,
    /// When GitHub last answered, or failed to.
    checked: Option<Checked>,
}

/// The last time Arcwise looked for a newer version.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Checked {
    pub(crate) at: Instant,
    /// GitHub did not answer, as when offline.
    pub(crate) failed: bool,
}

/// Where the newest version known stands.
#[derive(Debug)]
pub(crate) enum State {
    UpToDate,
    /// A newer version, not downloaded.
    Available(Version),
    Downloading {
        version: Version,
        /// What happens once it is verified.
        then: Then,
    },
    /// Downloaded and verified. It installs when Arcwise closes.
    Ready(Update),
}

/// What happens to a newer version once its download is verified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Then {
    /// It waits: to install when Arcwise closes with automatic updates on, or for someone to
    /// install it with them off.
    Keep,
    /// It installs and reopens Arcwise at once, because someone asked.
    Install,
}

/// A note about a version, shown over the window.
#[derive(Debug)]
pub(crate) enum Toast {
    /// Found by the check when Arcwise opens, with automatic updates off. Shown for
    /// [`TOAST_LIFETIME`].
    Available(Version),
    /// This version, which replaced an older one since Arcwise last ran. Shown for
    /// [`TOAST_LIFETIME`].
    Updated(Version),
    /// A version that did not install, and why. Shown until dismissed.
    Failed { version: Version, why: &'static str },
}

impl Updater {
    /// With `updated`, says this version has replaced an older one. Closing the window releases
    /// everything that holds this before GPUI quits, too late for an app-quit handler, so a ready
    /// update installs when this is released.
    pub(crate) fn new(
        automatic: bool,
        updated: Option<Version>,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        cx.on_release(|this, _| this.install_on_close()).detach();
        let mut this = Updater {
            automatic,
            state: State::UpToDate,
            toast: None,
            toasts: 0,
            refused: None,
            checking: false,
            checked: None,
        };
        if let Some(version) = updated {
            this.show(Toast::Updated(version), cx);
        }
        if arcwise_update::installed() {
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    let Some(live) = this.upgrade() else { break };
                    live.update(cx, Updater::check);
                    drop(live);
                    cx.background_executor().timer(CHECK_EVERY).await;
                }
            })
            .detach();
        }
        this
    }

    /// Whether asking GitHub now would do anything: not for a copy that does not update itself,
    /// while it is being asked, or while a newer version is downloading or downloaded.
    #[must_use]
    pub(crate) fn can_check(&self) -> bool {
        arcwise_update::installed() && !self.checking && self.checks()
    }

    /// Asks GitHub for a newer version now, when [`Updater::can_check`]. Installers that
    /// should have been deleted but could not be are reported.
    pub(crate) fn check(&mut self, cx: &mut Context<'_, Self>) {
        use gpui::AppContext as _;

        let Some(dir) = arcwise_update::directory() else {
            return;
        };
        if !self.can_check() {
            return;
        }
        self.checking = true;
        cx.notify();
        let current = arcwise_update::current_version();
        let task = cx.background_spawn(async move {
            arcwise_update::latest(&current, &dir).map(|Check { newer, kept }| {
                for file in kept {
                    remote::report(format_args!(
                        "could not delete {}: {}",
                        file.path.display(),
                        file.error
                    ));
                }
                newer
            })
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let latest = task.await;
            let Some(this) = this.upgrade() else { return };
            this.update(cx, |this, cx| {
                this.checking = false;
                let opening = this.checked.is_none();
                this.checked = Some(Checked {
                    at: Instant::now(),
                    failed: latest.is_err(),
                });
                this.found(latest, opening, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Whether GitHub is being asked now.
    #[must_use]
    pub(crate) fn checking(&self) -> bool {
        self.checking
    }

    /// When GitHub last answered, or failed to.
    #[must_use]
    pub(crate) fn checked(&self) -> Option<Checked> {
        self.checked
    }

    /// Whether to look for a newer version: not while one is downloading or downloaded, which
    /// the next launch looks past.
    fn checks(&self) -> bool {
        matches!(self.state, State::UpToDate | State::Available(_))
    }

    /// Whether updates download and install without asking.
    #[must_use]
    pub(crate) fn automatic(&self) -> bool {
        self.automatic
    }

    /// Where the newest version known stands.
    #[must_use]
    pub(crate) fn state(&self) -> &State {
        &self.state
    }

    /// The toast showing, if any.
    #[must_use]
    pub(crate) fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }

    /// Whether a newer version waits: with automatic updates off, for someone to install it from
    /// the settings window, and with them on, downloaded, for Arcwise to close.
    #[must_use]
    pub(crate) fn waiting(&self) -> bool {
        match self.state {
            State::UpToDate => false,
            State::Ready(_) => true,
            State::Available(_) | State::Downloading { .. } => !self.automatic,
        }
    }

    /// With `opening`, the check Arcwise makes when it opens, the only one that shows a toast.
    /// A check that failed, as when offline, is reported and asked again at the next check. Someone
    /// may have asked to install while the check waited on GitHub, which this must not undo. With
    /// automatic updates on, a download that failed is tried again at the next check.
    fn found(
        &mut self,
        latest: anyhow::Result<Option<Version>>,
        opening: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let version = match latest {
            Ok(Some(version)) => version,
            Ok(None) => return,
            Err(e) => {
                remote::report(format_args!("could not check for updates: {e:#}"));
                return;
            }
        };
        if !self.checks()
            || self
                .refused
                .as_ref()
                .is_some_and(|refused| *refused >= version)
        {
            return;
        }
        if self.automatic {
            self.download(version, Then::Keep, cx);
        } else if !matches!(&self.state, State::Available(known) if *known >= version) {
            if opening {
                self.show(Toast::Available(version.clone()), cx);
            }
            self.state = State::Available(version);
        }
        cx.notify();
    }

    /// Turns automatic updates on or off. Turning them on downloads a known newer version;
    /// turning them off drops one downloaded but not yet installed.
    pub(crate) fn set_automatic(&mut self, on: bool, cx: &mut Context<'_, Self>) {
        self.automatic = on;
        match mem::replace(&mut self.state, State::UpToDate) {
            State::Available(version) if on => self.download(version, Then::Keep, cx),
            State::Ready(update) if !on => {
                self.state = State::Available(update.version.clone());
                discard(update);
            }
            state => self.state = state,
        }
        cx.notify();
    }

    /// Installs the newer version now and reopens Arcwise, downloading it first if needed.
    pub(crate) fn install_now(&mut self, cx: &mut Context<'_, Self>) {
        match mem::replace(&mut self.state, State::UpToDate) {
            State::Available(version) => self.download(version, Then::Install, cx),
            State::Downloading { version, .. } => {
                self.state = State::Downloading {
                    version,
                    then: Then::Install,
                };
            }
            State::Ready(update) => self.install(update, cx),
            State::UpToDate => {}
        }
        cx.notify();
    }

    /// Hides the toast showing.
    pub(crate) fn dismiss_toast(&mut self, cx: &mut Context<'_, Self>) {
        self.toast = None;
        cx.notify();
    }

    /// Downloads `version` in the background, and does `then` once it is verified.
    fn download(&mut self, version: Version, then: Then, cx: &mut Context<'_, Self>) {
        use gpui::AppContext as _;

        let Some(dir) = arcwise_update::directory() else {
            return;
        };
        self.state = State::Downloading {
            version: version.clone(),
            then,
        };
        let fetch = version.clone();
        let task = cx.background_spawn(async move { arcwise_update::fetch(&fetch, &dir) });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let outcome = task.await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.fetched(version, outcome, cx));
            }
        })
        .detach();
    }

    /// Takes in a finished download. One whose automatic updates were turned off while it
    /// downloaded is dropped. A background download that failed tries again at the next check;
    /// one someone asked for says so.
    fn fetched(
        &mut self,
        version: Version,
        outcome: anyhow::Result<Outcome>,
        cx: &mut Context<'_, Self>,
    ) {
        let install = matches!(
            self.state,
            State::Downloading {
                then: Then::Install,
                ..
            }
        );
        match outcome {
            Ok(Outcome::Ready(update)) if install => self.install(update, cx),
            Ok(Outcome::Ready(update)) if self.automatic => self.state = State::Ready(update),
            Ok(Outcome::Ready(update)) => {
                discard(update);
                self.state = State::Available(version);
            }
            Ok(Outcome::Unverified { reason }) => {
                remote::report(format_args!("did not install {version}: {reason}"));
                self.refused = Some(version.clone());
                self.fail(version, "Its download could not be verified.");
            }
            Err(e) => {
                remote::report(format_args!("could not download {version}: {e:#}"));
                if install {
                    self.fail(version, "It could not be downloaded.");
                } else {
                    self.state = State::Available(version);
                }
            }
        }
        cx.notify();
    }

    /// Runs the installer, which reopens Arcwise, and quits.
    fn install(&mut self, update: Update, cx: &mut Context<'_, Self>) {
        match arcwise_update::install(&update, Relaunch::Yes) {
            Ok(()) => cx.quit(),
            Err(e) => {
                remote::report(format_args!("could not start the installer: {e}"));
                let version = update.version.clone();
                discard(update);
                self.fail(version, "Its installer would not start.");
            }
        }
    }

    /// Shows that `version` did not install, and why, until dismissed.
    fn fail(&mut self, version: Version, why: &'static str) {
        self.toasts += 1;
        self.toast = Some(Toast::Failed {
            version: version.clone(),
            why,
        });
        self.state = State::Available(version);
    }

    /// Shows a toast that is gone after [`TOAST_LIFETIME`], unless another has replaced it.
    fn show(&mut self, toast: Toast, cx: &mut Context<'_, Self>) {
        self.toast = Some(toast);
        self.toasts += 1;
        let shown = self.toasts;
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            cx.background_executor().timer(TOAST_LIFETIME).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| {
                    if this.toasts == shown {
                        this.dismiss_toast(cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Installs a ready update, without reopening Arcwise, as it closes.
    fn install_on_close(&mut self) {
        if let State::Ready(update) = mem::replace(&mut self.state, State::UpToDate)
            && let Err(e) = arcwise_update::install(&update, Relaunch::No)
        {
            remote::report(format_args!("could not start the installer: {e}"));
        }
    }
}

/// Deletes the installer of an update that will not be installed, and reports a failure to. A
/// file left over goes at a later check, which clears old installers.
fn discard(update: Update) {
    let installer = update.installer.clone();
    if let Err(e) = arcwise_update::discard(update) {
        remote::report(format_args!(
            "could not delete {}: {e}",
            installer.display()
        ));
    }
}
