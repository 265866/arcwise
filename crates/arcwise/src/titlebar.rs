//! The window's title bar: Windows' own, or Arcwise's header standing in for it, with its own
//! minimize, maximize and close buttons.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    Div, Stateful, TitlebarOptions, Window, WindowControlArea, div, prelude::*, px, rgb, svg,
};

use crate::{theme, ui};

/// Whether this window draws its own title bar. It is fixed when the window opens; GPUI cannot
/// change it on a live window.
static CUSTOM: AtomicBool = AtomicBool::new(true);
/// Whether the settings ask for one, which the next window gets.
static CHOSEN: AtomicBool = AtomicBool::new(true);

const MINIMIZE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M0 5.5h10" stroke="#000"/></svg>"##;
const MAXIMIZE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="#000"/></svg>"##;
const RESTORE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M2.5 2.5V0.5h7v7h-2M0.5 2.5h7v7h-7z" fill="none" stroke="#000"/></svg>"##;
const CLOSE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><path d="M0.5 0.5l9 9M9.5 0.5l-9 9" stroke="#000"/></svg>"##;

/// Windows' own red for a close button under the pointer.
const CLOSE_HOVER: u32 = 0xc42b1c;

/// Sets whether this window draws its own title bar, as the settings ask.
pub fn init(custom: bool) {
    CUSTOM.store(custom, Ordering::Relaxed);
    CHOSEN.store(custom, Ordering::Relaxed);
}

pub fn custom() -> bool {
    CUSTOM.load(Ordering::Relaxed)
}

pub fn chosen() -> bool {
    CHOSEN.load(Ordering::Relaxed)
}

pub fn choose(custom: bool) {
    CHOSEN.store(custom, Ordering::Relaxed);
}

pub fn options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("Arcwise".into()),
        appears_transparent: custom(),
        ..TitlebarOptions::default()
    }
}

/// `bar`, a header [`ui::BAR_HEIGHT`] tall, standing in for the title bar when the window draws
/// its own: its empty parts move the window, and it ends in the window's buttons. Its own buttons
/// must `occlude` so a press on one is not taken for a drag.
///
/// GPUI drops the window's drag and button areas inside a cached view when it reuses the view's
/// last frame, so they go on this wrapper, outside any cache.
pub fn frame(bar: impl IntoElement, window: &Window) -> Div {
    let frame = div()
        .flex_none()
        .flex()
        .w_full()
        .h(ui::BAR_HEIGHT)
        .child(div().flex_1().min_w_0().h_full().child(bar));
    if !custom() {
        return frame;
    }
    let maximized = window.is_maximized();
    let active = window.is_window_active();
    frame.window_control_area(WindowControlArea::Drag).child(
        div()
            .flex()
            .h_full()
            .bg(theme::surface())
            .border_b_1()
            .border_color(theme::line())
            .child(button(WindowControlArea::Min, MINIMIZE, active))
            .child(button(
                WindowControlArea::Max,
                if maximized { RESTORE } else { MAXIMIZE },
                active,
            ))
            .child(button(WindowControlArea::Close, CLOSE, active)),
    )
}

/// One of the window's buttons. Windows itself handles the press, as with its own title bar.
fn button(area: WindowControlArea, icon: &'static [u8], active: bool) -> Stateful<Div> {
    let id = match area {
        WindowControlArea::Min => "minimize",
        WindowControlArea::Max => "maximize",
        WindowControlArea::Close => "close",
        WindowControlArea::Drag => "drag",
    };
    let close = area == WindowControlArea::Close;
    div()
        .id(id)
        .group(id)
        .occlude()
        .window_control_area(area)
        .flex()
        .items_center()
        .justify_center()
        .w(px(46.))
        .h_full()
        .hover(|s| {
            if close {
                s.bg(rgb(CLOSE_HOVER))
            } else {
                s.bg(theme::hover())
            }
        })
        .child(
            svg()
                .data(icon)
                .size(px(10.))
                .text_color(if active {
                    theme::text()
                } else {
                    theme::faint()
                })
                .group_hover(id, |s| {
                    if close {
                        s.text_color(gpui::white())
                    } else {
                        s.text_color(theme::text())
                    }
                }),
        )
}

/// Makes Windows' own title bar, and the window's border, dark or light as the theme is.
/// Windows otherwise follows the system's theme, and GPUI sets it again when that changes.
#[cfg(windows)]
pub fn sync_native(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    use windows::core::BOOL;

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let dark = BOOL::from(theme::mode() == theme::Mode::Dark);
    // SAFETY: the handle is this live window's, and the attribute is a BOOL read from `dark`
    // for the call's duration.
    let _ = unsafe {
        DwmSetWindowAttribute(
            HWND(handle.hwnd.get() as *mut _),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            size_of::<BOOL>() as u32,
        )
    };
}

#[cfg(not(windows))]
pub fn sync_native(_: &Window) {}
