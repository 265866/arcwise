//! The window's root: finds and loads the library, then shows it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use arcwise_core::{Library, LoadError, Start};
use gpui::{
    AsyncApp, ClickEvent, Context, DispatchPhase, Div, Entity, Focusable, FontWeight, IntoElement,
    MouseDownEvent, MouseUpEvent, PathPromptOptions, SharedString, WeakEntity, Window, canvas, div,
    prelude::*, px,
};
use semver::Version;

use crate::args::Args;
use crate::browser::{self, Browser, ChangeLibrary, Chosen, SetBlur, SetBlurStrength};
use crate::images::{self, Picture};
use crate::settings::{Last, Settings};
use crate::update::{self, Outcome, Update};
use crate::{theme, trace, ui};

enum Screen {
    /// No library yet, or the last one failed to load.
    Welcome {
        problem: Option<SharedString>,
    },
    Loading(PathBuf),
    Browsing(Entity<Browser>),
}

pub struct Arcwise {
    settings: Settings,
    screen: Screen,
    /// Where the command line asked to start; used by the first library only.
    start: Option<Args>,
    /// Why the last library asked for could not be opened, shown over the one still open.
    notice: Option<SharedString>,
    /// Library loads started, to tell the latest from older ones still running.
    loads: u64,
    /// A newer version, downloaded and verified, which installs when Arcwise closes.
    update: Option<Update>,
    /// What to say about updates, until the user answers it.
    update_banner: Option<UpdateBanner>,
}

enum UpdateBanner {
    Ready(Version),
    /// A newer version that will not install, and why.
    NotInstalled(Version, &'static str),
}

impl Arcwise {
    pub fn new(
        args: Result<Args, String>,
        settings: Settings,
        preload: Option<Receiver<Opened>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Arcwise {
            screen: Screen::Welcome { problem: None },
            settings,
            start: None,
            notice: None,
            loads: 0,
            update: None,
            update_banner: None,
        };
        // Closing the window releases this view before GPUI quits, too late for an app-quit
        // handler on it, so the update installs on release.
        cx.on_release(|this, _| this.install_on_close()).detach();
        if this.settings.auto_update {
            this.check_for_update(cx);
        }
        match args {
            Err(problem) => {
                this.screen = Screen::Welcome {
                    problem: Some(problem.into()),
                }
            }
            Ok(args) => {
                let library = args
                    .library
                    .clone()
                    .or_else(|| this.settings.library.clone());
                this.start = Some(args);
                match (library, preload) {
                    (Some(path), Some(preload)) => this.take_preload(&path, preload, cx),
                    (Some(path), None) => this.load(path, cx),
                    (None, _) => {}
                }
            }
        }
        this
    }

    /// Takes the library read since launch: at once when it is ready, as it usually is by the
    /// time the window opens, so the first frame shows it.
    fn take_preload(&mut self, path: &Path, preload: Receiver<Opened>, cx: &mut Context<Self>) {
        let load = self.begin_load(path, cx);
        match preload.recv_timeout(PRELOAD_WAIT) {
            Ok(opened) => self.opened(load, opened, cx),
            // The thread reading it is gone without an answer; read it here instead.
            Err(RecvTimeoutError::Disconnected) => self.load(path.to_owned(), cx),
            Err(RecvTimeoutError::Timeout) => {
                let path = path.to_owned();
                let task = cx.background_spawn(async move { preload.recv().ok() });
                cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                    let opened = task.await;
                    this.update(cx, |this, cx| match opened {
                        Some(opened) => this.opened(load, opened, cx),
                        None if this.loads == load => this.load(path, cx),
                        None => {}
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Loads the library at `path`. A library already open stays on screen until the new one has
    /// loaded, and stays if it fails.
    fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let load = self.begin_load(&path, cx);
        let request = Request::new(path, self.start.clone().unwrap_or_default(), &self.settings);
        let task = cx.background_spawn(async move { request.open() });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let opened = task.await;
            this.update(cx, |this, cx| this.opened(load, opened, cx))
                .ok();
        })
        .detach();
    }

    /// Starts a load, and returns its number. Only the latest load counts; one overtaken by
    /// another is dropped when it finishes.
    fn begin_load(&mut self, path: &Path, cx: &mut Context<Self>) -> u64 {
        self.loads += 1;
        if !matches!(self.screen, Screen::Browsing(_)) {
            self.screen = Screen::Loading(path.to_owned());
        }
        self.notice = None;
        cx.notify();
        self.loads
    }

    /// Shows a library that has been read, or why it could not be.
    fn opened(&mut self, load: u64, opened: Opened, cx: &mut Context<Self>) {
        if self.loads != load {
            return;
        }
        let Opened { path, result } = opened;
        match result {
            Ok((library, start, pictures)) => {
                let same_library = self.settings.library.as_ref() == Some(&path);
                self.start = None;
                let library = Arc::new(library);
                let browser = cx.new(|cx| Browser::new(library, start, pictures, cx));
                cx.subscribe(&browser, |this, _, chosen: &Chosen, _| {
                    this.settings.last = Some(Last {
                        map: chosen.map.clone(),
                        agent: chosen.agent.clone(),
                        side: chosen.side,
                    });
                    this.save_settings();
                })
                .detach();
                cx.subscribe(&browser, |this, _, _: &ChangeLibrary, cx| {
                    this.pick_folder(cx);
                })
                .detach();
                cx.subscribe(&browser, |this, _, &SetBlur(on): &SetBlur, _| {
                    this.settings.blur = on;
                    this.save_settings();
                })
                .detach();
                cx.subscribe(
                    &browser,
                    |this, _, &SetBlurStrength(s): &SetBlurStrength, _| {
                        this.settings.blur_strength = s;
                        this.save_settings();
                    },
                )
                .detach();
                if let Screen::Browsing(old) = &self.screen {
                    old.update(cx, |old, cx| old.release_all(cx));
                }
                self.screen = Screen::Browsing(browser);
                if !same_library {
                    self.settings.library = Some(path);
                    self.settings.last = None;
                    self.save_settings();
                }
            }
            Err(e) => {
                let problem = format!("{:#}", anyhow::Error::from(e)).into();
                if matches!(self.screen, Screen::Browsing(_)) {
                    self.notice = Some(problem);
                } else {
                    self.screen = Screen::Welcome {
                        problem: Some(problem),
                    };
                }
            }
        }
        cx.notify();
    }

    /// A note along the bottom of the window; clicking it dismisses it.
    fn notice_banner(&self, notice: SharedString, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(20.))
            .flex()
            .justify_center()
            .child(
                div()
                    .id("notice")
                    .occlude()
                    .max_w(px(560.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::alpha(theme::accent(), 0.4))
                    .bg(theme::panel())
                    .shadow_lg()
                    .text_size(px(13.))
                    .text_color(theme::attack())
                    .cursor_pointer()
                    .child(format!("Could not open that library: {notice}"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    })),
            )
    }

    /// Looks for a newer version off the UI thread, and readies it to install.
    fn check_for_update(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = update::directory() else {
            return;
        };
        let task =
            cx.background_spawn(async move { update::check(&update::current_version(), &dir) });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let outcome = task.await;
            this.update(cx, |this, cx| this.update_checked(outcome, cx))
                .ok();
        })
        .detach();
    }

    fn update_checked(&mut self, outcome: anyhow::Result<Outcome>, cx: &mut Context<Self>) {
        match outcome {
            Ok(Outcome::UpToDate) => return,
            Ok(Outcome::Ready(update)) => {
                self.update_banner = Some(UpdateBanner::Ready(update.version.clone()));
                self.update = Some(update);
            }
            Ok(Outcome::Unverified { version, reason }) => {
                eprintln!("arcwise: did not install {version}: {reason}");
                self.update_banner = Some(UpdateBanner::NotInstalled(
                    version,
                    "Its download could not be verified.",
                ));
            }
            // Offline, or GitHub did not answer; the next launch tries again.
            Err(e) => {
                eprintln!("arcwise: could not check for updates: {e:#}");
                return;
            }
        }
        cx.notify();
    }

    fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let Some(update) = self.update.take() else {
            return;
        };
        match update::install(&update, true) {
            Ok(()) => cx.quit(),
            Err(e) => {
                eprintln!("arcwise: could not start the installer: {e}");
                self.update_banner = Some(UpdateBanner::NotInstalled(
                    update.version,
                    "Its installer would not start.",
                ));
                cx.notify();
            }
        }
    }

    fn install_on_close(&mut self) {
        if let Some(update) = self.update.take()
            && update::enabled()
            && let Err(e) = update::install(&update, false)
        {
            eprintln!("arcwise: could not start the installer: {e}");
        }
    }

    /// A card at the bottom right about a new version: ready to install, or why it was not.
    fn update_card(&self, banner: &UpdateBanner, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, detail) = match banner {
            UpdateBanner::Ready(version) => (
                format!("Arcwise {version} is ready"),
                "It installs when you close Arcwise.".to_owned(),
            ),
            UpdateBanner::NotInstalled(version, why) => (
                format!("Arcwise {version} was not installed"),
                format!("{why} Get it from the releases page instead."),
            ),
        };
        let buttons = match banner {
            UpdateBanner::Ready(_) => div()
                .flex()
                .gap(px(8.))
                .child(
                    ui::button("restart-to-update", "Restart now")
                        .on_click(cx.listener(|this, _, _, cx| this.restart_to_update(cx))),
                )
                .child(ui::button("update-later", "Later").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.update_banner = None;
                        cx.notify();
                    },
                ))),
            UpdateBanner::NotInstalled(..) => div()
                .flex()
                .gap(px(8.))
                .child(
                    ui::button("open-releases", "Open releases").on_click(cx.listener(
                        |this, _, _, cx| {
                            cx.open_url(update::RELEASES_PAGE);
                            this.update_banner = None;
                            cx.notify();
                        },
                    )),
                )
                .child(
                    ui::button("dismiss-update", "Dismiss").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.update_banner = None;
                            cx.notify();
                        },
                    )),
                ),
        };
        div()
            .id("update")
            .absolute()
            .right(px(20.))
            .bottom(px(20.))
            .occlude()
            .w(px(340.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(14.))
            .rounded(px(12.))
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg()
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .child(detail),
            )
            .child(buttons)
    }

    /// Ends each frame for [`trace`] with a canvas painted last, which also starts an input at
    /// every click. Keys start theirs in `main`, before any binding can take them.
    fn traced(&self, root: Div, start: Instant) -> Div {
        let browser = match &self.screen {
            Screen::Browsing(browser) => Some(browser.downgrade()),
            Screen::Welcome { .. } | Screen::Loading(_) => None,
        };
        let loading = matches!(self.screen, Screen::Loading(_));
        root.child(
            canvas(
                |_, _, _| {},
                move |_, (), window, cx| {
                    let settled = match browser.as_ref().and_then(|b| b.upgrade()) {
                        Some(browser) => browser.read(cx).settled(cx),
                        None => !loading,
                    };
                    trace::frame(start, settled);
                    // Window-wide, so clicks on an occluding dropdown count too.
                    window.on_mouse_event(|event: &MouseDownEvent, phase, _, _| {
                        if phase == DispatchPhase::Capture {
                            trace::input(|| format!("down {:?}", event.position));
                        }
                    });
                    window.on_mouse_event(|event: &MouseUpEvent, phase, _, _| {
                        if phase == DispatchPhase::Capture {
                            trace::input(|| format!("up {:?}", event.position));
                        }
                    });
                },
            )
            .absolute()
            .size_0(),
        )
    }

    fn save_settings(&self) {
        if let Err(e) = self.settings.save() {
            eprintln!("arcwise: could not save settings: {e:#}");
        }
    }

    fn choose_folder(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pick_folder(cx);
    }

    /// Asks for a library folder and loads it.
    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use this library".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            if let Ok(Ok(Some(mut paths))) = picked.await
                && let Some(path) = paths.pop()
            {
                this.update(cx, |this, cx| this.load(path, cx)).ok();
            }
        })
        .detach();
    }

    fn welcome(&self, problem: Option<&SharedString>, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(460.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(28.))
                    .rounded(px(16.))
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::panel())
                    .child(
                        div()
                            .font_family(theme::DISPLAY_FONT)
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(24.))
                            .child("Choose your lineup library"),
                    )
                    .child(div().text_color(theme::muted()).line_height(px(21.)).child(
                        "Arcwise reads lineups from a folder on this computer. \
                         Pick the folder that holds library.json.",
                    ))
                    .children(problem.map(|problem| {
                        div()
                            .p(px(10.))
                            .rounded(px(8.))
                            .bg(theme::alpha(theme::accent(), 0.12))
                            .text_color(theme::attack())
                            .text_size(px(13.))
                            .child(problem.clone())
                    }))
                    .child(
                        ui::button("choose-library", "Choose folder…")
                            .on_click(cx.listener(Self::choose_folder)),
                    ),
            )
    }
}

impl Render for Arcwise {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let start = Instant::now();
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::backdrop())
            .text_color(theme::text())
            .font_family(theme::FONT)
            .text_size(px(14.));
        let root = if let Screen::Browsing(browser) = &self.screen {
            if !browser.focus_handle(cx).contains_focused(window, cx) {
                browser.focus_handle(cx).focus(window, cx);
            }
            root.relative().child(browser.clone()).children(
                self.notice
                    .clone()
                    .map(|notice| self.notice_banner(notice, cx)),
            )
        } else {
            let body = match &self.screen {
                Screen::Welcome { problem } => {
                    self.welcome(problem.as_ref(), cx).into_any_element()
                }
                Screen::Loading(path) => div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::muted())
                    .child(format!("Loading {}…", path.display()))
                    .into_any_element(),
                Screen::Browsing(_) => unreachable!("rendered above"),
            };
            root.relative()
                .child(ui::bar())
                .child(div().flex_1().min_h_0().child(body))
        };
        let root = root.children(
            self.update_banner
                .as_ref()
                .map(|banner| self.update_card(banner, cx)),
        );
        if trace::enabled() {
            self.traced(root, start)
        } else {
            root
        }
    }
}

/// How long the window waits for the library read since launch before opening without it.
const PRELOAD_WAIT: Duration = Duration::from_millis(100);

/// A library to read off the UI thread, and where to open it.
pub struct Request {
    path: PathBuf,
    /// The command line's choice of where to start.
    args: Args,
    /// What was on screen last, when this is the library open last time.
    last: Option<Last>,
}

/// A library read off the UI thread, with the pictures it first shows already decoded.
pub struct Opened {
    path: PathBuf,
    result: Result<(Library, Start, Vec<Picture>), LoadError>,
}

impl Request {
    pub fn new(path: PathBuf, args: Args, settings: &Settings) -> Request {
        let last = settings
            .last
            .clone()
            .filter(|_| settings.library.as_ref() == Some(&path));
        Request { path, args, last }
    }

    /// Reads the library and decodes the pictures it opens on. Takes tens of milliseconds.
    pub fn open(self) -> Opened {
        let begun = Instant::now();
        let library = Library::open(&self.path);
        trace::task(|| "library".into(), begun);
        let result = library.map(|library| {
            let start = start(&library, self.args, self.last);
            let begun = Instant::now();
            let pictures = images::decode_all(browser::first_pictures(&library, start.clone()));
            trace::task(|| "first pictures".into(), begun);
            (library, start, pictures)
        });
        Opened {
            path: self.path,
            result,
        }
    }
}

/// Starts reading the library launch will open, on a thread of its own, while GPUI spends a few
/// hundred milliseconds opening the window.
pub fn preload(args: &Result<Args, String>, settings: &Settings) -> Option<Receiver<Opened>> {
    let args = args.as_ref().ok()?;
    let path = args.library.clone().or_else(|| settings.library.clone())?;
    let request = Request::new(path, args.clone(), settings);
    let (opened, preload) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("arcwise-preload".into())
        .spawn(move || {
            let _ = opened.send(request.open());
        })
        .ok()?;
    Some(preload)
}

/// Where to open: the command line's choice when it makes one, otherwise what was on screen
/// last. Names the library does not know are ignored.
fn start(library: &Library, args: Args, last: Option<Last>) -> Start {
    if args.map.is_none()
        && args.agent.is_none()
        && args.lineup.is_none()
        && let Some(last) = last
    {
        return Start {
            map: Some(last.map),
            agent: Some(last.agent),
            side: Some(args.side.unwrap_or(last.side)),
            lineup: None,
            view: args.view,
        };
    }
    Start {
        map: args
            .map
            .and_then(|name| library.map_by_name(&name))
            .map(|m| m.id.clone()),
        agent: args
            .agent
            .and_then(|name| library.agent_by_name(&name))
            .map(|a| a.id.clone()),
        side: args.side,
        lineup: args.lineup,
        view: args.view,
    }
}

#[cfg(test)]
mod tests {
    use arcwise_core::{AgentId, LineupId, MapId, Side};

    use super::*;

    fn library() -> Library {
        Library::from_json(
            "lib",
            include_bytes!("../../arcwise-core/tests/fixtures/library.json"),
        )
        .unwrap()
    }

    fn last() -> Last {
        Last {
            map: MapId::new("m-bind"),
            agent: AgentId::new("a-jett"),
            side: Side::Defense,
        }
    }

    #[test]
    fn opens_where_it_was_left() {
        let start = start(&library(), Args::default(), Some(last()));
        assert_eq!(start.map, Some(MapId::new("m-bind")));
        assert_eq!(start.agent, Some(AgentId::new("a-jett")));
        assert_eq!(start.side, Some(Side::Defense));
    }

    #[test]
    fn the_command_line_wins() {
        let args = Args {
            map: Some("ascent".into()),
            agent: Some("Sova".into()),
            ..Args::default()
        };
        let start = start(&library(), args, Some(last()));
        assert_eq!(start.map, Some(MapId::new("m-ascent")));
        assert_eq!(start.agent, Some(AgentId::new("a-sova")));
        assert_eq!(start.side, None);
    }

    #[test]
    fn a_lineup_on_the_command_line_wins_too() {
        let args = Args {
            lineup: Some(LineupId(5)),
            ..Args::default()
        };
        let start = start(&library(), args, Some(last()));
        assert_eq!(start.lineup, Some(LineupId(5)));
        assert_eq!(start.map, None);
    }

    #[test]
    fn unknown_names_are_ignored() {
        let args = Args {
            map: Some("Kasbah".into()),
            ..Args::default()
        };
        assert_eq!(start(&library(), args, None).map, None);
    }
}
