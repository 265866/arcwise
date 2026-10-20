//! Pieces of the browser drawn as views of their own. GPUI reuses a view's last frame, layout
//! and all, until the view changes, so while a clip plays its frames redraw the panel alone,
//! not the map and header beside it.

use gpui::{AnyElement, Context, Entity, IntoElement, Render, WeakEntity, Window, div, prelude::*};

use super::Browser;

pub(super) type Draw = fn(&Browser, &mut Window, &mut Context<Browser>) -> AnyElement;

pub(super) struct Part {
    browser: WeakEntity<Browser>,
    draw: Draw,
    /// Whether the last draw showed a picture that was still loading.
    waiting: bool,
}

impl Part {
    /// A part drawn by `draw`, redrawn whenever the browser changes.
    pub fn new(browser: &Entity<Browser>, draw: Draw, cx: &mut Context<Self>) -> Part {
        cx.observe(browser, |_, _, cx| cx.notify()).detach();
        Part {
            browser: browser.downgrade(),
            draw,
            waiting: false,
        }
    }

    pub fn waiting(&self) -> bool {
        self.waiting
    }
}

impl Render for Part {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draw = self.draw;
        let drawn = self.browser.update(cx, |browser, cx| {
            browser.images.update(cx, |images, _| images.begin());
            let element = draw(browser, window, cx);
            (element, browser.images.read(cx).waiting())
        });
        match drawn {
            Ok((element, waiting)) => {
                self.waiting = waiting;
                // Laid out on its own, the part fills the space the browser gave it.
                div().size_full().flex().flex_col().child(element)
            }
            Err(_) => div(),
        }
    }
}
