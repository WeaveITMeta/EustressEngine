//! The 3D deal room: a scored pipeline you can walk through.
//!
//! A ranked list in a table is a ranked list. The same ranking laid out in
//! space is a room where urgency is a direction, fit is a height, and the
//! bid/watch/no-bid decision is a lane you physically stand in. That is the
//! difference this module exists to make.
//!
//! # Encoding
//!
//! | Axis | Meaning |
//! |---|---|
//! | X | days to deadline, near origin = closing soonest |
//! | Y | fit score, taller = better fit |
//! | Z | lane: Bid at the front, then Watch, then No-bid |
//! | scale | award ceiling, bigger block = bigger contract |
//! | color | the recommendation, so a lane reads at a glance |
//!
//! # Mechanism
//!
//! Nothing here spawns an entity directly. It writes `_instance.toml` files,
//! and the Space file watcher turns them into live objects, which is the same
//! path the Data menu's Connector and Dataset writes already take. That keeps
//! one creation path instead of two, and it means the deal room is inspectable
//! on disk before it is ever rendered.

use std::collections::BTreeMap;
use std::path::Path;

use super::model::Opportunity;
use super::score::{FitScore, Recommendation};
use super::{Assessment, CaptureError};

/// Directory under the Space that holds the deal room.
pub const DEAL_ROOM_DIR: &str = "Workspace/DealRoom";

/// Metres per day along X. Thirty days of runway reads as 30 m of walk, which
/// is far enough to feel and close enough to see the far end.
const METRES_PER_DAY: f32 = 1.0;
/// Days beyond which everything piles up at the far end rather than scattering
/// into the distance.
const MAX_DAYS: f32 = 120.0;
/// Metres of height at a perfect fit score.
const MAX_HEIGHT: f32 = 12.0;
/// Lane spacing along Z.
const LANE_SPACING: f32 = 18.0;
/// Smallest and largest footprint, in metres.
const MIN_FOOTPRINT: f32 = 1.2;
const MAX_FOOTPRINT: f32 = 5.0;
/// A contract at or above this reads at the maximum footprint.
const FOOTPRINT_CEILING: i64 = 5_000_000;

/// Which lane a node stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Bid,
    Watch,
    NoBid,
}

impl Lane {
    fn of(rec: &Recommendation) -> Self {
        match rec {
            Recommendation::Bid => Self::Bid,
            Recommendation::Watch { .. } => Self::Watch,
            Recommendation::NoBid { .. } => Self::NoBid,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bid => "Bid",
            Self::Watch => "Watch",
            Self::NoBid => "No-bid",
        }
    }

    fn z(self) -> f32 {
        match self {
            Self::Bid => 0.0,
            Self::Watch => -LANE_SPACING,
            Self::NoBid => -LANE_SPACING * 2.0,
        }
    }

    /// Lane colour, 0-255 RGB. Cyan is the editor's selection colour and is
    /// deliberately avoided here, so a selected node still reads as selected.
    fn color(self) -> [u8; 3] {
        match self {
            Self::Bid => [74, 222, 128],
            Self::Watch => [251, 191, 36],
            Self::NoBid => [248, 113, 113],
        }
    }
}

/// One opportunity, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct DealNode {
    /// Directory-safe instance name.
    pub name: String,
    pub label: String,
    pub position: [f32; 3],
    pub scale: [f32; 3],
    pub color: [u8; 3],
    pub lane: Lane,
    /// Everything Properties should show, and everything a script can read.
    pub attributes: BTreeMap<String, String>,
}

/// A complete placement of one pipeline run.
#[derive(Debug, Clone, PartialEq)]
pub struct DealRoom {
    pub nodes: Vec<DealNode>,
}

impl DealRoom {
    pub fn lane_counts(&self) -> (usize, usize, usize) {
        let bid = self.nodes.iter().filter(|n| n.lane == Lane::Bid).count();
        let watch = self.nodes.iter().filter(|n| n.lane == Lane::Watch).count();
        let no = self.nodes.iter().filter(|n| n.lane == Lane::NoBid).count();
        (bid, watch, no)
    }

    pub fn headline(&self) -> String {
        let (b, w, n) = self.lane_counts();
        format!("{} node(s): {b} bid, {w} watch, {n} no-bid", self.nodes.len())
    }
}

/// Place a scored run in space.
///
/// Nodes at the same deadline are spread along Z within their lane rather than
/// stacked into one another. Two solicitations closing the same day is the
/// normal case, not the edge case, and a pile of coincident blocks is unusable.
pub fn build(assessments: &[&Assessment], now: chrono::DateTime<chrono::Utc>) -> DealRoom {
    // Deterministic order so a rebuild produces the same room and a diff of the
    // written files is readable.
    let mut sorted: Vec<&&Assessment> = assessments.iter().collect();
    sorted.sort_by(|a, b| {
        a.opportunity
            .days_until_deadline(now)
            .unwrap_or(i64::MAX)
            .cmp(&b.opportunity.days_until_deadline(now).unwrap_or(i64::MAX))
            .then_with(|| a.opportunity.notice_id.cmp(&b.opportunity.notice_id))
    });

    let mut occupied: BTreeMap<(i64, i32), u32> = BTreeMap::new();
    let mut nodes = Vec::with_capacity(sorted.len());

    for a in sorted {
        let lane = Lane::of(&a.score.recommendation);
        let days = a.opportunity.days_until_deadline(now).unwrap_or(MAX_DAYS as i64);
        let x = (days.clamp(0, MAX_DAYS as i64) as f32) * METRES_PER_DAY;

        // Nudge coincident nodes sideways within their lane.
        let slot = occupied.entry((days, lane as i32)).or_insert(0);
        let offset = *slot as f32 * 3.0;
        *slot += 1;

        let footprint = footprint_for(a.opportunity.ceiling);
        let height = (a.score.total * MAX_HEIGHT).max(0.4);

        nodes.push(DealNode {
            name: node_name(&a.opportunity),
            label: label_for(&a.opportunity, &a.score),
            // Y is the block's centre, so a block of `height` sits on the floor
            // at y = height / 2 rather than half-buried.
            position: [x, height / 2.0, lane.z() - offset],
            scale: [footprint, height, footprint],
            color: lane.color(),
            lane,
            attributes: attributes_for(a, now),
        });
    }

    DealRoom { nodes }
}

/// Footprint from award ceiling, on a square-root curve.
///
/// Linear scaling would make a five-million-dollar contract forty times the
/// footprint of a hundred-and-twenty-thousand-dollar one and crowd everything
/// else out of the room.
fn footprint_for(ceiling: Option<i64>) -> f32 {
    let Some(c) = ceiling.filter(|c| *c > 0) else { return MIN_FOOTPRINT };
    let ratio = (c as f32 / FOOTPRINT_CEILING as f32).clamp(0.0, 1.0).sqrt();
    MIN_FOOTPRINT + ratio * (MAX_FOOTPRINT - MIN_FOOTPRINT)
}

/// Instance name: unique, path-safe, and readable in the Explorer.
fn node_name(o: &Opportunity) -> String {
    let title: String = o
        .title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-')
        .take(40)
        .collect();
    let title = title.trim();
    if title.is_empty() {
        o.slug()
    } else {
        format!("{title} [{}]", o.slug())
    }
}

/// The billboard text: what a person needs from across the room.
fn label_for(o: &Opportunity, score: &FitScore) -> String {
    let agency = if o.agency.is_empty() { "unknown agency" } else { &o.agency };
    format!("{}%  {}\n{}", score.percent(), score.recommendation.label(), agency)
}

/// Everything Properties shows and a script can read.
fn attributes_for(a: &Assessment, now: chrono::DateTime<chrono::Utc>) -> BTreeMap<String, String> {
    let o = &a.opportunity;
    let mut m = BTreeMap::new();
    m.insert("notice_id".into(), o.notice_id.clone());
    m.insert("source".into(), o.source.as_str().to_string());
    m.insert("title".into(), o.title.clone());
    m.insert("agency".into(), o.agency.clone());
    m.insert("solicitation_number".into(), o.solicitation_number.clone());
    m.insert("classification".into(), o.classification.clone());
    m.insert("set_aside".into(), o.set_aside.label());
    m.insert("url".into(), o.url.clone());
    m.insert("fit_percent".into(), a.score.percent().to_string());
    m.insert("recommendation".into(), a.score.recommendation.label().to_string());
    m.insert("lane".into(), Lane::of(&a.score.recommendation).as_str().to_string());
    if let Some(d) = o.days_until_deadline(now) {
        m.insert("days_left".into(), d.to_string());
    }
    if let Some(d) = o.deadline {
        m.insert("deadline".into(), d.to_rfc3339());
    }
    if let Some(c) = o.ceiling {
        m.insert("ceiling".into(), c.to_string());
    }
    if let Some(m2) = &a.matrix {
        m.insert("requirements".into(), m2.rows.len().to_string());
        m.insert("coverage_percent".into(), m2.coverage_percent().to_string());
        m.insert("gaps".into(), m2.gaps().len().to_string());
    }
    // The audit folder this node came from, so a click leads to the evidence.
    m.insert("capture_record".into(), format!("{}/{}", super::audit::CAPTURE_DIR, o.slug()));
    m
}

/// Write the room to disk for the file watcher to spawn.
///
/// Clears the previous room first: a re-sync replaces the pipeline, and leaving
/// last week's nodes standing would show a firm opportunities that closed.
pub fn write_room(space_root: &Path, room: &DealRoom) -> Result<usize, CaptureError> {
    let dir = space_root.join(DEAL_ROOM_DIR);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| CaptureError::Io(format!("clearing {}: {e}", dir.display())))?;
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| CaptureError::Io(format!("{}: {e}", dir.display())))?;

    // A Folder so the room is one collapsible node in the Explorer rather than
    // several hundred loose siblings.
    std::fs::write(
        dir.join("_instance.toml"),
        "[metadata]\nclass_name = \"Folder\"\narchivable = true\n",
    )
    .map_err(|e| CaptureError::Io(format!("deal room folder: {e}")))?;

    let mut written = 0usize;
    for (i, node) in room.nodes.iter().enumerate() {
        // Prefix with the index so the Explorer lists them in deadline order.
        let node_dir = dir.join(sanitize(&format!("{:03} {}", i + 1, node.name)));
        std::fs::create_dir_all(&node_dir)
            .map_err(|e| CaptureError::Io(format!("{}: {e}", node_dir.display())))?;
        std::fs::write(node_dir.join("_instance.toml"), part_toml(node))
            .map_err(|e| CaptureError::Io(format!("{}: {e}", node_dir.display())))?;

        let label_dir = node_dir.join("Label");
        std::fs::create_dir_all(&label_dir)
            .map_err(|e| CaptureError::Io(format!("{}: {e}", label_dir.display())))?;
        std::fs::write(label_dir.join("_instance.toml"), billboard_toml(node))
            .map_err(|e| CaptureError::Io(format!("{}: {e}", label_dir.display())))?;
        std::fs::write(label_dir.join("Text.textlabel.toml"), text_label_toml(node))
            .map_err(|e| CaptureError::Io(format!("{}: {e}", label_dir.display())))?;
        written += 1;
    }
    Ok(written)
}

/// The node's `_instance.toml`.
///
/// Floats are written with a decimal point throughout: TOML `8` is an integer
/// and serde rejects it for an `f32` field, which fails the whole file and
/// silently degrades the instance to a Folder.
fn part_toml(node: &DealNode) -> String {
    let mut s = String::new();
    s.push_str("[properties]\n");
    s.push_str(&format!(
        "color = [{}, {}, {}]\n",
        node.color[0], node.color[1], node.color[2]
    ));
    s.push_str("transparency = 0.0\nanchored = true\ncan_collide = true\nmaterial = \"SmoothPlastic\"\n\n");
    s.push_str("[transform]\n");
    s.push_str(&format!("position = [{}]\n", floats(&node.position)));
    s.push_str("rotation = [0.0, 0.0, 0.0, 1.0]\n");
    s.push_str(&format!("scale = [{}]\n\n", floats(&node.scale)));
    s.push_str("[attributes]\n");
    for (k, v) in &node.attributes {
        s.push_str(&format!("{k} = \"{}\"\n", escape(v)));
    }
    s.push_str("\n[metadata]\nclass_name = \"Part\"\narchivable = true\n");
    s
}

fn billboard_toml(node: &DealNode) -> String {
    // The billboard floats above the block rather than inside it, so the label
    // reads from any angle instead of clipping through the geometry.
    let lift = node.scale[1] / 2.0 + 1.2;
    format!(
        "[gui]\nenabled = true\nactive = true\nalways_on_top = false\nadornee = \"\"\n\
         max_distance = 240.0\nsize_offset = [220.0, 46.0]\nsize_scale = [0.0, 0.0]\n\
         studs_offset = [0.0, {lift:.1}, 0.0]\nlight_influence = 0.0\nclips_descendants = false\n\
         z_index_behavior = \"Sibling\"\n\n[metadata]\nclass_name = \"BillboardGui\"\narchivable = true\n"
    )
}

fn text_label_toml(node: &DealNode) -> String {
    let [r, g, b] = node.color;
    format!(
        "[gui]\nvisible = true\nbackground_color = [24, 24, 27]\nbackground_transparency = 0.25\n\
         border_color = [{r}, {g}, {b}]\nborder_size_pixel = 2\nborder_mode = \"Outline\"\n\
         clips_descendants = false\nz_index = 1\nlayout_order = 0\nrotation = 0.0\n\
         anchor_point = [0.0, 0.0]\nposition_scale = [0.0, 0.0]\nposition_offset = [0.0, 0.0]\n\
         size_scale = [1.0, 1.0]\nsize_offset = [0.0, 0.0]\nautomatic_size = \"None\"\nactive = true\n\n\
         [text]\ntext = \"{}\"\ntext_color = [{r}, {g}, {b}]\ntext_transparency = 0.0\n\
         text_stroke_color = [0, 0, 0]\ntext_stroke_transparency = 0.4\nfont_size = 15.0\n\
         font = \"GothamSsm\"\ntext_scaled = false\ntext_wrapped = true\n\
         text_x_alignment = \"Center\"\ntext_y_alignment = \"Center\"\nrich_text = false\n\
         line_height = 1.1\n\n[metadata]\nclass_name = \"TextLabel\"\narchivable = true\n",
        escape(&node.label)
    )
}

/// Always emit a decimal point. See [`part_toml`].
fn floats(v: &[f32; 3]) -> String {
    format!("{:.3}, {:.3}, {:.3}", v[0], v[1], v[2])
}

/// Escape for a TOML basic string. A raw newline or quote in an agency name
/// would otherwise break the file.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "")
}

/// Directory-safe instance name.
fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let t = cleaned.trim().to_string();
    if t.is_empty() { "node".into() } else { t }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::model::{NoticeSource, SetAside};
    use crate::capture::score::Criterion;
    use chrono::TimeZone;

    fn now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc.with_ymd_and_hms(2026, 8, 23, 12, 0, 0).unwrap()
    }

    fn assessment(id: &str, days: i64, total: f32, rec: Recommendation, ceiling: Option<i64>) -> Assessment {
        let mut o = Opportunity::empty(NoticeSource::Sam, id);
        o.title = format!("Notice {id}");
        o.agency = "ACC-APG".into();
        o.set_aside = SetAside::ServiceDisabledVeteran;
        o.deadline = Some(now() + chrono::Duration::days(days));
        o.ceiling = ceiling;
        Assessment {
            screen: crate::capture::screen::ScreenResult {
                notice_id: id.into(),
                rejections: vec![],
                unknowns: vec![],
            },
            score: FitScore {
                notice_id: id.into(),
                total,
                criteria: vec![Criterion {
                    name: "classification".into(),
                    weight: 1.0,
                    score: total,
                    evidence: "test".into(),
                }],
                recommendation: rec,
            },
            matrix: None,
            outline: None,
            opportunity: o,
        }
    }

    #[test]
    fn urgency_is_a_direction_and_fit_is_a_height() {
        let soon = assessment("soon", 5, 0.9, Recommendation::Bid, None);
        let later = assessment("later", 60, 0.9, Recommendation::Bid, None);
        let weak = assessment("weak", 5, 0.4, Recommendation::Watch { reasons: vec![] }, None);
        let room = build(&[&soon, &later, &weak], now());

        let get = |n: &str| room.nodes.iter().find(|x| x.attributes["notice_id"] == n).unwrap().clone();
        assert!(get("soon").position[0] < get("later").position[0], "sooner sits nearer the origin");
        assert!(get("soon").scale[1] > get("weak").scale[1], "a better fit stands taller");
    }

    #[test]
    fn the_recommendation_decides_the_lane_and_the_colour() {
        let bid = assessment("a", 10, 0.8, Recommendation::Bid, None);
        let watch = assessment("b", 10, 0.5, Recommendation::Watch { reasons: vec![] }, None);
        let no = assessment("c", 10, 0.2, Recommendation::NoBid { reasons: vec![] }, None);
        let room = build(&[&bid, &watch, &no], now());

        let (b, w, n) = room.lane_counts();
        assert_eq!((b, w, n), (1, 1, 1));
        assert!(room.headline().contains("1 bid"), "{}", room.headline());

        let lanes: Vec<f32> = room.nodes.iter().map(|x| x.lane.z()).collect();
        assert_eq!(lanes.len(), 3);
        let colors: std::collections::BTreeSet<[u8; 3]> =
            room.nodes.iter().map(|x| x.color).collect();
        assert_eq!(colors.len(), 3, "each lane reads as its own colour");
    }

    #[test]
    fn coincident_deadlines_are_spread_rather_than_stacked() {
        let a = assessment("a", 10, 0.8, Recommendation::Bid, None);
        let b = assessment("b", 10, 0.8, Recommendation::Bid, None);
        let c = assessment("c", 10, 0.8, Recommendation::Bid, None);
        let room = build(&[&a, &b, &c], now());
        let zs: Vec<f32> = room.nodes.iter().map(|n| n.position[2]).collect();
        let unique: std::collections::BTreeSet<String> =
            zs.iter().map(|z| format!("{z:.3}")).collect();
        assert_eq!(unique.len(), 3, "three notices closing the same day must not occupy one spot");
    }

    #[test]
    fn a_block_sits_on_the_floor_rather_than_half_buried() {
        let a = assessment("a", 10, 1.0, Recommendation::Bid, None);
        let room = build(&[&a], now());
        let n = &room.nodes[0];
        assert!((n.position[1] - n.scale[1] / 2.0).abs() < 0.001, "{:?}", n.position);
    }

    #[test]
    fn footprint_grows_with_value_but_does_not_swamp_the_room() {
        let small = footprint_for(Some(100_000));
        let large = footprint_for(Some(5_000_000));
        let huge = footprint_for(Some(500_000_000));
        assert!(small < large);
        assert_eq!(large, MAX_FOOTPRINT);
        assert_eq!(huge, MAX_FOOTPRINT, "an outlier is clamped, not allowed to fill the room");
        assert_eq!(footprint_for(None), MIN_FOOTPRINT);
    }

    #[test]
    fn a_node_carries_the_path_to_its_own_evidence() {
        let a = assessment("abc", 10, 0.8, Recommendation::Bid, None);
        let room = build(&[&a], now());
        assert_eq!(room.nodes[0].attributes["capture_record"], "Capture/sam-abc");
    }

    #[test]
    fn every_float_in_the_written_toml_carries_a_decimal_point() {
        // TOML `8` is an integer and serde rejects it for an f32, which fails
        // the whole file and silently degrades the Part to a Folder.
        let a = assessment("a", 10, 1.0, Recommendation::Bid, Some(5_000_000));
        let room = build(&[&a], now());
        let toml = part_toml(&room.nodes[0]);
        let parsed: toml::Value = toml::from_str(&toml).expect("the written TOML must parse");
        let pos = parsed["transform"]["position"].as_array().unwrap();
        assert!(pos.iter().all(|v| v.as_float().is_some()), "an integer slipped in: {toml}");
        let scale = parsed["transform"]["scale"].as_array().unwrap();
        assert!(scale.iter().all(|v| v.as_float().is_some()), "an integer slipped in: {toml}");
    }

    #[test]
    fn a_quote_in_an_agency_name_cannot_break_the_file() {
        let mut a = assessment("a", 10, 0.8, Recommendation::Bid, None);
        a.opportunity.agency = "The \"Big\" Office\nSecond line".into();
        let room = build(&[&a], now());
        let toml = part_toml(&room.nodes[0]);
        let parsed: toml::Value = toml::from_str(&toml).expect("escaped TOML must still parse");
        assert!(parsed["attributes"]["agency"].as_str().unwrap().contains("Big"));
    }

    #[test]
    fn writing_the_room_replaces_the_previous_one() {
        let root = std::env::temp_dir().join(format!("eustress-dealroom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let three: Vec<Assessment> = ["a", "b", "c"]
            .iter()
            .map(|id| assessment(id, 10, 0.8, Recommendation::Bid, None))
            .collect();
        let refs: Vec<&Assessment> = three.iter().collect();
        assert_eq!(write_room(&root, &build(&refs, now())).unwrap(), 3);

        let one = [assessment("only", 10, 0.8, Recommendation::Bid, None)];
        let one_refs: Vec<&Assessment> = one.iter().collect();
        assert_eq!(write_room(&root, &build(&one_refs, now())).unwrap(), 1);

        let dir = root.join(DEAL_ROOM_DIR);
        let children = std::fs::read_dir(&dir).unwrap().filter(|e| {
            e.as_ref().map(|e| e.path().is_dir()).unwrap_or(false)
        }).count();
        assert_eq!(children, 1, "a re-sync must not leave closed opportunities standing");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_written_room_has_a_folder_root_and_a_label_per_node() {
        let root = std::env::temp_dir().join(format!("eustress-dealroom-lbl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let a = [assessment("a", 10, 0.8, Recommendation::Bid, None)];
        let refs: Vec<&Assessment> = a.iter().collect();
        write_room(&root, &build(&refs, now())).unwrap();

        let dir = root.join(DEAL_ROOM_DIR);
        let folder = std::fs::read_to_string(dir.join("_instance.toml")).unwrap();
        assert!(folder.contains("class_name = \"Folder\""), "{folder}");

        let node_dir = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| p.is_dir())
            .expect("one node directory");
        let billboard = std::fs::read_to_string(node_dir.join("Label").join("_instance.toml")).unwrap();
        assert!(billboard.contains("BillboardGui"), "{billboard}");
        let text = std::fs::read_to_string(node_dir.join("Label").join("Text.textlabel.toml")).unwrap();
        let parsed: toml::Value = toml::from_str(&text).expect("label TOML parses");
        assert!(parsed["text"]["text"].as_str().unwrap().contains("Bid"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
