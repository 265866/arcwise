//! Gives the Windows executable its icon and version information. GPUI loads icon resource 1
//! for the window and the taskbar.

fn main() {
    println!("cargo:rerun-if-changed=assets/arcwise.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/arcwise.ico")
        .set("FileDescription", "Arcwise")
        .set("ProductName", "Arcwise");
    if let Err(e) = resource.compile() {
        panic!("could not embed the icon and version information: {e}");
    }
}
