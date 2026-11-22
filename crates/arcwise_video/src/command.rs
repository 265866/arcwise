use std::path::PathBuf;
use std::time::Duration;

/// A request from the [`Decoder`](crate::Decoder) to its thread.
pub(crate) enum Command {
    /// See [`Decoder::open`](crate::Decoder::open).
    Open {
        generation: u64,
        path: PathBuf,
        max_height: Option<u32>,
    },
    /// See [`Decoder::stop`](crate::Decoder::stop).
    Stop,
    /// See [`Decoder::seek`](crate::Decoder::seek).
    Seek { generation: u64, position: Duration },
    /// See [`Decoder::prepare`](crate::Decoder::prepare).
    Prepare {
        path: PathBuf,
        max_height: Option<u32>,
    },
}
