//! Tool / Accessory equip runtime — equip/unequip, hotbar, activation.
//!
//! Roblox semantics implemented here:
//!
//! - **Equip**: parenting a [`Tool`] under the character "equips" it — its
//!   `Handle` child part is welded to the character's hand attachment. We
//!   model the weld as: re-parent the Handle under the character's right-hand
//!   limb and zero its local transform (offset by the Tool's `grip_pos`). The
//!   reverse (unequip) returns the Tool to the Backpack (detaches).
//! - **Hotbar**: number keys `1`..=`9` select among the character's equipped
//!   Tools (Roblox's toolbar). Selecting equips the chosen Tool and unequips
//!   the rest.
//! - **Activation**: left-click (or a script call) fires the equipped Tool's
//!   `Activated` event on the [`EventBus`] (unless `manual_activation_only`).
//!
//! ## What's fully wired vs scaffolded
//!
//! - Equip-weld upkeep ([`weld_equipped_tools_system`]) — FULLY WIRED: detects
//!   newly [`Equipped`] tools, finds their `Handle` child, re-parents it to
//!   the character's right hand, and applies the grip offset. Unequip restores
//!   the original parent.
//! - Hotbar select ([`hotbar_select_system`]) — FULLY WIRED for keys 1–9.
//! - Activation ([`tool_activate_system`]) — FULLY WIRED: fires `Activated` on
//!   left-click for the currently-equipped Tool.
//! - Accessory attach — SCAFFOLDED-WITH-TODO: Accessories weld to a named
//!   character `Attachment`; the attachment-point lookup needs the
//!   `Attachment`-by-name resolver (see [`weld_equipped_tools_system`] TODO).
//!
//! ## No-op safety
//!
//! Resolves the local character via [`LocalCharacter`] and returns early when
//! absent. The hand-limb lookup tolerates a character with no skeleton (it
//! falls back to welding directly under the character root).
//!
//! [`Tool`]: eustress_common::classes::Tool
//! [`EventBus`]: eustress_common::events::EventBus

use bevy::prelude::*;

use eustress_common::classes::Tool;
use eustress_common::events::EventBusResource;
use eustress_common::plugins::humanoid::CharacterLimb;
use eustress_common::scripting::events::SignalArg;

use super::LocalCharacter;

// ─────────────────────────────────────────────────────────────────────────
// Components & resources
// ─────────────────────────────────────────────────────────────────────────

/// Marker inserted on a [`Tool`] entity while it is equipped (parented to the
/// holder and its Handle welded to the hand). Removing it triggers unequip.
#[derive(Component, Debug, Clone, Copy)]
pub struct Equipped {
    /// The character entity holding this tool.
    pub holder: Entity,
}

/// Records a welded Tool's Handle + the Handle's original parent and local
/// transform, so unequip can restore them. Inserted **on the Tool entity**
/// (not the Handle) by [`weld_equipped_tools_system`] when it performs the
/// weld; consumed (removed) on unequip. Keyed on the Tool so each unequip
/// undoes exactly its own weld.
#[derive(Component, Debug, Clone)]
pub struct WeldRestore {
    /// The Handle part that was re-parented.
    pub handle: Entity,
    /// The Handle's parent before equipping (the Tool entity, typically).
    pub original_parent: Option<Entity>,
    /// The Handle's local transform before equipping.
    pub original_transform: Transform,
}

/// The local player's tool hotbar — equipped tools in slot order, plus the
/// currently-selected slot. Slots map to number keys `1`..=`9`.
#[derive(Resource, Default)]
pub struct Hotbar {
    /// Equipped/available tools in slot order. Rebuilt each frame from the
    /// character's child Tools so it stays in sync with the instance tree.
    pub slots: Vec<Entity>,
    /// The currently-equipped slot index, or `None` (nothing equipped).
    pub selected: Option<usize>,
}

// ─────────────────────────────────────────────────────────────────────────
// Hotbar selection
// ─────────────────────────────────────────────────────────────────────────

/// Number-key (1–9) hotbar selection among the character's child [`Tool`]s.
///
/// Rebuilds [`Hotbar::slots`] from the Tools currently parented under the
/// character (Roblox keeps equippable Tools in the character or its Backpack;
/// here we treat the character's direct Tool children as the toolbar). On a
/// number-key press, equips the chosen Tool (inserting [`Equipped`]) and
/// unequips the rest (removing [`Equipped`]).
pub fn hotbar_select_system(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    local: LocalCharacter,
    children: Query<&Children>,
    tools: Query<Entity, With<Tool>>,
    equipped: Query<(), With<Equipped>>,
    mut hotbar: ResMut<Hotbar>,
) {
    let Some(character) = local.entity() else {
        hotbar.slots.clear();
        hotbar.selected = None;
        return;
    };

    // Rebuild slots from the character's direct Tool children.
    hotbar.slots.clear();
    if let Ok(kids) = children.get(character) {
        for child in kids.iter() {
            if tools.get(child).is_ok() {
                hotbar.slots.push(child);
            }
        }
    }

    // Map number keys 1..=9 to slot indices 0..=8.
    const NUMBER_KEYS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];

    let mut requested: Option<usize> = None;
    for (slot, key) in NUMBER_KEYS.iter().enumerate() {
        if keys.just_pressed(*key) {
            requested = Some(slot);
            break;
        }
    }

    let Some(slot) = requested else { return };
    if slot >= hotbar.slots.len() {
        return; // empty slot — ignore
    }

    // Toggle: pressing the selected slot again unequips it (Roblox behavior).
    let target = hotbar.slots[slot];
    let already_equipped = equipped.get(target).is_ok();

    // Unequip everything first.
    for &tool in hotbar.slots.iter() {
        if equipped.get(tool).is_ok() {
            commands.entity(tool).remove::<Equipped>();
        }
    }

    if already_equipped {
        hotbar.selected = None;
    } else {
        commands.entity(target).insert(Equipped { holder: character });
        hotbar.selected = Some(slot);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Equip weld upkeep (runs in all states)
// ─────────────────────────────────────────────────────────────────────────

/// Performs the Handle-weld on newly [`Equipped`] tools and restores it on
/// unequip.
///
/// Equip: find the Tool's `Handle` child (a child named "Handle", or the
/// first child with a mesh), re-parent it under the holder's right-hand limb
/// (or the holder root if there's no skeleton), and set its local transform
/// to the Tool's grip offset. Records a [`WeldRestore`] so unequip can undo.
///
/// Unequip (Tool lost [`Equipped`] but still has [`WeldRestore`]): re-parent
/// the Handle back to its original parent + transform, then drop the
/// [`WeldRestore`].
pub fn weld_equipped_tools_system(
    mut commands: Commands,
    // Newly equipped tools that haven't been welded yet.
    newly_equipped: Query<(Entity, &Equipped, &Tool), (Added<Equipped>, Without<WeldRestore>)>,
    // Tools that were unequipped but still carry a stale weld.
    mut removed_equipped: RemovedComponents<Equipped>,
    weld_restores: Query<&WeldRestore>,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    child_of: Query<&ChildOf>,
    transforms: Query<&Transform>,
    limbs: Query<(Entity, &CharacterLimb)>,
) {
    // ── Equip: weld the Handle ──────────────────────────────────────────
    for (tool_entity, equipped, tool) in newly_equipped.iter() {
        let holder = equipped.holder;

        // Resolve the Handle child of the Tool.
        let Some(handle) = find_handle(tool_entity, &children, &names, &meshes) else {
            // requires_handle Tools with no Handle can't weld; that's a
            // soft failure (Roblox warns). Skip silently for non-handle tools.
            if tool.requires_handle {
                tracing::warn!(
                    "[equip] Tool {:?} requires a Handle child but none found — not welded",
                    tool_entity
                );
            }
            continue;
        };

        // Record restore info ON THE TOOL (so unequip undoes exactly this
        // tool's weld) before mutating the Handle.
        let original_parent = child_of.get(handle).ok().map(|c| c.parent());
        let original_transform = transforms.get(handle).copied().unwrap_or_default();
        commands.entity(tool_entity).insert(WeldRestore {
            handle,
            original_parent,
            original_transform,
        });

        // Find the holder's right-hand limb; fall back to the holder root.
        let hand = find_right_hand(holder, &limbs, &children).unwrap_or(holder);

        // Re-parent the Handle under the hand and apply the grip offset.
        commands
            .entity(handle)
            .insert(ChildOf(hand))
            .insert(Transform::from_translation(tool.grip_pos));

        tracing::debug!(
            "[equip] welded Handle {:?} of Tool {:?} to hand {:?}",
            handle,
            tool_entity,
            hand
        );

        // TODO(accessory): the Accessory path welds its Handle to a *named*
        // character Attachment (the `attachment_point` field) rather than the
        // right hand. That needs an Attachment-by-name resolver over the
        // character subtree; wire it when the Attachment lookup lands.
    }

    // ── Unequip: restore the weld of each Tool that lost `Equipped` ─────
    for tool_entity in removed_equipped.read() {
        // The WeldRestore lives on THIS tool — look up its own record so we
        // undo exactly this tool's weld (not every equipped tool's).
        let Ok(restore) = weld_restores.get(tool_entity) else {
            continue; // tool had no active weld (e.g. requires_handle = false)
        };
        // Re-parent the Handle back to where it was before equipping.
        if let Ok(mut handle_cmds) = commands.get_entity(restore.handle) {
            match restore.original_parent {
                Some(parent) => {
                    handle_cmds.insert(ChildOf(parent));
                }
                None => {
                    handle_cmds.remove::<ChildOf>();
                }
            }
            handle_cmds.insert(restore.original_transform);
        }
        // Drop the now-stale weld record from the Tool (if it still exists).
        if let Ok(mut tool_cmds) = commands.get_entity(tool_entity) {
            tool_cmds.remove::<WeldRestore>();
        }
    }
}

/// Find a Tool's `Handle` child: prefer a child named "Handle", else the
/// first child carrying a mesh. Returns `None` if the Tool has no children.
fn find_handle(
    tool: Entity,
    children: &Query<&Children>,
    names: &Query<&Name>,
    meshes: &Query<(), With<Mesh3d>>,
) -> Option<Entity> {
    let kids = children.get(tool).ok()?;
    // Named "Handle" wins.
    for child in kids.iter() {
        if let Ok(name) = names.get(child) {
            if name.as_str() == "Handle" {
                return Some(child);
            }
        }
    }
    // Else first child with a renderable mesh.
    for child in kids.iter() {
        if meshes.get(child).is_ok() {
            return Some(child);
        }
    }
    // Else first child at all.
    kids.iter().next()
}

/// Find the holder's right-hand limb entity. Checks the holder itself and its
/// descendants for a [`CharacterLimb::RightHand`]; returns `None` when the
/// character has no skeletal hand limb (simplified humanoid).
fn find_right_hand(
    holder: Entity,
    limbs: &Query<(Entity, &CharacterLimb)>,
    descendants: &Query<&Children>,
) -> Option<Entity> {
    // BFS over the holder's subtree looking for the RightHand limb.
    let mut stack = vec![holder];
    let mut visited = 0;
    while let Some(entity) = stack.pop() {
        visited += 1;
        if visited > 256 {
            break; // safety cap
        }
        if let Ok((e, limb)) = limbs.get(entity) {
            if matches!(limb, CharacterLimb::RightHand) {
                return Some(e);
            }
        }
        if let Ok(kids) = descendants.get(entity) {
            for child in kids.iter() {
                stack.push(child);
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────
// Activation
// ─────────────────────────────────────────────────────────────────────────

/// Fires the equipped Tool's `Activated` event on left-click.
///
/// Mirrors Roblox `Tool.Activated`: when a Tool is equipped and the player
/// clicks (and the Tool is `enabled` and not `manual_activation_only`), the
/// `Activated` event fires. Scripts connect to it to perform the tool's
/// action (swing, shoot, …).
///
/// Events:
/// - per-entity: `Tool.<entity_bits>.Activated`
/// - aggregate:  `Tool.Activated`
///
/// Payload: `[EntityId(tool), EntityId(holder)]`.
pub fn tool_activate_system(
    mouse: Res<ButtonInput<MouseButton>>,
    equipped_tools: Query<(Entity, &Equipped, &Tool)>,
    bus: Res<EventBusResource>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (tool_entity, equipped, tool) in equipped_tools.iter() {
        if !tool.enabled || tool.manual_activation_only {
            continue;
        }
        let payload = vec![
            SignalArg::EntityId(tool_entity.to_bits()),
            SignalArg::EntityId(equipped.holder.to_bits()),
        ];
        bus.0.fire(
            &format!("Tool.{}.Activated", tool_entity.to_bits()),
            payload.clone(),
        );
        bus.0.fire("Tool.Activated", payload);
        tracing::debug!("[Tool] Activated {:?}", tool_entity);
    }
}
