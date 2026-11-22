//! Gives the Windows executable its icon and version information. GPUI loads icon resource 1
//! for the window and the taskbar.

use winresource::WindowsResource;

fn main() {
    println!("cargo::rerun-if-changed=assets/arcwise.ico");
    WindowsResource::new()
        .set_icon("assets/arcwise.ico")
        .set("FileDescription", "Arcwise")
        .set("ProductName", "Arcwise")
        .compile()
        .expect("the icon and version information embed in the executable");
}
