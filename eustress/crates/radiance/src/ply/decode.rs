//! Pure decoding core of the fast `.ply` reader: header parse plus the
//! binary payload decode into planar Gaussian storage. No Bevy and no I/O, so
//! the same file also compiles into a standalone benchmark that checks it
//! against the upstream `ply-rs` path on real captures.

use super::{
    PositionVisibility, Rotation, ScaleOpacity, SphericalHarmonicCoefficients, MAX_SIZE_VARIANCE,
    SH_CHANNELS, SH_COEFF_COUNT, SH_COEFF_COUNT_PER_CHANNEL,
};

/// Where one `float` property of a vertex record lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Position(u8),
    Visibility,
    /// Interleaved spherical-harmonic index: `coefficient * 3 + channel`.
    Sh(u8),
    Scale(u8),
    Opacity,
    Rotation(u8),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Field {
    /// Byte offset of the property inside one vertex record.
    pub offset: usize,
    pub slot: Slot,
}

/// Byte layout of the `vertex` element of a binary little-endian `.ply`.
#[derive(Debug)]
pub(crate) struct Layout {
    /// First byte of the binary payload (just past `end_header`).
    pub data_start: usize,
    pub vertex_count: usize,
    /// Bytes per vertex record.
    pub stride: usize,
    /// Mapped properties, in file order (a later duplicate wins, as upstream).
    pub fields: Vec<Field>,
}

pub(crate) enum HeaderParse {
    Fast(Layout),
    /// Valid PLY that this reader does not handle. The caller falls back to
    /// the upstream `ply-rs` parser, which covers ASCII, big-endian, list
    /// properties and non-`float` Gaussian fields.
    Unsupported(&'static str),
}

/// How far into the file a header may run before we call it malformed.
const MAX_HEADER_BYTES: usize = 1 << 20;

/// The properties upstream requires before it will load a cloud.
const REQUIRED: [&str; 13] = [
    "x", "y", "z", "f_dc_0", "f_dc_1", "f_dc_2", "scale_0", "scale_1", "opacity", "rot_0",
    "rot_1", "rot_2", "rot_3",
];

fn scalar_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "char" | "int8" | "uchar" | "uint8" => 1,
        "short" | "int16" | "ushort" | "uint16" => 2,
        "int" | "int32" | "uint" | "uint32" | "float" | "float32" => 4,
        "double" | "float64" => 8,
        _ => return None,
    })
}

/// Parse the header and plan the decode.
///
/// `upstream_sh` keeps `bevy_gaussian_splatting` 8.0.1's `f_rest_*` mapping
/// for comparison. The default mapping follows the layout the INRIA trainer
/// writes: the 45 degree-1..3 coefficients are stored channel-major, 15 per
/// channel (`features_rest.transpose(1, 2).flatten()` gives
/// `[R1..R15, G1..G15, B1..B15]`), so `f_rest_i` is coefficient `i % 15 + 1`
/// of channel `i / 15`. Upstream divides by 16 for the channel and so stores
/// G1 in R1, B1 in G1 and B2 in G2, and leaves B1 and B2 at zero.
pub(crate) fn parse_header(bytes: &[u8], upstream_sh: bool) -> std::io::Result<HeaderParse> {
    use std::io::{Error, ErrorKind};
    let invalid = |m: &str| Error::new(ErrorKind::InvalidData, m.to_string());

    let mut pos = 0usize;
    let mut first_line = true;
    let mut format_seen = false;
    let mut in_vertex = false;
    let mut seen_vertex = false;
    let mut vertex_count = 0usize;
    let mut stride = 0usize;
    // (name, byte offset, is a 4-byte float)
    let mut props: Vec<(String, usize, bool)> = Vec::new();

    loop {
        if pos >= bytes.len() || pos > MAX_HEADER_BYTES {
            return Err(invalid("ply header has no end_header"));
        }
        let Some(nl) = bytes[pos..].iter().position(|&b| b == b'\n').map(|i| pos + i) else {
            return Err(invalid("ply header has no end_header"));
        };
        let mut raw = &bytes[pos..nl];
        if raw.last() == Some(&b'\r') {
            raw = &raw[..raw.len() - 1];
        }
        pos = nl + 1;
        let line = std::str::from_utf8(raw)
            .map_err(|_| invalid("ply header is not text"))?
            .trim();
        if first_line {
            if line != "ply" {
                return Err(invalid("not a ply file"));
            }
            first_line = false;
            continue;
        }
        let mut tok = line.split_ascii_whitespace();
        match tok.next() {
            None | Some("comment") | Some("obj_info") => continue,
            Some("format") => {
                if tok.next() != Some("binary_little_endian") {
                    return Ok(HeaderParse::Unsupported("not binary_little_endian"));
                }
                format_seen = true;
            }
            Some("element") => {
                let name = tok.next().ok_or_else(|| invalid("element without a name"))?;
                let count = tok
                    .next()
                    .and_then(|c| c.parse::<usize>().ok())
                    .ok_or_else(|| invalid("element without a count"))?;
                if seen_vertex {
                    // Elements after `vertex` follow its payload; never read.
                    in_vertex = false;
                } else if name == "vertex" {
                    seen_vertex = true;
                    in_vertex = true;
                    vertex_count = count;
                } else {
                    return Ok(HeaderParse::Unsupported("vertex is not the first element"));
                }
            }
            Some("property") => {
                if !in_vertex {
                    continue;
                }
                let ty = tok.next().ok_or_else(|| invalid("property without a type"))?;
                if ty == "list" {
                    return Ok(HeaderParse::Unsupported("list property in vertex"));
                }
                let name = tok.next().ok_or_else(|| invalid("property without a name"))?;
                let size = scalar_size(ty).ok_or_else(|| invalid("unknown ply property type"))?;
                props.push((name.to_string(), stride, ty == "float" || ty == "float32"));
                stride += size;
            }
            Some("end_header") => break,
            Some(_) => return Err(invalid("unexpected ply header line")),
        }
    }
    if !format_seen {
        return Ok(HeaderParse::Unsupported("missing format line"));
    }
    if !seen_vertex {
        return Ok(HeaderParse::Unsupported("no vertex element"));
    }
    if REQUIRED.iter().any(|r| !props.iter().any(|(n, _, _)| n == r)) {
        return Err(invalid("missing required properties"));
    }

    let mut fields = Vec::with_capacity(props.len());
    let mut rest: Vec<(usize, usize)> = Vec::new(); // (f_rest index, offset)
    for (name, offset, is_f32) in &props {
        let slot = match name.as_str() {
            "x" => Slot::Position(0),
            "y" => Slot::Position(1),
            "z" => Slot::Position(2),
            "visibility" => Slot::Visibility,
            "f_dc_0" => Slot::Sh(0),
            "f_dc_1" => Slot::Sh(1),
            "f_dc_2" => Slot::Sh(2),
            "scale_0" => Slot::Scale(0),
            "scale_1" => Slot::Scale(1),
            "scale_2" => Slot::Scale(2),
            "opacity" => Slot::Opacity,
            "rot_0" => Slot::Rotation(0),
            "rot_1" => Slot::Rotation(1),
            "rot_2" => Slot::Rotation(2),
            "rot_3" => Slot::Rotation(3),
            n if n.starts_with("f_rest_") => {
                if !is_f32 {
                    return Ok(HeaderParse::Unsupported("non-float gaussian property"));
                }
                if let Ok(i) = n["f_rest_".len()..].parse::<usize>() {
                    rest.push((i, *offset));
                }
                continue;
            }
            // Normals, colours and anything else the renderer does not use.
            _ => continue,
        };
        if !is_f32 {
            return Ok(HeaderParse::Unsupported("non-float gaussian property"));
        }
        fields.push(Field { offset: *offset, slot });
    }

    let per_channel = rest.len() / SH_CHANNELS;
    for (i, offset) in rest {
        let idx = if upstream_sh {
            let channel = i / SH_COEFF_COUNT_PER_CHANNEL;
            let coefficient = if SH_COEFF_COUNT_PER_CHANNEL == 1 {
                1
            } else {
                i % (SH_COEFF_COUNT_PER_CHANNEL - 1) + 1
            };
            Some(coefficient * SH_CHANNELS + channel)
        } else if per_channel > 0 {
            let channel = i / per_channel;
            let coefficient = i % per_channel + 1;
            (channel < SH_CHANNELS && coefficient < SH_COEFF_COUNT_PER_CHANNEL)
                .then(|| coefficient * SH_CHANNELS + channel)
        } else {
            None
        };
        if let Some(idx) = idx.filter(|&idx| idx < SH_COEFF_COUNT) {
            fields.push(Field { offset, slot: Slot::Sh(idx as u8) });
        }
    }

    if stride == 0 {
        return Err(invalid("vertex element has no properties"));
    }
    let end = vertex_count
        .checked_mul(stride)
        .and_then(|len| len.checked_add(pos));
    if !matches!(end, Some(end) if end <= bytes.len()) {
        return Err(Error::new(ErrorKind::UnexpectedEof, "ply vertex payload is truncated"));
    }
    Ok(HeaderParse::Fast(Layout { data_start: pos, vertex_count, stride, fields }))
}

/// Decode vertices `first..first + out.len()` of `payload` (the vertex
/// records only) into the four planar output slices, then apply upstream's
/// post-pass: sigmoid opacity, log-scale clamped to the mean ± 4 then
/// exponentiated, rotation normalised.
pub(crate) fn decode_range(
    payload: &[u8],
    layout: &Layout,
    first: usize,
    position_visibility: &mut [PositionVisibility],
    spherical_harmonic: &mut [SphericalHarmonicCoefficients],
    rotation: &mut [Rotation],
    scale_opacity: &mut [ScaleOpacity],
) {
    let stride = layout.stride;
    for (i, (((pv, sh), rot), so)) in position_visibility
        .iter_mut()
        .zip(spherical_harmonic.iter_mut())
        .zip(rotation.iter_mut())
        .zip(scale_opacity.iter_mut())
        .enumerate()
    {
        let base = (first + i) * stride;
        let record = &payload[base..base + stride];
        for field in &layout.fields {
            let b = &record[field.offset..field.offset + 4];
            let v = f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            match field.slot {
                Slot::Position(k) => pv.position[k as usize] = v,
                Slot::Visibility => pv.visibility = v,
                Slot::Sh(k) => sh.coefficients[k as usize] = v,
                Slot::Scale(k) => so.scale[k as usize] = v,
                Slot::Opacity => so.opacity = 1.0 / (1.0 + (-v).exp()),
                Slot::Rotation(k) => rot.rotation[k as usize] = v,
            }
        }
        let mean = (so.scale[0] + so.scale[1] + so.scale[2]) / 3.0;
        for s in &mut so.scale {
            *s = s.max(mean - MAX_SIZE_VARIANCE).min(mean + MAX_SIZE_VARIANCE).exp();
        }
        let r = &mut rot.rotation;
        let norm = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]).sqrt();
        if norm > 0.0 && norm.is_finite() {
            for c in r.iter_mut() {
                *c /= norm;
            }
        } else {
            // A zero quaternion would divide to NaN, and one NaN depth can
            // poison the GPU sort. Fall back to the identity rotation.
            *r = [1.0, 0.0, 0.0, 0.0];
        }
    }
}
