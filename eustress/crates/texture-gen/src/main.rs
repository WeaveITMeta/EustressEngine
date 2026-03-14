//! # Procedural PBR Texture Generator
//!
//! Standalone crate to generate procedural PBR textures for Eustress default materials.
//! Run with: `cargo run -p texture-gen`
//!
//! Generates base_color, normal, and metallic_roughness maps for textured materials
//! (BrushedMetal, DiamondPlate, CorrodedMetal, Wood, Brick, Marble, Concrete, Fabric).
//!
//! Uses the `png` crate to write PNG files directly — zero heavy dependencies.

use std::path::Path;

fn main() {
    // Output into the engine's assets/materials/textures/ directory
    let output_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("engine")
        .join("assets")
        .join("materials")
        .join("textures");
    std::fs::create_dir_all(&output_dir).expect("Failed to create textures directory");

    generate_brushed_metal(&output_dir, 512);
    generate_diamond_plate(&output_dir, 512);
    generate_corroded_metal(&output_dir, 512);
    generate_wood(&output_dir, 512);
    generate_brick(&output_dir, 512);
    generate_marble(&output_dir, 512);
    generate_concrete(&output_dir, 512);
    generate_fabric(&output_dir, 512);

    println!("All textures generated in {:?}", output_dir);
}

/// Simple deterministic hash for reproducible noise
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let n = x.wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(seed.wrapping_mul(1274126177));
    let n = n ^ (n >> 13);
    let n = n.wrapping_mul(n.wrapping_mul(n.wrapping_mul(60493)).wrapping_add(19990303));
    let n = n ^ (n >> 16);
    (n as f32) / (u32::MAX as f32)
}

/// Value noise with bilinear interpolation
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let ix = x.floor() as u32;
    let iy = y.floor() as u32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    // Smooth interpolation
    let fx = fx * fx * (3.0 - 2.0 * fx);
    let fy = fy * fy * (3.0 - 2.0 * fy);

    let v00 = hash(ix, iy, seed);
    let v10 = hash(ix.wrapping_add(1), iy, seed);
    let v01 = hash(ix, iy.wrapping_add(1), seed);
    let v11 = hash(ix.wrapping_add(1), iy.wrapping_add(1), seed);

    let a = v00 + (v10 - v00) * fx;
    let b = v01 + (v11 - v01) * fx;
    a + (b - a) * fy
}

/// Fractal Brownian motion — layered noise octaves
fn fbm(x: f32, y: f32, octaves: u32, seed: u32) -> f32 {
    let mut value = 0.0;
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    for i in 0..octaves {
        value += amplitude * value_noise(x * frequency, y * frequency, seed + i * 17);
        amplitude *= 0.5;
        frequency *= 2.0;
    }
    value
}

/// Generate BrushedMetal textures — directional grain pattern
fn generate_brushed_metal(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            // Directional grain: noise stretched along X axis
            let grain = fbm(u * 2.0, v * 80.0, 4, 42);
            let fine_grain = value_noise(u * 4.0, v * 200.0, 99) * 0.15;
            let combined = (grain + fine_grain).clamp(0.0, 1.0);

            // Base color: steel gray with grain variation
            let brightness = 0.65 + combined * 0.2;
            let r = (brightness * 0.95 * 255.0) as u8;
            let g = (brightness * 0.95 * 255.0) as u8;
            let b = (brightness * 1.0 * 255.0) as u8;
            base_color[idx] = r;
            base_color[idx + 1] = g;
            base_color[idx + 2] = b;
            base_color[idx + 3] = 255;

            // Normal map: derive from grain height (tangent-space, OpenGL convention)
            let dx = fbm(u * 2.0 + 0.001, v * 80.0, 4, 42) - combined;
            let dy = fbm(u * 2.0, (v + 0.001) * 80.0, 4, 42) - combined;
            let nx = (-dx * 8.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
            let ny = (-dy * 8.0).clamp(-1.0, 1.0) * 0.5 + 0.5;
            normal[idx] = (nx * 255.0) as u8;
            normal[idx + 1] = (ny * 255.0) as u8;
            normal[idx + 2] = 255; // Z always up for flat-ish surface
            normal[idx + 3] = 255;

            // Metallic (Blue) = high, Roughness (Green) = varies with grain
            let roughness = 0.25 + combined * 0.3;
            metallic_roughness[idx] = 0;                         // R: unused (occlusion)
            metallic_roughness[idx + 1] = (roughness * 255.0) as u8; // G: roughness
            metallic_roughness[idx + 2] = 240;                   // B: metallic (high)
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "brushed_metal_base_color.png", size, &base_color);
    save_png(output_dir, "brushed_metal_normal.png", size, &normal);
    save_png(output_dir, "brushed_metal_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated BrushedMetal textures ({}x{})", size, size);
}

/// Generate DiamondPlate textures — raised diamond pattern
fn generate_diamond_plate(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            // Diamond pattern: rotated grid
            let diamond_u = (u * 8.0 + v * 8.0).fract();
            let diamond_v = (u * 8.0 - v * 8.0).fract();
            let diamond = ((diamond_u - 0.5).abs() + (diamond_v - 0.5).abs()).min(1.0);
            let raised = if diamond < 0.35 { 1.0 - diamond / 0.35 } else { 0.0 };

            let noise = fbm(u * 20.0, v * 20.0, 3, 77) * 0.1;
            let brightness = 0.6 + raised * 0.2 + noise;
            base_color[idx] = (brightness * 0.93 * 255.0).min(255.0) as u8;
            base_color[idx + 1] = (brightness * 0.93 * 255.0).min(255.0) as u8;
            base_color[idx + 2] = (brightness * 0.96 * 255.0).min(255.0) as u8;
            base_color[idx + 3] = 255;

            let nx = if raised > 0.01 { (diamond_u - 0.5) * raised * 0.5 + 0.5 } else { 0.5 };
            let ny = if raised > 0.01 { (diamond_v - 0.5) * raised * 0.5 + 0.5 } else { 0.5 };
            normal[idx] = (nx.clamp(0.0, 1.0) * 255.0) as u8;
            normal[idx + 1] = (ny.clamp(0.0, 1.0) * 255.0) as u8;
            normal[idx + 2] = 255;
            normal[idx + 3] = 255;

            let roughness = if raised > 0.1 { 0.3 } else { 0.5 + noise };
            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = (roughness.clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = 230;
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "diamond_plate_base_color.png", size, &base_color);
    save_png(output_dir, "diamond_plate_normal.png", size, &normal);
    save_png(output_dir, "diamond_plate_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated DiamondPlate textures ({}x{})", size, size);
}

/// Generate CorrodedMetal textures — rust patches
fn generate_corroded_metal(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            let rust_mask = fbm(u * 6.0, v * 6.0, 5, 31);
            let is_rusty = rust_mask > 0.45;
            let rust_amount = ((rust_mask - 0.45) * 4.0).clamp(0.0, 1.0);

            let noise = fbm(u * 30.0, v * 30.0, 3, 55) * 0.1;

            if is_rusty {
                // Rust: orange-brown
                let r = (0.55 + rust_amount * 0.15 + noise).clamp(0.0, 1.0);
                let g = (0.25 + rust_amount * 0.1 + noise * 0.5).clamp(0.0, 1.0);
                let b = (0.12 + noise * 0.3).clamp(0.0, 1.0);
                base_color[idx] = (r * 255.0) as u8;
                base_color[idx + 1] = (g * 255.0) as u8;
                base_color[idx + 2] = (b * 255.0) as u8;
            } else {
                // Bare metal underneath
                let brightness = 0.55 + noise;
                base_color[idx] = (brightness * 0.9 * 255.0).min(255.0) as u8;
                base_color[idx + 1] = (brightness * 0.88 * 255.0).min(255.0) as u8;
                base_color[idx + 2] = (brightness * 0.85 * 255.0).min(255.0) as u8;
            }
            base_color[idx + 3] = 255;

            // Bumpy normals for rust
            let bump = fbm(u * 40.0, v * 40.0, 4, 88) * rust_amount;
            let dx = fbm(u * 40.0 + 0.001, v * 40.0, 4, 88) - bump;
            let dy = fbm(u * 40.0, v * 40.0 + 0.001, 4, 88) - bump;
            normal[idx] = ((-dx * 4.0).clamp(-1.0, 1.0) * 0.5 * 255.0 + 128.0) as u8;
            normal[idx + 1] = ((-dy * 4.0).clamp(-1.0, 1.0) * 0.5 * 255.0 + 128.0) as u8;
            normal[idx + 2] = 255;
            normal[idx + 3] = 255;

            let roughness = if is_rusty { 0.8 + rust_amount * 0.15 } else { 0.4 + noise };
            let metallic = if is_rusty { 0.3 - rust_amount * 0.2 } else { 0.85 };
            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = (roughness.clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = (metallic.clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "corroded_metal_base_color.png", size, &base_color);
    save_png(output_dir, "corroded_metal_normal.png", size, &normal);
    save_png(output_dir, "corroded_metal_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated CorrodedMetal textures ({}x{})", size, size);
}

/// Generate Wood textures — grain rings
fn generate_wood(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            // Wood grain: stretched noise rings
            let warp = fbm(u * 4.0, v * 4.0, 3, 22) * 0.3;
            let grain = ((v * 12.0 + warp) * std::f32::consts::PI * 2.0).sin() * 0.5 + 0.5;
            let fine = fbm(u * 2.0, v * 60.0, 3, 44) * 0.15;
            let combined = (grain * 0.6 + fine + 0.4).clamp(0.0, 1.0);

            let r = (0.45 + combined * 0.2).clamp(0.0, 1.0);
            let g = (0.28 + combined * 0.15).clamp(0.0, 1.0);
            let b = (0.12 + combined * 0.1).clamp(0.0, 1.0);
            base_color[idx] = (r * 255.0) as u8;
            base_color[idx + 1] = (g * 255.0) as u8;
            base_color[idx + 2] = (b * 255.0) as u8;
            base_color[idx + 3] = 255;

            let dx = fbm(u * 2.0 + 0.001, v * 60.0, 3, 44) - fine;
            normal[idx] = ((-dx * 3.0).clamp(-1.0, 1.0) * 0.5 * 255.0 + 128.0) as u8;
            normal[idx + 1] = 128;
            normal[idx + 2] = 255;
            normal[idx + 3] = 255;

            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = ((0.75 + fine * 2.0).clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = 0; // Not metallic
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "wood_base_color.png", size, &base_color);
    save_png(output_dir, "wood_normal.png", size, &normal);
    save_png(output_dir, "wood_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated Wood textures ({}x{})", size, size);
}

/// Generate Brick textures — mortar grid with brick faces
fn generate_brick(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            // Brick grid: offset every other row
            let row = (v * 6.0).floor() as u32;
            let offset = if row % 2 == 0 { 0.0 } else { 0.5 };
            let brick_u = ((u * 4.0 + offset).fract() - 0.5).abs() * 2.0;
            let brick_v = ((v * 6.0).fract() - 0.5).abs() * 2.0;

            // Mortar gap
            let mortar_u: f32 = if brick_u > 0.92 { 1.0 } else { 0.0 };
            let mortar_v = if brick_v > 0.88 { 1.0 } else { 0.0 };
            let is_mortar = mortar_u.max(mortar_v) > 0.5;

            let noise = fbm(u * 30.0, v * 30.0, 3, 66) * 0.12;

            if is_mortar {
                let g = (0.65 + noise).clamp(0.0, 1.0);
                base_color[idx] = (g * 255.0) as u8;
                base_color[idx + 1] = (g * 0.95 * 255.0) as u8;
                base_color[idx + 2] = (g * 0.9 * 255.0) as u8;
            } else {
                // Brick face — warm red with variation per brick
                let brick_noise = hash(
                    (u * 4.0 + offset).floor() as u32,
                    row,
                    123,
                ) * 0.15;
                let r = (0.58 + brick_noise + noise).clamp(0.0, 1.0);
                let g = (0.24 + brick_noise * 0.5 + noise * 0.5).clamp(0.0, 1.0);
                let b = (0.15 + noise * 0.3).clamp(0.0, 1.0);
                base_color[idx] = (r * 255.0) as u8;
                base_color[idx + 1] = (g * 255.0) as u8;
                base_color[idx + 2] = (b * 255.0) as u8;
            }
            base_color[idx + 3] = 255;

            // Mortar is recessed
            let height = if is_mortar { 0.0 } else { 0.5 + noise };
            normal[idx] = 128;
            normal[idx + 1] = 128;
            normal[idx + 2] = ((0.5 + height * 0.5) * 255.0) as u8;
            normal[idx + 3] = 255;

            let roughness = if is_mortar { 0.9 } else { 0.8 + noise };
            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = (roughness.clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = 0;
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "brick_base_color.png", size, &base_color);
    save_png(output_dir, "brick_normal.png", size, &normal);
    save_png(output_dir, "brick_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated Brick textures ({}x{})", size, size);
}

/// Generate Marble textures — veined stone
fn generate_marble(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            // Marble veining: warped sine
            let warp = fbm(u * 5.0, v * 5.0, 5, 13) * 2.0;
            let vein = ((u * 4.0 + v * 2.0 + warp) * std::f32::consts::PI).sin() * 0.5 + 0.5;
            let fine = fbm(u * 20.0, v * 20.0, 3, 77) * 0.1;

            // White marble with gray-blue veins
            let base = 0.88 + fine;
            let vein_strength = vein.powf(3.0) * 0.4;
            let r = (base - vein_strength * 0.8).clamp(0.0, 1.0);
            let g = (base - vein_strength * 0.7).clamp(0.0, 1.0);
            let b = (base - vein_strength * 0.5).clamp(0.0, 1.0);
            base_color[idx] = (r * 255.0) as u8;
            base_color[idx + 1] = (g * 255.0) as u8;
            base_color[idx + 2] = (b * 255.0) as u8;
            base_color[idx + 3] = 255;

            normal[idx] = 128;
            normal[idx + 1] = 128;
            normal[idx + 2] = 255;
            normal[idx + 3] = 255;

            let roughness = 0.2 + vein_strength * 0.3 + fine;
            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = (roughness.clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = 0;
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "marble_base_color.png", size, &base_color);
    save_png(output_dir, "marble_normal.png", size, &normal);
    save_png(output_dir, "marble_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated Marble textures ({}x{})", size, size);
}

/// Generate Concrete textures — speckled aggregate
fn generate_concrete(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            let coarse = fbm(u * 8.0, v * 8.0, 4, 55) * 0.2;
            let fine = fbm(u * 40.0, v * 40.0, 3, 88) * 0.08;
            let speckle = hash(x, y, 33);
            let aggregate = if speckle > 0.92 { 0.15 } else { 0.0 };

            let brightness = (0.58 + coarse + fine + aggregate).clamp(0.0, 1.0);
            base_color[idx] = (brightness * 255.0) as u8;
            base_color[idx + 1] = (brightness * 0.97 * 255.0) as u8;
            base_color[idx + 2] = (brightness * 0.94 * 255.0) as u8;
            base_color[idx + 3] = 255;

            let _bump = coarse + fine;
            let dx = fbm(u * 8.0 + 0.001, v * 8.0, 4, 55) * 0.2 - coarse;
            normal[idx] = ((-dx * 3.0).clamp(-1.0, 1.0) * 0.5 * 255.0 + 128.0) as u8;
            normal[idx + 1] = 128;
            normal[idx + 2] = 255;
            normal[idx + 3] = 255;

            let roughness = 0.85 + fine;
            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = (roughness.clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = 0;
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "concrete_base_color.png", size, &base_color);
    save_png(output_dir, "concrete_normal.png", size, &normal);
    save_png(output_dir, "concrete_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated Concrete textures ({}x{})", size, size);
}

/// Generate Fabric textures — woven cross-hatch
fn generate_fabric(output_dir: &Path, size: u32) {
    let mut base_color = vec![0u8; (size * size * 4) as usize];
    let mut normal = vec![0u8; (size * size * 4) as usize];
    let mut metallic_roughness = vec![0u8; (size * size * 4) as usize];

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;

            // Woven pattern: alternating warp/weft
            let warp = ((u * 40.0 * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(2.0);
            let weft = ((v * 40.0 * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(2.0);
            let weave = (warp + weft) * 0.5;
            let noise = fbm(u * 60.0, v * 60.0, 2, 19) * 0.08;

            let brightness = (0.6 + weave * 0.15 + noise).clamp(0.0, 1.0);
            base_color[idx] = (brightness * 0.95 * 255.0) as u8;
            base_color[idx + 1] = (brightness * 0.90 * 255.0) as u8;
            base_color[idx + 2] = (brightness * 0.85 * 255.0) as u8;
            base_color[idx + 3] = 255;

            normal[idx] = ((warp - 0.5) * 0.3 * 255.0 + 128.0).clamp(0.0, 255.0) as u8;
            normal[idx + 1] = ((weft - 0.5) * 0.3 * 255.0 + 128.0).clamp(0.0, 255.0) as u8;
            normal[idx + 2] = 255;
            normal[idx + 3] = 255;

            metallic_roughness[idx] = 0;
            metallic_roughness[idx + 1] = ((0.9 + noise).clamp(0.0, 1.0) * 255.0) as u8;
            metallic_roughness[idx + 2] = 0;
            metallic_roughness[idx + 3] = 255;
        }
    }

    save_png(output_dir, "fabric_base_color.png", size, &base_color);
    save_png(output_dir, "fabric_normal.png", size, &normal);
    save_png(output_dir, "fabric_metallic_roughness.png", size, &metallic_roughness);
    println!("  Generated Fabric textures ({}x{})", size, size);
}

/// Write RGBA pixel data as a PNG file
fn save_png(dir: &Path, filename: &str, size: u32, data: &[u8]) {
    let path = dir.join(filename);
    let file = std::fs::File::create(&path).expect("Failed to create texture file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("Failed to write PNG header");
    writer.write_image_data(data).expect("Failed to write PNG data");
    println!("    Wrote {:?}", path);
}
