//! Builds Gitto's icon, name and version into its Windows .exe: Windows takes the task bar's icon,
//! and the name and version Task Manager and Explorer show, from there. GPUI gives its windows the
//! .exe's icon with the id 1. The version is the release build's `GITTO_VERSION`, as the app's is
//! (see `APP_VERSION`), rather than the crate's.

fn main() {
    println!("cargo:rerun-if-changed=resources/icon.ico");
    println!("cargo:rerun-if-env-changed=GITTO_VERSION");
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
    if let Ok(version) = std::env::var("GITTO_VERSION") {
        resource
            .set("FileVersion", &version)
            .set("ProductVersion", &version);
        if let Some(numbers) = numeric_version(&version) {
            resource
                .set_version_info(winresource::VersionInfo::FILEVERSION, numbers)
                .set_version_info(winresource::VersionInfo::PRODUCTVERSION, numbers);
        }
    }
    if let Err(error) = resource.compile() {
        panic!("Couldn't build Gitto's icon into the .exe: {error}");
    }
}

/// The four 16-bit words Windows keeps a version in, of `version`'s major, minor and patch (a
/// nightly's `1.2.4-nightly29853462` is 1.2.4.0: its number doesn't fit a word).
fn numeric_version(version: &str) -> Option<u64> {
    let release = version
        .split_once('-')
        .map_or(version, |(release, _)| release);
    let mut parts = release.split('.').map(|part| part.parse::<u16>().ok());
    let (major, minor, patch) = (parts.next()??, parts.next()??, parts.next()??);
    Some(u64::from(major) << 48 | u64::from(minor) << 32 | u64::from(patch) << 16)
}
