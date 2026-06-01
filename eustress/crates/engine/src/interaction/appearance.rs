//! Character-appearance runtime — BodyColors / CharacterMesh / Shirt / Pants
//! / ShirtGraphic applied to the local character's rendered limbs.
//!
//! These five classes restyle a character. In Roblox each is a child of the
//! character model; the engine applies them to the limb entities (which carry
//! [`CharacterLimb`] + a [`MeshMaterial3d`]). Because the appearance classes
//! are siblings of the limbs under the character, the systems here resolve
//! "the character that owns this appearance instance" by walking up to the
//! nearest ancestor carrying a [`Character`] component, then recolor / re-skin
//! that character's limbs.
//!
//! ## Systems (all change-detected — cheap on idle frames)
//!
//! - [`apply_body_colors_system`] — recolors the 6 limb groups
//!   (head / torso / arms / legs) from a [`BodyColors`]' BrickColor indices.
//! - [`apply_clothing_system`] — applies [`Shirt`] (torso + arms) and
//!   [`Pants`] (legs) clothing-template textures, and the [`ShirtGraphic`]
//!   front-torso decal, to the matching limb materials.
//! - [`apply_character_mesh_system`] — applies a [`CharacterMesh`]'s
//!   replacement texture to the named body part's material (mesh swap is
//!   scaffolded — see the TODO).
//!
//! ## Limb → BodyColors group mapping
//!
//! [`BodyColors`] has 6 groups; [`CharacterLimb`] has ~18 bones. The mapping:
//!
//! | BodyColors field | CharacterLimb variants |
//! |------------------|------------------------|
//! | `head_color`     | Head, Neck |
//! | `torso_color`    | Hips, Spine, Chest |
//! | `left_arm_color` | LeftShoulder, LeftUpperArm, LeftLowerArm, LeftHand |
//! | `right_arm_color`| RightShoulder, RightUpperArm, RightLowerArm, RightHand |
//! | `left_leg_color` | LeftUpperLeg, LeftLowerLeg, LeftFoot |
//! | `right_leg_color`| RightUpperLeg, RightLowerLeg, RightFoot |
//!
//! ## No-op safety
//!
//! If an appearance instance has no character ancestor (e.g. it's sitting in
//! ServerStorage), the systems skip it. If a limb has no material handle they
//! skip that limb. No panics on a skeleton-less / simplified humanoid.
//!
//! [`CharacterLimb`]: eustress_common::plugins::humanoid::CharacterLimb
//! [`Character`]: eustress_common::services::player::Character

use bevy::prelude::*;

use eustress_common::classes::{BodyColors, CharacterMesh, Pants, Shirt, ShirtGraphic};
use eustress_common::plugins::humanoid::CharacterLimb;
use eustress_common::services::player::Character;

// ─────────────────────────────────────────────────────────────────────────
// BrickColor → linear RGB
// ─────────────────────────────────────────────────────────────────────────

/// Resolve a Roblox BrickColor palette index to an (approximate) sRGB
/// [`Color`]. Covers the common indices used by avatars; unknown indices fall
/// back to the BrickColor default (194 "Medium stone grey").
///
/// This is a deliberately small table — the full BrickColor palette has ~120
/// named entries. The avatar-relevant skin/clothing colors are covered; the
/// Wave-4 Roblox importer carries the authoritative palette
/// (`roblox-import/src/property_map.rs::to_color3uint8`) and can populate a
/// fuller table later. TODO: replace with the shared palette when it's lifted
/// out of the importer crate into `eustress-common`.
pub fn brick_color_to_srgb(index: i32) -> Color {
    // (index, r, g, b) — values in 0..=255, converted to 0..1 sRGB below.
    let rgb: (u8, u8, u8) = match index {
        1 => (242, 243, 243),    // White
        5 => (215, 197, 154),    // Brick yellow
        18 => (204, 142, 105),   // Nougat
        21 => (196, 40, 28),     // Bright red
        23 => (13, 105, 172),    // Bright blue
        24 => (245, 205, 48),    // Bright yellow
        26 => (27, 42, 53),      // Black
        28 => (40, 127, 71),     // Dark green
        37 => (75, 151, 75),     // Bright green
        38 => (160, 95, 53),     // Dark orange
        45 => (180, 210, 228),   // Light blue
        102 => (110, 153, 202),  // Medium blue
        105 => (226, 155, 64),   // Br. yellowish orange
        106 => (218, 133, 65),   // Bright orange
        119 => (164, 189, 71),   // Br. yellowish green
        125 => (234, 184, 146),  // Light orange
        135 => (116, 134, 157),  // Sand blue
        141 => (39, 70, 45),     // Earth green
        151 => (120, 144, 130),  // Sand green
        153 => (149, 121, 119),  // Sand red
        192 => (105, 64, 40),    // Reddish brown
        194 => (163, 162, 165),  // Medium stone grey (default)
        199 => (99, 95, 98),     // Dark stone grey
        208 => (229, 228, 223),  // Light stone grey
        217 => (124, 92, 70),    // Brown
        226 => (253, 234, 141),  // Cool yellow
        1001 => (248, 248, 248), // Institutional white
        1004 => (255, 0, 0),     // Really red
        1011 => (0, 16, 176),    // Navy blue
        1018 => (18, 238, 212),  // Teal
        1032 => (255, 0, 191),   // Hot pink
        _ => (163, 162, 165),    // fall back to Medium stone grey
    };
    Color::srgb_u8(rgb.0, rgb.1, rgb.2)
}

/// Which [`BodyColors`] group a [`CharacterLimb`] belongs to. Returns a
/// closure-friendly selector index 0..6, or `None` for limbs with no group
/// (none today — every variant maps).
fn limb_color(limb: &CharacterLimb, colors: &BodyColors) -> i32 {
    use CharacterLimb::*;
    match limb {
        Head | Neck => colors.head_color,
        Hips | Spine | Chest => colors.torso_color,
        LeftShoulder | LeftUpperArm | LeftLowerArm | LeftHand => colors.left_arm_color,
        RightShoulder | RightUpperArm | RightLowerArm | RightHand => colors.right_arm_color,
        LeftUpperLeg | LeftLowerLeg | LeftFoot => colors.left_leg_color,
        RightUpperLeg | RightLowerLeg | RightFoot => colors.right_leg_color,
    }
}

/// True if a limb is part of the torso group (shirt covers torso + arms).
fn is_torso_or_arm(limb: &CharacterLimb) -> bool {
    use CharacterLimb::*;
    matches!(
        limb,
        Hips | Spine
            | Chest
            | LeftShoulder
            | LeftUpperArm
            | LeftLowerArm
            | LeftHand
            | RightShoulder
            | RightUpperArm
            | RightLowerArm
            | RightHand
    )
}

/// True if a limb is part of the legs group (pants cover legs).
fn is_leg(limb: &CharacterLimb) -> bool {
    use CharacterLimb::*;
    matches!(
        limb,
        LeftUpperLeg | LeftLowerLeg | LeftFoot | RightUpperLeg | RightLowerLeg | RightFoot
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Ancestor walk: appearance instance → owning character
// ─────────────────────────────────────────────────────────────────────────

/// Walk up from `start` to the nearest ancestor carrying a [`Character`].
/// Capped to avoid pathological hierarchies. Returns `None` when the
/// appearance instance isn't under a character.
fn owning_character(
    start: Entity,
    characters: &Query<(), With<Character>>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    let mut current = start;
    for _ in 0..32 {
        if characters.get(current).is_ok() {
            return Some(current);
        }
        match parents.get(current) {
            Ok(child_of) => current = child_of.parent(),
            Err(_) => return None,
        }
    }
    None
}

/// Collect every [`CharacterLimb`] entity in `character`'s subtree (BFS),
/// returning `(entity, limb)` pairs. Includes the character root if it
/// carries a limb (the simplified humanoid puts `CharacterLimb::Hips` on the
/// root).
fn collect_limbs(
    character: Entity,
    limbs: &Query<&CharacterLimb>,
    children: &Query<&Children>,
) -> Vec<(Entity, CharacterLimb)> {
    let mut out = Vec::new();
    let mut stack = vec![character];
    let mut visited = 0;
    while let Some(entity) = stack.pop() {
        visited += 1;
        if visited > 256 {
            break;
        }
        if let Ok(limb) = limbs.get(entity) {
            out.push((entity, *limb));
        }
        if let Ok(kids) = children.get(entity) {
            for child in kids.iter() {
                stack.push(child);
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────
// Systems
// ─────────────────────────────────────────────────────────────────────────

/// Recolor a character's limbs from a [`BodyColors`]. Runs when a
/// [`BodyColors`] is added or changed (change-detection keeps it cheap).
pub fn apply_body_colors_system(
    body_colors: Query<(Entity, &BodyColors), Changed<BodyColors>>,
    characters: Query<(), With<Character>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    limbs: Query<&CharacterLimb>,
    limb_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (bc_entity, colors) in body_colors.iter() {
        let Some(character) = owning_character(bc_entity, &characters, &parents) else {
            continue; // not under a character — nothing to recolor
        };
        for (limb_entity, limb) in collect_limbs(character, &limbs, &children) {
            let Ok(mat_handle) = limb_materials.get(limb_entity) else {
                continue; // limb has no material — skip
            };
            let Some(material) = materials.get_mut(&mat_handle.0) else {
                continue;
            };
            material.base_color = brick_color_to_srgb(limb_color(&limb, colors));
        }
    }
}

/// Apply [`Shirt`] / [`Pants`] / [`ShirtGraphic`] to the owning character's
/// limb materials. Loads the clothing-template texture (a Roblox asset id /
/// path) via the asset server and assigns it to the matching limbs'
/// `base_color_texture`.
///
/// Roblox clothing templates are UV-laid-out atlases; mapping them precisely
/// onto skinned limbs needs per-limb UV regions (TODO). This applies the
/// template texture to the limb materials so the clothing is *visible* and
/// hot-swaps when the template changes — the visual-fidelity UV pass is a
/// follow-up.
pub fn apply_clothing_system(
    shirts: Query<(Entity, &Shirt), Changed<Shirt>>,
    pants: Query<(Entity, &Pants), Changed<Pants>>,
    graphics: Query<(Entity, &ShirtGraphic), Changed<ShirtGraphic>>,
    characters: Query<(), With<Character>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    limbs: Query<&CharacterLimb>,
    limb_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Shirts cover torso + arms.
    for (entity, shirt) in shirts.iter() {
        if shirt.shirt_template.is_empty() {
            continue;
        }
        let Some(character) = owning_character(entity, &characters, &parents) else {
            continue;
        };
        let texture: Handle<Image> = asset_server.load(shirt.shirt_template.clone());
        apply_texture_to_limbs(
            character,
            &limbs,
            &children,
            &limb_materials,
            &mut materials,
            &texture,
            is_torso_or_arm,
        );
    }

    // Pants cover legs.
    for (entity, p) in pants.iter() {
        if p.pants_template.is_empty() {
            continue;
        }
        let Some(character) = owning_character(entity, &characters, &parents) else {
            continue;
        };
        let texture: Handle<Image> = asset_server.load(p.pants_template.clone());
        apply_texture_to_limbs(
            character,
            &limbs,
            &children,
            &limb_materials,
            &mut materials,
            &texture,
            is_leg,
        );
    }

    // ShirtGraphic is a front-torso decal — applied to the chest/spine limbs.
    for (entity, g) in graphics.iter() {
        if g.graphic.is_empty() {
            continue;
        }
        let Some(character) = owning_character(entity, &characters, &parents) else {
            continue;
        };
        let texture: Handle<Image> = asset_server.load(g.graphic.clone());
        apply_texture_to_limbs(
            character,
            &limbs,
            &children,
            &limb_materials,
            &mut materials,
            &texture,
            |limb| matches!(limb, CharacterLimb::Chest | CharacterLimb::Spine),
        );
    }
}

/// Apply a [`CharacterMesh`] override to the named body part. Applies the
/// base texture to the matching limb's material; the mesh swap itself is
/// scaffolded (TODO) because it requires loading the replacement mesh and
/// re-binding the skinned-mesh handle, which the simplified humanoid doesn't
/// yet expose per-limb.
pub fn apply_character_mesh_system(
    char_meshes: Query<(Entity, &CharacterMesh), Changed<CharacterMesh>>,
    characters: Query<(), With<Character>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    limbs: Query<&CharacterLimb>,
    limb_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    for (entity, cm) in char_meshes.iter() {
        let Some(character) = owning_character(entity, &characters, &parents) else {
            continue;
        };
        // Resolve which limb group the `body_part` string targets.
        let part_filter = body_part_filter(&cm.body_part);

        if !cm.base_texture_id.is_empty() {
            let texture: Handle<Image> = asset_server.load(cm.base_texture_id.clone());
            apply_texture_to_limbs(
                character,
                &limbs,
                &children,
                &limb_materials,
                &mut materials,
                &texture,
                part_filter,
            );
        }

        // TODO(mesh-swap): load `cm.mesh_id` as a Mesh and rebind the matching
        // limb's `Mesh3d`. The simplified humanoid shares one body mesh, so a
        // true per-limb mesh swap needs the full skeletal hierarchy's per-limb
        // Mesh3d handles. Texture override above is the visible portion today.
        if !cm.mesh_id.is_empty() {
            tracing::debug!(
                "[CharacterMesh] mesh swap for body_part='{}' (mesh_id='{}') is scaffolded — texture applied, mesh swap pending skeletal per-limb meshes",
                cm.body_part,
                cm.mesh_id
            );
        }
    }
}

/// Map a `CharacterMesh.body_part` string to a limb filter predicate.
fn body_part_filter(body_part: &str) -> fn(&CharacterLimb) -> bool {
    match body_part.trim() {
        "Head" => |l: &CharacterLimb| matches!(l, CharacterLimb::Head | CharacterLimb::Neck),
        "LeftArm" => |l: &CharacterLimb| {
            matches!(
                l,
                CharacterLimb::LeftShoulder
                    | CharacterLimb::LeftUpperArm
                    | CharacterLimb::LeftLowerArm
                    | CharacterLimb::LeftHand
            )
        },
        "RightArm" => |l: &CharacterLimb| {
            matches!(
                l,
                CharacterLimb::RightShoulder
                    | CharacterLimb::RightUpperArm
                    | CharacterLimb::RightLowerArm
                    | CharacterLimb::RightHand
            )
        },
        "LeftLeg" => |l: &CharacterLimb| {
            matches!(
                l,
                CharacterLimb::LeftUpperLeg | CharacterLimb::LeftLowerLeg | CharacterLimb::LeftFoot
            )
        },
        "RightLeg" => |l: &CharacterLimb| {
            matches!(
                l,
                CharacterLimb::RightUpperLeg
                    | CharacterLimb::RightLowerLeg
                    | CharacterLimb::RightFoot
            )
        },
        // "Torso" and anything else → torso group.
        _ => |l: &CharacterLimb| {
            matches!(l, CharacterLimb::Hips | CharacterLimb::Spine | CharacterLimb::Chest)
        },
    }
}

/// Apply `texture` as the `base_color_texture` of every limb material in
/// `character`'s subtree that passes `filter`.
fn apply_texture_to_limbs(
    character: Entity,
    limbs: &Query<&CharacterLimb>,
    children: &Query<&Children>,
    limb_materials: &Query<&MeshMaterial3d<StandardMaterial>>,
    materials: &mut Assets<StandardMaterial>,
    texture: &Handle<Image>,
    filter: impl Fn(&CharacterLimb) -> bool,
) {
    for (limb_entity, limb) in collect_limbs(character, limbs, children) {
        if !filter(&limb) {
            continue;
        }
        let Ok(mat_handle) = limb_materials.get(limb_entity) else {
            continue;
        };
        if let Some(material) = materials.get_mut(&mat_handle.0) {
            material.base_color_texture = Some(texture.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brick_color_known_indices() {
        // White, Bright red, Black map to roughly the right hue.
        let white = brick_color_to_srgb(1).to_srgba();
        assert!(white.red > 0.9 && white.green > 0.9 && white.blue > 0.9);
        let red = brick_color_to_srgb(21).to_srgba();
        assert!(red.red > 0.6 && red.green < 0.3 && red.blue < 0.3);
    }

    #[test]
    fn brick_color_unknown_falls_back_to_grey() {
        let unknown = brick_color_to_srgb(999_999).to_srgba();
        let default = brick_color_to_srgb(194).to_srgba();
        assert_eq!(unknown.red, default.red);
        assert_eq!(unknown.green, default.green);
        assert_eq!(unknown.blue, default.blue);
    }

    #[test]
    fn limb_group_mapping_is_total() {
        // Every CharacterLimb variant resolves to one of the 6 colors.
        let colors = BodyColors {
            head_color: 1,
            torso_color: 2,
            left_arm_color: 3,
            right_arm_color: 4,
            left_leg_color: 5,
            right_leg_color: 6,
        };
        use CharacterLimb::*;
        for limb in [
            Hips, Spine, Chest, Neck, Head, LeftShoulder, LeftUpperArm, LeftLowerArm, LeftHand,
            RightShoulder, RightUpperArm, RightLowerArm, RightHand, LeftUpperLeg, LeftLowerLeg,
            LeftFoot, RightUpperLeg, RightLowerLeg, RightFoot,
        ] {
            let c = limb_color(&limb, &colors);
            assert!((1..=6).contains(&c), "limb {:?} mapped to unexpected color {}", limb, c);
        }
    }
}
