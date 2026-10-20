//! The main screen: header, map and lineup panel, over one library.

mod board;
mod chooser;
mod follow;
mod header;
mod keys;
mod menu;
mod panel;
mod part;
mod slide;

pub use keys::bind as bind_keys;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use arcwise_core::{
    AgentId, Browse, LandingKey, Library, Lineup, LineupId, MapId, Side, Start, View,
};
use gpui::{
    Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Pixels,
    StyleRefinement, Window, div, prelude::*,
};

use crate::drawn::Drawn;
use crate::images::{Image, Images, Look, Picture};
use crate::player::Player;
use crate::ui;
use crate::updater::Updater;
use follow::Follow;
use part::Part;
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
    /// Whether the settings menu is open.
    menu_open: bool,
    /// What the settings menu says about updates.
    updater: Entity<Updater>,
    /// Whether the pointer is dragging along the blur strength track.
    strength_drag: bool,
    slide: Slide,
    /// The lineup in the panel: the selected one, or the last one while the panel slides away.
    shown: Option<LineupId>,
    /// The selected landing, the first and last shown, and the viewer's rows, when pictures were
    /// last fetched ahead.
    prefetched: Option<(Option<LandingKey>, LandingKey, LandingKey, u32)>,
    /// The map, side and map size whose other side was last fetched ahead.
    prefetched_map: Option<(MapId, Side, u32)>,
    /// The lineup whose clip was last readied ahead, with the last lineup shown and the viewer's
    /// rows then.
    prepared: Option<(LineupId, Option<LineupId>, u32)>,
    /// The choice last emitted as [`Chosen`].
    chosen: Option<Chosen>,
    /// The header and map, as views of their own; made on the first render.
    parts: Option<Parts>,
    /// Whether the last render showed a picture that was still loading.
    waiting: bool,
    /// The most rows the viewer shows, from the last render; see [`viewer_rows`].
    rows: u32,
    /// How many pixels across maps are drawn for the board; see [`map_across`].
    across: u32,
    /// How far open the panel is, from the last render.
    open: f32,
    /// Where the panel last placed the clip.
    clip_slot: follow::Slot,
    /// The selected landing's throws on the map, and the throw in the stand views.
    throws: Rc<Drawn>,
    stand_tile: Rc<Drawn>,
    stand_view: Rc<Drawn>,
}

impl EventEmitter<Chosen> for Browser {}

/// Emitted when the user asks to open a different library.
#[derive(Clone, Copy, Debug)]
pub struct ChangeLibrary;

impl EventEmitter<ChangeLibrary> for Browser {}

/// Emitted when the user turns background blur on or off, which [`crate::theme::blur`] already
/// shows.
#[derive(Clone, Copy, Debug)]
pub struct SetBlur(pub bool);

impl EventEmitter<SetBlur> for Browser {}

/// Emitted when the user lets go of the blur strength track, with where it stopped, from 0 to
/// 100.
#[derive(Clone, Copy, Debug)]
pub struct SetBlurStrength(pub u8);

impl EventEmitter<SetBlurStrength> for Browser {}

/// Emitted when the user turns automatic updates on or off, which the [`Updater`] already knows.
#[derive(Clone, Copy, Debug)]
pub struct SetAutoUpdate(pub bool);

impl EventEmitter<SetAutoUpdate> for Browser {}

impl Browser {
    /// A browser over `library`, opening at `start`, with `pictures` already decoded for it.
    pub fn new(
        library: Arc<Library>,
        start: Start,
        pictures: Vec<Picture>,
        updater: Entity<Updater>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&updater, |_, _, cx| cx.notify()).detach();
        let images = cx.new(|_| {
            let mut images = Images::default();
            images.seed(pictures);
            images
        });
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
            menu_open: false,
            updater,
            strength_drag: false,
            slide: Slide::closed(),
            shown: None,
            prefetched: None,
            prefetched_map: None,
            prepared: None,
            chosen: None,
            parts: None,
            waiting: false,
            rows: viewer_rows_for(crate::WINDOW_WIDTH, 1.),
            across: map_across_for(crate::WINDOW_WIDTH, crate::WINDOW_HEIGHT, 1.),
            open: 0.,
            clip_slot: Rc::default(),
            throws: Rc::default(),
            stand_tile: Rc::default(),
            stand_view: Rc::default(),
        };
        // Portraits load while the map is being picked, so the agent step opens with them.
        for agent in this.library.agents() {
            let path = this.library.portrait(&agent.id);
            this.images
                .update(cx, |images, cx| images.fetch(&path, Look::Plain, cx));
        }
        this
    }

    fn image(&self, path: &Path, cx: &mut Context<Self>) -> Image {
        self.images.update(cx, |images, cx| images.get(path, cx))
    }

    /// A lineup's aim picture, shrunk to the viewer's size.
    fn aim(&self, id: LineupId, cx: &mut Context<Self>) -> Image {
        let path = self.library.aim_image(id);
        let look = Look::Fit(self.rows);
        self.images
            .update(cx, |images, cx| images.get_as(&path, look, cx))
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

    /// Whether everything on screen has loaded: no picture is still decoding, and a clip in view
    /// is showing its frames.
    pub fn settled(&self, cx: &gpui::App) -> bool {
        !self.waiting
            && !self.player.read(cx).waiting()
            && self.parts.as_ref().is_none_or(|parts| {
                // A closed panel keeps whatever it waited for when last drawn.
                let panel = self.shown.is_some().then_some(&parts.panel);
                [&parts.header, &parts.map]
                    .into_iter()
                    .chain(panel)
                    .all(|part| !part.read(cx).waiting())
            })
    }

    /// Frees every image and frame this screen holds, before it is dropped. Call outside any
    /// window's update.
    pub fn release_all(&mut self, cx: &mut Context<Self>) {
        self.images.update(cx, |images, cx| images.clear(cx));
        self.player.update(cx, |player, cx| player.release_all(cx));
        for drawn in [&self.throws, &self.stand_tile, &self.stand_view] {
            drawn.release(cx);
        }
    }

    /// Fetches the other side's map, so the side switch shows it at once.
    fn prefetch_map(&mut self, cx: &mut Context<Self>) {
        let Some(map) = self.browse.map() else { return };
        let key = (map.clone(), self.browse.side(), self.across);
        if self.prefetched_map.as_ref() == Some(&key) {
            return;
        }
        let other = key.1.other();
        self.prefetched_map = Some(key);
        if self.browse.side_count(&self.library, other) == 0 {
            return;
        }
        let path = self.library.map_image(map, other);
        let look = Look::Across(self.across);
        self.images
            .update(cx, |images, cx| images.fetch(&path, look, cx));
    }

    /// A map's picture for the board, or one drawn at another size while that one loads.
    fn map_picture(&self, map: &MapId, side: Side, cx: &mut Context<Self>) -> Image {
        let path = self.library.map_image(map, side);
        let look = Look::Across(self.across);
        let image = self
            .images
            .update(cx, |images, cx| images.get_as(&path, look, cx));
        if !matches!(image, Image::Loading) {
            return image;
        }
        self.images.update(cx, |images, _| {
            MAP_SIZES
                .iter()
                .rev()
                .find_map(|&across| images.peek(&path, Look::Across(across)))
                .map_or(image, Image::Ready)
        })
    }

    /// The part of a lineup's map the viewer's stand view shows: the map at three times the
    /// viewer's width, around the stand spot.
    fn stand_window(&self, lineup: &Lineup) -> Look {
        let (width, height) = (self.rows * 16 / 9, self.rows);
        let stand = lineup.stand.at();
        Look::Window {
            across: (width as f32 * panel::STAND_ZOOM) as u32,
            x: (stand.x * 100.).round() as u32,
            y: (stand.y * 100.).round() as u32,
            width,
            height,
        }
    }

    /// Fetches the pictures the panel shows for a lineup.
    fn fetch_lineup(&self, id: LineupId, cx: &mut Context<Self>) {
        let mut pictures = vec![
            (self.library.start_still(id), Look::Plain),
            (self.library.end_still(id), Look::Plain),
            (self.library.aim_image(id), Look::Fit(self.rows)),
        ];
        if let Some(lineup) = self.library.lineup(id) {
            let path = self.library.map_image(&lineup.map, lineup.side);
            pictures.push((path, self.stand_window(lineup)));
        }
        self.images.update(cx, |images, cx| {
            for (path, look) in &pictures {
                images.fetch(path, *look, cx);
            }
        });
    }

    /// Fetches the pictures the keys can reach next, so stepping to them shows them at once.
    fn prefetch(&mut self, cx: &mut Context<Self>) {
        let landings = self.browse.landings();
        let (Some(first), Some(last)) = (landings.first(), landings.last()) else {
            return;
        };
        let selected = self.browse.selected_landing().map(|g| g.key);
        let key = (selected, first.key, last.key, self.rows);
        if self.prefetched == Some(key) {
            return;
        }
        self.prefetched = Some(key);
        for id in ahead(&self.browse) {
            self.fetch_lineup(id, cx);
        }
    }

    /// Readies the clip most likely to play next: a stand spot or landing under the pointer, or
    /// else the landing the down key reaches.
    fn prepare_clip(&mut self, window: &Window, cx: &mut Context<Self>) {
        let landings = self.browse.landings();
        let next = || {
            let at = self
                .browse
                .selected_landing()
                .and_then(|g| landings.iter().position(|l| l.key == g.key))
                .map_or(0, |at| at + 1);
            Some(landings.get(at % landings.len().max(1))?.lineups[0])
        };
        let target = self
            .peek_lineup
            .or_else(|| Some(self.browse.landing(self.hovered?)?.lineups[0]))
            .or_else(next)
            .filter(|&id| Some(id) != self.shown);
        let Some(id) = target else { return };
        // The decoder lets a prepared clip go when another opens, so ask again after one does;
        // closing the panel only stops the clip, which keeps it.
        let rows = viewer_rows(window);
        let opened = self
            .shown
            .or(self.prepared.and_then(|(_, opened, _)| opened));
        let key = (id, opened, rows);
        if self.prepared == Some(key) {
            return;
        }
        self.prepared = Some(key);
        let path = self.library.clip(id);
        self.player.read(cx).prepare(path, rows);
    }

    /// Whether the viewer shows the clip, rather than a picture, and nothing covers it.
    fn clip_in_view(&self) -> bool {
        self.peek_view.unwrap_or(self.browse.view()) == View::Clip && self.lightbox.is_none()
    }

    /// Keeps the player on the shown lineup's clip, or stopped when the panel is closed.
    /// The player is held still while the viewer shows a picture instead of the clip, or the
    /// lightbox covers it, and frees the frames it replaced here because it is not always on
    /// screen to do so itself.
    fn sync_player(&self, window: &mut Window, cx: &mut Context<Self>) {
        let clip = self.shown.map(|id| (id, self.library.clip(id)));
        let max_height = viewer_rows(window);
        let visible = self.clip_in_view();
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
        self.images.update(cx, |images, _| images.begin());
        self.rows = viewer_rows(window);
        let rows = self.rows;
        self.images.update(cx, |images, _| images.fit_rows(rows));
        self.across = map_across(window);
        let selected = self.browse.selected_lineup();
        self.slide.set(selected.is_some(), now);
        let open = self.slide.at(now);
        self.open = open;
        if self.slide.moving(now) {
            window.request_animation_frame();
        }
        if selected.is_some() {
            self.shown = selected;
        } else if open == 0. {
            self.shown = None;
        }
        self.sync_player(window, cx);
        self.prefetch_map(cx);
        self.prefetch(cx);
        self.prepare_clip(window, cx);
        self.emit_choice(cx);
        let clip = self.shown.is_some() && self.clip_in_view();
        let parts = self.parts(cx);
        let element = div()
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
            .child(
                parts.header.cached(
                    StyleRefinement::default()
                        .w_full()
                        .h(ui::BAR_HEIGHT)
                        .flex_none(),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        parts
                            .map
                            .cached(StyleRefinement::default().flex_1().min_w_0().h_full()),
                    )
                    .when(self.shown.is_some(), |row| {
                        row.child(
                            parts.panel.cached(
                                StyleRefinement::default()
                                    .flex_none()
                                    .w(gpui::relative(open * 0.5))
                                    .h_full(),
                            ),
                        )
                    }),
            )
            .when(clip, |d| {
                let player = div()
                    .relative()
                    .size_full()
                    .opacity(open)
                    .child(self.player.clone())
                    .children(self.clip_note());
                d.child(Follow::new(self.clip_slot.clone(), player))
            })
            .children(self.menu_layer(cx))
            .children(self.chooser(window, cx))
            .children(self.lightbox(cx));
        self.waiting = self.images.read(cx).waiting();
        element
    }
}

/// The browser's parts that GPUI can reuse from the last frame.
#[derive(Clone)]
struct Parts {
    header: Entity<Part>,
    map: Entity<Part>,
    panel: Entity<Part>,
}

impl Browser {
    fn parts(&mut self, cx: &mut Context<Self>) -> Parts {
        let parts = self.parts.get_or_insert_with(|| {
            let browser = cx.entity();
            Parts {
                header: cx.new(|cx| {
                    Part::new(
                        &browser,
                        |b, window, cx| b.header(window, cx).into_any_element(),
                        cx,
                    )
                }),
                map: cx.new(|cx| {
                    Part::new(&browser, |b, _, cx| b.map_column(cx).into_any_element(), cx)
                }),
                panel: cx.new(|cx| {
                    Part::new(
                        &browser,
                        |b, _, cx| {
                            b.panel(b.open, cx)
                                .unwrap_or_else(|| div().into_any_element())
                        },
                        cx,
                    )
                }),
            }
        });
        parts.clone()
    }
}

impl Browser {
    /// The picture in the lightbox, fitted to the window; any click closes it.
    fn lightbox(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let path = self.lightbox.as_ref()?;
        // An aim picture shows at the viewer's size until the full size is ready.
        let image = self.image(path, cx).ready().or_else(|| {
            let look = Look::Fit(self.rows);
            self.images.update(cx, |images, _| images.peek(path, look))
        });
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

/// The lineups the keys reach next: every stand spot of the selected landing and the first of
/// the landings either side, or with nothing selected, the first of the first and last landings.
fn ahead(browse: &Browse) -> Vec<LineupId> {
    let landings = browse.landings();
    let n = landings.len();
    let selected = browse
        .selected_landing()
        .and_then(|g| landings.iter().position(|l| l.key == g.key));
    let ids: Vec<LineupId> = match selected {
        Some(at) => landings[at]
            .lineups
            .iter()
            .copied()
            .chain([
                landings[(at + 1) % n].lineups[0],
                landings[(at + n - 1) % n].lineups[0],
            ])
            .collect(),
        None if n > 0 => vec![landings[0].lineups[0], landings[n - 1].lineups[0]],
        None => Vec::new(),
    };
    // With one or two landings, the ones either side are the same.
    let mut unique = Vec::with_capacity(ids.len());
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

/// The pictures the browser first shows when it opens at `start`, and those its keys reach
/// next, to decode before it opens.
pub fn first_pictures(library: &Library, start: Start) -> Vec<(PathBuf, Look)> {
    let browse = Browse::new(library, start);
    let mut maps = Vec::new();
    let mut paths: Vec<PathBuf> = library
        .agents()
        .iter()
        .map(|agent| library.portrait(&agent.id))
        .collect();
    if browse.chooser() == Some(arcwise_core::Chooser::Map) {
        paths.extend(library.maps().iter().map(|map| library.map_card(&map.id)));
    }
    if let (Some(map), Some(agent)) = (browse.map(), browse.agent()) {
        paths.push(library.map_card(map));
        let side = browse.side();
        maps.push(library.map_image(map, side));
        if browse.side_count(library, side.other()) > 0 {
            maps.push(library.map_image(map, side.other()));
        }
        if let Some(agent) = library.agent(agent) {
            paths.extend(agent.abilities.iter().map(|ab| library.ability_icon(ab.id)));
        }
        for id in ahead(&browse) {
            paths.extend([library.start_still(id), library.end_still(id)]);
        }
    }
    let mut pictures: Vec<_> = paths.into_iter().map(|path| (path, Look::Plain)).collect();
    // At the window's first size; a display that scales it decodes them again at its own.
    let across = map_across_for(crate::WINDOW_WIDTH, crate::WINDOW_HEIGHT, 1.);
    pictures.extend(maps.into_iter().map(|path| (path, Look::Across(across))));
    let rows = viewer_rows_for(crate::WINDOW_WIDTH, 1.);
    pictures.extend(
        ahead(&browse)
            .into_iter()
            .map(|id| (library.aim_image(id), Look::Fit(rows))),
    );
    pictures
}

/// The sizes maps are drawn at for the board, in pixels across. Steps, so a resize seldom draws
/// them again.
const MAP_SIZES: [u32; 3] = [1024, 1536, 2048];

/// The size to draw maps at for the board: the smallest of [`MAP_SIZES`] that fills the largest
/// square the window could give it.
fn map_across(window: &Window) -> u32 {
    let viewport = window.viewport_size();
    map_across_for(
        f32::from(viewport.width),
        f32::from(viewport.height),
        window.scale_factor(),
    )
}

fn map_across_for(width: f32, height: f32, scale: f32) -> u32 {
    let most = width.min(height) * scale;
    MAP_SIZES
        .into_iter()
        .find(|&across| across as f32 >= most)
        .unwrap_or(MAP_SIZES[2])
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
    viewer_rows_for(
        f32::from(window.viewport_size().width),
        window.scale_factor(),
    )
}

/// [`viewer_rows`] for a window `width` wide at `scale`.
fn viewer_rows_for(width: f32, scale: f32) -> u32 {
    let width = width / 2. - 48.;
    let rows = (width * 9. / 16. * scale).max(360.);
    (rows / 90.).ceil() as u32 * 90
}

#[cfg(test)]
mod tests {
    #[test]
    fn maps_are_drawn_big_enough_for_the_window() {
        assert_eq!(super::map_across_for(1440., 900., 1.), 1024);
        assert_eq!(super::map_across_for(2560., 1440., 1.), 1536);
        assert_eq!(super::map_across_for(3840., 2160., 1.5), 2048);
    }

    #[test]
    fn plural_adds_an_s() {
        assert_eq!(super::plural(1, "lineup"), "1 lineup");
        assert_eq!(super::plural(0, "lineup"), "0 lineups");
        assert_eq!(super::plural(12, "lineup"), "12 lineups");
    }
}
