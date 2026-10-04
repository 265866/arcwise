//! Plays video files through Windows Media Foundation on a thread of its own.
//!
//! The thread owns one Direct3D 11 device for its whole life, because creating it is the slow
//! part of opening a clip, and opens a source reader per clip. The reader demuxes the file,
//! decodes it on the GPU when the codec allows, and converts it to BGRA on the GPU too. Frames
//! are read back into memory, since the UI cannot sample a texture from another device, and sent
//! a few ahead of the presentation clock.

#![cfg(windows)]

mod decode;

use std::path::PathBuf;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};

/// Frames decoded ahead of display. Three is 100 ms at 30 fps: enough to ride over a slow
/// readback or the seek back to the start, and little to throw away when the clip changes.
const AHEAD: usize = 3;

/// One decoded frame, BGRA, top row first, rows packed with no padding.
pub struct Frame {
    /// The `generation` of the [`Decoder::open`] call this frame belongs to.
    pub generation: u64,
    /// When the frame is due, from the start of the clip. Counts on across loops, so it only
    /// ever increases within one generation.
    pub pts: Duration,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Opened {
        generation: u64,
        width: u32,
        height: u32,
        duration: Duration,
    },
    Failed {
        generation: u64,
        error: String,
    },
}

enum Command {
    Open {
        generation: u64,
        path: PathBuf,
        max_height: Option<u32>,
    },
    Stop,
}

/// A handle to the decode thread. Dropping it ends the thread.
pub struct Decoder {
    commands: Sender<Command>,
    frames: Receiver<Frame>,
    events: Receiver<Event>,
}

impl Decoder {
    pub fn spawn() -> std::io::Result<Decoder> {
        let (commands, command_rx) = unbounded();
        let (frame_tx, frames) = bounded(AHEAD);
        let (event_tx, events) = unbounded();
        std::thread::Builder::new()
            .name("arcwise-video".into())
            .spawn(move || decode::run(command_rx, frame_tx, event_tx))?;
        Ok(Decoder {
            commands,
            frames,
            events,
        })
    }

    /// Starts playing `path` from its start, looping, in place of whatever was playing. Frames
    /// and events for it carry `generation`; anything with an older one can be dropped.
    pub fn open(&self, generation: u64, path: impl Into<PathBuf>) {
        self.open_scaled(generation, path, None);
    }

    /// Like [`Decoder::open`], but scales frames down on the GPU to at most `max_height` rows,
    /// keeping the aspect ratio. Smaller frames are cheaper to copy back and draw.
    pub fn open_scaled(&self, generation: u64, path: impl Into<PathBuf>, max_height: Option<u32>) {
        // Sending fails only once the thread has gone, which reports itself through an event.
        let _ = self.commands.send(Command::Open {
            generation,
            path: path.into(),
            max_height,
        });
    }

    /// Stops decoding until the next [`Decoder::open`].
    pub fn stop(&self) {
        let _ = self.commands.send(Command::Stop);
    }

    pub fn frames(&self) -> &Receiver<Frame> {
        &self.frames
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }
}
