//! Pieces of the browser drawn as views of their own. GPUI reuses a view's last frame, layout
//! and all, until the view changes, so while a clip plays its frames redraw the panel alone,
//! not the map and header beside it.

use gpui::{AnyElement, Context, Entity, IntoElement, Render, WeakEntity, Window};

use super::browser_view::BrowserView;

/// Draws one part of the browser.
pub(super) type Draw = fn(&BrowserView, &mut Window, &mut Context<'_, BrowserView>) -> AnyElement;

/// A piece of the browser drawn as a view of its own, redrawn whenever the browser changes.
pub(super) struct Part {
    browser: WeakEntity<BrowserView>,
    draw: Draw,
    /// Whether the last draw showed a picture that was still loading.
    waiting: bool,
}

impl Part {
    /// A part drawn by `draw`, redrawn whenever the browser changes.
    pub(super) fn new(
        browser: &Entity<BrowserView>,
        draw: Draw,
        cx: &mut Context<'_, Self>,
    ) -> Part {
        cx.observe(browser, |_, _, cx| cx.notify()).detach();
        Part {
            browser: browser.downgrade(),
            draw,
            waiting: false,
        }
    }

    /// Whether the last draw showed a picture that was still loading.
    pub(super) fn waiting(&self) -> bool {
        self.waiting
    }
}

impl Render for Part {
    /// Laid out on its own, the part fills the space the browser gave it.
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        use gpui::{ParentElement as _, Styled as _};

        let Some(browser) = self.browser.upgrade() else {
            return gpui::div();
        };
        let draw = self.draw;
        let (element, waiting) = browser.update(cx, |browser, cx| {
            browser.images.update(cx, |images, _| images.begin());
            let element = draw(browser, window, cx);
            (element, browser.images.read(cx).waiting())
        });
        self.waiting = waiting;
        gpui::div().size_full().flex().flex_col().child(element)
    }
}
