//! Lineup libraries on the web, each mirrored in a folder of its own so the rest of Arcwise reads
//! it as a folder; keeping one on disk to use offline; and reading the list of libraries shown,
//! from folders and mirrors alike, into parts for `arcwise_core` to merge.
//!
//! The libraries known, the files being fetched and the folders being deleted are kept behind
//! locks shared by every thread. A function that takes one panics if another thread panicked
//! while holding it, which a release build never reaches: there, the first panic ends Arcwise.

pub mod downloads;
pub mod remote;
pub mod sources;
