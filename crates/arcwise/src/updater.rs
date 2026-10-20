//! What Arcwise knows about updates, shared by the window, which shows toasts, and the settings
//! menu, which shows the version and an install button. With automatic updates on, a newer
//! version downloads in the background and installs when Arcwise closes. With them off, a toast
//! says it exists and the settings menu offers to install it.

use std::time::Duration;

use gpui::{AsyncApp, Context, WeakEntity, prelude::*};
use semver::Version;

use crate::update::{self, Outcome, Update};

/// How long the toast about an available version shows before it is gone, fade included.
pub const TOAST_LIFETIME: Duration = Duration::from_secs(6);

pub struct Updater {
    /// Whether updates download and install without asking.
    automatic: bool,
    state: State,
    toast: Option<Toast>,
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
    /// A version that did not install, and why. Shown until dismissed.
    Failed { version: Version, why: &'static str },
}

impl Updater {
    pub fn new(automatic: bool, cx: &mut Context<Self>) -> Self {
        // Closing the window releases everything that holds this before GPUI quits, too late for
        // an app-quit handler, so a ready update installs on release.
        cx.on_release(|this, _| this.install_on_close()).detach();
        let this = Updater {
            automatic,
            state: State::UpToDate,
            toast: None,
        };
        if update::installed()
            && let Some(dir) = update::directory()
        {
            let current = update::current_version();
            let task = cx.background_spawn(async move { update::latest(&current, &dir) });
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let latest = task.await;
                this.update(cx, |this, cx| this.found(latest, cx)).ok();
            })
            .detach();
        }
        this
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

    /// Whether a newer version waits for someone to install it from the settings menu, which
    /// happens only with automatic updates off.
    pub fn waiting(&self) -> bool {
        !self.automatic && !matches!(self.state, State::UpToDate)
    }

    fn found(&mut self, latest: anyhow::Result<Option<Version>>, cx: &mut Context<Self>) {
        let version = match latest {
            Ok(Some(version)) => version,
            Ok(None) => return,
            // Offline, or GitHub did not answer; the next launch asks again.
            Err(e) => {
                eprintln!("arcwise: could not check for updates: {e:#}");
                return;
            }
        };
        if self.automatic {
            self.download(version, false, cx);
        } else {
            self.show_available(version.clone(), cx);
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
            Ok(Outcome::Ready(update)) if self.automatic => self.state = State::Ready(update),
            // Automatic updates were turned off while it downloaded.
            Ok(Outcome::Ready(update)) => {
                update::discard(update);
                self.state = State::Available(version);
            }
            Ok(Outcome::Unverified { reason }) => {
                eprintln!("arcwise: did not install {version}: {reason}");
                self.fail(version, "Its download could not be verified.");
            }
            Err(e) => {
                eprintln!("arcwise: could not download {version}: {e:#}");
                // A background download tries again next launch; one someone asked for says so.
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
        self.toast = Some(Toast::Failed {
            version: version.clone(),
            why,
        });
        self.state = State::Available(version);
    }

    fn show_available(&mut self, version: Version, cx: &mut Context<Self>) {
        self.toast = Some(Toast::Available(version));
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            cx.background_executor().timer(TOAST_LIFETIME).await;
            this.update(cx, |this, cx| {
                // Unless a failure replaced it, which stays until dismissed.
                if matches!(this.toast, Some(Toast::Available(_))) {
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
