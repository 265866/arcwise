#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use arcwise_video::{Decoder, Event, Frame};

const DEADLINE: Duration = Duration::from_secs(10);

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
    loop {
        let frame = decoder
            .frames()
            .recv_timeout(DEADLINE.saturating_sub(start.elapsed()))
            .expect("a frame before the deadline");
        if frame.generation == generation {
            return frame;
        }
    }
}

#[test]
fn decodes_a_clip_to_bgra_frames() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture());
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
    decoder.open(7, fixture());
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
    decoder.open(1, fixture());
    next_frame(&decoder, 1);
    decoder.open(2, fixture());
    let frame = next_frame(&decoder, 2);
    assert_eq!(frame.pts, Duration::ZERO, "starts from the beginning");
}

#[test]
fn reports_a_missing_file() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(3, "no/such/clip.mp4");
    match next_event(&decoder) {
        Event::Failed { generation, error } => {
            assert_eq!(generation, 3);
            assert!(error.contains("clip.mp4"), "{error}");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn stop_pauses_until_the_next_open() {
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, fixture());
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
    decoder.open(2, fixture());
    next_frame(&decoder, 2);
}

/// Decodes a clip of your own, such as an AV1 one, which needs Windows' AV1 Video Extension:
/// `ARCWISE_CLIP=<file> cargo test -p arcwise-video -- --ignored`.
#[test]
#[ignore = "needs a clip on disk"]
fn decodes_a_real_clip() {
    let path = std::env::var_os("ARCWISE_CLIP").expect("ARCWISE_CLIP is set");
    let decoder = Decoder::spawn().unwrap();
    decoder.open(1, PathBuf::from(path));
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
