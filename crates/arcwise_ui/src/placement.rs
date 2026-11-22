//! Where the window was, so it opens there again: often on a second monitor beside the game.

use gpui::{App, Bounds, DisplayId, Pixels, Window, WindowBounds};
use serde::{Deserialize, Serialize};

/// The width the window opens at the first time, in logical pixels.
pub(crate) const WINDOW_WIDTH: f32 = 1440.;
/// The height the window opens at the first time, in logical pixels.
pub(crate) const WINDOW_HEIGHT: f32 = 900.;

/// The window's place, in the logical pixels of the monitor it was on. Maximized, the size is
/// the one it goes back to. The settings file keeps it under these field names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Placement {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
    pub(crate) maximized: bool,
    /// The monitor, by an id GPUI makes from Windows' name for it, such as `\\.\DISPLAY2`.
    /// Windows can renumber monitors after one is plugged in, and then the window opens
    /// centered instead.
    pub(crate) monitor: Option<String>,
}

impl Placement {
    /// Where `window` is, or nothing while it is minimized, when Windows reports it as neither
    /// where it was nor maximized.
    pub(crate) fn of(window: &Window, cx: &App) -> Option<Placement> {
        let monitor = monitor(window)?;
        let (bounds, maximized) = match window.window_bounds() {
            WindowBounds::Windowed(bounds) => (bounds, false),
            WindowBounds::Maximized(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, true),
        };
        Some(Placement {
            x: bounds.origin.x.as_f32(),
            y: bounds.origin.y.as_f32(),
            width: bounds.size.width.as_f32(),
            height: bounds.size.height.as_f32(),
            maximized,
            monitor: cx
                .find_display(monitor)
                .and_then(|display| display.uuid().ok())
                .map(|uuid| uuid.to_string()),
        })
    }
}

/// The monitor Windows says `window` is on, which is the one its scale, and so the units of its
/// bounds, comes from. GPUI's own idea of the window's monitor can lag behind as it is dragged
/// between monitors at different scales. GPUI's display ids are the monitors' handles. Nothing
/// while the window is minimized.
fn monitor(window: &Window) -> Option<DisplayId> {
    arcwise_ffi::monitor(window).map(DisplayId::new)
}

/// Where the window opens, and on which monitor.
#[derive(Debug)]
pub(crate) struct Restored {
    pub(crate) bounds: WindowBounds,
    /// The monitor, or `None` for the main one.
    pub(crate) display: Option<DisplayId>,
}

/// Where to open the window: where `saved` says when its monitor is still connected, kept within
/// that monitor's work area so the header is never out of reach, otherwise centered on the main
/// monitor.
pub(crate) fn restore(saved: Option<&Placement>, cx: &App) -> Restored {
    let centered = || {
        let size = gpui::size(gpui::px(WINDOW_WIDTH), gpui::px(WINDOW_HEIGHT));
        Restored {
            bounds: WindowBounds::Windowed(Bounds::centered(None, size, cx)),
            display: None,
        }
    };
    let Some(saved) = saved else {
        return centered();
    };
    let display = cx.displays().into_iter().find(|display| {
        let uuid = display.uuid().ok().map(|uuid| uuid.to_string());
        uuid.is_some() && uuid == saved.monitor
    });
    let Some(display) = display else {
        return centered();
    };
    let bounds = within(
        Bounds::new(
            gpui::point(gpui::px(saved.x), gpui::px(saved.y)),
            gpui::size(gpui::px(saved.width), gpui::px(saved.height)),
        ),
        display.visible_bounds(),
    );
    let bounds = if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    Restored {
        bounds,
        display: Some(display.id()),
    }
}

/// `bounds` moved, and shrunk if they must be, to fit inside `area`. Not by `clamp`, which panics
/// when rounding leaves the far limit a hair before the near one, as for a window exactly as tall
/// as a scaled monitor's work area.
fn within(bounds: Bounds<Pixels>, area: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = bounds.size.width.min(area.size.width);
    let height = bounds.size.height.min(area.size.height);
    let x = bounds.origin.x.min(area.right() - width).max(area.left());
    let y = bounds.origin.y.min(area.bottom() - height).max(area.top());
    Bounds::new(gpui::point(x, y), gpui::size(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(
            gpui::point(gpui::px(x), gpui::px(y)),
            gpui::size(gpui::px(width), gpui::px(height)),
        )
    }

    #[test]
    fn keeps_the_window_within_its_monitor() {
        let left_monitor_above_a_taskbar = rect(-1920., 0., 1920., 1032.);
        let area = left_monitor_above_a_taskbar;
        let inside = rect(-1500., 100., 1200., 800.);
        assert_eq!(within(inside, area), inside, "already inside, it stays put");
        assert_eq!(
            within(rect(-1500., -60., 1200., 800.), area),
            rect(-1500., 0., 1200., 800.),
            "the header above the top comes down"
        );
        assert_eq!(
            within(rect(300., 500., 1200., 800.), area),
            rect(-1200., 232., 1200., 800.),
            "off the right and bottom edges, it moves back in"
        );
        assert_eq!(
            within(rect(-1920., 0., 2560., 1440.), area),
            area,
            "larger than the monitor, it shrinks to fit"
        );
        let work_area_at_150_percent_from_physical_y_40 =
            rect(0., 40. / 1.5, 1706.6666, 2112. / 1.5);
        let area = work_area_at_150_percent_from_physical_y_40;
        let tall = rect(0., 40. / 1.5, 1200., 2112. / 1.5);
        assert_eq!(
            within(tall, area).origin.y,
            area.top(),
            "a full-height window, where rounding puts the bottom limit just above the top one"
        );
    }

    #[test]
    fn the_settings_file_keeps_the_placement_as_before() {
        let saved = r#"{"x":-1500.0,"y":100.0,"width":1200.0,"height":800.0,"maximized":true,"monitor":"abc"}"#;
        let placement: Placement = serde_json::from_str(saved).expect("a saved placement loads");
        assert_eq!(
            placement,
            Placement {
                x: -1500.,
                y: 100.,
                width: 1200.,
                height: 800.,
                maximized: true,
                monitor: Some("abc".to_owned()),
            }
        );
        assert_eq!(
            serde_json::to_string(&placement).expect("a placement serializes"),
            saved
        );
    }
}
