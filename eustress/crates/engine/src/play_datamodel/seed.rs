//! Building the session tree from the ECS at Play start.

use std::collections::HashMap;

use bevy::prelude::*;

use eustress_common::classes::{BasePart, ClassName, Humanoid, Instance, Part};
use eustress_common::datamodel::record::{attribute_to_dm, luau_script_class, part_props, strip_script_suffix};
use eustress_common::datamodel::{
    is_base_part, is_storage_service, new_shared, DataModel, DmValue, EnumItem, InstanceId,
};
use eustress_common::gui::billboard_renderer::GuiElementDisplay;
use eustress_common::luau::play::{PlayLuau, ScriptLaunch};
use eustress_common::scripting::{Color3, UDim2, Vector2};

use super::{DataModelSpawned, HiddenForPlay, PlayDataModel, PlayLuauHost};
use crate::space::service_loader::ServiceComponent;

/// OnEnter(Playing): seed the tree (with Workspace's Terrain), set up the
/// local player, start the VM.
pub fn seed_session(world: &mut World) {
    if world.contains_resource::<PlayDataModel>() {
        // Resuming from a pause keeps the running session.
        return;
    }
    // Scripts may change the lighting (a day-night cycle); Stop puts the
    // editor's back.
    if let Some(lighting) = world.get_resource::<eustress_common::services::lighting::LightingService>() {
        let snapshot = super::PlayLightingSnapshot(lighting.clone());
        world.insert_resource(snapshot);
    }
    let started = std::time::Instant::now();
    let dm = new_shared();
    let (seeded, service_count) = seed_scene(world, &dm);

    // ── Camera, player, PlayerGui ──────────────────────────────────────
    let player_gui_entity = world
        .spawn((
            Instance { name: "PlayerGui".into(), class_name: ClassName::Folder, archivable: false, id: 0, uuid: String::new(), ai: false },
            ServiceComponent { class_name: "PlayerGui".into(), ..Default::default() },
            Name::new("PlayerGui"),
            Transform::default(),
            Visibility::default(),
            DataModelSpawned,
        ))
        .id();
    let display_name = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref().map(|u| u.username.clone()))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Player1".to_string());
    // The Space's `Players.CharacterAutoLoads`, loaded as Play started.
    let character_auto_loads = world
        .get_resource::<crate::play_mode::CharacterAutoLoads>()
        .map_or(true, |a| a.0);
    // The live gravity as Play starts, which `workspace.Gravity` reports.
    let live_gravity = eustress_common::services::workspace::live_gravity(
        world.get_resource::<eustress_common::services::workspace::Workspace>(),
    );
    let (player_id, starter_gui_children) = {
        let mut g = dm.lock();
        let root = g.root();
        let ws = g.get_service("Workspace").unwrap_or(root);
        // `workspace.Terrain`, the scripts' handle on the terrain root. It is
        // never spawned: the root draws and collides the ground.
        eustress_common::luau::play::terrain::ensure_terrain_instance(&mut g, ws);
        // Services scripts reach through GetService.
        for s in ["Players", "RunService", "UserInputService", "TweenService", "Debris", "CollectionService",
                  "HttpService", "SoundService", "ContextActionService", "StarterGui", "StarterPlayer",
                  "ReplicatedStorage", "ServerStorage", "ServerScriptService", "Lighting", "Teams"] {
            let _ = g.get_service(s);
        }
        // The player joins after the server scripts have started (see
        // `drive_luau`), as in Roblox, so `PlayerAdded` plus a
        // `GetPlayers()` sweep sets each player up exactly once.
        let player = g.create_virtual("Player", &display_name, None);
        let _ = g.set_prop(player, "DisplayName", DmValue::String(display_name.clone()));
        let _ = g.set_prop(player, "UserId", DmValue::Number(1.0));
        g.create_bound("PlayerGui", "PlayerGui", player_gui_entity.to_bits(), Some(player), Vec::new());
        g.create_virtual("Backpack", "Backpack", Some(player));
        g.create_virtual("PlayerScripts", "PlayerScripts", Some(player));
        // What every app's session starts from (`Players.LocalPlayer`, a
        // fresh `CurrentCamera`, `workspace.Gravity`, `RespawnTime`,
        // `CharacterAutoLoads`), from the one function the Player calls too.
        eustress_common::play_session::begin_session(
            &mut g,
            player,
            &eustress_common::play_session::SessionStart {
                camera: eustress_common::play_session::SessionCamera::Fresh,
                gravity: live_gravity,
                character_auto_loads,
            },
        );
        let starter = g.find_service("StarterGui");
        let kids: Vec<InstanceId> = starter.map(|s| g.children(s).to_vec()).unwrap_or_default();
        let _ = g.take_dirty();
        (player, kids)
    };

    // Roblox copies StarterGui into each player's PlayerGui; the copies are
    // what render and what runs. The originals are hidden for the session
    // (restored with the rest of the GUI state on Stop).
    {
        let mut g = dm.lock();
        let player_gui = g.find_first_child(player_id, "PlayerGui", false);
        if let Some(pg) = player_gui {
            for child in &starter_gui_children {
                if let Some(copy) = g.clone_instance(*child) {
                    let _ = g.set_parent(copy, Some(pg));
                }
            }
        }
    }
    if !starter_gui_children.is_empty() {
        let g = dm.lock();
        let entities: Vec<Entity> = starter_gui_children
            .iter()
            .filter_map(|c| g.entity_of(*c))
            .map(Entity::from_bits)
            .collect();
        drop(g);
        for e in entities {
            if let Some(mut d) = world.get_mut::<GuiElementDisplay>(e) {
                d.visible = false;
            }
        }
    }

    // ── Storage services never render or collide ───────────────────────
    hide_storage(world, &dm);

    // ── Scripts to start ────────────────────────────────────────────────
    let (server, client) = collect_launches(&dm.lock());

    // ── The VM ──────────────────────────────────────────────────────────
    match PlayLuau::new(dm.clone()) {
        Ok(vm) => {
            let (ns, nc) = (server.len(), client.len());
            world.insert_resource(PlayLuauHost {
                vm,
                pending_server: server,
                pending_client: client,
                joining_player: Some(player_id),
            });
            info!("🌙 Luau Play VM ready: {} server and {} client script(s) queued", ns, nc);
        }
        Err(e) => {
            error!("❌ Luau Play VM failed to start: {}", e);
        }
    }
    eustress_common::datamodel::set_active(Some(dm.clone()));
    world.insert_resource(PlayDataModel { dm });
    info!(
        "🌳 Play DataModel seeded: {} instances from {} services in {:.1} ms",
        seeded,
        service_count,
        started.elapsed().as_secs_f64() * 1000.0
    );
}

/// The scene half of the session tree: every service and loaded instance the
/// ECS holds, bound to its entity, with a Luau script folder's source folded
/// in from its child file. A Player builds the same tree from the world's
/// records (`eustress_common::tree_read`), and the parity test below compares
/// the two. Returns the instances seeded and the services found.
pub fn seed_scene(world: &mut World, dm: &eustress_common::datamodel::SharedDataModel) -> (usize, usize) {
    // ── Gather the ECS hierarchy ────────────────────────────────────────
    let mut services: Vec<(Entity, String)> = Vec::new();
    let mut children_of: HashMap<Entity, Vec<Entity>> = HashMap::new();
    {
        let mut q = world.query::<(Entity, &Instance, Option<&ChildOf>, Option<&ServiceComponent>)>();
        for (e, _inst, parent, service) in q.iter(world) {
            if let Some(sc) = service {
                services.push((e, sc.class_name.clone()));
                continue;
            }
            if let Some(p) = parent {
                // A child of a pose anchor hangs under the anchor's part.
                let p = p.parent();
                let p = if world.get::<crate::space::pose_anchor::PoseAnchor>(p).is_some() {
                    world.get::<ChildOf>(p).map_or(p, |c| c.parent())
                } else {
                    p
                };
                children_of.entry(p).or_default().push(e);
            }
        }
    }
    services.sort_by(|a, b| a.1.cmp(&b.1));
    for kids in children_of.values_mut() {
        kids.sort_by_key(|e| e.index());
    }

    // ── Services and everything below them ─────────────────────────────
    let mut seeded = 0usize;
    let mut script_folders: Vec<(InstanceId, Entity)> = Vec::new();
    {
        let mut g = dm.lock();
        let root = g.root();
        for (service_entity, class) in &services {
            if g.find_service(class).is_some() {
                continue; // duplicate service folder; the first wins
            }
            let id = g.create_bound(class, class, service_entity.to_bits(), Some(root), Vec::new());
            g.register_service(class, id);
            seeded += 1;
            let mut stack: Vec<(Entity, InstanceId)> = vec![(*service_entity, id)];
            while let Some((parent_entity, parent_id)) = stack.pop() {
                let Some(kids) = children_of.get(&parent_entity) else { continue };
                for kid in kids {
                    let Some((class, name, props)) = describe(world, *kid) else { continue };
                    let kid_id = g.create_bound(&class, &name, kid.to_bits(), Some(parent_id), props);
                    seed_attributes(world, *kid, kid_id, &mut g);
                    record_script_file(world, *kid, kid_id, &mut g);
                    if matches!(class.as_str(), "Script" | "LocalScript" | "ModuleScript") {
                        script_folders.push((kid_id, *kid));
                    }
                    seeded += 1;
                    stack.push((*kid, kid_id));
                }
            }
        }
    }

    // A LuauScript folder keeps its code in a child `script.luau` file: fold
    // that child's source into the folder instance and drop the child.
    {
        let mut g = dm.lock();
        for (folder_id, _) in &script_folders {
            let has_source = g.get_prop(*folder_id, "Source").and_then(|v| v.as_str().map(|s| !s.is_empty())).unwrap_or(false);
            if has_source {
                continue;
            }
            let code_child = g.children(*folder_id).iter().copied().find(|c| {
                g.get_prop(*c, "Source").and_then(|v| v.as_str().map(|s| !s.is_empty())).unwrap_or(false)
            });
            if let Some(child) = code_child {
                let src = g.get_prop(child, "Source").unwrap_or_default();
                let _ = g.set_prop(*folder_id, "Source", src);
                // The code file is where the source lives, so Output points there.
                if let Some(file) = g.script_file(child).map(str::to_string) {
                    g.set_script_file(*folder_id, file);
                }
                g.destroy(child);
            }
        }
        // Seeding writes are not script writes.
        let _ = g.take_dirty();
        let _ = g.take_despawns();
        let _ = g.take_spawns();
        g.end_frame();
    }
    (seeded, services.len())
}

/// The scripts Play starts, as (server, client). Server: Scripts in
/// Workspace, ServerScriptService and Eustress's SoulService. Client:
/// LocalScripts in the local player (its PlayerGui holds the StarterGui
/// copies) and in StarterPlayerScripts. Nothing in storage or StarterGui
/// runs. LocalScripts placed beside server Scripts also run, as client
/// scripts, so a Space that never split them keeps working.
fn collect_launches(g: &DataModel) -> (Vec<ScriptLaunch>, Vec<ScriptLaunch>) {
    let mut server = Vec::new();
    let mut client = Vec::new();
    // The player is not in the tree yet, so walk its subtree explicitly.
    let mut stack = vec![g.root()];
    if let Some(p) = g.local_player {
        stack.push(p);
    }
    while let Some(id) = stack.pop() {
        for c in g.children(id) {
            stack.push(*c);
        }
        let Some(inst) = g.get(id) else { continue };
        let is_script = inst.class_name == "Script";
        let is_local = inst.class_name == "LocalScript";
        if !(is_script || is_local) {
            continue;
        }
        if g.get_prop(id, "Disabled").and_then(|v| v.as_bool()).unwrap_or(false)
            || !g.get_prop(id, "Enabled").and_then(|v| v.as_bool()).unwrap_or(true)
        {
            continue;
        }
        let in_player = g.local_player.map_or(false, |p| g.is_descendant_of(id, p));
        let service = g.service_of(id).and_then(|s| g.class_of(s).map(str::to_string)).unwrap_or_default();
        // Spaces keep StarterPlayerScripts either inside StarterPlayer or as a
        // top-level folder of its own; both run.
        let runs = in_player
            || match service.as_str() {
                // A LocalScript inside a character runs only in the local
                // player's own, which arrives later.
                "Workspace" => !(is_local && in_character(g, id)),
                "ServerScriptService" | "SoulService" => true,
                "StarterPlayer" => is_local && g.find_first_ancestor(id, "StarterCharacterScripts").is_none(),
                "StarterPlayerScripts" => is_local,
                _ => false,
            };
        if !runs {
            continue;
        }
        let source = g.get_prop(id, "Source").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        if source.trim().is_empty() {
            continue;
        }
        let launch = ScriptLaunch { instance: id, source, chunk_name: g.full_name(id) };
        if is_script && !in_player {
            server.push(launch);
        } else {
            client.push(launch);
        }
    }
    // Deterministic start order: by full name.
    server.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    client.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    (server, client)
}

/// Whether `id` sits in a character: a Model above it holds a Humanoid.
fn in_character(g: &DataModel, id: InstanceId) -> bool {
    let mut cur = g.parent(id);
    while let Some(p) = cur {
        if g.class_of(p) == Some("Model") && g.find_first_child_of_class(p, "Humanoid", false).is_some() {
            return true;
        }
        cur = g.parent(p);
    }
    false
}

/// Services whose parts Roblox keeps out of the world: templates live here
/// and must neither render nor collide while the game runs.
const HIDDEN_SERVICES: &[&str] = &[
    "ServerStorage",
    "ReplicatedStorage",
    "ReplicatedFirst",
    "ServerScriptService",
    "StarterPack",
    "StarterPlayer",
    "SoulService",
];

/// Hide the storage services and switch off collision for the parts inside
/// them. Lighting, StarterGui and SoundService keep drawing.
fn hide_storage(world: &mut World, dm: &eustress_common::datamodel::SharedDataModel) {
    let (service_entities, part_entities) = {
        let g = dm.lock();
        let mut svc = Vec::new();
        let mut parts = Vec::new();
        for s in g.children(g.root()) {
            let Some(class) = g.class_of(*s) else { continue };
            if !HIDDEN_SERVICES.contains(&class) || !is_storage_service(class) {
                continue;
            }
            if let Some(e) = g.entity_of(*s) {
                svc.push(Entity::from_bits(e));
            }
            for d in g.descendants(*s) {
                if g.get(d).map_or(false, |i| is_base_part(&i.class_name)) {
                    if let Some(e) = g.entity_of(d) {
                        parts.push(Entity::from_bits(e));
                    }
                }
            }
        }
        (svc, parts)
    };
    for e in service_entities.into_iter().chain(part_entities) {
        let Ok(mut ent) = world.get_entity_mut(e) else { continue };
        let Some(previous) = ent.get::<Visibility>().copied() else { continue };
        let collider_was_disabled = ent.contains::<avian3d::prelude::ColliderDisabled>();
        if let Some(mut v) = ent.get_mut::<Visibility>() {
            *v = Visibility::Hidden;
        }
        ent.insert((HiddenForPlay { previous, collider_was_disabled }, avian3d::prelude::ColliderDisabled));
    }
}

/// Class, name and properties for one entity, or `None` to leave it out of
/// the tree.
fn describe(world: &World, e: Entity) -> Option<(String, String, Vec<(String, DmValue)>)> {
    let inst = world.get::<Instance>(e)?;
    let mut name = inst.name.clone();
    let loaded = world.get::<crate::space::LoadedFromFile>(e);
    let script = world.get::<crate::soul::SoulScriptData>(e);
    let class = match inst.class_name {
        ClassName::LuauScript => "Script".to_string(),
        ClassName::LuauLocalScript => "LocalScript".to_string(),
        ClassName::LuauModuleScript => "ModuleScript".to_string(),
        ClassName::SoulScript => {
            let is_luau = script.map_or(false, |s| s.run_context == crate::soul::SoulRunContext::Luau)
                || loaded.map_or(false, |l| {
                    matches!(l.path.extension().and_then(|x| x.to_str()), Some("lua") | Some("luau"))
                });
            if is_luau {
                let file = loaded
                    .and_then(|l| l.path.file_name().and_then(|f| f.to_str()).map(str::to_string))
                    .unwrap_or_default()
                    .to_lowercase();
                let service = loaded.map(|l| l.service.clone()).unwrap_or_default();
                // Rojo-style names, by the rule a Player's reader shares.
                name = strip_script_suffix(&name);
                luau_script_class(&file, &service).to_string()
            } else {
                "SoulScript".to_string()
            }
        }
        ClassName::Part => {
            let custom_mesh = world
                .get::<crate::spawn::MeshSource>(e)
                .map_or(false, |m| {
                    let p = m.path.to_lowercase();
                    !["parts/block", "parts/ball", "parts/cylinder", "parts/wedge", "parts/corner_wedge", "parts/cone"]
                        .iter()
                        .any(|h| p.contains(h))
                });
            if custom_mesh { "MeshPart".to_string() } else { "Part".to_string() }
        }
        ClassName::Camera => "Camera".to_string(),
        other => other.as_str().to_string(),
    };

    let mut props: Vec<(String, DmValue)> = Vec::new();

    // A script whose `[script]` section says `enabled = false` must not start:
    // in Roblox that is a disabled template another script clones and enables
    // at runtime, and starting it where it sits runs it twice or in the wrong
    // place. A script folder carries the loader's marker; a script file that
    // `spawn_instance` loaded has the section's keys as attributes.
    if matches!(class.as_str(), "Script" | "LocalScript") {
        let disabled = world.get::<crate::space::file_loader::ScriptDisabled>(e).is_some()
            || world
                .get::<eustress_common::Attributes>(e)
                .and_then(|a| a.get("enabled"))
                .is_some_and(|v| matches!(v, eustress_common::AttributeValue::Bool(false)));
        if disabled {
            props.push(("Disabled".into(), DmValue::Bool(true)));
        }
    }

    // A part's properties come from the one conversion a Player's reader uses
    // too (`eustress_common::datamodel::record`), so the two trees agree.
    if let Some(bp) = world.get::<BasePart>(e) {
        let gt = world.get::<GlobalTransform>(e).copied().unwrap_or_default();
        let shape = world.get::<Part>(e).map(|part| part.shape);
        let mesh_id = (class == "MeshPart").then(|| {
            world
                .get::<crate::space::instance_loader::InstanceFile>(e)
                .map(|f| space_relative(&f.mesh_path))
                .or_else(|| world.get::<crate::spawn::MeshSource>(e).map(|m| m.path.clone()))
                .unwrap_or_default()
        });
        props.extend(part_props(bp, world_cframe(&gt), shape, mesh_id));
    }

    // A Model's stored pivot, by the rule a Player's reader shares: pushed
    // only when it has one (an identity pivot means none is stored).
    if let Some(model) = world.get::<eustress_common::classes::Model>(e) {
        if let Some(prop) = eustress_common::datamodel::record::model_pivot_prop(model.world_pivot) {
            props.push(prop);
        }
    }

    if let Some(d) = world.get::<GuiElementDisplay>(e) {
        props.extend(gui_props(d, &class));
    }

    // A Humanoid loaded from a file brings its properties in its record
    // (below, in metres by the file's unit); the component speaks for a
    // Humanoid spawned in Play, and for the keys a file leaves unset.
    if let Some(h) = world.get::<Humanoid>(e) {
        let record = world.get::<eustress_common::datamodel::record::RecordClassProps>(e);
        let from_record = |key: &str| record.is_some_and(|r| r.0.iter().any(|(name, _)| name == key));
        let component = [
            ("Health", DmValue::Number(h.health as f64)),
            ("MaxHealth", DmValue::Number(h.max_health as f64)),
            ("WalkSpeed", DmValue::Number(h.walk_speed as f64)),
            ("JumpPower", DmValue::Number(h.jump_power as f64)),
            ("AutoRotate", DmValue::Bool(h.auto_rotate)),
            ("HipHeight", DmValue::Number(h.hip_height as f64)),
        ];
        for (key, value) in component {
            if !from_record(key) {
                props.push((key.into(), value));
            }
        }
    }

    // An Animation's clip, as its record names it (`[properties] animation_id`).
    if let Some(a) = world.get::<eustress_common::classes::Animation>(e) {
        props.push(("AnimationId".into(), DmValue::String(a.animation_id.clone())));
    }

    // A class's own properties from its file (a seat's settings), by the
    // conversion a Player's reader shares.
    if let Some(r) = world.get::<eustress_common::datamodel::record::RecordClassProps>(e) {
        props.extend(r.0.iter().cloned());
    }

    if let Some(s) = script {
        props.push(("Source".into(), DmValue::String(s.source.clone())));
    }

    // Everything else the Properties panel can read (lights, sounds,
    // emitters, beams, ...), without overriding what was set above.
    for (k, v) in generic_props(world, e) {
        if !props.iter().any(|(n, _)| *n == k) {
            props.push((k, v));
        }
    }

    Some((class, name, props))
}

/// The Luau file an instance's source was read from, for the Output lines
/// about its code to point at. Only a code file counts, never an
/// `_instance.toml`.
fn record_script_file(world: &World, e: Entity, id: InstanceId, g: &mut DataModel) {
    let Some(loaded) = world.get::<crate::space::LoadedFromFile>(e) else { return };
    if matches!(loaded.path.extension().and_then(|x| x.to_str()), Some("lua") | Some("luau")) {
        g.set_script_file(id, loaded.path.to_string_lossy());
    }
}

fn seed_attributes(world: &World, e: Entity, id: InstanceId, g: &mut DataModel) {
    if let Some(attrs) = world.get::<eustress_common::Attributes>(e) {
        for (k, v) in attrs.iter() {
            if let Some(dv) = attribute_to_dm(v) {
                g.seed_attribute(id, k, dv);
            }
        }
    }
    if let Some(tags) = world.get::<eustress_common::Tags>(e) {
        for t in tags.iter() {
            g.seed_tag(id, t);
        }
    }
}

/// The value conversions a Player's reader shares: a world pose as a
/// `CFrame` (size-as-scale stripped), a colour, a shape's name.
pub use eustress_common::datamodel::record::{color3_of, world_cframe};
pub use eustress_common::space_read::shape_name;

/// A path under the open Space as `a/b/c.glb`; anything else unchanged.
pub fn space_relative(p: &std::path::Path) -> String {
    let root = crate::space::space_asset_source::space_asset_root();
    p.strip_prefix(&root)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| p.to_string_lossy().replace('\\', "/"))
}

fn gui_props(d: &GuiElementDisplay, class: &str) -> Vec<(String, DmValue)> {
    let mut p = Vec::new();
    if class == "ScreenGui" {
        p.push(("Enabled".into(), DmValue::Bool(d.visible)));
        p.push(("DisplayOrder".into(), DmValue::Number(d.z_order as f64)));
        return p;
    }
    let u = |a: [f32; 4]| DmValue::UDim2(UDim2::new(a[0] as f64, a[1] as f64, a[2] as f64, a[3] as f64));
    p.push(("Position".into(), u(d.position_udim2)));
    p.push(("Size".into(), u(d.size_udim2)));
    p.push(("AnchorPoint".into(), DmValue::Vector2(Vector2::new(d.anchor_point[0] as f64, d.anchor_point[1] as f64))));
    p.push(("Visible".into(), DmValue::Bool(d.visible)));
    p.push(("ZIndex".into(), DmValue::Number(d.z_order as f64)));
    p.push(("BackgroundColor3".into(), DmValue::Color3(Color3::new(d.bg_color[0] as f64, d.bg_color[1] as f64, d.bg_color[2] as f64))));
    p.push(("BackgroundTransparency".into(), DmValue::Number(1.0 - d.bg_color[3] as f64)));
    p.push(("BorderSizePixel".into(), DmValue::Number(d.border_size as f64)));
    p.push(("BorderColor3".into(), DmValue::Color3(Color3::new(d.border_color[0] as f64, d.border_color[1] as f64, d.border_color[2] as f64))));
    p.push(("Text".into(), DmValue::String(d.text.clone())));
    p.push(("TextColor3".into(), DmValue::Color3(Color3::new(d.text_color[0] as f64, d.text_color[1] as f64, d.text_color[2] as f64))));
    p.push(("TextTransparency".into(), DmValue::Number(1.0 - d.text_color[3] as f64)));
    p.push(("TextSize".into(), DmValue::Number(d.font_size as f64)));
    p.push(("TextScaled".into(), DmValue::Bool(d.text_scaled)));
    let font = if d.font.is_empty() { "SourceSans".to_string() } else { d.font.clone() };
    p.push(("Font".into(), DmValue::Enum(EnumItem::new("Font", font))));
    let xa = if d.text_align.is_empty() { "Center".to_string() } else { title_case(&d.text_align) };
    p.push(("TextXAlignment".into(), DmValue::Enum(EnumItem::new("TextXAlignment", xa))));
    let ya = if d.text_y_align.is_empty() { "Center".to_string() } else { title_case(&d.text_y_align) };
    p.push(("TextYAlignment".into(), DmValue::Enum(EnumItem::new("TextYAlignment", ya))));
    p.push(("Image".into(), DmValue::String(d.image_path.clone())));
    p
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// Properties of every PropertyAccess component on the entity.
fn generic_props(world: &World, e: Entity) -> Vec<(String, DmValue)> {
    use eustress_common::classes::*;
    let mut out: Vec<(String, DmValue)> = Vec::new();
    macro_rules! read {
        ($ty:ty) => {
            if let Some(c) = world.get::<$ty>(e) {
                out.extend(eustress_common::datamodel::record::component_props(c));
            }
        };
    }
    read!(EustressPointLight);
    read!(EustressSpotLight);
    read!(SurfaceLight);
    read!(Sound);
    read!(ParticleEmitter);
    read!(Beam);
    read!(Decal);
    read!(Attachment);
    read!(SpecialMesh);
    read!(BlockMesh);
    read!(CylinderMesh);
    read!(FileMesh);
    out
}

/// `PropertyValue` (Properties panel) as a script value.
pub use eustress_common::datamodel::record::property_to_dm;

#[cfg(test)]
mod parity {
    //! A Player's tree and Studio's tree of the same records must start equal,
    //! because replication only sends later writes (SA-1 in
    //! `docs/networking/SERVER_AUTHORITY.md`).
    //!
    //! Studio's side runs its real code wherever a file becomes an entity:
    //! `spawn_instance` for every Part folder and every flat file (parse,
    //! heal, class, units, pose, BasePart, attributes, tags), and
    //! `spawn_general_entity` for a folder of any other class (name, pose,
    //! attributes, tags, a Model's pivot, a disabled script). Then the pose
    //! anchors under sized parts (`PoseAnchorPlugin`), Bevy's transform
    //! propagation, [`super::seed_scene`] and the Play session's
    //! `workspace.Terrain`. The file loader's walk around them is copied by
    //! hand from `space::file_loader`: a service per top-level folder, and a
    //! bare `.luau` file as a `SoulScript` holding its source. The fixture's
    //! `space.toml` names the parent-pose rule. The Player's side is
    //! `eustress_common::tree_read::read_space_records`.

    use std::collections::{BTreeMap, HashMap};
    use std::path::PathBuf;

    use bevy::ecs::system::RunSystemOnce;
    use bevy::pbr::decal::ForwardDecalMaterial;
    use bevy::prelude::*;

    use eustress_common::classes::{ClassName, Instance};
    use eustress_common::datamodel::record::raw_class_name;
    use eustress_common::datamodel::{new_shared, DataModel, DmValue, InstanceId};
    use eustress_networking::repl::ops::replicates_prop;

    use crate::space::file_loader::{spawn_general_entity, FileMetadata, FileType, LoadedFromFile};
    use crate::space::instance_loader::{spawn_instance_from_toml_str, PrimitiveMeshCache};
    use crate::space::material_loader::MaterialRegistry;
    use crate::space::service_loader::ServiceComponent;

    /// A small Pong court in the on-disk format.
    fn world() -> Vec<(String, String)> {
        let part = |name: &str, rest: &str| format!("[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\n\n{rest}");
        vec![
            ("space.toml".into(), "[space]\ntransform_rule = \"parent_pose\"\n".into()),
            (
                "Workspace/Floor/_instance.toml".into(),
                part(
                    "Floor",
                    "[transform]\nposition = [0.0, -0.5, 0.0]\nscale = [60.0, 1.0, 30.0]\n\n\
                     [properties]\ncolor = [80, 80, 90]\nanchored = true\nmaterial = \"Slate\"\n\n\
                     [properties.physics]\ndensity = 2400.0\ndensity_unit = \"kg/m3\"\n",
                ),
            ),
            (
                "Workspace/Ball/_instance.toml".into(),
                part(
                    "Ball",
                    "[asset]\nmesh = \"parts/ball.glb\"\n\n[transform]\nposition = [0.0, 1.0, 0.0]\nscale = [1.0, 1.0, 1.0]\n\n\
                     [properties]\ntransparency = 0.25\nreflectance = 0.1\n",
                ),
            ),
            (
                "Workspace/Base/_instance.toml".into(),
                part(
                    "Base",
                    "[transform]\nposition = [0.0, 0.5, 20.0]\nrotation = [0.0, 0.7071068, 0.0, 0.7071068]\nscale = [4.0, 1.0, 4.0]\n\n\
                     [properties]\nanchored = true\n",
                ),
            ),
            (
                "Workspace/Base/Flag/_instance.toml".into(),
                part(
                    "Flag",
                    "[transform]\nposition = [0.5, 1.0, 0.0]\nscale = [0.2, 2.0, 0.2]\n\n[attributes]\nteam = \"red\"\n",
                ),
            ),
            (
                "Workspace/Base/Grip/_instance.toml".into(),
                "[metadata]\nclass_name = \"Attachment\"\n\n\
                 [transform]\nposition = [0.5, 0.0, 0.0]\nrotation = [0.0, 0.7071068, 0.0, 0.7071068]\n\n\
                 [attachment]\nvisible = true\n"
                    .into(),
            ),
            (
                "Workspace/Wheel/_instance.toml".into(),
                part(
                    "Wheel",
                    "[asset]\nmesh = \"parts/ball.glb\"\n\n[transform]\nposition = [8.0, 1.0, 0.0]\nscale = [2.0, 2.0, 2.0]\n",
                ),
            ),
            (
                "Workspace/Wheel/Mesh/_instance.toml".into(),
                "[metadata]\nclass_name = \"SpecialMesh\"\n\n\
                 [mesh]\nmesh_type = \"Cylinder\"\nscale = [0.33, 1.0, 1.0]\noffset = [0.0, 0.1, 0.0]\nvertex_color = [255, 128, 0]\n"
                    .into(),
            ),
            (
                "Workspace/Pillar/_instance.toml".into(),
                part("Pillar", "[transform]\nposition = [-8.0, 5.0, 0.0]\nscale = [1.0, 10.0, 1.0]\n"),
            ),
            (
                "Workspace/Pillar/Mesh/_instance.toml".into(),
                "[metadata]\nclass_name = \"CylinderMesh\"\n\n[mesh]\nscale = [1.0, 1.0, 1.0]\noffset = [0.0, 0.5, 0.0]\n".into(),
            ),
            (
                "Workspace/LooseMesh.instance.toml".into(),
                "[metadata]\nclass_name = \"BlockMesh\"\nname = \"LooseMesh\"\n\n\
                 [mesh]\nscale = [2.0, 1.0, 1.0]\nvertex_color = [0.5, 0.5, 0.5]\n"
                    .into(),
            ),
            (
                "Workspace/Imported/_instance.toml".into(),
                "[metadata]\nclass_name = \"Part\"\nname = \"Imported\"\nunit = \"ft\"\n\n\
                 [transform]\nposition = [10.0, 3.0, 0.0]\nscale = [4.0, 1.0, 2.0]\n"
                    .into(),
            ),
            (
                "Workspace/Paddle.instance.toml".into(),
                "tags = [\"paddle\"]\n\n[metadata]\nclass_name = \"Part\"\nname = \"Paddle\"\n\n\
                 [transform]\nposition = [-25.0, 1.0, 0.0]\nscale = [1.0, 1.0, 6.0]\n\n\
                 [properties]\nanchored = true\ncolor = [0.9, 0.9, 1.0]\n\n[attributes]\nspeed = 12\nside = \"left\"\n"
                    .into(),
            ),
            (
                "Workspace/Spawn.instance.toml".into(),
                "[metadata]\nclass_name = \"SpawnLocation\"\nname = \"Spawn\"\n\n[asset]\nmesh = \"parts/block.glb\"\n\n\
                 [transform]\nposition = [0.0, 0.5, -10.0]\nscale = [6.0, 1.0, 6.0]\n"
                    .into(),
            ),
            (
                "Workspace/decor/Lamp.instance.toml".into(),
                part("Lamp", "[transform]\nposition = [5.0, 2.0, 5.0]\nscale = [0.5, 3.0, 0.5]\n"),
            ),
            (
                "Workspace/Score.instance.toml".into(),
                "[metadata]\nclass_name = \"Folder\"\nname = \"Score\"\n\n[attributes]\nleft = 0\nright = 0\n\n\
                 [gameplay]\nwin_at = { type = \"number\", value = 7 }\nmode = \"classic\"\n"
                    .into(),
            ),
            (
                "Workspace/Clock/_instance.toml".into(),
                "tags = [\"timer\"]\n\n[metadata]\nclass_name = \"LuauScript\"\n\n\
                 [script]\nrun_context = \"Server\"\nenabled = false\n\n[attributes]\nperiod = 1.5\n"
                    .into(),
            ),
            (
                "Workspace/court/_instance.toml".into(),
                "tags = [\"arena\"]\n\n[metadata]\nclass_name = \"Model\"\nname = \"Arena\"\n\n\
                 [transform]\nposition = [0.0, 0.0, 40.0]\n\n[attributes]\nlit = true\n\n\
                 [model]\nworld_pivot = { position = [0.0, 1.0, 40.0], rotation = [0.0, 0.0, 0.0, 1.0] }\n"
                    .into(),
            ),
            (
                "Workspace/court/Net/_instance.toml".into(),
                part("Net", "[transform]\nposition = [0.0, 1.0, 0.0]\nscale = [0.2, 2.0, 30.0]\n"),
            ),
            (
                "Workspace/Walk/_instance.toml".into(),
                "[metadata]\nclass_name = \"Animation\"\n\n[properties]\nanimation_id = \"rbxassetid://507\"\n".into(),
            ),
            (
                "Workspace/Wave/_instance.toml".into(),
                "[metadata]\nclass_name = \"KeyframeSequence\"\n\n[keyframe_sequence]\nloop = false\npriority = \"Action2\"\n"
                    .into(),
            ),
            ("Workspace/Clock/script.luau".into(), "print('tick')\n".into()),
            (
                "ReplicatedStorage/Config.instance.toml".into(),
                "[metadata]\nclass_name = \"Folder\"\nname = \"Config\"\n\n[attributes]\nball_speed = 18.5\ntint = [1.0, 0.5, 0.0]\n"
                    .into(),
            ),
        ]
    }

    #[derive(Resource)]
    struct Fixture(Vec<(String, String)>);

    /// The fixture, spawned the way Studio's file loader spawns a Space.
    fn spawn_world(
        mut commands: Commands,
        fixture: Res<Fixture>,
        asset_server: Res<AssetServer>,
        mut materials: ResMut<Assets<StandardMaterial>>,
        mut registry: ResMut<MaterialRegistry>,
        mut mesh_cache: ResMut<PrimitiveMeshCache>,
        mut decals: ResMut<Assets<ForwardDecalMaterial<StandardMaterial>>>,
    ) {
        let text_of: HashMap<&str, &str> = fixture.0.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let mut dirs: HashMap<String, Entity> = HashMap::new();
        // Parents before children.
        let mut files: Vec<&(String, String)> = fixture.0.iter().collect();
        files.sort_by_key(|(p, _)| (p.matches('/').count(), p.clone()));
        for (path, text) in files {
            // The Space's own `space.toml` is no instance.
            let Some((dir, file)) = path.rsplit_once('/') else { continue };
            if file == "_instance.toml" {
                let class = raw_class_name(&text.parse::<toml::Value>().expect("fixture parses"))
                    .map(crate::space::representation::class_from_toml)
                    .unwrap_or(ClassName::Folder);
                if class == ClassName::Part {
                    let parent = dir_entity(&mut commands, &mut dirs, &text_of, dir.rsplit_once('/').map_or("", |(up, _)| up));
                    let e = spawn_instance_from_toml_str(
                        &mut commands,
                        &asset_server,
                        &mut materials,
                        &mut registry,
                        &mut mesh_cache,
                        &mut decals,
                        PathBuf::from(path.as_str()),
                        text,
                    )
                    .expect("Studio's loader reads the fixture");
                    commands.entity(e).insert(ChildOf(parent));
                    dirs.insert(dir.to_string(), e);
                } else {
                    dir_entity(&mut commands, &mut dirs, &text_of, dir);
                }
                continue;
            }
            let parent = dir_entity(&mut commands, &mut dirs, &text_of, dir);
            let service = path.split('/').next().unwrap_or_default().to_string();
            let e = if file.ends_with(".luau") {
                // `spawn_file_entry`'s Luau branch.
                let stem = file.strip_suffix(".luau").unwrap_or(file).to_string();
                commands
                    .spawn((
                        Instance {
                            name: stem.clone(),
                            class_name: ClassName::SoulScript,
                            archivable: true,
                            id: 0,
                            ai: false,
                            uuid: String::new(),
                        },
                        crate::soul::SoulScriptData {
                            source: text.clone(),
                            run_context: crate::soul::SoulRunContext::Luau,
                            ..Default::default()
                        },
                        LoadedFromFile { path: PathBuf::from(path.as_str()), file_type: FileType::Lua, service },
                        Name::new(stem),
                    ))
                    .id()
            } else {
                spawn_instance_from_toml_str(
                    &mut commands,
                    &asset_server,
                    &mut materials,
                    &mut registry,
                    &mut mesh_cache,
                    &mut decals,
                    PathBuf::from(path.as_str()),
                    text,
                )
                .expect("Studio's loader reads the fixture")
            };
            commands.entity(e).insert(ChildOf(parent));
        }
    }

    /// The entity for directory `dir` (not a Part folder, which spawns from
    /// its file): a service at the top, else the entity Studio's general
    /// branch makes for the folder's class, made on first use.
    fn dir_entity(
        commands: &mut Commands,
        dirs: &mut HashMap<String, Entity>,
        text_of: &HashMap<&str, &str>,
        dir: &str,
    ) -> Entity {
        if let Some(e) = dirs.get(dir) {
            return *e;
        }
        let name = dir.rsplit('/').next().unwrap_or(dir).to_string();
        let e = match dir.rsplit_once('/') {
            None => commands
                .spawn((
                    Instance {
                        name: name.clone(),
                        class_name: ClassName::Folder,
                        archivable: true,
                        id: 0,
                        ai: false,
                        uuid: String::new(),
                    },
                    ServiceComponent { class_name: name.clone(), ..Default::default() },
                    Name::new(name),
                    Transform::default(),
                    Visibility::default(),
                ))
                .id(),
            Some((up, _)) => {
                let parent = dir_entity(commands, dirs, text_of, up);
                let doc = text_of
                    .get(format!("{dir}/_instance.toml").as_str())
                    .and_then(|t| t.parse::<toml::Value>().ok());
                let class = doc
                    .as_ref()
                    .and_then(raw_class_name)
                    .map(crate::space::representation::class_from_toml)
                    .unwrap_or(ClassName::Folder);
                let meta = FileMetadata {
                    path: PathBuf::from(dir),
                    file_type: FileType::Directory,
                    service: dir.split('/').next().unwrap_or_default().to_string(),
                    name,
                    size: 0,
                    modified: std::time::SystemTime::UNIX_EPOCH,
                    children: Vec::new(),
                };
                let e = spawn_general_entity(commands, &meta, doc.as_ref(), class, String::new());
                commands.entity(e).insert(ChildOf(parent));
                e
            }
        };
        dirs.insert(dir.to_string(), e);
        e
    }

    type Entry = (String, BTreeMap<String, String>, BTreeMap<String, String>, Vec<String>);

    /// A value with floats rounded, so one pose computed twice compares equal
    /// and a zero never prints as -0.
    fn render(v: &DmValue) -> String {
        let r = |x: f64| {
            let v = (x * 1e4).round() / 1e4;
            if v == 0.0 {
                0.0
            } else {
                v
            }
        };
        match v {
            DmValue::Number(n) => format!("{:.4}", r(*n)),
            DmValue::Vector3(v) => format!("({:.4}, {:.4}, {:.4})", r(v.x), r(v.y), r(v.z)),
            DmValue::Color3(c) => format!("rgb({:.4}, {:.4}, {:.4})", r(c.r), r(c.g), r(c.b)),
            DmValue::CFrame(cf) => {
                let t = cf.to_transform();
                let q = t.rotation;
                format!(
                    "at ({:.4}, {:.4}, {:.4}) rot ({:.4}, {:.4}, {:.4}, {:.4})",
                    r(t.translation.x as f64),
                    r(t.translation.y as f64),
                    r(t.translation.z as f64),
                    r(q.x as f64),
                    r(q.y as f64),
                    r(q.z as f64),
                    r(q.w as f64)
                )
            }
            other => format!("{other:?}"),
        }
    }

    /// Every instance by full name: class, replicated properties, attributes
    /// and tags. Cameras stay host-local and are left out.
    fn dump(dm: &DataModel) -> BTreeMap<String, Entry> {
        let mut out = BTreeMap::new();
        let mut stack: Vec<InstanceId> = dm.children(dm.root()).to_vec();
        while let Some(id) = stack.pop() {
            let Some(inst) = dm.get(id) else { continue };
            if inst.class_name == "Camera" {
                continue;
            }
            stack.extend(dm.children(id).iter().copied());
            let props = inst
                .props
                .iter()
                .filter(|(k, _)| replicates_prop(&inst.class_name, k))
                .map(|(k, v)| (k.clone(), render(v)))
                .collect();
            let attrs = inst.attributes.iter().map(|(k, v)| (k.clone(), render(v))).collect();
            out.insert(dm.full_name(id), (inst.class_name.clone(), props, attrs, dm.tags_of(id)));
        }
        out
    }

    /// Every part's density by full name: the one hidden property the
    /// replicated dump leaves out, and the one the Luau VM's `Mass` reads.
    fn densities(dm: &DataModel) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut stack: Vec<InstanceId> = dm.children(dm.root()).to_vec();
        while let Some(id) = stack.pop() {
            let Some(inst) = dm.get(id) else { continue };
            stack.extend(dm.children(id).iter().copied());
            if let Some(v) = inst.props.get(eustress_common::datamodel::PART_DENSITY) {
                out.insert(dm.full_name(id), render(v));
            }
        }
        out
    }

    #[test]
    fn a_players_tree_starts_equal_to_studios() {
        let fixture = world();

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), bevy::transform::TransformPlugin));
        app.init_asset::<Mesh>();
        app.init_asset::<StandardMaterial>();
        app.init_asset::<Image>();
        app.init_asset::<ForwardDecalMaterial<StandardMaterial>>();
        app.init_resource::<MaterialRegistry>();
        app.init_resource::<PrimitiveMeshCache>();
        app.insert_resource(Fixture(fixture.clone()));
        // The fixture's space.toml names the parent-pose rule, which the file
        // loader would read at load.
        app.add_plugins(crate::space::pose_anchor::PoseAnchorPlugin);
        app.insert_resource(crate::space::pose_anchor::ParentPoseRule(true));
        app.world_mut().run_system_once(spawn_world).expect("the fixture spawns");
        // The anchors hang each sized part's children, then transform
        // propagation fills every GlobalTransform.
        app.update();
        app.update();

        let studio = new_shared();
        super::seed_scene(app.world_mut(), &studio);
        {
            let mut g = studio.lock();
            let ws = g.find_service("Workspace").expect("the fixture has a Workspace");
            eustress_common::luau::play::terrain::ensure_terrain_instance(&mut g, ws);
        }
        let records: Vec<(String, Vec<u8>)> =
            fixture.iter().map(|(p, t)| (p.clone(), t.as_bytes().to_vec())).collect();
        let player = eustress_common::tree_read::read_space_records(&records);
        assert!(player.report.problems.is_empty(), "{:?}", player.report.problems);

        let a = dump(&studio.lock());
        let b = dump(&player.dm);
        let mut diffs = Vec::new();
        for (name, entry) in &a {
            match b.get(name) {
                None => diffs.push(format!("{name}: only in Studio's tree")),
                Some(other) if other != entry => diffs.push(format!("{name}:\n  studio {entry:?}\n  player {other:?}")),
                _ => {}
            }
        }
        for name in b.keys() {
            if !a.contains_key(name) {
                diffs.push(format!("{name}: only in the Player's tree"));
            }
        }
        assert!(diffs.is_empty(), "the trees differ:\n{}", diffs.join("\n"));
        // Every record as one instance, by name: the two services, the
        // Terrain handle, the plain `decor` directory's Folder, and the Clock
        // with its script file folded into it. Naming them (rather than
        // counting) says which one appeared or went missing.
        let names: Vec<&str> = a.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "ReplicatedStorage",
                "ReplicatedStorage.Config",
                "Workspace",
                "Workspace.Arena",
                "Workspace.Arena.Net",
                "Workspace.Ball",
                "Workspace.Base",
                "Workspace.Base.Flag",
                "Workspace.Base.Grip",
                "Workspace.Clock",
                "Workspace.Decor",
                "Workspace.Decor.Lamp",
                "Workspace.Floor",
                "Workspace.Imported",
                "Workspace.LooseMesh",
                "Workspace.Paddle",
                "Workspace.Pillar",
                "Workspace.Pillar.Mesh",
                "Workspace.Score",
                "Workspace.Spawn",
                "Workspace.Terrain",
                "Workspace.Walk",
                "Workspace.Wave",
                "Workspace.Wheel",
                "Workspace.Wheel.Mesh",
            ]
        );
        // The density each part weighs at, which replication leaves out: the
        // same in both trees, and the Floor's authored override, not Slate's.
        let (studio_densities, player_densities) = (densities(&studio.lock()), densities(&player.dm));
        assert_eq!(studio_densities, player_densities, "the trees weigh their parts alike");
        assert_eq!(
            studio_densities.get("Workspace.Floor"),
            Some(&render(&DmValue::Number(2400.0))),
            "{studio_densities:?}"
        );
    }
}
