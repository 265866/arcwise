//! The main screen: header, map and lineup panel, over one library.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use arcwise_core::{
    AbilityId, Browse, Chooser, LandingKey, Library, Lineup, LineupId, MapId, Side, Start, View,
};
use arcwise_remote::remote;
use arcwise_remote::sources::Status;
use gpui::{
    App, AppContext, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, ImageSource,
    InteractiveElement, IntoElement, MouseButton, ObjectFit, ParentElement, Pixels, Render,
    StyleRefinement, Styled, Subscription, Task, Window,
};

use super::events::{
    AddLibraryFolder, Chosen, Relaunch, SetAutoUpdate, SetBlur, SetBlurStrength, SetCustomTitleBar,
    SetLibraries, SetReopenLast, SetShortcuts, SetStartSpeed, SetTheme,
};
use super::follow::{Follow, Slot};
use super::keyboard::{Conflict, EndedBy, Recording};
use super::libraries::Libraries;
use super::panel;
use super::part::Part;
use super::slide::Slide;
use super::tab::Tab;
use crate::downloads::Downloads;
use crate::drawn::Drawn;
use crate::images::{Image, Images, Look, Picture, Wanted};
use crate::pacing::Speed;
use crate::player::Player;
use crate::settings::{LibrarySource, Settings};
use crate::theme::Mode;
use crate::updater::Updater;
use crate::{placement, shortcuts, theme, titlebar, ui};

/// The main screen over one library: the header, the map, the lineup panel, and the choosers,
/// settings window and lightbox drawn over them.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about the screen (a window open, a saved setting, a \
              hint shown, a drag under way, a picture loading) that changes on its own, not one \
              state an enum could name"
)]
pub(crate) struct BrowserView {
    pub(super) library: Library,
    pub(super) browse: Browse,
    pub(super) images: Entity<Images>,
    focus: FocusHandle,
    /// The space the map is fitted into.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the measuring canvas GPUI owns for one frame writes the bounds the map is \
                      sized by in the next"
        )
    )]
    pub(super) map_area: Rc<Cell<Bounds<Pixels>>>,
    /// The landing under the pointer, which shows its card.
    pub(super) hovered: Option<LandingKey>,
    /// A lineup pointed at in the panel, whose throw the map highlights.
    pub(super) peek_lineup: Option<LineupId>,
    /// A step tile pointed at, which the viewer previews.
    pub(super) peek_view: Option<View>,
    pub(super) player: Entity<Player>,
    /// A picture shown over the whole window.
    pub(super) lightbox: Option<PathBuf>,
    /// Whether the settings window is open.
    pub(super) settings_open: bool,
    /// The settings window's tab, kept while it is closed to open on again.
    pub(super) settings_tab: Tab,
    /// Whether Arcwise opens where it was left, as the settings say.
    pub(super) reopen_last: bool,
    /// The speed clips play at when Arcwise opens, as the settings say.
    pub(super) start_speed: Speed,
    /// The shortcut being chosen in the Keyboard tab.
    pub(super) recording: Option<Recording>,
    /// A shortcut chosen that another command already has, waiting for a yes or no.
    pub(super) conflict: Option<Conflict>,
    /// The key press that last ended choosing a shortcut, swallowed while Windows repeats it.
    pub(super) ended_by: Option<EndedBy>,
    /// Hands key presses to [`BrowserView::recorded`] while a shortcut is being chosen.
    _intercept: Subscription,
    /// Calls [`BrowserView::activation_changed`]; made on the first render, which has the window.
    activation: Option<Subscription>,
    /// Redraws the Updates tab each minute while it says how long ago Arcwise checked.
    pub(super) minutes: Option<Task<()>>,
    /// What the settings window says about updates.
    pub(super) updater: Entity<Updater>,
    /// The settings' list of libraries.
    pub(super) library_list: Vec<LibrarySource>,
    /// How each library in the list fared when last read.
    pub(super) library_statuses: Vec<Status>,
    pub(super) downloads: Entity<Downloads>,
    /// The library on the web whose maps to keep offline the settings show.
    pub(super) offline_open: Option<String>,
    /// Whether "Paste address" was last clicked without a web address on the clipboard.
    pub(super) paste_hint: bool,
    /// Whether the pointer is dragging along the blur strength track.
    pub(super) strength_drag: bool,
    slide: Slide,
    /// The lineup in the panel: the selected one, or the last one while the panel slides away.
    pub(super) shown: Option<LineupId>,
    /// What the pictures the keys reach were last fetched ahead for.
    prefetched: Option<Prefetched>,
    /// What the other side's map was last fetched ahead for.
    prefetched_map: Option<PrefetchedMap>,
    /// The clip last readied ahead, and for what.
    prepared: Option<Prepared>,
    /// The choice last emitted as [`Chosen`].
    chosen: Option<Chosen>,
    /// The header and map, as views of their own; made on the first render.
    parts: Option<Parts>,
    /// Whether the last render showed a picture that was still loading.
    waiting: bool,
    /// The most rows the viewer shows, from the last render; see [`viewer_rows`].
    pub(super) rows: u32,
    /// How many pixels across maps are drawn for the board; see [`map_across`].
    pub(super) across: u32,
    /// How far open the panel is, from the last render.
    open: f32,
    /// Where the panel last placed the clip.
    pub(super) clip_slot: Slot,
    /// The selected landing's throws on the map.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the canvas GPUI owns for one frame paints through the picture the browser \
                      keeps across frames, to draw the lines again only when they change"
        )
    )]
    pub(super) throws: Rc<Drawn>,
    /// The throw in the stand view's step tile.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the canvas GPUI owns for one frame paints through the picture the browser \
                      keeps across frames, to draw the lines again only when they change"
        )
    )]
    pub(super) stand_tile: Rc<Drawn>,
    /// The throw in the viewer's stand view.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the canvas GPUI owns for one frame paints through the picture the browser \
                      keeps across frames, to draw the lines again only when they change"
        )
    )]
    pub(super) stand_view: Rc<Drawn>,
}

/// What the pictures the keys reach next were fetched ahead for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefetched {
    selected: Option<LandingKey>,
    first: LandingKey,
    last: LandingKey,
    /// The viewer's rows.
    rows: u32,
}

/// What the other side's map was fetched ahead for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PrefetchedMap {
    map: MapId,
    side: Side,
    /// The size maps are drawn at.
    across: u32,
}

/// The clip readied ahead, with what decided it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prepared {
    lineup: LineupId,
    /// The last lineup shown then.
    opened: Option<LineupId>,
    /// The viewer's rows then.
    rows: u32,
}

impl EventEmitter<Chosen> for BrowserView {}
impl EventEmitter<AddLibraryFolder> for BrowserView {}
impl EventEmitter<SetLibraries> for BrowserView {}
impl EventEmitter<SetBlur> for BrowserView {}
impl EventEmitter<SetBlurStrength> for BrowserView {}
impl EventEmitter<SetAutoUpdate> for BrowserView {}
impl EventEmitter<SetTheme> for BrowserView {}
impl EventEmitter<SetCustomTitleBar> for BrowserView {}
impl EventEmitter<SetShortcuts> for BrowserView {}
impl EventEmitter<SetReopenLast> for BrowserView {}
impl EventEmitter<SetStartSpeed> for BrowserView {}
impl EventEmitter<Relaunch> for BrowserView {}

impl BrowserView {
    /// A browser over `library`, opening at `start`, with `pictures` already decoded for it. Every
    /// agent's portrait starts loading at once, so the agent step of the first-run dialog opens
    /// with them while the map is still being picked.
    pub(crate) fn new(
        library: Library,
        start: Start,
        pictures: Vec<Picture>,
        updater: Entity<Updater>,
        libraries: Libraries,
        settings: &Settings,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        cx.observe(&updater, |_, _, cx| cx.notify()).detach();
        cx.observe(&libraries.downloads, |_, _, cx| cx.notify())
            .detach();
        let images = cx.new(|_| {
            let mut images = Images::default();
            images.seed(pictures);
            images
        });
        cx.observe(&images, |_, _, cx| cx.notify()).detach();
        let browse = Browse::new(&library, start);
        let browser = cx.weak_entity();
        let intercept = cx.intercept_keystrokes(move |event, _, cx| {
            if let Some(browser) = browser.upgrade() {
                browser.update(cx, |browser, cx| browser.recorded(&event.keystroke, cx));
            }
        });
        let this = BrowserView {
            library,
            browse,
            images,
            focus: cx.focus_handle(),
            map_area: Rc::default(),
            hovered: None,
            peek_lineup: None,
            peek_view: None,
            player: cx.new(|cx| {
                let mut player = Player::new();
                player.set_speed(settings.start_speed, cx);
                player
            }),
            lightbox: None,
            settings_open: libraries.reopen.is_some(),
            settings_tab: libraries.reopen.unwrap_or_default(),
            reopen_last: settings.reopen_last,
            start_speed: settings.start_speed,
            recording: None,
            conflict: None,
            ended_by: None,
            _intercept: intercept,
            activation: None,
            minutes: None,
            updater,
            library_list: libraries.list,
            library_statuses: libraries.statuses,
            downloads: libraries.downloads,
            offline_open: None,
            paste_hint: false,
            strength_drag: false,
            slide: Slide::closed(),
            shown: None,
            prefetched: None,
            prefetched_map: None,
            prepared: None,
            chosen: None,
            parts: None,
            waiting: false,
            rows: viewer_rows_for(placement::WINDOW_WIDTH, 1.),
            across: map_across_for(placement::WINDOW_WIDTH, placement::WINDOW_HEIGHT, 1.),
            open: 0.,
            clip_slot: Rc::default(),
            throws: Rc::default(),
            stand_tile: Rc::default(),
            stand_view: Rc::default(),
        };
        for agent in this.library.agents() {
            let path = this.library.portrait(&agent.id);
            this.images
                .update(cx, |images, cx| images.fetch(&path, Look::Plain, cx));
        }
        this
    }

    pub(super) fn image(&self, path: &Path, cx: &mut Context<'_, Self>) -> Image {
        self.images.update(cx, |images, cx| images.get(path, cx))
    }

    /// An ability's icon for a chip in the theme's colors. The icons are white, which does not
    /// show on the light theme, so there they are black.
    pub(super) fn chip_icon(&self, ability: AbilityId, cx: &mut Context<'_, Self>) -> Image {
        let look = match theme::mode() {
            Mode::Dark => Look::Plain,
            Mode::Light => Look::Silhouette,
        };
        let path = self.library.ability_icon(ability);
        self.images
            .update(cx, |images, cx| images.get_as(&path, look, cx))
    }

    /// A lineup's aim picture, shrunk to the viewer's size.
    pub(super) fn aim(&self, id: LineupId, cx: &mut Context<'_, Self>) -> Image {
        let path = self.library.aim_image(id);
        let look = Look::Fit(self.rows);
        self.images
            .update(cx, |images, cx| images.get_as(&path, look, cx))
    }

    fn emit_choice(&mut self, cx: &mut Context<'_, Self>) {
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
    /// is showing its frames. A closed panel keeps whatever it waited for when last drawn, so it
    /// counts only while it shows a lineup.
    pub(crate) fn settled(&self, cx: &App) -> bool {
        !self.waiting
            && !self.player.read(cx).waiting()
            && self.parts.as_ref().is_none_or(|parts| {
                let panel = self.shown.is_some().then_some(&parts.panel);
                [&parts.header, &parts.map]
                    .into_iter()
                    .chain(panel)
                    .all(|part| !part.read(cx).waiting())
            })
    }

    /// Frees every image and frame this screen holds, before it is dropped. Call outside any
    /// window's update.
    pub(crate) fn release_all(&mut self, cx: &mut Context<'_, Self>) {
        self.images.update(cx, |images, cx| images.clear(cx));
        self.player.update(cx, |player, cx| player.release_all(cx));
        for drawn in [&self.throws, &self.stand_tile, &self.stand_view] {
            drawn.release(cx);
        }
    }

    /// Fetches the other side's map, so the side switch shows it at once.
    fn prefetch_map(&mut self, cx: &mut Context<'_, Self>) {
        let Some(map) = self.browse.map() else { return };
        let key = PrefetchedMap {
            map: map.clone(),
            side: self.browse.side(),
            across: self.across,
        };
        if self.prefetched_map.as_ref() == Some(&key) {
            return;
        }
        let other = key.side.other();
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
    pub(super) fn map_picture(&self, map: &MapId, side: Side, cx: &mut Context<'_, Self>) -> Image {
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
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "the viewer's width is a few thousand pixels at most and the stand spot is a \
                  percentage of the map, so each value fits a u32 once rounded"
    )]
    pub(super) fn stand_window(&self, lineup: &Lineup) -> Look {
        let width = self.rows * 16 / 9;
        let height = self.rows;
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
    pub(super) fn fetch_lineup(&self, id: LineupId, cx: &mut Context<'_, Self>) {
        let mut pictures = vec![
            Wanted {
                path: self.library.start_still(id),
                look: Look::Plain,
            },
            Wanted {
                path: self.library.end_still(id),
                look: Look::Plain,
            },
            Wanted {
                path: self.library.aim_image(id),
                look: Look::Fit(self.rows),
            },
        ];
        if let Some(lineup) = self.library.lineup(id) {
            pictures.push(Wanted {
                path: self.library.map_image(&lineup.map, lineup.side),
                look: self.stand_window(lineup),
            });
        }
        self.images.update(cx, |images, cx| {
            for wanted in &pictures {
                images.fetch(&wanted.path, wanted.look, cx);
            }
        });
    }

    /// Fetches the pictures the keys can reach next, so stepping to them shows them at once.
    fn prefetch(&mut self, cx: &mut Context<'_, Self>) {
        let landings = self.browse.landings();
        let (Some(first), Some(last)) = (landings.first(), landings.last()) else {
            return;
        };
        let key = Prefetched {
            selected: self.browse.selected_landing().map(|g| g.key),
            first: first.key,
            last: last.key,
            rows: self.rows,
        };
        if self.prefetched == Some(key) {
            return;
        }
        self.prefetched = Some(key);
        for id in ahead(&self.browse) {
            self.fetch_lineup(id, cx);
        }
    }

    /// Readies the clip most likely to play next: a stand spot or landing under the pointer, or
    /// else the landing the down key reaches. The decoder lets a prepared clip go when another
    /// opens, so this asks again after one does; closing the panel only stops the clip, which
    /// keeps it.
    fn prepare_clip(&mut self, window: &Window, cx: &mut Context<'_, Self>) {
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
        let rows = viewer_rows(window);
        let opened = self
            .shown
            .or(self.prepared.and_then(|prepared| prepared.opened));
        let key = Prepared {
            lineup: id,
            opened,
            rows,
        };
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
    fn sync_player(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let clip = self.shown.map(|id| (id, self.library.clip(id)));
        let max_height = viewer_rows(window);
        let visible = self.clip_in_view();
        self.player.update(cx, |player, cx| {
            match clip {
                Some((id, path)) => player.play(id, path, max_height, cx),
                None => player.stop(cx),
            }
            player.set_visible(visible, cx);
            if let Err(e) = player.release(window) {
                remote::report(format_args!("freeing a clip's frames: {e:#}"));
            }
        });
    }

    fn parts(&mut self, cx: &mut Context<'_, Self>) -> Parts {
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
                                .unwrap_or_else(|| gpui::div().into_any_element())
                        },
                        cx,
                    )
                }),
            }
        });
        parts.clone()
    }

    /// The picture in the lightbox, fitted to the window; any click closes it. An aim picture
    /// shows at the viewer's size until the full size is ready. It sits below the header, which
    /// stays usable as the window's title bar.
    fn lightbox(&self, cx: &mut Context<'_, Self>) -> Option<impl IntoElement> {
        use gpui::StyledImage as _;

        let path = self.lightbox.as_ref()?;
        let image = self.image(path, cx).ready().or_else(|| {
            let look = Look::Fit(self.rows);
            self.images.update(cx, |images, _| images.peek(path, look))
        });
        Some(
            gpui::div()
                .id("lightbox")
                .absolute()
                .inset_0()
                .top(ui::BAR_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .px(gpui::relative(0.05))
                .py(gpui::relative(0.05))
                .bg(theme::alpha(gpui::black(), 0.87))
                .occlude()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.lightbox = None;
                        cx.notify();
                    }),
                )
                .children(image.map(|image| {
                    gpui::img(ImageSource::Render(image))
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                })),
        )
    }
}

impl Focusable for BrowserView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        use gpui::prelude::FluentBuilder as _;

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
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |this, _, cx| {
                this.activation_changed(cx);
            }));
        }
        self.tick_minutes(cx);
        self.sync_player(window, cx);
        self.prefetch_map(cx);
        self.prefetch(cx);
        self.prepare_clip(window, cx);
        self.emit_choice(cx);
        let clip = self.shown.is_some() && self.clip_in_view();
        let parts = self.parts(cx);
        let element = gpui::div()
            .track_focus(&self.focus)
            .key_context(shortcuts::CONTEXT)
            .on_action(cx.listener(Self::next_landing))
            .on_action(cx.listener(Self::previous_landing))
            .on_action(cx.listener(Self::next_stand))
            .on_action(cx.listener(Self::previous_stand))
            .on_action(cx.listener(Self::watch_clip))
            .on_action(cx.listener(Self::show_stand))
            .on_action(cx.listener(Self::show_aim))
            .on_action(cx.listener(Self::show_landing))
            .on_action(cx.listener(Self::toggle_pause))
            .on_action(cx.listener(Self::restart_clip))
            .on_action(cx.listener(Self::slow_motion))
            .on_action(cx.listener(Self::switch_side))
            .on_action(cx.listener(Self::toggle_settings))
            .on_key_down(cx.listener(Self::fixed_key))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_key_up(cx.listener(Self::key_released))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .child(titlebar::frame(
                parts.header.cached(StyleRefinement::default().size_full()),
                window,
            ))
            .child(
                gpui::div()
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
                                    .min_w(PANEL_MIN * open)
                                    .h_full(),
                            ),
                        )
                    }),
            )
            .when(clip, |d| {
                let player = gpui::div()
                    .relative()
                    .size_full()
                    .opacity(open)
                    .child(self.player.clone())
                    .children(self.clip_note());
                d.child(Follow::new(self.clip_slot.clone(), player))
            })
            .children(self.chooser(window, cx))
            .children(self.settings_window(window, cx))
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

/// The lineups the keys reach next: every stand spot of the selected landing and the first of
/// the landings either side, or with nothing selected, the first of the first and last landings.
/// Each comes once, since with one or two landings the ones either side are the same.
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
    let mut unique = Vec::with_capacity(ids.len());
    for id in ids {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    unique
}

/// The pictures the browser first shows when it opens at `start`, and those its keys reach
/// next, to decode before it opens: the light theme's chips draw ability icons black, beside the
/// markers' own, and maps and aim pictures are drawn for the window's first size, which a display
/// that scales it decodes again at its own.
pub(crate) fn first_pictures(library: &Library, start: Start) -> Vec<Wanted> {
    let browse = Browse::new(library, start);
    let mut maps = Vec::new();
    let mut icons = Vec::new();
    let mut paths: Vec<PathBuf> = library
        .agents()
        .iter()
        .map(|agent| library.portrait(&agent.id))
        .collect();
    if browse.chooser() == Some(Chooser::Map) {
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
            icons.extend(agent.abilities.iter().map(|ab| library.ability_icon(ab.id)));
        }
        for id in ahead(&browse) {
            paths.extend([library.start_still(id), library.end_still(id)]);
        }
    }
    let silhouettes: Vec<Wanted> = match theme::mode() {
        Mode::Light => icons
            .iter()
            .map(|icon| Wanted {
                path: icon.clone(),
                look: Look::Silhouette,
            })
            .collect(),
        Mode::Dark => Vec::new(),
    };
    paths.extend(icons);
    let mut pictures: Vec<Wanted> = paths
        .into_iter()
        .map(|path| Wanted {
            path,
            look: Look::Plain,
        })
        .collect();
    pictures.extend(silhouettes);
    let across = map_across_for(placement::WINDOW_WIDTH, placement::WINDOW_HEIGHT, 1.);
    pictures.extend(maps.into_iter().map(|path| Wanted {
        path,
        look: Look::Across(across),
    }));
    let rows = viewer_rows_for(placement::WINDOW_WIDTH, 1.);
    pictures.extend(ahead(&browse).into_iter().map(|id| Wanted {
        path: library.aim_image(id),
        look: Look::Fit(rows),
    }));
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

/// [`map_across`] for a window `width` by `height` at `scale`.
#[expect(
    clippy::cast_precision_loss,
    reason = "the map sizes are a few thousand, which an f32 holds exactly"
)]
fn map_across_for(width: f32, height: f32, scale: f32) -> u32 {
    let most = width.min(height) * scale;
    MAP_SIZES
        .into_iter()
        .find(|&across| across as f32 >= most)
        .unwrap_or(MAP_SIZES[2])
}

/// `n word`, with an `s` unless `n` is 1.
pub(super) fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The narrowest the open panel gets: its content's own least width and its border. In a narrow
/// window it takes more than half, so its content is never cut off.
const PANEL_MIN: Pixels = gpui::px(panel::CONTENT_MIN + 1.);

/// The most device pixel rows the viewer can show once the panel is open: the panel's width, half
/// the window or [`PANEL_MIN`], less its padding, at 16:9. Rounded up to a multiple of 90 rows (a
/// 16th of 1440p), so small changes to the window do not reopen clips at a new size.
fn viewer_rows(window: &Window) -> u32 {
    viewer_rows_for(
        f32::from(window.viewport_size().width),
        window.scale_factor(),
    )
}

/// [`viewer_rows`] for a window `width` wide at `scale`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the rows are at least 360 and a window's width at most some thousands of pixels, so \
              the count of 90-row steps is small and positive"
)]
fn viewer_rows_for(width: f32, scale: f32) -> u32 {
    let width = (width / 2.).max(f32::from(PANEL_MIN)) - 48.;
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
    fn the_viewer_fits_the_panel_in_a_narrow_window() {
        assert_eq!(
            super::viewer_rows_for(1440., 1.),
            450,
            "half of 1440 is wider than the panel's least width"
        );
        assert_eq!(
            super::viewer_rows_for(2560., 1.),
            720,
            "half of 2560 is wider than the panel's least width"
        );
        assert_eq!(
            super::viewer_rows_for(960., 1.75),
            540,
            "960 wide, the panel takes 521 rather than 480: 466 device rows at 1.75x rather than \
             425, which round up to 540 rather than 450"
        );
    }

    #[test]
    fn plural_adds_an_s() {
        assert_eq!(super::plural(1, "lineup"), "1 lineup");
        assert_eq!(super::plural(0, "lineup"), "0 lineups");
        assert_eq!(super::plural(12, "lineup"), "12 lineups");
    }
}
