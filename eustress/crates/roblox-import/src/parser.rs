//! File → `RobloxDom` wrapper around `rbx_dom_weak::WeakDom`.
//!
//! Wave 1 scaffold — body is `todo!()`. See `docs/architecture/
//! ROBLOX_IMPORT_SPEC.md` §2 (pipeline) and §4 (module structure).

use std::path::{Path, PathBuf};

use crate::error::ImportError;

/// Which on-disk Roblox format the input was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RobloxFormat {
    /// `.rbxl` — binary place file.
    #[default]
    BinaryPlace,
    /// `.rbxm` — binary model file.
    BinaryModel,
    /// `.rbxlx` — XML place file.
    XmlPlace,
    /// `.rbxmx` — XML model file.
    XmlModel,
}

/// Owned in-memory DataModel + provenance for diagnostics.
///
/// Wraps `rbx_dom_weak::WeakDom`. Held opaque in Wave 1 because the
/// `rbx_dom_weak` dep is not yet enabled — once it is, the inner field
/// becomes `pub(crate) dom: rbx_dom_weak::WeakDom`.
#[derive(Debug)]
pub struct RobloxDom {
    /// The originating file path, retained for error messages and the
    /// `ImportReport::source_path` field.
    pub source_path: PathBuf,
    /// Which format produced this DOM.
    pub format: RobloxFormat,
    // pub(crate) dom: rbx_dom_weak::WeakDom,   // Wave 2
}

/// Parse a Roblox file (auto-detects format from extension + magic bytes).
///
/// Wave 1: returns `todo!()`. Wave 2 implementation:
///   1. Detect extension → tentative `RobloxFormat`.
///   2. Open + read first 6 bytes; binary files start with `<roblox!`,
///      XML files start with `<roblox `. Disagreement = warning, trust
///      magic.
///   3. Dispatch to `rbx_binary::from_reader` or `rbx_xml::from_reader`.
///   4. Wrap in `RobloxDom`.
pub fn parse(path: &Path) -> Result<RobloxDom, ImportError> {
    let _ = path; // silence unused warning until Wave 2
    todo!("Wave 2: implement once rbx_binary + rbx_xml deps are enabled");
}
