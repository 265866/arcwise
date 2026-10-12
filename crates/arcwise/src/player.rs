//! Plays the selected lineup's clip: takes frames from the decode thread and shows each when
//! its time comes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arcwise_core::LineupId;
use arcwise_video::{Decoder, Event, Frame};
use gpui::{
    Context, FontWeight, IntoElement, MouseButton, RenderImage, SharedString, Window, div,
    prelude::*, px,
};

use crate::{theme, ui};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speed {
    #[default]
    Full,
    Half,
    Quarter,
}

impl Speed {
    fn factor(self) -> f64 {
        match self {
            Speed::Full => 1.,
            Speed::Half => 0.5,
            Speed::Quarter => 0.25,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Speed::Full => "1×",
            Speed::Half => "½×",
            Speed::Quarter => "¼×",
        }
    }

    /// The next slower speed, back to full after the slowest.
    fn next(self) -> Speed {
        match self {
            Speed::Full => Speed::Half,
            Speed::Half => Speed::Quarter,
            Speed::Quarter => Speed::Full,
        }
    }
}

/// Where the clip is: the media time at an instant of wall time.
#[derive(Clone, Copy)]
struct Clock {
    anchor: Instant,
    media: Duration,
}

impl Clock {
    fn at(self, now: Instant, speed: Speed) -> Duration {
        self.media
            + now
                .saturating_duration_since(self.anchor)
                .mul_f64(speed.factor())
    }
}

pub struct Player {
    decoder: Result<Decoder, SharedString>,
    generation: u64,
    clip: Option<LineupId>,
    shown: Option<(Arc<RenderImage>, Duration)>,
    /// Decoded, and waiting for its time.
    next: Option<Frame>,
    clock: Option<Clock>,
    duration: Option<Duration>,
    speed: Speed,
    /// When paused, the media time it stopped at.
    paused: Option<Duration>,
    failed: Option<SharedString>,
    /// Frames replaced since the last render, to free from the window.
    retired: Vec<Arc<RenderImage>>,
    ticking: bool,
}

impl Player {
    pub fn new() -> Self {
        Player {
            decoder: Decoder::spawn().map_err(|e| format!("Could not start video: {e}").into()),
            generation: 0,
            clip: None,
            shown: None,
            next: None,
            clock: None,
            duration: None,
            speed: Speed::Full,
            paused: None,
            failed: None,
            retired: Vec::new(),
            ticking: false,
        }
    }

    /// Plays a lineup's clip from the start, unless it is the one already playing.
    /// Frames are scaled down to `max_height` rows, the most the viewer can show.
    pub fn play(&mut self, id: LineupId, path: PathBuf, max_height: u32, cx: &mut Context<Self>) {
        if self.clip == Some(id) {
            return;
        }
        self.reset();
        self.clip = Some(id);
        match &self.decoder {
            Err(e) => self.failed = Some(e.clone()),
            Ok(_) if !path.is_file() => {
                self.failed = Some("This clip is missing from the library.".into());
            }
            Ok(decoder) => decoder.open(self.generation, path, Some(max_height)),
        }
        cx.notify();
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
        self.next = None;
        self.clock = None;
        self.duration = None;
        self.paused = None;
        self.failed = None;
        if let Some((image, _)) = self.shown.take() {
            self.retired.push(image);
        }
    }

    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(clock) = self.clock else { return };
        let now = Instant::now();
        match self.paused.take() {
            Some(media) => self.clock = Some(Clock { anchor: now, media }),
            None => self.paused = Some(clock.at(now, self.speed)),
        }
        cx.notify();
    }

    pub fn cycle_speed(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if let (Some(clock), None) = (self.clock.as_mut(), self.paused) {
            *clock = Clock {
                anchor: now,
                media: clock.at(now, self.speed),
            };
        }
        self.speed = self.speed.next();
        cx.notify();
    }

    fn media_now(&self, now: Instant) -> Option<Duration> {
        self.paused
            .or_else(|| Some(self.clock?.at(now, self.speed)))
    }

    /// Takes the next frame off the decode thread, skipping any from an older clip.
    fn peek(&mut self) -> Option<&Frame> {
        let decoder = self.decoder.as_ref().ok()?;
        while self.next.is_none() {
            let frame = decoder.frames().try_recv().ok()?;
            if frame.generation == self.generation {
                self.next = Some(frame);
            }
        }
        self.next.as_ref()
    }

    fn due(&mut self, now: Instant) -> bool {
        let media = self.media_now(now);
        let first = self.clock.is_none();
        self.peek()
            .is_some_and(|frame| first || media.is_some_and(|m| frame.pts <= m))
    }

    /// Shows the newest frame that is due, dropping any it overtook.
    fn advance(&mut self, now: Instant) {
        if let Ok(decoder) = &self.decoder {
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
                    } if generation == self.generation => {
                        self.failed = Some(if codec_missing {
                            "Windows has no decoder for this clip. AV1 clips need the AV1 \
                             Video Extension from the Microsoft Store."
                                .into()
                        } else {
                            format!("Windows can't play this clip. {error}").into()
                        });
                    }
                    _ => {}
                }
            }
        }
        let mut latest = None;
        while self.due(now) {
            let frame = self.next.take();
            if self.clock.is_none() {
                // The first frame shows as soon as it exists, and starts the clock.
                if let Some(frame) = &frame {
                    self.clock = Some(Clock {
                        anchor: now,
                        media: frame.pts,
                    });
                }
                latest = frame;
                break;
            }
            latest = frame;
        }
        if let Some(frame) = latest
            && let Some(image) = image(frame.width, frame.height, frame.bgra)
            && let Some((old, _)) = self.shown.replace((image, frame.pts))
        {
            self.retired.push(old);
        }
    }

    /// Runs once a frame while a clip plays, and redraws only when a new frame is due. A redraw
    /// every frame would cost more and show nothing new.
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.clip.is_none() || self.failed.is_some() || self.paused.is_some() {
            self.ticking = false;
            return;
        }
        if self.due(Instant::now()) {
            cx.notify();
        }
        let this = cx.entity().downgrade();
        window.on_next_frame(move |window, cx| {
            this.update(cx, |player, cx| player.tick(window, cx)).ok();
        });
    }

    fn progress(&self, now: Instant) -> f32 {
        match (self.media_now(now), self.duration) {
            (Some(media), Some(duration)) if !duration.is_zero() => {
                (media.as_secs_f64() % duration.as_secs_f64() / duration.as_secs_f64()) as f32
            }
            _ => 0.,
        }
    }
}

fn image(width: u32, height: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(buffer)])))
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
        let now = Instant::now();
        if self.clip.is_some() && self.paused.is_none() {
            self.advance(now);
        }
        if self.clip.is_some() && self.failed.is_none() && self.paused.is_none() && !self.ticking {
            self.ticking = true;
            self.tick(window, cx);
        }
        let waiting = self.clip.is_some() && self.shown.is_none() && self.failed.is_none();
        div()
            .id("player")
            .relative()
            .size_full()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.toggle_pause(cx)),
            )
            .children(
                self.shown
                    .as_ref()
                    .map(|(image, _)| ui::cover(image.clone())),
            )
            .when(waiting, |d| d.child(ui::spinner(px(34.))))
            .when(self.paused.is_some(), |d| {
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
                            .text_color(theme::muted())
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
                    .bg(if self.speed == Speed::Full {
                        theme::alpha(gpui::black(), 0.72)
                    } else {
                        theme::accent().into()
                    })
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(12.))
                    .child(self.speed.label())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.cycle_speed(cx);
                        }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(3.))
                    .bg(theme::alpha(gpui::white(), 0.12))
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative(self.progress(now)))
                            .bg(theme::text()),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_step_slower_then_wrap() {
        assert_eq!(Speed::Full.next(), Speed::Half);
        assert_eq!(Speed::Half.next(), Speed::Quarter);
        assert_eq!(Speed::Quarter.next(), Speed::Full);
    }

    #[test]
    fn the_clock_runs_at_the_speed() {
        let anchor = Instant::now();
        let clock = Clock {
            anchor,
            media: Duration::from_secs(2),
        };
        let later = anchor + Duration::from_secs(4);
        assert_eq!(clock.at(later, Speed::Full), Duration::from_secs(6));
        assert_eq!(clock.at(later, Speed::Quarter), Duration::from_secs(3));
        assert_eq!(
            clock.at(anchor - Duration::from_secs(1), Speed::Full),
            Duration::from_secs(2),
            "never runs backwards"
        );
    }
}
