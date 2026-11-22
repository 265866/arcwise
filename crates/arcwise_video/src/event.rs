use std::time::Duration;

/// What became of a clip asked for with [`Decoder::open`](crate::Decoder::open).
#[derive(Clone, Debug, PartialEq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "only the arcwise binary in this workspace matches on it, and it handles every \
              variant, so a new one should fail its build rather than fall into a wildcard"
)]
pub enum Event {
    /// The clip opened, and its frames follow.
    Opened {
        /// The `generation` of the open.
        generation: u64,
        /// The width of its frames in pixels, after any scaling.
        width: u32,
        /// The height of its frames in pixels, after any scaling.
        height: u32,
        /// How long one pass through the clip lasts.
        duration: Duration,
    },
    /// The clip could not be opened or stopped playing on an error; no more frames of it follow.
    Failed {
        /// The `generation` of the open, or of the last seek, which moves a clip on to a newer
        /// one.
        generation: u64,
        /// What failed, starting with the clip's path.
        error: String,
        /// Windows has no decoder for the clip's codec, such as AV1 without the AV1 Video
        /// Extension.
        codec_missing: bool,
    },
}
