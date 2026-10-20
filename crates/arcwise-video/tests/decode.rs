#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use arcwise_video::{Decoder, Event, Frame};

/// Generous: the first clip opened on a freshly set-up machine, such as a CI runner, can take
/// over ten seconds while Windows prepares its decoders. Later ones take milliseconds.
const DEADLINE: Duration = Duration::from_secs(60);

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/testsrc.mp4")
}

fn next_event(decoder: &Decoder) -> Event {
    decoder
        .events()
        .recv_timeout(DEADLINE)
        .expect("an event before the deadline")
}

fn next_frame(decoder: &Decoder, generation: u64) -> Frame {
    let start = Instant::now();
    let mut seen = 0;
    loop {
        let Ok(frame) = decoder
            .frames()
            .recv_timeout(DEADLINE.saturating_sub(start.elapsed()))
        else {
            // Say why: the decode thread reports a failure as an event, not a frame.
            let events: Vec<Event> = decoder.events().try_iter().collect();
            panic!(
                "no frame of clip {generation} in {DEADLINE:?} after {seen} others; events: {events:?}"
            );
        };
        if frame.generation == generation {
            return frame;
        }
        seen += 1;
    }
}

#[test]
fn decodes_a_clip_to_bgra_frames() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture(), None);
    assert_eq!(
        next_event(&decoder),
        Event::Opened {
            generation: 1,
            width: 320,
            height: 180,
            duration: Duration::from_secs(1),
        }
    );
    let frame = next_frame(&decoder, 1);
    assert_eq!((frame.width, frame.height), (320, 180));
    assert_eq!(frame.bgra.len(), 320 * 180 * 4);
    assert_eq!(frame.pts, Duration::ZERO);
    assert!(
        frame.bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 255),
        "opaque"
    );
    // testsrc2 is colorful; an all-black frame would mean the copy back failed.
    assert!(
        frame
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[..3] != [0, 0, 0])
    );
}

#[test]
fn loops_with_presentation_times_that_keep_rising() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(7, fixture(), None);
    let mut last = None;
    for _ in 0..45 {
        let frame = next_frame(&decoder, 7);
        if let Some(last) = last {
            assert!(frame.pts > last, "{:?} after {last:?}", frame.pts);
        }
        last = Some(frame.pts);
    }
    assert!(
        last.unwrap() > Duration::from_secs(1),
        "past the first loop"
    );
}

#[test]
fn a_new_clip_replaces_the_old_one() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture(), None);
    next_frame(&decoder, 1);
    decoder.open(2, fixture(), None);
    let frame = next_frame(&decoder, 2);
    assert_eq!(frame.pts, Duration::ZERO, "starts from the beginning");
}

#[test]
fn reports_a_missing_file() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(3, "no/such/clip.mp4", None);
    match next_event(&decoder) {
        Event::Failed {
            generation,
            error,
            codec_missing,
        } => {
            assert_eq!(generation, 3);
            assert!(!codec_missing);
            assert!(error.contains("clip.mp4"), "{error}");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn stop_pauses_until_the_next_open() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture(), None);
    next_frame(&decoder, 1);
    decoder.stop();
    // Drain what was decoded before the stop arrived.
    std::thread::sleep(Duration::from_millis(200));
    while decoder.frames().try_recv().is_ok() {}
    assert!(
        decoder
            .frames()
            .recv_timeout(Duration::from_millis(300))
            .is_err(),
        "no frames after stop"
    );
    decoder.open(2, fixture(), None);
    next_frame(&decoder, 2);
}

#[test]
fn a_prepared_clip_opens_at_its_first_frame() {
    let decoder = Decoder::spawn().unwrap();
    decoder.prepare(fixture(), Some(90));
    decoder.open(1, fixture(), Some(90));
    assert!(matches!(
        next_event(&decoder),
        Event::Opened { generation: 1, .. }
    ));
    let first = next_frame(&decoder, 1);
    assert_eq!(
        (first.width, first.height, first.pts),
        (160, 90, Duration::ZERO)
    );
    let second = next_frame(&decoder, 1);
    assert!(second.pts > first.pts, "plays on from the prepared reader");
}

#[test]
fn preparing_does_not_interrupt_the_clip_playing() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture(), None);
    let mut last = next_frame(&decoder, 1).pts;
    decoder.prepare(fixture(), Some(90));
    for _ in 0..10 {
        let frame = next_frame(&decoder, 1);
        assert_eq!((frame.width, frame.height), (320, 180));
        assert!(frame.pts > last);
        last = frame.pts;
    }
}

#[test]
fn a_clip_prepared_at_another_size_opens_afresh() {
    let decoder = Decoder::spawn().unwrap();
    decoder.prepare(fixture(), Some(90));
    decoder.open(1, fixture(), None);
    let frame = next_frame(&decoder, 1);
    assert_eq!((frame.width, frame.height), (320, 180));
}

#[test]
fn a_missing_clip_prepared_still_fails_to_open() {
    let decoder = Decoder::spawn().unwrap();
    decoder.prepare("no/such/clip.mp4", None);
    decoder.open(4, "no/such/clip.mp4", None);
    assert!(matches!(
        next_event(&decoder),
        Event::Failed { generation: 4, .. }
    ));
}

/// Decodes a clip of your own, such as an AV1 one, which needs Windows' AV1 Video Extension:
/// `ARCWISE_CLIP=<file> cargo test -p arcwise-video -- --ignored`.
#[test]
#[ignore = "needs a clip on disk"]
fn decodes_a_real_clip() {
    let path = std::env::var_os("ARCWISE_CLIP").expect("ARCWISE_CLIP is set");
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, PathBuf::from(path), None);
    let Event::Opened { width, height, .. } = next_event(&decoder) else {
        panic!("the clip did not open");
    };
    let start = Instant::now();
    for _ in 0..60 {
        let frame = next_frame(&decoder, 1);
        assert_eq!(frame.bgra.len(), width as usize * height as usize * 4);
    }
    let fps = 60. / start.elapsed().as_secs_f64();
    assert!(fps > 30., "decodes faster than it plays: {fps:.0} fps");
}

#[test]
fn scales_frames_down_to_a_height() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture(), Some(90));
    let frame = next_frame(&decoder, 1);
    assert_eq!((frame.width, frame.height), (160, 90));
    assert_eq!(frame.bgra.len(), 160 * 90 * 4);
    decoder.open(2, fixture(), Some(1080));
    let frame = next_frame(&decoder, 2);
    assert_eq!((frame.width, frame.height), (320, 180), "never scaled up");
}
