//! A Space's format version: one number, `[space] format` in `space.toml`,
//! naming the one-time migrations the Space has had. A Space without it is
//! format 1.
//!
//! Studio moves a Space to [`CURRENT`] when it opens it, rewriting its files,
//! its database and its baked cores together and writing the number last. A
//! reader that never writes (the Player, an export of a Space nobody opened)
//! applies the same rules at read time to a Space whose format is older
//! ([`migrated_records`]), so every reader sees one world.
//!
//! - 2 ([`WEDGE_MESHES`]): native wedges and corner wedges turned to fit the
//!   Roblox-correct meshes (`part_meshes::migrate_native_wedge`), the poses
//!   given relative to them turned back, and their faces remapped.
//! - 3 ([`SCALE_KINDS`]): every custom-mesh part states what its
//!   `transform.scale` is (`[asset] scale_kind`, see `scale_kind`): a length
//!   for a Roblox import, whose meshes are unit-sized, a multiplier for the
//!   rest.

/// The format a Space opened by this build ends up in, and the one a new
/// Space starts in.
pub const CURRENT: u32 = 3;

/// The format from which native wedges are drawn by the Roblox-correct
/// meshes. An older Space's native wedges are turned to fit them.
pub const WEDGE_MESHES: u32 = 2;

/// The format from which every custom-mesh part states its scale's kind. An
/// older Space's custom meshes are stamped once (`scale_kind::plan`).
pub const SCALE_KINDS: u32 = 3;

/// A Space's format, from its parsed `space.toml`. No file, no `[space]`
/// table, no `format` key, or a value that is not a whole number of at least
/// 1, all read as format 1. Table and key names match in any case, as the
/// other `space.toml` readers match them.
pub fn of_space(space_toml: Option<&toml::Value>) -> u32 {
    let field = |table: &toml::Value, name: &str| -> Option<toml::Value> {
        table
            .as_table()?
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    space_toml
        .and_then(|doc| field(doc, "space"))
        .and_then(|space| field(&space, "format"))
        .and_then(|v| v.as_integer())
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1)
}

/// [`of_space`] from `space.toml`'s text, `None` when there is no file.
pub fn of_space_text(space_toml: Option<&str>) -> u32 {
    of_space(space_toml.and_then(|text| text.parse::<toml::Value>().ok()).as_ref())
}

/// Whether a Space of this format still has its native wedges turned for the
/// old meshes.
pub fn needs_wedge_migration(format: u32) -> bool {
    format < WEDGE_MESHES
}

/// Whether a Space of this format still has custom meshes that do not state
/// their scale's kind.
pub fn needs_scale_kind_migration(format: u32) -> bool {
    format < SCALE_KINDS
}

/// `space.toml`'s text naming `format`, every other key kept. `None` (no
/// file, or one that does not parse) gives a file holding only the format.
/// The `[space]` table and the `format` key are matched in any case and
/// written as `space` and `format` when absent.
pub fn with_format(space_toml: Option<&str>, format: u32) -> String {
    let mut doc: toml::Value = space_toml
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| toml::Value::Table(toml::value::Table::new()));
    if let Some(root) = doc.as_table_mut() {
        let key = root
            .keys()
            .find(|k| k.eq_ignore_ascii_case("space"))
            .cloned()
            .unwrap_or_else(|| "space".to_string());
        let space = root.entry(key).or_insert_with(|| toml::Value::Table(toml::value::Table::new()));
        if let Some(space) = space.as_table_mut() {
            space.retain(|k, _| !k.eq_ignore_ascii_case("format"));
            space.insert("format".to_string(), toml::Value::Integer(i64::from(format)));
        }
    }
    toml::to_string_pretty(&doc).unwrap_or_else(|_| format!("[space]\nformat = {format}\n"))
}

/// Whether a record key names the Space's `space.toml`.
fn is_space_toml(key: &str) -> bool {
    key.replace('\\', "/").trim_start_matches("./") == "space.toml"
}

/// `records` (a Space's files by their path from the Space's folder) as a
/// Space at [`CURRENT`] reads, in memory: each move its format still needs,
/// in order (the wedges, then the scale kinds), each one planned on the
/// last one's output, then the number [`CURRENT`]. Studio's runner makes the
/// same moves on disk. `None` when nothing changes: read `records` as they
/// are. The second value is every move's problems.
pub fn migrated_records(records: &[(String, Vec<u8>)]) -> Option<(Vec<(String, Vec<u8>)>, Vec<String>)> {
    let space_toml = records.iter().find(|(key, _)| is_space_toml(key)).and_then(|(_, bytes)| std::str::from_utf8(bytes).ok());
    let format = of_space_text(space_toml);
    if format >= CURRENT {
        return None;
    }
    let mut current: Vec<(String, Vec<u8>)> = records.to_vec();
    let mut problems = Vec::new();
    let mut changed = false;
    let mut apply = |current: &mut Vec<(String, Vec<u8>)>, rewrites: Vec<(String, String)>| {
        let by_key: std::collections::HashMap<String, String> = rewrites.into_iter().collect();
        for (key, bytes) in current.iter_mut() {
            if let Some(text) = by_key.get(key.as_str()) {
                *bytes = text.clone().into_bytes();
                changed = true;
            }
        }
    };
    if needs_wedge_migration(format) {
        let plan = crate::wedge_migration::plan(&current);
        problems.extend(plan.problems.iter().cloned());
        apply(&mut current, plan.rewrites.into_iter().map(|r| (r.key, r.text)).collect());
    }
    if needs_scale_kind_migration(format) {
        let plan = crate::scale_kind::plan(&current);
        problems.extend(plan.problems.iter().cloned());
        apply(&mut current, plan.rewrites);
    }
    if !changed {
        return None;
    }
    match current.iter_mut().find(|(key, _)| is_space_toml(key)) {
        Some((_, bytes)) => *bytes = with_format(std::str::from_utf8(bytes).ok(), CURRENT).into_bytes(),
        None => current.push(("space.toml".to_string(), with_format(None, CURRENT).into_bytes())),
    }
    Some((current, problems))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> toml::Value {
        text.parse().unwrap()
    }

    #[test]
    fn a_space_without_the_number_is_format_one() {
        assert_eq!(of_space(None), 1);
        assert_eq!(of_space(Some(&parse("[space]\nname = \"Garage\"\n"))), 1);
        assert_eq!(of_space(Some(&parse("name = \"Garage\"\n"))), 1);
        assert_eq!(of_space(Some(&parse("[space]\nformat = 0\n"))), 1, "below 1 reads as 1");
        assert_eq!(of_space(Some(&parse("[space]\nformat = -3\n"))), 1);
        assert_eq!(of_space(Some(&parse("[space]\nformat = \"2\"\n"))), 1, "only a whole number counts");
        assert_eq!(of_space_text(Some("not toml [")), 1);
    }

    #[test]
    fn the_number_is_read_in_any_case() {
        assert_eq!(of_space(Some(&parse("[space]\nformat = 2\n"))), 2);
        assert_eq!(of_space(Some(&parse("[Space]\nFormat = 2\n"))), 2);
        assert_eq!(of_space_text(Some("[space]\nformat = 7\n")), 7);
    }

    #[test]
    fn wedges_migrate_below_format_two() {
        assert!(needs_wedge_migration(1));
        assert!(!needs_wedge_migration(WEDGE_MESHES));
        assert!(!needs_wedge_migration(CURRENT));
    }

    #[test]
    fn scale_kinds_migrate_below_format_three() {
        assert!(needs_scale_kind_migration(1));
        assert!(needs_scale_kind_migration(WEDGE_MESHES));
        assert!(!needs_scale_kind_migration(SCALE_KINDS));
        assert!(!needs_scale_kind_migration(CURRENT));
    }

    /// The number goes in, an old one goes, and every other key stays.
    #[test]
    fn writing_the_number_keeps_the_rest() {
        let text = with_format(
            Some("[space]\nname = \"Garage\"\ntransform_rule = \"parent_pose\"\nFORMAT = 1\n\n[other]\nkept = true\n"),
            2,
        );
        let doc = parse(&text);
        assert_eq!(of_space(Some(&doc)), 2);
        assert_eq!(doc["space"]["name"].as_str(), Some("Garage"));
        assert_eq!(doc["space"]["transform_rule"].as_str(), Some("parent_pose"));
        assert!(doc["space"].get("FORMAT").is_none(), "the old spelling goes");
        assert_eq!(doc["other"]["kept"].as_bool(), Some(true));

        let capital = parse(&with_format(Some("[Space]\nname = \"G\"\n"), 2));
        assert_eq!(capital["Space"]["format"].as_integer(), Some(2), "the file's own table name is kept");

        assert_eq!(of_space_text(Some(&with_format(None, 2))), 2);
        assert_eq!(of_space_text(Some(&with_format(Some("not toml ["), 2))), 2);
    }

    fn rec(key: &str, text: &str) -> (String, Vec<u8>) {
        (key.to_string(), text.as_bytes().to_vec())
    }

    /// A format-1 Space read in memory: its native wedge turned and its
    /// imported custom mesh stamped a length, both in the one pass, and the
    /// number set to the current format; a current Space is read as it is.
    #[test]
    fn an_older_space_is_read_at_the_current_format() {
        let records = vec![
            rec("space.toml", "[space]\nname = \"Drive\"\n"),
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec(
                "Workspace/Roof/_instance.toml",
                "[metadata]\nclass_name = \"Part\"\n\n[asset]\nmesh = \"parts/wedge.glb\"\n\n[transform]\nposition = [0.0, 1.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 1.0, 4.0]\n",
            ),
            rec(
                "Workspace/HotDog/_instance.toml",
                "[metadata]\nclass_name = \"Part\"\nunit = \"stud\"\nroblox_color_srgb = [1.0, 0.5, 0.2]\n\n[asset]\nmesh = \"assets/meshes/rbx-1.glb\"\n\n[transform]\nscale = [1.0, 0.5, 0.5]\n",
            ),
        ];
        let (migrated, problems) = migrated_records(&records).expect("a format-1 Space moves");
        assert!(problems.is_empty(), "{problems:?}");
        let get = |key: &str| -> toml::Value {
            std::str::from_utf8(&migrated.iter().find(|(k, _)| k == key).unwrap().1).unwrap().parse().unwrap()
        };
        assert_eq!(of_space(Some(&get("space.toml"))), CURRENT);
        assert_eq!(get("space.toml")["space"]["name"].as_str(), Some("Drive"));
        let roof = get("Workspace/Roof/_instance.toml");
        let q = roof["transform"]["rotation"].as_array().unwrap();
        assert!((q[1].as_float().unwrap().abs() - 1.0).abs() < 1e-5, "the wedge took its half turn: {q:?}");
        assert_eq!(get("Workspace/HotDog/_instance.toml")["asset"]["scale_kind"].as_str(), Some("length"));
        assert!(migrated_records(&migrated).is_none(), "the current format is read as it is");
    }
}
