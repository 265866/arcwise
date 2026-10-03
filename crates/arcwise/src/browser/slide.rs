//! The lineup panel's slide in and out beside the map.

use std::time::{Duration, Instant};

pub(super) const DURATION: Duration = Duration::from_millis(420);

/// How far open the panel is, from 0 (closed) to 1 (open), animating between the two.
#[derive(Clone, Copy, Debug)]
pub(super) struct Slide {
    open: bool,
    /// Where it was when it last changed direction.
    from: f32,
    since: Instant,
}

impl Slide {
    pub fn closed() -> Slide {
        Slide {
            open: false,
            from: 0.,
            since: Instant::now() - DURATION,
        }
    }

    /// Heads open or closed. Turning back partway starts from where it is, without a jump.
    pub fn set(&mut self, open: bool, now: Instant) {
        if open != self.open {
            self.from = self.at(now);
            self.open = open;
            self.since = now;
        }
    }

    pub fn at(&self, now: Instant) -> f32 {
        let t = now.saturating_duration_since(self.since).as_secs_f32() / DURATION.as_secs_f32();
        let to = if self.open { 1. } else { 0. };
        // Exactly there once done, so a closed panel reads as 0 and lets go of its lineup.
        if t >= 1. {
            return to;
        }
        self.from + (to - self.from) * ease(t)
    }

    pub fn moving(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) < DURATION
    }
}

/// CSS `cubic-bezier(0.2, 0.8, 0.2, 1)`: quick to start, gentle to settle.
fn ease(t: f32) -> f32 {
    let bezier = |p1: f32, p2: f32, s: f32| {
        let u = 1. - s;
        3. * u * u * s * p1 + 3. * u * s * s * p2 + s * s * s
    };
    // x(s) rises monotonically, so bisection finds the s whose x is t.
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if bezier(0.2, 0.2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier(0.8, 1., (lo + hi) / 2.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_from_zero_to_one_without_going_back() {
        assert!(ease(0.).abs() < 1e-4);
        assert!((ease(1.) - 1.).abs() < 1e-4);
        let samples: Vec<f32> = (0..=20).map(|i| ease(i as f32 / 20.)).collect();
        assert!(samples.windows(2).all(|w| w[1] >= w[0]));
        assert!(
            ease(0.25) > 0.6,
            "most of the way there early: {}",
            ease(0.25)
        );
    }

    #[test]
    fn opens_over_the_duration() {
        let start = Instant::now();
        let mut slide = Slide::closed();
        assert_eq!(slide.at(start), 0.);
        slide.set(true, start);
        assert!(slide.moving(start));
        assert!(slide.at(start) < 1e-4);
        assert_eq!(slide.at(start + DURATION), 1.);
        assert!(!slide.moving(start + DURATION));
    }

    #[test]
    fn turning_back_starts_where_it_is() {
        let start = Instant::now();
        let mut slide = Slide::closed();
        slide.set(true, start);
        let midway = start + DURATION / 4;
        let there = slide.at(midway);
        slide.set(false, midway);
        assert!((slide.at(midway) - there).abs() < 1e-6);
        assert_eq!(slide.at(midway + DURATION), 0.);
    }
}
