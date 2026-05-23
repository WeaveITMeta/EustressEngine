//! Phase 4 — rkyv zero-copy component values.
//!
//! Bevy's `Transform` (and friends) don't derive rkyv, and this crate
//! is engine-free (no `bevy` dep), so the hot ECS components get a
//! plain-data mirror here that DOES derive rkyv. The engine's
//! `world_db_plugin` converts `bevy::Transform` ↔ [`ArchTransform`]
//! and stores the rkyv archive in the `entities` partition.
//!
//! Read path is [`decode_transform`] / [`decode_instance_core`]: copy
//! past the tag byte into a 16-byte-aligned buffer, then `rkyv::access`
//! (validate) + `rkyv::deserialize`. The copy is forced by storage
//! reality, not laziness — Fjall returns unaligned `Vec<u8>` and rkyv's
//! `access` requires the archive root to be aligned, while the leading
//! tag byte offsets the archive by one. True zero-copy would need the
//! store to hand back aligned, untagged buffers; it doesn't. Even with
//! the copy this is far cheaper than the TOML parse the pivot replaced.
//! Write path is one `rkyv::to_bytes` + a tag byte.
//!
//! Versioned: byte 0 is a layout tag so a future component-schema
//! bump can coexist (mirrors the `header.bin` / `v{N}:` story).

use rkyv::{Archive, Deserialize, Serialize};

/// Layout tag prepended to every archived value. Bump in lockstep
/// with [`crate::header::WorldSchemaVersion`] if the mirror structs
/// change shape.
pub const RKYV_VALUE_TAG: u8 = 1;

/// rkyv mirror of `bevy::prelude::Transform`. Field order/types are
/// the stable wire contract — never reorder; add trailing fields with
/// a tag bump instead.
#[derive(Archive, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct ArchTransform {
    /// translation x,y,z
    pub t: [f32; 3],
    /// rotation quaternion x,y,z,w
    pub r: [f32; 4],
    /// scale x,y,z
    pub s: [f32; 3],
}

impl ArchTransform {
    /// Build from raw components (engine passes Bevy `Transform`
    /// fields; no `bevy` dep here).
    pub fn new(t: [f32; 3], r: [f32; 4], s: [f32; 3]) -> Self {
        Self { t, r, s }
    }
}

/// Serialize a transform to a tagged rkyv archive. The leading tag
/// byte lets [`decode_transform`] reject foreign / wrong-version bytes
/// before the access.
pub fn encode_transform(v: &ArchTransform) -> crate::error::Result<Vec<u8>> {
    let archived = rkyv::to_bytes::<rkyv::rancor::Error>(v)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv encode: {e}")))?;
    let mut out = Vec::with_capacity(archived.len() + 1);
    out.push(RKYV_VALUE_TAG);
    out.extend_from_slice(&archived);
    Ok(out)
}

/// Owned deserialize — the read path for bytes coming back from the
/// store. Fjall hands back byte buffers with no alignment guarantee and
/// the 1-byte tag offsets the archive, so we copy into an aligned buffer
/// before the rkyv access (genuine zero-copy would require the store to
/// return aligned bytes, which Fjall doesn't). Still far cheaper than a
/// TOML parse — the original comparison the pivot was measured against.
pub fn decode_transform(bytes: &[u8]) -> crate::error::Result<ArchTransform> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(crate::error::Error::Archive(
            "rkyv value tag mismatch (foreign or wrong schema version)".into(),
        ));
    }
    let mut aligned = rkyv::util::AlignedVec::<16>::new();
    aligned.extend_from_slice(&bytes[1..]);
    let archived = rkyv::access::<ArchivedArchTransform, rkyv::rancor::Error>(aligned.as_slice())
        .map_err(|e| crate::error::Error::Archive(format!("rkyv access: {e}")))?;
    rkyv::deserialize::<ArchTransform, rkyv::rancor::Error>(archived)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv decode: {e}")))
}

// ── EusValue — rkyv mirror of `toml::Value` (the K2 unblock) ─────────
//
// rkyv CANNOT archive `toml::Value` (it deserialises via
// `deserialize_any`, and its `Datetime` isn't rkyv-Archivable), and it
// cannot archive a `#[serde(flatten)]` map. The rkyv-everywhere world
// model therefore stores its extensible/plugin tail (`extra`,
// `attributes`, `parameters`) as `EusValue` instead, converting
// to/from `toml::Value` ONLY at bake/parse time — never at runtime.

/// rkyv-archivable value tree mirroring `toml::Value`. Recursion is
/// heap-broken through `Vec`, so the archived form has a fixed size.
/// `Table` is a sorted key/value list (deterministic bytes, no map
/// ordering ambiguity). `Datetime` is the RFC3339 string form (TOML
/// datetimes are rare in the world model; text keeps it rkyv-friendly).
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
// Recursive type: `Array`/`Table` hold `EusValue`s. rkyv's derive would
// otherwise emit a per-field `where EusValue: Archive` bound that
// recurses forever (E0275 "overflow evaluating the requirement"). Fix is
// the canonical rkyv-0.8 recursive-type pattern: `omit_bounds` on the
// recursive fields drops the self-referential auto-bound, and the
// container restates the MINIMAL bounds each derived impl actually needs
// (Vec serialization wants a Writer+Allocator; validation wants an
// ArchiveContext). The concrete recursion then resolves structurally at
// monomorphization. Same shape as rkyv's own recursive-JSON example.
#[rkyv(
    serialize_bounds(
        __S: rkyv::ser::Writer + rkyv::ser::Allocator,
        __S::Error: rkyv::rancor::Source,
    ),
    deserialize_bounds(__D::Error: rkyv::rancor::Source),
    bytecheck(
        bounds(
            __C: rkyv::validation::ArchiveContext,
            __C::Error: rkyv::rancor::Source,
        )
    )
)]
pub enum EusValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Datetime(String),
    Array(#[rkyv(omit_bounds)] Vec<EusValue>),
    Table(#[rkyv(omit_bounds)] Vec<(String, EusValue)>),
}

/// Encode an [`EusValue`] to a tagged rkyv archive (same tag scheme as
/// [`encode_transform`]).
pub fn encode_eusvalue(v: &EusValue) -> crate::error::Result<Vec<u8>> {
    let archived = rkyv::to_bytes::<rkyv::rancor::Error>(v)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv encode EusValue: {e}")))?;
    let mut out = Vec::with_capacity(archived.len() + 1);
    out.push(RKYV_VALUE_TAG);
    out.extend_from_slice(&archived);
    Ok(out)
}

/// Owned decode of a tagged [`EusValue`] archive. Like
/// [`decode_transform`], copies past the tag byte into an aligned buffer
/// because Fjall hands back unaligned `Vec<u8>` and rkyv's `access`
/// requires the archive root to be aligned (EusValue contains i64/f64 →
/// 8-byte alignment).
pub fn decode_eusvalue(bytes: &[u8]) -> crate::error::Result<EusValue> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(crate::error::Error::Archive(
            "rkyv value tag mismatch (EusValue)".into(),
        ));
    }
    let mut aligned = rkyv::util::AlignedVec::<16>::new();
    aligned.extend_from_slice(&bytes[1..]);
    let archived = rkyv::access::<ArchivedEusValue, rkyv::rancor::Error>(aligned.as_slice())
        .map_err(|e| crate::error::Error::Archive(format!("rkyv access EusValue: {e}")))?;
    rkyv::deserialize::<EusValue, rkyv::rancor::Error>(archived)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv decode EusValue: {e}")))
}

// `toml::Value ↔ EusValue` — used ONLY at bake/parse time (the eager
// TOML→rkyv conversion), never at runtime. These impls live here
// because `EusValue` is local to this crate (orphan rule), and worlddb
// already depends on `toml` (header.bin body). Datetimes round-trip via
// their RFC3339 string form; `Table` keys are sorted for deterministic
// archive bytes.
impl From<toml::Value> for EusValue {
    fn from(v: toml::Value) -> Self {
        match v {
            toml::Value::String(s) => EusValue::String(s),
            toml::Value::Integer(i) => EusValue::Int(i),
            toml::Value::Float(f) => EusValue::Float(f),
            toml::Value::Boolean(b) => EusValue::Bool(b),
            toml::Value::Datetime(dt) => EusValue::Datetime(dt.to_string()),
            toml::Value::Array(a) => EusValue::Array(a.into_iter().map(EusValue::from).collect()),
            toml::Value::Table(t) => {
                let mut kvs: Vec<(String, EusValue)> =
                    t.into_iter().map(|(k, v)| (k, EusValue::from(v))).collect();
                kvs.sort_by(|a, b| a.0.cmp(&b.0));
                EusValue::Table(kvs)
            }
        }
    }
}

impl From<EusValue> for toml::Value {
    fn from(v: EusValue) -> Self {
        match v {
            EusValue::String(s) => toml::Value::String(s),
            EusValue::Int(i) => toml::Value::Integer(i),
            EusValue::Float(f) => toml::Value::Float(f),
            EusValue::Bool(b) => toml::Value::Boolean(b),
            EusValue::Datetime(s) => s
                .parse::<toml::value::Datetime>()
                .map(toml::Value::Datetime)
                .unwrap_or(toml::Value::String(s)),
            EusValue::Array(a) => {
                toml::Value::Array(a.into_iter().map(toml::Value::from).collect())
            }
            EusValue::Table(kvs) => {
                let mut t = toml::value::Table::new();
                for (k, val) in kvs {
                    t.insert(k, toml::Value::from(val));
                }
                toml::Value::Table(t)
            }
        }
    }
}

// ── ArchInstanceCore — the zero-copy rkyv archive-model (path B) ─────
//
// The user chose per-type zero-copy Archive derives. To avoid the
// serde-`flatten` + `toml::Value` + rkyv minefield, the rkyv
// archive-model is a SEPARATE mirror of the engine's serde
// `InstanceDefinition` (which stays unchanged as the parse model). The
// engine maps `InstanceDefinition` ↔ this at bake/load time; the load
// hot path reads `&ArchivedArchInstanceCore` zero-copy.
//
// This first cut covers the load-bearing instance core (identity,
// asset, transform, the rendered/physics properties) + the extensible
// tail as `EusValue`. Remaining nested sections (realism material /
// thermodynamic / electrochemical, UI) are added in later increments
// as their own rkyv mirror structs — same pattern.

/// rkyv archive-model mirror of an instance's core. Field order/types
/// are the stable wire contract (append-only; bump [`RKYV_VALUE_TAG`]
/// on a shape change).
#[derive(Archive, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ArchInstanceCore {
    pub class_name: String,
    /// `asset.mesh` (empty for non-visual instances).
    pub mesh: String,
    /// `asset.scene` (e.g. "Scene0").
    pub scene: String,
    /// translation x,y,z
    pub t: [f32; 3],
    /// rotation quaternion x,y,z,w
    pub r: [f32; 4],
    /// scale x,y,z
    pub s: [f32; 3],
    /// linear-rgba color
    pub color: [f32; 4],
    pub transparency: f32,
    pub reflectance: f32,
    pub anchored: bool,
    pub can_collide: bool,
    pub cast_shadow: bool,
    pub locked: bool,
    /// material preset / custom material name
    pub material: String,
    pub tags: Vec<String>,
    /// Extensible tail: merged `attributes` + `parameters` + unknown
    /// `extra` sections, as rkyv-archivable values (sorted by key).
    pub extra: Vec<(String, EusValue)>,
}

/// Encode an [`ArchInstanceCore`] to a tagged rkyv archive.
pub fn encode_instance_core(v: &ArchInstanceCore) -> crate::error::Result<Vec<u8>> {
    let archived = rkyv::to_bytes::<rkyv::rancor::Error>(v)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv encode ArchInstanceCore: {e}")))?;
    let mut out = Vec::with_capacity(archived.len() + 1);
    out.push(RKYV_VALUE_TAG);
    out.extend_from_slice(&archived);
    Ok(out)
}

/// Owned decode — the real load path. Copies past the tag byte into an
/// aligned buffer (Fjall buffers are unaligned and the tag offsets the
/// archive; the contained EusValue tail needs 8-byte alignment), then
/// validates + deserializes.
pub fn decode_instance_core(bytes: &[u8]) -> crate::error::Result<ArchInstanceCore> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(crate::error::Error::Archive(
            "rkyv value tag mismatch (ArchInstanceCore)".into(),
        ));
    }
    let mut aligned = rkyv::util::AlignedVec::<16>::new();
    aligned.extend_from_slice(&bytes[1..]);
    let archived = rkyv::access::<ArchivedArchInstanceCore, rkyv::rancor::Error>(aligned.as_slice())
        .map_err(|e| crate::error::Error::Archive(format!("rkyv access ArchInstanceCore: {e}")))?;
    rkyv::deserialize::<ArchInstanceCore, rkyv::rancor::Error>(archived)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv decode ArchInstanceCore: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_rkyv_roundtrip() {
        let v = ArchTransform::new([1.0, -2.5, 3.0], [0.0, 0.0, 0.0, 1.0], [2.0, 2.0, 2.0]);
        let bytes = encode_transform(&v).unwrap();
        let d = decode_transform(&bytes).unwrap();
        assert_eq!(d.t[0], 1.0);
        assert_eq!(d.s[1], 2.0);
        // full owned roundtrip
        assert_eq!(d, v);
    }

    #[test]
    fn rejects_untagged() {
        assert!(decode_transform(&[]).is_err());
        assert!(decode_transform(&[0xff, 1, 2, 3]).is_err());
    }

    #[test]
    fn eusvalue_rkyv_roundtrip() {
        // Nested/recursive value proves the heap-broken recursive enum
        // archives + roundtrips — the prerequisite for archiving the
        // whole InstanceDefinition graph on top of it.
        let v = EusValue::Table(vec![
            ("name".into(), EusValue::String("Part".into())),
            ("count".into(), EusValue::Int(3)),
            ("scale".into(), EusValue::Float(1.5)),
            ("on".into(), EusValue::Bool(true)),
            ("when".into(), EusValue::Datetime("2026-05-17T00:00:00Z".into())),
            (
                "xs".into(),
                EusValue::Array(vec![EusValue::Int(1), EusValue::Int(2)]),
            ),
            (
                "nested".into(),
                EusValue::Table(vec![("k".into(), EusValue::String("v".into()))]),
            ),
        ]);
        let bytes = encode_eusvalue(&v).unwrap();
        assert_eq!(decode_eusvalue(&bytes).unwrap(), v);
    }

    #[test]
    fn toml_eusvalue_roundtrip() {
        // A representative TOML tail (strings, ints, floats, bool,
        // nested array + table) survives toml → EusValue → rkyv →
        // EusValue → toml. This is the bake/parse bridge the
        // rkyv-everywhere model relies on for its extensible sections.
        let toml_src = r#"
            name = "Part"
            count = 3
            scale = 1.5
            on = true
            tags = ["a", "b"]
            [sub]
            k = "v"
            n = 7
        "#;
        let original: toml::Value = toml::from_str(toml_src).unwrap();
        let eus: EusValue = original.clone().into();
        // through rkyv and back
        let bytes = encode_eusvalue(&eus).unwrap();
        let eus2 = decode_eusvalue(&bytes).unwrap();
        let back: toml::Value = eus2.into();
        assert_eq!(back, original);
    }

    #[test]
    fn instance_core_rkyv_roundtrip() {
        // Real instance-shaped record: identity + asset + transform +
        // rendered/physics props + tags + EusValue tail. Proves the
        // per-type zero-copy archive-model (path B) roundtrips and is
        // zero-copy-accessible — the foundation the full graph mirror +
        // engine InstanceDefinition↔ArchInstanceCore mapping builds on.
        let v = ArchInstanceCore {
            class_name: "Part".into(),
            mesh: "parts/block.glb".into(),
            scene: "Scene0".into(),
            t: [1.0, 2.0, 3.0],
            r: [0.0, 0.0, 0.0, 1.0],
            s: [1.5, 1.5, 1.5],
            color: [0.2, 0.4, 0.6, 1.0],
            transparency: 0.0,
            reflectance: 0.1,
            anchored: true,
            can_collide: false,
            cast_shadow: false,
            locked: false,
            material: "Plastic".into(),
            tags: vec!["bench".into(), "static".into()],
            extra: vec![(
                "Appearance".into(),
                EusValue::Table(vec![("emissive".into(), EusValue::Float(0.0))]),
            )],
        };
        let bytes = encode_instance_core(&v).unwrap();
        let d = decode_instance_core(&bytes).unwrap();
        assert_eq!(d.t[0], 1.0);
        assert_eq!(d.s[1], 1.5);
        // full owned roundtrip
        assert_eq!(d, v);
    }
}
