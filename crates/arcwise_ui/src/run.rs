//! Starting Arcwise: the settings and theme it opens with, and its one window.

use arcwise_core::Args;
use arcwise_remote::remote;
use gpui::{App, Window, WindowOptions};

use crate::app::{self, Arcwise};
use crate::placement::{self, Restored};
use crate::settings::Settings;
use crate::shortcuts::{self, Shortcuts};
use crate::theme::{self, Mode};
use crate::{titlebar, trace};

/// The narrowest the window goes, in logical pixels. With [`MIN_HEIGHT`] it fits a 1080p laptop at
/// 150% scaling, which leaves about 1280 by 670 above the taskbar; any smaller and the map and the
/// lineup panel no longer fit side by side.
const MIN_WIDTH: f32 = 960.;
/// The shortest the window goes, in logical pixels; see [`MIN_WIDTH`].
const MIN_HEIGHT: f32 = 600.;

/// Runs Arcwise until its window closes. `args` is the command line as
/// [`Args::parse`] read it; when it could not be read, the window opens on why instead of a
/// library, rather than Arcwise exiting with nothing on screen.
///
/// Before the window opens this tells the uninstaller that Arcwise is running, starts the trace
/// that `ARCWISE_TRACE` asks for, loads the settings, takes the theme they choose (or Windows'
/// own, when they follow it, in case it changed while Arcwise was closed), and starts reading the
/// library the window opens on. A problem nothing on screen shows, such as a window that can't
/// open, goes to stderr through [`remote::report`].
pub fn run(args: Result<Args, String>) {
    if let Err(e) = arcwise_ffi::announce_running() {
        remote::report(format_args!(
            "could not tell the uninstaller that Arcwise is running: {e}"
        ));
    }
    trace::init();
    let settings = Settings::load();
    theme::set_blur(settings.blur);
    theme::set_strength(settings.blur_strength);
    theme::set_mode(settings.theme);
    theme::set_follow_system(settings.follow_system_theme);
    if settings.follow_system_theme
        && let Some(light) = arcwise_ffi::apps_use_light_theme()
    {
        theme::set_mode(if light { Mode::Light } else { Mode::Dark });
    }
    titlebar::init(settings.custom_title_bar);
    let preload = app::preload(&args, &settings);
    gpui_platform::application().run(move |cx: &mut App| {
        use gpui::AppContext as _;

        if trace::enabled() {
            cx.intercept_keystrokes(|event, _, _| {
                trace::input(|| format!("key {}", event.keystroke.key));
            })
            .detach();
        }
        shortcuts::apply(Shortcuts::with_changes(&settings.shortcuts), cx);
        let restored = placement::restore(settings.window.as_ref(), cx);
        let opened = cx.open_window(window_options(&restored), |window, cx| {
            theme::sync_system(window);
            sync_title_bar(window);
            cx.new(|cx| {
                cx.observe_window_appearance(window, |this: &mut Arcwise, window, _| {
                    if theme::sync_system(window) {
                        window.refresh();
                        this.theme_followed();
                    }
                    sync_title_bar(window);
                })
                .detach();
                cx.observe_window_bounds(window, |this: &mut Arcwise, window, cx| {
                    this.window_moved(window, cx);
                })
                .detach();
                Arcwise::new(args, settings, preload, cx)
            })
        });
        match opened {
            Ok(_) => cx.activate(true),
            Err(e) => {
                remote::report(format_args!("could not open the window: {e:#}"));
                cx.quit();
            }
        }
    });
}

/// How the window opens: where `restored` says, behind the theme's background, with the title bar
/// the settings choose. GPUI otherwise slows a window without focus to about 30 frames a second,
/// which made clips stutter while the game had focus, exactly when they are watched, so it keeps
/// its full frame rate.
fn window_options(restored: &Restored) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(restored.bounds),
        display_id: restored.display,
        window_background: theme::window_background(),
        inactive_frame_interval: None,
        window_min_size: Some(gpui::size(gpui::px(MIN_WIDTH), gpui::px(MIN_HEIGHT))),
        titlebar: Some(titlebar::options()),
        ..WindowOptions::default()
    }
}

/// Makes Windows' title bar dark or light as the theme is, or says why it can't.
fn sync_title_bar(window: &Window) {
    if let Err(e) = titlebar::sync_native(window) {
        remote::report(format_args!("could not theme the title bar: {e}"));
    }
}
