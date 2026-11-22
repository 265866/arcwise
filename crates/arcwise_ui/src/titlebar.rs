//! The window's title bar: Windows' own, or Arcwise's header standing in for it, with its own
//! minimize, maximize and close buttons.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use arcwise_ffi::Appearance;
use gpui::{
    Div, InteractiveElement, IntoElement, ParentElement, Stateful, Styled, TitlebarOptions, Window,
    WindowControlArea,
};

use crate::theme::Mode;
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
const CLOSE_HOVER: u32 = 0xc4_2b_1c;

/// Sets whether this window draws its own title bar, as the settings ask.
pub(crate) fn init(custom: bool) {
    CUSTOM.store(custom, Ordering::Relaxed);
    CHOSEN.store(custom, Ordering::Relaxed);
}

/// Whether this window draws its own title bar.
#[must_use]
pub(crate) fn custom() -> bool {
    CUSTOM.load(Ordering::Relaxed)
}

/// Whether the settings ask for the window to draw its own title bar, which the next window gets.
#[must_use]
pub(crate) fn chosen() -> bool {
    CHOSEN.load(Ordering::Relaxed)
}

/// Asks for the next window to draw its own title bar, or not.
pub(crate) fn choose(custom: bool) {
    CHOSEN.store(custom, Ordering::Relaxed);
}

/// The title bar the window opens with.
#[must_use]
pub(crate) fn options() -> TitlebarOptions {
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
pub(crate) fn frame(bar: impl IntoElement, window: &Window) -> Div {
    let frame = gpui::div()
        .flex_none()
        .flex()
        .w_full()
        .h(ui::BAR_HEIGHT)
        .child(gpui::div().flex_1().min_w_0().h_full().child(bar));
    if !custom() {
        return frame;
    }
    let maximized = window.is_maximized();
    let active = window.is_window_active();
    frame.window_control_area(WindowControlArea::Drag).child(
        gpui::div()
            .flex()
            .h_full()
            .bg(theme::surface())
            .border_b_1()
            .border_color(theme::line())
            .child(button("minimize", WindowControlArea::Min, MINIMIZE, active))
            .child(button(
                "maximize",
                WindowControlArea::Max,
                if maximized { RESTORE } else { MAXIMIZE },
                active,
            ))
            .child(button("close", WindowControlArea::Close, CLOSE, active)),
    )
}

/// One of the window's buttons. Windows itself handles the press, as with its own title bar.
fn button(
    id: &'static str,
    area: WindowControlArea,
    icon: &'static [u8],
    active: bool,
) -> Stateful<Div> {
    let close = area == WindowControlArea::Close;
    gpui::div()
        .id(id)
        .group(id)
        .occlude()
        .window_control_area(area)
        .flex()
        .items_center()
        .justify_center()
        .w(gpui::px(46.))
        .h_full()
        .hover(|s| {
            if close {
                s.bg(gpui::rgb(CLOSE_HOVER))
            } else {
                s.bg(theme::hover())
            }
        })
        .child(
            gpui::svg()
                .data(icon)
                .size(gpui::px(10.))
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
///
/// # Errors
///
/// The error Windows gives when it refuses the change.
pub(crate) fn sync_native(window: &Window) -> io::Result<()> {
    let appearance = match theme::mode() {
        Mode::Dark => Appearance::Dark,
        Mode::Light => Appearance::Light,
    };
    arcwise_ffi::set_title_bar(window, appearance)
}
