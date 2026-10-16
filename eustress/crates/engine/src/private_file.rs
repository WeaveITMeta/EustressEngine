//! Files and folders only the signed-in user can open: the module lives in
//! `eustress-identity` (stream-node uses it too); this path keeps the
//! engine's callers as they were.

pub use eustress_identity::private_file::*;
