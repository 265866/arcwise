use std::fmt::{self, Debug, Formatter};
use std::time::Duration;

/// One decoded frame, BGRA, top row first, rows packed with no padding.
#[non_exhaustive]
pub struct Frame {
    /// The `generation` of the [`Decoder::open`](crate::Decoder::open) call this frame belongs
    /// to.
    pub generation: u64,
    /// When the frame is due, from the start of the clip. Counts on across loops, so it only
    /// ever increases within one generation.
    pub pts: Duration,
    /// The width in pixels.
    pub width: u32,
    /// The height in pixels.
    pub height: u32,
    /// The pixels, four bytes each in the order blue, green, red, alpha: `width * height * 4`
    /// bytes.
    pub bgra: Vec<u8>,
}

/// Leaves out the pixels, which would fill pages.
impl Debug for Frame {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("generation", &self.generation)
            .field("pts", &self.pts)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
