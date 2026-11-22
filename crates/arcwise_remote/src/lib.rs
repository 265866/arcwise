//! Lineup libraries on the web, each mirrored in a folder of its own so the rest of Arcwise reads
//! it as a folder; keeping one on disk to use offline; and reading the list of libraries shown,
//! from folders and mirrors alike, into parts for `arcwise_core` to merge.

pub mod downloads;
pub mod remote;
pub mod sources;
