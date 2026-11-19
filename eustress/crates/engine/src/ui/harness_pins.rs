//! Pinned Studio state for harness Spaces.
//!
//! A harness Space (see `bin/harness_scenes.rs`) can carry
//! `.eustress/harness/layout.json` (a [`PanelLayout`] in the shape the Layout
//! menu's Save Layout writes) and `.eustress/harness/studio_pins.toml` (the
//! instance to select, by uuid). When such a Space finishes loading, the
//! Studio applies the layout through the same `PanelLayout::apply` the
//! Import Layout dialog uses and selects the instance through the selection
//! manager, once per open. A Space without these files is untouched.
//!
//! This module holds the pure parts (paths, parsing, the once-per-open
//! decision, the bounded wait); the system that touches the window lives
//! beside `follow_mode_layout` in `slint_ui.rs`, because `PanelLayout::apply`
//! is private to that module.
//!
//! [`PanelLayout`]: super::layout_tables::PanelLayout

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The folder inside a Space that holds the harness pins.
pub const HARNESS_DIR: [&str; 2] = [".eustress", "harness"];

/// The pins file inside [`HARNESS_DIR`].
pub const PINS_FILE: &str = "studio_pins.toml";

/// The layout file inside [`HARNESS_DIR`] when the pins file names none.
pub const DEFAULT_LAYOUT_FILE: &str = "layout.json";

/// How many frames after the load settles the pins wait: for the mode's
/// own layout to be applied first, and for the pinned instance to appear.
/// CONFIG DEFAULT: 600 frames (about 10 s at 60 FPS, TARGET frame rate).
pub const MAX_WAIT_FRAMES: u32 = 600;

/// `<space>/.eustress/harness`.
pub fn harness_dir(space_root: &Path) -> PathBuf {
    space_root.join(HARNESS_DIR[0]).join(HARNESS_DIR[1])
}

/// `studio_pins.toml` as the Studio reads it. `[window]` and `[camera]` are
/// read elsewhere (the window size by `EUSTRESS_WINDOW_SIZE`) and ignored here.
#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct StudioPins {
    #[serde(default)]
    pub selection: Option<PinnedSelection>,
    #[serde(default)]
    pub layout: Option<PinnedLayout>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct PinnedSelection {
    #[serde(default)]
    pub uuid: Option<String>,
    /// The instance's path, for messages only; the uuid is the identity.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, PartialEq)]
pub struct PinnedLayout {
    #[serde(default)]
    pub file: Option<String>,
}

/// Parse a pins file.
pub fn parse_pins(text: &str) -> Result<StudioPins, String> {
    toml::from_str::<StudioPins>(text).map_err(|e| e.to_string())
}

/// A uuid in the form `Instance.uuid` holds: 32 lowercase hex characters.
/// Dashes and case are forgiven; anything else is refused.
pub fn normalize_uuid(raw: &str) -> Option<String> {
    let hex: String = raw.trim().chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase();
    (hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

impl StudioPins {
    /// The uuid to select. `Err` when the file names one that is not a uuid.
    pub fn selection_uuid(&self) -> Result<Option<String>, String> {
        match self.selection.as_ref().and_then(|s| s.uuid.as_deref()) {
            None => Ok(None),
            Some(raw) => normalize_uuid(raw)
                .map(Some)
                .ok_or_else(|| format!("selection uuid '{raw}' is not a 32-character hex uuid")),
        }
    }

    /// The layout file's name inside the harness folder. Only a plain file
    /// name is accepted, so a pins file cannot point outside its folder.
    pub fn layout_file(&self) -> Result<&str, String> {
        let name = self.layout.as_ref().and_then(|l| l.file.as_deref()).unwrap_or(DEFAULT_LAYOUT_FILE);
        let plain = !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\', ':']);
        if plain {
            Ok(name)
        } else {
            Err(format!("layout file '{name}' is not a plain file name"))
        }
    }
}

/// One Space open: its root and its `SpaceLoadGeneration`. A reopen of the
/// same path bumps the generation, so it counts as a new open.
pub type SpaceOpen = (PathBuf, u64);

/// True when the pins should be read now: this open has not been handled
/// and the load has settled (`LoadInProgress.active` is false).
pub fn should_read_pins(handled: Option<&SpaceOpen>, this_open: &SpaceOpen, load_active: bool) -> bool {
    !load_active && handled != Some(this_open)
}

/// One frame of a bounded wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wait {
    /// The awaited thing is here: act now.
    Ready,
    /// Not yet; keep waiting.
    Waiting,
    /// Not after [`MAX_WAIT_FRAMES`]: give up (act anyway, or warn).
    GaveUp,
}

/// Advance a wait by one frame. `frames` counts the frames already waited.
pub fn step_wait(frames: &mut u32, ready: bool) -> Wait {
    if ready {
        return Wait::Ready;
    }
    *frames = frames.saturating_add(1);
    if *frames >= MAX_WAIT_FRAMES {
        Wait::GaveUp
    } else {
        Wait::Waiting
    }
}

/// What one Space open asks of the Studio, decided from the files.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PinPlan {
    /// The layout file to apply, if it exists.
    pub layout: Option<PathBuf>,
    /// The uuid to select, and the path to name in messages.
    pub select: Option<(String, String)>,
    /// Problems to warn about (an unreadable or invalid pins file).
    pub warnings: Vec<String>,
}

impl PinPlan {
    pub fn is_empty(&self) -> bool {
        self.layout.is_none() && self.select.is_none() && self.warnings.is_empty()
    }
}

/// Decide the plan for a Space from its harness folder. `pins_text` is the
/// pins file's text (`None` when absent, `Some(Err)` when unreadable);
/// `layout_exists` says whether a named layout file is on disk.
pub fn plan_pins(
    dir: &Path,
    pins_text: Option<Result<String, String>>,
    layout_exists: impl Fn(&Path) -> bool,
) -> PinPlan {
    let mut plan = PinPlan::default();
    let pins_path = dir.join(PINS_FILE);
    let pins = match pins_text {
        None => None,
        Some(Err(e)) => {
            plan.warnings.push(format!("{} could not be read: {e}", pins_path.display()));
            None
        }
        Some(Ok(text)) => match parse_pins(&text) {
            Ok(p) => Some(p),
            Err(e) => {
                plan.warnings.push(format!("{} could not be parsed: {e}", pins_path.display()));
                None
            }
        },
    };
    let layout_name = match pins.as_ref().map(StudioPins::layout_file) {
        None => DEFAULT_LAYOUT_FILE,
        Some(Ok(name)) => name,
        Some(Err(e)) => {
            plan.warnings.push(format!("{}: {e}", pins_path.display()));
            DEFAULT_LAYOUT_FILE
        }
    };
    let layout_path = dir.join(layout_name);
    if layout_exists(&layout_path) {
        plan.layout = Some(layout_path);
    } else if pins.as_ref().and_then(|p| p.layout.as_ref()).is_some() {
        plan.warnings.push(format!("{} names a layout that is not there: {}", pins_path.display(), layout_path.display()));
    }
    if let Some(pins) = pins.as_ref() {
        match pins.selection_uuid() {
            Ok(Some(uuid)) => {
                let path = pins.selection.as_ref().and_then(|s| s.path.clone()).unwrap_or_default();
                plan.select = Some((uuid, path));
            }
            Ok(None) => {}
            Err(e) => plan.warnings.push(format!("{}: {e}", pins_path.display())),
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const S4_PINS: &str = "# S4 studio pins.\n[selection]\npath = \"Workspace/Props/PinnedSelection\"\nuuid = \"62616faf08348f5c2a645b60b91dab7f\"\n\n[layout]\nfile = \"layout.json\"\n\n[window]\nwidth = 1920\nheight = 1080\n\n[camera]\nposition = [8.0, 6.0, 8.0]\nlook_at = [0.0, 0.5, -1.0]\n";

    #[test]
    fn parses_the_generated_s4_pins() {
        let pins = parse_pins(S4_PINS).expect("the generator's pins parse");
        assert_eq!(pins.selection_uuid().unwrap().as_deref(), Some("62616faf08348f5c2a645b60b91dab7f"));
        assert_eq!(pins.layout_file().unwrap(), "layout.json");
        assert_eq!(
            pins.selection.as_ref().and_then(|s| s.path.as_deref()),
            Some("Workspace/Props/PinnedSelection")
        );
    }

    #[test]
    fn uuid_forms() {
        assert_eq!(
            normalize_uuid(" 62616FAF-0834-8F5C-2A64-5B60B91DAB7F ").as_deref(),
            Some("62616faf08348f5c2a645b60b91dab7f")
        );
        assert_eq!(normalize_uuid("62616faf08348f5c2a645b60b91dab7"), None); // 31 chars
        assert_eq!(normalize_uuid("z2616faf08348f5c2a645b60b91dab7f"), None);
        let bad = parse_pins("[selection]\nuuid = \"PinnedSelection\"\n").unwrap();
        assert!(bad.selection_uuid().is_err());
    }

    #[test]
    fn layout_file_must_be_a_plain_name() {
        let pins = |f: &str| parse_pins(&format!("[layout]\nfile = {f:?}\n")).unwrap();
        assert_eq!(pins("pinned.json").layout_file().unwrap(), "pinned.json");
        assert!(pins("../../layout.json").layout_file().is_err());
        assert!(pins("sub\\layout.json").layout_file().is_err());
        assert!(pins("C:layout.json").layout_file().is_err());
        assert_eq!(StudioPins::default().layout_file().unwrap(), DEFAULT_LAYOUT_FILE);
    }

    #[test]
    fn reads_once_per_open_and_only_after_settle() {
        let open = (PathBuf::from("S4"), 3);
        assert!(!should_read_pins(None, &open, true), "never while loading");
        assert!(should_read_pins(None, &open, false));
        assert!(!should_read_pins(Some(&open), &open, false), "once per open");
        let reopen = (PathBuf::from("S4"), 4);
        assert!(should_read_pins(Some(&open), &reopen, false), "a reopen applies again");
    }

    #[test]
    fn wait_is_bounded_in_frames() {
        let mut frames = 0;
        for _ in 0..MAX_WAIT_FRAMES - 1 {
            assert_eq!(step_wait(&mut frames, false), Wait::Waiting);
        }
        assert_eq!(step_wait(&mut frames, false), Wait::GaveUp);
        let mut frames = 5;
        assert_eq!(step_wait(&mut frames, true), Wait::Ready);
        assert_eq!(frames, 5);
    }

    #[test]
    fn plan_for_a_space_without_pins_is_empty() {
        let plan = plan_pins(Path::new("Space/.eustress/harness"), None, |_| false);
        assert!(plan.is_empty());
    }

    #[test]
    fn plan_for_s4() {
        let dir = Path::new("S4/.eustress/harness");
        let plan = plan_pins(dir, Some(Ok(S4_PINS.to_string())), |p| p == dir.join("layout.json"));
        assert_eq!(plan.layout, Some(dir.join("layout.json")));
        assert_eq!(
            plan.select,
            Some(("62616faf08348f5c2a645b60b91dab7f".to_string(), "Workspace/Props/PinnedSelection".to_string()))
        );
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn plan_warns_on_a_bad_pins_file_but_keeps_the_layout() {
        let dir = Path::new("S4/.eustress/harness");
        let plan = plan_pins(dir, Some(Ok("[selection\nuuid=".to_string())), |_| true);
        assert_eq!(plan.layout, Some(dir.join(DEFAULT_LAYOUT_FILE)));
        assert!(plan.select.is_none());
        assert_eq!(plan.warnings.len(), 1);
        let plan = plan_pins(dir, Some(Err("denied".to_string())), |_| false);
        assert!(plan.layout.is_none());
        assert_eq!(plan.warnings.len(), 1);
    }
}
