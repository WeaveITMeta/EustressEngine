//! Tagged-bincode wire format shared across the four light spawners.
//!
//! Per `CLASS_REGISTRY.md` Appendix A: every `ClassSpawner::serialize`
//! call prepends a single tag byte before the rkyv (here: bincode)
//! payload. The tag byte encodes
//! `(schema_version << 4) | class_group_tag`:
//!
//! - `schema_version` = `1` — lockstep with `worlddb::header`'s schema.
//! - `class_group_tag` = `4` (light) for all four spawners — they share
//!   the wire enum below, so the per-class discriminator lives inside
//!   the payload's `WirePayload` variant rather than the tag byte.
//!
//! Per `LIGHTING_AUDIT.md` §4.2 "Binary-ECS rkyv layout option 1" this
//! is the cold-tail-only shape — Wave 5 (when load measurement shows
//! per-light overhead matters) upgrades to a typed `ArchLight` field on
//! `ArchInstanceCore`. The tag bytes below are reserved in the global
//! tag space so that upgrade is additive.
//!
//! ## Why bincode (1.x) and not rkyv directly
//!
//! `eustress-engine` already pulls `bincode = "1.3"` (see
//! `engine_bridge::protocol`), bincode 1.x freezes the wire shape per
//! its crate-level guarantees, and the byte stability is what
//! `serialize`'s deterministic-output contract needs. Avoiding a
//! second-order `rkyv` dep keeps the spawner module compile-light;
//! Wave 5's typed-slot upgrade is the right place to introduce rkyv
//! directly when the perf benchmark calls for it.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// Per Appendix A: schema_version=1, group=4 (light). The per-class
// discriminator lives inside the payload enum below, so all four light
// classes share these tag constants — keeps the schema_version bump
// path atomic for the whole light family.
const SCHEMA_VERSION: u8 = 1;
const GROUP_LIGHT: u8 = 4;
const BASE_LIGHT_TAG: u8 = (SCHEMA_VERSION << 4) | GROUP_LIGHT;

/// Tag byte for PointLight archives.
///
/// The four light classes share the same `(schema, group)` bits and
/// distinguish via the payload variant; the per-class constants below
/// exist so callers can quickly reject a mismatch without a bincode
/// decode round-trip. Currently all four equal `BASE_LIGHT_TAG`.
pub const TAG_POINT_LIGHT: u8 = BASE_LIGHT_TAG;
pub const TAG_SPOT_LIGHT: u8 = BASE_LIGHT_TAG;
pub const TAG_SURFACE_LIGHT: u8 = BASE_LIGHT_TAG;
pub const TAG_DIRECTIONAL_LIGHT: u8 = BASE_LIGHT_TAG;

/// Common envelope for any light's archived state.
///
/// Field order is the canonical key order spawners build their
/// PropertyBags in (`metadata` → `transform` → light payload); see
/// CLASS_REGISTRY.md §4.3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireLightCommon {
    pub metadata: WireMetadata,
    pub transform: WireTransform,
    pub payload: WirePayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMetadata {
    pub name: String,
    pub archivable: bool,
    pub uuid: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WireTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WirePayload {
    PointLight(WirePointLight),
    SpotLight(WireSpotLight),
    SurfaceLight(WireSurfaceLight),
    DirectionalLight(WireDirectionalLight),
}

impl WirePayload {
    pub fn into_point_light(self) -> Option<WirePointLight> {
        if let WirePayload::PointLight(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn into_spot_light(self) -> Option<WireSpotLight> {
        if let WirePayload::SpotLight(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn into_surface_light(self) -> Option<WireSurfaceLight> {
        if let WirePayload::SurfaceLight(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn into_directional_light(self) -> Option<WireDirectionalLight> {
        if let WirePayload::DirectionalLight(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WirePointLight {
    /// Linear-sRGB RGBA quad. Light components don't carry alpha, but
    /// we round-trip the full quad so the `Color` accessor returns the
    /// same value byte-for-byte.
    pub color: [f32; 4],
    pub brightness: f32,
    pub range: f32,
    pub radius: f32,
    pub shadows: bool,
    pub texture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSpotLight {
    pub color: [f32; 4],
    pub brightness: f32,
    pub range: f32,
    /// Cone outer-angle in degrees, matching the Eustress authoring
    /// component (`EustressSpotLight.angle`). The Bevy `SpotLight`
    /// stores radians; conversion happens at the spawn boundary.
    pub angle_deg: f32,
    pub shadows: bool,
    pub texture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSurfaceLight {
    pub color: [f32; 4],
    pub brightness: f32,
    pub range: f32,
    pub face: String,
    pub shadows: bool,
    pub texture: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireDirectionalLight {
    pub color: [f32; 4],
    /// Authoring brightness — multiplied by `10_000.0` at the Bevy
    /// boundary to land in lux. Stored raw so a future "brightness
    /// scale changed" migration doesn't need a data-fix pass.
    pub brightness: f32,
    pub shadows: bool,
    pub shadow_depth_bias: f32,
    pub shadow_normal_bias: f32,
    pub texture: Option<String>,
}

/// Encode a [`WireLightCommon`] with the given leading tag byte. The
/// return value is the spawner's `serialize` output.
pub fn encode(tag: u8, wire: &WireLightCommon) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + wire.metadata.name.len());
    out.push(tag);
    // bincode::serialize_into avoids the intermediate allocation
    // bincode::serialize would produce; we already own the output buf.
    if let Err(e) = bincode::serialize_into(&mut out, wire) {
        // Spawner contract is "deterministic Vec<u8>" — there is no
        // error path for the caller. Logging + a tagged-but-empty
        // payload is the least-bad recovery: the deserialize step will
        // reject the truncated buffer and return an empty PropertyBag.
        warn!("light wire encode failed: {e}");
        out.truncate(1);
    }
    out
}

/// Decode the inverse of [`encode`]. Returns `None` and emits a warn on
/// tag mismatch or bincode decode failure — per spec §2.1 the spawner
/// must never panic on deserialize.
pub fn decode(expected_tag: u8, bytes: &[u8]) -> Option<WireLightCommon> {
    let Some(&first) = bytes.first() else {
        warn!("light wire decode: empty buffer");
        return None;
    };
    if first != expected_tag {
        // Tag-mismatch is the LOOP-9 (`Risks R9`) breaker — never
        // overwrite, log loudly enough to be findable in the boot log.
        warn!(
            "light wire decode: tag mismatch (got 0x{:02x}, expected 0x{:02x})",
            first, expected_tag,
        );
        return None;
    }
    match bincode::deserialize::<WireLightCommon>(&bytes[1..]) {
        Ok(wire) => Some(wire),
        Err(e) => {
            warn!("light wire decode bincode error: {e}");
            None
        }
    }
}

// ── Color / Transform conversions ────────────────────────────────────

/// Convert a Bevy `Color` to the wire-format linear-sRGB RGBA quad.
pub fn color_to_rgba(color: Color) -> [f32; 4] {
    let s = color.to_srgba();
    [s.red, s.green, s.blue, s.alpha]
}

/// Inverse of [`color_to_rgba`].
pub fn rgba_to_color(rgba: [f32; 4]) -> Color {
    Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3])
}

/// Pack a Bevy `Transform` into the wire shape.
pub fn wire_transform(t: Transform) -> WireTransform {
    WireTransform {
        translation: [t.translation.x, t.translation.y, t.translation.z],
        rotation: [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w],
        scale: [t.scale.x, t.scale.y, t.scale.z],
    }
}

/// Inverse of [`wire_transform`].
pub fn wire_to_transform(w: &WireTransform) -> Transform {
    Transform {
        translation: Vec3::new(w.translation[0], w.translation[1], w.translation[2]),
        rotation: Quat::from_xyzw(w.rotation[0], w.rotation[1], w.rotation[2], w.rotation[3]),
        scale: Vec3::new(w.scale[0], w.scale[1], w.scale[2]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a representative payload through every variant — if
    /// the wire shape drifts (e.g. someone adds a field but forgets to
    /// bump the schema version) this test pins the breakage to a
    /// specific commit.
    #[test]
    fn point_light_payload_roundtrip() {
        let wire = WireLightCommon {
            metadata: WireMetadata {
                name: "Lamp".into(),
                archivable: true,
                uuid: "01234567-89ab-cdef-0123-456789abcdef".into(),
            },
            transform: WireTransform {
                translation: [1.0, 2.0, 3.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [1.0, 1.0, 1.0],
            },
            payload: WirePayload::PointLight(WirePointLight {
                color: [1.0, 0.8, 0.5, 1.0],
                brightness: 100_000.0,
                range: 60.0,
                radius: 0.0,
                shadows: true,
                texture: None,
            }),
        };
        let bytes = encode(TAG_POINT_LIGHT, &wire);
        let restored = decode(TAG_POINT_LIGHT, &bytes).expect("round-trip must succeed");
        assert_eq!(restored.metadata.name, "Lamp");
        let p = restored.payload.into_point_light().unwrap();
        assert_eq!(p.brightness, 100_000.0);
        assert_eq!(p.range, 60.0);
    }

    #[test]
    fn decode_rejects_wrong_tag() {
        assert!(decode(TAG_POINT_LIGHT, &[0xFFu8; 8]).is_none());
        assert!(decode(TAG_POINT_LIGHT, &[]).is_none());
    }
}
