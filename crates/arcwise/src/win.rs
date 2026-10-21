//! Reaching the Windows window under GPUI's, for what GPUI does not do itself.

use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{MAPVK_VK_TO_CHAR, MapVirtualKeyW, VkKeyScanW};

/// `window`'s handle. `Window` has its own `window_handle`, GPUI's, so the trait's is named.
pub fn hwnd(window: &Window) -> Option<HWND> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}

/// The key that types `symbol` with Shift in the keyboard layout in use, such as `1` for `!` on a
/// US keyboard. `None` when it is typed without Shift, or needs more than Shift.
pub fn shifted(symbol: char) -> Option<char> {
    let unit = u16::try_from(u32::from(symbol)).ok()?;
    // SAFETY: plain lookups in the thread's keyboard layout, with no pointers.
    let code = unsafe { VkKeyScanW(unit) };
    let virtual_key = u32::from(code.to_le_bytes()[0]);
    // SAFETY: as above.
    let base = unsafe { MapVirtualKeyW(virtual_key, MAPVK_VK_TO_CHAR) };
    shifted_from(code, base, symbol)
}

/// [`shifted`] from what Windows says: `code` from `VkKeyScanW`, the key in its low byte and the
/// modifiers in its high byte, and `base`, the key's own character from `MapVirtualKeyW`.
fn shifted_from(code: i16, base: u32, symbol: char) -> Option<char> {
    const SHIFT: u8 = 1;
    let [_, modifiers] = code.to_le_bytes();
    if code == -1 || modifiers != SHIFT {
        return None;
    }
    // The high bit marks a dead key, such as an accent.
    char::from_u32(base & 0x7fff_ffff)
        .filter(|c| *c != symbol && !c.is_control())
        .map(|c| c.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::shifted_from;

    #[test]
    fn a_symbol_typed_with_shift_names_its_key() {
        // VkKeyScanW('!') on a US layout: VK_1 (0x31) with Shift (1).
        assert_eq!(shifted_from(0x0131, '1' as u32, '!'), Some('1'));
        // VkKeyScanW('/') on a US layout: VK_OEM_2 (0xBF), no modifiers.
        assert_eq!(shifted_from(0x00bf, '/' as u32, '/'), None);
        // With Ctrl or Alt as well, as AltGr keys are.
        assert_eq!(shifted_from(0x0632, '2' as u32, '@'), None);
        // Not on the keyboard at all.
        assert_eq!(shifted_from(-1, 0, '€'), None);
    }
}
