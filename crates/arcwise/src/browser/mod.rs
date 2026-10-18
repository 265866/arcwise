//! The main screen: header, map and lineup panel, over one library.

mod board;
mod chooser;
mod header;
mod keys;
mod panel;
mod slide;

pub use keys::bind as bind_keys;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use arcwise_core::{AgentId, Browse, LandingKey, Library, LineupId, MapId, Side, Start, View};
use gpui::{
    Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Pixels, Window,
    div, prelude::*,
};

use crate::images::{Image, Images};
use crate::player::Player;
use slide::Slide;

/// Emitted when the map, agent or side on screen changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    pub map: MapId,
    pub agent: AgentId,
    pub side: Side,
}

pub struct Browser {
    library: Arc<Library>,
    browse: Browse,
    images: Entity<Images>,
    focus: FocusHandle,
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
    slide: Slide,
    /// The lineup in the panel: the selected one, or the last one while the panel slides away.
    shown: Option<LineupId>,
    /// The landing whose neighbours' stills were last fetched ahead.
    prefetched: Option<LandingKey>,
    /// The choice last emitted as [`Chosen`].
    chosen: Option<Chosen>,
}

impl EventEmitter<Chosen> for Browser {}

/// Emitted when the user asks to open a different library.
#[derive(Clone, Copy, Debug)]
pub struct ChangeLibrary;

impl EventEmitter<ChangeLibrary> for Browser {}

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
            map_area: Rc::default(),
            hovered: None,
            peek_lineup: None,
            peek_view: None,
            player: cx.new(|_| Player::new()),
            lightbox: None,
            slide: Slide::closed(),
            shown: None,
            prefetched: None,
            chosen: None,
        };
        // Portraits load while the map is being picked, so the agent step opens with them.
        for agent in this.library.agents() {
            let path = this.library.portrait(&agent.id);
            this.images.update(cx, |images, cx| images.get(&path, cx));
        }
        this
    }

    fn image(&self, path: &Path, cx: &mut Context<Self>) -> Image {
        self.images.update(cx, |images, cx| images.get(path, cx))
    }

    fn emit_choice(&mut self, cx: &mut Context<Self>) {
        let (Some(map), Some(agent)) = (self.browse.map(), self.browse.agent()) else {
            return;
        };
        if !self.browse.can_close(&self.library) {
            return;
        }
        let chosen = Chosen {
            map: map.clone(),
            agent: agent.clone(),
            side: self.browse.side(),
        };
        if self.chosen.as_ref() != Some(&chosen) {
            self.chosen = Some(chosen.clone());
            cx.emit(chosen);
        }
    }

    /// Frees every image and frame this screen holds, before it is dropped. Call outside any
    /// window's update.
    pub fn release_all(&mut self, cx: &mut Context<Self>) {
        self.images.update(cx, |images, cx| images.clear(cx));
        self.player.update(cx, |player, cx| player.release_all(cx));
    }

    /// Fetches the pictures the keys can reach next: every stand spot of the selected landing,
    /// and the first of the landings either side, so stepping through them shows them at once.
    fn prefetch(&mut self, cx: &mut Context<Self>) {
        let landings = self.browse.landings();
        let Some(at) = self
            .browse
            .selected_landing()
            .and_then(|g| landings.iter().position(|l| l.key == g.key))
        else {
            return;
        };
        if self.prefetched == Some(landings[at].key) {
            return;
        }
        self.prefetched = Some(landings[at].key);
        let n = landings.len();
        let ids: Vec<LineupId> = landings[at]
            .lineups
            .iter()
            .copied()
            .chain([
                landings[(at + 1) % n].lineups[0],
                landings[(at + n - 1) % n].lineups[0],
            ])
            .collect();
        let mut paths: Vec<PathBuf> = ids
            .into_iter()
            .flat_map(|id| [self.library.start_still(id), self.library.end_still(id)])
            .collect();
        // Aim pictures are large; only those of the landing on screen.
        paths.extend(
            landings[at]
                .lineups
                .iter()
                .map(|&id| self.library.aim_image(id)),
        );
        self.images.update(cx, |images, cx| {
            for path in &paths {
                images.get(path, cx);
            }
        });
    }

    /// Keeps the player on the shown lineup's clip, or stopped when the panel is closed.
    /// The player is held still while the viewer shows a picture instead of the clip, or the
    /// lightbox covers it, and frees the frames it replaced here because it is not always on
    /// screen to do so itself.
    fn sync_player(&self, window: &mut Window, cx: &mut Context<Self>) {
        let clip = self.shown.map(|id| (id, self.library.clip(id)));
        let max_height = viewer_rows(window);
        let visible =
            self.peek_view.unwrap_or(self.browse.view()) == View::Clip && self.lightbox.is_none();
        self.player.update(cx, |player, cx| {
            match clip {
                Some((id, path)) => player.play(id, path, max_height, cx),
                None => player.stop(cx),
            }
            player.set_visible(visible, cx);
            player.release(window);
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
        let now = Instant::now();
        let selected = self.browse.selected_lineup();
        self.slide.set(selected.is_some(), now);
        let open = self.slide.at(now);
        if self.slide.moving(now) {
            window.request_animation_frame();
        }
        if selected.is_some() {
            self.shown = selected;
        } else if open == 0. {
            self.shown = None;
        }
        self.sync_player(window, cx);
        self.prefetch(cx);
        self.emit_choice(cx);
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
            .child(self.header(window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.map_column(cx))
                    .children(self.panel(open, cx)),
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

/// The most device pixel rows the viewer can show once the panel is open: half the window,
/// less the panel's padding, at 16:9. Rounded up to a multiple of 90 rows (a 16th of 1440p),
/// so small changes to the window do not reopen clips at a new size.
fn viewer_rows(window: &Window) -> u32 {
    let width = f32::from(window.viewport_size().width) / 2. - 48.;
    let rows = (width * 9. / 16. * window.scale_factor()).max(360.);
    (rows / 90.).ceil() as u32 * 90
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
