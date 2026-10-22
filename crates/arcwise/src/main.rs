//! Arcwise: a desktop lineup browser for Valorant.

// No console window behind the app in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod args;
mod browser;
mod drawn;
mod images;
mod pacing;
mod placement;
mod player;
mod settings;
mod shortcuts;
mod theme;
mod titlebar;
mod trace;
mod ui;
mod update;
mod updater;
#[cfg(windows)]
mod win;

use gpui::{App, WindowOptions, prelude::*, px, size};

use crate::app::Arcwise;
use crate::args::Args;
use crate::settings::Settings;

/// The window's size when it opens with nowhere saved to open it.
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
    theme::set_mode(settings.theme);
    titlebar::init(settings.custom_title_bar);
    let preload = app::preload(&args, &settings);
    gpui_platform::application().run(move |cx: &mut App| {
        if trace::enabled() {
            cx.intercept_keystrokes(|event, _, _| {
                trace::input(|| format!("key {}", event.keystroke.key));
            })
            .detach();
        }
        shortcuts::apply(shortcuts::Shortcuts::with_changes(&settings.shortcuts), cx);
        let (window_bounds, display_id) = placement::restore(settings.window.as_ref(), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(window_bounds),
                display_id,
                window_background: theme::window_background(),
                // GPUI slows windows without focus to about 30 frames a second, which made clips
                // stutter while the game had focus: exactly when they are watched.
                inactive_frame_interval: None,
                // Below this the map and panel no longer fit side by side.
                window_min_size: Some(size(px(1100.), px(700.))),
                titlebar: Some(titlebar::options()),
                ..WindowOptions::default()
            },
            |window, cx| {
                titlebar::sync_native(window);
                cx.new(|cx| {
                    cx.observe_window_appearance(window, |_, window, _| {
                        titlebar::sync_native(window);
                    })
                    .detach();
                    cx.observe_window_bounds(window, |this: &mut Arcwise, window, cx| {
                        this.window_moved(window, cx);
                    })
                    .detach();
                    Arcwise::new(args, settings, preload, cx)
                })
            },
        )
        .expect("open the main window");
        cx.activate(true);
    });
}
