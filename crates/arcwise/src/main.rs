//! Arcwise: a desktop lineup browser for Valorant.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod args;
mod browser;
mod drawn;
mod images;
mod pacing;
mod player;
mod settings;
mod theme;
mod trace;
mod ui;
mod update;
mod updater;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};

use crate::app::Arcwise;
use crate::args::Args;
use crate::settings::Settings;

/// The window's size when it opens.
const WINDOW_WIDTH: f32 = 1440.;
const WINDOW_HEIGHT: f32 = 900.;

/// Creates the named mutex the uninstaller looks for to tell that Arcwise is running; the name
/// must match `installer/arcwise.iss`. The handle is never closed, so the mutex lasts exactly as
/// long as the process.
#[cfg(windows)]
fn announce_running() {
    use windows::{Win32::System::Threading::CreateMutexW, core::w};
    // SAFETY: the name is a static NUL-terminated wide string and no security attributes are
    // passed.
    let _ = unsafe { CreateMutexW(None, false, w!("Arcwise.Running")) };
}

fn main() {
    #[cfg(windows)]
    announce_running();
    trace::init();
    let args = Args::parse(std::env::args_os().skip(1));
    let settings = Settings::load();
    theme::set_blur(settings.blur);
    theme::set_strength(settings.blur_strength);
    let preload = app::preload(&args, &settings);
    gpui_platform::application().run(move |cx: &mut App| {
        if trace::enabled() {
            cx.intercept_keystrokes(|event, _, _| {
                trace::input(|| format!("key {}", event.keystroke.key));
            })
            .detach();
        }
        browser::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_background: theme::window_background(),
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
            |_, cx| cx.new(|cx| Arcwise::new(args, settings, preload, cx)),
        )
        .expect("open the main window");
        cx.activate(true);
    });
}
