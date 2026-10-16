fn main() {
    use std::path::Path;
    use std::fs::File;
    use std::io::BufWriter;
    
    // Compile Slint UI files.
    //
    // The Slint compiler descends `main.slint` and everything it imports
    // recursively, and this UI tree is now deep enough that the descent
    // exhausts the 1 MiB stack Windows gives a build script's main thread:
    // `cargo build --release -p eustress-engine` died with
    // STATUS_STACK_OVERFLOW (0xc00000fd) inside build-script-build, before a
    // single line of engine code was compiled. The debug profile only appeared
    // healthy because its build-script output was still cached from before the
    // UI grew. Stack reserve is fixed at link time and is not something a
    // profile can raise, but a build script is free to choose the stack of a
    // thread it spawns — so do the compile there.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(|| {
            let slint_config = slint_build::CompilerConfiguration::new()
                .with_style("fluent-dark".into());

            slint_build::compile_with_config(
                "ui/slint/main.slint",
                slint_config,
            ).expect("Failed to compile Slint UI");
        })
        .expect("Failed to spawn Slint compile thread")
        .join()
        .expect("Slint compile thread panicked");

    println!("cargo:rerun-if-changed=ui/slint/");

    // Which commit and day this build came from, for About.
    emit_build_info();
    
    let svg_path = Path::new("assets/icon.svg");
    let ico_path = Path::new("assets/icon.ico");
    let png_path = Path::new("assets/icon.png");
    let icns_path = Path::new("assets/icon.icns");
    
    println!("cargo:rerun-if-changed=assets/icon.svg");
    println!("cargo:rerun-if-changed=build.rs");
    
    // Convert SVG to ICO, PNG, and ICNS if SVG exists and is newer
    let should_convert = if svg_path.exists() {
        let svg_modified = std::fs::metadata(svg_path).unwrap().modified().unwrap();
        let ico_needs_update = !ico_path.exists() || {
            let ico_modified = std::fs::metadata(ico_path).unwrap().modified().unwrap();
            svg_modified > ico_modified
        };
        let png_needs_update = !png_path.exists() || {
            let png_modified = std::fs::metadata(png_path).unwrap().modified().unwrap();
            svg_modified > png_modified
        };
        let icns_needs_update = !icns_path.exists() || {
            let icns_modified = std::fs::metadata(icns_path).unwrap().modified().unwrap();
            svg_modified > icns_modified
        };
        ico_needs_update || png_needs_update || icns_needs_update
    } else {
        false
    };
    
    if should_convert {
        println!("cargo:warning=Converting SVG to ICO and PNG...");
        
        // Load and render SVG
        let svg_data = std::fs::read(svg_path).expect("Failed to read SVG file");
        let tree = resvg::usvg::Tree::from_data(&svg_data, &resvg::usvg::Options::default())
            .expect("Failed to parse SVG");
        
        // Generate ICO with multiple sizes
        let sizes = [256, 64, 48, 32, 16];
        let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
        
        for size in sizes {
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
            let scale = size as f32 / tree.size().width().max(tree.size().height());
            let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
            resvg::render(&tree, transform, &mut pixmap.as_mut());
            
            // NO channel swap. `ico::IconImage::from_rgba_data` takes RGBA and
            // does its own conversion; the swap that used to be here fed it
            // BGRA, so every ICO entry shipped with red and blue transposed.
            // The gear is #7a8290 blue-grey in the SVG and in icon.png (which
            // never went through the swap), but the .ico Windows actually
            // displays measured #7c6d64 brown-grey — the same bytes reversed.
            let image = ico::IconImage::from_rgba_data(size, size, pixmap.take());
            icon_dir.add_entry(ico::IconDirEntry::encode(&image).unwrap());
        }
        
        let file = File::create(ico_path).expect("Failed to create ICO file");
        icon_dir.write(BufWriter::new(file)).expect("Failed to write ICO file");
        println!("cargo:warning=✅ SVG converted to ICO successfully");
        
        // Generate PNG at 256x256 for window icon
        let png_size = 256u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(png_size, png_size).unwrap();
        let scale = png_size as f32 / tree.size().width().max(tree.size().height());
        let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        pixmap.save_png(png_path).expect("Failed to save PNG file");
        println!("cargo:warning=✅ SVG converted to PNG successfully");
        
        // Generate ICNS for macOS (multiple sizes required by Apple)
        // Note: icns crate expects PNG input, so we render each size and use read_png
        let icns_sizes = [16, 32, 64, 128, 256, 512];
        let mut icon_family = icns::IconFamily::new();
        
        for size in icns_sizes {
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).unwrap();
            let scale = size as f32 / tree.size().width().max(tree.size().height());
            let transform = resvg::tiny_skia::Transform::from_scale(scale, scale);
            resvg::render(&tree, transform, &mut pixmap.as_mut());
            
            // Encode as PNG in memory
            let mut png_data = Vec::new();
            let mut encoder = png::Encoder::new(&mut png_data, size, size);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("Failed to write PNG header");
            writer.write_image_data(pixmap.data()).expect("Failed to write PNG data");
            drop(writer);
            
            // Read back as icns::Image
            let image = icns::Image::read_png(std::io::Cursor::new(png_data))
                .expect("Failed to read PNG for ICNS");
            icon_family.add_icon(&image).expect("Failed to add icon to ICNS family");
        }
        
        let file = File::create(icns_path).expect("Failed to create ICNS file");
        icon_family.write(BufWriter::new(file)).expect("Failed to write ICNS file");
        println!("cargo:warning=✅ SVG converted to ICNS successfully");
    }
    
    // Windows-specific: embed icon in executable
    #[cfg(target_os = "windows")]
    {
        if !ico_path.exists() {
            panic!("Icon file not found at assets/icon.ico (and no SVG to convert)");
        }
        
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("OriginalFilename", "eustress-engine.exe");
        res.set("FileDescription", "Eustress Engine - 3D Editor");
        res.set("ProductName", "Eustress Engine");
        res.set("LegalCopyright", "Copyright © 2025 EustressEngine Contributors");
        
        println!("cargo:warning=Embedding icon from: {:?}", ico_path.canonicalize().unwrap());
        
        if let Err(e) = res.compile() {
            eprintln!("ERROR: Failed to compile Windows resources: {}", e);
            panic!("Icon embedding failed");
        } else {
            println!("cargo:warning=✅ Icon embedded in executable successfully");
        }
    }
}

/// The commit this build came from (`EUSTRESS_GIT_COMMIT`, a short hash) and
/// the day it was built (`EUSTRESS_BUILD_DATE`, YYYY-MM-DD, UTC), for the
/// About dialog. Both are empty when git is not there (a source archive);
/// About then shows only the version.
///
/// The script runs again when HEAD moves (a new commit or branch), watching
/// only the git files that exist: a watched path that is missing would make
/// it run, and the Slint compile with it, on every build.
fn emit_build_info() {
    use std::process::Command;
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!text.is_empty()).then_some(text)
    };
    let commit = git(&["rev-parse", "--short=10", "HEAD"]).unwrap_or_default();
    println!("cargo:rustc-env=EUSTRESS_GIT_COMMIT={commit}");

    let mut watched = Vec::new();
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        watched.push(head);
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(r) = git(&["rev-parse", "--git-path", &branch]) {
            watched.push(r);
        }
    }
    if let Some(packed) = git(&["rev-parse", "--git-path", "packed-refs"]) {
        watched.push(packed);
    }
    for path in watched {
        if std::path::Path::new(&path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }

    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    println!("cargo:rustc-env=EUSTRESS_BUILD_DATE={y:04}-{m:02}-{d:02}");
}

/// A day count since 1970-01-01 as (year, month, day) in the proleptic
/// Gregorian calendar (Howard Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe as i64 + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}
