//! Arcwise: a desktop lineup browser for Valorant.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod args;
mod settings;
mod theme;
mod ui;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::app::Arcwise;
use crate::args::Args;

fn main() {
    let args = Args::parse(std::env::args_os().skip(1));
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
