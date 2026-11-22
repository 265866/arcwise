//! Reaching the Windows window under a window handle, for what GPUI doesn't do itself.

use std::ffi::c_void;
use std::ptr;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{self, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::WindowsAndMessaging;

/// The Win32 handle of `window`, when it has one.
pub(crate) fn hwnd(window: &impl HasWindowHandle) -> Option<HWND> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return None;
    };
    let address = win32.hwnd.get().cast_unsigned();
    Some(HWND(ptr::with_exposed_provenance_mut::<c_void>(address)))
}

/// The monitor Windows says `window` is on, as the number GPUI's display ids are made from: the
/// monitor's handle. That monitor's scale is the one the window's bounds are in, while GPUI's own
/// idea of the window's monitor can lag behind as it is dragged between monitors at different
/// scales. `None` while the window is minimized, or for a window with no Win32 handle.
#[expect(
    unsafe_code,
    reason = "IsIconic and MonitorFromWindow are Win32 calls with no safe binding"
)]
#[must_use]
pub fn monitor(window: &impl HasWindowHandle) -> Option<u64> {
    let hwnd = hwnd(window)?;
    // SAFETY: `hwnd` is the handle of a window that `window` keeps alive for this call.
    if unsafe { WindowsAndMessaging::IsIconic(hwnd) }.as_bool() {
        return None;
    }
    // SAFETY: as above; with MONITOR_DEFAULTTONEAREST, Windows always returns a monitor.
    let monitor = unsafe { Gdi::MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    u64::try_from(monitor.0.expose_provenance()).ok()
}
