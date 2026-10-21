//! What Arcwise knows about updates, shared by the window, which shows toasts, and the settings
//! menu, which shows the version and an install button. Arcwise looks for a newer version when it
//! opens and every [`CHECK_EVERY`] while it stays open. With automatic updates on, a newer version
//! downloads in the background, a toast says it is ready, and it installs when Arcwise closes.
//! With them off, a toast says it exists and the settings menu offers to install it. Either way
//! the settings gear wears a badge until it is installed.

use std::time::Duration;

use gpui::{AsyncApp, Context, WeakEntity, prelude::*};
use semver::Version;

use crate::update::{self, Outcome, Update};

/// How long a toast about a version shows before it is gone, fade included.
pub const TOAST_LIFETIME: Duration = Duration::from_secs(6);
/// How often an Arcwise left open looks for a newer version again.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

pub struct Updater {
    /// Whether updates download and install without asking.
    automatic: bool,
    state: State,
    toast: Option<Toast>,
    /// Toasts shown so far, so a fading toast's timer leaves a newer toast alone.
    toasts: u64,
    /// A version whose download could not be verified, not tried again until a newer one is out.
    refused: Option<Version>,
}

pub enum State {
    UpToDate,
    /// A newer version, not downloaded.
    Available(Version),
    Downloading {
        version: Version,
        /// Whether to install and reopen Arcwise as soon as it is verified, because someone asked.
        install: bool,
    },
    /// Downloaded and verified. It installs when Arcwise closes.
    Ready(Update),
}

pub enum Toast {
    /// Shown for [`TOAST_LIFETIME`] when automatic updates are off.
    Available(Version),
    /// Downloaded with automatic updates on, to install when Arcwise closes. Shown for
    /// [`TOAST_LIFETIME`].
    Ready(Version),
    /// This version, which replaced an older one since Arcwise last ran. Shown for
    /// [`TOAST_LIFETIME`].
    Updated(Version),
    /// A version that did not install, and why. Shown until dismissed.
    Failed { version: Version, why: &'static str },
}

impl Updater {
    /// With `updated`, says this version has replaced an older one.
    pub fn new(automatic: bool, updated: Option<Version>, cx: &mut Context<Self>) -> Self {
        // Closing the window releases everything that holds this before GPUI quits, too late for
        // an app-quit handler, so a ready update installs on release.
        cx.on_release(|this, _| this.install_on_close()).detach();
        let mut this = Updater {
            automatic,
            state: State::UpToDate,
            toast: None,
            toasts: 0,
            refused: None,
        };
        if let Some(version) = updated {
            this.show(Toast::Updated(version), cx);
        }
        if update::installed()
            && let Some(dir) = update::directory()
        {
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    let Ok(check) = this.read_with(cx, |this, _| this.checks()) else {
                        break;
                    };
                    if check {
                        let (current, dir) = (update::current_version(), dir.clone());
                        let latest = cx
                            .background_spawn(async move { update::latest(&current, &dir) })
                            .await;
                        if this.update(cx, |this, cx| this.found(latest, cx)).is_err() {
                            break;
                        }
                    }
                    cx.background_executor().timer(CHECK_EVERY).await;
                }
            })
            .detach();
        }
        this
    }

    /// Whether to look for a newer version: not while one is downloading or downloaded, which
    /// the next launch looks past.
    fn checks(&self) -> bool {
        matches!(self.state, State::UpToDate | State::Available(_))
    }

    pub fn automatic(&self) -> bool {
        self.automatic
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn toast(&self) -> Option<&Toast> {
        self.toast.as_ref()
    }

    /// Whether a newer version waits: with automatic updates off, for someone to install it from
    /// the settings window, and with them on, downloaded, for Arcwise to close.
    pub fn waiting(&self) -> bool {
        match self.state {
            State::UpToDate => false,
            State::Ready(_) => true,
            State::Available(_) | State::Downloading { .. } => !self.automatic,
        }
    }

    fn found(&mut self, latest: anyhow::Result<Option<Version>>, cx: &mut Context<Self>) {
        let version = match latest {
            Ok(Some(version)) => version,
            Ok(None) => return,
            // Offline, or GitHub did not answer; the next check asks again.
            Err(e) => {
                eprintln!("arcwise: could not check for updates: {e:#}");
                return;
            }
        };
        // Someone may have asked to install while the check waited on GitHub, which this must
        // not undo.
        if !self.checks()
            || self
                .refused
                .as_ref()
                .is_some_and(|refused| *refused >= version)
        {
            return;
        }
        if self.automatic {
            // Again an hour after a download that failed.
            self.download(version, false, cx);
        } else if !matches!(&self.state, State::Available(known) if *known >= version) {
            self.show(Toast::Available(version.clone()), cx);
            self.state = State::Available(version);
        }
        cx.notify();
    }

    /// Turns automatic updates on or off. Turning them on downloads a known newer version;
    /// turning them off drops one downloaded but not yet installed.
    pub fn set_automatic(&mut self, on: bool, cx: &mut Context<Self>) {
        self.automatic = on;
        match std::mem::replace(&mut self.state, State::UpToDate) {
            State::Available(version) if on => self.download(version, false, cx),
            State::Ready(update) if !on => {
                self.state = State::Available(update.version.clone());
                update::discard(update);
                // It no longer installs on close.
                if matches!(self.toast, Some(Toast::Ready(_))) {
                    self.toast = None;
                }
            }
            state => self.state = state,
        }
        cx.notify();
    }

    /// Installs the newer version now and reopens Arcwise, downloading it first if needed.
    pub fn install_now(&mut self, cx: &mut Context<Self>) {
        match std::mem::replace(&mut self.state, State::UpToDate) {
            State::Available(version) => self.download(version, true, cx),
            State::Downloading { version, .. } => {
                self.state = State::Downloading {
                    version,
                    install: true,
                };
            }
            State::Ready(update) => self.install(update, cx),
            State::UpToDate => {}
        }
        cx.notify();
    }

    pub fn dismiss_toast(&mut self, cx: &mut Context<Self>) {
        self.toast = None;
        cx.notify();
    }

    fn download(&mut self, version: Version, install: bool, cx: &mut Context<Self>) {
        let Some(dir) = update::directory() else {
            return;
        };
        self.state = State::Downloading {
            version: version.clone(),
            install,
        };
        let fetch = version.clone();
        let task = cx.background_spawn(async move { update::fetch(&fetch, &dir) });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let outcome = task.await;
            this.update(cx, |this, cx| this.fetched(version, outcome, cx))
                .ok();
        })
        .detach();
    }

    fn fetched(
        &mut self,
        version: Version,
        outcome: anyhow::Result<Outcome>,
        cx: &mut Context<Self>,
    ) {
        let install = matches!(self.state, State::Downloading { install: true, .. });
        match outcome {
            Ok(Outcome::Ready(update)) if install => self.install(update, cx),
            Ok(Outcome::Ready(update)) if self.automatic => {
                self.show(Toast::Ready(version), cx);
                self.state = State::Ready(update);
            }
            // Automatic updates were turned off while it downloaded.
            Ok(Outcome::Ready(update)) => {
                update::discard(update);
                self.state = State::Available(version);
            }
            Ok(Outcome::Unverified { reason }) => {
                eprintln!("arcwise: did not install {version}: {reason}");
                self.refused = Some(version.clone());
                self.fail(version, "Its download could not be verified.");
            }
            Err(e) => {
                eprintln!("arcwise: could not download {version}: {e:#}");
                // A background download tries again at the next check; one someone asked for
                // says so.
                if install {
                    self.fail(version, "It could not be downloaded.");
                } else {
                    self.state = State::Available(version);
                }
            }
        }
        cx.notify();
    }

    fn install(&mut self, update: Update, cx: &mut Context<Self>) {
        match update::install(&update, true) {
            Ok(()) => cx.quit(),
            Err(e) => {
                eprintln!("arcwise: could not start the installer: {e}");
                let version = update.version.clone();
                update::discard(update);
                self.fail(version, "Its installer would not start.");
            }
        }
    }

    fn fail(&mut self, version: Version, why: &'static str) {
        self.toasts += 1;
        self.toast = Some(Toast::Failed {
            version: version.clone(),
            why,
        });
        self.state = State::Available(version);
    }

    /// Shows a toast that is gone after [`TOAST_LIFETIME`], unless another has replaced it.
    fn show(&mut self, toast: Toast, cx: &mut Context<Self>) {
        self.toast = Some(toast);
        self.toasts += 1;
        let shown = self.toasts;
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            cx.background_executor().timer(TOAST_LIFETIME).await;
            this.update(cx, |this, cx| {
                if this.toasts == shown {
                    this.dismiss_toast(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn install_on_close(&mut self) {
        if let State::Ready(update) = std::mem::replace(&mut self.state, State::UpToDate)
            && let Err(e) = update::install(&update, false)
        {
            eprintln!("arcwise: could not start the installer: {e}");
        }
    }
}
