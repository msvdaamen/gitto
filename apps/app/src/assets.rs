//! The app's assets: Gitto's own art (its logo, the mascot), and every Lucide icon, which
//! `IconName` names (`gpui_kit::assets::IconName`, the full catalog, so a screen can use any icon
//! without registering it).

use std::borrow::Cow;

use gpui_kit::{AssetSource, SharedString};

/// Gitto's art, by its path.
const BRAND: [(&str, &[u8]); 4] = [
    (
        "brand/gitto.svg",
        include_bytes!("../assets/brand/gitto.svg"),
    ),
    (
        "brand/mascot.svg",
        include_bytes!("../assets/brand/mascot.svg"),
    ),
    (
        "brand/hero-mascot-dark.svg",
        include_bytes!("../assets/brand/hero-mascot-dark.svg"),
    ),
    (
        "brand/hero-mascot-light.svg",
        include_bytes!("../assets/brand/hero-mascot-light.svg"),
    ),
];

/// Gitto's art, then every icon (see the module's doc).
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = BRAND.iter().find(|(brand, _)| *brand == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths: Vec<SharedString> = BRAND
            .iter()
            .filter(|(brand, _)| brand.starts_with(path))
            .map(|(brand, _)| SharedString::new_static(brand))
            .collect();
        paths.extend(gpui_kit::assets::AllAssets.list(path)?);
        Ok(paths)
    }
}
