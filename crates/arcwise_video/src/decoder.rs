use std::io;
use std::path::PathBuf;
use std::thread::Builder;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::command::Command;
use crate::decode;
use crate::event::Event;
use crate::frame::Frame;

/// Frames decoded ahead of display. Three is 100 ms at 30 fps: enough to ride over a slow
/// readback or the seek back to the start, and little to throw away when the clip changes.
const AHEAD: usize = 3;

/// A handle to the decode thread. Dropping it ends the thread.
#[derive(Debug)]
pub struct Decoder {
    commands: Sender<Command>,
    frames: Receiver<Frame>,
    events: Receiver<Event>,
}

impl Decoder {
    /// Starts the decode thread, with nothing playing.
    ///
    /// # Errors
    ///
    /// Fails when the operating system cannot start a thread.
    pub fn spawn() -> io::Result<Decoder> {
        let (commands, command_rx) = crossbeam_channel::unbounded();
        let (frame_tx, frames) = crossbeam_channel::bounded(AHEAD);
        let (event_tx, events) = crossbeam_channel::unbounded();
        Builder::new()
            .name(String::from("arcwise-video"))
            .spawn(move || decode::run(command_rx, &frame_tx, &event_tx))?;
        Ok(Decoder {
            commands,
            frames,
            events,
        })
    }

    /// Starts playing `path` from its start, looping, in place of whatever was playing. Frames
    /// and events for it carry `generation`; anything with an older one can be dropped.
    ///
    /// With `max_height`, frames are scaled down on the GPU to at most that many rows, keeping
    /// the aspect ratio. Smaller frames are cheaper to copy back and draw.
    pub fn open(&self, generation: u64, path: PathBuf, max_height: Option<u32>) {
        self.send(Command::Open {
            generation,
            path,
            max_height,
        });
    }

    /// Opens `path` ahead of time and decodes its first frame, so that an [`Decoder::open`] of
    /// it with the same `max_height` shows that frame at once rather than about 20 ms later.
    /// Only the latest clip prepared is kept, and a clip playing carries on meanwhile.
    pub fn prepare(&self, path: PathBuf, max_height: Option<u32>) {
        self.send(Command::Prepare { path, max_height });
    }

    /// Carries on playing the open clip from `position`, starting with the frame showing then.
    /// Its frames carry `generation`, as after an open; without a clip playing, nothing happens.
    pub fn seek(&self, generation: u64, position: Duration) {
        self.send(Command::Seek {
            generation,
            position,
        });
    }

    /// Stops decoding until the next [`Decoder::open`].
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// The decoded frames, at most three waiting at a time.
    #[must_use]
    pub fn frames(&self) -> &Receiver<Frame> {
        &self.frames
    }

    /// Each clip's [`Event::Opened`] or [`Event::Failed`].
    #[must_use]
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    /// Hands `command` to the decode thread.
    fn send(&self, command: Command) {
        #[expect(
            let_underscore_drop,
            clippy::let_underscore_must_use,
            reason = "sending fails only once the decode thread has gone, which while this \
                      handle lives means it panicked, and then no one is left to tell"
        )]
        let _ = self.commands.send(command);
    }
}
