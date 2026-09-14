//! The window's root: finds and loads the library, then shows it.

use std::path::PathBuf;
use std::sync::Arc;

use arcwise_core::{Library, Start};
use gpui::{
    AsyncApp, ClickEvent, Context, Entity, Focusable, FontWeight, IntoElement, PathPromptOptions,
    SharedString, WeakEntity, Window, div, prelude::*, px,
};

use crate::args::Args;
use crate::browser::Browser;
use crate::settings::Settings;
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
}

impl Arcwise {
    pub fn new(args: Result<Args, String>, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let mut this = Arcwise {
            screen: Screen::Welcome { problem: None },
            settings,
            start: None,
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

    fn load(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.screen = Screen::Loading(path.clone());
        cx.notify();
        let task = cx.background_spawn({
            let path = path.clone();
            async move { Library::open(path) }
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(library) => {
                        let start = this.start.take().map(|args| start(&library, args));
                        let library = Arc::new(library);
                        let browser =
                            cx.new(|cx| Browser::new(library, start.unwrap_or_default(), cx));
                        this.screen = Screen::Browsing(browser);
                        if this.settings.library.as_ref() != Some(&path) {
                            this.settings.library = Some(path);
                            if let Err(e) = this.settings.save() {
                                eprintln!("arcwise: could not save settings: {e:#}");
                            }
                        }
                    }
                    Err(e) => {
                        this.screen = Screen::Welcome {
                            problem: Some(format!("{:#}", anyhow::Error::from(e)).into()),
                        };
                    }
                }
                cx.notify();
            })
        })
        .detach();
    }

    fn choose_folder(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
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
            return root.child(browser.clone());
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

/// Resolves the command line's names against the library; unknown names are ignored.
fn start(library: &Library, args: Args) -> Start {
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
