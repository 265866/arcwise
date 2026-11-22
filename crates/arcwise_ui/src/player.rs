//! Plays the selected lineup's clip: takes frames from the decode thread and shows each when
//! its time comes.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arcwise_core::LineupId;
use arcwise_remote::remote;
use arcwise_video::{Decoder, Event, Frame};
use gpui::{
    App, Bounds, Context, DispatchPhase, Div, FontWeight, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render,
    RenderImage, Rgba, SharedString, Stateful, Styled, WeakEntity, Window,
};
use image::RgbaImage;

use crate::pacing::{Pacing, Speed};
use crate::{theme, trace, ui};

/// The height of the strip along the bottom that takes clicks on the progress bar: taller than
/// the bar, so it is easy to hit. The bar thickens while the pointer is over it.
const PROGRESS_HEIGHT: Pixels = gpui::px(14.);

/// The clip player: one decode thread, the clip it plays, and the frame on screen.
#[derive(Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "paused, hidden, ticking and the pointer over the bar each change on events of their \
              own and in any combination, which an enum of states would only multiply"
)]
pub(crate) struct Player {
    /// The decode thread, or why it could not start.
    decoder: Result<Decoder, SharedString>,
    generation: u64,
    /// The generation the clip opened with. Seeks count on from it, and a failure reported for
    /// any generation since is this clip's, even one from before a seek the player has not read.
    opened_as: u64,
    clip: Option<LineupId>,
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded frame through an Arc<RenderImage>"
        )
    )]
    shown: Option<Arc<RenderImage>>,
    pacing: Pacing<Frame>,
    duration: Option<Duration>,
    /// Paused by the user.
    paused: bool,
    /// Out of view, so held still until it is back.
    hidden: bool,
    failed: Option<SharedString>,
    /// Frames replaced since they were last freed from the window.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "GPUI draws and frees a decoded frame through an Arc<RenderImage>"
        )
    )]
    retired: Vec<Arc<RenderImage>>,
    ticking: bool,
    /// When the clip was asked for, until its first frame shows.
    opened: Option<Instant>,
    /// Where a seek went, until a frame from there shows.
    seeking: Option<Duration>,
    /// Where the progress bar is, to turn a click on it into a place in the clip.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(
            shared_ownership,
            reason = "the canvas GPUI owns for one frame writes the bounds the view reads in the next"
        )
    )]
    bar: Rc<Cell<Bounds<Pixels>>>,
    /// The pointer is over the progress bar, which then thickens.
    over_bar: bool,
    /// A drag along the progress bar, which holds the clip still until it ends.
    scrub: Option<Scrub>,
}

/// A drag along the progress bar.
#[derive(Clone, Copy, Debug)]
struct Scrub {
    /// Where the pointer went while a seek was still on its way, as a fraction of the clip.
    /// Seeks go out one at a time, each once the last one's frame shows, so the frames keep up
    /// with the pointer rather than queueing behind it.
    next: Option<f32>,
}

impl Player {
    /// A player with nothing playing, its decode thread started.
    #[must_use]
    pub(crate) fn new() -> Self {
        Player {
            decoder: Decoder::spawn().map_err(|e| format!("Could not start video: {e}").into()),
            generation: 0,
            opened_as: 0,
            clip: None,
            shown: None,
            pacing: Pacing::default(),
            duration: None,
            paused: false,
            hidden: false,
            failed: None,
            retired: Vec::new(),
            ticking: false,
            opened: None,
            seeking: None,
            bar: Rc::default(),
            over_bar: false,
            scrub: None,
        }
    }

    /// Plays a lineup's clip from the start, unless it is the one already playing. A clip of a
    /// library on the web not on disk yet streams from the web, and is fetched to disk meanwhile.
    /// Frames are scaled down to `max_height` rows, the most the viewer can show.
    pub(crate) fn play(
        &mut self,
        id: LineupId,
        path: PathBuf,
        max_height: u32,
        cx: &mut Context<'_, Self>,
    ) {
        if self.clip == Some(id) {
            return;
        }
        let local = path;
        let path = remote::playable(&local);
        let streams = remote::is_url(&path.to_string_lossy());
        if streams {
            remote::fetch_in_background(&local);
        }
        self.reset();
        self.opened_as = self.generation;
        self.clip = Some(id);
        self.opened = Some(Instant::now());
        match &self.decoder {
            Err(e) => self.failed = Some(e.clone()),
            Ok(_) if !streams && !path.is_file() => {
                self.failed = Some("This clip is missing from the library.".into());
            }
            Ok(decoder) => decoder.open(self.generation, path, Some(max_height)),
        }
        self.update_hold();
        cx.notify();
    }

    /// Readies a clip that may play next, so it shows at once if it does. One from the web is not
    /// fetched to disk unless it plays, so pointing at many lineups does not fetch each.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "callers hand over the path they built for the clip, as they do to play"
    )]
    pub(crate) fn prepare(&self, path: PathBuf, max_height: u32) {
        if let Ok(decoder) = &self.decoder {
            decoder.prepare(remote::playable(&path), Some(max_height));
        }
    }

    /// Stops playing and lets go of the clip.
    pub(crate) fn stop(&mut self, cx: &mut Context<'_, Self>) {
        if self.clip.is_none() {
            return;
        }
        self.reset();
        if let Ok(decoder) = &self.decoder {
            decoder.stop();
        }
        cx.notify();
    }

    /// Forgets the clip, and retires the frame shown.
    fn reset(&mut self) {
        self.generation += 1;
        self.clip = None;
        self.pacing.reset();
        self.duration = None;
        self.paused = false;
        self.failed = None;
        self.opened = None;
        self.seeking = None;
        self.scrub = None;
        self.retired.extend(self.shown.take());
    }

    /// Whether a clip in view is still waiting for its first frame.
    #[must_use]
    pub(crate) fn waiting(&self) -> bool {
        self.clip.is_some() && self.shown.is_none() && self.failed.is_none() && !self.hidden
    }

    /// Pauses the clip playing, or plays the clip paused.
    pub(crate) fn toggle_pause(&mut self, cx: &mut Context<'_, Self>) {
        if self.clip.is_some() {
            self.paused = !self.paused;
            self.update_hold();
            cx.notify();
        }
    }

    /// Plays the clip from `fraction` of the way through, or shows that frame if paused. A
    /// fraction of 1 goes just short of the end, which has no frame of its own to show.
    pub(crate) fn seek(&mut self, fraction: f32, cx: &mut Context<'_, Self>) {
        let (Ok(decoder), Some(duration), None) = (&self.decoder, self.duration, &self.failed)
        else {
            return;
        };
        let position = duration
            .mul_f32(fraction.clamp(0., 1.))
            .min(duration.saturating_sub(Duration::from_millis(1)));
        self.generation += 1;
        self.seeking = Some(position);
        self.pacing.seek();
        decoder.seek(self.generation, position);
        cx.notify();
    }

    /// Whether a drag along the progress bar is under way.
    #[must_use]
    pub(crate) fn scrubbing(&self) -> bool {
        self.scrub.is_some()
    }

    /// Starts a drag along the progress bar at `fraction` of the way through. Until the clip
    /// opens there is nowhere to go, and holding it would keep it from opening, so nothing
    /// happens.
    fn scrub_start(&mut self, fraction: f32, cx: &mut Context<'_, Self>) {
        if self.duration.is_none() || self.failed.is_some() {
            return;
        }
        self.scrub = Some(Scrub { next: None });
        self.update_hold();
        self.seek(fraction, cx);
    }

    /// Follows a drag to `fraction` of the way through: at once, or once the seek on its way
    /// shows.
    fn scrub_to(&mut self, fraction: f32, cx: &mut Context<'_, Self>) {
        let Some(scrub) = &mut self.scrub else { return };
        if self.seeking.is_some() {
            scrub.next = Some(fraction);
            cx.notify();
        } else {
            self.seek(fraction, cx);
        }
    }

    /// Ends a drag at the last place it reached, and plays on from there unless paused.
    fn scrub_end(&mut self, cx: &mut Context<'_, Self>) {
        let Some(scrub) = self.scrub.take() else {
            return;
        };
        if let Some(fraction) = scrub.next {
            self.seek(fraction, cx);
        }
        self.update_hold();
        cx.notify();
    }

    /// How far along the progress bar a window position is, as a fraction of the clip.
    fn along(&self, x: Pixels) -> Option<f32> {
        let bar = self.bar.get();
        (bar.size.width > gpui::px(0.)).then(|| (x - bar.left()) / bar.size.width)
    }

    /// Plays the clip again from the start.
    pub(crate) fn restart(&mut self, cx: &mut Context<'_, Self>) {
        self.paused = false;
        self.update_hold();
        self.seek(0., cx);
    }

    /// Holds the clip still while the viewer shows something else, so frames are not decoded
    /// and drawn for nobody, and lets it carry on from there when it is back in view. Out of
    /// view, the player is not drawn and so cannot hear a drag end, so hiding ends one.
    pub(crate) fn set_visible(&mut self, visible: bool, cx: &mut Context<'_, Self>) {
        if self.hidden == visible {
            self.hidden = !visible;
            if self.hidden {
                self.scrub_end(cx);
            }
            self.update_hold();
            cx.notify();
        }
    }

    /// Holds the clock while paused, hidden or scrubbing, and lets it run otherwise.
    fn update_hold(&mut self) {
        let now = Instant::now();
        if self.paused || self.hidden || self.scrub.is_some() {
            self.pacing.hold(now);
        } else {
            self.pacing.release(now);
        }
    }

    /// Plays at `speed` from now on.
    pub(crate) fn set_speed(&mut self, speed: Speed, cx: &mut Context<'_, Self>) {
        self.pacing.set_speed(speed, Instant::now());
        cx.notify();
    }

    /// Plays at the next slower speed, back to full after the slowest.
    pub(crate) fn cycle_speed(&mut self, cx: &mut Context<'_, Self>) {
        self.pacing
            .set_speed(self.pacing.speed().next(), Instant::now());
        cx.notify();
    }

    /// Frees replaced frames from the window. Call while rendering; the player is not always
    /// on screen to do it itself.
    ///
    /// # Errors
    ///
    /// When the window cannot free a frame; the rest are freed all the same.
    pub(crate) fn release(&mut self, window: &mut Window) -> anyhow::Result<()> {
        let mut freed = Ok(());
        for image in self.retired.drain(..) {
            let dropped = window.drop_image(image);
            if freed.is_ok() {
                freed = dropped;
            }
        }
        freed
    }

    /// Frees every frame the player holds, before it is dropped. Call outside any window's
    /// update, so `drop_image` reaches every window.
    pub(crate) fn release_all(&mut self, cx: &mut App) {
        for image in self.retired.drain(..).chain(self.shown.take()) {
            cx.drop_image(image, None);
        }
    }

    /// Takes in what the decode thread says of the clips it opened: the duration of the clip
    /// playing, or why it failed.
    fn handle_events(&mut self) {
        let Ok(decoder) = &self.decoder else { return };
        for event in decoder.events().try_iter() {
            match event {
                Event::Opened {
                    generation,
                    duration,
                    ..
                } if generation == self.generation => self.duration = Some(duration),
                Event::Failed {
                    generation,
                    error,
                    codec_missing,
                } if (self.opened_as..=self.generation).contains(&generation) => {
                    self.scrub = None;
                    self.failed = Some(if codec_missing {
                        "Windows has no decoder for this clip. AV1 clips need the AV1 Video \
                         Extension from the Microsoft Store."
                            .into()
                    } else {
                        format!("Windows can't play this clip. {error}").into()
                    });
                }
                Event::Opened { .. } | Event::Failed { .. } => {}
            }
        }
    }

    /// Takes the next frame of the current clip off the decode thread, skipping older clips'.
    fn pull(
        decoder: &Result<Decoder, SharedString>,
        generation: u64,
    ) -> impl FnMut() -> Option<Frame> + use<> {
        let frames = decoder.as_ref().ok().map(|d| d.frames().clone());
        move || {
            loop {
                let frame = frames.as_ref()?.try_recv().ok()?;
                if frame.generation == generation {
                    return Some(frame);
                }
            }
        }
    }

    /// Runs once a frame while a clip plays, and redraws only when a new frame is due. A redraw
    /// every frame would cost more and show nothing new. Hidden, it is not drawn, so a frame it is
    /// waiting to show would ask for a redraw every frame until it is back in view: it stops.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.clip.is_none() || self.failed.is_some() || self.pacing.is_held() || self.hidden {
            self.ticking = false;
            return;
        }
        let pull = Self::pull(&self.decoder, self.generation);
        if self.pacing.due(Instant::now(), pull) {
            cx.notify();
        }
        let this = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            if let Some(this) = this.upgrade() {
                this.update(cx, |player, cx| player.tick(window, cx));
            }
        });
    }

    /// How far through the clip the frame showing is, from 0 to 1: where a drag waits to go,
    /// otherwise the clock, or where a seek went.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a fraction from 0 to 1 needs no more precision than an f32 has"
    )]
    fn progress(&self, now: Instant) -> f32 {
        if let Some(Scrub {
            next: Some(fraction),
        }) = self.scrub
        {
            return fraction.clamp(0., 1.);
        }
        match (self.pacing.media_now(now).or(self.seeking), self.duration) {
            (Some(media), Some(duration)) if !duration.is_zero() => {
                (media.as_secs_f64() % duration.as_secs_f64() / duration.as_secs_f64()) as f32
            }
            _ => 0.,
        }
    }
}

/// Follows a drag along the progress bar wherever the pointer goes, off the bar or out of the
/// window, which Windows still reports while the button is held, until the button is let go.
/// Always listening, not only once a drag has drawn, so a release right after the press is not
/// missed. A move without the button held is a release the window missed, as when another
/// window took the pointer.
fn follow_drag(player: WeakEntity<Player>) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |_, (), window, _| {
            let moved = player.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let Some(player) = moved.upgrade() else {
                    return;
                };
                player.update(cx, |player, cx| {
                    if event.pressed_button != Some(MouseButton::Left) {
                        player.scrub_end(cx);
                    } else if let Some(along) = player.along(event.position.x) {
                        player.scrub_to(along, cx);
                    }
                });
            });
            let released = player.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture
                    && event.button == MouseButton::Left
                    && let Some(player) = released.upgrade()
                {
                    player.update(cx, Player::scrub_end);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// The color of text and marks over the clip, which is dark in either theme.
fn on_clip() -> Rgba {
    gpui::rgb(theme::DARK.text)
}

/// A decoded frame as a picture GPUI draws, or nothing when its pixels do not fit its size.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI draws and frees a decoded frame through an Arc<RenderImage>"
    )
)]
fn image(frame: Frame) -> Option<Arc<RenderImage>> {
    let buffer = RgbaImage::from_raw(frame.width, frame.height, frame.bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

impl Player {
    /// Takes in the decode thread's news and the frame now due, and starts ticking while a clip
    /// plays.
    fn advance(&mut self, now: Instant, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.clip.is_some() {
            self.handle_events();
            let pull = Self::pull(&self.decoder, self.generation);
            if let Some(image) = self.pacing.advance(now, pull).and_then(image) {
                self.seeking = None;
                self.retired.extend(self.shown.replace(image));
                if let Some(fraction) = self.scrub.as_mut().and_then(|scrub| scrub.next.take()) {
                    self.seek(fraction, cx);
                }
                if let (Some(opened), Some(clip)) = (self.opened.take(), self.clip) {
                    trace::task(|| format!("clip {clip}"), opened);
                }
            }
        }
        if self.clip.is_some() && self.failed.is_none() && !self.pacing.is_held() && !self.ticking {
            self.ticking = true;
            self.tick(window, cx);
        }
    }

    /// The button in the top right that shows the speed and cycles through the speeds.
    fn speed_button(&self, cx: &mut Context<'_, Self>) -> Stateful<Div> {
        let speed = self.pacing.speed();
        gpui::div()
            .id("speed")
            .absolute()
            .top(gpui::px(10.))
            .right(gpui::px(10.))
            .min_w(gpui::px(36.))
            .h(gpui::px(24.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(gpui::px(6.))
            .bg(if speed == Speed::Full {
                theme::alpha(gpui::black(), 0.72)
            } else {
                theme::accent().into()
            })
            .font_weight(FontWeight::BOLD)
            .text_size(gpui::px(12.))
            .child(speed.label())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    this.cycle_speed(cx);
                }),
            )
    }

    /// The progress bar along the bottom, which a click or a drag seeks along. A click on it
    /// does not reach the clip around it, which would pause.
    fn progress_bar(&self, now: Instant, cx: &mut Context<'_, Self>) -> Stateful<Div> {
        use gpui::StatefulInteractiveElement as _;

        gpui::div()
            .id("progress")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(PROGRESS_HEIGHT)
            .flex()
            .items_end()
            .child(ui::measure(self.bar.clone()))
            .child(follow_drag(cx.entity().downgrade()))
            .child(
                gpui::div()
                    .w_full()
                    .h(gpui::px(if self.over_bar || self.scrub.is_some() {
                        6.
                    } else {
                        3.
                    }))
                    .bg(theme::alpha(gpui::white(), 0.12))
                    .child(
                        gpui::div()
                            .h_full()
                            .w(gpui::relative(self.progress(now)))
                            .bg(on_clip()),
                    ),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.over_bar = *hovered;
                cx.notify();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(along) = this.along(event.position.x) {
                        this.scrub_start(along, cx);
                    }
                }),
            )
    }
}

/// Why the clip cannot play, centered over it.
fn failed_note(failed: SharedString) -> Div {
    gpui::div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .p(gpui::px(24.))
        .child(
            gpui::div()
                .max_w(gpui::px(360.))
                .px(gpui::px(14.))
                .py(gpui::px(8.))
                .rounded(gpui::px(8.))
                .bg(theme::alpha(gpui::black(), 0.8))
                .text_color(gpui::rgb(theme::DARK.muted))
                .text_size(gpui::px(13.))
                .child(failed),
        )
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        use gpui::prelude::FluentBuilder as _;

        if let Err(e) = self.release(window) {
            remote::report(format_args!("freeing a clip's frames: {e:#}"));
        }
        let now = Instant::now();
        self.advance(now, window, cx);
        let waiting = self.clip.is_some() && self.shown.is_none() && self.failed.is_none();
        gpui::div()
            .id("player")
            .relative()
            .size_full()
            .text_color(on_clip())
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.toggle_pause(cx)),
            )
            .children(self.shown.clone().map(ui::cover))
            .when(waiting, |d| {
                d.child(ui::spinner(gpui::px(34.), on_clip().into()))
            })
            .when(self.paused, |d| {
                d.child(
                    gpui::div()
                        .absolute()
                        .inset_0()
                        .bg(theme::alpha(gpui::black(), 0.4))
                        .child(ui::play_glyph(gpui::px(40.))),
                )
            })
            .children(self.failed.clone().map(failed_note))
            .child(self.speed_button(cx))
            .child(self.progress_bar(now, cx))
    }
}
