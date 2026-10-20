//! Arcwise: a desktop lineup browser for Valorant.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod args;
mod browser;
mod images;
mod pacing;
mod player;
mod settings;
mod theme;
mod trace;
mod ui;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::app::Arcwise;
use crate::args::Args;

fn main() {
    trace::init();
    let args = Args::parse(std::env::args_os().skip(1));
    gpui_platform::application().run(move |cx: &mut App| {
        if trace::enabled() {
            cx.intercept_keystrokes(|event, _, _| {
                trace::input(|| format!("key {}", event.keystroke.key));
            })
            .detach();
        }
        browser::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // GPUI slows windows without focus to about 30 frames a second, which made clips
                // stutter while the game had focus: exactly when they are watched.
                inactive_frame_interval: None,
                // Below this the map and panel no longer fit side by side.
                window_min_size: Some(size(px(1100.), px(700.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Arcwise".into()),
                    ..TitlebarOptions::default()
                }),
                ..WindowOptions::default()
            },
            |_, cx| cx.new(|cx| Arcwise::new(args, cx)),
        )
        .expect("open the main window");
        cx.activate(true);
    });
}
