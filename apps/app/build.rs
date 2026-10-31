//! Builds Gitto's icon and name into its Windows .exe: Windows takes the task bar's icon, and the
//! name Task Manager and Explorer show, from there. GPUI gives its windows the .exe's icon with
//! the id 1.

fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon_with_id("resources/icon.ico", "1")
        .set("ProductName", "Gitto")
        .set("FileDescription", "Gitto")
        .set("InternalName", "gitto")
        .set("OriginalFilename", "gitto.exe");
    if let Err(error) = resource.compile() {
        panic!("Couldn't build Gitto's icon into the .exe: {error}");
    }
}
