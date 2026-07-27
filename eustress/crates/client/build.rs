//! Renders `assets/icon.svg` into the icon formats the Player needs, and
//! embeds the Windows one into `eustress-client.exe`.
//!
//! The SVG is the single source of truth. Everything else — the multi-size
//! `.ico` in the PE resource table, and the 256px PNG the runtime hands to
//! winit — is generated here into `OUT_DIR`, so the two can never drift and no
//! generated binary lands in the source tree.
//!
//! Two surfaces need two different things, which is why both are produced:
//! Explorer, the desktop and the Start menu read the `.ico` out of the
//! executable, while the running window's title bar, Alt-Tab entry and taskbar
//! button do not — winit registers its window class with `hIcon: 0` and
//! Windows does not fall back to the executable's resource. See
//! `src/systems/window_icon.rs`.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.svg");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR unset"));
    let svg_path = Path::new("assets/icon.svg");
    let ico_path = out_dir.join("icon.ico");
    let png_path = out_dir.join("icon_256.png");

    let svg = std::fs::read(svg_path).expect("assets/icon.svg is missing");
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())
        .expect("assets/icon.svg failed to parse");

    let render = |size: u32| {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
        let scale = size as f32 / tree.size().width().max(tree.size().height());
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        pixmap
    };

    // Windows picks the closest entry to whatever it is drawing, so shipping
    // the small sizes matters: without a hand-rendered 16px it downsamples the
    // 256 and the result is mush at taskbar scale.
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [256u32, 128, 64, 48, 32, 24, 16] {
        // `from_rgba_data` takes RGBA and converts internally. Handing it BGRA
        // "to match the ICO format" transposes red and blue in every entry —
        // the bug this crate's sibling shipped for the Studio gear.
        let image = ico::IconImage::from_rgba_data(size, size, render(size).take());
        dir.add_entry(ico::IconDirEntry::encode(&image).expect("ICO encode failed"));
    }
    dir.write(BufWriter::new(
        File::create(&ico_path).expect("could not create icon.ico"),
    ))
    .expect("could not write icon.ico");

    render(256).save_png(&png_path).expect("could not write icon_256.png");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("non-UTF8 OUT_DIR"));
        res.set("ProductName", "Eustress Player");
        res.set("FileDescription", "Eustress Player");
        if let Err(e) = res.compile() {
            // Not fatal: a machine without the Windows SDK resource compiler
            // should still be able to build and run the Player.
            println!("cargo:warning=icon embed skipped: {e}");
        }
    }
}
