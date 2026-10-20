//! # Eustress material textures
//!
//! * [`mips`] builds the full mip chain for a decoded RGBA8 material map. The
//!   engine runs it when it loads a map, because PNG carries no mip levels and
//!   Bevy 0.19 generates none: without it every material texture is sampled at
//!   its full 2048² level at any distance, which aliases into shimmer and moiré.
//! * The `texture-gen` binary (`src/main.rs`) generates the bundled material
//!   library in `common/assets/materials/textures/`. Every pattern it draws is
//!   periodic over the tile, so each map repeats without a seam.
//!
//! The engine builds this crate at `opt-level = 3` even in dev (see
//! `eustress/.cargo/config.toml`), so a 2048² chain takes milliseconds in a
//! debug engine rather than the better part of a second.

pub mod mips;
