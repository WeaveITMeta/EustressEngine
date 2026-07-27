//! # The avatar descriptor
//!
//! One serde shape describing a complete avatar. It is deliberately free of
//! Bevy and glam so the Leptos web customizer (pure CSR wasm, no
//! `eustress-common` dependency) and the engine link *the same compiled
//! code*. Shared compilation is a linker fact; a hand-mirrored struct on the
//! web side would be the exact class of drift this crate exists to prevent.

use serde::{Deserialize, Serialize};

#[cfg(feature = "bevy")]
use bevy::prelude::Component;
#[cfg(feature = "bevy")]
use bevy::reflect::Reflect;

/// Bumped whenever a stored descriptor needs migration.
pub const AVATAR_SCHEMA_VERSION: u16 = 1;

// ─────────────────────────────────────────────────────────────────────────────
// Scalars
// ─────────────────────────────────────────────────────────────────────────────

/// A `0.0..=1.0` authoring scalar.
///
/// The website ships `0..100` integer range inputs
/// (`web/src/pages/profile.rs:379`). `from_percent` / `percent` are the ONLY
/// conversion, so the slider value and the runtime value are structurally
/// incapable of disagreeing.
///
/// Non-finite input clamps to the midpoint rather than propagating NaN into
/// `BodyMetrics` — a NaN capsule radius is an Avian panic several frames later
/// with no indication of where it came from.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(transparent)]
pub struct Norm01(f32);

impl Norm01 {
    pub fn new(v: f32) -> Self {
        Self(if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 })
    }
    pub fn from_percent(p: i32) -> Self {
        Self::new(p as f32 / 100.0)
    }
    pub fn percent(self) -> i32 {
        (self.0 * 100.0).round() as i32
    }
    pub fn get(self) -> f32 {
        self.0
    }
    /// Map into an arbitrary range. The single place authoring scalars become
    /// physical quantities.
    pub fn remap(self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.0
    }
}

impl Default for Norm01 {
    fn default() -> Self {
        Norm01(0.5)
    }
}

/// sRGB bytes. Round-trips byte-exact with the website's `"#rrggbb"` swatches.
///
/// This is the NATIVE colour model for avatars. The 31-entry BrickColor
/// integer table at `engine/src/interaction/appearance.rs:65-102` is demoted to
/// an import adapter — it cannot represent the website's palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct Srgb8(pub u8, pub u8, pub u8);

impl Srgb8 {
    pub fn from_hex(s: &str) -> Result<Self, AvatarError> {
        let h = s.strip_prefix('#').unwrap_or(s);
        if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AvatarError::BadHex(s.to_string()));
        }
        let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
        Ok(Srgb8(p(0), p(2), p(4)))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// Linear-space floats for `StandardMaterial::base_color`.
    ///
    /// Bevy's `Color::srgb_u8` does this internally, but this crate cannot
    /// depend on `bevy_color`, and the web preview needs the same numbers.
    pub fn to_linear(self) -> [f32; 3] {
        fn eotf(c: u8) -> f32 {
            let c = c as f32 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        [eotf(self.0), eotf(self.1), eotf(self.2)]
    }
}

impl Serialize for Srgb8 {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Srgb8 {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Srgb8::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Base body — mesh and clip prefix from ONE match
// ─────────────────────────────────────────────────────────────────────────────

/// Replaces the crossed pair `BiologicalSex::character_model`
/// (`common/src/services/player.rs:985`, Female → XBot) and
/// `SkinnedCharacter::new` (`skinned_character.rs:143`, XBot → Male).
///
/// Because the mesh path and the clip prefix come out of the same type, the
/// pairing is uncrossable. That is the entire point — the old code let Studio
/// default Female and the Client default Male, and each then resolved a
/// different body through a different function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(rename_all = "snake_case")]
pub enum BaseBody {
    Feminine,
    /// Default body: Mixamo X Bot. Both shells spawn this, so the default
    /// avatar is identical in Studio Play Mode and the Client.
    #[default]
    Masculine,
}

impl BaseBody {
    /// Mixamo ships **X Bot as the feminine silhouette** and **Y Bot as the
    /// masculine one** — confirmed visually, not inferred from the names.
    ///
    /// The old `BiologicalSex::character_model` mapped `Female -> XBot`
    /// (`services/player.rs:985`) and was correct. The genuinely wrong half of
    /// that pair was `SkinnedCharacter::new`, which mapped `XBot -> Male`
    /// (`skinned_character.rs:143`). Both now come out of this one match, so
    /// the body and its clip set cannot disagree.
    pub const fn body_asset(self) -> &'static str {
        match self {
            BaseBody::Feminine => "bundled://characters/x_bot.glb",
            BaseBody::Masculine => "bundled://characters/y_bot.glb",
        }
    }
    /// Clip filename prefix: `{prefix}_idle.glb`, `{prefix}_walking.glb`, …
    pub const fn clip_prefix(self) -> &'static str {
        match self {
            BaseBody::Feminine => "female",
            BaseBody::Masculine => "male",
        }
    }
    pub const fn all() -> [BaseBody; 2] {
        [BaseBody::Feminine, BaseBody::Masculine]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Continuous body axes
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(default)]
pub struct BodyMorphs {
    /// Website "Height" slider (`profile.rs:379`).
    pub height: Norm01,
    /// Website "Build" slider (`profile.rs:383`).
    pub build: Norm01,
    /// Reserved; remaps `0.46..=0.56` of total height.
    pub leg_ratio: Norm01,
    /// Face Shape stored as four exclusive weights so today's 4-option
    /// dropdown and a future continuous face slider are the SAME data with no
    /// migration.
    pub face_round: Norm01,
    pub face_square: Norm01,
    pub face_oval: Norm01,
    pub face_diamond: Norm01,
}

impl Default for BodyMorphs {
    fn default() -> Self {
        Self {
            height: Norm01::default(),
            build: Norm01::default(),
            leg_ratio: Norm01::default(),
            face_round: Norm01::new(1.0),
            face_square: Norm01::new(0.0),
            face_oval: Norm01::new(0.0),
            face_diamond: Norm01::new(0.0),
        }
    }
}

/// The website's 4-option Face Shape dropdown (`profile.rs:394-399`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(rename_all = "snake_case")]
pub enum FaceShape {
    Round,
    Square,
    Oval,
    Diamond,
}

impl FaceShape {
    pub const fn label(self) -> &'static str {
        match self {
            FaceShape::Round => "Round",
            FaceShape::Square => "Square",
            FaceShape::Oval => "Oval",
            FaceShape::Diamond => "Diamond",
        }
    }
    pub const fn all() -> [FaceShape; 4] {
        [FaceShape::Round, FaceShape::Square, FaceShape::Oval, FaceShape::Diamond]
    }
    /// Collapse the one-hot weights back to a dropdown selection.
    pub fn dominant(m: &BodyMorphs) -> FaceShape {
        let mut best = (FaceShape::Round, m.face_round.get());
        for (s, w) in [
            (FaceShape::Square, m.face_square.get()),
            (FaceShape::Oval, m.face_oval.get()),
            (FaceShape::Diamond, m.face_diamond.get()),
        ] {
            if w > best.1 {
                best = (s, w);
            }
        }
        best.0
    }
    /// Write this selection as one-hot weights.
    pub fn apply(self, m: &mut BodyMorphs) {
        let one = Norm01::new(1.0);
        let zero = Norm01::new(0.0);
        m.face_round = zero;
        m.face_square = zero;
        m.face_oval = zero;
        m.face_diamond = zero;
        match self {
            FaceShape::Round => m.face_round = one,
            FaceShape::Square => m.face_square = one,
            FaceShape::Oval => m.face_oval = one,
            FaceShape::Diamond => m.face_diamond = one,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Wearable slots
// ─────────────────────────────────────────────────────────────────────────────

/// One slot per website control. `Hat`/`Glasses` ride bound sockets;
/// `Hair`/`Top`/`Bottom` are skinned or decal depending on the item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Hair,
    Face,
    Top,
    Bottom,
    Shoes,
    Hat,
    Glasses,
    Back,
}

impl SlotKind {
    pub const fn all() -> [SlotKind; 8] {
        [
            SlotKind::Hair,
            SlotKind::Face,
            SlotKind::Top,
            SlotKind::Bottom,
            SlotKind::Shoes,
            SlotKind::Hat,
            SlotKind::Glasses,
            SlotKind::Back,
        ]
    }
    pub const fn label(self) -> &'static str {
        match self {
            SlotKind::Hair => "Hair",
            SlotKind::Face => "Face",
            SlotKind::Top => "Top",
            SlotKind::Bottom => "Bottom",
            SlotKind::Shoes => "Shoes",
            SlotKind::Hat => "Hat",
            SlotKind::Glasses => "Glasses",
            SlotKind::Back => "Back",
        }
    }
}

/// A catalog item reference. `None` in a slot renders nothing — which is the
/// honest state for every slot until art exists.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
pub struct ItemId(pub String);

// ─────────────────────────────────────────────────────────────────────────────
// Palette
// ─────────────────────────────────────────────────────────────────────────────

/// Every colour the website exposes. Defaults are the FIRST swatch of each
/// row in `profile.rs`, so a default descriptor renders as the website's
/// default preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(default)]
pub struct Palette {
    pub skin: Srgb8,
    pub hair: Srgb8,
    pub top: Srgb8,
    pub bottom: Srgb8,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            skin: Srgb8(0xf5, 0xd0, 0xa9),
            hair: Srgb8(0x1a, 0x1a, 0x1a),
            top: Srgb8(0x2e, 0x5c, 0x8a),
            bottom: Srgb8(0x26, 0x26, 0x33),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Motion overrides
// ─────────────────────────────────────────────────────────────────────────────

/// Authored overrides on top of body-derived defaults. `None` = derive from
/// `BodyMetrics`, which is what makes a taller avatar walk faster and take
/// longer strides without anyone authoring a number.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Reflect))]
#[serde(default)]
pub struct MotionOverrides {
    pub walk_speed_mps: Option<f32>,
    pub run_speed_mps: Option<f32>,
    pub sprint_multiplier: Option<f32>,
    pub jump_apex_m: Option<f32>,
}

// ─────────────────────────────────────────────────────────────────────────────
// The descriptor
// ─────────────────────────────────────────────────────────────────────────────

/// A complete avatar. This is what the website stores, what the API returns,
/// and what `SpawnAvatar` carries. There is exactly one of these types in the
/// codebase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(Component, Reflect))]
#[serde(default)]
pub struct AvatarDescriptor {
    pub schema_version: u16,
    pub base_body: BaseBody,
    pub morphs: BodyMorphs,
    pub palette: Palette,
    /// Sorted on write so the content hash is order-independent.
    pub slots: Vec<(SlotKind, Option<ItemId>)>,
    pub motion: MotionOverrides,
}

impl Default for AvatarDescriptor {
    fn default() -> Self {
        Self {
            schema_version: AVATAR_SCHEMA_VERSION,
            base_body: BaseBody::default(),
            morphs: BodyMorphs::default(),
            palette: Palette::default(),
            slots: Vec::new(),
            motion: MotionOverrides::default(),
        }
    }
}

impl AvatarDescriptor {
    pub fn get_slot(&self, k: SlotKind) -> Option<&ItemId> {
        self.slots.iter().find(|(s, _)| *s == k).and_then(|(_, i)| i.as_ref())
    }

    pub fn set_slot(&mut self, k: SlotKind, item: Option<ItemId>) {
        if let Some(e) = self.slots.iter_mut().find(|(s, _)| *s == k) {
            e.1 = item;
        } else {
            self.slots.push((k, item));
        }
        self.slots.sort_by_key(|(s, _)| *s);
    }

    /// Canonical content hash — declaration-order byte encoding, never
    /// `serde_json` of anything map-shaped (map iteration order is not
    /// guaranteed stable across builds, and this hash keys the R2 preview
    /// cache and the LOD atlas bake).
    pub fn content_hash(&self) -> u64 {
        let mut h = Fnv1a::new();
        h.u16(self.schema_version);
        h.u8(self.base_body as u8);
        for v in [
            self.morphs.height,
            self.morphs.build,
            self.morphs.leg_ratio,
            self.morphs.face_round,
            self.morphs.face_square,
            self.morphs.face_oval,
            self.morphs.face_diamond,
        ] {
            h.f32(v.get());
        }
        for c in [self.palette.skin, self.palette.hair, self.palette.top, self.palette.bottom] {
            h.u8(c.0);
            h.u8(c.1);
            h.u8(c.2);
        }
        let mut slots = self.slots.clone();
        slots.sort_by_key(|(s, _)| *s);
        for (s, item) in &slots {
            h.u8(*s as u8);
            match item {
                Some(ItemId(id)) => {
                    h.u8(1);
                    h.bytes(id.as_bytes());
                }
                None => h.u8(0),
            }
        }
        for o in [
            self.motion.walk_speed_mps,
            self.motion.run_speed_mps,
            self.motion.sprint_multiplier,
            self.motion.jump_apex_m,
        ] {
            match o {
                Some(v) => {
                    h.u8(1);
                    h.f32(v);
                }
                None => h.u8(0),
            }
        }
        h.finish()
    }
}

/// Small, dependency-free, and stable across builds — which `DefaultHasher`
/// explicitly is not.
struct Fnv1a(u64);

impl Fnv1a {
    fn new() -> Self {
        Fnv1a(0xcbf2_9ce4_8422_2325)
    }
    fn u8(&mut self, v: u8) {
        self.0 ^= v as u64;
        self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
    }
    fn u16(&mut self, v: u16) {
        for b in v.to_le_bytes() {
            self.u8(b);
        }
    }
    fn f32(&mut self, v: f32) {
        // Normalise -0.0 and any NaN payload so equal descriptors hash equal.
        let v = if v == 0.0 {
            0.0
        } else if v.is_nan() {
            f32::NAN
        } else {
            v
        };
        for b in v.to_bits().to_le_bytes() {
            self.u8(b);
        }
    }
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.u8(*x);
        }
    }
    fn finish(self) -> u64 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AvatarError {
    BadHex(String),
    UnknownItem(String),
    WrongSlot { item: String, expected: SlotKind },
    SchemaTooNew(u16),
}

impl core::fmt::Display for AvatarError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AvatarError::BadHex(s) => write!(f, "not a #rrggbb colour: {s}"),
            AvatarError::UnknownItem(s) => write!(f, "unknown catalog item: {s}"),
            AvatarError::WrongSlot { item, expected } => {
                write!(f, "item {item} cannot go in slot {}", expected.label())
            }
            AvatarError::SchemaTooNew(v) => {
                write!(f, "descriptor schema v{v} is newer than supported v{AVATAR_SCHEMA_VERSION}")
            }
        }
    }
}

impl std::error::Error for AvatarError {}
