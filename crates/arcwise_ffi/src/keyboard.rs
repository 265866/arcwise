//! Which key types a symbol, in the keyboard layout in use.

use windows::Win32::UI::Input::KeyboardAndMouse::{self, MAPVK_VK_TO_CHAR};

/// The modifier byte `VkKeyScanW` returns for Shift alone.
const SHIFT: u8 = 1;
/// The bit `MapVirtualKeyW` sets on a dead key, such as an accent.
const DEAD_KEY: u32 = 0x80_00_00_00;

/// The key that types `symbol` with Shift in the keyboard layout in use, such as `1` for `!` on a
/// US keyboard. `None` when it is typed without Shift, or needs more than Shift.
#[expect(
    unsafe_code,
    reason = "VkKeyScanW and MapVirtualKeyW are Win32 calls with no safe binding"
)]
#[must_use]
pub fn shifted(symbol: char) -> Option<char> {
    let unit = u16::try_from(u32::from(symbol)).ok()?;
    // SAFETY: a plain lookup in the thread's keyboard layout, with no pointers.
    let code = unsafe { KeyboardAndMouse::VkKeyScanW(unit) };
    let virtual_key = u32::from(code.to_le_bytes()[0]);
    // SAFETY: as above.
    let base = unsafe { KeyboardAndMouse::MapVirtualKeyW(virtual_key, MAPVK_VK_TO_CHAR) };
    shifted_from(code, base, symbol)
}

/// [`shifted`] from what Windows says: `code` from `VkKeyScanW`, the key in its low byte and the
/// modifiers in its high byte, and `base`, the key's own character from `MapVirtualKeyW`.
fn shifted_from(code: i16, base: u32, symbol: char) -> Option<char> {
    let [_, modifiers] = code.to_le_bytes();
    if code == -1 || modifiers != SHIFT {
        return None;
    }
    char::from_u32(base & !DEAD_KEY)
        .filter(|c| *c != symbol && !c.is_control())
        .map(|c| c.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symbol_typed_with_shift_names_its_key() {
        assert_eq!(
            shifted_from(0x0131, u32::from('1'), '!'),
            Some('1'),
            "VkKeyScanW('!') on a US layout is VK_1 (0x31) with Shift (1)"
        );
        assert_eq!(
            shifted_from(0x00bf, u32::from('/'), '/'),
            None,
            "VkKeyScanW('/') on a US layout is VK_OEM_2 (0xBF) with no modifiers"
        );
        assert_eq!(
            shifted_from(0x0632, u32::from('2'), '@'),
            None,
            "a key that needs Ctrl and Alt as well, as AltGr keys do"
        );
        assert_eq!(
            shifted_from(-1, 0, '€'),
            None,
            "a symbol that isn't on the keyboard at all"
        );
    }
}
