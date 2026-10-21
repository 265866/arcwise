//! Where the window was, so it opens there again: often on a second monitor beside the game.

use gpui::{App, Bounds, DisplayId, Pixels, Window, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

/// The window's place, in the logical pixels of the monitor it was on. Maximized, the size is
/// the one it goes back to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub maximized: bool,
    /// The monitor, by an id GPUI makes from Windows' name for it, such as `\\.\DISPLAY2`.
    /// Windows can renumber monitors after one is plugged in, and then the window opens
    /// centered instead.
    pub monitor: Option<String>,
}

impl Placement {
    /// Where `window` is, or nothing while it is minimized, when Windows reports it as neither
    /// where it was nor maximized.
    pub fn of(window: &Window, cx: &App) -> Option<Placement> {
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
            monitor: monitor
                .and_then(|id| cx.find_display(id))
                .and_then(|display| display.uuid().ok())
                .map(|uuid| uuid.to_string()),
        })
    }
}

/// The monitor Windows says `window` is on, which is the one its scale, and so the units of its
/// bounds, comes from. GPUI's own idea of the window's monitor can lag behind as it is dragged
/// between monitors at different scales. Nothing while the window is minimized.
#[cfg(windows)]
fn monitor(window: &Window) -> Option<Option<DisplayId>> {
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow};
    use windows::Win32::UI::WindowsAndMessaging::IsIconic;

    let Some(hwnd) = crate::win::hwnd(window) else {
        return Some(None);
    };
    // SAFETY: the handle is this live window's.
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return None;
    }
    // SAFETY: as above; the nearest monitor is always returned.
    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    // GPUI's display ids are the monitors' handles.
    Some(Some(DisplayId::new(monitor.0 as u64)))
}

#[cfg(not(windows))]
fn monitor(_: &Window) -> Option<Option<DisplayId>> {
    Some(None)
}

/// Where to open the window: where `saved` says when its monitor is still connected, kept within
/// that monitor's work area so the header is never out of reach, otherwise centered on the main
/// monitor.
pub fn restore(saved: Option<&Placement>, cx: &App) -> (WindowBounds, Option<DisplayId>) {
    let centered = || {
        let size = size(px(crate::WINDOW_WIDTH), px(crate::WINDOW_HEIGHT));
        (
            WindowBounds::Windowed(Bounds::centered(None, size, cx)),
            None,
        )
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
            point(px(saved.x), px(saved.y)),
            size(px(saved.width), px(saved.height)),
        ),
        display.visible_bounds(),
    );
    let bounds = if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    };
    (bounds, Some(display.id()))
}

/// `bounds` moved, and shrunk if they must be, to fit inside `area`.
fn within(bounds: Bounds<Pixels>, area: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = bounds.size.width.min(area.size.width);
    let height = bounds.size.height.min(area.size.height);
    // Not `clamp`, which panics when rounding leaves the far limit a hair before the near one,
    // as for a window exactly as tall as a scaled monitor's work area.
    let x = bounds.origin.x.min(area.right() - width).max(area.left());
    let y = bounds.origin.y.min(area.bottom() - height).max(area.top());
    Bounds::new(point(x, y), size(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    #[test]
    fn keeps_the_window_within_its_monitor() {
        // A second monitor to the left of the main one, with a taskbar along its bottom.
        let area = rect(-1920., 0., 1920., 1032.);
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
        // A full-height window on a 150% monitor whose work area starts at physical y = 40, where
        // rounding puts the bottom limit just above the top one.
        let area = rect(0., 40. / 1.5, 1706.6666, 2112. / 1.5);
        let tall = rect(0., 40. / 1.5, 1200., 2112. / 1.5);
        assert_eq!(within(tall, area).origin.y, area.top());
    }
}
