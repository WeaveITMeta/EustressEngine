//! Textured part meshes.
//!
//! A Roblox `MeshPart.TextureID` (or a `SpecialMesh.TextureId` folded into its
//! part) has no slot in a part's `[asset]`, so the importer bakes it into the
//! part's own `.glb` as the glTF material and sets `respect_gltf_materials`,
//! which makes the engine draw the mesh with that material instead of its
//! single tinted part material.
//!
//! Colour follows Roblox:
//! - **MeshPart**: the texture shows as drawn, and the part's `Color` shows
//!   through its transparent texels. The texture is composited over the part
//!   colour here, so the result is opaque and needs no blending.
//! - **SpecialMesh**: the texture is multiplied by `VertexColor`, and its own
//!   alpha stays (transparent texels are see-through).
//!
//! Roblox mesh UVs and glTF share one convention (origin at the image's top
//! left), so UVs pass through unchanged.

use std::io::Cursor;

/// How a baked texture should look on its part.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureLook {
    /// MeshPart: the part colour (sRGB 0-255) that shows through transparent
    /// texels. `None` for a SpecialMesh, whose texture keeps its alpha.
    pub under: Option<[u8; 3]>,
    /// Multiplies the texture (a SpecialMesh's `VertexColor`); white for a
    /// MeshPart.
    pub tint: [f32; 3],
    /// `1 - Transparency` of the part.
    pub opacity: f32,
    /// Material roughness and metalness (the part's material preset).
    pub roughness: f32,
    /// See `roughness`.
    pub metallic: f32,
}

impl TextureLook {
    /// A short, stable key for everything that changes the baked result, so
    /// parts that look the same share one file.
    pub fn key(&self) -> String {
        let text = format!(
            "{:?}|{:.4},{:.4},{:.4}|{:.4}|{:.4}|{:.4}",
            self.under, self.tint[0], self.tint[1], self.tint[2], self.opacity, self.roughness, self.metallic
        );
        // FNV-1a, 64-bit: stable across runs and platforms.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in text.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{:08x}", h as u32 ^ (h >> 32) as u32)
    }
}

/// `image/png` or `image/jpeg` when the bytes are one of the two image kinds
/// glTF can embed; `None` otherwise.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// Decode a PNG to tightly packed RGBA8.
pub fn decode_png_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(
        png::Transformations::EXPAND | png::Transformations::STRIP_16 | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(|e| format!("png header: {e}"))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("png data: {e}"))?;
    let (w, h) = (info.width, info.height);
    let n = (w as usize) * (h as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => {
            let mut out = Vec::with_capacity(n * 4);
            for row in buf.chunks(info.line_size).take(h as usize) {
                out.extend_from_slice(&row[..w as usize * 4]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity(n * 4);
            for row in buf.chunks(info.line_size).take(h as usize) {
                for px in row[..w as usize * 2].chunks(2) {
                    out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
                }
            }
            out
        }
        other => return Err(format!("unexpected png colour type {other:?} after expansion")),
    };
    Ok((w, h, rgba))
}

/// Encode tightly packed RGBA8 as a PNG.
pub fn encode_png_rgba(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| format!("png header: {e}"))?;
        writer.write_image_data(rgba).map_err(|e| format!("png data: {e}"))?;
        writer.finish().map_err(|e| format!("png finish: {e}"))?;
    }
    Ok(out)
}

/// Whether any texel of a PNG is less than fully opaque.
pub fn png_has_transparency(bytes: &[u8]) -> Result<bool, String> {
    let (_, _, rgba) = decode_png_rgba(bytes)?;
    Ok(rgba.chunks(4).any(|px| px[3] < 255))
}

/// Composite a PNG over a solid colour, the way a MeshPart shows its `Color`
/// through a texture. `None` when the PNG is already fully opaque, so the
/// original bytes can be embedded unchanged.
pub fn composite_over(bytes: &[u8], under: [u8; 3]) -> Result<Option<Vec<u8>>, String> {
    let (w, h, mut rgba) = decode_png_rgba(bytes)?;
    if rgba.chunks(4).all(|px| px[3] == 255) {
        return Ok(None);
    }
    for px in rgba.chunks_mut(4) {
        let a = px[3] as u32;
        for c in 0..3 {
            px[c] = ((px[c] as u32 * a + under[c] as u32 * (255 - a) + 127) / 255) as u8;
        }
        px[3] = 255;
    }
    encode_png_rgba(w, h, &rgba).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
        encode_png_rgba(w, h, rgba).unwrap()
    }

    #[test]
    fn composite_shows_the_part_colour_through_transparent_texels() {
        // Left texel opaque red, right texel fully transparent.
        let png = png_of(2, 1, &[255, 0, 0, 255, 0, 0, 0, 0]);
        let out = composite_over(&png, [0, 0, 255]).unwrap().expect("had transparency");
        let (w, h, rgba) = decode_png_rgba(&out).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255], "opaque texel keeps its colour");
        assert_eq!(&rgba[4..8], &[0, 0, 255, 255], "transparent texel shows the part colour");
    }

    #[test]
    fn an_opaque_png_is_left_alone() {
        let png = png_of(1, 1, &[10, 20, 30, 255]);
        assert!(composite_over(&png, [0, 0, 0]).unwrap().is_none());
        assert!(!png_has_transparency(&png).unwrap());
    }

    #[test]
    fn mime_sniffing_accepts_only_png_and_jpeg() {
        assert_eq!(image_mime(&png_of(1, 1, &[0, 0, 0, 255])), Some("image/png"));
        assert_eq!(image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(image_mime(b"GIF89a"), None);
    }

    #[test]
    fn look_key_changes_with_the_look() {
        let a = TextureLook { under: Some([1, 2, 3]), tint: [1.0; 3], opacity: 1.0, roughness: 0.5, metallic: 0.0 };
        let mut b = a.clone();
        assert_eq!(a.key(), b.key());
        b.under = Some([1, 2, 4]);
        assert_ne!(a.key(), b.key());
    }
}
