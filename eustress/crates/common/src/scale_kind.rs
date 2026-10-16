//! What a mesh part's `transform.scale` means: a length or a ratio.
//!
//! A primitive (`parts/block.glb` and friends) is a unit mesh, so its scale
//! is its size, a length in the file's `[metadata] unit`. A custom mesh is
//! one of two kinds, and the part says which in `[asset] scale_kind`:
//!
//! - `"length"`: the mesh is unit-sized and drawn at the part's size, so the
//!   scale is a length and converts through the unit like a position. Roblox
//!   imports write their meshes this way (a MeshPart's Size).
//! - `"multiplier"`: the scale multiplies the mesh's own size, a ratio that
//!   never converts. Meshes made or placed in Studio are this kind, and a
//!   custom mesh with no key reads as one.
//!
//! Studio's loader and save and every reader decide by [`is_length_of`] (or
//! [`is_length`] on a record's `[asset]` table), so a part draws at one size
//! everywhere. A Space older than format 3
//! ([`crate::space_format::SCALE_KINDS`]) has custom meshes with no key, some
//! of them imports whose scale is a length: [`plan`] stamps each one once
//! (Studio writes it when it opens the Space; a reader applies it in memory
//! through [`migrated_records`]), so no reader ever guesses at run time.

use crate::space_read::primitive_shape;

/// The `[asset]` key.
pub const KEY: &str = "scale_kind";
/// The scale is a length: a unit-sized mesh drawn at the part's size.
pub const LENGTH: &str = "length";
/// The scale multiplies the mesh's own size.
pub const MULTIPLIER: &str = "multiplier";

/// Whether a part's `transform.scale` is a length, from its `[asset] mesh`
/// and `scale_kind`: no mesh or a primitive is (its scale is its size), a
/// custom mesh only when `scale_kind` is `"length"` (in any case). Anything
/// else on a custom mesh (no key, `"multiplier"`, an unknown value) is a
/// ratio.
pub fn is_length_of(mesh: Option<&str>, scale_kind: Option<&str>) -> bool {
    match mesh.map(str::trim).filter(|mesh| !mesh.is_empty()) {
        None => true,
        Some(mesh) if primitive_shape(mesh).is_some() => true,
        Some(_) => scale_kind.is_some_and(|kind| kind.trim().eq_ignore_ascii_case(LENGTH)),
    }
}

/// [`is_length_of`] from a record's `[asset]` table (`None`: the record has
/// none), keys in any case.
pub fn is_length(asset: Option<&toml::Value>) -> bool {
    let field = |name: &str| -> Option<&str> {
        asset?.as_table()?.iter().find(|(key, _)| key.eq_ignore_ascii_case(name))?.1.as_str()
    };
    is_length_of(field("mesh"), field(KEY))
}

/// What [`plan`] found.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScaleKindPlan {
    /// Each record to rewrite, with its whole new text, in key order.
    pub rewrites: Vec<(String, String)>,
    /// Custom meshes stamped `"length"` (imports).
    pub lengths: usize,
    /// Custom meshes stamped `"multiplier"`.
    pub multipliers: usize,
    /// Records left as they are, and why.
    pub problems: Vec<String>,
}

/// The rewrites that move `records` (a Space's files by their path from the
/// Space's folder) to format 3: every record whose custom `[asset] mesh` has
/// no `scale_kind` gets one, `"length"` for a Roblox import
/// (`part_meshes::is_roblox_import`: such an import's meshes are unit-sized
/// and its scale is the part's size) and `"multiplier"` for anything else.
/// A record that already names a kind, a primitive, and a record with no
/// mesh are left alone. The key goes in on its own line under the record's
/// `[asset]` header, so the rest of the file is kept byte for byte.
pub fn plan(records: &[(String, Vec<u8>)]) -> ScaleKindPlan {
    let mut out = ScaleKindPlan::default();
    for (key, bytes) in records {
        if !key.ends_with(".toml") || !bytes.windows(4).any(|w| w.eq_ignore_ascii_case(b"mesh")) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(bytes) else { continue };
        let Ok(doc) = text.parse::<toml::Value>() else { continue };
        let Some(kind) = kind_for(&doc) else { continue };
        match stamped(text, &doc, kind) {
            Some(new) => {
                out.rewrites.push((key.clone(), new));
                if kind == LENGTH {
                    out.lengths += 1;
                } else {
                    out.multipliers += 1;
                }
            }
            None => out.problems.push(format!("{key}: its [asset] could not be stamped, left as it is")),
        }
    }
    out.rewrites.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The kind [`plan`] stamps on one record, from its parsed text:
/// `Some(LENGTH)` for a Roblox import's custom mesh with no `scale_kind`,
/// `Some(MULTIPLIER)` for any other custom mesh with none, and `None` when
/// the plan leaves the record alone (it states a kind, its mesh is a
/// primitive, or it has none). For a reader that parses each file on its own;
/// [`plan`] decides by this too.
pub fn kind_for(doc: &toml::Value) -> Option<&'static str> {
    let asset = section(doc, "asset")?;
    let mesh = get_ci(asset, "mesh").and_then(|m| m.as_str()).map(str::trim).filter(|m| !m.is_empty())?;
    if primitive_shape(mesh).is_some() || get_ci(asset, KEY).is_some() {
        return None;
    }
    let imported = crate::part_meshes::is_roblox_import(|name| {
        section(doc, "metadata").and_then(|m| m.as_table()).is_some_and(|m| m.contains_key(name))
    });
    Some(if imported { LENGTH } else { MULTIPLIER })
}

/// `records` as a Space at format 3 reads: a Space whose `space.toml` names
/// an older format, with [`plan`]'s rewrites and the number 3, in memory.
/// `None` when nothing changes. The second value is the plan's problems.
pub fn migrated_records(records: &[(String, Vec<u8>)]) -> Option<(Vec<(String, Vec<u8>)>, Vec<String>)> {
    let space_toml = records
        .iter()
        .find(|(key, _)| key.trim_start_matches("./").replace('\\', "/") == "space.toml")
        .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok());
    let format = crate::space_format::of_space_text(space_toml);
    if !crate::space_format::needs_scale_kind_migration(format) {
        return None;
    }
    let plan = plan(records);
    let by_key: std::collections::HashMap<&str, &str> = plan.rewrites.iter().map(|(k, t)| (k.as_str(), t.as_str())).collect();
    let mut named = false;
    let mut out: Vec<(String, Vec<u8>)> = records
        .iter()
        .map(|(key, bytes)| {
            if key.trim_start_matches("./").replace('\\', "/") == "space.toml" {
                named = true;
                let text = crate::space_format::with_format(std::str::from_utf8(bytes).ok(), crate::space_format::SCALE_KINDS);
                return (key.clone(), text.into_bytes());
            }
            match by_key.get(key.as_str()) {
                Some(text) => (key.clone(), text.as_bytes().to_vec()),
                None => (key.clone(), bytes.clone()),
            }
        })
        .collect();
    if !named {
        out.push(("space.toml".to_string(), crate::space_format::with_format(None, crate::space_format::SCALE_KINDS).into_bytes()));
    }
    Some((out, plan.problems))
}

/// `text` with `scale_kind = "<kind>"` added to its `[asset]` table: on its
/// own line under the `[asset]` header when the file has one, else (an inline
/// or dotted table) the whole document written back with the key.
fn stamped(text: &str, doc: &toml::Value, kind: &str) -> Option<String> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let header = line.trim().trim_end_matches(|c: char| c == '#' || c.is_whitespace());
        let header = header.split('#').next().unwrap_or("").trim();
        if header.len() >= 2 && header.starts_with('[') && header.ends_with(']') && !header.starts_with("[[") {
            if header[1..header.len() - 1].trim().eq_ignore_ascii_case("asset") {
                let at = offset + line.len();
                let newline = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
                let prefix = if line.ends_with('\n') { "" } else { newline };
                return Some(format!("{}{prefix}{KEY} = \"{kind}\"{newline}{}", &text[..at], &text[at..]));
            }
        }
        offset += line.len();
    }
    let mut doc = doc.clone();
    let table = doc.as_table_mut()?;
    let name = table.keys().find(|k| k.eq_ignore_ascii_case("asset"))?.clone();
    table.get_mut(&name)?.as_table_mut()?.insert(KEY.to_string(), toml::Value::String(kind.to_string()));
    toml::to_string_pretty(&doc).ok()
}

fn section<'v>(doc: &'v toml::Value, name: &str) -> Option<&'v toml::Value> {
    get_ci(doc, name).filter(|v| v.is_table())
}

fn get_ci<'v>(value: &'v toml::Value, name: &str) -> Option<&'v toml::Value> {
    value.as_table()?.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(key: &str, text: &str) -> (String, Vec<u8>) {
        (key.to_string(), text.as_bytes().to_vec())
    }

    fn asset(text: &str) -> toml::Value {
        text.parse::<toml::Value>().unwrap()["asset"].clone()
    }

    /// Every row of the rule.
    #[test]
    fn the_scale_is_a_length_for_primitives_and_length_meshes_only() {
        assert!(is_length_of(None, None), "no mesh");
        assert!(is_length_of(Some(""), Some("multiplier")), "an empty mesh is no mesh");
        assert!(is_length_of(Some("parts/block.glb"), Some("multiplier")), "a primitive, the key ignored");
        assert!(is_length_of(Some("assets/meshes/rbx-1.glb"), Some("length")));
        assert!(is_length_of(Some("assets/meshes/rbx-1.glb"), Some(" LENGTH ")));
        assert!(!is_length_of(Some("../meshes/VCell_Housing.glb"), None), "a custom mesh with no key");
        assert!(!is_length_of(Some("meshes/x.glb"), Some("multiplier")));
        assert!(!is_length_of(Some("meshes/x.glb"), Some("lengthy")), "an unknown value is a ratio");
        assert!(is_length(None));
        assert!(is_length(Some(&asset("[asset]\nMesh = \"meshes/x.glb\"\nScale_Kind = \"Length\"\n"))), "keys in any case");
        assert!(!is_length(Some(&asset("[asset]\nmesh = \"meshes/x.glb\"\n"))));
    }

    fn space() -> Vec<(String, Vec<u8>)> {
        vec![
            rec("space.toml", "[space]\nname = \"Drive\"\nformat = 2\n"),
            rec(
                "Workspace/HotDog/Handle/_instance.toml",
                "# imported\n[metadata]\nclass_name = \"Part\"\nunit = \"stud\"\nroblox_color_srgb = [1.0, 0.5, 0.2]\n\n[asset]\nmesh = \"../../../assets/meshes/rbx-29896287.glb\"\n\n[transform]\nscale = [1.0, 0.5, 0.5]\n",
            ),
            rec(
                "Workspace/VCell/Housing/_instance.toml",
                "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"../meshes/VCell_Housing.glb\"\nscene = \"Scene0\"\n",
            ),
            rec("Workspace/Floor/_instance.toml", "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"parts/block.glb\"\n"),
            rec(
                "Workspace/Stated/_instance.toml",
                "[metadata]\nclass_name = \"Part\"\nroblox_brick_color = 194\n\n[asset]\nmesh = \"meshes/x.glb\"\nscale_kind = \"multiplier\"\n",
            ),
            rec("Workspace/Folder/_instance.toml", "[metadata]\nclass_name = \"Folder\"\n"),
        ]
    }

    /// An import is stamped "length", a native mesh "multiplier"; a
    /// primitive, a stated kind and a record with no mesh are left alone;
    /// the rest of each file is kept byte for byte.
    #[test]
    fn the_plan_stamps_imports_length_and_native_meshes_multiplier() {
        let records = space();
        let plan = plan(&records);
        assert_eq!((plan.lengths, plan.multipliers), (1, 1));
        assert!(plan.problems.is_empty(), "{:?}", plan.problems);
        let keys: Vec<&str> = plan.rewrites.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["Workspace/HotDog/Handle/_instance.toml", "Workspace/VCell/Housing/_instance.toml"]);

        let hotdog = &plan.rewrites[0].1;
        assert_eq!(
            hotdog,
            "# imported\n[metadata]\nclass_name = \"Part\"\nunit = \"stud\"\nroblox_color_srgb = [1.0, 0.5, 0.2]\n\n[asset]\nscale_kind = \"length\"\nmesh = \"../../../assets/meshes/rbx-29896287.glb\"\n\n[transform]\nscale = [1.0, 0.5, 0.5]\n"
        );
        let doc: toml::Value = hotdog.parse().unwrap();
        assert!(is_length(doc.get("asset")), "the imported mesh's scale is a length");
        let vcell: toml::Value = plan.rewrites[1].1.parse().unwrap();
        assert_eq!(vcell["asset"]["scale_kind"].as_str(), Some("multiplier"));
        assert_eq!(vcell["asset"]["scene"].as_str(), Some("Scene0"));
    }

    /// One record's planned kind, as a per-file reader asks it: the plan's
    /// own decision.
    #[test]
    fn kind_for_is_the_plans_decision_for_one_record() {
        let doc = |text: &str| text.parse::<toml::Value>().unwrap();
        let import = "[metadata]\nroblox_brick_color = 194\n[asset]\nmesh = \"assets/meshes/rbx.glb\"\n";
        assert_eq!(kind_for(&doc(import)), Some(LENGTH));
        assert_eq!(kind_for(&doc("[asset]\nmesh = \"../meshes/a.glb\"\n")), Some(MULTIPLIER));
        assert_eq!(kind_for(&doc("[asset]\nmesh = \"parts/ball.glb\"\n")), None, "a primitive");
        assert_eq!(kind_for(&doc("[asset]\nmesh = \"meshes/a.glb\"\nscale_kind = \"length\"\n")), None, "a stated kind");
        assert_eq!(kind_for(&doc("[metadata]\nclass_name = \"Folder\"\n")), None, "no mesh");
        for (key, text) in space() {
            let planned = plan(&[(key.clone(), text.clone())]);
            let decided = kind_for(&std::str::from_utf8(&text).unwrap().parse::<toml::Value>().unwrap_or(toml::Value::Table(Default::default())));
            assert_eq!(planned.rewrites.is_empty(), decided.is_none(), "{key}");
        }
    }

    /// A CRLF file gets a CRLF line, an inline [asset] table is written back
    /// whole, and a second plan finds nothing to do.
    #[test]
    fn the_stamp_keeps_line_endings_and_is_idempotent() {
        let crlf = vec![rec("Workspace/A/_instance.toml", "[metadata]\r\nclass_name = \"Part\"\r\n[asset]\r\nmesh = \"meshes/a.glb\"\r\n")];
        let plan1 = plan(&crlf);
        assert_eq!(plan1.rewrites[0].1, "[metadata]\r\nclass_name = \"Part\"\r\n[asset]\r\nscale_kind = \"multiplier\"\r\nmesh = \"meshes/a.glb\"\r\n");
        let again = vec![rec("Workspace/A/_instance.toml", &plan1.rewrites[0].1)];
        assert!(plan(&again).rewrites.is_empty(), "a stamped record is not stamped twice");

        let inline = vec![rec("Workspace/B/_instance.toml", "asset = { mesh = \"meshes/b.glb\" }\n")];
        let plan2 = plan(&inline);
        let doc: toml::Value = plan2.rewrites[0].1.parse().unwrap();
        assert_eq!(doc["asset"]["scale_kind"].as_str(), Some("multiplier"));
        assert_eq!(doc["asset"]["mesh"].as_str(), Some("meshes/b.glb"));
    }

    /// A reader of a format-2 Space sees the stamped records and format 3; a
    /// format-3 Space is read as it is.
    #[test]
    fn a_reader_migrates_an_older_space_in_memory_once() {
        let records = space();
        let (migrated, problems) = migrated_records(&records).expect("a format-2 Space moves");
        assert!(problems.is_empty());
        let get = |key: &str| -> toml::Value {
            std::str::from_utf8(&migrated.iter().find(|(k, _)| k == key).unwrap().1).unwrap().parse().unwrap()
        };
        assert_eq!(crate::space_format::of_space(Some(&get("space.toml"))), crate::space_format::SCALE_KINDS);
        assert!(is_length(get("Workspace/HotDog/Handle/_instance.toml").get("asset")));
        assert!(!is_length(get("Workspace/VCell/Housing/_instance.toml").get("asset")));
        assert!(migrated_records(&migrated).is_none(), "format 3 is read as it is");
    }
}
