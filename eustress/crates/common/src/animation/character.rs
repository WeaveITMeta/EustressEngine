//! # The local avatar in the tree
//!
//! One binder for both shells puts the local avatar into the tree as the
//! local player's `Character`: a Model named after the player, holding
//! `HumanoidRootPart` (bound to the avatar body), `Head` and `Humanoid` with
//! its `Animator`, the Space's character scripts and the default `Animate`.
//! Studio runs it on its Play tree; the Player runs it on its own tree. Each
//! frame it follows the avatar's pose and reports the Humanoid's state. In a
//! host's session a Player builds no character: it follows the one the host
//! replicates in, and only reports its Humanoid's state.

use crate::datamodel::{DataModel, DmEvent, DmValue, InstanceId};
use crate::scripting::{CFrame, Vector3};

use super::animate::populate_character_scripts;
use super::humanoid::{report_state, LocomotionSample};

/// What a shell knows about its local avatar this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct AvatarSnapshot {
    /// The avatar entity's bits: `HumanoidRootPart` is bound to it.
    pub body: u64,
    /// The capsule centre's pose.
    pub root: CFrame,
    /// Half the capsule's height, metres.
    pub half_height: f64,
    /// The eyes above the capsule centre, metres.
    pub eye_offset: f64,
    pub walk_speed: f64,
    pub run_speed: f64,
    pub jump_height: f64,
    /// The take-off speed, m/s, when the avatar jumps by speed rather than by
    /// height (`UseJumpPower`).
    pub jump_power: Option<f64>,
    /// The body's stride over the stride its clips were authored for.
    pub stride_scale: f64,
    /// The movement switches (`JumpEnabled`, `ClimbingEnabled`, ...).
    pub abilities: Vec<(&'static str, bool)>,
    pub move_direction: Vector3,
    pub locomotion: LocomotionSample,
}

/// The local player's character, bound to an avatar.
#[derive(Debug, Clone)]
pub struct BoundCharacter {
    pub body: u64,
    pub model: InstanceId,
    pub humanoid: InstanceId,
    /// This binder built it. A character someone else built, such as the one
    /// a host replicates in, is only followed for its Humanoid's state: it is
    /// never moved or retired here.
    pub owned: bool,
}

/// What every character gets besides its parts, whoever builds it: the
/// Humanoid's `Animator`, as in Roblox; the `RunSpeed` and `StrideScale`
/// attributes the default `Animate` blends and paces by; and the Space's
/// character scripts, or the default `Animate`. Call it before
/// `CharacterAdded`, so the scripts are in place when handlers look.
pub fn furnish_character(dm: &mut DataModel, model: InstanceId, humanoid: InstanceId, run_speed: f64, stride_scale: f64) {
    dm.seed_attribute(humanoid, "RunSpeed", DmValue::Number(run_speed));
    dm.seed_attribute(humanoid, "StrideScale", DmValue::Number(stride_scale));
    if dm.find_first_child_of_class(humanoid, "Animator", false).is_none() {
        dm.create_virtual("Animator", "Animator", Some(humanoid));
    }
    populate_character_scripts(dm, model);
}

/// Make `player`'s character for `avatar`. Fires `CharacterAdded` once its
/// scripts are in place. `None` when the tree has no Workspace yet.
pub fn bind_character(dm: &mut DataModel, player: InstanceId, avatar: &AvatarSnapshot) -> Option<BoundCharacter> {
    let ws = dm.find_service("Workspace")?;
    let name = dm.name_of(player).unwrap_or("Player").to_string();
    let model = dm.create_virtual("Model", &name, Some(ws));
    let root = dm.create_bound(
        "Part",
        "HumanoidRootPart",
        avatar.body,
        Some(model),
        vec![
            ("CFrame".into(), DmValue::CFrame(avatar.root)),
            ("Size".into(), DmValue::Vector3(Vector3::new(0.6, avatar.half_height * 2.0, 0.4))),
            ("Transparency".into(), DmValue::Number(1.0)),
            ("CanCollide".into(), DmValue::Bool(true)),
            ("Anchored".into(), DmValue::Bool(false)),
        ],
    );
    let head = dm.create_virtual("Part", "Head", Some(model));
    dm.set_prop_from_engine(head, "CFrame", DmValue::CFrame(head_cframe(avatar)));
    dm.set_prop_from_engine(head, "Size", DmValue::Vector3(Vector3::new(0.3, 0.3, 0.3)));
    dm.set_prop_from_engine(head, "Transparency", DmValue::Number(1.0));

    let humanoid = dm.create_virtual("Humanoid", "Humanoid", Some(model));
    dm.set_prop_from_engine(humanoid, "WalkSpeed", DmValue::Number(avatar.walk_speed));
    dm.set_prop_from_engine(humanoid, "JumpHeight", DmValue::Number(avatar.jump_height));
    dm.set_prop_from_engine(humanoid, "UseJumpPower", DmValue::Bool(avatar.jump_power.is_some()));
    if let Some(power) = avatar.jump_power {
        dm.set_prop_from_engine(humanoid, "JumpPower", DmValue::Number(power));
    }
    for (property, on) in &avatar.abilities {
        dm.set_prop_from_engine(humanoid, property, DmValue::Bool(*on));
    }
    dm.set_prop_from_engine(model, "PrimaryPart", DmValue::Instance(root));
    furnish_character(dm, model, humanoid, avatar.run_speed, avatar.stride_scale);
    dm.set_prop_from_engine(player, "Character", DmValue::Instance(model));
    // Setup is not a script write: nothing for the engine to apply.
    let _ = dm.take_dirty_of(model);
    let _ = dm.take_dirty_of(player);
    dm.push_event(DmEvent::CharacterAdded { player, character: model });
    Some(BoundCharacter { body: avatar.body, model, humanoid, owned: true })
}

/// Follow the character someone else built for `player` (a host's), on the
/// local avatar `body`: its Humanoid reports the avatar's state, so the
/// player's own Animate plays by it. `None` while it has no Humanoid.
pub fn adopt_character(dm: &DataModel, player: InstanceId, body: u64) -> Option<BoundCharacter> {
    let model = match dm.get_prop(player, "Character") {
        Some(DmValue::Instance(c)) if dm.exists(c) => c,
        _ => return None,
    };
    let humanoid = dm.find_first_child_of_class(model, "Humanoid", false)?;
    Some(BoundCharacter { body, model, humanoid, owned: false })
}

/// Follow the avatar: `HumanoidRootPart` and `Head`, `MoveDirection`, and the
/// Humanoid's state and signals.
pub fn update_character(dm: &mut DataModel, bound: &BoundCharacter, avatar: &AvatarSnapshot) {
    if bound.owned {
        place_parts(dm, bound, avatar);
    }
    if !dm.exists(bound.humanoid) {
        return;
    }
    let d = avatar.move_direction;
    dm.set_prop_from_engine(bound.humanoid, "MoveDirection", DmValue::Vector3(Vector3::new(d.x, 0.0, d.z)));
    follow_run_speed(dm, bound.humanoid, avatar.run_speed);
    let dead = dm.get_prop(bound.humanoid, "Health").and_then(|v| v.as_number()).is_some_and(|h| h <= 0.0);
    let sample = LocomotionSample { dead: avatar.locomotion.dead || dead, ..avatar.locomotion };
    report_state(dm, bound.humanoid, sample);
}

/// The Humanoid's `RunSpeed`: the avatar's running pace, capped at a human
/// sprint, which a script's `WalkSpeed` moves. Kept current each frame, as
/// `WalkSpeed` is, so the gait blends toward the run the avatar can reach.
/// It is rewritten only when the pace moves by more than 1 mm/s, and as an
/// engine value: no attribute signal fires and nothing is marked to send.
pub fn follow_run_speed(dm: &mut DataModel, humanoid: InstanceId, run_speed: f64) {
    let current = dm.get_attribute(humanoid, "RunSpeed").and_then(|v| v.as_number());
    if current.map_or(true, |c| (c - run_speed).abs() > 1e-3) {
        dm.seed_attribute(humanoid, "RunSpeed", DmValue::Number(run_speed));
    }
}

/// `HumanoidRootPart` and `Head` where the avatar is.
fn place_parts(dm: &mut DataModel, bound: &BoundCharacter, avatar: &AvatarSnapshot) {
    if let Some(root) = dm.find_first_child(bound.model, "HumanoidRootPart", false) {
        dm.set_prop_from_engine(root, "CFrame", DmValue::CFrame(avatar.root));
    }
    if let Some(head) = dm.find_first_child(bound.model, "Head", false) {
        dm.set_prop_from_engine(head, "CFrame", DmValue::CFrame(head_cframe(avatar)));
    }
}

/// The avatar went away, or another character took this one's place:
/// `CharacterRemoving`, then the model goes. `Character` clears only while it
/// still names this model, so a newer character keeps its place.
pub fn retire_character(dm: &mut DataModel, player: InstanceId, bound: &BoundCharacter) {
    if let Some(root) = dm.find_first_child(bound.model, "HumanoidRootPart", false) {
        if dm.entity_of(root) == Some(bound.body) {
            dm.unbind_entity(root);
        }
    }
    dm.push_event(DmEvent::CharacterRemoving { player, character: bound.model });
    dm.destroy(bound.model);
    if dm.get_prop(player, "Character") == Some(DmValue::Instance(bound.model)) {
        dm.set_prop_from_engine(player, "Character", DmValue::Nil);
        let _ = dm.take_dirty_of(player);
    }
}

/// Whether `bound` is still `player`'s character. One this binder built stays
/// until another model takes its place, such as the one a host replicates in;
/// one someone else built is followed only while it is the player's.
fn still_current(dm: &DataModel, player: InstanceId, bound: &BoundCharacter) -> bool {
    if !dm.exists(bound.model) {
        return false;
    }
    match dm.get_prop(player, "Character") {
        Some(DmValue::Instance(c)) if dm.exists(c) => c == bound.model,
        _ => bound.owned,
    }
}

/// Whether this machine builds its own player's character. A Player in a
/// host's session never does: the host builds every character, and this
/// player's arrives with the world, so a second one here would put two
/// characters and two Animators on one avatar.
fn builds_characters(dm: &DataModel) -> bool {
    dm.is_server || !dm.networked
}

fn head_cframe(avatar: &AvatarSnapshot) -> CFrame {
    let mut cf = avatar.root;
    cf.position = avatar.root.position + Vector3::new(0.0, avatar.eye_offset, 0.0);
    cf
}

#[cfg(feature = "physics")]
mod system {
    use bevy::prelude::*;
    use tracing::info;

    use super::{
        adopt_character, bind_character, builds_characters, retire_character, still_current, update_character,
        AvatarSnapshot, BoundCharacter,
    };
    use crate::animation::humanoid::LocomotionSample;
    use crate::animation::LiveTree;
    use crate::avatar::abilities::AvatarAbilities;
    use crate::avatar::climb::AvatarClimb;
    use crate::avatar::spawn::{AvatarBody, AvatarIntent, AvatarLocomotion};
    use crate::avatar::LocalAvatar;
    use crate::datamodel::DmValue;
    use crate::scripting::{CFrame, Vector3};

    /// An avatar's movement this frame, as a Humanoid's state reads it. Every
    /// attached climb phase is Climbing.
    pub fn locomotion_sample(loco: &AvatarLocomotion, climb: Option<&AvatarClimb>) -> LocomotionSample {
        LocomotionSample {
            grounded: loco.grounded,
            planar_speed: loco.planar_speed,
            vertical_velocity: loco.vertical_velocity,
            climbing: climb.is_some_and(|c| c.is_climbing()),
            climb_speed: 0.0,
            seated: loco.seated,
            swimming: false,
            dead: false,
        }
    }

    /// Keep the local player's `Character` bound to the local avatar, on
    /// whichever tree this shell animates from.
    #[allow(clippy::type_complexity)]
    pub fn sync_local_character(
        tree: Option<Res<LiveTree>>,
        avatars: Query<
            (
                Entity,
                &Transform,
                &AvatarBody,
                &AvatarIntent,
                &AvatarLocomotion,
                Option<&AvatarAbilities>,
                Option<&AvatarClimb>,
            ),
            With<LocalAvatar>,
        >,
        mut bound: Local<Option<BoundCharacter>>,
        mut session: Local<usize>,
    ) {
        let Some(tree) = tree else {
            *bound = None;
            return;
        };
        // A new tree is a new session: a character id from the last one
        // must not carry over.
        let id = std::sync::Arc::as_ptr(&tree.dm) as usize;
        if *session != id {
            *session = id;
            *bound = None;
        }
        let mut g = tree.dm.lock();
        let Some(player) = g.local_player else { return };

        // Keep the body the character is bound to while it exists.
        let avatar = bound
            .as_ref()
            .and_then(|b| avatars.iter().find(|(e, ..)| e.to_bits() == b.body))
            .or_else(|| avatars.iter().next());

        if let Some(b) = bound.as_ref() {
            let same = avatar.as_ref().is_some_and(|(e, ..)| e.to_bits() == b.body);
            if !same || !still_current(&g, player, b) {
                if b.owned && g.exists(b.model) {
                    retire_character(&mut g, player, b);
                }
                *bound = None;
            }
        }
        let Some((entity, tf, body, intent, loco, abilities, climb)) = avatar else { return };
        if bound.is_none() {
            // The character arrives once the player has joined, so
            // `CharacterAdded` reaches the handlers `PlayerAdded` connected.
            if !g.in_tree(player) {
                return;
            }
            // A character someone else made, such as the one a host
            // replicates in, is theirs: follow it, never build a second.
            if let Some(DmValue::Instance(c)) = g.get_prop(player, "Character") {
                if g.exists(c) {
                    *bound = adopt_character(&g, player, entity.to_bits());
                    if bound.is_none() {
                        return;
                    }
                    info!("animation: the local player follows the character the host built, on its avatar ({entity:?})");
                }
            }
            // In a host's session the character is the host's: wait for it.
            if bound.is_none() && !builds_characters(&g) {
                return;
            }
        }

        let half = body.metrics.capsule_half_extent() as f64;
        let mut root = CFrame::from_quaternion([
            tf.rotation.x as f64,
            tf.rotation.y as f64,
            tf.rotation.z as f64,
            tf.rotation.w as f64,
        ]);
        root.position = Vector3::from_vec3(tf.translation);
        let abilities = abilities.copied().unwrap_or_default();
        let snapshot = AvatarSnapshot {
            body: entity.to_bits(),
            root,
            half_height: half,
            eye_offset: body.metrics.eye_height as f64 - half,
            walk_speed: body.motion.walk_speed as f64,
            run_speed: body.motion.capped_run_and_sprint().0 as f64,
            jump_height: body.motion.jump_apex_m as f64,
            jump_power: body.motion.jump_speed_mps.map(f64::from),
            stride_scale: body.metrics.stride_scale as f64,
            abilities: AvatarAbilities::PROPERTIES.iter().map(|p| (*p, abilities.get(p).unwrap_or(true))).collect(),
            move_direction: Vector3::new(intent.direction.x as f64, 0.0, intent.direction.z as f64),
            locomotion: locomotion_sample(loco, climb),
        };

        if bound.is_none() {
            *bound = bind_character(&mut g, player, &snapshot);
            if bound.is_some() {
                info!("animation: the local player's character is bound to its avatar ({entity:?})");
            }
        }
        if let Some(b) = bound.as_ref() {
            update_character(&mut g, b, &snapshot);
        }
    }
}

#[cfg(feature = "physics")]
pub use system::{locomotion_sample, sync_local_character};

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> AvatarSnapshot {
        AvatarSnapshot {
            body: 42,
            root: CFrame::new(0.0, 1.0, 0.0),
            half_height: 0.9,
            eye_offset: 0.7,
            walk_speed: 1.45,
            run_speed: 3.9,
            jump_height: 1.2,
            jump_power: None,
            stride_scale: 1.0,
            abilities: vec![("JumpEnabled", true)],
            move_direction: Vector3::ZERO,
            locomotion: LocomotionSample { grounded: true, ..Default::default() },
        }
    }

    #[test]
    fn a_bound_character_has_an_animator_and_animate_before_character_added() {
        let mut dm = DataModel::new();
        dm.get_service("Workspace").unwrap();
        let players = dm.get_service("Players").unwrap();
        let player = dm.create_virtual("Player", "Ada", Some(players));
        dm.local_player = Some(player);
        let bound = bind_character(&mut dm, player, &snapshot()).expect("bound");
        let humanoid = dm.find_first_child(bound.model, "Humanoid", false).unwrap();
        assert!(dm.find_first_child_of_class(humanoid, "Animator", false).is_some());
        assert!(dm.find_first_child(bound.model, "Animate", false).is_some());
        assert_eq!(dm.get_prop(player, "Character"), Some(DmValue::Instance(bound.model)));
        assert_eq!(dm.get_attribute(humanoid, "StrideScale"), Some(DmValue::Number(1.0)));
        let added = dm
            .events_since(0)
            .0
            .into_iter()
            .any(|e| matches!(e, DmEvent::CharacterAdded { character, .. } if character == bound.model));
        assert!(added);
    }

    #[test]
    fn updates_report_running_and_retire_removes_the_model() {
        let mut dm = DataModel::new();
        dm.get_service("Workspace").unwrap();
        let players = dm.get_service("Players").unwrap();
        let player = dm.create_virtual("Player", "Ada", Some(players));
        let bound = bind_character(&mut dm, player, &snapshot()).unwrap();
        let mut s = snapshot();
        s.locomotion.planar_speed = 2.0;
        update_character(&mut dm, &bound, &s);
        let running = dm.events_since(0).0.into_iter().any(|e| {
            matches!(&e, DmEvent::Signal { id, name, .. } if *id == bound.humanoid && name == "Running")
        });
        assert!(running);
        assert_eq!(
            dm.get_prop(bound.humanoid, super::super::humanoid::STATE_PROPERTY),
            Some(DmValue::String("Running".into()))
        );
        retire_character(&mut dm, player, &bound);
        assert!(!dm.exists(bound.model));
        assert_eq!(dm.get_prop(player, "Character"), Some(DmValue::Nil));
    }

    #[test]
    fn a_newer_character_takes_the_place_of_one_the_binder_built() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let players = dm.get_service("Players").unwrap();
        let player = dm.create_virtual("Player", "Ada", Some(players));
        let own = bind_character(&mut dm, player, &snapshot()).unwrap();
        assert!(still_current(&dm, player, &own));
        // A script clearing `Character` leaves the built one in place.
        dm.set_prop_from_engine(player, "Character", DmValue::Nil);
        assert!(still_current(&dm, player, &own));

        let host = dm.create_virtual("Model", "Ada", Some(ws));
        dm.create_virtual("Humanoid", "Humanoid", Some(host));
        dm.set_prop_from_engine(player, "Character", DmValue::Instance(host));
        assert!(!still_current(&dm, player, &own));
        retire_character(&mut dm, player, &own);
        assert!(!dm.exists(own.model));
        assert_eq!(dm.get_prop(player, "Character"), Some(DmValue::Instance(host)), "the newer character keeps its place");
        let adopted = adopt_character(&dm, player, 42).expect("adopted");
        assert_eq!(adopted.model, host);
        assert!(still_current(&dm, player, &adopted));
    }

    #[test]
    fn only_a_player_in_a_hosts_session_leaves_characters_to_the_host() {
        let mut dm = DataModel::new();
        assert!(builds_characters(&dm), "a Space playing on its own");
        dm.networked = true;
        assert!(builds_characters(&dm), "the host");
        dm.is_server = false;
        assert!(!builds_characters(&dm), "a Player in a host's session");
        dm.networked = false;
        assert!(builds_characters(&dm), "a Player playing on its own");
    }

    #[test]
    fn a_host_built_character_reports_state_and_is_never_moved() {
        let mut dm = DataModel::new();
        let players = dm.get_service("Players").unwrap();
        let player = dm.create_virtual("Player", "Ada", Some(players));
        let ws = dm.get_service("Workspace").unwrap();
        let model = dm.create_virtual("Model", "Ada", Some(ws));
        let root = dm.create_virtual("Part", "HumanoidRootPart", Some(model));
        let at = DmValue::CFrame(CFrame::new(5.0, 1.0, 5.0));
        dm.set_prop_from_engine(root, "CFrame", at.clone());
        let humanoid = dm.create_virtual("Humanoid", "Humanoid", Some(model));
        dm.set_prop_from_engine(player, "Character", DmValue::Instance(model));

        let bound = adopt_character(&dm, player, 42).expect("adopted");
        assert!(!bound.owned);
        let mut s = snapshot();
        s.locomotion.planar_speed = 1.2;
        update_character(&mut dm, &bound, &s);
        assert_eq!(dm.get_prop(root, "CFrame"), Some(at), "the host places its own character");
        assert_eq!(
            dm.get_prop(humanoid, super::super::humanoid::STATE_PROPERTY),
            Some(DmValue::String("Running".into()))
        );
    }
}
