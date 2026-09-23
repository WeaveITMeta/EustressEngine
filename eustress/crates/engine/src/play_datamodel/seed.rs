//! Building the session tree from the ECS at Play start.

use std::collections::HashMap;

use bevy::prelude::*;

use eustress_common::classes::{BasePart, ClassName, Humanoid, Instance, Part, PartType, PropertyValue};
use eustress_common::datamodel::{
    is_base_part, is_storage_service, new_shared, DataModel, DmValue, EnumItem, InstanceId,
};
use eustress_common::gui::billboard_renderer::GuiElementDisplay;
use eustress_common::luau::play::{PlayLuau, ScriptLaunch};
use eustress_common::scripting::{CFrame, Color3, UDim2, Vector2, Vector3};

use super::{DataModelSpawned, HiddenForPlay, PlayDataModel, PlayLuauHost};
use crate::space::service_loader::ServiceComponent;

/// OnEnter(Playing): seed the tree, set up the local player, start the VM.
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
                children_of.entry(p.parent()).or_default().push(e);
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
                g.destroy(child);
            }
        }
        // Seeding writes are not script writes.
        let _ = g.take_dirty();
        let _ = g.take_despawns();
        let _ = g.take_spawns();
        g.end_frame();
    }

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
    let (player_id, starter_gui_children) = {
        let mut g = dm.lock();
        let root = g.root();
        let ws = g.get_service("Workspace").unwrap_or(root);
        let cam = g.create_virtual("Camera", "Camera", Some(ws));
        let _ = g.set_prop(ws, "CurrentCamera", DmValue::Instance(cam));
        let _ = g.set_prop(ws, "Gravity", DmValue::Number(9.81));
        // Services scripts reach through GetService.
        for s in ["Players", "RunService", "UserInputService", "TweenService", "Debris", "CollectionService",
                  "HttpService", "SoundService", "ContextActionService", "StarterGui", "StarterPlayer",
                  "ReplicatedStorage", "ServerStorage", "ServerScriptService", "Lighting", "Teams"] {
            let _ = g.get_service(s);
        }
        let players = g.get_service("Players").unwrap_or(root);
        // The player joins after the server scripts have started (see
        // `drive_luau`), as in Roblox, so `PlayerAdded` plus a
        // `GetPlayers()` sweep sets each player up exactly once.
        let player = g.create_virtual("Player", &display_name, None);
        let _ = g.set_prop(player, "DisplayName", DmValue::String(display_name.clone()));
        let _ = g.set_prop(player, "UserId", DmValue::Number(1.0));
        let _ = g.set_prop(players, "LocalPlayer", DmValue::Instance(player));
        let _ = g.set_prop(players, "CharacterAutoLoads", DmValue::Bool(true));
        let _ = g.set_prop(players, "RespawnTime", DmValue::Number(5.0));
        g.create_bound("PlayerGui", "PlayerGui", player_gui_entity.to_bits(), Some(player), Vec::new());
        g.create_virtual("Backpack", "Backpack", Some(player));
        g.create_virtual("PlayerScripts", "PlayerScripts", Some(player));
        g.local_player = Some(player);
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
        services.len(),
        started.elapsed().as_secs_f64() * 1000.0
    );
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
                "Workspace" | "ServerScriptService" | "SoulService" => true,
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
                // Rojo-style names: `x.server.luau` Script, `x.client.luau`
                // LocalScript, `x.module.luau` ModuleScript. A plain file is a
                // ModuleScript where Roblox never runs code (storage), else a
                // Script, so existing Spaces keep running.
                let class = if file.contains(".server.") {
                    "Script"
                } else if file.contains(".client.") {
                    "LocalScript"
                } else if file.contains(".module.") {
                    "ModuleScript"
                } else if matches!(service.as_str(), "ReplicatedStorage" | "ServerStorage" | "ReplicatedFirst") {
                    "ModuleScript"
                } else {
                    "Script"
                };
                for suffix in [".server", ".client", ".module"] {
                    if let Some(stripped) = name.strip_suffix(suffix) {
                        name = stripped.to_string();
                    }
                }
                class.to_string()
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

    if let Some(bp) = world.get::<BasePart>(e) {
        let gt = world.get::<GlobalTransform>(e).copied().unwrap_or_default();
        props.push(("CFrame".into(), DmValue::CFrame(world_cframe(&gt))));
        props.push(("Size".into(), DmValue::Vector3(Vector3::from_vec3(bp.size))));
        props.push(("Color".into(), DmValue::Color3(color3_of(bp.color))));
        let mat = if !bp.material_name.is_empty() { bp.material_name.clone() } else { bp.material.as_str().to_string() };
        props.push(("Material".into(), DmValue::Enum(EnumItem::new("Material", mat))));
        props.push(("Transparency".into(), DmValue::Number(bp.transparency as f64)));
        props.push(("Reflectance".into(), DmValue::Number(bp.reflectance as f64)));
        props.push(("Anchored".into(), DmValue::Bool(bp.anchored)));
        props.push(("CanCollide".into(), DmValue::Bool(bp.can_collide)));
        props.push(("CanTouch".into(), DmValue::Bool(bp.can_touch)));
        props.push(("CastShadow".into(), DmValue::Bool(bp.cast_shadow)));
        props.push(("Locked".into(), DmValue::Bool(bp.locked)));
        props.push(("Mass".into(), DmValue::Number(bp.mass as f64)));
        props.push(("CollisionGroup".into(), DmValue::String(bp.collision_group.clone())));
        if let Some(part) = world.get::<Part>(e) {
            props.push(("Shape".into(), DmValue::Enum(EnumItem::new("PartType", shape_name(part.shape)))));
        }
        if class == "MeshPart" {
            let mesh = world
                .get::<crate::space::instance_loader::InstanceFile>(e)
                .map(|f| space_relative(&f.mesh_path))
                .or_else(|| world.get::<crate::spawn::MeshSource>(e).map(|m| m.path.clone()))
                .unwrap_or_default();
            props.push(("MeshId".into(), DmValue::String(mesh)));
        }
    }

    if let Some(d) = world.get::<GuiElementDisplay>(e) {
        props.extend(gui_props(d, &class));
    }

    if let Some(h) = world.get::<Humanoid>(e) {
        props.push(("Health".into(), DmValue::Number(h.health as f64)));
        props.push(("MaxHealth".into(), DmValue::Number(h.max_health as f64)));
        props.push(("WalkSpeed".into(), DmValue::Number(h.walk_speed as f64)));
        props.push(("JumpPower".into(), DmValue::Number(h.jump_power as f64)));
        props.push(("AutoRotate".into(), DmValue::Bool(h.auto_rotate)));
        props.push(("HipHeight".into(), DmValue::Number(h.hip_height as f64)));
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

/// World pose with the size-as-scale stripped.
pub fn world_cframe(gt: &GlobalTransform) -> CFrame {
    let (_, rot, pos) = gt.to_scale_rotation_translation();
    let mut cf = CFrame::from_quaternion([rot.x as f64, rot.y as f64, rot.z as f64, rot.w as f64]);
    cf.position = Vector3::from_vec3(pos);
    cf
}

pub fn color3_of(c: Color) -> Color3 {
    let s = c.to_srgba();
    Color3::new(s.red as f64, s.green as f64, s.blue as f64)
}

pub fn shape_name(s: PartType) -> &'static str {
    match s {
        PartType::Block => "Block",
        PartType::Ball => "Ball",
        PartType::Cylinder => "Cylinder",
        PartType::Wedge => "Wedge",
        PartType::CornerWedge => "CornerWedge",
        PartType::Cone => "Cone",
    }
}

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
                for d in c.list_properties() {
                    if let Some(v) = c.get_property(&d.name) {
                        if let Some(dv) = property_to_dm(&d.name, &v) {
                            out.push((d.name.clone(), dv));
                        }
                    }
                }
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
    out
}

/// `PropertyValue` (Properties panel) as a script value.
pub fn property_to_dm(name: &str, v: &PropertyValue) -> Option<DmValue> {
    Some(match v {
        PropertyValue::String(s) => DmValue::String(s.clone()),
        PropertyValue::Float(f) => DmValue::Number(*f as f64),
        PropertyValue::Int(i) => DmValue::Number(*i as f64),
        PropertyValue::Bool(b) => DmValue::Bool(*b),
        PropertyValue::Vector2(a) => DmValue::Vector2(Vector2::new(a[0] as f64, a[1] as f64)),
        PropertyValue::Vector3(v) => DmValue::Vector3(Vector3::from_vec3(*v)),
        PropertyValue::UDim2(u) => DmValue::UDim2(UDim2::new(
            u.x.scale as f64,
            u.x.offset as f64,
            u.y.scale as f64,
            u.y.offset as f64,
        )),
        PropertyValue::Color(c) => DmValue::Color3(color3_of(*c)),
        PropertyValue::Color3(c) => DmValue::Color3(Color3::new(c[0] as f64, c[1] as f64, c[2] as f64)),
        PropertyValue::Transform(t) => {
            let mut cf = CFrame::from_quaternion([t.rotation.x as f64, t.rotation.y as f64, t.rotation.z as f64, t.rotation.w as f64]);
            cf.position = Vector3::from_vec3(t.translation);
            DmValue::CFrame(cf)
        }
        PropertyValue::Material(m) => DmValue::Enum(EnumItem::new("Material", m.as_str())),
        PropertyValue::Enum(s) => DmValue::Enum(EnumItem::parse(s, name)),
    })
}

fn attribute_to_dm(v: &eustress_common::AttributeValue) -> Option<DmValue> {
    use eustress_common::AttributeValue as A;
    Some(match v {
        A::String(s) => DmValue::String(s.clone()),
        A::Number(n) => DmValue::Number(*n),
        A::Int(i) => DmValue::Number(*i as f64),
        A::Bool(b) => DmValue::Bool(*b),
        A::Vector2(v) => DmValue::Vector2(Vector2::new(v.x as f64, v.y as f64)),
        A::Vector3(v) => DmValue::Vector3(Vector3::from_vec3(*v)),
        A::Color(c) | A::Color3(c) => DmValue::Color3(color3_of(*c)),
        A::CFrame(t) => {
            let mut cf = CFrame::from_quaternion([t.rotation.x as f64, t.rotation.y as f64, t.rotation.z as f64, t.rotation.w as f64]);
            cf.position = Vector3::from_vec3(t.translation);
            DmValue::CFrame(cf)
        }
        A::UDim2 { x_scale, x_offset, y_scale, y_offset } => {
            DmValue::UDim2(UDim2::new(*x_scale as f64, *x_offset as f64, *y_scale as f64, *y_offset as f64))
        }
        A::NumberRange { min, max } => DmValue::NumberRange(eustress_common::scripting::NumberRange::new(*min, *max)),
        _ => return None,
    })
}
