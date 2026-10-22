//! When to show each decoded frame: a media clock that runs at a chosen speed and can be held
//! still, and the newest frame it has reached.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

    pub fn label(self) -> &'static str {
        match self {
            Speed::Full => "1×",
            Speed::Half => "½×",
            Speed::Quarter => "¼×",
        }
    }

    pub const ALL: [Speed; 3] = [Speed::Full, Speed::Half, Speed::Quarter];

    /// The next slower speed, back to full after the slowest.
    pub fn next(self) -> Speed {
        match self {
            Speed::Full => Speed::Half,
            Speed::Half => Speed::Quarter,
            Speed::Quarter => Speed::Full,
        }
    }
}

/// Something with a presentation time.
pub trait Timed {
    fn pts(&self) -> Duration;
}

impl Timed for arcwise_video::Frame {
    fn pts(&self) -> Duration {
        self.pts
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    /// No frame shown yet; the clock starts at the first one.
    Waiting { held: bool },
    /// The media time `media` at the wall time `anchor`, running at the speed since.
    Running { anchor: Instant, media: Duration },
    /// Stopped at a media time.
    Held(Duration),
    /// Moved by a seek: the next frame shows, and the clock starts there, or stays held there
    /// if held.
    Seeking { held: bool },
}

#[derive(Debug)]
pub struct Pacing<F> {
    state: State,
    speed: Speed,
    /// Taken from the decoder, and waiting for its time.
    next: Option<F>,
}

impl<F: Timed> Default for Pacing<F> {
    fn default() -> Self {
        Pacing {
            state: State::Waiting { held: false },
            speed: Speed::Full,
            next: None,
        }
    }
}

impl<F: Timed> Pacing<F> {
    /// Starts over for a new clip, keeping the speed.
    pub fn reset(&mut self) {
        self.state = State::Waiting { held: false };
        self.next = None;
    }

    /// Starts over from the next frame, after a seek, and stays held if it was.
    pub fn seek(&mut self) {
        self.state = match self.state {
            State::Waiting { held } | State::Seeking { held } => State::Seeking { held },
            State::Running { .. } => State::Seeking { held: false },
            State::Held(_) => State::Seeking { held: true },
        };
        self.next = None;
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    pub fn set_speed(&mut self, speed: Speed, now: Instant) {
        if let Some(media) = self.running_media(now) {
            self.state = State::Running { anchor: now, media };
        }
        self.speed = speed;
    }

    /// Whether no frame is coming until the clock is released. A seek still takes one.
    pub fn is_held(&self) -> bool {
        matches!(self.state, State::Waiting { held: true } | State::Held(_))
    }

    /// Stops the clock where it is.
    pub fn hold(&mut self, now: Instant) {
        self.state = match self.state {
            State::Waiting { .. } => State::Waiting { held: true },
            State::Seeking { .. } => State::Seeking { held: true },
            State::Running { .. } => State::Held(self.media_now(now).unwrap_or_default()),
            held @ State::Held(_) => held,
        };
    }

    /// Starts the clock again from where it was held.
    pub fn release(&mut self, now: Instant) {
        self.state = match self.state {
            State::Waiting { .. } => State::Waiting { held: false },
            State::Seeking { .. } => State::Seeking { held: false },
            State::Held(media) => State::Running { anchor: now, media },
            running @ State::Running { .. } => running,
        };
    }

    fn running_media(&self, now: Instant) -> Option<Duration> {
        match self.state {
            State::Running { anchor, media } => Some(
                media
                    + now
                        .saturating_duration_since(anchor)
                        .mul_f64(self.speed.factor()),
            ),
            _ => None,
        }
    }

    /// Where the clip is, once it has started.
    pub fn media_now(&self, now: Instant) -> Option<Duration> {
        match self.state {
            State::Held(media) => Some(media),
            _ => self.running_media(now),
        }
    }

    fn peek(&mut self, pull: &mut impl FnMut() -> Option<F>) -> Option<&F> {
        if self.next.is_none() {
            self.next = pull();
        }
        self.next.as_ref()
    }

    /// Whether a new frame should be shown now. `pull` takes the next frame from the decoder.
    pub fn due(&mut self, now: Instant, mut pull: impl FnMut() -> Option<F>) -> bool {
        match self.state {
            State::Waiting { held: false } | State::Seeking { .. } => {
                self.peek(&mut pull).is_some()
            }
            State::Running { .. } => {
                let media = self.running_media(now);
                self.peek(&mut pull)
                    .is_some_and(|frame| media.is_some_and(|m| frame.pts() <= m))
            }
            State::Waiting { held: true } | State::Held(_) => false,
        }
    }

    /// The frame to show now, if it changed: the newest the clock has reached, dropping any it
    /// overtook. The first frame of a clip, or after a seek, shows as soon as it exists and
    /// starts the clock, or holds it there after a seek while held.
    pub fn advance(&mut self, now: Instant, mut pull: impl FnMut() -> Option<F>) -> Option<F> {
        if matches!(
            self.state,
            State::Waiting { held: false } | State::Seeking { .. }
        ) {
            let media = self.peek(&mut pull)?.pts();
            self.state = match self.state {
                State::Seeking { held: true } => State::Held(media),
                _ => State::Running { anchor: now, media },
            };
            return self.next.take();
        }
        let mut latest = None;
        while self.due(now, &mut pull) {
            latest = self.next.take();
        }
        latest
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct F(u64);

    impl Timed for F {
        fn pts(&self) -> Duration {
            Duration::from_millis(self.0)
        }
    }

    /// Frames every 100 ms from 0, as a decoder would hand them over.
    fn decoder() -> VecDeque<F> {
        (0..10).map(|i| F(i * 100)).collect()
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn speeds_step_slower_then_wrap() {
        assert_eq!(Speed::Full.next(), Speed::Half);
        assert_eq!(Speed::Half.next(), Speed::Quarter);
        assert_eq!(Speed::Quarter.next(), Speed::Full);
    }

    #[test]
    fn the_first_frame_shows_at_once_and_starts_the_clock() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now() + ms(5000);
        assert_eq!(pacing.media_now(t0), None);
        assert!(pacing.due(t0, || frames.pop_front()));
        assert_eq!(pacing.advance(t0, || frames.pop_front()), Some(F(0)));
        assert_eq!(pacing.media_now(t0 + ms(40)), Some(ms(40)));
        assert!(
            !pacing.due(t0 + ms(40), || frames.pop_front()),
            "frame 100 is not due yet"
        );
        assert_eq!(
            pacing.advance(t0 + ms(100), || frames.pop_front()),
            Some(F(100))
        );
    }

    #[test]
    fn frames_the_clock_overtook_are_dropped() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        assert_eq!(
            pacing.advance(t0 + ms(350), || frames.pop_front()),
            Some(F(300))
        );
        assert_eq!(
            frames.front(),
            Some(&F(500)),
            "400 was taken and waits for its time"
        );
    }

    #[test]
    fn nothing_new_shows_while_held() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        pacing.hold(t0 + ms(150));
        assert!(pacing.is_held());
        assert_eq!(pacing.media_now(t0 + ms(900)), Some(ms(150)));
        assert!(!pacing.due(t0 + ms(900), || frames.pop_front()));
        assert_eq!(pacing.advance(t0 + ms(900), || frames.pop_front()), None);
        // Released a second later, it carries on from 150 ms.
        pacing.release(t0 + ms(1150));
        assert_eq!(pacing.media_now(t0 + ms(1200)), Some(ms(200)));
        assert_eq!(
            pacing.advance(t0 + ms(1200), || frames.pop_front()),
            Some(F(200))
        );
    }

    #[test]
    fn a_hold_before_the_first_frame_keeps_it_back() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.hold(t0);
        assert!(!pacing.due(t0, || frames.pop_front()));
        pacing.release(t0);
        assert_eq!(pacing.advance(t0, || frames.pop_front()), Some(F(0)));
    }

    #[test]
    fn slow_motion_changes_speed_without_a_jump() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        pacing.set_speed(Speed::Quarter, t0 + ms(200));
        assert_eq!(pacing.media_now(t0 + ms(200)), Some(ms(200)));
        assert_eq!(pacing.media_now(t0 + ms(600)), Some(ms(300)));
        assert_eq!(pacing.speed(), Speed::Quarter);
    }

    #[test]
    fn reset_starts_a_new_clip_from_its_first_frame() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        pacing.set_speed(Speed::Half, t0);
        pacing.reset();
        assert_eq!(pacing.media_now(t0), None);
        assert_eq!(pacing.speed(), Speed::Half, "the speed carries over");
        let mut next_clip = VecDeque::from([F(0)]);
        assert_eq!(pacing.advance(t0, || next_clip.pop_front()), Some(F(0)));
    }

    #[test]
    fn a_seek_while_playing_starts_the_clock_at_the_next_frame() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        pacing.advance(t0 + ms(250), || frames.pop_front());
        pacing.seek();
        assert_eq!(
            pacing.media_now(t0 + ms(300)),
            None,
            "no frame from after the seek yet"
        );
        let mut after = VecDeque::from([F(700), F(800)]);
        assert_eq!(
            pacing.advance(t0 + ms(300), || after.pop_front()),
            Some(F(700)),
            "the frame held back from before the seek is dropped"
        );
        assert_eq!(pacing.media_now(t0 + ms(350)), Some(ms(750)));
        assert!(!pacing.is_held());
    }

    #[test]
    fn a_seek_while_paused_shows_that_frame_and_stays_paused() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        pacing.hold(t0 + ms(150));
        pacing.seek();
        assert!(!pacing.is_held(), "still takes the frame it was moved to");
        let mut after = VecDeque::from([F(600), F(700)]);
        assert_eq!(
            pacing.advance(t0 + ms(500), || after.pop_front()),
            Some(F(600))
        );
        assert!(pacing.is_held());
        assert_eq!(pacing.media_now(t0 + ms(900)), Some(ms(600)));
        assert_eq!(pacing.advance(t0 + ms(900), || after.pop_front()), None);
        pacing.release(t0 + ms(1000));
        assert_eq!(pacing.media_now(t0 + ms(1100)), Some(ms(700)));
    }

    #[test]
    fn a_pause_just_after_a_seek_shows_the_frame_it_moved_to() {
        let mut frames = decoder();
        let mut pacing = Pacing::default();
        let t0 = Instant::now();
        pacing.advance(t0, || frames.pop_front());
        pacing.seek();
        pacing.hold(t0 + ms(50));
        let mut after = VecDeque::from([F(600), F(700)]);
        assert_eq!(
            pacing.advance(t0 + ms(100), || after.pop_front()),
            Some(F(600))
        );
        assert!(pacing.is_held());
        assert_eq!(pacing.media_now(t0 + ms(900)), Some(ms(600)));
    }
}
