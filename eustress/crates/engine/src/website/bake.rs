//! # Baking the manifest
//!
//! Turns a Space's `Website` service into the one small JSON object a website
//! fetches to update every number it shows.
//!
//! ## One fetch, not one per value
//!
//! The design constraint that shapes everything here: a page quoting
//! twenty-five numbers must cost one request, and adding a twenty-sixth must
//! cost nothing. So the bake emits a single document keyed by reference name,
//! and the consumer walks its own DOM rather than the network.
//!
//! ## Two objects, never one
//!
//! A publish writes two things and they are never the same object:
//!
//! | object | key | what it is |
//! |---|---|---|
//! | manifest | `universes/{id}/website-manifest.json` | the numbers a site quotes |
//! | namespace pointer | `universes/_namespaces/{namespace}.json` | which publish is current |
//!
//! Neither is the simulation listing record. The listing is the marketplace
//! entry: name, description, thumbnail, play count. Writing a manifest over it
//! takes the Space out of the marketplace and nothing surfaces that until
//! somebody goes looking, so the two are written by different requests to
//! different keys and they never share one.
//!
//! `_namespaces` cannot collide with a simulation id because a UUID contains
//! no underscore, which is why the pointer can live under the same `universes/`
//! prefix as everything else a publish writes.
//!
//! ## Both `value` and `display`
//!
//! Every entry carries a typed `value` and a formatted `display`. That pairing
//! is load-bearing: a consumer that formats `value` itself drifts from the
//! author's rounding, and a consumer that parses `display` back into a number
//! gets it wrong the first time a unit appears in it. The author's formatting
//! is authored once, here, and never reimplemented downstream.
//!
//! ## No key appears in the body
//!
//! A manifest carrying its own key hands that key to everyone holding a cached
//! copy, including every CDN and every archive, and makes rotation pointless.
//! The key travels on the request. [`WebsiteManifest`] has no field for one,
//! and a test asserts a baked document contains no key material.
//!
//! ## Table of Contents
//!
//! 1. Object keys and [`BakeError`]
//! 2. [`WebsiteManifest`] and [`ManifestValue`] - the published shape
//! 3. Formatting: `format` to `display`
//! 4. [`WebsiteServiceConfig`] - reading `Website/_service.toml`
//! 5. [`bake_from_world`] - the publish seam
//! 6. [`PendingManifest`] and [`BakedManifest`]
//! 7. Tests

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bevy::prelude::*;
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

use super::key::{squash, KeyKind, KeySet};
use super::resolve::{
    resolve_all, ErrorKind, Reference, ReferenceKind, ResolveError, Scalar, WorldDatamodel,
};
use crate::space::space_source::{ActiveSpaceSource, DiskSource, SpaceSource};

// ============================================================================
// 1. Object keys and errors
// ============================================================================

/// The folder a Space keeps its references in.
pub const SERVICE_FOLDER: &str = "Website";

/// Leaf name of the published manifest object.
pub const MANIFEST_OBJECT_NAME: &str = "website-manifest.json";

/// Prefix the namespace pointers live under.
pub const NAMESPACE_PREFIX: &str = "universes/_namespaces";

/// R2 key of the manifest for one publish.
///
/// A site should not use the id route: `POST /api/simulations/publish` mints a
/// fresh UUID on every publish, so a URL with a UUID in it is correct until the
/// next publish and silently stale after. The id route is for pinning one
/// publish and for the engine to verify its own upload.
pub fn manifest_object_key(simulation_id: &str) -> String {
    format!("universes/{simulation_id}/{MANIFEST_OBJECT_NAME}")
}

/// R2 key of the pointer a site actually reads through.
pub fn namespace_pointer_key(namespace: &str) -> String {
    format!("{NAMESPACE_PREFIX}/{namespace}.json")
}

/// The publish hash in a form that names its algorithm.
///
/// The engine's own `.last_publish_hash` file holds a bare BLAKE3 hex, which
/// is fine because only the engine reads it. This value travels to third
/// parties as an `ETag` and as a `?v=` pin, so it says which digest it is.
pub fn blake3_publish_hash(bytes: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(bytes).to_hex())
}

/// Why a bake stopped.
///
/// Every variant fails the publish. There is no variant meaning "carry on
/// without this value": a manifest that silently drops a reference is a
/// website quietly keeping last month's number.
#[derive(Debug)]
pub enum BakeError {
    /// The `Website` service itself is misconfigured.
    Service(String),
    /// One reference could not be resolved.
    Reference(ResolveError),
    /// The manifest could not be serialized.
    Encode(String),
}

impl std::fmt::Display for BakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BakeError::Service(m) => write!(f, "Website service: {m}"),
            BakeError::Reference(e) => write!(f, "{e}"),
            BakeError::Encode(m) => write!(f, "manifest encode failed: {m}"),
        }
    }
}

impl std::error::Error for BakeError {}

impl From<ResolveError> for BakeError {
    fn from(e: ResolveError) -> Self {
        BakeError::Reference(e)
    }
}

// ============================================================================
// 2. The published shape
// ============================================================================

/// One value in the manifest.
///
/// `value` is for arithmetic and `display` is for the DOM. Both are always
/// present; a consumer never has to choose between reimplementing the author's
/// formatting and parsing a formatted string back into a number.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ManifestValue {
    /// Typed. A JSON number, string or boolean, never null.
    pub value: serde_json::Value,
    /// Unit for display, when the author declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Human label.
    pub label: String,
    /// Where the number came from: measured, simulated, derived, counted.
    ///
    /// It travels with every value because a figure without its basis is not a
    /// figure. A site can render `simulated` differently from `measured`
    /// without maintaining its own list of which is which.
    pub basis: String,
    /// The author's format string, echoed so a build-time bake can reproduce
    /// `display` without guessing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Formatted for the DOM.
    pub display: String,
    /// The source this was resolved from, so a reader can trace it back.
    pub source: String,
    /// `sim` values only: the run this number came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_label: Option<String>,
}

/// The published document.
///
/// Note the absent field: there is no key here, and there never will be.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WebsiteManifest {
    /// Stable identifier the manifest is published under.
    pub namespace: String,
    /// Bumped by the author when the meaning of a key changes rather than its
    /// value, so a rename is a breaking change consumers opt into. Issuing or
    /// rotating a key never bumps it: transport is not meaning, and bumping on
    /// a key change would hard-stop every consumer on a day no number moved.
    pub schema_version: u32,
    /// The UUID in the R2 key and in the route, not a Space identifier. Here
    /// so a reader of a saved manifest can find the publish it came from.
    pub simulation_id: String,
    /// Digest of the packaged Universe. Doubles as the `ETag`.
    pub publish_hash: String,
    /// RFC 3339 UTC.
    pub baked_at: String,
    /// Engine that produced this.
    pub engine_version: String,
    /// Keyed by reference name. Sorted, so two bakes of one Space diff
    /// cleanly.
    pub values: BTreeMap<String, ManifestValue>,
}

/// What the namespace pointer holds.
///
/// Written by the same publish as the manifest, so it always resolves to the
/// current one.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NamespacePointer {
    /// Which namespace this points for.
    pub namespace: String,
    /// The publish the manifest lives under.
    pub simulation_id: String,
    /// Digest of that publish.
    pub publish_hash: String,
    /// RFC 3339 UTC of this publish.
    pub updated_at: String,
}

// ============================================================================
// 3. Formatting
// ============================================================================

/// The subset of format specs this engine applies.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NumberSpec {
    /// Fixed decimal places.
    precision: Option<usize>,
    /// Thousands separators in the integer part.
    group: bool,
    /// Scientific notation.
    scientific: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Placeholder {
    /// `{}`: whatever the value's natural rendering is.
    Plain,
    /// `{:...}`: numeric, so it needs a number.
    Number(NumberSpec),
}

/// Render a value the way the author asked.
///
/// The supported grammar is deliberately small and deliberately explicit:
///
/// | spec | result for `2341.5` |
/// |---|---|
/// | absent, or `{}` | `2341.5` |
/// | `{:.0}` | `2342` |
/// | `{:.2}` | `2341.50` |
/// | `{:,}` | `2,341.5` |
/// | `{:,.0}` | `2,342` |
/// | `{:e}` | `2.3415e3` |
///
/// Literal text around the placeholder is kept, so `"{:.0} Wh/kg"` renders
/// `"953 Wh/kg"`, and `{{` / `}}` escape a literal brace.
///
/// Anything outside that grammar is an error rather than a silent fallback to
/// `{}`. An author who wrote a spec this engine cannot apply wants to be told,
/// not to find a differently-rounded number on their site.
pub fn format_scalar(value: &Scalar, format: Option<&str>) -> Result<String, String> {
    let Some(fmt) = format else {
        return Ok(natural(value));
    };

    let (prefix, placeholder, suffix) = parse_format(fmt)?;
    let body = match placeholder {
        Placeholder::Plain => natural(value),
        Placeholder::Number(spec) => {
            let Some(n) = value.as_number() else {
                return Err(format!(
                    "format \"{fmt}\" is numeric but the value is {}",
                    value.type_name()
                ));
            };
            render_number(n, &spec)
        }
    };
    Ok(format!("{prefix}{body}{suffix}"))
}

/// A value's rendering with no spec applied.
fn natural(value: &Scalar) -> String {
    match value {
        // A whole number prints without a trailing `.0`, because "237 cycles"
        // is what the specification says and "237.0 cycles" is not.
        Scalar::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        Scalar::Number(n) => format!("{n}"),
        Scalar::Text(s) => s.clone(),
        Scalar::Bool(b) => b.to_string(),
    }
}

fn render_number(n: f64, spec: &NumberSpec) -> String {
    if spec.scientific {
        return match spec.precision {
            Some(p) => format!("{:.*e}", p, n),
            None => format!("{n:e}"),
        };
    }
    let mut s = match spec.precision {
        Some(p) => format!("{:.*}", p, n),
        None => natural(&Scalar::Number(n)),
    };
    if spec.group {
        s = group_thousands(&s);
    }
    s
}

/// Insert `,` every three digits of the integer part, sign and fraction left
/// alone.
fn group_thousands(s: &str) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s),
    };
    let (int_part, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    if !int_part.chars().all(|c| c.is_ascii_digit()) {
        // Not a plain decimal (infinity, a NaN that slipped through). Leave it
        // exactly as it came rather than inventing separators inside it.
        return s.to_string();
    }

    let digits: Vec<char> = int_part.chars().collect();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(*c);
    }

    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// Split a format string into literal prefix, the one placeholder, and
/// literal suffix.
fn parse_format(fmt: &str) -> Result<(String, Placeholder, String), String> {
    let chars: Vec<char> = fmt.chars().collect();
    let mut prefix = String::new();
    let mut i = 0;

    // Prefix, honouring `{{` and `}}`.
    let mut placeholder_src: Option<String> = None;
    while i < chars.len() {
        match chars[i] {
            '{' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                prefix.push('{');
                i += 2;
            }
            '}' if i + 1 < chars.len() && chars[i + 1] == '}' => {
                prefix.push('}');
                i += 2;
            }
            '{' => {
                let start = i + 1;
                let end = chars[start..]
                    .iter()
                    .position(|c| *c == '}')
                    .map(|p| start + p)
                    .ok_or_else(|| format!("format \"{fmt}\" has an unclosed {{"))?;
                placeholder_src = Some(chars[start..end].iter().collect());
                i = end + 1;
                break;
            }
            c => {
                prefix.push(c);
                i += 1;
            }
        }
    }

    let Some(src) = placeholder_src else {
        return Err(format!(
            "format \"{fmt}\" has no {{}} placeholder, so it would render the same text for \
             every value"
        ));
    };

    // Suffix, same escaping, and no second placeholder.
    let mut suffix = String::new();
    while i < chars.len() {
        match chars[i] {
            '{' if i + 1 < chars.len() && chars[i + 1] == '{' => {
                suffix.push('{');
                i += 2;
            }
            '}' if i + 1 < chars.len() && chars[i + 1] == '}' => {
                suffix.push('}');
                i += 2;
            }
            '{' => {
                return Err(format!(
                    "format \"{fmt}\" has more than one placeholder, and a reference has one \
                     value"
                ))
            }
            c => {
                suffix.push(c);
                i += 1;
            }
        }
    }

    Ok((prefix, parse_placeholder(&src, fmt)?, suffix))
}

fn parse_placeholder(src: &str, fmt: &str) -> Result<Placeholder, String> {
    if src.is_empty() {
        return Ok(Placeholder::Plain);
    }
    let Some(spec) = src.strip_prefix(':') else {
        return Err(format!(
            "format \"{fmt}\": a placeholder is {{}} or {{:spec}}, and \"{src}\" is neither"
        ));
    };
    if spec.is_empty() {
        return Ok(Placeholder::Plain);
    }

    let mut rest = spec;
    let group = rest.starts_with(',');
    if group {
        rest = &rest[1..];
    }
    let scientific = rest.ends_with('e');
    if scientific {
        rest = &rest[..rest.len() - 1];
    }
    let precision = if let Some(digits) = rest.strip_prefix('.') {
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err(unsupported(fmt, spec));
        }
        Some(digits.parse::<usize>().map_err(|_| unsupported(fmt, spec))?)
    } else if rest.is_empty() {
        None
    } else {
        return Err(unsupported(fmt, spec));
    };

    if group && scientific {
        return Err(format!(
            "format \"{fmt}\": thousands separators and scientific notation cannot both apply"
        ));
    }

    Ok(Placeholder::Number(NumberSpec {
        precision,
        group,
        scientific,
    }))
}

fn unsupported(fmt: &str, spec: &str) -> String {
    format!(
        "format \"{fmt}\": \"{spec}\" is not a spec this engine applies. Supported: {{}}, \
         {{:.N}} for N decimals, {{:,}} and {{:,.N}} for thousands separators, {{:e}} and \
         {{:.Ne}} for scientific notation"
    )
}

// ============================================================================
// 4. The service configuration
// ============================================================================

/// What `Website/_service.toml` declares.
#[derive(Clone, Debug)]
pub struct WebsiteServiceConfig {
    /// Stable identifier the manifest is published under.
    pub namespace: String,
    /// The consumer's pinned contract version.
    pub schema_version: u32,
    /// The browser key, public by design.
    pub browser_key: KeySet,
    /// The build key, held as a CI secret.
    pub build_key: KeySet,
}

impl WebsiteServiceConfig {
    /// Every `[properties]` key the shipped service template has to contain.
    ///
    /// A key absent from the template renders in the Properties panel, accepts
    /// typing, and persists nothing, because the dynamic write back path only
    /// writes keys that are already in the service file. So this list is the
    /// contract between this module and
    /// `common/assets/service_templates/Website/_service.toml`.
    pub fn property_keys() -> Vec<String> {
        let mut keys = vec!["Namespace".to_string(), "SchemaVersion".to_string()];
        keys.extend(KeySet::property_keys(KeyKind::Browser));
        keys.extend(KeySet::property_keys(KeyKind::Build));
        keys
    }

    /// Parse a `_service.toml`.
    ///
    /// Flat keys under `[properties]` are canonical, because
    /// `service_loader::toml_to_property_value` returns `None` for a
    /// `toml::Value::Table`: a nested `[website]` section parses and is then
    /// dropped from `ServiceComponent.properties`, so the panel would render
    /// nothing and persist nothing. The nested form is still read here so a
    /// file written against the specification's illustrative shape is not
    /// silently ignored.
    pub fn parse(text: &str) -> Result<Self, BakeError> {
        let doc: toml::Value = text
            .parse()
            .map_err(|e: toml::de::Error| BakeError::Service(format!("_service.toml: {e}")))?;

        let mut props: BTreeMap<String, toml::Value> = BTreeMap::new();
        // `service_loader` rebuilds `_service.toml` through a flattened serde
        // map, so every property lands directly under [service]. That is the
        // shape the engine round-trips and therefore the one to read first.
        // [properties] and [website] follow as fallbacks for a hand-authored
        // file; reading only those meant namespace was always "" and the whole
        // bake silently produced nothing.
        if let Some(table) = doc.get("service").and_then(|v| v.as_table()) {
            for (k, v) in table {
                if v.as_table().is_some() {
                    continue;
                }
                props.insert(k.clone(), v.clone());
            }
        }
        if let Some(table) = doc.get("properties").and_then(|v| v.as_table()) {
            for (k, v) in table {
                props.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }
        // Nested compatibility: `[website]` scalars fill anything the flat
        // table left unset.
        if let Some(table) = doc.get("website").and_then(|v| v.as_table()) {
            for (k, v) in table {
                if v.as_table().is_some() {
                    continue;
                }
                props.entry(k.clone()).or_insert_with(|| v.clone());
            }
        }

        let lookup: BTreeMap<String, &toml::Value> =
            props.iter().map(|(k, v)| (squash(k), v)).collect();

        let namespace = lookup
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();

        let schema_version = lookup
            .get("schemaversion")
            .and_then(|v| v.as_integer())
            .unwrap_or(1)
            .clamp(0, u32::MAX as i64) as u32;

        Ok(Self {
            namespace,
            schema_version,
            browser_key: KeySet::from_flat_properties(KeyKind::Browser, &props),
            build_key: KeySet::from_flat_properties(KeyKind::Build, &props),
        })
    }
}

/// A namespace lands in a URL path and in an R2 object key, so it is checked
/// rather than trusted.
///
/// Lowercase letters, digits, `-` and `_`, starting with a letter or digit.
/// That rules out `..`, `/`, and anything that would need escaping in either
/// place, and it keeps the pointer key unambiguous.
fn validate_namespace(namespace: &str) -> Result<(), BakeError> {
    if namespace.is_empty() {
        return Err(BakeError::Service(
            "no Namespace set. The manifest is published under it and a site reads through \
             it, so it has to be chosen before the first publish"
                .to_string(),
        ));
    }
    let ok_first = namespace
        .chars()
        .next()
        .map(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .unwrap_or(false);
    let ok_rest = namespace
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok_first || !ok_rest {
        return Err(BakeError::Service(format!(
            "Namespace \"{namespace}\" is not usable in a URL or an object key. Use lowercase \
             letters, digits, - and _, starting with a letter or a digit"
        )));
    }
    Ok(())
}

// ============================================================================
// 5. The publish seam
// ============================================================================

/// Resolve every reference and assemble the manifest, short of the two fields
/// that only exist after the upload starts.
///
/// Returns `Ok(None)` when this Space has nothing to publish: no `Website`
/// service, or a service with no references in it. Every new Space is
/// scaffolded with the folder, so treating an empty one as a failure would
/// break every publish in the engine on the day the service shipped.
///
/// A service that DOES hold references and has no namespace is a different
/// case and does fail: the author did the work and left the one field that
/// decides where it lands unset.
///
/// Read-only by design. A bake must not mutate the scene it is describing, or
/// the manifest stops being a description of what was published.
///
/// # Where to call this
///
/// From `do_publish`, which holds the `World`, after `prepare_publish_manifests`
/// and before the upload thread is spawned. Failing here fails the publish
/// before any listing is created, which is what "a failed reference fails the
/// publish" has to mean: a listing pointing at a Universe whose numbers did not
/// resolve is worse than no listing.
///
/// `execute_publish_upload` cannot do this itself. It runs on a background
/// thread that captures only owned values, so it has no `World` and could only
/// read TOML off disk, which is exactly what specification 3.1 rules out.
pub fn bake_from_world(
    world: &World,
    space_root: &Path,
    universe_root: &Path,
) -> Result<Option<PendingManifest>, BakeError> {
    bake_from_world_at(world, space_root, universe_root, Utc::now())
}

/// [`bake_from_world`] with the clock supplied, so a caller can stamp a bake
/// with the publish's own instant.
pub fn bake_from_world_at(
    world: &World,
    space_root: &Path,
    universe_root: &Path,
    baked_at: DateTime<Utc>,
) -> Result<Option<PendingManifest>, BakeError> {
    // The active source reads through Fjall for a migrated Space and through
    // the filesystem otherwise, so this one path covers both. Falling back to
    // disk keeps a headless or test World working without the resource.
    let source: Arc<dyn SpaceSource> = world
        .get_resource::<ActiveSpaceSource>()
        .map(|s| s.0.clone())
        .unwrap_or_else(|| Arc::new(DiskSource::new(space_root)));

    let service_rel = format!("{SERVICE_FOLDER}/_service.toml");
    let Ok(service_text) = source.read_to_string(&service_rel) else {
        // No Website service. Nothing to bake, and nothing to complain about.
        return Ok(None);
    };
    let config = WebsiteServiceConfig::parse(&service_text)?;

    let references = collect_references(source.as_ref())?;
    if references.is_empty() {
        return Ok(None);
    }
    validate_namespace(&config.namespace)?;

    let data = WorldDatamodel::new(world, space_root);
    debug!(
        "website bake: {} references over {} indexed instances",
        references.len(),
        data.indexed()
    );

    let resolved = resolve_all(&references, &data, universe_root)?;
    let manifest = assemble(&config, &references, &resolved, baked_at)?;

    Ok(Some(PendingManifest { manifest }))
}

/// Read every `Reference` in the `Website` folder.
///
/// Both shapes the loader accepts: a folder holding `_instance.toml`, and a
/// flat `.toml` beside `_service.toml`. Anything that is not a `Reference` is
/// skipped, because a `Website` folder may reasonably hold a note.
fn collect_references(source: &dyn SpaceSource) -> Result<Vec<Reference>, BakeError> {
    let Ok(entries) = source.list(SERVICE_FOLDER) else {
        return Ok(Vec::new());
    };

    // Sorted so a bake is deterministic and two runs of the same Space produce
    // byte-identical output, which is what makes the publish-hash skip honest.
    let mut rels: Vec<String> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            let marker = format!("{}/_instance.toml", entry.rel_path);
            if source.exists(&marker) {
                rels.push(marker);
            }
        } else if entry.name.ends_with(".toml") && entry.name != "_service.toml" {
            rels.push(entry.rel_path.clone());
        }
    }
    rels.sort();

    let mut refs = Vec::new();
    for rel in rels {
        let Ok(text) = source.read_to_string(&rel) else {
            continue;
        };
        if let Some(r) = Reference::from_toml_str(&rel, &text)? {
            refs.push(r);
        }
    }
    Ok(refs)
}

/// Build the document from resolved values. Pure, so the shape is testable
/// without a World.
fn assemble(
    config: &WebsiteServiceConfig,
    references: &[Reference],
    resolved: &BTreeMap<String, Scalar>,
    baked_at: DateTime<Utc>,
) -> Result<WebsiteManifest, BakeError> {
    let mut values = BTreeMap::new();

    for r in references {
        let scalar = resolved.get(&r.key).ok_or_else(|| {
            BakeError::Reference(ResolveError::new(
                r.key.as_str(),
                ErrorKind::NoField,
                "resolution produced no value for this reference",
            ))
        })?;

        let display = format_scalar(scalar, r.format.as_deref()).map_err(|reason| {
            BakeError::Reference(
                ResolveError::new(r.key.as_str(), ErrorKind::BadFormat, reason)
                    .with_source(r.source.as_str()),
            )
        })?;

        values.insert(
            r.key.clone(),
            ManifestValue {
                value: json_value(scalar, &r.key)?,
                unit: r.unit.clone(),
                label: r.label.clone(),
                basis: r.basis.clone(),
                format: r.format.clone(),
                display,
                source: r.source.clone(),
                // Only a sim value has a run behind it. Emitting an empty
                // run_label on the others would invite a consumer to render a
                // provenance that does not exist.
                run_label: if r.kind == ReferenceKind::Sim {
                    r.run_label.clone()
                } else {
                    None
                },
            },
        );
    }

    Ok(WebsiteManifest {
        namespace: config.namespace.clone(),
        schema_version: config.schema_version,
        // Both stamped by the upload, which is where they first exist.
        simulation_id: String::new(),
        publish_hash: String::new(),
        baked_at: baked_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        values,
    })
}

/// Narrow a resolved value into JSON.
///
/// The one thing this refuses is a non-finite number. JSON has no form for it
/// other than `null`, and a `null` in the manifest is the stale-number failure
/// wearing a different hat.
fn json_value(scalar: &Scalar, key: &str) -> Result<serde_json::Value, BakeError> {
    match scalar {
        Scalar::Number(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                BakeError::Reference(ResolveError::new(
                    key,
                    ErrorKind::NotFinite,
                    format!("{n} has no JSON number form"),
                ))
            }),
        Scalar::Text(s) => Ok(serde_json::Value::String(s.clone())),
        Scalar::Bool(b) => Ok(serde_json::Value::Bool(*b)),
    }
}

// ============================================================================
// 6. Pending and baked
// ============================================================================

/// A resolved manifest waiting for the two fields the upload produces.
///
/// `publish_hash` is a digest of the packaged `.pak` and `simulation_id` comes
/// back from `POST /api/simulations/publish`, so neither exists while the
/// `World` is still in hand. This carries the resolved values across the thread
/// boundary and [`PendingManifest::finalize`] stamps the rest in.
#[derive(Clone, Debug)]
pub struct PendingManifest {
    manifest: WebsiteManifest,
}

impl PendingManifest {
    /// The namespace this will publish under.
    pub fn namespace(&self) -> &str {
        &self.manifest.namespace
    }

    /// The consumer contract version.
    pub fn schema_version(&self) -> u32 {
        self.manifest.schema_version
    }

    /// How many values resolved. Worth logging: a number that moves when the
    /// author did not add a reference is a sign something changed underneath.
    pub fn value_count(&self) -> usize {
        self.manifest.values.len()
    }

    /// The manifest keys, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.manifest.values.keys().map(String::as_str)
    }

    /// Stamp in the publish identity and close the document.
    pub fn finalize(mut self, simulation_id: &str, publish_hash: &str) -> BakedManifest {
        self.manifest.simulation_id = simulation_id.to_string();
        self.manifest.publish_hash = publish_hash.to_string();
        BakedManifest {
            manifest: self.manifest,
        }
    }
}

/// A finished manifest and the two objects a publish writes for it.
#[derive(Clone, Debug)]
pub struct BakedManifest {
    manifest: WebsiteManifest,
}

impl BakedManifest {
    /// The document itself.
    pub fn manifest(&self) -> &WebsiteManifest {
        &self.manifest
    }

    /// The namespace this publishes under.
    pub fn namespace(&self) -> &str {
        &self.manifest.namespace
    }

    /// The publish this belongs to.
    pub fn simulation_id(&self) -> &str {
        &self.manifest.simulation_id
    }

    /// Digest of the publish, and the `ETag` a consumer revalidates against.
    pub fn publish_hash(&self) -> &str {
        &self.manifest.publish_hash
    }

    /// R2 key for the manifest object.
    pub fn object_key(&self) -> String {
        manifest_object_key(&self.manifest.simulation_id)
    }

    /// R2 key for the namespace pointer.
    pub fn pointer_key(&self) -> String {
        namespace_pointer_key(&self.manifest.namespace)
    }

    /// The manifest body, pretty printed because it is small, published, and
    /// read by people as often as by scripts.
    pub fn manifest_json(&self) -> Result<String, BakeError> {
        serde_json::to_string_pretty(&self.manifest)
            .map_err(|e| BakeError::Encode(e.to_string()))
    }

    /// The pointer body.
    pub fn pointer(&self) -> NamespacePointer {
        NamespacePointer {
            namespace: self.manifest.namespace.clone(),
            simulation_id: self.manifest.simulation_id.clone(),
            publish_hash: self.manifest.publish_hash.clone(),
            updated_at: self.manifest.baked_at.clone(),
        }
    }

    /// The pointer body, serialized.
    pub fn pointer_json(&self) -> Result<String, BakeError> {
        serde_json::to_string_pretty(&self.pointer()).map_err(|e| BakeError::Encode(e.to_string()))
    }
}

// ============================================================================
// 7. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn at(iso: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(iso)
            .expect("test timestamp")
            .with_timezone(&Utc)
    }

    fn config(namespace: &str) -> WebsiteServiceConfig {
        WebsiteServiceConfig {
            namespace: namespace.to_string(),
            schema_version: 3,
            browser_key: KeySet::new(KeyKind::Browser),
            build_key: KeySet::new(KeyKind::Build),
        }
    }

    fn reference(key: &str, kind: ReferenceKind, source: &str) -> Reference {
        Reference {
            key: key.to_string(),
            kind,
            source: source.to_string(),
            label: key.to_string(),
            unit: None,
            format: None,
            basis: "derived".to_string(),
            run_label: None,
            at: "final".to_string(),
            cycle_key: None,
            filter: None,
            axis: None,
            origin: format!("Website/{key}/_instance.toml"),
        }
    }

    // ---- formatting -------------------------------------------------------

    #[test]
    fn no_format_renders_a_whole_number_without_a_decimal_point() {
        assert_eq!(format_scalar(&Scalar::Number(237.0), None).unwrap(), "237");
        assert_eq!(
            format_scalar(&Scalar::Number(953.25), None).unwrap(),
            "953.25"
        );
        assert_eq!(
            format_scalar(&Scalar::Text("cathode".into()), None).unwrap(),
            "cathode"
        );
        assert_eq!(format_scalar(&Scalar::Bool(true), None).unwrap(), "true");
    }

    #[test]
    fn fixed_precision_rounds_the_way_the_author_asked() {
        let v = Scalar::Number(952.6);
        assert_eq!(format_scalar(&v, Some("{:.0}")).unwrap(), "953");
        assert_eq!(format_scalar(&v, Some("{:.2}")).unwrap(), "952.60");
    }

    #[test]
    fn thousands_separators_group_only_the_integer_part() {
        assert_eq!(
            format_scalar(&Scalar::Number(2341.0), Some("{:,}")).unwrap(),
            "2,341"
        );
        assert_eq!(
            format_scalar(&Scalar::Number(1234567.891), Some("{:,.2}")).unwrap(),
            "1,234,567.89"
        );
        assert_eq!(
            format_scalar(&Scalar::Number(-2341.5), Some("{:,.1}")).unwrap(),
            "-2,341.5"
        );
        assert_eq!(
            format_scalar(&Scalar::Number(999.0), Some("{:,}")).unwrap(),
            "999"
        );
    }

    #[test]
    fn literal_text_around_the_placeholder_is_kept() {
        assert_eq!(
            format_scalar(&Scalar::Number(953.0), Some("{:.0} Wh/kg")).unwrap(),
            "953 Wh/kg"
        );
        assert_eq!(
            format_scalar(&Scalar::Number(80.0), Some("to {:.0} % retention")).unwrap(),
            "to 80 % retention"
        );
    }

    #[test]
    fn braces_can_be_escaped() {
        assert_eq!(
            format_scalar(&Scalar::Number(3.0), Some("{{{}}}")).unwrap(),
            "{3}"
        );
    }

    #[test]
    fn scientific_notation_is_available() {
        assert_eq!(
            format_scalar(&Scalar::Number(2341.5), Some("{:e}")).unwrap(),
            "2.3415e3"
        );
        assert_eq!(
            format_scalar(&Scalar::Number(2341.5), Some("{:.2e}")).unwrap(),
            "2.34e3"
        );
    }

    #[test]
    fn an_unsupported_spec_is_an_error_rather_than_a_silent_fallback() {
        for bad in ["{:>10}", "{:08.2}", "{:x}", "{:.}"] {
            let err = format_scalar(&Scalar::Number(1.0), Some(bad)).unwrap_err();
            assert!(err.contains("not a spec this engine applies"), "{bad}: {err}");
        }
    }

    #[test]
    fn a_format_with_no_placeholder_is_an_error() {
        let err = format_scalar(&Scalar::Number(1.0), Some("Wh/kg")).unwrap_err();
        assert!(err.contains("no {} placeholder"), "{err}");
    }

    #[test]
    fn a_second_placeholder_is_an_error() {
        let err = format_scalar(&Scalar::Number(1.0), Some("{} of {}")).unwrap_err();
        assert!(err.contains("more than one placeholder"), "{err}");
    }

    #[test]
    fn a_numeric_spec_on_text_is_an_error() {
        let err =
            format_scalar(&Scalar::Text("cathode".into()), Some("{:.2}")).unwrap_err();
        assert!(err.contains("numeric"), "{err}");
        // A plain placeholder still works on text.
        assert_eq!(
            format_scalar(&Scalar::Text("cathode".into()), Some("{}")).unwrap(),
            "cathode"
        );
    }

    // ---- assembly ---------------------------------------------------------

    #[test]
    fn a_manifest_carries_both_value_and_display() {
        let mut r = reference(
            "specific_energy",
            ReferenceKind::Instance,
            "Workspace/V-Cell/V1/Core/Enclosure#material.custom.wh_per_kg",
        );
        r.label = "Specific energy, pack level".into();
        r.unit = Some("Wh/kg".into());
        r.format = Some("{:.0}".into());

        let resolved: BTreeMap<String, Scalar> =
            [("specific_energy".to_string(), Scalar::Number(952.6))]
                .into_iter()
                .collect();
        let manifest = assemble(
            &config("vcell"),
            &[r],
            &resolved,
            at("2026-08-26T07:41:00Z"),
        )
        .unwrap();

        let v = &manifest.values["specific_energy"];
        assert_eq!(v.value, serde_json::json!(952.6));
        assert_eq!(v.display, "953");
        assert_eq!(v.unit.as_deref(), Some("Wh/kg"));
        assert_eq!(v.basis, "derived");
        assert_eq!(v.label, "Specific energy, pack level");
        assert_eq!(manifest.namespace, "vcell");
        assert_eq!(manifest.schema_version, 3);
        assert_eq!(manifest.baked_at, "2026-08-26T07:41:00Z");
        assert!(v.run_label.is_none());
    }

    #[test]
    fn only_a_sim_value_carries_a_run_label() {
        let mut sim = reference("cycles", ReferenceKind::Sim, "battery.capacity_retention");
        sim.run_label = Some("I_life_25C_5MPa_res0".into());
        sim.basis = "simulated".into();
        let mut inst = reference("mass", ReferenceKind::Instance, "Workspace/Pack#a.b");
        // A run_label on a non-sim reference must not travel to the manifest.
        inst.run_label = Some("stray".into());

        let resolved: BTreeMap<String, Scalar> = [
            ("cycles".to_string(), Scalar::Number(237.0)),
            ("mass".to_string(), Scalar::Number(3.526)),
        ]
        .into_iter()
        .collect();

        let manifest = assemble(
            &config("vcell"),
            &[sim, inst],
            &resolved,
            at("2026-08-26T07:41:00Z"),
        )
        .unwrap();
        assert_eq!(
            manifest.values["cycles"].run_label.as_deref(),
            Some("I_life_25C_5MPa_res0")
        );
        assert!(manifest.values["mass"].run_label.is_none());
    }

    #[test]
    fn no_key_material_appears_in_the_body() {
        let mut cfg = config("vcell");
        let browser = cfg.browser_key.generate(at("2026-07-02T09:15:00Z"));
        let build = cfg.build_key.generate(at("2026-07-02T09:15:00Z"));

        let r = reference("mass", ReferenceKind::Instance, "Workspace/Pack#a.b");
        let resolved: BTreeMap<String, Scalar> =
            [("mass".to_string(), Scalar::Number(3.526))]
                .into_iter()
                .collect();
        let baked = PendingManifest {
            manifest: assemble(&cfg, &[r], &resolved, at("2026-08-26T07:41:00Z")).unwrap(),
        }
        .finalize("8f3a1c04-0000-0000-0000-000000000000", "blake3:1c9fa83");

        let body = baked.manifest_json().unwrap();
        for forbidden in [
            browser.secret.as_str(),
            build.secret.as_str(),
            browser.record.hash.as_str(),
            build.record.hash.as_str(),
            "eus_pk_",
            "eus_bk_",
        ] {
            assert!(
                !body.contains(forbidden),
                "key material leaked into the manifest: {forbidden}"
            );
        }
    }

    #[test]
    fn finalize_stamps_the_publish_and_derives_both_object_keys() {
        let r = reference("mass", ReferenceKind::Instance, "Workspace/Pack#a.b");
        let resolved: BTreeMap<String, Scalar> =
            [("mass".to_string(), Scalar::Number(1.0))]
                .into_iter()
                .collect();
        let pending = PendingManifest {
            manifest: assemble(
                &config("vcell"),
                &[r],
                &resolved,
                at("2026-08-26T07:41:00Z"),
            )
            .unwrap(),
        };
        assert_eq!(pending.value_count(), 1);
        assert_eq!(pending.namespace(), "vcell");

        let baked = pending.finalize("8f3a1c04-1111-2222-3333-444444444444", "blake3:abc");
        assert_eq!(
            baked.object_key(),
            "universes/8f3a1c04-1111-2222-3333-444444444444/website-manifest.json"
        );
        assert_eq!(baked.pointer_key(), "universes/_namespaces/vcell.json");

        let pointer: NamespacePointer =
            serde_json::from_str(&baked.pointer_json().unwrap()).unwrap();
        assert_eq!(pointer.namespace, "vcell");
        assert_eq!(pointer.publish_hash, "blake3:abc");
        assert_eq!(pointer.updated_at, "2026-08-26T07:41:00Z");
    }

    #[test]
    fn the_manifest_round_trips_through_json() {
        let mut r = reference("cycles", ReferenceKind::Sim, "battery.capacity_retention");
        r.run_label = Some("I_life_25C_5MPa_res0".into());
        r.basis = "simulated".into();
        r.unit = Some("cycles".into());
        let resolved: BTreeMap<String, Scalar> =
            [("cycles".to_string(), Scalar::Number(237.0))]
                .into_iter()
                .collect();
        let baked = PendingManifest {
            manifest: assemble(
                &config("vcell"),
                &[r],
                &resolved,
                at("2026-08-26T07:41:00Z"),
            )
            .unwrap(),
        }
        .finalize("id", "blake3:abc");

        let text = baked.manifest_json().unwrap();
        let back: WebsiteManifest = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, baked.manifest());
        // The consumer contract: read `display`, compute with `value`.
        assert_eq!(back.values["cycles"].display, "237");
        assert_eq!(back.values["cycles"].value.as_f64(), Some(237.0));
    }

    #[test]
    fn a_bad_format_fails_the_bake_and_names_the_reference() {
        let mut r = reference("mass", ReferenceKind::Instance, "Workspace/Pack#a.b");
        r.format = Some("{:>8}".into());
        let resolved: BTreeMap<String, Scalar> =
            [("mass".to_string(), Scalar::Number(1.0))]
                .into_iter()
                .collect();
        let err = assemble(
            &config("vcell"),
            &[r],
            &resolved,
            at("2026-08-26T07:41:00Z"),
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("mass:"), "{err}");
    }

    // ---- service configuration -------------------------------------------

    #[test]
    fn flat_properties_are_the_canonical_service_shape() {
        let text = r#"
[service]
class_name = "WebsiteService"
icon = "website"

[properties]
Namespace = "vcell"
SchemaVersion = 3
"#;
        let cfg = WebsiteServiceConfig::parse(text).unwrap();
        assert_eq!(cfg.namespace, "vcell");
        assert_eq!(cfg.schema_version, 3);
        assert!(!cfg.browser_key.is_issued());
    }

    #[test]
    fn the_specifications_nested_shape_still_reads() {
        let text = r#"
[service]
class_name = "WebsiteService"

[website]
namespace = "vcell"
schema_version = 3
"#;
        let cfg = WebsiteServiceConfig::parse(text).unwrap();
        assert_eq!(cfg.namespace, "vcell");
        assert_eq!(cfg.schema_version, 3);
    }

    #[test]
    fn a_key_hash_in_the_service_file_is_read_back() {
        let mut set = KeySet::new(KeyKind::Browser);
        let minted = set.generate(at("2026-07-02T09:15:00Z"));
        let mut props = String::from(
            "[service]\nclass_name = \"WebsiteService\"\n\n[properties]\nNamespace = \"vcell\"\n",
        );
        for (k, v) in set.to_flat_properties() {
            // Rendered by hand rather than through Display, so the test does
            // not depend on the toml crate's optional `display` feature.
            let rendered = match v {
                toml::Value::String(s) => format!("\"{s}\""),
                toml::Value::Integer(i) => i.to_string(),
                other => panic!("unexpected property type {}", other.type_str()),
            };
            props.push_str(&format!("{k} = {rendered}\n"));
        }
        let cfg = WebsiteServiceConfig::parse(&props).unwrap();
        assert!(cfg.browser_key.is_issued());
        assert_eq!(
            cfg.browser_key
                .verify(&minted.secret, at("2026-07-02T09:15:00Z")),
            super::super::key::KeyVerdict::Current
        );
    }

    #[test]
    fn namespaces_that_would_break_a_url_or_an_object_key_are_rejected() {
        for bad in ["", "V-Cell", "../evil", "with space", "-leading"] {
            assert!(
                validate_namespace(bad).is_err(),
                "{bad} should not be accepted"
            );
        }
        for good in ["vcell", "v-cell", "v_cell_2", "3d"] {
            assert!(validate_namespace(good).is_ok(), "{good} should be accepted");
        }
    }

    #[test]
    fn the_property_key_list_covers_the_whole_service_template() {
        let keys = WebsiteServiceConfig::property_keys();
        for expected in ["Namespace", "SchemaVersion", "WebsiteKeyHash", "BuildKeyHash"] {
            assert!(keys.iter().any(|k| k == expected), "{expected} missing");
        }
    }

    #[test]
    fn a_publish_hash_names_its_algorithm() {
        let h = blake3_publish_hash(b"pak bytes");
        assert!(h.starts_with("blake3:"));
        assert_eq!(h.len(), "blake3:".len() + 64);
    }
}
