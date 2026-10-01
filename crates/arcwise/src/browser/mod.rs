//! The main screen: header, map and lineup panel, over one library.

mod board;
mod chooser;
mod header;
mod keys;
mod panel;

pub use keys::bind as bind_keys;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use arcwise_core::{Browse, LandingKey, Library, LineupId, Start, View};
use gpui::{
    Bounds, Context, Entity, FocusHandle, Focusable, IntoElement, Pixels, Window, div, prelude::*,
};

use crate::images::{Image, Images};
use crate::player::Player;

pub struct Browser {
    library: Arc<Library>,
    browse: Browse,
    images: Entity<Images>,
    focus: FocusHandle,
    /// Where the map and agent pickers are, for opening their dropdowns under them.
    map_picker: Rc<Cell<Bounds<Pixels>>>,
    agent_picker: Rc<Cell<Bounds<Pixels>>>,
    /// The space the map is fitted into.
    map_area: Rc<Cell<Bounds<Pixels>>>,
    /// The landing under the pointer, which shows its card.
    hovered: Option<LandingKey>,
    /// A lineup pointed at in the panel, whose throw the map highlights.
    peek_lineup: Option<LineupId>,
    /// A step tile pointed at, which the viewer previews.
    peek_view: Option<View>,
    player: Entity<Player>,
    /// A picture shown over the whole window.
    lightbox: Option<PathBuf>,
}

impl Browser {
    pub fn new(library: Arc<Library>, start: Start, cx: &mut Context<Self>) -> Self {
        let images = cx.new(|_| Images::default());
        cx.observe(&images, |_, _, cx| cx.notify()).detach();
        let browse = Browse::new(&library, start);
        let this = Browser {
            library,
            browse,
            images,
            focus: cx.focus_handle(),
            map_picker: Rc::default(),
            agent_picker: Rc::default(),
            map_area: Rc::default(),
            hovered: None,
            peek_lineup: None,
            peek_view: None,
            player: cx.new(|_| Player::new()),
            lightbox: None,
        };
        // Portraits load while the map is being picked, so the agent step opens with them.
        for agent in this.library.agents() {
            let path = this.library.portrait(&agent.id);
            this.images
                .update(cx, |images, cx| images.prefetch(&path, cx));
        }
        this
    }

    fn image(&self, path: &Path, cx: &mut Context<Self>) -> Image {
        self.images.update(cx, |images, cx| images.get(path, cx))
    }

    /// Keeps the player on the selected lineup's clip, or stopped when nothing is selected.
    fn sync_player(&self, cx: &mut Context<Self>) {
        let selected = self.browse.selected_lineup();
        let clip = selected.map(|id| (id, self.library.clip(id)));
        self.player.update(cx, |player, cx| match clip {
            Some((id, path)) => player.play(id, path, cx),
            None => player.stop(cx),
        });
    }
}

impl Focusable for Browser {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.images.update(cx, |images, _| images.release(window));
        self.sync_player(cx);
        div()
            .track_focus(&self.focus)
            .key_context(keys::CONTEXT)
            .on_action(cx.listener(Self::next_landing))
            .on_action(cx.listener(Self::previous_landing))
            .on_action(cx.listener(Self::next_stand))
            .on_action(cx.listener(Self::previous_stand))
            .on_action(cx.listener(Self::watch_clip))
            .on_action(cx.listener(Self::show_stand))
            .on_action(cx.listener(Self::show_aim))
            .on_action(cx.listener(Self::show_landing))
            .on_action(cx.listener(Self::toggle_pause))
            .on_action(cx.listener(Self::slow_motion))
            .on_action(cx.listener(Self::switch_side))
            .on_action(cx.listener(Self::close))
            .on_key_down(cx.listener(Self::number_key))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(self.header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.map_column(cx))
                    .children(self.panel(cx)),
            )
            .children(self.chooser(window, cx))
            .children(self.lightbox(cx))
    }
}

impl Browser {
    /// The picture in the lightbox, fitted to the window; any click closes it.
    fn lightbox(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let path = self.lightbox.as_ref()?;
        let image = self.image(path, cx).ready();
        Some(
            div()
                .id("lightbox")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .px(gpui::relative(0.05))
                .py(gpui::relative(0.05))
                .bg(crate::theme::alpha(gpui::black(), 0.87))
                .occlude()
                .cursor_pointer()
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.lightbox = None;
                        cx.notify();
                    }),
                )
                .children(image.map(|image| {
                    gpui::img(gpui::ImageSource::Render(image))
                        .size_full()
                        .object_fit(gpui::ObjectFit::Contain)
                })),
        )
    }
}

/// `n word`, with an `s` unless `n` is 1.
fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plural_adds_an_s() {
        assert_eq!(super::plural(1, "lineup"), "1 lineup");
        assert_eq!(super::plural(0, "lineup"), "0 lineups");
        assert_eq!(super::plural(12, "lineup"), "12 lineups");
    }
}
