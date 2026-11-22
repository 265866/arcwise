//! The window's root: finds and loads the library, then shows it.

use std::env;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, Builder, ScopedJoinHandle};
use std::time::{Duration, Instant};

use arcwise_core::{Args, Library, Start};
use arcwise_remote::remote;
use arcwise_remote::sources::{self, ReadOutcome, State as SourceState, Status};
use gpui::{
    Animation, AnyElement, AppContext as _, AsyncApp, Context, DispatchPhase, Div, Entity,
    FontWeight, InteractiveElement, IntoElement, MouseDownEvent, MouseUpEvent, ParentElement,
    PathPromptOptions, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Task,
    WeakEntity, Window,
};

use semver::Version;

use crate::browser::{
    self, AddLibraryFolder, BrowserView, Chosen, Libraries, Relaunch, SetAutoUpdate, SetBlur,
    SetBlurStrength, SetCustomTitleBar, SetLibraries, SetReopenLast, SetShortcuts, SetStartSpeed,
    SetTheme, Tab,
};
use crate::downloads::Downloads;
use crate::images::{self, Picture};
use crate::placement::Placement;
use crate::settings::{Last, LibrarySource, Settings};
use crate::updater::{State, TOAST_LIFETIME, Toast, Updater};
use crate::{theme, titlebar, trace, ui};

/// What the window shows.
enum Screen {
    /// No library yet, or the last one failed to load.
    Welcome {
        problem: Option<SharedString>,
    },
    Loading,
    Browsing(Entity<BrowserView>),
}

/// The window's root view: the welcome screen, the loading note or the lineup browser, with the
/// notices and update toasts over them, and the settings they change.
pub(crate) struct Arcwise {
    settings: Settings,
    screen: Screen,
    /// Where the command line asked to start; used by the first library only.
    start: Option<Args>,
    /// Why the last library asked for could not be opened, shown over the one still open.
    notice: Option<SharedString>,
    /// Library loads started, to tell the latest from older ones still running.
    loads: u64,
    /// How each library in the list fared when they were last read.
    statuses: Vec<Status>,
    downloads: Entity<Downloads>,
    /// The settings tab to show again once the libraries changed there have been read.
    reopen: Option<Tab>,
    updater: Entity<Updater>,
    /// Saves where the window is once it has stayed still for [`SAVE_AFTER_MOVE`].
    save_window: Option<Task<()>>,
}

impl Arcwise {
    /// The root over `settings`, opening where `args` asks, or showing why it can't be read; with
    /// `preload`, the library [`preload`] started reading before the window opened.
    pub(crate) fn new(
        args: Result<Args, String>,
        mut settings: Settings,
        preload: Option<Receiver<Opened>>,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let replaced = settings.note_version(&arcwise_update::current_version());
        let updater = cx.new(|cx| Updater::new(settings.auto_update, replaced, cx));
        cx.observe(&updater, |_, _, cx| cx.notify()).detach();
        let mut this = Arcwise {
            screen: Screen::Welcome { problem: None },
            settings,
            start: None,
            notice: None,
            loads: 0,
            statuses: Vec::new(),
            downloads: cx.new(|_| Downloads::default()),
            reopen: None,
            updater,
            save_window: None,
        };
        this.theme_followed();
        match args {
            Err(problem) => {
                this.screen = Screen::Welcome {
                    problem: Some(problem.into()),
                };
            }
            Ok(args) => {
                this.start = Some(args);
                match preload {
                    Some(preload) => this.take_preload(preload, cx),
                    None => this.load(cx),
                }
            }
        }
        this
    }

    /// Takes the library read since launch: at once when it is ready, as it usually is by the
    /// time the window opens, so the first frame shows it. When the thread reading it is gone
    /// without an answer, this reads it here instead.
    fn take_preload(&mut self, preload: Receiver<Opened>, cx: &mut Context<'_, Self>) {
        let load = self.begin_load(cx);
        match preload.recv_timeout(PRELOAD_WAIT) {
            Ok(opened) => self.opened(load, opened, cx),
            Err(RecvTimeoutError::Disconnected) => self.load(cx),
            Err(RecvTimeoutError::Timeout) => {
                let task = cx.background_spawn(async move { preload.recv().ok() });
                cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                    let opened = task.await;
                    if let Some(this) = this.upgrade() {
                        this.update(cx, |this, cx| match opened {
                            Some(opened) => this.opened(load, opened, cx),
                            None if this.loads == load => this.load(cx),
                            None => {}
                        });
                    }
                })
                .detach();
            }
        }
    }

    /// Reads the libraries in the list. The lineups already open stay on screen until the new ones
    /// have loaded, and stay if none can be.
    fn load(&mut self, cx: &mut Context<'_, Self>) {
        let load = self.begin_load(cx);
        let request = Request::new(self.start.clone().unwrap_or_default(), &self.settings);
        let task = cx.background_spawn(async move { request.open() });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let opened = task.await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.opened(load, opened, cx));
            }
        })
        .detach();
    }

    /// Starts a load, and returns its number. Only the latest load counts; one overtaken by
    /// another is dropped when it finishes.
    fn begin_load(&mut self, cx: &mut Context<'_, Self>) -> u64 {
        self.loads += 1;
        if !matches!(self.screen, Screen::Browsing(_)) {
            self.screen = Screen::Loading;
        }
        self.notice = None;
        cx.notify();
        self.loads
    }

    /// Shows a library that has been read, or why it could not be.
    fn opened(&mut self, load: u64, opened: Opened, cx: &mut Context<'_, Self>) {
        if self.loads != load {
            return;
        }
        let Opened { statuses, result } = opened;
        self.statuses = statuses;
        self.keep_offline(cx);
        match result {
            Ok(Merged {
                library,
                start,
                pictures,
            }) => {
                self.start = None;
                let updater = self.updater.clone();
                let settings = &self.settings;
                let libraries = Libraries {
                    list: settings.libraries(),
                    statuses: self.statuses.clone(),
                    downloads: self.downloads.clone(),
                    reopen: self.reopen.take(),
                };
                let browser = cx.new(|cx| {
                    BrowserView::new(library, start, pictures, updater, libraries, settings, cx)
                });
                Self::subscribe(&browser, cx);
                if let Screen::Browsing(old) = &self.screen {
                    old.update(cx, BrowserView::release_all);
                }
                self.screen = Screen::Browsing(browser);
            }
            Err(problem) => {
                let problem: SharedString = problem.into();
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

    /// Saves each setting `browser` changes, and acts on what it asks for.
    fn subscribe(browser: &Entity<BrowserView>, cx: &mut Context<'_, Self>) {
        cx.subscribe(browser, |this, _, chosen: &Chosen, _| {
            this.settings.last = Some(Last {
                map: chosen.map.clone(),
                agent: chosen.agent.clone(),
                side: chosen.side,
            });
            this.save_settings();
        })
        .detach();
        cx.subscribe(browser, |_, _, _: &AddLibraryFolder, cx| {
            Self::pick_folder(cx);
        })
        .detach();
        cx.subscribe(browser, |this, _, changed: &SetLibraries, cx| {
            this.settings.libraries = Some(changed.list.clone());
            this.save_settings();
            if changed.reload {
                this.reopen = Some(Tab::General);
                this.start = Some(Args {
                    resume: true,
                    ..Args::default()
                });
                this.load(cx);
            }
        })
        .detach();
        cx.subscribe(browser, |this, _, &SetBlur(on): &SetBlur, _| {
            this.settings.blur = on;
            this.save_settings();
        })
        .detach();
        cx.subscribe(
            browser,
            |this, _, &SetBlurStrength(strength): &SetBlurStrength, _| {
                this.settings.blur_strength = strength;
                this.save_settings();
            },
        )
        .detach();
        cx.subscribe(browser, |this, _, &SetAutoUpdate(on): &SetAutoUpdate, _| {
            this.settings.auto_update = on;
            this.save_settings();
        })
        .detach();
        cx.subscribe(browser, |this, _, _: &SetTheme, _| {
            this.settings.theme = theme::mode();
            this.settings.follow_system_theme = theme::follows_system();
            this.save_settings();
        })
        .detach();
        cx.subscribe(browser, |this, _, &SetReopenLast(on): &SetReopenLast, _| {
            this.settings.reopen_last = on;
            this.save_settings();
        })
        .detach();
        cx.subscribe(
            browser,
            |this, _, &SetStartSpeed(speed): &SetStartSpeed, _| {
                this.settings.start_speed = speed;
                this.save_settings();
            },
        )
        .detach();
        cx.subscribe(
            browser,
            |this, _, &SetCustomTitleBar(on): &SetCustomTitleBar, _| {
                this.settings.custom_title_bar = on;
                this.save_settings();
            },
        )
        .detach();
        cx.subscribe(
            browser,
            |this, _, SetShortcuts(changes): &SetShortcuts, _| {
                this.settings.shortcuts.clone_from(changes);
                this.save_settings();
            },
        )
        .detach();
        cx.subscribe(browser, |this, _, _: &Relaunch, cx| this.relaunch(cx))
            .detach();
    }

    /// A toast at the bottom right about a version. Ones saying a version is available or installed
    /// fade away by themselves; one saying it did not install stays until dismissed.
    fn update_toast(&self, cx: &mut Context<'_, Self>) -> Option<impl IntoElement> {
        use gpui::AnimationExt as _;

        let toast = gpui::div()
            .absolute()
            .right(gpui::px(20.))
            .bottom(gpui::px(20.))
            .occlude()
            .rounded(gpui::px(12.))
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg();
        let fade = 1. / TOAST_LIFETIME.as_secs_f32();
        let row = |toast: Div, id: &'static str, text: String| {
            toast
                .id(id)
                .flex()
                .items_center()
                .gap(gpui::px(12.))
                .py(gpui::px(8.))
                .pl(gpui::px(14.))
                .pr(gpui::px(8.))
                .min_h(gpui::px(46.))
                .text_size(gpui::px(13.))
                .child(text)
        };
        let fade_out = move |toast: Stateful<Div>, id: &'static str| {
            toast
                .with_animation(id, Animation::new(TOAST_LIFETIME), move |toast, t| {
                    toast.opacity(((1. - t) / fade).min(1.))
                })
                .into_any_element()
        };
        Some(match self.updater.read(cx).toast()? {
            Toast::Available(version) => {
                let browser = match &self.screen {
                    Screen::Browsing(browser) => Some(browser.clone()),
                    Screen::Welcome { .. } | Screen::Loading => None,
                };
                let toast = row(
                    toast,
                    "update-available",
                    format!("Arcwise {version} is available"),
                )
                .children(browser.map(|browser| {
                    ui::icon_button(
                        "update-settings".into(),
                        ui::GEAR,
                        gpui::px(30.),
                        theme::muted().into(),
                    )
                    .tooltip(ui::tip("Open settings".into()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let opened = browser.update(cx, |browser, cx| {
                            browser.open_settings(Some(Tab::Updates), cx)
                        });
                        if opened {
                            this.updater.update(cx, Updater::dismiss_toast);
                        }
                    }))
                }));
                fade_out(toast, "update-available-fade")
            }
            Toast::Updated(version) => {
                let toast = row(toast, "updated", format!("Updated to Arcwise {version}"))
                    .pr(gpui::px(14.));
                fade_out(toast, "updated-fade")
            }
            Toast::Failed { version, why } => {
                let version = version.clone();
                let why = *why;
                failed_toast(toast, &version, why, cx)
            }
        })
    }

    /// Ends each frame for [`trace`] with a canvas painted last, which also starts an input at
    /// every click. Keys start theirs in [`run()`](crate::run()), before any binding can take them.
    fn traced(&self, root: Div, start: Instant) -> Div {
        let browser = match &self.screen {
            Screen::Browsing(browser) => Some(browser.downgrade()),
            Screen::Welcome { .. } | Screen::Loading => None,
        };
        let loading = matches!(self.screen, Screen::Loading);
        root.child(
            gpui::canvas(
                |_, _, _| {},
                move |_, (), window, cx| {
                    let settled = match browser.as_ref().and_then(WeakEntity::upgrade) {
                        Some(browser) => browser.read(cx).settled(cx),
                        None => !loading,
                    };
                    trace::frame(start, settled);
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

    /// Saves the theme the window shows now, after it followed Windows', so the next launch starts
    /// on it before the window can say. Only the theme, over the file, since nobody changed a
    /// setting here and another Arcwise may have since.
    pub(crate) fn theme_followed(&mut self) {
        if self.settings.theme == theme::mode() {
            return;
        }
        self.settings.theme = theme::mode();
        if let Err(e) = self.settings.save_theme() {
            remote::report(format_args!("could not save the theme: {e:#}"));
        }
    }

    fn save_settings(&self) {
        if let Err(e) = self.settings.save() {
            remote::report(format_args!("could not save settings: {e:#}"));
        }
    }

    /// Remembers where the window is, and saves it once the window has stayed still for a moment
    /// rather than at every step of a drag. GPUI also reports a bounds change when the window
    /// gains or loses focus, as when switching to the game and back, where nothing has moved, and
    /// that saves nothing. A move replaces the last move's save, which cancels it.
    pub(crate) fn window_moved(&mut self, window: &Window, cx: &mut Context<'_, Self>) {
        let Some(placement) = Placement::of(window, cx)
            .filter(|placement| self.settings.window.as_ref() != Some(placement))
        else {
            return;
        };
        self.settings.window = Some(placement);
        self.save_window = Some(cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor().timer(SAVE_AFTER_MOVE).await;
                if let Some(this) = this.upgrade() {
                    this.update(cx, |this, _| {
                        if let Err(e) = this.settings.save_window() {
                            remote::report(format_args!(
                                "could not save where the window is: {e:#}"
                            ));
                        }
                    });
                }
            },
        ));
    }

    /// Opens a new Arcwise and closes this one, which reopens where it was from the settings, the
    /// window included. A downloaded update installs instead, and the installer opens the new
    /// version.
    fn relaunch(&mut self, cx: &mut Context<'_, Self>) {
        self.save_settings();
        if matches!(self.updater.read(cx).state(), State::Ready(_)) {
            self.updater.update(cx, Updater::install_now);
            return;
        }
        let relaunched =
            env::current_exe().and_then(|exe| Command::new(exe).arg("--resume").spawn());
        match relaunched {
            Ok(_) => cx.quit(),
            Err(e) => remote::report(format_args!("could not relaunch: {e}")),
        }
    }

    /// Asks for a library folder, adds it to the list, and reads the libraries again.
    fn pick_folder(cx: &mut Context<'_, Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Use this library".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            if let Ok(Ok(Some(mut paths))) = picked.await
                && let Some(path) = paths.pop()
                && let Some(this) = this.upgrade()
            {
                this.update(cx, |this, cx| {
                    this.add_library(path.to_string_lossy().into_owned(), cx);
                });
            }
        })
        .detach();
    }

    /// Fetches what changed in the maps each library on the web keeps offline, and works out how
    /// much of each is on disk for the settings.
    fn keep_offline(&mut self, cx: &mut Context<'_, Self>) {
        for library in self.settings.libraries() {
            let loaded = self.statuses.iter().any(|status| {
                status.location == library.location
                    && matches!(status.state, SourceState::Loaded { .. })
            });
            if !loaded || !remote::is_url(&library.location) {
                continue;
            }
            self.downloads.update(cx, |downloads, cx| {
                if library.keep_all {
                    downloads.start(&library.location, None, cx);
                } else if !library.keep.is_empty() {
                    downloads.start(&library.location, Some(library.keep.clone()), cx);
                }
                Downloads::measure(&library.location, cx);
            });
        }
    }

    /// Adds a library to the list, or turns it on when it is there already, and reads the
    /// libraries again.
    fn add_library(&mut self, location: String, cx: &mut Context<'_, Self>) {
        let mut libraries = self.settings.libraries();
        match libraries
            .iter_mut()
            .find(|known| known.location == location)
        {
            Some(known) => known.on = true,
            None => libraries.push(LibrarySource::new(location)),
        }
        self.settings.libraries = Some(libraries);
        self.save_settings();
        self.load(cx);
    }
}

/// The toast saying `version` did not install, and `why`, over `toast`'s frame: shown until
/// dismissed, with a way to the releases page.
fn failed_toast(
    toast: Div,
    version: &Version,
    why: &str,
    cx: &mut Context<'_, Arcwise>,
) -> AnyElement {
    toast
        .id("update-failed")
        .flex()
        .flex_col()
        .gap(gpui::px(10.))
        .p(gpui::px(14.))
        .w(gpui::px(340.))
        .child(
            gpui::div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("Arcwise {version} was not installed")),
        )
        .child(
            gpui::div()
                .text_size(gpui::px(13.))
                .text_color(theme::muted())
                .child(format!("{why} Get it from the releases page instead.")),
        )
        .child(
            gpui::div()
                .flex()
                .gap(gpui::px(8.))
                .child(
                    ui::button("open-releases".into(), "Open releases").on_click(cx.listener(
                        |this, _, _, cx| {
                            cx.open_url(arcwise_update::RELEASES_PAGE);
                            this.updater.update(cx, Updater::dismiss_toast);
                        },
                    )),
                )
                .child(
                    ui::button("dismiss-update".into(), "Dismiss").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.updater.update(cx, Updater::dismiss_toast);
                        },
                    )),
                ),
        )
        .into_any_element()
}

/// A note along the bottom of the window; clicking it dismisses it.
fn notice_banner(notice: &str, cx: &mut Context<'_, Arcwise>) -> impl IntoElement {
    gpui::div()
        .absolute()
        .left_0()
        .right_0()
        .bottom(gpui::px(20.))
        .flex()
        .justify_center()
        .child(
            gpui::div()
                .id("notice")
                .occlude()
                .max_w(gpui::px(560.))
                .px(gpui::px(14.))
                .py(gpui::px(10.))
                .rounded(gpui::px(10.))
                .border_1()
                .border_color(theme::alpha(theme::accent().into(), 0.4))
                .bg(theme::panel())
                .shadow_lg()
                .text_size(gpui::px(13.))
                .text_color(theme::attack())
                .cursor_pointer()
                .child(format!("Could not open that library: {notice}"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.notice = None;
                    cx.notify();
                })),
        )
}

/// The screen with no library to show: why, and buttons to try again or add a folder.
fn welcome(problem: Option<&SharedString>, cx: &mut Context<'_, Arcwise>) -> impl IntoElement {
    gpui::div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui::div()
                .w(gpui::px(460.))
                .flex()
                .flex_col()
                .gap(gpui::px(14.))
                .p(gpui::px(28.))
                .rounded(gpui::px(16.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::panel())
                .child(
                    gpui::div()
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::BOLD)
                        .text_size(gpui::px(24.))
                        .child("No lineups to show"),
                )
                .child(gpui::div().text_color(theme::muted()).child(
                    "Arcwise could not read any of its libraries. Check the connection, or add a \
                     library folder that holds library.json.",
                ))
                .children(problem.map(|problem| {
                    gpui::div()
                        .p(gpui::px(10.))
                        .rounded(gpui::px(8.))
                        .bg(theme::alpha(theme::accent().into(), 0.12))
                        .text_color(theme::attack())
                        .text_size(gpui::px(13.))
                        .child(problem.clone())
                }))
                .child(
                    gpui::div()
                        .flex()
                        .gap(gpui::px(8.))
                        .child(
                            ui::button("retry-libraries".into(), "Try again")
                                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                        )
                        .child(
                            ui::button("choose-library".into(), "Add a folder…")
                                .on_click(cx.listener(|_, _, _, cx| Arcwise::pick_folder(cx))),
                        ),
                ),
        )
}

impl Render for Arcwise {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        use gpui::Focusable as _;

        let start = Instant::now();
        let root = gpui::div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::backdrop())
            .text_color(theme::text())
            .font_family(theme::FONT)
            .text_size(gpui::px(14.));
        let root = match &self.screen {
            Screen::Browsing(browser) => {
                let focus = browser.focus_handle(cx);
                if !focus.contains_focused(window, cx) {
                    focus.focus(window, cx);
                }
                root.relative()
                    .child(browser.clone())
                    .children(self.notice.as_ref().map(|notice| notice_banner(notice, cx)))
            }
            Screen::Welcome { problem } => root
                .relative()
                .child(titlebar::frame(ui::bar(), window))
                .child(
                    gpui::div()
                        .flex_1()
                        .min_h_0()
                        .child(welcome(problem.as_ref(), cx)),
                ),
            Screen::Loading => root
                .relative()
                .child(titlebar::frame(ui::bar(), window))
                .child(
                    gpui::div().flex_1().min_h_0().child(
                        gpui::div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(theme::muted())
                            .child("Loading lineups…"),
                    ),
                ),
        };
        let root = root.children(self.update_toast(cx));
        if trace::enabled() {
            self.traced(root, start)
        } else {
            root
        }
    }
}

/// How long the window must stay still before where it is gets saved. A move this soon before
/// closing is not saved.
const SAVE_AFTER_MOVE: Duration = Duration::from_millis(500);

/// How long the window waits for the library read since launch before opening without it.
const PRELOAD_WAIT: Duration = Duration::from_millis(100);

/// The libraries to read off the UI thread, and where to open.
struct Request {
    libraries: Vec<LibrarySource>,
    /// The command line's choice of where to start.
    args: Args,
    /// What was on screen last.
    last: Option<Last>,
}

/// The libraries read off the UI thread, merged, with the pictures they first show already
/// decoded; or why none could be read.
pub(crate) struct Opened {
    statuses: Vec<Status>,
    result: Result<Merged, String>,
}

/// The libraries read, merged into one, where to open it, and the pictures it first shows.
struct Merged {
    library: Library,
    start: Start,
    pictures: Vec<Picture>,
}

impl Request {
    /// Reads the settings' list of libraries, or only the one the command line names. What was on
    /// screen last counts only when the settings reopen it or the command line resumes.
    fn new(args: Args, settings: &Settings) -> Request {
        let libraries = match &args.library {
            Some(location) => vec![LibrarySource::new(location.to_string_lossy().into_owned())],
            None => settings.libraries(),
        };
        let last = settings
            .last
            .clone()
            .filter(|_| settings.reopen_last || args.resume);
        Request {
            libraries,
            args,
            last,
        }
    }

    /// Reads every library turned on, at once, and decodes the pictures they open on. Folders
    /// take tens of milliseconds; a library on the web as long as the web takes.
    fn open(self) -> Opened {
        let begun = Instant::now();
        let read: Vec<Option<ReadOutcome>> = thread::scope(|scope| {
            let reading: Vec<Option<ScopedJoinHandle<'_, ReadOutcome>>> = self
                .libraries
                .iter()
                .map(|library| {
                    library
                        .on
                        .then(|| scope.spawn(|| sources::read(&library.location)))
                })
                .collect();
            reading
                .into_iter()
                .map(|handle| handle.map(joined))
                .collect()
        });
        trace::task(|| "libraries".into(), begun);
        let mut statuses = Vec::with_capacity(read.len());
        let mut parts = Vec::new();
        let mut problems = Vec::new();
        for (library, read) in self.libraries.iter().zip(read) {
            let state = match read {
                None => SourceState::Off,
                Some(ReadOutcome {
                    part: Ok(part),
                    stale,
                }) => {
                    let state = sources::loaded(&part, stale);
                    parts.push(part);
                    state
                }
                Some(ReadOutcome {
                    part: Err(problem), ..
                }) => {
                    problems.push(format!("{}: {problem}", sources::title(&library.location)));
                    SourceState::Failed(problem)
                }
            };
            statuses.push(Status {
                location: library.location.clone(),
                state,
            });
        }
        let result = if parts.is_empty() {
            Err(if problems.is_empty() {
                "Every library is turned off in the settings.".to_owned()
            } else {
                problems.join("\n")
            })
        } else {
            let library = Library::merge(parts);
            let mut skipped = library.skipped().iter();
            for status in &mut statuses {
                if let SourceState::Loaded { hidden, .. } = &mut status.state {
                    *hidden = skipped.next().copied().unwrap_or_default();
                }
            }
            let start = start(&library, self.args, self.last);
            let begun = Instant::now();
            let pictures = images::decode_all(&browser::first_pictures(&library, start.clone()));
            trace::task(|| "first pictures".into(), begun);
            Ok(Merged {
                library,
                start,
                pictures,
            })
        };
        Opened { statuses, result }
    }
}

/// What a thread reading a library gave, or why it gave nothing when it panicked.
fn joined(handle: ScopedJoinHandle<'_, ReadOutcome>) -> ReadOutcome {
    handle.join().unwrap_or_else(|_| ReadOutcome {
        part: Err("it could not be read".into()),
        stale: false,
    })
}

/// Starts reading the library launch will open, on a thread of its own, while GPUI spends a few
/// hundred milliseconds opening the window. Nothing when the command line can't be read, or the
/// thread can't start, and the window reads it itself. The window may stop waiting for it, and
/// then what was read is dropped.
pub(crate) fn preload(
    args: &Result<Args, String>,
    settings: &Settings,
) -> Option<Receiver<Opened>> {
    let args = args.as_ref().ok()?;
    let request = Request::new(args.clone(), settings);
    let (opened, preload) = mpsc::sync_channel(1);
    Builder::new()
        .name("arcwise-preload".into())
        .spawn(move || opened.send(request.open()).unwrap_or_else(drop))
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
            .map(|map| map.id.clone()),
        agent: args
            .agent
            .and_then(|name| library.agent_by_name(&name))
            .map(|agent| agent.id.clone()),
        side: args.side,
        lineup: args.lineup,
        view: args.view,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::{fs, process};

    use arcwise_core::{AgentId, LineupId, MapId, Side};

    use super::*;

    const LIBRARY: &[u8] = include_bytes!("../tests/fixtures/library.json");

    fn library() -> Library {
        Library::from_json(Path::new("lib"), LIBRARY).unwrap()
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
    fn opening_where_it_was_left_can_be_turned_off() {
        let settings = Settings {
            last: Some(last()),
            reopen_last: false,
            ..Settings::default()
        };
        let request = Request::new(Args::default(), &settings);
        assert_eq!(request.last, None);
        let resume = Args {
            resume: true,
            ..Args::default()
        };
        assert_eq!(
            Request::new(resume, &settings).last,
            Some(last()),
            "relaunching to apply a setting or an update still goes back there"
        );
    }

    #[test]
    fn the_libraries_are_the_lists_or_the_command_lines() {
        let settings = Settings {
            library: Some(r"D:\lineups".into()),
            ..Settings::default()
        };
        let request = Request::new(Args::default(), &settings);
        let locations: Vec<&str> = request
            .libraries
            .iter()
            .map(|library| library.location.as_str())
            .collect();
        assert_eq!(
            locations,
            [remote::DEFAULT, r"D:\lineups"],
            "the old folder is kept"
        );
        let args = Args {
            library: Some("https://example.com/lib".into()),
            ..Args::default()
        };
        let request = Request::new(args, &settings);
        let locations: Vec<&str> = request
            .libraries
            .iter()
            .map(|library| library.location.as_str())
            .collect();
        assert_eq!(locations, ["https://example.com/lib"]);
    }

    #[test]
    fn reads_the_libraries_turned_on_and_says_how_each_fared() {
        let dir = env::temp_dir().join(format!("arcwise-app-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("library.json"), LIBRARY).unwrap();
        let folder = dir.to_string_lossy().into_owned();
        let copy = dir.join("copy");
        fs::create_dir_all(&copy).unwrap();
        fs::copy(dir.join("library.json"), copy.join("library.json")).unwrap();
        let copy = copy.to_string_lossy().into_owned();
        let missing = dir.join("missing").to_string_lossy().into_owned();
        let off = LibrarySource {
            on: false,
            ..LibrarySource::new("https://example.invalid/".to_owned())
        };
        let settings = Settings {
            libraries: Some(vec![
                off,
                LibrarySource::new(folder),
                LibrarySource::new(missing),
                LibrarySource::new(copy),
            ]),
            ..Settings::default()
        };
        let opened = Request::new(Args::default(), &settings).open();
        let merged = opened.result.unwrap();
        assert_eq!(merged.library.lineups().len(), 9);
        assert_eq!(opened.statuses[0].state, SourceState::Off);
        assert!(matches!(
            opened.statuses[1].state,
            SourceState::Loaded {
                lineups: 9,
                hidden: 0,
                ..
            }
        ));
        assert!(matches!(opened.statuses[2].state, SourceState::Failed(_)));
        assert!(
            matches!(
                opened.statuses[3].state,
                SourceState::Loaded {
                    lineups: 9,
                    hidden: 9,
                    ..
                }
            ),
            "a copy of the same library lower in the list has every lineup shown already"
        );
        fs::remove_dir_all(&dir).unwrap();
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
