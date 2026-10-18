//! Prepares Azura's logo for the UI.
//!
//! The master render (`assets/logo/azura.png`, made in Blender from
//! `assets/logo/azura-logo.blend`) is downscaled once here, so the UI never
//! samples a 1024px image down while rendering. Packaging sizes are produced
//! separately, by `cargo run --example package`.

use std::path::Path;

const MASTER: &str = "assets/logo/azura.png";

/// Sizes the UI asks for: a 32px icon on a 200% display, and the welcome
/// screen's larger mark.
const UI_SIZES: [u32; 2] = [64, 192];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={MASTER}");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let out = Path::new(&out_dir);

    let master = image::open(MASTER).expect("read the logo").to_rgba8();
    for size in UI_SIZES {
        resize(&master, size)
            .save(out.join(format!("azura-{size}.png")))
            .expect("write a logo size");
    }
}

fn resize(master: &image::RgbaImage, size: u32) -> image::RgbaImage {
    image::imageops::resize(master, size, size, image::imageops::FilterType::Lanczos3)
}
