//! Plays video files through Windows Media Foundation on a thread of its own.
//!
//! The thread owns one Direct3D 11 device for its whole life, because creating it is the slow
//! part of opening a clip, and opens a source reader per clip. The reader demuxes the file,
//! decodes it on the GPU when the codec allows, and converts it to BGRA on the GPU too. Frames
//! are read back into memory, since the UI cannot sample a texture from another device, and sent
//! a few ahead of the presentation clock.

mod command;
mod decode;
mod decoder;
mod event;
mod frame;

pub use decoder::Decoder;
pub use event::Event;
pub use frame::Frame;
