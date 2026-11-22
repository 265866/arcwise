//! Finds, downloads and checks updates. [`latest`] asks GitHub for the latest release. [`fetch`]
//! downloads a release's installer and checks its attestation: that this repository's release
//! workflow built it on GitHub's own runners. The UI decides when each runs.

mod builder;
mod update;

pub use builder::{Builder, RELEASE_BUILDER};
pub use update::{
    Check, Kept, Outcome, RELEASES, RELEASES_PAGE, Relaunch, Update, current_version, directory,
    discard, fetch, install, installed, latest,
};
