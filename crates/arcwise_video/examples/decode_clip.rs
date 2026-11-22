//! Decodes a clip of your own, such as an AV1 one, which needs Windows' AV1 Video Extension, and
//! reports how many frames a second it decodes:
//! `cargo run -p arcwise_video --example decode_clip -- <file>`.
//!
//! Exits with a failure when the clip does not open, or decodes slower than it plays, at 30
//! frames a second.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use std::{env, io};

use arcwise_video::{Decoder, Event};

/// Generous: the first clip opened on a freshly set-up machine can take over ten seconds while
/// Windows prepares its decoders.
const DEADLINE: Duration = Duration::from_secs(60);

/// How many frames to time.
const FRAMES: u32 = 60;

/// The rate the clip must decode faster than to play smoothly.
const PLAYBACK_FPS: f64 = 30.0;

fn main() -> io::Result<ExitCode> {
    use std::io::Write as _;

    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let Some(path) = env::args_os().nth(1).map(PathBuf::from) else {
        writeln!(stderr, "usage: decode_clip <clip>")?;
        return Ok(ExitCode::FAILURE);
    };
    let decoder = Decoder::spawn()?;
    decoder.open(1, path.clone(), None);
    let (width, height) = match decoder.events().recv_timeout(DEADLINE) {
        Ok(Event::Opened { width, height, .. }) => (width, height),
        Ok(Event::Failed { error, .. }) => {
            writeln!(stderr, "the clip did not open: {error}")?;
            return Ok(ExitCode::FAILURE);
        }
        Err(e) => {
            writeln!(stderr, "the clip did not open in {DEADLINE:?}: {e}")?;
            return Ok(ExitCode::FAILURE);
        }
    };
    let expected = width as usize * height as usize * 4;
    let start = Instant::now();
    for _ in 0..FRAMES {
        let frame = match decoder.frames().recv_timeout(DEADLINE) {
            Ok(frame) => frame,
            Err(e) => {
                let events: Vec<Event> = decoder.events().try_iter().collect();
                writeln!(stderr, "no frame in {DEADLINE:?}: {e}; events: {events:?}")?;
                return Ok(ExitCode::FAILURE);
            }
        };
        if frame.bgra.len() != expected {
            writeln!(
                stderr,
                "a {}x{} frame holds {} bytes, not {expected}",
                frame.width,
                frame.height,
                frame.bgra.len()
            )?;
            return Ok(ExitCode::FAILURE);
        }
    }
    let fps = f64::from(FRAMES) / start.elapsed().as_secs_f64();
    writeln!(stdout, "{}: {width}x{height}, {fps:.0} fps", path.display())?;
    if fps > PLAYBACK_FPS {
        Ok(ExitCode::SUCCESS)
    } else {
        writeln!(
            stderr,
            "decodes slower than it plays: {fps:.0} fps, not above {PLAYBACK_FPS:.0}"
        )?;
        Ok(ExitCode::FAILURE)
    }
}
