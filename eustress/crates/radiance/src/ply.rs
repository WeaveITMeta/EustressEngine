//! Splat-cloud loading: a fast `.ply` reader and the two asset loaders that
//! use it.
//!
//! Upstream's `.ply` path goes through `ply-rs`, which hands every property of
//! every vertex to `set_property` as an owned `String` key. A 6.1M-splat
//! MipNeRF360 capture has 62 properties per vertex, so that is about 380
//! million key allocations and string matches before a single splat renders.
//! [`decode`] reads the header once, turns it into a table of byte offsets,
//! and copies floats straight out of the payload into the planar storage the
//! renderer uses, across worker threads. Anything outside the common binary
//! little-endian layout falls back to upstream's parser.
//!
//! Two loaders share the code:
//! - [`RawSplatCloudLoader`] loads into [`RawSplatCloud`], a main-world-only
//!   asset. The engine's import path uses it so the multi-gigabyte raw cloud
//!   never reaches the GPU: `RenderAssetPlugin` prepares every
//!   `PlanarGaussian3d` added to the asset store, referenced or not, so a raw
//!   cloud parked in that store was uploaded and then replaced by the culled
//!   one. Only the processed cloud enters that store now.
//! - [`FastGaussian3dLoader`] replaces upstream's `Gaussian3dLoader` for
//!   anything that loads a `PlanarGaussian3d` by path directly.
//!
//! Bevy picks a loader by the requested asset type first, so the two share
//! the `ply` and `gcloud` extensions without ambiguity.

use std::io;
use std::path::Path;

use bevy::asset::{io::Reader, AssetLoader, LoadContext};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy_gaussian_splatting::{
    gaussian::{
        f32::{PositionVisibility, Rotation, ScaleOpacity},
        formats::planar_3d::PlanarGaussian3d,
    },
    io::{codec::CloudCodec, ply::MAX_SIZE_VARIANCE},
    material::spherical_harmonics::{
        SphericalHarmonicCoefficients, SH_CHANNELS, SH_COEFF_COUNT, SH_COEFF_COUNT_PER_CHANNEL,
    },
};

mod decode;

/// A parsed splat cloud that the renderer has not seen yet. The engine loads
/// clouds into this type and moves the data into the processing task (floater
/// cull and collider fit), which hands back the final `PlanarGaussian3d`.
#[derive(Asset, TypePath, Debug)]
pub struct RawSplatCloud(pub PlanarGaussian3d);

/// Loads `.ply` / `.gcloud` into a [`RawSplatCloud`] (main world only).
#[derive(Default, TypePath)]
pub struct RawSplatCloudLoader;

impl AssetLoader for RawSplatCloudLoader {
    type Asset = RawSplatCloud;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        Ok(RawSplatCloud(load_cloud(reader, load_context.path().path()).await?))
    }

    fn extensions(&self) -> &[&str] {
        &["ply", "gcloud"]
    }
}

/// Drop-in replacement for upstream's `Gaussian3dLoader` (same extensions,
/// same asset type), built on the fast reader.
#[derive(Default, TypePath)]
pub struct FastGaussian3dLoader;

impl AssetLoader for FastGaussian3dLoader {
    type Asset = PlanarGaussian3d;
    type Settings = ();
    type Error = io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _: &Self::Settings,
        load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        load_cloud(reader, load_context.path().path()).await
    }

    fn extensions(&self) -> &[&str] {
        &["ply", "gcloud"]
    }
}

async fn load_cloud(reader: &mut dyn Reader, path: &Path) -> io::Result<PlanarGaussian3d> {
    let started = Instant::now();
    let bytes = read_all(reader, path).await?;
    let read_secs = started.elapsed().as_secs_f32();
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let decoded = Instant::now();
    let (cloud, how) = match ext.as_deref() {
        Some("ply") => decode_ply(bytes)?,
        Some("gcloud") => (PlanarGaussian3d::decode(&bytes), "gcloud"),
        _ => return Err(io::Error::other("only .ply and .gcloud splat clouds are supported")),
    };
    // `warn!` like the rest of this crate: the engine's log filter drops
    // `info!` from eustress_radiance, and the timings are the evidence.
    warn!(
        "splat load: {} gaussians from {} (read {:.2}s, decode {:.2}s, {})",
        cloud.position_visibility.len(),
        path.display(),
        read_secs,
        decoded.elapsed().as_secs_f32(),
        how,
    );
    Ok(cloud)
}

/// Read the whole file. Bevy's file reader (`async_fs::File`) does not
/// override `read_to_end`, so the generic async path pulls a multi-gigabyte
/// cloud through a small pipe with a task wake-up per chunk, growing the
/// buffer as it goes. A local absolute path (how the engine addresses
/// imported clouds) is read with one pre-sized `std::fs::read` instead; any
/// other source goes through the reader.
async fn read_all(reader: &mut dyn Reader, path: &Path) -> io::Result<Vec<u8>> {
    if path.is_absolute() {
        if let Ok(bytes) = std::fs::read(path) {
            return Ok(bytes);
        }
    }
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

/// `EUSTRESS_SPLAT_FAST_PLY=0` routes every `.ply` through upstream's parser
/// (the A/B and safety switch); `EUSTRESS_SPLAT_SH_UPSTREAM=1` keeps
/// upstream's `f_rest` mapping in the fast reader (see
/// [`decode::parse_header`]).
fn decode_ply(bytes: Vec<u8>) -> io::Result<(PlanarGaussian3d, &'static str)> {
    let fast = std::env::var("EUSTRESS_SPLAT_FAST_PLY").map_or(true, |v| v != "0");
    if fast {
        let upstream_sh = std::env::var("EUSTRESS_SPLAT_SH_UPSTREAM").is_ok_and(|v| v == "1");
        match decode::parse_header(&bytes, upstream_sh)? {
            decode::HeaderParse::Fast(layout) => return Ok((decode_fast(&bytes, &layout), "fast")),
            decode::HeaderParse::Unsupported(why) => {
                warn!("splat load: fast .ply reader does not cover this file ({why}); using ply-rs");
            }
        }
    }
    let mut reader = io::BufReader::new(io::Cursor::new(bytes));
    Ok((bevy_gaussian_splatting::io::ply::parse_ply_3d(&mut reader)?, "ply-rs"))
}

/// Decode every vertex into fresh planar storage. Matches upstream's
/// padding (default, zero-opacity splats up to the next multiple of 32).
fn decode_fast(bytes: &[u8], layout: &decode::Layout) -> PlanarGaussian3d {
    let n = layout.vertex_count;
    let padded = n + (32 - n % 32);
    let mut position_visibility = vec![PositionVisibility::default(); padded];
    let mut spherical_harmonic = vec![SphericalHarmonicCoefficients::default(); padded];
    let mut rotation = vec![Rotation::default(); padded];
    let mut scale_opacity = vec![ScaleOpacity::default(); padded];
    let payload = &bytes[layout.data_start..layout.data_start + n * layout.stride];
    decode_parallel(
        payload,
        layout,
        &mut position_visibility[..n],
        &mut spherical_harmonic[..n],
        &mut rotation[..n],
        &mut scale_opacity[..n],
    );
    PlanarGaussian3d { position_visibility, spherical_harmonic, rotation, scale_opacity }
}

/// Split the vertex range across scoped OS threads (not the Bevy task pools:
/// this already runs inside a loader task, and plain threads cannot deadlock
/// against the pool that is running us).
fn decode_parallel(
    payload: &[u8],
    layout: &decode::Layout,
    pv: &mut [PositionVisibility],
    sh: &mut [SphericalHarmonicCoefficients],
    rot: &mut [Rotation],
    so: &mut [ScaleOpacity],
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let n = pv.len();
        let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).clamp(1, 16);
        if threads > 1 && n >= 1 << 16 {
            let chunk = n.div_ceil(threads);
            std::thread::scope(|scope| {
                let mut first = 0;
                for (((pv, sh), rot), so) in pv
                    .chunks_mut(chunk)
                    .zip(sh.chunks_mut(chunk))
                    .zip(rot.chunks_mut(chunk))
                    .zip(so.chunks_mut(chunk))
                {
                    let start = first;
                    first += pv.len();
                    scope.spawn(move || decode::decode_range(payload, layout, start, pv, sh, rot, so));
                }
            });
            return;
        }
    }
    decode::decode_range(payload, layout, 0, pv, sh, rot, so);
}

#[cfg(test)]
mod tests {
    //! The fast reader against upstream's `ply-rs` path, on generated
    //! fixtures. Every comparison is on raw `f32` bits.

    use super::*;

    /// The INRIA trainer's vertex layout: 62 floats per splat (position,
    /// normals, 3 DC + 45 higher-order SH terms, opacity, scale, rotation).
    fn inria_names() -> Vec<String> {
        let mut names: Vec<String> = ["x", "y", "z", "nx", "ny", "nz", "f_dc_0", "f_dc_1", "f_dc_2"]
            .map(String::from)
            .to_vec();
        names.extend((0..45).map(|i| format!("f_rest_{i}")));
        names.extend(
            ["opacity", "scale_0", "scale_1", "scale_2", "rot_0", "rot_1", "rot_2", "rot_3"]
                .map(String::from),
        );
        names
    }

    /// Deterministic values in [-8, 8), wide enough that the scale clamp
    /// (mean ± 4) fires on some splats. An all-zero rotation has odds of
    /// 2^-96 per splat, so upstream's divide never sees one.
    fn records(n: usize, properties: usize) -> Vec<Vec<f32>> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let mut record = Vec::with_capacity(properties);
            for _ in 0..properties {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                record.push((state >> 40) as f32 / (1u64 << 24) as f32 * 16.0 - 8.0);
            }
            out.push(record);
        }
        out
    }

    fn header(format: &str, names: &[String], n: usize) -> String {
        let mut header = format!("ply\nformat {format} 1.0\ncomment fixture\nelement vertex {n}\n");
        for name in names {
            header += &format!("property float {name}\n");
        }
        header + "end_header\n"
    }

    fn payload(records: &[Vec<f32>]) -> Vec<u8> {
        records.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn binary_ply(names: &[String], records: &[Vec<f32>]) -> Vec<u8> {
        let mut bytes = header("binary_little_endian", names, records.len()).into_bytes();
        bytes.extend(payload(records));
        bytes
    }

    fn fast(bytes: &[u8], upstream_sh: bool) -> PlanarGaussian3d {
        match decode::parse_header(bytes, upstream_sh).expect("header parses") {
            decode::HeaderParse::Fast(layout) => decode_fast(bytes, &layout),
            decode::HeaderParse::Unsupported(why) => panic!("fast reader refused the fixture: {why}"),
        }
    }

    fn upstream(bytes: &[u8]) -> PlanarGaussian3d {
        let mut reader = io::BufReader::new(io::Cursor::new(bytes.to_vec()));
        bevy_gaussian_splatting::io::ply::parse_ply_3d(&mut reader).expect("upstream parses")
    }

    fn bits(values: impl Iterator<Item = f32>) -> Vec<u32> {
        values.map(f32::to_bits).collect()
    }

    fn sections(cloud: &PlanarGaussian3d) -> [(&'static str, Vec<u32>); 4] {
        [
            (
                "position_visibility",
                bits(cloud.position_visibility.iter().flat_map(|g| {
                    [g.position[0], g.position[1], g.position[2], g.visibility]
                })),
            ),
            ("spherical_harmonic", bits(cloud.spherical_harmonic.iter().flat_map(|g| g.coefficients))),
            ("rotation", bits(cloud.rotation.iter().flat_map(|g| g.rotation))),
            (
                "scale_opacity",
                bits(cloud.scale_opacity.iter().flat_map(|g| {
                    [g.scale[0], g.scale[1], g.scale[2], g.opacity]
                })),
            ),
        ]
    }

    fn assert_same_bits(actual: &PlanarGaussian3d, expected: &PlanarGaussian3d) {
        for ((field, a), (_, b)) in sections(actual).into_iter().zip(sections(expected)) {
            assert_eq!(a.len(), b.len(), "{field}: lengths differ");
            let differ = a.iter().zip(&b).filter(|(x, y)| x != y).count();
            let first = a.iter().zip(&b).position(|(x, y)| x != y);
            assert_eq!(differ, 0, "{field}: {differ} values differ, first at {first:?}");
        }
    }

    #[test]
    fn fast_reader_matches_upstream_bit_for_bit() {
        // 1000 is not a multiple of 32, so upstream's padding is compared too.
        let names = inria_names();
        let bytes = binary_ply(&names, &records(1000, names.len()));
        let reference = upstream(&bytes);
        assert_eq!(reference.position_visibility.len(), 1024);
        assert_same_bits(&fast(&bytes, true), &reference);
    }

    #[test]
    fn parallel_decode_matches_serial() {
        // Past the 65,536-splat threshold, so the decode splits across threads.
        let names = inria_names();
        let n = 70_001;
        let bytes = binary_ply(&names, &records(n, names.len()));
        let decode::HeaderParse::Fast(layout) = decode::parse_header(&bytes, false).unwrap() else {
            panic!("fast reader refused the fixture");
        };
        let parallel = decode_fast(&bytes, &layout);
        let len = parallel.position_visibility.len();
        let mut serial = PlanarGaussian3d {
            position_visibility: vec![PositionVisibility::default(); len],
            spherical_harmonic: vec![SphericalHarmonicCoefficients::default(); len],
            rotation: vec![Rotation::default(); len],
            scale_opacity: vec![ScaleOpacity::default(); len],
        };
        decode::decode_range(
            &bytes[layout.data_start..],
            &layout,
            0,
            &mut serial.position_visibility[..n],
            &mut serial.spherical_harmonic[..n],
            &mut serial.rotation[..n],
            &mut serial.scale_opacity[..n],
        );
        assert_same_bits(&parallel, &serial);
    }

    #[test]
    fn corrected_sh_follows_the_inria_layout() {
        let names = inria_names();
        let mut record = vec![0.0f32; names.len()];
        record[names.iter().position(|n| n == "rot_0").unwrap()] = 1.0;
        for (value, name) in record.iter_mut().zip(&names) {
            if let Some(c) = name.strip_prefix("f_dc_") {
                *value = 10.0 + c.parse::<f32>().unwrap();
            } else if let Some(i) = name.strip_prefix("f_rest_") {
                *value = 100.0 + i.parse::<f32>().unwrap();
            }
        }
        let bytes = binary_ply(&names, &[record]);

        let sh = fast(&bytes, false).spherical_harmonic[0].coefficients;
        for channel in 0..3 {
            assert_eq!(sh[channel], 10.0 + channel as f32, "f_dc_{channel}");
        }
        for i in 0..45 {
            // f_rest_i is coefficient i % 15 + 1 of channel i / 15.
            assert_eq!(sh[(i % 15 + 1) * 3 + i / 15], 100.0 + i as f32, "f_rest_{i}");
        }

        // Upstream divides by 16 for the channel, so the red degree-1 term
        // holds f_rest_15 (green). If this fails after an upgrade, upstream
        // fixed its mapping and `EUSTRESS_SPLAT_SH_UPSTREAM` can go.
        assert_eq!(upstream(&bytes).spherical_harmonic[0].coefficients[3], 115.0);
    }

    #[test]
    fn zero_rotation_becomes_identity() {
        let names = inria_names();
        let bytes = binary_ply(&names, &[vec![0.0; names.len()]]);
        assert_eq!(fast(&bytes, false).rotation[0].rotation, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn crlf_header_and_trailing_elements_parse() {
        let names = inria_names();
        let records = records(40, names.len());
        let plain = fast(&binary_ply(&names, &records), false);

        let mut crlf = header("binary_little_endian", &names, records.len())
            .replace("end_header\n", "element face 0\nproperty list uchar int vertex_indices\nend_header\n")
            .replace('\n', "\r\n")
            .into_bytes();
        crlf.extend(payload(&records));
        assert_same_bits(&fast(&crlf, false), &plain);
    }

    #[test]
    fn ascii_falls_back_to_ply_rs() {
        let names = inria_names();
        let records = records(100, names.len());
        let mut ascii = header("ascii", &names, records.len());
        for record in &records {
            let line: Vec<String> = record.iter().map(|v| v.to_string()).collect();
            ascii += &(line.join(" ") + "\n");
        }
        let ascii = ascii.into_bytes();
        assert!(matches!(
            decode::parse_header(&ascii, false),
            Ok(decode::HeaderParse::Unsupported(_))
        ));
        let (cloud, how) = decode_ply(ascii).expect("ascii loads");
        assert_eq!(how, "ply-rs");
        assert_same_bits(&cloud, &fast(&binary_ply(&names, &records), true));
    }

    #[test]
    fn malformed_files_are_refused() {
        let names = inria_names();
        let records = records(4, names.len());
        let good = binary_ply(&names, &records);

        let truncated = decode::parse_header(&good[..good.len() - 1], false).err();
        assert_eq!(truncated.map(|e| e.kind()), Some(io::ErrorKind::UnexpectedEof));

        let no_opacity: Vec<String> = names.iter().filter(|n| *n != "opacity").cloned().collect();
        let missing = binary_ply(&no_opacity, &self::records(4, no_opacity.len()));
        let missing = decode::parse_header(&missing, false).err();
        assert_eq!(missing.map(|e| e.kind()), Some(io::ErrorKind::InvalidData));

        // A `double` Gaussian field is valid PLY that only ply-rs reads.
        let mut double = header("binary_little_endian", &names, records.len())
            .replace("property float x\n", "property double x\n")
            .into_bytes();
        double.extend(payload(&records));
        assert!(matches!(
            decode::parse_header(&double, false),
            Ok(decode::HeaderParse::Unsupported(_))
        ));
    }
}
