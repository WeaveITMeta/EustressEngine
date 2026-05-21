//! Phase 4 — rkyv zero-copy component values.
//!
//! Bevy's `Transform` (and friends) don't derive rkyv, and this crate
//! is engine-free (no `bevy` dep), so the hot ECS components get a
//! plain-data mirror here that DOES derive rkyv. The engine's
//! `world_db_plugin` converts `bevy::Transform` ↔ [`ArchTransform`]
//! and stores the rkyv archive in the `entities` partition.
//!
//! Read path is zero-copy: [`access_transform`] casts the stored
//! bytes straight to `&ArchivedArchTransform` with no allocation and
//! no field-by-field parse — the Tier-1 #2 win. Write path is one
//! `rkyv::to_bytes`.
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
/// byte lets [`access_transform`] reject foreign / wrong-version
/// bytes before the zero-copy cast.
pub fn encode_transform(v: &ArchTransform) -> crate::error::Result<Vec<u8>> {
    let archived = rkyv::to_bytes::<rkyv::rancor::Error>(v)
        .map_err(|e| crate::error::Error::Archive(format!("rkyv encode: {e}")))?;
    let mut out = Vec::with_capacity(archived.len() + 1);
    out.push(RKYV_VALUE_TAG);
    out.extend_from_slice(&archived);
    Ok(out)
}

/// Zero-copy access — validates + casts the stored bytes to the
/// archived view without deserialising. Hot read path: a 50k-entity
/// load is 50k pointer casts, not 50k allocations.
pub fn access_transform(
    bytes: &[u8],
) -> crate::error::Result<&ArchivedArchTransform> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(crate::error::Error::Archive(
            "rkyv value tag mismatch (foreign or wrong schema version)".into(),
        ));
    }
    rkyv::access::<ArchivedArchTransform, rkyv::rancor::Error>(&bytes[1..])
        .map_err(|e| crate::error::Error::Archive(format!("rkyv access: {e}")))
}

/// Full owned deserialize — for the cold path that needs an owned
/// `ArchTransform` (e.g. the engine converting back to
/// `bevy::Transform`). Still cheaper than TOML parse.
pub fn decode_transform(bytes: &[u8]) -> crate::error::Result<ArchTransform> {
    let archived = access_transform(bytes)?;
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
pub enum EusValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Datetime(String),
    Array(Vec<EusValue>),
    Table(Vec<(String, EusValue)>),
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

/// Owned decode of a tagged [`EusValue`] archive.
pub fn decode_eusvalue(bytes: &[u8]) -> crate::error::Result<EusValue> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(crate::error::Error::Archive(
            "rkyv value tag mismatch (EusValue)".into(),
        ));
    }
    let archived = rkyv::access::<ArchivedEusValue, rkyv::rancor::Error>(&bytes[1..])
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

/// Zero-copy view of a stored [`ArchInstanceCore`] — the load hot path
/// (a cast + validate, no allocation, no parse).
pub fn access_instance_core(bytes: &[u8]) -> crate::error::Result<&ArchivedArchInstanceCore> {
    if bytes.is_empty() || bytes[0] != RKYV_VALUE_TAG {
        return Err(crate::error::Error::Archive(
            "rkyv value tag mismatch (ArchInstanceCore)".into(),
        ));
    }
    rkyv::access::<ArchivedArchInstanceCore, rkyv::rancor::Error>(&bytes[1..])
        .map_err(|e| crate::error::Error::Archive(format!("rkyv access ArchInstanceCore: {e}")))
}

/// Owned decode (cold path).
pub fn decode_instance_core(bytes: &[u8]) -> crate::error::Result<ArchInstanceCore> {
    let archived = access_instance_core(bytes)?;
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
        // zero-copy view
        let a = access_transform(&bytes).unwrap();
        assert_eq!(a.t[0], 1.0);
        assert_eq!(a.s[1], 2.0);
        // owned roundtrip
        assert_eq!(decode_transform(&bytes).unwrap(), v);
    }

    #[test]
    fn rejects_untagged() {
        assert!(access_transform(&[]).is_err());
        assert!(access_transform(&[0xff, 1, 2, 3]).is_err());
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
        // zero-copy view
        let a = access_instance_core(&bytes).unwrap();
        assert_eq!(a.t[0], 1.0);
        assert_eq!(a.s[1], 1.5);
        // owned roundtrip
        assert_eq!(decode_instance_core(&bytes).unwrap(), v);
    }
}
