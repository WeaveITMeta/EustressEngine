//! Key layout for the Fjall partitions.
//!
//! ## The encoder trait
//!
//! [`KeyEncoder`] hides the byte layout behind a small surface so the
//! flat key scheme used today can be replaced by a spatial
//! Morton/Hilbert scheme (Phase 2) without touching call sites. The
//! engine plugin picks an encoder at open time; the backend forwards
//! every put/get through it.
//!
//! ## Today: [`FlatKeyEncoder`]
//!
//! ```text
//! entity:{schema_version_u8}:{component_type_u16_be}:{entity_id_u64_be}
//! ```
//!
//! Big-endian on both `component_type` and `entity_id` so a range scan
//! over `entity:{v}:{c}:00..entity:{v}:{c}:ff` yields entities in
//! ascending id order. Schema version prefix lets v1/v2 coexist during
//! the migration windows documented in [`crate::schema`].
//!
//! ## Phase 2: spatial encoder (sketched, not wired)
//!
//! `MortonKeyEncoder` will append a 64-bit Morton-interleaved
//! `(chunk_x, chunk_z)` so range scans over a `chunk_morton..` prefix
//! return all entities in a spatial region — directly usable by [05]
//! `SpatialChunkGrid` and by Fjall's compaction filter for locality
//! preservation. The trait is structured to allow encoder swap on a
//! per-component basis (e.g. `Transform` uses spatial, `Tags` uses
//! flat) so high-cardinality non-spatial reads don't pay the Morton
//! cost.

use crate::backend::EntityId;
use crate::error::{Error, Result};

/// Identifies a Rust component type inside the DB. Choose stable
/// 16-bit ids per component family — collisions are detected at
/// engine startup against a `schema/components.toml` registry (TODO
/// Phase 4 once the wire fmt locks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComponentTypeId(pub u16);

impl ComponentTypeId {
    /// Reserved id for `bevy::prelude::Transform`. Every world has
    /// transforms so giving it id `1` keeps the most-scanned prefix at
    /// the smallest byte value, which marginally helps Fjall's
    /// prefix-truncation block format.
    pub const TRANSFORM: ComponentTypeId = ComponentTypeId(1);
    /// `eustress_common::classes::BasePart`.
    pub const BASE_PART: ComponentTypeId = ComponentTypeId(2);
    /// `eustress_common::attributes::Tags`.
    pub const TAGS: ComponentTypeId = ComponentTypeId(3);
    /// `eustress_common::attributes::Attributes`.
    pub const ATTRIBUTES: ComponentTypeId = ComponentTypeId(4);
    /// Class name + metadata header (`Instance`).
    pub const INSTANCE_META: ComponentTypeId = ComponentTypeId(5);
    /// Asset reference (mesh path, scene name).
    pub const ASSET_REF: ComponentTypeId = ComponentTypeId(6);
    /// MeasureUnit (Dynamic Unit System — see project_dynamic_unit_system memory).
    pub const MEASURE_UNIT: ComponentTypeId = ComponentTypeId(7);
    /// Full instance core — the rkyv `ArchInstanceCore` archive-model
    /// holding a whole bare entity's authoritative state in one zero-copy
    /// record (identity, asset, transform, render/physics flags, tags,
    /// extensible tail). This is the "binary ECS" home for the scalable,
    /// file-less set; it is Morton-keyed by the entity's position via
    /// [`MortonKeyEncoder::encode_spatial`] so a region scan returns a
    /// spatial neighbourhood. File-bearing entities never live here —
    /// they stay in the `tree` partition (see the engine's
    /// `space::representation` router).
    pub const INSTANCE_CORE: ComponentTypeId = ComponentTypeId(8);
}

/// Trait that turns `(EntityId, ComponentTypeId)` into the byte key
/// Fjall stores. See module docs for the layout intent.
pub trait KeyEncoder: Send + Sync + 'static {
    /// Schema version this encoder produces. Persisted in
    /// `header.bin` so loaders can refuse keys older than they
    /// understand (or older than any registered migration covers).
    fn schema_version(&self) -> u8;

    /// Encode a single component key.
    fn encode_component(&self, entity: EntityId, component: ComponentTypeId) -> Vec<u8>;

    /// Encode a half-open prefix for "all values of component `c`".
    fn component_prefix(&self, component: ComponentTypeId) -> Vec<u8>;

    /// Encode a half-open prefix for "every component of entity `e`".
    /// Used by `despawn` to range-delete in one pass.
    fn entity_prefix(&self, entity: EntityId) -> Vec<u8>;

    /// Inverse of [`Self::encode_component`]. Returns `Err` if `bytes`
    /// has the wrong schema version or shape.
    fn decode_component(&self, bytes: &[u8]) -> Result<(EntityId, ComponentTypeId)>;
}

/// Today's default: flat keys with schema-version + component-type +
/// entity-id big-endian. Cheap to encode, cheap to range-scan by
/// component type, no spatial locality.
#[derive(Debug, Clone, Default)]
pub struct FlatKeyEncoder;

impl FlatKeyEncoder {
    /// Schema version this encoder emits. Hardcoded `1` for today;
    /// bump in lockstep with [`crate::schema::WORLD_SCHEMA_VERSION`].
    pub const SCHEMA_VERSION: u8 = 1;

    /// Byte prefix that tags every flat-encoded key. Lets the decoder
    /// reject Morton-encoded keys (Phase 2) and vice versa without
    /// guessing.
    const TAG: u8 = b'F';
}

impl KeyEncoder for FlatKeyEncoder {
    fn schema_version(&self) -> u8 {
        Self::SCHEMA_VERSION
    }

    fn encode_component(&self, entity: EntityId, component: ComponentTypeId) -> Vec<u8> {
        // F | schema_version(u8) | component(u16 be) | entity(u64 be)
        let mut out = Vec::with_capacity(1 + 1 + 2 + 8);
        out.push(Self::TAG);
        out.push(Self::SCHEMA_VERSION);
        out.extend_from_slice(&component.0.to_be_bytes());
        out.extend_from_slice(&entity.0.to_be_bytes());
        out
    }

    fn component_prefix(&self, component: ComponentTypeId) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + 1 + 2);
        out.push(Self::TAG);
        out.push(Self::SCHEMA_VERSION);
        out.extend_from_slice(&component.0.to_be_bytes());
        out
    }

    fn entity_prefix(&self, entity: EntityId) -> Vec<u8> {
        // Entity prefix isn't natural in flat layout because component
        // sorts before entity. We emit the schema-version tag and let
        // the backend do a full sweep for despawn. Phase 2 will move
        // to a spatial-then-entity layout where entity_prefix is a
        // first-class range. Until then this returns an empty Vec so
        // callers know to fall back to per-component-type deletion.
        let _ = entity;
        Vec::new()
    }

    fn decode_component(&self, bytes: &[u8]) -> Result<(EntityId, ComponentTypeId)> {
        if bytes.len() != 1 + 1 + 2 + 8 {
            return Err(Error::KeyDecode(format!(
                "flat key wrong length: {} (expected 12)",
                bytes.len()
            )));
        }
        if bytes[0] != Self::TAG {
            return Err(Error::KeyDecode(format!(
                "flat key wrong tag: 0x{:02x} (expected 0x{:02x})",
                bytes[0],
                Self::TAG
            )));
        }
        if bytes[1] != Self::SCHEMA_VERSION {
            return Err(Error::KeyDecode(format!(
                "flat key schema version mismatch: {} (this build expects {})",
                bytes[1],
                Self::SCHEMA_VERSION
            )));
        }
        let mut c = [0u8; 2];
        c.copy_from_slice(&bytes[2..4]);
        let mut e = [0u8; 8];
        e.copy_from_slice(&bytes[4..12]);
        Ok((
            EntityId(u64::from_be_bytes(e)),
            ComponentTypeId(u16::from_be_bytes(c)),
        ))
    }
}

/// Encode the UUID-keyed entity-core key for the new
/// `entities_uuid` partition (IDENTITY.md §5.2). Returns the 16-byte
/// raw UUID; the partition-level distinction makes a schema-version
/// prefix optional, but this helper exists so a future schema bump
/// can add one without touching call sites — mirrors the
/// `FlatKeyEncoder::TAG` discipline elsewhere in this module.
///
/// Wave 2.1 wire form: the 16 raw bytes of the UUID. No tag/version
/// — partition separation enforces the schema boundary.
pub fn encode_uuid_entity_key(uuid: &[u8; 16]) -> [u8; 16] {
    *uuid
}

/// Spread the low 21 bits of `v` into every 3rd bit (Z-order / Morton
/// dilation for 3D). Pure bit-twiddle, no branches — the canonical
/// magic-number method, exact for inputs < 2^21.
pub fn part1by2(v: u32) -> u64 {
    let mut x = (v as u64) & 0x1f_ffff; // 21 bits
    x = (x | (x << 32)) & 0x001f_0000_0000_ffff;
    x = (x | (x << 16)) & 0x1f00_00ff_0000_ff;
    x = (x | (x << 8)) & 0x100f_00f0_0f00_f00f;
    x = (x | (x << 4)) & 0x10c3_0c30_c30c_30c3;
    x = (x | (x << 2)) & 0x1249_2492_4924_9249;
    x
}

/// Inverse of [`part1by2`] — gather every 3rd bit back into 21 bits.
pub fn compact1by2(mut x: u64) -> u32 {
    x &= 0x1249_2492_4924_9249;
    x = (x | (x >> 2)) & 0x10c3_0c30_c30c_30c3;
    x = (x | (x >> 4)) & 0x100f_00f0_0f00_f00f;
    x = (x | (x >> 8)) & 0x1f00_00ff_0000_ff;
    x = (x | (x >> 16)) & 0x001f_0000_0000_ffff;
    x = (x | (x >> 32)) & 0x1f_ffff;
    (x & 0x1f_ffff) as u32
}

/// Interleave 3 unsigned 21-bit cell coords into one 63-bit Morton
/// code. Spatially-adjacent cells get numerically-close codes, so a
/// range scan over `[morton(lo)..morton(hi)]` returns a spatial
/// neighbourhood — exactly what [05] `SpatialChunkGrid` needs to turn
/// "entities within radius R" into a Fjall prefix scan.
pub fn morton3_encode(x: u32, y: u32, z: u32) -> u64 {
    part1by2(x) | (part1by2(y) << 1) | (part1by2(z) << 2)
}

/// Inverse of [`morton3_encode`] → `(x, y, z)` cell coords.
pub fn morton3_decode(code: u64) -> (u32, u32, u32) {
    (
        compact1by2(code),
        compact1by2(code >> 1),
        compact1by2(code >> 2),
    )
}

/// World position → unsigned 21-bit cell coordinate at `chunk_size`.
/// Biased by +2^20 so the world origin sits mid-range and negative
/// coordinates stay non-negative (Morton needs unsigned input).
pub fn world_to_cell(coord: f32, chunk_size: f32) -> u32 {
    let cell = (coord / chunk_size).floor() as i64 + (1 << 20);
    cell.clamp(0, 0x1f_ffff) as u32
}

// ── Voxel-chunk keys — Wave 9.A (terrain in Fjall) ───────────────────
//
// Terrain lives in the same one-handle streaming DB as entities via a
// dedicated `voxels` partition. A voxel chunk is identified by signed
// integer chunk coordinates `(cx, cy, cz)` (NOT a world position — the
// caller has already divided by the chunk edge). The key is the Morton
// interleave of those coords so spatially-near chunks land in adjacent
// LSM keys, exactly the locality the entity `INSTANCE_CORE` rows get
// from [`MortonKeyEncoder`]. A region scan then becomes a Morton range
// walk over the chunk-coord box.

/// Bias added to each signed chunk-coordinate axis so negatives map into
/// the unsigned 21-bit range Morton requires. Mirrors the `(1 << 20)`
/// origin-centring used by [`world_to_cell`]. Representable chunk-coord
/// span: `[-(1<<20), (1<<20)-1]` per axis (±1,048,576 chunks ≈ ±134
/// million studs at a 128-stud chunk edge — far beyond any real world).
pub const VOXEL_CHUNK_BIAS: i64 = 1 << 20;

/// Tag byte stamped on every voxel-chunk key so a stray entity/tree key
/// can never be mis-decoded as a chunk coord (matches the
/// `FlatKeyEncoder::TAG` / `MortonKeyEncoder::TAG` discipline).
const VOXEL_KEY_TAG: u8 = b'V';
/// Schema version of the voxel-chunk key wire form.
const VOXEL_KEY_VERSION: u8 = 1;

/// Map one signed chunk-coordinate axis into its biased unsigned 21-bit
/// cell, clamped to the Morton-representable range.
fn chunk_axis_to_cell(c: i32) -> u32 {
    let v = c as i64 + VOXEL_CHUNK_BIAS;
    v.clamp(0, 0x1f_ffff) as u32
}

/// Inverse of [`chunk_axis_to_cell`] — biased cell back to signed coord.
fn cell_to_chunk_axis(cell: u32) -> i32 {
    (cell as i64 - VOXEL_CHUNK_BIAS) as i32
}

/// Encode signed voxel-chunk coords `(cx, cy, cz)` into the storage key.
///
/// Layout: `V | version(u8) | morton63(biased cx,cy,cz) be8` — 10 bytes.
/// The Morton interleave makes spatially-adjacent chunks numerically
/// adjacent, so a range scan over `[encode(lo)..encode(hi)]` returns a
/// spatial neighbourhood of chunks (the property [`morton3_encode`]
/// gives entity keys). Round-trips exactly via [`decode_voxel_chunk_key`]
/// for coords within `±(1<<20)`, including negatives.
pub fn encode_voxel_chunk_key(cx: i32, cy: i32, cz: i32) -> [u8; 10] {
    let morton = morton3_encode(
        chunk_axis_to_cell(cx),
        chunk_axis_to_cell(cy),
        chunk_axis_to_cell(cz),
    );
    let mut out = [0u8; 10];
    out[0] = VOXEL_KEY_TAG;
    out[1] = VOXEL_KEY_VERSION;
    out[2..10].copy_from_slice(&morton.to_be_bytes());
    out
}

/// The just-tag-and-version prefix every voxel-chunk key starts with —
/// the bound for a full `voxels`-partition scan (`iter_all`).
pub fn voxel_key_prefix() -> [u8; 2] {
    [VOXEL_KEY_TAG, VOXEL_KEY_VERSION]
}

/// Inverse of [`encode_voxel_chunk_key`] → signed `(cx, cy, cz)`.
/// Returns `Err` on a wrong-length, wrong-tag, or wrong-version key.
pub fn decode_voxel_chunk_key(bytes: &[u8]) -> Result<(i32, i32, i32)> {
    if bytes.len() != 10 {
        return Err(Error::KeyDecode(format!(
            "voxel-chunk key wrong length: {} (expected 10)",
            bytes.len()
        )));
    }
    if bytes[0] != VOXEL_KEY_TAG || bytes[1] != VOXEL_KEY_VERSION {
        return Err(Error::KeyDecode(format!(
            "voxel-chunk key wrong tag/version: 0x{:02x} v{} (expected 0x{:02x} v{})",
            bytes[0], bytes[1], VOXEL_KEY_TAG, VOXEL_KEY_VERSION
        )));
    }
    let mut m = [0u8; 8];
    m.copy_from_slice(&bytes[2..10]);
    let (cx, cy, cz) = morton3_decode(u64::from_be_bytes(m));
    Ok((
        cell_to_chunk_axis(cx),
        cell_to_chunk_axis(cy),
        cell_to_chunk_axis(cz),
    ))
}

/// Default voxel chunk edge in world studs.
///
/// ASSUMPTION (Wave 9.A): the Roblox terrain importer writes chunks at
/// `CHUNK_EDGE = 32` cells × `ROBLOX_CELL_STUDS = 4` studs/cell =
/// **128 studs** per chunk along each axis (per
/// `docs/architecture/TERRAIN_FJALL_MIGRATION.md §9.A`). The terrain
/// importer (`roblox-import/src/terrain.rs`) does NOT exist in this
/// crate's branch yet (it lands in a later wave), so this constant is
/// the single source of truth the storage layer uses to translate a
/// world-space region box into a chunk-coord box. When the importer
/// lands it MUST agree with this value (or both move together). The
/// `*_in_region` API also exposes a `chunk_edge`-parameterised variant
/// so callers with a different edge are never silently wrong.
pub const VOXEL_CHUNK_EDGE_STUDS: f32 = 128.0;

/// World coordinate → the chunk index that contains it, at `chunk_edge`
/// studs per chunk. Floor division so the boundary stud belongs to the
/// higher chunk consistently and negative coords map correctly
/// (e.g. `-1.0 / 128.0` → chunk `-1`, not `0`).
pub fn world_to_chunk_coord(coord: f32, chunk_edge: f32) -> i32 {
    (coord / chunk_edge).floor() as i32
}

/// Spatial Morton key encoder — Phase 2, real. The key layout is
///
/// ```text
/// M | schema_ver(2) | morton63(cell_x,cell_y,cell_z) be8 | component be2 | entity be8
/// ```
///
/// so a Fjall range scan over a Morton prefix yields spatially
/// clustered entities and Fjall's block format keeps neighbours in
/// the same SSTable block.
///
/// The [`KeyEncoder`] trait can't carry a position into
/// `encode_component`, so spatial placement is done via
/// [`MortonKeyEncoder::encode_spatial`] (called by the bake / stream
/// path which *does* hold the transform). The trait methods produce a
/// valid, fully round-trippable v2 key with the entity in the spatial
/// slot zeroed — correct and decodable, just not yet clustered. This
/// is a real encoder (not the old empty-vec stub); wiring the
/// position-carrying path is a [05]/Phase-5 integration, not a
/// keys.rs change.
#[derive(Debug, Clone)]
pub struct MortonKeyEncoder {
    /// Cell size in world units; must match
    /// `streaming::types::StreamingConfig::chunk_size` so a range scan
    /// translates 1:1 to a chunk request.
    pub chunk_size: f32,
}

impl Default for MortonKeyEncoder {
    fn default() -> Self {
        Self { chunk_size: 256.0 }
    }
}

impl MortonKeyEncoder {
    const TAG: u8 = b'M';
    const SCHEMA_VERSION: u8 = 2;

    /// Encode a spatially-placed component key. Called by the
    /// bake/stream path which holds the entity's world position.
    pub fn encode_spatial(
        &self,
        entity: EntityId,
        component: ComponentTypeId,
        pos: (f32, f32, f32),
    ) -> Vec<u8> {
        let cx = world_to_cell(pos.0, self.chunk_size);
        let cy = world_to_cell(pos.1, self.chunk_size);
        let cz = world_to_cell(pos.2, self.chunk_size);
        let morton = morton3_encode(cx, cy, cz);
        let mut out = Vec::with_capacity(1 + 1 + 8 + 2 + 8);
        out.push(Self::TAG);
        out.push(Self::SCHEMA_VERSION);
        out.extend_from_slice(&morton.to_be_bytes());
        out.extend_from_slice(&component.0.to_be_bytes());
        out.extend_from_slice(&entity.0.to_be_bytes());
        out
    }

    /// Half-open Morton prefix covering one cell — the building block
    /// for "entities within radius" (callers union the cells a sphere
    /// touches and range-scan each).
    pub fn cell_prefix(&self, cx: u32, cy: u32, cz: u32) -> Vec<u8> {
        let morton = morton3_encode(cx, cy, cz);
        let mut out = Vec::with_capacity(1 + 1 + 8);
        out.push(Self::TAG);
        out.push(Self::SCHEMA_VERSION);
        out.extend_from_slice(&morton.to_be_bytes());
        out
    }
}

impl KeyEncoder for MortonKeyEncoder {
    fn schema_version(&self) -> u8 {
        Self::SCHEMA_VERSION
    }

    fn encode_component(&self, entity: EntityId, component: ComponentTypeId) -> Vec<u8> {
        // No position available at this trait boundary → place at the
        // origin cell. Still a valid, decodable v2 key.
        self.encode_spatial(entity, component, (0.0, 0.0, 0.0))
    }

    fn component_prefix(&self, _component: ComponentTypeId) -> Vec<u8> {
        // Component isn't a prefix in the spatial layout (Morton comes
        // first). A full component scan walks all cells; callers that
        // need per-component iteration use the flat encoder. Return the
        // tag+version so the scan is bounded to v2 keys.
        vec![Self::TAG, Self::SCHEMA_VERSION]
    }

    fn entity_prefix(&self, _entity: EntityId) -> Vec<u8> {
        Vec::new()
    }

    fn decode_component(&self, bytes: &[u8]) -> Result<(EntityId, ComponentTypeId)> {
        if bytes.len() != 1 + 1 + 8 + 2 + 8 {
            return Err(Error::KeyDecode(format!(
                "morton key wrong length: {} (expected 20)",
                bytes.len()
            )));
        }
        if bytes[0] != Self::TAG || bytes[1] != Self::SCHEMA_VERSION {
            return Err(Error::KeyDecode(
                "morton key wrong tag/version".to_string(),
            ));
        }
        let mut c = [0u8; 2];
        c.copy_from_slice(&bytes[10..12]);
        let mut e = [0u8; 8];
        e.copy_from_slice(&bytes[12..20]);
        Ok((
            EntityId(u64::from_be_bytes(e)),
            ComponentTypeId(u16::from_be_bytes(c)),
        ))
    }
}

#[cfg(test)]
mod morton_tests {
    use super::*;

    #[test]
    fn morton_roundtrip() {
        for &(x, y, z) in &[(0, 0, 0), (1, 2, 3), (1_048_576, 7, 99), (0x1f_ffff, 0, 0x1f_ffff)] {
            let code = morton3_encode(x, y, z);
            assert_eq!(morton3_decode(code), (x, y, z), "roundtrip {x},{y},{z}");
        }
    }

    #[test]
    fn morton_locality() {
        // Adjacent cells must produce codes closer than far cells.
        let a = morton3_encode(100, 100, 100);
        let near = morton3_encode(101, 100, 100);
        let far = morton3_encode(100_000, 100, 100);
        assert!((a as i128 - near as i128).abs() < (a as i128 - far as i128).abs());
    }

    #[test]
    fn morton_key_roundtrip() {
        let enc = MortonKeyEncoder::default();
        let k = enc.encode_spatial(EntityId(42), ComponentTypeId::TRANSFORM, (10.0, -5.0, 3.0));
        let (e, c) = enc.decode_component(&k).unwrap();
        assert_eq!(e, EntityId(42));
        assert_eq!(c, ComponentTypeId::TRANSFORM);
    }

    #[test]
    fn voxel_chunk_key_roundtrip_incl_negatives() {
        // decode(encode(c)) == c for the full range we promise, including
        // negatives, zero, and the addressable extremes.
        for &(cx, cy, cz) in &[
            (0, 0, 0),
            (1, 2, 3),
            (-1, -1, -1),
            (-7, 12, -300),
            (123_456, -654_321, 7),
            (-(1 << 20), 0, (1 << 20) - 1),
            ((1 << 20) - 1, -(1 << 20), -(1 << 20)),
        ] {
            let k = encode_voxel_chunk_key(cx, cy, cz);
            assert_eq!(
                decode_voxel_chunk_key(&k).unwrap(),
                (cx, cy, cz),
                "voxel chunk roundtrip {cx},{cy},{cz}"
            );
        }
    }

    #[test]
    fn voxel_chunk_key_is_tagged_and_bounded() {
        let k = encode_voxel_chunk_key(-5, 9, -2);
        assert_eq!(k.len(), 10);
        assert_eq!(&k[..2], &voxel_key_prefix()[..]);
        // The prefix every key shares is the scan bound.
        assert!(k.starts_with(&voxel_key_prefix()));
    }

    #[test]
    fn voxel_chunk_key_rejects_foreign_bytes() {
        // Wrong length.
        assert!(decode_voxel_chunk_key(&[b'V', 1, 0, 0]).is_err());
        // Wrong tag (an entity Morton key, say).
        let mut bad = encode_voxel_chunk_key(1, 1, 1);
        bad[0] = b'M';
        assert!(decode_voxel_chunk_key(&bad).is_err());
    }

    #[test]
    fn voxel_chunk_key_preserves_locality() {
        // Neighbouring chunks → numerically closer keys than far chunks.
        let here = u64::from_be_bytes(encode_voxel_chunk_key(50, 50, 50)[2..10].try_into().unwrap());
        let near = u64::from_be_bytes(encode_voxel_chunk_key(51, 50, 50)[2..10].try_into().unwrap());
        let far = u64::from_be_bytes(encode_voxel_chunk_key(5000, 50, 50)[2..10].try_into().unwrap());
        assert!(
            (here as i128 - near as i128).abs() < (here as i128 - far as i128).abs(),
            "near chunk key must be closer than far chunk key"
        );
    }

    #[test]
    fn world_to_chunk_coord_floors_and_handles_negatives() {
        let edge = VOXEL_CHUNK_EDGE_STUDS;
        assert_eq!(world_to_chunk_coord(0.0, edge), 0);
        assert_eq!(world_to_chunk_coord(127.9, edge), 0);
        assert_eq!(world_to_chunk_coord(128.0, edge), 1);
        assert_eq!(world_to_chunk_coord(-0.1, edge), -1);
        assert_eq!(world_to_chunk_coord(-128.0, edge), -1);
        assert_eq!(world_to_chunk_coord(-128.1, edge), -2);
    }
}
