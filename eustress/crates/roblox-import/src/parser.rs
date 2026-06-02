//! File → `RobloxDom` wrapper around `rbx_dom_weak::WeakDom`.
//!
//! Auto-detects which of the four Roblox file formats is on disk and
//! dispatches to the matching `rbx_binary` / `rbx_xml` decoder.
//! See `docs/architecture/ROBLOX_IMPORT_SPEC.md` §2 (pipeline) and §4
//! (module structure).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use rbx_dom_weak::WeakDom;

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

impl RobloxFormat {
    /// Is this format binary (`.rbxl`, `.rbxm`) rather than XML
    /// (`.rbxlx`, `.rbxmx`)?
    pub fn is_binary(self) -> bool {
        matches!(self, RobloxFormat::BinaryPlace | RobloxFormat::BinaryModel)
    }

    /// Is this format a place file (`.rbxl`, `.rbxlx`) rather than a
    /// model fragment (`.rbxm`, `.rbxmx`)?
    pub fn is_place(self) -> bool {
        matches!(self, RobloxFormat::BinaryPlace | RobloxFormat::XmlPlace)
    }

    /// Tentative format from a file extension. `None` for an unknown
    /// extension; the magic-bytes sniff in [`parse`] is the
    /// authoritative source.
    fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "rbxl" => Some(RobloxFormat::BinaryPlace),
            "rbxm" => Some(RobloxFormat::BinaryModel),
            "rbxlx" => Some(RobloxFormat::XmlPlace),
            "rbxmx" => Some(RobloxFormat::XmlModel),
            _ => None,
        }
    }
}

/// Owned in-memory DataModel + provenance for diagnostics.
///
/// Wraps `rbx_dom_weak::WeakDom`. The wrapped DOM is available via
/// [`RobloxDom::dom`] for the materializer + walker.
#[derive(Debug)]
pub struct RobloxDom {
    /// The originating file path, retained for error messages and the
    /// `ImportReport::source_path` field. Empty for in-memory fixtures.
    pub source_path: PathBuf,
    /// Which format produced this DOM.
    pub format: RobloxFormat,
    /// The decoded DataModel tree.
    pub(crate) dom: WeakDom,
}

impl RobloxDom {
    /// Construct a `RobloxDom` from an in-memory `WeakDom`. Useful for
    /// tests and programmatic fixtures; the production path is [`parse`].
    pub fn from_dom(dom: WeakDom, format: RobloxFormat, source_path: PathBuf) -> Self {
        Self {
            source_path,
            format,
            dom,
        }
    }

    /// Read-only access to the underlying `WeakDom`.
    pub fn dom(&self) -> &WeakDom {
        &self.dom
    }
}

/// Parse a Roblox file (auto-detects format from extension + magic bytes).
///
/// 1. Try the file extension to tentatively pick `RobloxFormat`.
/// 2. Peek the first 8 bytes; binary files start with `<roblox!`,
///    XML files start with `<roblox `. Magic wins over extension.
/// 3. Dispatch to `rbx_binary::from_reader` or `rbx_xml::from_reader_default`.
/// 4. Wrap in `RobloxDom`.
pub fn parse(path: &Path) -> Result<RobloxDom, ImportError> {
    let mut file = File::open(path).map_err(|e| ImportError::Io(path.to_path_buf(), e))?;

    // Peek up to 8 bytes for the magic sniff.
    let mut magic = [0u8; 8];
    let read = file
        .read(&mut magic)
        .map_err(|e| ImportError::Io(path.to_path_buf(), e))?;
    let magic = &magic[..read];

    let ext_format = path
        .extension()
        .and_then(|s| s.to_str())
        .and_then(RobloxFormat::from_extension);

    let magic_is_binary = magic.starts_with(b"<roblox!");
    let magic_is_xml = magic.starts_with(b"<roblox ")
        || magic.starts_with(b"<?xml")
        || magic.starts_with(b"\xef\xbb\xbf<roblox") // UTF-8 BOM + XML start
        || magic.starts_with(b"\xef\xbb\xbf<?xml");

    let format = match (ext_format, magic_is_binary, magic_is_xml) {
        // Extension says binary, magic confirms.
        (Some(f @ (RobloxFormat::BinaryPlace | RobloxFormat::BinaryModel)), true, _) => f,
        // Extension says XML, magic confirms.
        (Some(f @ (RobloxFormat::XmlPlace | RobloxFormat::XmlModel)), false, true) => f,
        // Extension and magic disagree — trust magic and pick a sensible
        // place/model variant. .rbxl / .rbxlx are places; the rest are
        // models. If the extension said anything, use its place/model
        // distinction; otherwise default to "place".
        (Some(ext), _, _) if magic_is_binary => {
            if ext.is_place() {
                RobloxFormat::BinaryPlace
            } else {
                RobloxFormat::BinaryModel
            }
        }
        (Some(ext), _, _) if magic_is_xml => {
            if ext.is_place() {
                RobloxFormat::XmlPlace
            } else {
                RobloxFormat::XmlModel
            }
        }
        // No extension hint — trust magic alone, default to place.
        (None, true, _) => RobloxFormat::BinaryPlace,
        (None, _, true) => RobloxFormat::XmlPlace,
        // Neither extension nor magic recognised → caller passed something we
        // can't handle.
        _ => return Err(ImportError::UnsupportedFormat),
    };

    // Re-open the file for the actual decode (we already consumed the
    // magic bytes from the first handle).
    let file = File::open(path).map_err(|e| ImportError::Io(path.to_path_buf(), e))?;
    let reader = BufReader::new(file);

    let dom = if format.is_binary() {
        rbx_binary::from_reader(reader).map_err(|e| ImportError::BinaryParse(e.to_string()))?
    } else {
        rbx_xml::from_reader_default(reader).map_err(|e| ImportError::XmlParse(e.to_string()))?
    };

    Ok(RobloxDom {
        source_path: path.to_path_buf(),
        format,
        dom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::InstanceBuilder;

    #[test]
    fn format_from_extension() {
        assert_eq!(
            RobloxFormat::from_extension("rbxl"),
            Some(RobloxFormat::BinaryPlace)
        );
        assert_eq!(
            RobloxFormat::from_extension("RBXLX"),
            Some(RobloxFormat::XmlPlace)
        );
        assert_eq!(
            RobloxFormat::from_extension("rbxm"),
            Some(RobloxFormat::BinaryModel)
        );
        assert_eq!(
            RobloxFormat::from_extension("rbxmx"),
            Some(RobloxFormat::XmlModel)
        );
        assert_eq!(RobloxFormat::from_extension("txt"), None);
    }

    #[test]
    fn from_dom_round_trip() {
        let dom = WeakDom::new(InstanceBuilder::new("DataModel"));
        let wrapped =
            RobloxDom::from_dom(dom, RobloxFormat::BinaryPlace, PathBuf::from("test.rbxl"));
        assert_eq!(wrapped.format, RobloxFormat::BinaryPlace);
        assert_eq!(wrapped.dom().root().class, "DataModel");
    }

    #[test]
    fn format_classifiers() {
        assert!(RobloxFormat::BinaryPlace.is_binary());
        assert!(RobloxFormat::BinaryModel.is_binary());
        assert!(!RobloxFormat::XmlPlace.is_binary());
        assert!(!RobloxFormat::XmlModel.is_binary());

        assert!(RobloxFormat::BinaryPlace.is_place());
        assert!(RobloxFormat::XmlPlace.is_place());
        assert!(!RobloxFormat::BinaryModel.is_place());
        assert!(!RobloxFormat::XmlModel.is_place());
    }
}
