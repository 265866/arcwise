//! Windows' light or dark setting, and the theme of a window's own title bar.

use std::io;

use raw_window_handle::HasWindowHandle;
use windows::Win32::Graphics::Dwm::{self, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::System::Registry::{self, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::core::{BOOL, w};

use crate::window;

/// Light or dark, for a window's title bar and border.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    /// Dark text on a light bar.
    Light,
    /// Light text on a dark bar.
    Dark,
}

/// Whether Windows' apps use the light theme, from the setting Windows keeps for it, for when no
/// window is open yet to ask. `None` when the setting can't be read.
#[expect(
    unsafe_code,
    reason = "RegGetValueW is a Win32 call with no safe binding"
)]
#[must_use]
pub fn apps_use_light_theme() -> Option<bool> {
    let mut value = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).ok()?;
    // SAFETY: the key and value names are static wide strings, and `value` is a DWORD with its
    // size in `size`, as RRF_RT_REG_DWORD writes.
    let read = unsafe {
        Registry::RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&raw mut size),
        )
    };
    read.is_ok().then_some(value != 0)
}

/// Makes Windows' own title bar, and the window's border, light or dark. Windows otherwise
/// follows the system's theme. A window with no Win32 handle is left as it is.
///
/// # Errors
///
/// The error Windows gives when it refuses the attribute.
#[expect(
    unsafe_code,
    reason = "DwmSetWindowAttribute is a Win32 call with no safe binding"
)]
pub fn set_title_bar(window: &impl HasWindowHandle, appearance: Appearance) -> io::Result<()> {
    let Some(hwnd) = window::hwnd(window) else {
        return Ok(());
    };
    let dark = BOOL::from(appearance == Appearance::Dark);
    let size = u32::try_from(size_of::<BOOL>()).map_err(io::Error::other)?;
    // SAFETY: `hwnd` is the handle of a window that `window` keeps alive for this call, and the
    // attribute is a BOOL read from `dark`, which outlives the call, with its size in `size`.
    unsafe {
        Dwm::DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&raw const dark).cast(),
            size,
        )
    }
    .map_err(io::Error::from)
}
