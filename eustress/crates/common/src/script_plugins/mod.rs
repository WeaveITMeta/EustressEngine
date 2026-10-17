//! Script-authored Studio plugins — shared (language-agnostic) data types.
//!
//! Both the Luau host (`mlua::runtime::MluaRuntime::
//! build_plugin_environment`) and the Rune host
//! (`engine::soul::rune_ecs_module::plugin_add_section` et al.) push onto
//! the SAME `PluginBridge` queue defined here. Lives outside the
//! Luau-specific `mlua::` tree on purpose — a Luau-named module hosting
//! Rune types would be the same kind of same-name confusion this
//! codebase has already hit once (`studio_plugins/rune_api.rs` vs
//! `soul/rune_api.rs`).

pub mod bridge;
pub use bridge::*;
