//! # The default `Animate`
//!
//! Every character gets the engine's `Animate` LocalScript unless the Space's
//! `StarterCharacterScripts` holds a script of that name, as in Roblox. Its
//! Animation children name the avatar's own clips through `rig://`, and
//! carry the speed each clip was authored at, so the script matches playback
//! to ground speed.

use crate::datamodel::{DataModel, DmValue, InstanceId};

/// The script's source, `common/assets/characters/Animate/Animate.client.luau`.
pub const ANIMATE_SOURCE: &str = include_str!("../../assets/characters/Animate/Animate.client.luau");

/// The script's name, which a Space's own script replaces it by.
pub const ANIMATE_NAME: &str = "Animate";

/// The Animation children: name, `AnimationId`, and the ground speed the
/// clip was authored at (m/s at playback rate 1).
pub const DEFAULT_ANIMATIONS: [(&str, &str, Option<f64>); 4] = [
    ("idle", "rig://idle", None),
    ("walk", "rig://walk", Some(1.45)),
    ("run", "rig://run", Some(3.9)),
    ("jump", "rig://jump", None),
];

/// Put the default `Animate` into `character`. Made as engine instances:
/// nothing is marked for the engine to spawn or write back.
pub fn insert_default_animate(dm: &mut DataModel, character: InstanceId) -> InstanceId {
    let script = dm.create_virtual("LocalScript", ANIMATE_NAME, Some(character));
    dm.set_prop_from_engine(script, "Source", DmValue::String(ANIMATE_SOURCE.to_string()));
    dm.set_prop_from_engine(script, "Disabled", DmValue::Bool(false));
    for (name, id, speed) in DEFAULT_ANIMATIONS {
        let animation = dm.create_virtual("Animation", name, Some(script));
        dm.set_prop_from_engine(animation, "AnimationId", DmValue::String(id.to_string()));
        if let Some(speed) = speed {
            dm.seed_attribute(animation, "AuthoredSpeed", DmValue::Number(speed));
        }
    }
    script
}

/// Copy the Space's character scripts into `character`, and add the default
/// `Animate` when none of them is named so. `StarterCharacterScripts` sits
/// in `StarterPlayer` or at the top of the Space; both count.
pub fn populate_character_scripts(dm: &mut DataModel, character: InstanceId) {
    let mut containers = Vec::new();
    if let Some(sp) = dm.find_service("StarterPlayer") {
        if let Some(scs) = dm.find_first_child(sp, "StarterCharacterScripts", false) {
            containers.push(scs);
        }
    }
    if let Some(scs) = dm.find_service("StarterCharacterScripts") {
        if !containers.contains(&scs) {
            containers.push(scs);
        }
    }
    let mut has_animate = false;
    for container in containers {
        let templates = dm.children(container).to_vec();
        for template in templates {
            if dm.name_of(template) == Some(ANIMATE_NAME) {
                has_animate = true;
            }
            if let Some(copy) = dm.clone_instance(template) {
                let _ = dm.set_parent(copy, Some(character));
            }
        }
    }
    if !has_animate {
        insert_default_animate(dm, character);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_gets_the_default_animate_with_rig_clips() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let character = dm.create_virtual("Model", "Player", Some(ws));
        populate_character_scripts(&mut dm, character);
        let animate = dm.find_first_child(character, ANIMATE_NAME, false).expect("Animate");
        assert_eq!(dm.class_of(animate), Some("LocalScript"));
        let source = dm.get_prop(animate, "Source").and_then(|v| v.as_str().map(str::to_string)).unwrap();
        assert!(source.contains("LoadAnimation"));
        let walk = dm.find_first_child(animate, "walk", false).expect("walk");
        assert_eq!(dm.get_prop(walk, "AnimationId"), Some(DmValue::String("rig://walk".into())));
        assert_eq!(dm.get_attribute(walk, "AuthoredSpeed"), Some(DmValue::Number(1.45)));
    }

    #[test]
    fn a_spaces_own_animate_replaces_the_default() {
        let mut dm = DataModel::new();
        let sp = dm.get_service("StarterPlayer").unwrap();
        let scs = dm.create_virtual("StarterCharacterScripts", "StarterCharacterScripts", Some(sp));
        let own = dm.create_virtual("LocalScript", ANIMATE_NAME, Some(scs));
        dm.set_prop_from_engine(own, "Source", DmValue::String("-- mine".into()));
        let ws = dm.get_service("Workspace").unwrap();
        let character = dm.create_virtual("Model", "Player", Some(ws));
        populate_character_scripts(&mut dm, character);
        let animates: Vec<_> = dm
            .children(character)
            .iter()
            .filter(|c| dm.name_of(**c) == Some(ANIMATE_NAME))
            .copied()
            .collect();
        assert_eq!(animates.len(), 1);
        assert_eq!(dm.get_prop(animates[0], "Source"), Some(DmValue::String("-- mine".into())));
    }
}
