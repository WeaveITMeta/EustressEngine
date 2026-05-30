//! Hard import failures. These short-circuit the import; soft
//! diagnostics live in [`crate::import_report`].
//!
//! Real (not stubbed) — `thiserror` is a workspace dep.

use std::path::PathBuf;

/// A short-circuiting import error.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// Couldn't open the source file.
    #[error("could not open {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),

    /// File extension / magic bytes didn't match one of the four
    /// supported formats.
    #[error("unsupported file format (expected .rbxl, .rbxlx, .rbxm, .rbxmx)")]
    UnsupportedFormat,

    /// `rbx_binary` reported a parse failure.
    #[error("rbx_binary parse failed: {0}")]
    BinaryParse(String),

    /// `rbx_xml` reported a parse failure.
    #[error("rbx_xml parse failed: {0}")]
    XmlParse(String),

    /// Target Space root doesn't exist or isn't writable.
    #[error("target space root not writable: {0}")]
    SpaceNotWritable(PathBuf),

    /// `eustress_common::instance_create::create_instance` returned an
    /// error — propagated up as a string to avoid a circular dep on
    /// `eustress_common::instance_create::CreateError` in the public
    /// API.
    #[error("instance_create failed for {class}: {source_msg}")]
    InstanceCreate {
        /// Roblox class (e.g. `"Part"`) being created at the time.
        class: String,
        /// Stringified inner error.
        source_msg: String,
    },

    /// The walker attempted to route into one of the Eustress-only
    /// folders (`SoulService/`, `AdornmentService/`, `_retired_layers/`).
    /// Should never happen if the router's deny-list is honored.
    #[error("attempt to write into Eustress-only folder {0}")]
    OffLimits(PathBuf),

    /// Service router could not resolve a service name. Retained for
    /// catastrophic cases (deny-listed names). Most "unknown" services
    /// route to `_imported/<ServiceName>/` rather than failing.
    #[error("service router could not resolve service {service}: {reason}")]
    ServiceRouter {
        /// The Roblox service name that failed to resolve.
        service: String,
        /// Human-readable reason.
        reason: String,
    },
}
