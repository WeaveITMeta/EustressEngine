fn main() {
    use std::path::Path;
    use std::fs::File;
    use std::io::BufWriter;
    
    let svg_path = Path::new("assets/icon.svg");
    let ico_path = Path::new("assets/icon.ico");
    let png_path = Path::new("assets/icon.png");
    
    println!("cargo:rerun-if-changed=assets/icon.svg");
    println!("cargo:rerun-if-changed=build.rs");
    
    // Convert SVG to ICO and PNG if SVG exists and is newer
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
        ico_needs_update || png_needs_update
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
            
            // Convert RGBA to BGRA for ICO format
            let mut rgba_data = pixmap.take();
            for chunk in rgba_data.chunks_exact_mut(4) {
                chunk.swap(0, 2); // Swap R and B
            }
            
            let image = ico::IconImage::from_rgba_data(size, size, rgba_data);
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
