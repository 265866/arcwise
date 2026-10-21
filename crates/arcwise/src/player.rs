//! Plays the selected lineup's clip: takes frames from the decode thread and shows each when
//! its time comes.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arcwise_core::LineupId;
use arcwise_video::{Decoder, Event, Frame};
use gpui::{
    App, Bounds, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, Pixels,
    RenderImage, SharedString, Window, div, prelude::*, px,
};

use crate::pacing::{Pacing, Speed};
use crate::{theme, ui};

pub struct Player {
    decoder: Result<Decoder, SharedString>,
    generation: u64,
    /// The generation the clip opened with. Seeks count on from it, and a failure reported for
    /// any generation since is this clip's, even one from before a seek the player has not read.
    opened_as: u64,
    clip: Option<LineupId>,
    shown: Option<Arc<RenderImage>>,
    pacing: Pacing<Frame>,
    duration: Option<Duration>,
    /// Paused by the user.
    paused: bool,
    /// Out of view, so held still until it is back.
    hidden: bool,
    failed: Option<SharedString>,
    /// Frames replaced since they were last freed from the window.
    retired: Vec<Arc<RenderImage>>,
    ticking: bool,
    /// When the clip was asked for, until its first frame shows.
    opened: Option<Instant>,
    /// Where a seek went, until a frame from there shows.
    seeking: Option<Duration>,
    /// Where the progress bar is, to turn a click on it into a place in the clip.
    bar: Rc<Cell<Bounds<Pixels>>>,
    /// The pointer is over the progress bar, which then thickens.
    over_bar: bool,
}

impl Player {
    pub fn new() -> Self {
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
        }
    }

    /// Plays a lineup's clip from the start, unless it is the one already playing.
    /// Frames are scaled down to `max_height` rows, the most the viewer can show.
    pub fn play(&mut self, id: LineupId, path: PathBuf, max_height: u32, cx: &mut Context<Self>) {
        if self.clip == Some(id) {
            return;
        }
        self.reset();
        self.opened_as = self.generation;
        self.clip = Some(id);
        self.opened = Some(Instant::now());
        match &self.decoder {
            Err(e) => self.failed = Some(e.clone()),
            Ok(_) if !path.is_file() => {
                self.failed = Some("This clip is missing from the library.".into());
            }
            Ok(decoder) => decoder.open(self.generation, path, Some(max_height)),
        }
        self.update_hold();
        cx.notify();
    }

    /// Readies a clip that may play next, so it shows at once if it does.
    pub fn prepare(&self, path: PathBuf, max_height: u32) {
        if let Ok(decoder) = &self.decoder {
            decoder.prepare(path, Some(max_height));
        }
    }

    /// Stops playing and lets go of the clip.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        if self.clip.is_none() {
            return;
        }
        self.reset();
        if let Ok(decoder) = &self.decoder {
            decoder.stop();
        }
        cx.notify();
    }

    fn reset(&mut self) {
        self.generation += 1;
        self.clip = None;
        self.pacing.reset();
        self.duration = None;
        self.paused = false;
        self.failed = None;
        self.opened = None;
        self.seeking = None;
        self.retired.extend(self.shown.take());
    }

    /// Whether a clip in view is still waiting for its first frame.
    pub fn waiting(&self) -> bool {
        self.clip.is_some() && self.shown.is_none() && self.failed.is_none() && !self.hidden
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        if self.clip.is_some() {
            self.paused = !self.paused;
            self.update_hold();
            cx.notify();
        }
    }

    /// Plays the clip from `fraction` of the way through, or shows that frame if paused.
    pub fn seek(&mut self, fraction: f32, cx: &mut Context<Self>) {
        let (Ok(decoder), Some(duration), None) = (&self.decoder, self.duration, &self.failed)
        else {
            return;
        };
        // Short of the end, which has no frame of its own to show.
        let position = duration
            .mul_f32(fraction.clamp(0., 1.))
            .min(duration.saturating_sub(Duration::from_millis(1)));
        self.generation += 1;
        self.seeking = Some(position);
        self.pacing.seek();
        decoder.seek(self.generation, position);
        cx.notify();
    }

    /// Plays the clip again from the start.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.paused = false;
        self.update_hold();
        self.seek(0., cx);
    }

    /// Holds the clip still while the viewer shows something else, so frames are not decoded
    /// and drawn for nobody, and lets it carry on from there when it is back in view.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.hidden == visible {
            self.hidden = !visible;
            self.update_hold();
            cx.notify();
        }
    }

    fn update_hold(&mut self) {
        let now = Instant::now();
        if self.paused || self.hidden {
            self.pacing.hold(now);
        } else {
            self.pacing.release(now);
        }
    }

    pub fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        self.pacing
            .set_speed(self.pacing.speed().next(), Instant::now());
        cx.notify();
    }

    /// Frees replaced frames from the window. Call while rendering; the player is not always
    /// on screen to do it itself.
    pub fn release(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
    }

    /// Frees every frame the player holds, before it is dropped. Call outside any window's
    /// update, so `drop_image` reaches every window.
    pub fn release_all(&mut self, cx: &mut App) {
        for image in self.retired.drain(..).chain(self.shown.take()) {
            cx.drop_image(image, None);
        }
    }

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
                    self.failed = Some(if codec_missing {
                        "Windows has no decoder for this clip. AV1 clips need the AV1 Video \
                         Extension from the Microsoft Store."
                            .into()
                    } else {
                        format!("Windows can't play this clip. {error}").into()
                    });
                }
                _ => {}
            }
        }
    }

    /// Takes the next frame of the current clip off the decode thread, skipping older clips'.
    fn pull(
        decoder: &Result<Decoder, SharedString>,
        generation: u64,
    ) -> impl FnMut() -> Option<Frame> {
        let frames = decoder.as_ref().ok().map(|d| d.frames().clone());
        move || loop {
            let frame = frames.as_ref()?.try_recv().ok()?;
            if frame.generation == generation {
                return Some(frame);
            }
        }
    }

    /// Runs once a frame while a clip plays, and redraws only when a new frame is due. A redraw
    /// every frame would cost more and show nothing new.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.clip.is_none() || self.failed.is_some() || self.pacing.is_held() {
            self.ticking = false;
            return;
        }
        let pull = Self::pull(&self.decoder, self.generation);
        if self.pacing.due(Instant::now(), pull) {
            cx.notify();
        }
        let this = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            this.update(cx, |player, cx| player.tick(window, cx)).ok();
        });
    }

    fn progress(&self, now: Instant) -> f32 {
        match (self.pacing.media_now(now).or(self.seeking), self.duration) {
            (Some(media), Some(duration)) if !duration.is_zero() => {
                (media.as_secs_f64() % duration.as_secs_f64() / duration.as_secs_f64()) as f32
            }
            _ => 0.,
        }
    }
}

fn image(frame: Frame) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.release(window);
        let now = Instant::now();
        if self.clip.is_some() {
            self.handle_events();
            let pull = Self::pull(&self.decoder, self.generation);
            if let Some(image) = self.pacing.advance(now, pull).and_then(image) {
                self.seeking = None;
                self.retired.extend(self.shown.replace(image));
                if let (Some(opened), Some(clip)) = (self.opened.take(), self.clip) {
                    crate::trace::task(|| format!("clip {clip}"), opened);
                }
            }
        }
        if self.clip.is_some() && self.failed.is_none() && !self.pacing.is_held() && !self.ticking {
            self.ticking = true;
            self.tick(window, cx);
        }
        let waiting = self.clip.is_some() && self.shown.is_none() && self.failed.is_none();
        let speed = self.pacing.speed();
        // Over the clip, which is dark in either theme.
        let on_clip = gpui::rgb(theme::DARK.text);
        div()
            .id("player")
            .relative()
            .size_full()
            .text_color(on_clip)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.toggle_pause(cx)),
            )
            .children(self.shown.clone().map(ui::cover))
            .when(waiting, |d| d.child(ui::spinner(px(34.), on_clip)))
            .when(self.paused, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .bg(theme::alpha(gpui::black(), 0.4))
                        .child(ui::play_glyph(px(40.))),
                )
            })
            .children(self.failed.clone().map(|failed| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.))
                    .child(
                        div()
                            .max_w(px(360.))
                            .px(px(14.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(theme::alpha(gpui::black(), 0.8))
                            .text_color(gpui::rgb(theme::DARK.muted))
                            .text_size(px(13.))
                            .child(failed),
                    )
            }))
            .child(
                div()
                    .id("speed")
                    .absolute()
                    .top(px(10.))
                    .right(px(10.))
                    .min_w(px(36.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .bg(if speed == Speed::Full {
                        theme::alpha(gpui::black(), 0.72)
                    } else {
                        theme::accent().into()
                    })
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(12.))
                    .child(speed.label())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.cycle_speed(cx);
                        }),
                    ),
            )
            .child(
                // Taller than the bar, so it is easy to hit; the bar thickens under the pointer.
                div()
                    .id("progress")
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(14.))
                    .flex()
                    .items_end()
                    .child(ui::measure(self.bar.clone()))
                    .child(
                        div()
                            .w_full()
                            .h(px(if self.over_bar { 6. } else { 3. }))
                            .bg(theme::alpha(gpui::white(), 0.12))
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui::relative(self.progress(now)))
                                    .bg(on_clip),
                            ),
                    )
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.over_bar = *hovered;
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            // The clip around it pauses when clicked.
                            cx.stop_propagation();
                            let bar = this.bar.get();
                            if bar.size.width > px(0.) {
                                let along = (event.position.x - bar.left()) / bar.size.width;
                                this.seek(along, cx);
                            }
                        }),
                    ),
            )
    }
}
