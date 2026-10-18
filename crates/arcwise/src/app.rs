//! The window's root: finds and loads the library, then shows it.

use std::path::PathBuf;
use std::sync::Arc;

use arcwise_core::{Library, Start};
use gpui::{
    AsyncApp, ClickEvent, Context, Entity, Focusable, FontWeight, IntoElement, PathPromptOptions,
    SharedString, WeakEntity, Window, div, prelude::*, px,
};

use crate::args::Args;
use crate::browser::{Browser, ChangeLibrary, Chosen};
use crate::settings::{Last, Settings};
use crate::theme;
use crate::ui;

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
}

impl Arcwise {
    pub fn new(args: Result<Args, String>, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let mut this = Arcwise {
            screen: Screen::Welcome { problem: None },
            settings,
            start: None,
            notice: None,
            loads: 0,
        };
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
                if let Some(path) = library {
                    this.load(path, cx);
                }
            }
        }
        this
    }

    /// Loads the library at `path`. A library already open stays on screen until the new one has
    /// loaded, and stays if it fails.
    fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // Only the latest request counts; one overtaken by another is dropped when it finishes.
        self.loads += 1;
        let load = self.loads;
        if !matches!(self.screen, Screen::Browsing(_)) {
            self.screen = Screen::Loading(path.clone());
        }
        self.notice = None;
        cx.notify();
        let task = cx.background_spawn({
            let path = path.clone();
            async move { Library::open(path) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = task.await;
            this.update(cx, |this, cx| {
                if this.loads != load {
                    return;
                }
                match result {
                    Ok(library) => {
                        let same_library = this.settings.library.as_ref() == Some(&path);
                        let last = this.settings.last.clone().filter(|_| same_library);
                        let start = start(&library, this.start.take().unwrap_or_default(), last);
                        let library = Arc::new(library);
                        let browser = cx.new(|cx| Browser::new(library, start, cx));
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
                        if let Screen::Browsing(old) = &this.screen {
                            old.update(cx, |old, cx| old.release_all(cx));
                        }
                        this.screen = Screen::Browsing(browser);
                        if !same_library {
                            this.settings.library = Some(path);
                            this.settings.last = None;
                            this.save_settings();
                        }
                    }
                    Err(e) => {
                        let problem = format!("{:#}", anyhow::Error::from(e)).into();
                        if matches!(this.screen, Screen::Browsing(_)) {
                            this.notice = Some(problem);
                        } else {
                            this.screen = Screen::Welcome {
                                problem: Some(problem),
                            };
                        }
                    }
                }
                cx.notify();
            })
        })
        .detach();
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
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT)
            .text_size(px(14.));
        if let Screen::Browsing(browser) = &self.screen {
            if !browser.focus_handle(cx).contains_focused(window, cx) {
                browser.focus_handle(cx).focus(window, cx);
            }
            return root.relative().child(browser.clone()).children(
                self.notice
                    .clone()
                    .map(|notice| self.notice_banner(notice, cx)),
            );
        }
        let body = match &self.screen {
            Screen::Welcome { problem } => self.welcome(problem.as_ref(), cx).into_any_element(),
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
        root.child(ui::bar())
            .child(div().flex_1().min_h_0().child(body))
    }
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
