//! ProximityPrompt runtime — distance/line-of-sight gating + hold-to-trigger.
//!
//! Roblox `ProximityPrompt` shows a contextual hold-to-interact UI when the
//! local character is within `max_activation_distance` of the prompt's parent
//! (with optional line-of-sight), and fires `Triggered` once the configured
//! key is held for `hold_duration`.
//!
//! ## Systems
//!
//! - [`proximity_prompt_system`] — each frame, finds the nearest in-range
//!   prompt (respecting `requires_line_of_sight` via a raycast from the
//!   character toward the prompt), records it in [`ActivePrompt`], and tracks
//!   the hold timer. When the hold completes it fires `Triggered` on the
//!   [`EventBus`].
//! - [`proximity_overlay_system`] — renders/updates a minimal screen-space
//!   prompt overlay (action text + object text + hold progress) for the
//!   active prompt, and hides it when none is active.
//!
//! ## Anchor position
//!
//! A prompt's world position is its own [`GlobalTransform`] if it has one,
//! else its parent's (Roblox anchors the prompt to its parent part). The
//! system reads the prompt entity's `GlobalTransform`; the spawner attaches a
//! `Transform`, and Bevy's transform-propagation gives a `GlobalTransform`
//! that follows the parent when parented.
//!
//! ## Events
//!
//! `Triggered` fires on the [`EventBus`]:
//! - per-entity: `ProximityPrompt.<entity_bits>.Triggered`
//! - aggregate:  `ProximityPrompt.Triggered`
//!
//! Payload: `[EntityId(prompt), EntityId(character)]`. Roblox also has
//! `PromptButtonHoldBegan` / `Ended`; those are scaffolded as TODO.
//!
//! ## No-op safety
//!
//! Resolves the local character via [`LocalCharacter`] and returns early when
//! absent. The overlay system tolerates a missing UI root (it spawns one on
//! demand under a dedicated marker).
//!
//! [`EventBus`]: eustress_common::events::EventBus

use bevy::prelude::*;

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};

use eustress_common::classes::ProximityPrompt;
use eustress_common::events::EventBusResource;
use eustress_common::scripting::events::SignalArg;

use super::LocalCharacter;

/// The prompt the character is currently closest-to-and-in-range-of, plus
/// the accumulated hold time. Reset whenever the active prompt changes.
#[derive(Resource, Default)]
pub struct ActivePrompt {
    /// The active prompt entity, or `None`.
    pub prompt: Option<Entity>,
    /// Seconds the trigger key has been held for the active prompt.
    pub held_for: f32,
    /// Whether `Triggered` has already fired for this hold (debounce so a
    /// continued hold doesn't re-fire every frame).
    pub fired: bool,
}

/// Marker on the on-demand prompt-overlay UI root.
#[derive(Component)]
pub struct ProximityOverlayRoot;

/// Marker on the overlay's action-text node (updated each frame).
#[derive(Component)]
pub struct ProximityOverlayText;

/// Parse a `ProximityPrompt.keyboard_key_code` string (e.g. `"E"`, `"F"`,
/// `"Space"`) into a Bevy [`KeyCode`]. Falls back to `KeyE` (the Roblox
/// default) for unrecognised values.
fn key_code_from_str(s: &str) -> KeyCode {
    match s.trim().to_ascii_uppercase().as_str() {
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "Q" => KeyCode::KeyQ,
        "R" => KeyCode::KeyR,
        "T" => KeyCode::KeyT,
        "G" => KeyCode::KeyG,
        "X" => KeyCode::KeyX,
        "C" => KeyCode::KeyC,
        "V" => KeyCode::KeyV,
        "SPACE" => KeyCode::Space,
        "RETURN" | "ENTER" => KeyCode::Enter,
        _ => KeyCode::KeyE,
    }
}

/// Per-frame distance + line-of-sight gating, hold-timer tracking, and
/// `Triggered` firing.
pub fn proximity_prompt_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    spatial: SpatialQuery,
    prompts: Query<(Entity, &ProximityPrompt, &GlobalTransform)>,
    transforms: Query<&GlobalTransform>,
    local: LocalCharacter,
    bus: Res<EventBusResource>,
    mut active: ResMut<ActivePrompt>,
) {
    // No local character ⇒ clear state and bail.
    let Some(character) = local.entity() else {
        active.prompt = None;
        active.held_for = 0.0;
        active.fired = false;
        return;
    };
    let Ok(char_transform) = transforms.get(character) else {
        return;
    };
    let char_pos = char_transform.translation();

    // Find the nearest in-range prompt (with line-of-sight if required).
    let mut best: Option<(Entity, f32, ProximityPrompt)> = None;
    for (entity, prompt, gt) in prompts.iter() {
        let prompt_pos = gt.translation();
        let dist = char_pos.distance(prompt_pos);
        if dist > prompt.max_activation_distance {
            continue;
        }
        if prompt.requires_line_of_sight
            && !has_line_of_sight(char_pos, prompt_pos, character, entity, &spatial)
        {
            continue;
        }
        if best.as_ref().map_or(true, |(_, best_dist, _)| dist < *best_dist) {
            best = Some((entity, dist, prompt.clone()));
        }
    }

    let Some((prompt_entity, _dist, prompt)) = best else {
        // Nothing in range — reset.
        if active.prompt.is_some() {
            active.prompt = None;
            active.held_for = 0.0;
            active.fired = false;
        }
        return;
    };

    // Active-prompt switch resets the hold timer.
    if active.prompt != Some(prompt_entity) {
        active.prompt = Some(prompt_entity);
        active.held_for = 0.0;
        active.fired = false;
    }

    let key = key_code_from_str(&prompt.keyboard_key_code);

    // Instant prompts (hold_duration <= 0) fire on key just-pressed.
    if prompt.hold_duration <= 0.0 {
        if keys.just_pressed(key) {
            fire_triggered(&bus, prompt_entity, character);
        }
        return;
    }

    // Hold prompts accumulate while the key is down.
    if keys.pressed(key) {
        active.held_for += time.delta_secs();
        if active.held_for >= prompt.hold_duration && !active.fired {
            fire_triggered(&bus, prompt_entity, character);
            active.fired = true;
        }
    } else {
        // Key released before completion — reset the hold (no re-fire until
        // the next full hold).
        active.held_for = 0.0;
        active.fired = false;
    }
}

/// Raycast from the character toward the prompt; true if nothing solid blocks
/// the line (other than the character or the prompt's own anchor). A short
/// tolerance is subtracted from the distance so the prompt's own collider
/// doesn't count as an occluder.
fn has_line_of_sight(
    from: Vec3,
    to: Vec3,
    character: Entity,
    prompt_anchor: Entity,
    spatial: &SpatialQuery,
) -> bool {
    let delta = to - from;
    let distance = delta.length();
    if distance < 1.0e-3 {
        return true;
    }
    let Ok(dir) = Dir3::new(delta / distance) else {
        return true;
    };
    let filter = SpatialQueryFilter::default()
        .with_excluded_entities([character, prompt_anchor]);
    // First solid hit before reaching the target ⇒ blocked.
    let hits = spatial.ray_hits(from, dir, distance - 0.05, 4, true, &filter);
    hits.is_empty()
}

/// Fire the `Triggered` event for `prompt` triggered by `character`.
fn fire_triggered(bus: &EventBusResource, prompt: Entity, character: Entity) {
    let payload = vec![
        SignalArg::EntityId(prompt.to_bits()),
        SignalArg::EntityId(character.to_bits()),
    ];
    bus.0.fire(
        &format!("ProximityPrompt.{}.Triggered", prompt.to_bits()),
        payload.clone(),
    );
    bus.0.fire("ProximityPrompt.Triggered", payload);
    tracing::debug!("[ProximityPrompt] Triggered {:?} by {:?}", prompt, character);
}

/// Renders / updates a minimal screen-space overlay for the active prompt.
/// Spawns the overlay root lazily on first need; hides it when no prompt is
/// active.
pub fn proximity_overlay_system(
    mut commands: Commands,
    active: Res<ActivePrompt>,
    prompts: Query<&ProximityPrompt>,
    // Combined root query — `&mut Visibility` + `&Children` in ONE query so
    // the two don't conflict on the same `ProximityOverlayRoot` archetype.
    mut root: Query<(&mut Visibility, &Children), With<ProximityOverlayRoot>>,
    // Text nodes carry a distinct marker (`ProximityOverlayText`), so this
    // query is disjoint from `root` (no aliasing conflict).
    mut text_nodes: Query<&mut Text, With<ProximityOverlayText>>,
) {
    // Resolve the active prompt's display strings (if any).
    let label = active
        .prompt
        .and_then(|e| prompts.get(e).ok())
        .map(|p| {
            let progress = if p.hold_duration > 0.0 {
                (active.held_for / p.hold_duration).clamp(0.0, 1.0)
            } else {
                0.0
            };
            format_prompt_label(p, progress)
        });

    // No overlay root yet — spawn one (hidden) and return; next frame fills it.
    if root.is_empty() {
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Percent(42.0),
                    left: Val::Percent(50.0),
                    ..default()
                },
                Visibility::Hidden,
                GlobalZIndex(120),
                ProximityOverlayRoot,
                Name::new("ProximityPromptOverlay"),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Text::new(""),
                    TextColor(Color::WHITE),
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                    Node {
                        padding: UiRect::all(Val::Px(8.0)),
                        ..default()
                    },
                    ProximityOverlayText,
                ));
            });
        return;
    }

    // Update visibility + text on the existing root.
    match label {
        Some(text) => {
            for (mut vis, children) in root.iter_mut() {
                *vis = Visibility::Inherited;
                // Update the text node(s) under this root.
                for child in children.iter() {
                    if let Ok(mut node_text) = text_nodes.get_mut(child) {
                        **node_text = text.clone();
                    }
                }
            }
        }
        None => {
            for (mut vis, _children) in root.iter_mut() {
                *vis = Visibility::Hidden;
            }
        }
    }
}

/// Compose the overlay's single-line label from a prompt + hold progress.
fn format_prompt_label(prompt: &ProximityPrompt, progress: f32) -> String {
    let key = prompt.keyboard_key_code.trim();
    let action = if prompt.object_text.is_empty() {
        prompt.action_text.clone()
    } else {
        format!("{} {}", prompt.action_text, prompt.object_text)
    };
    if prompt.hold_duration > 0.0 && progress > 0.0 {
        let pct = (progress * 100.0).round() as u32;
        format!("[{key}] {action}  ({pct}%)")
    } else {
        format!("[{key}] {action}")
    }
}
