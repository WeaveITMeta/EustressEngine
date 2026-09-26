//! # Replication: every player sees the host's world
//!
//! Server authority for any simulation (design:
//! `docs/networking/SERVER_AUTHORITY.md`). The host runs the scripts and the
//! physics; this module moves what they did to every player, and what each
//! player does back to the host.
//!
//! | Module | Holds |
//! |---|---|
//! | [`id`] | [`NetId`]: one instance's id on every side |
//! | [`value`] | [`WireValue`]: property values, attributes and remote arguments |
//! | [`ops`] | [`ReplOp`] and [`WorldFrame`]; what players may see ([`audience`]) |
//! | [`host`] | [`HostReplicator`]: the host's tree changes as ops, and a late joiner's catch-up |
//! | [`replica`] | [`Replica`]: a player applying those ops to its own tree |
//! | [`motion`] | the motion lane: bodies physics moved, and interpolation |
//! | [`input`] | the input lane: each player's devices, for anything on the host to read |
//! | [`remote`] | remote calls from players, bounded, typed and rate-limited |
//! | [`tracks`] | animation tracks: control changes out from their owner, applied everywhere else |
//!
//! All of it is IO-free, like [`crate::session`], so a browser build keeps it.

pub mod host;
pub mod id;
pub mod input;
pub mod motion;
pub mod ops;
pub mod remote;
pub mod replica;
pub mod tracks;
pub mod value;

pub use host::{split_frame, FrameEffects, HostReplicator, Outgoing, PreHostRecord, ReplicationTap, WorldLayout};
pub use id::{scene_net_id, service_net_id, NetId, NetIdMap, TERRAIN_KEY};
pub use input::{InputFrame, InputSample, PeerInputs};
pub use motion::{BodyState, Interpolator, MotionFrame, TickClock};
pub use ops::{audience, Audience, ReplOp, SpawnOp, WorldFrame};
pub use remote::{RemoteCall, RemoteRates, RemoteReply};
pub use replica::{Applied, Replica};
pub use tracks::{ClockLink, HostTracks, PlayerTracks, TrackWire, TracksApplied, WireControl};
pub use value::{ValueLimits, WireValue};

/// Heaviest world frame the host sends (see [`host::op_weight`]): a quarter
/// of the wire's frame limit.
pub const MAX_WORLD_FRAME_WEIGHT: usize = crate::wire::MAX_FRAME_BYTES / 4;

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use eustress_common::datamodel::{DataModel, DmValue, EnumItem, InstanceId};
    use eustress_common::scripting::{CFrame, Color3, Vector3};

    use super::*;

    /// The world both sides load: every scene instance, and its record key.
    fn load_world(dm: &mut DataModel) -> Vec<(InstanceId, String)> {
        let root = dm.root();
        for class in ["Workspace", "ReplicatedStorage", "ServerStorage", "Players", "Lighting"] {
            dm.create_virtual(class, class, Some(root));
        }
        let ws = dm.find_service("Workspace").unwrap();
        let rs = dm.find_service("ReplicatedStorage").unwrap();
        let mut keys = Vec::new();
        let mut bound = |dm: &mut DataModel, class: &str, name: &str, parent: InstanceId, key: &str, props: Vec<(String, DmValue)>| {
            let id = dm.create_bound(class, name, 1000 + keys.len() as u64, Some(parent), props);
            keys.push((id, key.to_string()));
            id
        };
        let floor = bound(dm, "Part", "Floor", ws, "Workspace/Floor.part.toml", vec![("Anchored".into(), DmValue::Bool(true))]);
        let _ = floor;
        bound(dm, "Part", "Door", ws, "Workspace/Door.part.toml", vec![]);
        bound(dm, "Part", "Crate", ws, "Workspace/Crate.part.toml", vec![]);
        bound(dm, "Part", "Lamp", ws, "Workspace/Lamp.part.toml", vec![]);
        let tools = bound(dm, "Folder", "Tools", rs, "ReplicatedStorage/Tools/_instance.toml", vec![]);
        bound(dm, "StringValue", "Motd", tools, "ReplicatedStorage/Tools/Motd.value.toml", vec![("Value".into(), DmValue::String("hi".into()))]);
        keys
    }

    /// One engine frame after the scripts ran: the apply step drains the
    /// tree, the replicator reads it, the frame ends.
    fn host_frame(host: &mut HostReplicator, dm: &mut DataModel, tick: u64) -> Outgoing {
        let dirty = dm.take_dirty();
        let _ = (dm.take_spawns(), dm.take_reparents(), dm.take_despawns());
        host.set_tick(tick);
        let out = host.observe(dm, &dirty, FrameEffects::default());
        let cursor = dm.event_cursor();
        dm.trim_events(cursor);
        dm.end_frame();
        out
    }

    fn player_apply(replica: &mut Replica, dm: &mut DataModel, frames: &[WorldFrame]) {
        for f in frames {
            let applied = replica.apply(dm, f);
            assert!(applied.problems.is_empty(), "{:?}", applied.problems);
        }
        let _ = (dm.take_dirty(), dm.take_spawns(), dm.take_reparents(), dm.take_despawns());
        dm.end_frame();
    }

    fn player(peer_world: &mut DataModel) -> Replica {
        // Allocate first, so this tree's InstanceIds differ from the host's.
        let _ = peer_world.create("Folder");
        let keys = load_world(peer_world);
        let mut r = Replica::new(peer_world);
        for (id, key) in keys {
            r.bind_scene(&key, id);
        }
        r
    }

    type Dump = BTreeMap<String, (String, BTreeMap<String, String>, BTreeMap<String, String>, Vec<String>)>;

    /// What a player can see, by full name, with references by full name.
    fn dump(dm: &DataModel, only_public: bool) -> Dump {
        let render = |v: &DmValue| match v {
            DmValue::Instance(id) => format!("-> {}", dm.full_name(*id)),
            other => format!("{other:?}"),
        };
        let mut out = Dump::new();
        let mut stack: Vec<InstanceId> = dm.children(dm.root()).to_vec();
        while let Some(id) = stack.pop() {
            stack.extend(dm.children(id).iter().copied());
            let public = audience(dm, id) == Some(Audience::Everyone);
            if (only_public && !public) || audience(dm, id).is_none() {
                continue;
            }
            let inst = dm.get(id).unwrap();
            let props = inst
                .props
                .iter()
                .filter(|(k, _)| ops::replicates_prop(&inst.class_name, k))
                .map(|(k, v)| (k.clone(), render(v)))
                .collect();
            let attrs = inst.attributes.iter().map(|(k, v)| (k.clone(), render(v))).collect();
            out.insert(dm.full_name(id), (inst.class_name.clone(), props, attrs, dm.tags_of(id)));
        }
        out
    }

    /// Scripts on the host do a bit of everything, one frame at a time.
    fn play(dm: &mut DataModel, host: &mut HostReplicator) -> Vec<Vec<WorldFrame>> {
        let find = |dm: &DataModel, path: &str| {
            let mut cur = dm.root();
            for name in path.split('.') {
                cur = dm.find_first_child(cur, name, false).unwrap_or_else(|| panic!("no {path}"));
            }
            cur
        };
        let mut frames = Vec::new();
        let mut tick = 0;
        let mut step = |dm: &mut DataModel, host: &mut HostReplicator, frames: &mut Vec<Vec<WorldFrame>>| {
            tick += 1;
            let out = host_frame(host, dm, tick);
            frames.push(host.frames_for(&out, None, MAX_WORLD_FRAME_WEIGHT));
        };

        // 1. A model built detached, then parented, naming its own part.
        let ws = find(dm, "Workspace");
        let model = dm.create("Model");
        dm.rename(model, "Car").unwrap();
        let body = dm.create("Part");
        dm.rename(body, "Body").unwrap();
        dm.set_parent(body, Some(model)).unwrap();
        dm.set_prop(model, "PrimaryPart", DmValue::Instance(body)).unwrap();
        dm.set_parent(model, Some(ws)).unwrap();
        step(dm, host, &mut frames);

        // 2. Writes, an attribute, a tag, and a scene part moved into the model.
        let door = find(dm, "Workspace.Door");
        dm.set_prop(door, "Color", DmValue::Color3(Color3 { r: 1.0, g: 0.0, b: 0.0 })).unwrap();
        dm.set_prop(door, "CFrame", DmValue::CFrame(CFrame::from_axis_angle(Vector3 { x: 0.0, y: 1.0, z: 0.0 }, 1.2))).unwrap();
        dm.set_attribute(door, "Open", DmValue::Bool(true)).unwrap();
        dm.add_tag(door, "Interactive");
        let lamp = find(dm, "Workspace.Lamp");
        dm.set_parent(lamp, Some(model)).unwrap();
        step(dm, host, &mut frames);

        // 3. A scene part destroyed, one hidden in ServerStorage, a secret
        //    made there, a rename, and a value edited.
        let crate_ = find(dm, "Workspace.Crate");
        dm.destroy(crate_);
        let ss = find(dm, "ServerStorage");
        let floor = find(dm, "Workspace.Floor");
        dm.set_parent(floor, Some(ss)).unwrap();
        let secret = dm.create("Part");
        dm.rename(secret, "Secret").unwrap();
        dm.set_parent(secret, Some(ss)).unwrap();
        dm.rename(model, "Racer").unwrap();
        let motd = find(dm, "ReplicatedStorage.Tools.Motd");
        dm.set_prop(motd, "Value", DmValue::String("welcome".into())).unwrap();
        step(dm, host, &mut frames);

        // 4. The floor comes back, and the door loses its tag.
        dm.set_parent(floor, Some(ws)).unwrap();
        dm.remove_tag(door, "Interactive");
        dm.set_prop(door, "Material", DmValue::Enum(EnumItem::new("Material", "Neon"))).unwrap();
        step(dm, host, &mut frames);
        frames
    }

    fn host_world() -> (DataModel, HostReplicator) {
        let mut dm = DataModel::new();
        let keys: HashMap<InstanceId, String> = load_world(&mut dm).into_iter().collect();
        let mut host = HostReplicator::new(&dm);
        host.bind_tree(&dm, &|id| keys.get(&id).cloned());
        assert!(host.problems.is_empty());
        (dm, host)
    }

    #[test]
    fn a_player_ends_with_the_hosts_world() {
        let (mut dm, mut host) = host_world();
        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        assert_eq!(dump(&pdm, true), dump(&dm, true), "the world loads the same on both sides");

        for frames in play(&mut dm, &mut host) {
            player_apply(&mut replica, &mut pdm, &frames);
        }
        let host_view = dump(&dm, true);
        assert!(host_view.contains_key("Workspace.Racer.Body"));
        assert!(host_view.contains_key("Workspace.Racer.Lamp"));
        assert!(host_view.contains_key("Workspace.Floor"));
        assert!(!host_view.contains_key("Workspace.Crate"));
        assert_eq!(dump(&pdm, true), host_view);
    }

    #[test]
    fn a_dropped_frame_is_caught() {
        let (mut dm, mut host) = host_world();
        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        let frames = play(&mut dm, &mut host);
        for (i, f) in frames.iter().enumerate() {
            if i != 1 {
                player_apply(&mut replica, &mut pdm, f);
            }
        }
        assert_ne!(dump(&pdm, true), dump(&dm, true), "this comparison must be able to fail");
    }

    #[test]
    fn a_late_joiner_catches_up_from_one_snapshot() {
        let (mut dm, mut host) = host_world();
        let _ = play(&mut dm, &mut host);
        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        let snapshot = host.snapshot(&dm, None);
        player_apply(&mut replica, &mut pdm, &split_frame(snapshot, MAX_WORLD_FRAME_WEIGHT));
        assert_eq!(dump(&pdm, true), dump(&dm, true));
    }

    /// Hosting that begins in a running Play session: everything the session
    /// did before then reaches a late joiner, who loaded the world as saved.
    #[test]
    fn a_late_joiner_sees_what_the_session_did_before_hosting() {
        // The world a joiner loads, and its layout, as the export gives it.
        let mut world = DataModel::new();
        let world_keys = load_world(&mut world);
        let layout = WorldLayout::of_tree(&world, &world_keys.iter().map(|(id, k)| (k.clone(), *id)).collect::<Vec<_>>());

        // The host's Play session, with a server-only Part players never load.
        let mut dm = DataModel::new();
        let mut keys: HashMap<InstanceId, String> = load_world(&mut dm).into_iter().collect();
        let ss = dm.find_service("ServerStorage").unwrap();
        let secret = dm.create_bound("Part", "Secret", 9000, Some(ss), vec![]);
        keys.insert(secret, "ServerStorage/Secret.part.toml".into());
        let _ = (dm.take_dirty(), dm.take_spawns(), dm.take_reparents(), dm.take_despawns());
        let find = |dm: &DataModel, path: &str| {
            let mut cur = dm.root();
            for name in path.split('.') {
                cur = dm.find_first_child(cur, name, false).unwrap_or_else(|| panic!("no {path}"));
            }
            cur
        };

        // What the session's scripts did before anyone hosted.
        let mut record = PreHostRecord::default();
        let ws = dm.find_service("Workspace").unwrap();
        let door = find(&dm, "Workspace.Door");
        dm.set_prop(door, "Transparency", DmValue::Number(0.5)).unwrap();
        let lighting = dm.find_service("Lighting").unwrap();
        dm.set_prop(lighting, "ClockTime", DmValue::Number(20.0)).unwrap();
        dm.set_attribute(find(&dm, "Workspace.Crate"), "hp", DmValue::Number(3.0)).unwrap();
        dm.destroy(find(&dm, "Workspace.Lamp"));
        let motd = find(&dm, "ReplicatedStorage.Tools.Motd");
        dm.set_parent(motd, Some(ws)).unwrap();
        dm.set_parent(secret, Some(ws)).unwrap();
        record.fold_writes(&dm.take_dirty());
        record.fold_events(&dm);
        let _ = (dm.take_spawns(), dm.take_reparents(), dm.take_despawns());
        let cursor = dm.event_cursor();
        dm.trim_events(cursor);
        dm.end_frame();

        // Hosting starts now.
        let mut host = HostReplicator::new(&dm);
        host.bind_tree(&dm, &|id| keys.get(&id).cloned());
        host.seed_before_hosting(&dm, Some(&layout), &|id| keys.get(&id).cloned(), &record);

        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        player_apply(&mut replica, &mut pdm, &split_frame(host.snapshot(&dm, None), MAX_WORLD_FRAME_WEIGHT));
        assert_eq!(dump(&pdm, true), dump(&dm, true));
        assert!(pdm.find_first_child(pdm.find_service("Workspace").unwrap(), "Lamp", false).is_none(), "destroyed");
        assert!(pdm.find_first_child(pdm.find_service("Workspace").unwrap(), "Motd", false).is_some(), "moved");
        assert!(pdm.find_first_child(pdm.find_service("Workspace").unwrap(), "Secret", false).is_some(), "moved into view");
        let plighting = pdm.find_service("Lighting").unwrap();
        assert_eq!(pdm.get_prop(plighting, "ClockTime").and_then(|v| v.as_number()), Some(20.0), "written");
    }

    #[test]
    fn server_content_never_leaves_the_host() {
        let (mut dm, mut host) = host_world();
        let ss = dm.find_service("ServerStorage").unwrap();
        let sss = dm.create_virtual("ServerScriptService", "ServerScriptService", Some(dm.root()));
        let brain = dm.create("Script");
        dm.set_prop(brain, "Source", DmValue::String("print('secret')".into())).unwrap();
        dm.set_parent(brain, Some(sss)).unwrap();
        let ws = dm.find_service("Workspace").unwrap();
        let visible_script = dm.create("Script");
        dm.set_prop(visible_script, "Source", DmValue::String("print('also secret')".into())).unwrap();
        dm.set_parent(visible_script, Some(ws)).unwrap();
        let hidden = dm.create("Part");
        dm.set_parent(hidden, Some(ss)).unwrap();
        let out = host_frame(&mut host, &mut dm, 1);
        let text = format!("{out:?}");
        assert!(!text.contains("secret"), "{text}");
        assert!(host.net_of(hidden).is_none() && host.net_of(brain).is_none());
        assert!(out.iter().any(|(_, op)| matches!(op, ReplOp::Spawn(s) if s.class == "Script")));
    }

    #[test]
    fn a_players_gui_goes_to_that_player_alone() {
        let (mut dm, mut host) = host_world();
        let players = dm.find_service("Players").unwrap();
        let alice = dm.create("Player");
        dm.rename(alice, "Alice").unwrap();
        host.set_player(alice, 7);
        let gui = dm.create("PlayerGui");
        dm.set_parent(gui, Some(alice)).unwrap();
        let label = dm.create("TextLabel");
        dm.set_parent(label, Some(gui)).unwrap();
        dm.set_parent(alice, Some(players)).unwrap();
        let bob = dm.create("Player");
        dm.set_parent(bob, Some(players)).unwrap();
        let out = host_frame(&mut host, &mut dm, 1);

        let classes = |frames: Vec<WorldFrame>| -> Vec<String> {
            frames.iter().flat_map(|f| &f.ops).filter_map(|op| match op {
                ReplOp::Spawn(s) => Some(s.class.clone()),
                _ => None,
            }).collect()
        };
        let for_alice = classes(host.frames_for(&out, Some(alice), MAX_WORLD_FRAME_WEIGHT));
        let for_bob = classes(host.frames_for(&out, Some(bob), MAX_WORLD_FRAME_WEIGHT));
        assert!(for_alice.contains(&"TextLabel".to_string()));
        assert!(!for_bob.contains(&"TextLabel".to_string()) && !for_bob.contains(&"PlayerGui".to_string()));
        assert_eq!(for_bob.iter().filter(|c| *c == "Player").count(), 2);

        // Alice's own Player lands on the one her Player app already has.
        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        let pplayers = pdm.find_service("Players").unwrap();
        let me = pdm.create("Player");
        pdm.set_parent(me, Some(pplayers)).unwrap();
        replica.set_local_player(7, me);
        let cursor = pdm.event_cursor();
        player_apply(&mut replica, &mut pdm, &host.frames_for(&out, Some(alice), MAX_WORLD_FRAME_WEIGHT));
        assert_eq!(pdm.name_of(me), Some("Alice"));
        assert_eq!(pdm.children(pplayers).len(), 2, "Alice (herself) and Bob, no third");
        assert!(pdm.find_first_child_of_class(me, "PlayerGui", false).is_some());
        // Alice's scripts hear Bob arrive, and never themselves.
        let bob_here = pdm.children(pplayers).iter().copied().find(|p| *p != me).unwrap();
        let (events, _) = pdm.events_since(cursor);
        let added: Vec<InstanceId> = events
            .iter()
            .filter_map(|e| match e {
                eustress_common::datamodel::DmEvent::PlayerAdded { player } => Some(*player),
                _ => None,
            })
            .collect();
        assert_eq!(added, vec![bob_here]);
    }

    #[test]
    fn players_and_characters_the_engine_makes_reach_every_player() {
        use eustress_common::datamodel::DmEvent;
        // The engine builds a joined player's `Player` and every character
        // already parented (`create_virtual`, which fires no structure event)
        // and announces them (remote_players.rs, pull.rs). This follows that
        // path exactly.
        let (mut dm, mut host) = host_world();
        let players = dm.find_service("Players").unwrap();
        let ws = dm.find_service("Workspace").unwrap();
        let join = |dm: &mut DataModel, name: &str| {
            let p = dm.create_virtual("Player", name, Some(players));
            dm.create_virtual("Backpack", "Backpack", Some(p));
            dm.push_event(DmEvent::PlayerAdded { player: p });
            p
        };
        let alice = join(&mut dm, "Alice");
        let bob = join(&mut dm, "Bob");
        host.set_player(alice, 7);
        host.set_player(bob, 8);

        // Bob's Player app.
        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        let pplayers = pdm.find_service("Players").unwrap();
        let me = pdm.create("Player");
        pdm.set_parent(me, Some(pplayers)).unwrap();
        replica.set_local_player(8, me);
        let cursor = pdm.event_cursor();
        let out = host_frame(&mut host, &mut dm, 1);
        player_apply(&mut replica, &mut pdm, &host.frames_for(&out, Some(bob), MAX_WORLD_FRAME_WEIGHT));
        let alice_here = replica.player_of(7).expect("Alice's Player reached Bob");
        assert_eq!(pdm.name_of(alice_here), Some("Alice"));
        assert_eq!((pdm.name_of(me), pdm.children(pplayers).len()), (Some("Bob"), 2));
        assert!(pdm.find_first_child_of_class(alice_here, "Backpack", false).is_none(), "hers alone");
        assert_eq!(replica.peer_of(alice_here), Some(7));

        // Alice's character, built as `pull_character` builds it.
        let model = dm.create_virtual("Model", "Alice", Some(ws));
        let root = dm.create_bound("Part", "HumanoidRootPart", 42, Some(model), vec![("Transparency".into(), DmValue::Number(1.0))]);
        let head = dm.create_virtual("Part", "Head", Some(model));
        let humanoid = dm.create_virtual("Humanoid", "Humanoid", Some(model));
        dm.set_prop(model, "PrimaryPart", DmValue::Instance(root)).unwrap();
        dm.set_prop(alice, "Character", DmValue::Instance(model)).unwrap();
        for id in [model, head, humanoid, alice] {
            let _ = dm.take_dirty_of(id);
        }
        dm.push_event(DmEvent::CharacterAdded { player: alice, character: model });
        let out = host_frame(&mut host, &mut dm, 2);
        player_apply(&mut replica, &mut pdm, &host.frames_for(&out, Some(bob), MAX_WORLD_FRAME_WEIGHT));
        let model_here = pdm.get_prop(alice_here, "Character").and_then(|v| v.as_instance()).expect("Character reached Bob");
        assert_eq!(pdm.parent(model_here), pdm.find_service("Workspace"));
        for part in ["HumanoidRootPart", "Head", "Humanoid"] {
            assert!(pdm.find_first_child(model_here, part, false).is_some(), "{part}");
        }
        // A player joining now gets both from its catch-up.
        let mut late = DataModel::new();
        let mut late_replica = player(&mut late);
        player_apply(&mut late_replica, &mut late, &split_frame(host.snapshot(&dm, None), MAX_WORLD_FRAME_WEIGHT));
        let late_alice = late_replica.player_of(7).expect("the catch-up names Alice's peer");
        assert!(late.get_prop(late_alice, "Character").and_then(|v| v.as_instance()).is_some());

        // The character goes, as `pull_character` retires it.
        dm.push_event(DmEvent::CharacterRemoving { player: alice, character: model });
        dm.destroy(model);
        dm.set_prop(alice, "Character", DmValue::Nil).unwrap();
        let _ = dm.take_dirty_of(alice);
        let out = host_frame(&mut host, &mut dm, 3);
        player_apply(&mut replica, &mut pdm, &host.frames_for(&out, Some(bob), MAX_WORLD_FRAME_WEIGHT));
        assert!(!pdm.exists(model_here));
        assert_eq!(pdm.get_prop(alice_here, "Character"), Some(DmValue::Nil));

        // Bob's scripts heard each once, in Roblox's order, and never himself.
        let (events, _) = pdm.events_since(cursor);
        let heard: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                DmEvent::PlayerAdded { player } if *player == alice_here => Some("PlayerAdded".into()),
                DmEvent::PlayerAdded { .. } => Some("PlayerAdded (someone else)".into()),
                DmEvent::CharacterAdded { player, character } if (*player, *character) == (alice_here, model_here) => {
                    Some("CharacterAdded".into())
                }
                DmEvent::CharacterRemoving { player, character } if (*player, *character) == (alice_here, model_here) => {
                    Some("CharacterRemoving".into())
                }
                DmEvent::CharacterAdded { .. } | DmEvent::CharacterRemoving { .. } => Some("a character event for someone else".into()),
                _ => None,
            })
            .collect();
        assert_eq!(heard, vec!["PlayerAdded", "CharacterAdded", "CharacterRemoving"]);
    }

    #[test]
    fn animation_tracks_reach_every_other_machine_on_its_own_clock() {
        use eustress_common::datamodel::DmEvent;
        let (mut dm, mut host) = host_world();
        let mut tracks = HostTracks::default();
        let players = dm.find_service("Players").unwrap();
        let ws = dm.find_service("Workspace").unwrap();
        let rig = |dm: &mut DataModel, parent: InstanceId, name: &str| {
            let model = dm.create_virtual("Model", name, Some(parent));
            let humanoid = dm.create_virtual("Humanoid", "Humanoid", Some(model));
            dm.create_virtual("Animator", "Animator", Some(humanoid));
            (model, humanoid)
        };
        // Two joined players, each with a character, and an NPC, as the
        // engine makes them.
        let mut joined = Vec::new();
        for (name, peer) in [("Alice", 7), ("Bob", 8)] {
            let p = dm.create_virtual("Player", name, Some(players));
            dm.push_event(DmEvent::PlayerAdded { player: p });
            host.set_player(p, peer);
            let (model, _) = rig(&mut dm, ws, name);
            dm.set_prop(p, "Character", DmValue::Instance(model)).unwrap();
            let _ = dm.take_dirty_of(p);
            dm.push_event(DmEvent::CharacterAdded { player: p, character: model });
            joined.push(p);
        }
        let (alice, bob) = (joined[0], joined[1]);
        let npc = dm.create("Model");
        dm.rename(npc, "Npc").unwrap();
        let npc_humanoid = dm.create("Humanoid");
        dm.set_parent(npc_humanoid, Some(npc)).unwrap();
        let npc_animator = dm.create("Animator");
        dm.set_parent(npc_animator, Some(npc_humanoid)).unwrap();
        dm.set_parent(npc, Some(ws)).unwrap();

        // Alice's and Bob's Player apps; Bob's clock reads 100 s at the host's 2 s.
        let join = |peer: u32| {
            let mut pdm = DataModel::new();
            let mut replica = player(&mut pdm);
            let pplayers = pdm.find_service("Players").unwrap();
            let me = pdm.create("Player");
            pdm.set_parent(me, Some(pplayers)).unwrap();
            replica.set_local_player(peer, me);
            pdm.local_player = Some(me);
            pdm.networked = true;
            (pdm, replica)
        };
        let (mut adm, mut areplica) = join(7);
        let (mut bdm, mut breplica) = join(8);
        let out = host_frame(&mut host, &mut dm, 1);
        player_apply(&mut areplica, &mut adm, &host.frames_for(&out, Some(alice), MAX_WORLD_FRAME_WEIGHT));
        player_apply(&mut breplica, &mut bdm, &host.frames_for(&out, Some(bob), MAX_WORLD_FRAME_WEIGHT));
        let animator_of = |pdm: &DataModel, path: [&str; 3]| {
            let mut cur = pdm.find_service("Workspace").unwrap();
            for name in path {
                cur = pdm.find_first_child(cur, name, false).unwrap_or_else(|| panic!("no {name}"));
            }
            cur
        };
        let host_tick = 2.0 * motion::TICK_HZ;
        dm.frame.time = 2.0;
        adm.frame.time = 40.0;
        bdm.frame.time = 100.0;
        let host_clock = ClockLink { frame_now: 2.0, tick_now: host_tick };
        areplica.set_clock(Some(ClockLink { frame_now: 40.0, tick_now: host_tick }));
        breplica.set_clock(Some(ClockLink { frame_now: 100.0, tick_now: host_tick }));

        // The host's script plays the NPC's idle: every player gets it, on its
        // own clock.
        dm.networked = true;
        let idle = dm.load_animation_content(npc_humanoid, "rig://idle").unwrap();
        dm.play_track(idle, 0.2, 1.0, 1.0).unwrap();
        let mut out = host_frame(&mut host, &mut dm, 2);
        out.extend(tracks.host_ops(&mut dm, &host, host_clock));
        player_apply(&mut breplica, &mut bdm, &host.frames_for(&out, Some(bob), MAX_WORLD_FRAME_WEIGHT));
        let bob_npc = animator_of(&bdm, ["Npc", "Humanoid", "Animator"]);
        let on_bob: Vec<_> = bdm.animation.tracks().filter(|(_, t)| t.animator == bob_npc).map(|(_, t)| t.clone()).collect();
        assert_eq!(on_bob.len(), 1, "the NPC's idle reached Bob");
        assert!(on_bob[0].is_remote() && on_bob[0].control.playing && on_bob[0].content == "rig://idle");
        assert!((on_bob[0].control.anchor - 100.0).abs() < 1e-9, "anchored on Bob's clock: {}", on_bob[0].control.anchor);

        // Alice's script waves on her own character, and also plays something
        // on the NPC, which stays on her machine.
        let alice_humanoid = animator_of(&adm, ["Alice", "Humanoid", "Animator"]);
        let wave = adm.load_animation_content(alice_humanoid, "rig://wave").unwrap();
        adm.play_track(wave, 0.1, 1.0, 1.5).unwrap();
        let local_only = adm.load_animation_content(animator_of(&adm, ["Npc", "Humanoid", "Animator"]), "rig://dance").unwrap();
        adm.play_track(local_only, 0.1, 1.0, 1.0).unwrap();
        let sent = areplica.outgoing_tracks(&mut adm, ClockLink { frame_now: 40.0, tick_now: host_tick });
        assert_eq!(sent.len(), 2, "a Load and a Control for the wave alone: {sent:?}");

        // The host checks, applies and relays them.
        let applied = tracks.player_ops(&mut dm, &host, 7, Some(alice), sent.clone(), host_clock, 1.0);
        assert_eq!((applied.refused, applied.over_rate), (0, 0));
        let on_host = dm.animation.tracks().filter(|(_, t)| t.is_remote() && t.content == "rig://wave").count();
        assert_eq!(on_host, 1, "the host's tree plays Alice's wave");
        let relay = host.frames_for(&applied.out, None, MAX_WORLD_FRAME_WEIGHT);
        player_apply(&mut breplica, &mut bdm, &relay);
        player_apply(&mut areplica, &mut adm, &relay);
        let bob_alice = animator_of(&bdm, ["Alice", "Humanoid", "Animator"]);
        let wave_on_bob: Vec<_> = bdm.animation.tracks().filter(|(_, t)| t.animator == bob_alice).map(|(_, t)| t.control).collect();
        assert_eq!(wave_on_bob.len(), 1, "Bob sees Alice wave");
        assert!(wave_on_bob[0].playing && wave_on_bob[0].speed == 1.5 && (wave_on_bob[0].anchor - 100.0).abs() < 1e-9);
        let waves_on_alice = adm.animation.tracks().filter(|(_, t)| t.content == "rig://wave").count();
        assert_eq!(waves_on_alice, 1, "Alice's own wave never comes back to her");

        // What a player may not do: play on an Animator outside its character,
        // name a clip outside the world, or change a track it never loaded.
        let forged = vec![
            TrackWire::Load {
                track: 900,
                animator: host.net_of(npc_animator).unwrap(),
                animation: None,
                content: "rig://idle".into(),
                name: "x".into(),
            },
            TrackWire::Load {
                track: 901,
                animator: host.net_of(dm.find_first_child(dm.find_first_child(dm.find_first_child(ws, "Alice", false).unwrap(), "Humanoid", false).unwrap(), "Animator", false).unwrap()).unwrap(),
                animation: None,
                content: "space://../../secret".into(),
                name: "x".into(),
            },
            TrackWire::Control { track: 902, control: WireControl::from_control(&Default::default(), host_clock) },
        ];
        let applied = tracks.player_ops(&mut dm, &host, 7, Some(alice), forged, host_clock, 2.0);
        assert_eq!(applied.refused, 3);
        assert!(applied.out.is_empty());

        // A player joining now catches up with every track, at the same phase.
        let (mut ldm, mut lreplica) = join(9);
        ldm.frame.time = 7.0;
        lreplica.set_clock(Some(ClockLink { frame_now: 7.0, tick_now: host_tick }));
        let mut catch_up = host.snapshot(&dm, None);
        catch_up.ops.extend(tracks.snapshot(&dm, &host, None, host_clock));
        player_apply(&mut lreplica, &mut ldm, &split_frame(catch_up, MAX_WORLD_FRAME_WEIGHT));
        let late: Vec<_> = ldm.animation.tracks().map(|(_, t)| (t.content.clone(), t.control.anchor)).collect();
        assert_eq!(late.len(), 2, "the idle and the wave: {late:?}");
        assert!(late.iter().all(|(_, anchor)| (anchor - 7.0).abs() < 1e-9));
    }

    #[test]
    fn a_joined_players_local_player_is_the_replicated_one() {
        use eustress_common::datamodel::DmEvent;
        let (mut dm, mut host) = host_world();
        let players = dm.find_service("Players").unwrap();
        // The host's own player and its Players.LocalPlayer, as seed.rs
        // makes them, and Alice (peer 7), as the engine makes a joined player.
        let host_player = dm.create("Player");
        dm.rename(host_player, "Host").unwrap();
        dm.set_parent(host_player, Some(players)).unwrap();
        dm.set_prop(players, "LocalPlayer", DmValue::Instance(host_player)).unwrap();
        host.set_player(host_player, 0);
        let alice = dm.create_virtual("Player", "Alice", Some(players));
        dm.set_prop(alice, "UserId", DmValue::Number(12345.0)).unwrap();
        dm.push_event(DmEvent::PlayerAdded { player: alice });
        host.set_player(alice, 7);

        // Alice's Player app: a local Player from its first frame, named by
        // Players.LocalPlayer, before the host's catch-up arrives.
        let mut pdm = DataModel::new();
        let mut replica = player(&mut pdm);
        let pplayers = pdm.find_service("Players").unwrap();
        let me = pdm.create_virtual("Player", "miksu", Some(pplayers));
        pdm.local_player = Some(me);
        pdm.set_prop(pplayers, "LocalPlayer", DmValue::Instance(me)).unwrap();
        replica.set_local_player(7, me);

        let out = host_frame(&mut host, &mut dm, 1);
        let sends_local_player = out
            .iter()
            .any(|(_, op)| matches!(op, ReplOp::SetProps { props, .. } if props.iter().any(|(n, _)| n == "LocalPlayer")));
        assert!(!sends_local_player, "Players.LocalPlayer is each machine's own");
        player_apply(&mut replica, &mut pdm, &host.frames_for(&out, Some(alice), MAX_WORLD_FRAME_WEIGHT));
        assert_eq!(pdm.get_prop(pplayers, "LocalPlayer"), Some(DmValue::Instance(me)));
        assert_eq!(pdm.local_player, Some(me));
        assert_eq!(pdm.name_of(me), Some("Alice"), "the host's Player for this peer landed on it");
        assert_eq!(pdm.get_prop(me, "UserId"), Some(DmValue::Number(12345.0)));
        let count = pdm.children(pplayers).iter().filter(|p| pdm.class_of(**p) == Some("Player")).count();
        assert_eq!(count, 2, "Alice (itself) and the host, no third");
    }

    #[test]
    fn workspace_terrain_is_one_instance_on_every_side() {
        // Play gives `workspace.Terrain` a handle with no entity and no
        // record, on the host and on a Player alike.
        let mut dm = DataModel::new();
        let keys: HashMap<InstanceId, String> = load_world(&mut dm).into_iter().collect();
        let ws = dm.find_service("Workspace").unwrap();
        let terrain = dm.create_virtual("Terrain", "Terrain", Some(ws));
        let mut host = HostReplicator::new(&dm);
        host.bind_tree(&dm, &|id| keys.get(&id).cloned());
        assert_eq!(host.net_of(terrain), Some(scene_net_id(TERRAIN_KEY)));

        let mut pdm = DataModel::new();
        let _ = pdm.create("Folder");
        let player_keys = load_world(&mut pdm);
        let pws = pdm.find_service("Workspace").unwrap();
        let player_terrain = pdm.create_virtual("Terrain", "Terrain", Some(pws));
        let mut replica = Replica::new(&pdm);
        for (id, key) in player_keys {
            replica.bind_scene(&key, id);
        }

        // A joiner is never sent a second Terrain...
        let snapshot = host.snapshot(&dm, None);
        assert!(!snapshot.ops.iter().any(|op| matches!(op, ReplOp::Spawn(s) if s.class == "Terrain")));
        // ...and a write to the host's lands on the Player's own.
        dm.set_prop(terrain, "WaterWaveSize", DmValue::Number(0.3)).unwrap();
        let out = host_frame(&mut host, &mut dm, 1);
        player_apply(&mut replica, &mut pdm, &host.frames_for(&out, None, MAX_WORLD_FRAME_WEIGHT));
        assert_eq!(pdm.get_prop(player_terrain, "WaterWaveSize"), Some(DmValue::Number(0.3)));
    }

    #[test]
    fn a_remote_fired_after_a_write_arrives_after_it() {
        let (mut dm, mut host) = host_world();
        let door = dm.find_first_child(dm.find_service("Workspace").unwrap(), "Door", false).unwrap();
        dm.set_prop(door, "Transparency", DmValue::Number(0.5)).unwrap();
        let mut out = host_frame(&mut host, &mut dm, 1);
        let remote = host.net_of(door).unwrap();
        out.push((Audience::Everyone, ReplOp::Remote { remote, args: vec![WireValue::String("opened".into())] }));
        let frames = host.frames_for(&out, None, MAX_WORLD_FRAME_WEIGHT);
        let kinds: Vec<&str> = frames.iter().flat_map(|f| &f.ops).map(|op| match op {
            ReplOp::SetProps { .. } => "write",
            ReplOp::Remote { .. } => "remote",
            _ => "other",
        }).collect();
        assert_eq!(kinds, vec!["write", "remote"]);
    }
}
