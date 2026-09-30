//! # What a climber may take hold of
//!
//! Every climb probe asks the same question of whatever its ray meets: is it
//! something to hold, something in the way, or something that is not there as
//! far as climbing goes? The answer comes from here, so a new probe cannot
//! forget to ask it.
//!
//! ## The defaults
//!
//! A character takes hold of the world: anchored parts, terrain, and anything
//! else that stays where it is. It never takes hold of, by default,
//!
//! * another character: a player's avatar, or any Model holding a `Humanoid`
//!   or an `AnimationController` (an NPC, however it was built),
//! * anything that moves: an unanchored part, or any collider on a Dynamic or
//!   Kinematic body,
//! * an invisible part (a `Transparency` of [`INVISIBLE_TRANSPARENCY`] or
//!   more), which is there to stop people rather than to be climbed.
//!
//! Those are still solid. A climber cannot reach through a person to the wall
//! behind them, or grab a ledge on the far side of an invisible barrier.
//!
//! Two kinds of collider are not there at all: trigger volumes (a part with
//! `CanCollide` off is a `Sensor`), and the climber's own body with everything
//! it carries.
//!
//! ## Overriding
//!
//! A `Climbable` attribute, true or false, on a part or on any Model or Folder
//! above it overrides the defaults for everything inside, and the nearest one
//! wins. It makes a statue of a character climbable, or puts a building's
//! trim off limits. Other players' avatars stay solid whatever the attribute
//! says.
//!
//! A shell whose entities carry no `Attributes`, like the Player, records the
//! same decision on the entity as a [`Climbable`] component when it spawns the
//! part; on one entity the component wins over the attribute.

use avian3d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::attributes::{AttributeValue, Attributes};
use crate::classes::{AnimationController, BasePart, ClassName, Humanoid, Instance};

use super::SpawnedByAvatarRuntime;

/// The attribute an author sets to override the defaults.
pub use crate::attributes::CLIMBABLE_ATTRIBUTE;

/// Transparency at or above which a part counts as invisible.
pub const INVISIBLE_TRANSPARENCY: f32 = 0.95;

/// How far up the hierarchy the rules look for a character, an override or
/// the climber.
const MAX_DEPTH: usize = 32;

/// The author's choice for this entity and everything under it, recorded by a
/// loader for shells whose entities carry no `Attributes`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub struct Climbable(pub bool);

/// A Model that is a character: it holds a `Humanoid` or an
/// `AnimationController`, as a component or as the instance class a loaded
/// Space records. Kept on the Model by [`mark_character_models`], so the rules
/// can recognise a character's parts by walking up from a hit instead of
/// scanning every sibling.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CharacterModel;

/// What a climbing probe does with a collider it meets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Not there as far as climbing goes: a trigger volume, or the climber's
    /// own body and whatever it carries. Rays pass through it.
    Ignore,
    /// Solid but not for holding: another character, a moving part, an
    /// invisible wall. It stops a ray, and nothing behind it is reached.
    Solid,
    /// Solid, and a hand or a foot may use it.
    Hold,
}

impl Surface {
    /// Whether it stops a ray or takes up room.
    pub const fn blocks(self) -> bool {
        !matches!(self, Surface::Ignore)
    }

    /// Whether a hand or a foot may use it.
    pub const fn holds(self) -> bool {
        matches!(self, Surface::Hold)
    }
}

/// Answers [`Surface`] for any collider, from one climber's point of view.
pub trait SurfaceRule {
    fn surface(&self, collider: Entity) -> Surface;
}

/// Everything the rules know about one collider.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SurfaceFacts {
    /// A trigger volume.
    pub sensor: bool,
    /// Part of the climber doing the asking.
    pub own_body: bool,
    /// Part of another avatar.
    pub other_avatar: bool,
    /// The nearest `Climbable` override, if any.
    pub explicit: Option<bool>,
    /// Inside a Model with a `Humanoid` or an `AnimationController`.
    pub in_character: bool,
    /// On a Dynamic or Kinematic body.
    pub moves: bool,
    /// Transparent enough to be an invisible wall.
    pub invisible: bool,
}

/// The rules, as one decision from the facts.
pub fn decide(facts: &SurfaceFacts) -> Surface {
    if facts.sensor || facts.own_body {
        return Surface::Ignore;
    }
    if facts.other_avatar {
        return Surface::Solid;
    }
    match facts.explicit {
        Some(true) => Surface::Hold,
        Some(false) => Surface::Solid,
        None if facts.in_character || facts.moves || facts.invisible => Surface::Solid,
        None => Surface::Hold,
    }
}

/// The marker a loader puts on a part it spawns, for shells that keep no
/// `Attributes` or part classes on their entities. `moves` is whether the
/// part is unanchored, which such a shell may not show in the part's body.
/// `None` means the defaults already give the right answer from the body.
pub fn loader_mark(explicit: Option<bool>, in_character: bool, moves: bool, invisible: bool) -> Option<Climbable> {
    match explicit {
        Some(b) => Some(Climbable(b)),
        None if in_character || moves || invisible => Some(Climbable(false)),
        None => None,
    }
}

/// Read a `Climbable` attribute from an `Attributes` component.
pub fn attribute_override(attributes: &Attributes) -> Option<bool> {
    match attributes.get(CLIMBABLE_ATTRIBUTE) {
        Some(AttributeValue::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// The ECS reads behind the rules. Read-only, so any climb system can take it
/// alongside the queries that move the body.
#[derive(SystemParam)]
pub struct ClimbSurfaces<'w, 's> {
    parents: Query<'w, 's, &'static ChildOf>,
    sensors: Query<'w, 's, (), With<Sensor>>,
    attached: Query<'w, 's, &'static ColliderOf>,
    bodies: Query<'w, 's, &'static RigidBody>,
    avatars: Query<'w, 's, (), With<SpawnedByAvatarRuntime>>,
    characters: Query<'w, 's, (), With<CharacterModel>>,
    marks: Query<'w, 's, &'static Climbable>,
    attributes: Query<'w, 's, &'static Attributes>,
    parts: Query<'w, 's, &'static BasePart>,
}

impl<'w, 's> ClimbSurfaces<'w, 's> {
    /// The rules as `climber` sees them: its own body is not there.
    pub fn for_climber(&self, climber: Entity) -> ClimberSurfaces<'_, 'w, 's> {
        ClimberSurfaces { surfaces: self, climber }
    }

    /// Gather the facts about `collider` and decide.
    pub fn classify(&self, collider: Entity, climber: Entity) -> Surface {
        decide(&self.facts(collider, climber))
    }

    fn facts(&self, collider: Entity, climber: Entity) -> SurfaceFacts {
        let mut facts = SurfaceFacts {
            sensor: self.sensors.contains(collider),
            ..default()
        };
        if facts.sensor {
            return facts;
        }

        let mut at = collider;
        for _ in 0..MAX_DEPTH {
            if at == climber {
                facts.own_body = true;
                return facts;
            }
            if self.avatars.contains(at) {
                facts.other_avatar = true;
                return facts;
            }
            if facts.explicit.is_none() {
                facts.explicit = self
                    .marks
                    .get(at)
                    .map(|m| m.0)
                    .ok()
                    .or_else(|| self.attributes.get(at).ok().and_then(attribute_override));
            }
            facts.in_character |= self.characters.contains(at);
            let Ok(parent) = self.parents.get(at) else { break };
            at = parent.parent();
        }

        // A collider with no body of its own is part of the static world, the
        // way terrain chunks are.
        let body = self.attached.get(collider).map(|c| c.body).unwrap_or(collider);
        facts.moves = self
            .bodies
            .get(body)
            .is_ok_and(|b| !matches!(b, RigidBody::Static));
        facts.invisible = self
            .parts
            .get(collider)
            .is_ok_and(|p| p.transparency >= INVISIBLE_TRANSPARENCY);
        facts
    }
}

/// [`ClimbSurfaces`] bound to one climber.
pub struct ClimberSurfaces<'a, 'w, 's> {
    surfaces: &'a ClimbSurfaces<'w, 's>,
    climber: Entity,
}

impl SurfaceRule for ClimberSurfaces<'_, '_, '_> {
    fn surface(&self, collider: Entity) -> Surface {
        self.surfaces.classify(collider, self.climber)
    }
}

/// Whether an entity is a rig: a `Humanoid` or an `AnimationController`,
/// either as the component or as the class a loaded Space gives its instance.
fn is_rig(instance: Option<&Instance>, humanoid: bool, controller: bool) -> bool {
    humanoid
        || controller
        || instance.is_some_and(|i| matches!(i.class_name, ClassName::Humanoid | ClassName::AnimationController))
}

/// Keep [`CharacterModel`] on exactly the Models that hold a rig.
///
/// Only a Model is a character. A rig left directly in Workspace or in a
/// Folder, which imported places do carry, would otherwise make everything
/// beside it a character and the whole world unclimbable. An entity with no
/// instance class at all, as another shell spawns, is taken as the Model.
///
/// Marking the parent when a rig arrives is the whole job in the ordinary
/// case. When a rig leaves (removed, despawned or moved to another parent),
/// every marked Model is checked again, which is rare and touches only the
/// few Models that are characters.
#[allow(clippy::type_complexity)]
pub(crate) fn mark_character_models(
    mut commands: Commands,
    arrived: Query<
        (&ChildOf, Option<&Instance>, Has<Humanoid>, Has<AnimationController>),
        Or<(Changed<ChildOf>, Added<Instance>, Added<Humanoid>, Added<AnimationController>)>,
    >,
    mut lost_humanoids: RemovedComponents<Humanoid>,
    mut lost_controllers: RemovedComponents<AnimationController>,
    mut lost_instances: RemovedComponents<Instance>,
    marked: Query<(Entity, Option<&Children>), With<CharacterModel>>,
    rigs: Query<(Option<&Instance>, Has<Humanoid>, Has<AnimationController>)>,
    classes: Query<&Instance>,
) {
    let is_model = |e: Entity| classes.get(e).map_or(true, |i| i.class_name == ClassName::Model);
    let mut recheck = lost_humanoids.read().count()
        + lost_controllers.read().count()
        + lost_instances.read().count()
        > 0;
    for (parent, instance, humanoid, controller) in arrived.iter() {
        if is_rig(instance, humanoid, controller) && is_model(parent.parent()) {
            commands.entity(parent.parent()).try_insert(CharacterModel);
            // A rig that moved may have left a Model behind.
            recheck = true;
        }
    }
    if !recheck {
        return;
    }
    for (model, children) in marked.iter() {
        let still = children.is_some_and(|c| {
            c.iter().any(|child| rigs.get(child).is_ok_and(|(i, h, a)| is_rig(i, h, a)))
        });
        if !still {
            commands.entity(model).try_remove::<CharacterModel>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_part() -> SurfaceFacts {
        SurfaceFacts::default()
    }

    #[test]
    fn the_world_is_climbable() {
        assert_eq!(decide(&world_part()), Surface::Hold);
    }

    #[test]
    fn trigger_volumes_and_the_climber_are_not_there() {
        let sensor = SurfaceFacts { sensor: true, ..world_part() };
        let own = SurfaceFacts { own_body: true, ..world_part() };
        assert_eq!(decide(&sensor), Surface::Ignore);
        assert_eq!(decide(&own), Surface::Ignore);
        // Not even an override makes a trigger volume solid.
        let marked = SurfaceFacts { sensor: true, explicit: Some(true), ..world_part() };
        assert_eq!(decide(&marked), Surface::Ignore);
    }

    /// "I don't want to start climbing other characters or custom NPCs or
    /// anything out of the ordinary by default."
    #[test]
    fn characters_moving_parts_and_invisible_walls_are_solid_but_not_held() {
        for facts in [
            SurfaceFacts { other_avatar: true, ..world_part() },
            SurfaceFacts { in_character: true, ..world_part() },
            SurfaceFacts { moves: true, ..world_part() },
            SurfaceFacts { invisible: true, ..world_part() },
        ] {
            assert_eq!(decide(&facts), Surface::Solid, "{facts:?}");
            assert!(decide(&facts).blocks(), "a person or a crate is see-through: {facts:?}");
        }
    }

    #[test]
    fn the_attribute_overrides_the_defaults_both_ways() {
        let statue = SurfaceFacts { in_character: true, explicit: Some(true), ..world_part() };
        let crate_prop = SurfaceFacts { moves: true, explicit: Some(true), ..world_part() };
        let trim = SurfaceFacts { explicit: Some(false), ..world_part() };
        assert_eq!(decide(&statue), Surface::Hold);
        assert_eq!(decide(&crate_prop), Surface::Hold);
        assert_eq!(decide(&trim), Surface::Solid);
    }

    #[test]
    fn another_player_stays_solid_whatever_the_attribute_says() {
        let player = SurfaceFacts { other_avatar: true, explicit: Some(true), ..world_part() };
        assert_eq!(decide(&player), Surface::Solid);
    }

    #[test]
    fn a_loader_marks_only_what_the_body_cannot_tell() {
        assert_eq!(loader_mark(None, false, false, false), None, "an ordinary part needs no mark");
        assert_eq!(loader_mark(None, true, false, false), Some(Climbable(false)));
        assert_eq!(loader_mark(None, false, true, false), Some(Climbable(false)));
        assert_eq!(loader_mark(None, false, false, true), Some(Climbable(false)));
        assert_eq!(loader_mark(Some(true), true, true, true), Some(Climbable(true)));
        assert_eq!(loader_mark(Some(false), false, false, false), Some(Climbable(false)));
    }

    #[test]
    fn the_attribute_is_read_only_as_a_bool() {
        let mut a = Attributes::new();
        assert_eq!(attribute_override(&a), None);
        a.set(CLIMBABLE_ATTRIBUTE, AttributeValue::Bool(false));
        assert_eq!(attribute_override(&a), Some(false));
        a.set(CLIMBABLE_ATTRIBUTE, AttributeValue::String("yes".into()));
        assert_eq!(attribute_override(&a), None, "a string is not a decision");
    }
}
