//! Safe wrappers over the foreign calls Arcwise makes beside GPUI: Windows' theme setting, the
//! title bar's theme, the monitor a window is on, the keyboard layout, the mutex that tells the
//! uninstaller Arcwise is running, and libwebp's decoder. Each wrapper checks what the call
//! needs, so the rest of Arcwise holds no unsafe code.

mod appearance;
mod keyboard;
mod running;
mod webp;
mod window;

pub use appearance::{Appearance, apps_use_light_theme, set_title_bar};
pub use keyboard::shifted;
pub use running::{RUNNING_MUTEX, announce_running};
pub use webp::{Bgra, WebpError, decode_bgra, encode_lossless_rgba};
pub use window::monitor;
