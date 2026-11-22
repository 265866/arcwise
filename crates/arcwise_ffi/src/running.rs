//! The named mutex that tells the uninstaller Arcwise is running.

use std::io;

use windows::Win32::System::Threading;
use windows::core::HSTRING;

/// The mutex's name. The installer script checks for it under the same name, so changing it
/// breaks uninstalling while Arcwise runs.
pub const RUNNING_MUTEX: &str = "Arcwise.Running";

/// Creates the named mutex [`RUNNING_MUTEX`] for the rest of the process's life: its handle is
/// never closed, so Windows releases it when the process ends.
///
/// # Errors
///
/// The error Windows gives when it can't create the mutex.
#[expect(
    unsafe_code,
    reason = "CreateMutexW is a Win32 call with no safe binding"
)]
pub fn announce_running() -> io::Result<()> {
    let name = HSTRING::from(RUNNING_MUTEX);
    // SAFETY: `name` is a NUL-terminated wide string that outlives the call, and no security
    // attributes are passed.
    unsafe { Threading::CreateMutexW(None, false, &name) }
        .map(drop)
        .map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{self, ERROR_ALREADY_EXISTS};

    #[test]
    #[expect(
        unsafe_code,
        reason = "the test opens the mutex through CreateMutexW, as the uninstaller's check does"
    )]
    fn a_second_open_finds_the_mutex_by_its_name() {
        announce_running().expect("the mutex is created");
        let name = HSTRING::from(RUNNING_MUTEX);
        // SAFETY: `name` is a NUL-terminated wide string that outlives the call, and no security
        // attributes are passed.
        let opened = unsafe { Threading::CreateMutexW(None, false, &name) };
        let handle = opened.expect("the mutex opens");
        // SAFETY: a plain read of this thread's last error, which CreateMutexW just set.
        let existed = unsafe { Foundation::GetLastError() } == ERROR_ALREADY_EXISTS;
        // SAFETY: `handle` is the one this test opened, closed once.
        unsafe { Foundation::CloseHandle(handle) }.expect("the test's handle closes");
        assert!(existed, "{RUNNING_MUTEX} already existed when opened again");
    }
}
