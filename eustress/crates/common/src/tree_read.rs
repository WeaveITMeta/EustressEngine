//! # Reading a world's records into the DataModel tree
//!
//! A Player keeps the world it plays as a DataModel tree, because the tree is
//! where replication writes ([`crate::tree_apply`] draws it). This module
//! builds that tree from a Space's records: `(Space-relative path, bytes)`
//! pairs, exactly as a downloaded world carries them. A Space on disk goes
//! through the same code by way of [`read_space_dir`], so a local Space and a
//! downloaded one are read one way.
//!
//! The tree has to match the one Studio seeds for the same world, since
//! replication only sends later writes (SA-1 in
//! `docs/networking/SERVER_AUTHORITY.md`). So the walk follows Studio's file
//! loader branch by branch, and every property comes from
//! [`crate::datamodel::record`], the conversion Studio's seed uses too.
//!
//! ## The walk, as Studio's loader walks a Space
//!
//! * Each top-level directory is a service, named and classed by its folder.
//! * A directory's class is its `_instance.toml`'s (the legacy `Script`
//!   resolves to `SoulScript`), or `Folder` without one. Then, by class:
//!   * The part classes ([`loads_as_part`]: `Part`, `UnionOperation`,
//!     `Seat`, `VehicleSeat`, `SpawnLocation`), the particle classes and the
//!     terrain layer classes are built from the file ([`record_props`]):
//!     pose, properties, attributes, tags. A part folder's files are the
//!     part's own assets; only its subfolders load.
//!   * `SoulScript` is one script: its source is the file its
//!     `[script] source` names, else its first `.rune`/`.luau`/`.soul`/`.lua`
//!     file, and without one it is a `Folder`. The file gives it its name,
//!     attributes and tags.
//!   * Every other class (`Folder`, `Model`, the Luau script classes, the
//!     rest) is an entity named by its file's `[metadata] name`, else after
//!     its folder with the first letter capitalised. Its file poses it (its
//!     children nest under that pose; in a Space from before the parent-pose
//!     rule, an Attachment, a Bone or a part class such as a Seat keeps the
//!     identity) and gives it its attributes and tags, a Model's stored
//!     pivot, and `Disabled` when it switches a script off. GUI, media, sky
//!     and lighting, and lights keep the folder's name and their parent's
//!     pose, and take the file's attributes and tags.
//! * A child composes onto its parent's pose, never its size, when the
//!   Space's `space.toml` names `[space] transform_rule = "parent_pose"`; a
//!   Space written before the rule composes onto the parent's whole
//!   transform ([`TransformRule`]).
//! * A flat `<Name>.instance.toml` (or `.part.toml`, `.glb.toml`,
//!   `.model.toml`) is built from the file, whatever its class. When a folder
//!   of the same name exists, the folder wins.
//! * A bare `.luau` or `.lua` file is a Script, LocalScript or ModuleScript by
//!   its Rojo-style name; a bare `.rune` or `.soul` file is a `SoulScript`.
//! * A Luau script folder with no source of its own takes the source of the
//!   first script file it holds, which is then not an instance (Studio's seed
//!   folds it the same way).
//! * `Workspace/Terrain` holding the terrain's own data (a `_terrain.toml`, or
//!   no `_instance.toml`) is not an instance; the layer instances in its
//!   `Layers` folder load beside it.
//! * Any other file (meshes, textures, sounds, materials, GUI element files)
//!   makes no instance here, and is counted in [`TreeReadReport::unread_files`].
//!
//! Last, the Workspace gets the `Terrain` handle scripts reach as
//! `workspace.Terrain`, as Studio's Play session makes it, unless the world
//! already has a Terrain.
//!
//! ## Record keys
//!
//! Every scene instance is returned with the key Studio's host binds it by:
//! the `_instance.toml` of a folder built from its file, the flat file, the
//! script's source file, or the folder's own path. Replication's
//! `Replica::bind_scene` then gives both sides the same ids.
//!
//! ## Posed records
//!
//! [`posed_records`] runs the same walk and returns, for every instance read
//! from a file, where a [`TransformRule`] puts it and what its children
//! compose onto. A Space's transform migration plans its rewrites from it, so
//! nothing walks a Space a second way.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use bevy::prelude::*;

use crate::classes::ClassName;
use crate::datamodel::record::{
    attribute_to_dm, camera_props, class_from_toml, component_props, folder_display_name, has_model_pivot, loads_as_part,
    is_known_class, luau_script_class, model_pivot_prop, raw_class_name, reads_own_properties, record_animation_id,
    warn_unknown_class,
    record_attachment, data_mesh_props, data_mesh_section, starter_player_props,
    record_class_props,
    record_attribute_values, record_model_pivot, record_name, record_pose, record_props, record_tags,
    script_starts_disabled, strip_script_suffix, tree_class, InstanceRecord, RecordForm, RecordInstance,
    TransformRule,
};
use crate::datamodel::{DataModel, DmValue, InstanceId};
use crate::scripting::CFrame;

/// A world's tree, built from its records.
pub struct SceneTree {
    pub dm: DataModel,
    /// The record key of every scene instance, for replication to bind.
    pub keys: Vec<(String, InstanceId)>,
    pub report: TreeReadReport,
}

/// What a read did, and what it did not.
#[derive(Debug, Clone, Default)]
pub struct TreeReadReport {
    /// Scene instances made (services not counted).
    pub instances: usize,
    /// Files no instance was made from, by extension, with counts.
    pub unread_files: BTreeMap<String, usize>,
    /// Instances whose own properties are not read yet, by class: they have
    /// their class defaults, their attributes and their tags.
    pub unread_properties: BTreeMap<String, usize>,
    /// Files that could not be read, with the reason.
    pub problems: Vec<String>,
}

impl TreeReadReport {
    /// One line for a log.
    pub fn summary(&self) -> String {
        let unread: usize = self.unread_files.values().sum();
        let partial: usize = self.unread_properties.values().sum();
        format!(
            "{} instances, {} files not instances ({:?}), {} instances with class defaults ({:?}), {} problems",
            self.instances,
            unread,
            self.unread_files,
            partial,
            self.unread_properties,
            self.problems.len()
        )
    }
}

/// Build the tree of one Space from its records: every path relative to the
/// Space's folder, with `/` separators. Only `.toml` files and script sources
/// are read; any other record may carry no bytes.
pub fn read_space_records(records: &[(String, Vec<u8>)]) -> SceneTree {
    read(records, None, false).0
}

/// Build the tree of the Space in `space_root`, read from disk. The same
/// walk as [`read_space_records`]; only `.toml` files and script sources are
/// read into memory.
pub fn read_space_dir(space_root: &Path) -> Result<SceneTree, String> {
    let (records, mut problems) = space_records(space_root)?;
    let mut tree = read_space_records(&records);
    problems.append(&mut tree.report.problems);
    tree.report.problems = problems;
    Ok(tree)
}

/// The records of the Space in `space_root`, as [`read_space_dir`] reads
/// them: every file's path, with bytes for `.toml` files and script sources
/// only, and the files that could not be read.
pub fn space_records(space_root: &Path) -> Result<(Vec<(String, Vec<u8>)>, Vec<String>), String> {
    if !space_root.join("Workspace").is_dir() {
        return Err(format!("not a Space: {} has no Workspace/ directory", space_root.display()));
    }
    let mut records = Vec::new();
    let mut problems = Vec::new();
    collect(space_root, space_root, &mut records, &mut problems);
    Ok((records, problems))
}

/// One instance read from a file, placed by a [`TransformRule`].
#[derive(Debug, Clone)]
pub struct PosedRecord {
    /// The file it was read from: a folder's `_instance.toml` or a flat file.
    pub key: String,
    /// The nearest ancestor read from a file; `None` under a service.
    pub parent: Option<String>,
    pub class: ClassName,
    /// Whether its own `[transform]` placed it under the rule. Scripts, the
    /// classes with branches of their own and, under the legacy rule, an
    /// Attachment, a Bone or a part class in the general branch sit on their
    /// parent's frame instead.
    pub posed: bool,
    /// Whether it was built from its file ([`record_props`]: the part classes,
    /// the other classes built from their file, flat files), so that its
    /// file's `scale` is part of its world. Every other instance ignores it.
    pub from_file: bool,
    /// Where the rule puts it. A part's scale is its size.
    pub world: GlobalTransform,
    /// What its children compose onto.
    pub frame: GlobalTransform,
}

/// Every instance of a Space read from a file, parents first, placed by
/// `rule` whatever the Space's `space.toml` names. The walk is
/// [`read_space_records`]'s: which files become instances, and under which
/// parent, does not depend on the rule. A folder with no `_instance.toml` is
/// left out, since it passes its parent's frame through under either rule.
pub fn posed_records(records: &[(String, Vec<u8>)], rule: TransformRule) -> Vec<PosedRecord> {
    read(records, Some(rule), true).1
}

/// The walk behind [`read_space_records`] and [`posed_records`]. `rule`
/// overrides the one the Space's `space.toml` names.
fn read(records: &[(String, Vec<u8>)], rule: Option<TransformRule>, posed: bool) -> (SceneTree, Vec<PosedRecord>) {
    let index = Index::build(records);
    let rule = rule.unwrap_or_else(|| {
        let space_toml = index
            .file("space.toml")
            .and_then(|i| std::str::from_utf8(index.bytes(i)).ok()?.parse::<toml::Value>().ok());
        TransformRule::of_space(space_toml.as_ref())
    });
    let mut reader = Reader {
        dm: DataModel::new(),
        keys: Vec::new(),
        report: TreeReadReport::default(),
        index: &index,
        rule,
        posed: posed.then(Vec::new),
        file_keys: HashMap::new(),
    };
    let root = reader.dm.root();
    if let Some(top) = index.dir("") {
        for service in &top.dirs {
            if skip_dir(service) {
                continue;
            }
            // A service is bound by its class, never by a key. StarterPlayer
            // carries its character settings from its service file.
            let props = match service.as_str() {
                "StarterPlayer" => {
                    let key = format!("{service}/_service.toml");
                    let doc = index.file(&key).and_then(|i| reader.parse(&key, i));
                    doc.as_ref().map(starter_player_props).unwrap_or_default()
                }
                _ => Vec::new(),
            };
            let id = reader.dm.create_scene(service, service, Some(root), props);
            reader.walk(service, id, GlobalTransform::IDENTITY, service, None);
        }
    }
    ensure_terrain(&mut reader.dm);
    let posed = reader.posed.take().unwrap_or_default();
    (SceneTree { dm: reader.dm, keys: reader.keys, report: reader.report }, posed)
}

/// `workspace.Terrain`, as Studio's Play session makes it: the scripts'
/// handle on the ground, first among the Workspace's children, locked to its
/// parent and never saved. One the world already has is kept, never doubled.
fn ensure_terrain(dm: &mut DataModel) {
    let Some(ws) = dm.find_service("Workspace") else { return };
    let id = match dm.find_first_child_of_class(ws, "Terrain", false) {
        Some(existing) => existing,
        None => {
            let id = dm.create_virtual("Terrain", "Terrain", Some(ws));
            if let Some(terrain) = dm.get_mut(id) {
                terrain.props.insert("CFrame".into(), DmValue::CFrame(CFrame::IDENTITY));
                terrain.props.insert("Anchored".into(), DmValue::Bool(true));
                terrain.props.insert("Locked".into(), DmValue::Bool(true));
            }
            id
        }
    };
    if let Some(terrain) = dm.get_mut(id) {
        terrain.parent_locked = true;
        terrain.archivable = false;
    }
    let moved = match dm.get_mut(ws) {
        Some(w) if w.children.first() != Some(&id) => {
            w.children.retain(|c| *c != id);
            w.children.insert(0, id);
            true
        }
        _ => false,
    };
    if moved {
        dm.structure_version += 1;
    }
}

// ── The index ──────────────────────────────────────────────────────────────

#[derive(Default)]
struct DirEntry {
    /// Child directory names, in name order.
    dirs: BTreeSet<String>,
    /// File name to record index, in name order.
    files: BTreeMap<String, usize>,
}

struct Index<'a> {
    records: &'a [(String, Vec<u8>)],
    /// Directory path (`""` is the Space's folder) to its entries.
    dirs: HashMap<String, DirEntry>,
}

impl<'a> Index<'a> {
    fn build(records: &'a [(String, Vec<u8>)]) -> Self {
        let mut dirs: HashMap<String, DirEntry> = HashMap::new();
        dirs.entry(String::new()).or_default();
        for (i, (raw, _)) in records.iter().enumerate() {
            let path = normalize(raw);
            if path.is_empty() {
                continue;
            }
            let (dir, file) = match path.rfind('/') {
                Some(slash) => (&path[..slash], &path[slash + 1..]),
                None => ("", path.as_str()),
            };
            // Register every ancestor directory under its parent.
            let mut parent = String::new();
            if !dir.is_empty() {
                for seg in dir.split('/') {
                    let here = if parent.is_empty() { seg.to_string() } else { format!("{parent}/{seg}") };
                    dirs.entry(parent.clone()).or_default().dirs.insert(seg.to_string());
                    dirs.entry(here.clone()).or_default();
                    parent = here;
                }
            }
            dirs.entry(dir.to_string()).or_default().files.insert(file.to_string(), i);
        }
        Self { records, dirs }
    }

    fn dir(&self, path: &str) -> Option<&DirEntry> {
        self.dirs.get(path)
    }

    fn bytes(&self, i: usize) -> &[u8] {
        &self.records[i].1
    }

    /// The record index of the file at `path`, if the world has it.
    fn file(&self, path: &str) -> Option<usize> {
        let (dir, file) = match path.rfind('/') {
            Some(slash) => (&path[..slash], &path[slash + 1..]),
            None => ("", path),
        };
        self.dirs.get(dir)?.files.get(file).copied()
    }
}

/// A record path in the one form the index keys on: forward slashes, no
/// leading `./` or `/`.
fn normalize(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    p.trim_start_matches('/').to_string()
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// `rel` resolved against the folder `dir`, with `.` and `..` folded.
fn join_rel(dir: &str, rel: &str) -> String {
    let rel = rel.replace('\\', "/");
    let mut parts: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Directory names Studio's loader never descends into.
fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "trash" | "_instance.toml" | "_service.toml")
}

/// Suffixes of a flat instance file, as Studio's scan strips them.
const FLAT_SUFFIXES: &[&str] = &["instance.toml", "part.toml", "glb.toml", "model.toml"];

/// A flat instance file's name without its suffix (`Sky.instance.toml` is
/// `Sky`), or `None` for any other file.
fn flat_stem(file: &str) -> Option<&str> {
    FLAT_SUFFIXES.iter().find_map(|s| file.strip_suffix(s)?.strip_suffix('.'))
}

fn is_luau_file(file: &str) -> bool {
    file.ends_with(".luau") || file.ends_with(".lua")
}

fn is_script_file(file: &str) -> bool {
    is_luau_file(file) || file.ends_with(".rune") || file.ends_with(".soul")
}

/// Everything before a file name's last dot, as `Path::file_stem` keeps it.
fn file_stem(file: &str) -> &str {
    match file.rfind('.') {
        Some(0) | None => file,
        Some(i) => &file[..i],
    }
}

/// Classes whose folders Studio builds from their `_instance.toml` through
/// `spawn_instance`, like a flat file.
fn spawned_from_file(class: ClassName) -> bool {
    loads_as_part(class)
        || matches!(
            class,
            ClassName::ParticleSimulation
                | ClassName::ParticleSpecies
                | ClassName::TerrainSpline
                | ClassName::TerrainSplinePoint
                | ClassName::TerrainStamp
                | ClassName::TerrainFlattenPad
                | ClassName::TerrainNoise
                | ClassName::TerrainMaterialFill
                | ClassName::TerrainScatter
                | ClassName::TerrainWaterBody
        )
}

/// Classes Studio's loader builds by branches of their own (GUI, media,
/// sky and lighting, lights), whose entities keep the folder's name as it
/// is and their parent's pose. Every other class goes to the general branch,
/// which names and poses it from its file.
fn own_branch(class: ClassName) -> bool {
    matches!(
        class,
        ClassName::ScreenGui
            | ClassName::Frame
            | ClassName::ScrollingFrame
            | ClassName::BillboardGui
            | ClassName::TextLabel
            | ClassName::TextButton
            | ClassName::TextBox
            | ClassName::ImageLabel
            | ClassName::ImageButton
            | ClassName::ViewportFrame
            | ClassName::Image
            | ClassName::Video
            | ClassName::Atmosphere
            | ClassName::Sky
            | ClassName::Clouds
            | ClassName::BloomEffect
            | ClassName::SunRaysEffect
            | ClassName::DirectionalLight
            | ClassName::PointLight
            | ClassName::SpotLight
            | ClassName::SurfaceLight
    )
}

fn is_luau_class(class: ClassName) -> bool {
    matches!(class, ClassName::LuauScript | ClassName::LuauLocalScript | ClassName::LuauModuleScript)
}

// ── The walk ───────────────────────────────────────────────────────────────

struct Reader<'i, 'r> {
    dm: DataModel,
    keys: Vec<(String, InstanceId)>,
    report: TreeReadReport,
    index: &'i Index<'r>,
    /// How this Space composes a child onto its parent.
    rule: TransformRule,
    /// The posed records, when asked for ([`posed_records`]).
    posed: Option<Vec<PosedRecord>>,
    /// The file each instance read from one came from, for the posed
    /// records' parent links.
    file_keys: HashMap<InstanceId, String>,
}

impl Reader<'_, '_> {
    /// The instances in the directory `dir`, under `parent`. `folded` names a
    /// file whose source went into the folder's own script.
    fn walk(&mut self, dir: &str, parent: InstanceId, parent_world: GlobalTransform, service: &str, folded: Option<&str>) {
        let index = self.index;
        let Some(entry) = index.dir(dir) else { return };
        for name in &entry.dirs {
            if skip_dir(name) {
                continue;
            }
            self.folder(&join(dir, name), name, parent, parent_world, service);
        }
        for (file, &i) in &entry.files {
            if file == "_instance.toml" || file == "_service.toml" || file == "_terrain.toml" || folded == Some(file.as_str())
            {
                continue;
            }
            let key = join(dir, file);
            if let Some(stem) = flat_stem(file) {
                // The folder form wins over a flat file of the same name.
                if entry.dirs.contains(stem) {
                    continue;
                }
                // Studio names a flat file up to its first dot when the file
                // gives no name.
                let fallback = file.split('.').next().unwrap_or(stem);
                self.flat_instance(&key, fallback, i, parent, parent_world);
            } else if is_script_file(file) {
                self.bare_script(&key, file, i, parent, service);
            } else {
                self.unread(file);
            }
        }
    }

    /// A part folder's children, as Studio loads them: its subfolders only.
    /// Its files are the part's own assets (a `.glb`, mesh metadata), never
    /// instances.
    fn walk_subfolders(&mut self, dir: &str, parent: InstanceId, parent_world: GlobalTransform, service: &str) {
        let index = self.index;
        let Some(entry) = index.dir(dir) else { return };
        for name in &entry.dirs {
            if !skip_dir(name) {
                self.folder(&join(dir, name), name, parent, parent_world, service);
            }
        }
        for file in entry.files.keys() {
            if file != "_instance.toml" {
                self.unread(file);
            }
        }
    }

    /// A directory, by the branch Studio's loader takes for its class.
    fn folder(&mut self, path: &str, name: &str, parent: InstanceId, parent_world: GlobalTransform, service: &str) {
        let key = format!("{path}/_instance.toml");
        let instance_file = self.index.file(&key);
        if self.index.file(&format!("{path}/_terrain.toml")).is_some()
            || (path == "Workspace/Terrain" && instance_file.is_none())
        {
            self.terrain_dir(path, parent, parent_world, service);
            return;
        }
        // A file that does not parse leaves a Folder, as in Studio.
        let doc = instance_file.and_then(|i| self.parse(&key, i));
        let raw_class = doc.as_ref().and_then(raw_class_name);
        if let Some(raw) = raw_class.filter(|raw| !is_known_class(raw)) {
            self.report.problems.push(format!("{key}: unknown class `{raw}`, read as a Folder"));
        }
        let class = raw_class.map(class_from_toml).unwrap_or(ClassName::Folder);
        match doc {
            Some(doc) if class == ClassName::SoulScript => {
                self.script_folder(path, name, &doc, parent, parent_world, service);
            }
            Some(doc) if spawned_from_file(class) => {
                let rec = record_props(InstanceRecord {
                    doc,
                    key: &key,
                    fallback_name: name,
                    form: RecordForm::Folder,
                    parent_world,
                });
                let id = self.create(&rec, parent, None);
                self.bind(&key, id);
                let basis = self.rule.basis(rec.world);
                self.note(id, &key, class, true, true, rec.world, basis);
                if loads_as_part(class) {
                    self.walk_subfolders(path, id, basis, service);
                } else {
                    self.walk(path, id, basis, service, None);
                }
            }
            doc => self.directory_entity(path, name, doc.as_ref(), class, parent, parent_world, service),
        }
    }

    /// A folder Studio builds from its class. The general branch names it
    /// from its file (else the folder's name, capitalised), poses it from its
    /// file ([`general_branch_pose`]; its children nest under that pose), and
    /// gives it the file's attributes and tags, a Model's stored pivot, and
    /// `Disabled` for a script the file switches off. A branch of its own
    /// keeps the folder's name and its parent's pose, and takes the file's
    /// attributes and tags. A Luau script folder takes a child file's source,
    /// as Studio's seed folds it. A camera also reads its view here, for the
    /// Player's own camera (cameras never replicate).
    #[allow(clippy::too_many_arguments)]
    fn directory_entity(
        &mut self,
        path: &str,
        name: &str,
        doc: Option<&toml::Value>,
        class: ClassName,
        parent: InstanceId,
        parent_world: GlobalTransform,
        service: &str,
    ) {
        let general = !own_branch(class);
        let display = if general {
            doc.and_then(record_name).map_or_else(|| folder_display_name(name), str::to_owned)
        } else {
            name.to_string()
        };
        let world = match doc {
            Some(doc) if general => parent_world.mul_transform(self.rule.folder_pose(class, doc)),
            _ => parent_world,
        };
        let class_name = tree_class(class, false);
        let mut props: Vec<(String, DmValue)> = Vec::new();
        if let Some(doc) = doc {
            if class_name == "Camera" {
                props = camera_props(doc, &parent_world.mul_transform(record_pose(doc)));
            }
            if has_model_pivot(class) {
                props.extend(record_model_pivot(doc).and_then(model_pivot_prop));
            }
            if script_starts_disabled(class, doc) {
                props.push(("Disabled".to_string(), DmValue::Bool(true)));
            }
            if class_name == "Animation" {
                props.extend(record_animation_id(doc).map(|id| ("AnimationId".to_string(), DmValue::String(id))));
            }
            props.extend(record_class_props(&class_name, doc));
            if class_name == "Attachment" {
                props.extend(component_props(&record_attachment(doc, &display)));
            }
            props.extend(data_mesh_props(class, data_mesh_section(doc)));
        }
        let folded = if is_luau_class(class) { self.child_source(path) } else { None };
        if let Some((code, _)) = &folded {
            props.push(("Source".to_string(), DmValue::String(code.clone())));
        }
        let id = self.dm.create_scene(&class_name, &display, Some(parent), props);
        if let Some(doc) = doc {
            self.seed_file_data(id, doc);
        }
        if !reads_own_properties(&class_name) {
            *self.report.unread_properties.entry(class_name.clone()).or_default() += 1;
        }
        self.bind(path, id);
        if doc.is_some() {
            let posed = general && self.rule.folder_is_posed(class);
            self.note(id, &format!("{path}/_instance.toml"), class, posed, false, world, world);
        }
        let folded_file = folded.map(|(_, file)| file);
        self.walk(path, id, world, service, folded_file.as_deref());
    }

    /// A flat instance file. It has no children.
    fn flat_instance(&mut self, key: &str, fallback: &str, i: usize, parent: InstanceId, parent_world: GlobalTransform) {
        let Some(doc) = self.parse(key, i) else { return };
        if let Some(raw) = raw_class_name(&doc).filter(|raw| !is_known_class(raw)) {
            self.unknown_flat(key, fallback, raw, &doc, parent, parent_world);
            return;
        }
        let rec = record_props(InstanceRecord { doc, key, fallback_name: fallback, form: RecordForm::Flat, parent_world });
        let source = rec
            .script
            .as_ref()
            .and_then(|s| s.get("source"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && is_luau_class(rec.engine_class))
            .map(str::to_owned);
        let id = self.create(&rec, parent, source);
        self.bind(key, id);
        let frame = self.rule.basis(rec.world);
        self.note(id, key, rec.engine_class, true, true, rec.world, frame);
    }

    /// A flat file naming a class the engine does not know: the inert
    /// `Folder` Studio makes of it, as of a folder record of that class. It is
    /// named by its file, else its capitalised stem, posed as a folder is, and
    /// given its attributes and tags; its other sections are its class's, so
    /// they stay out of the tree.
    fn unknown_flat(
        &mut self,
        key: &str,
        fallback: &str,
        raw: &str,
        doc: &toml::Value,
        parent: InstanceId,
        parent_world: GlobalTransform,
    ) {
        warn_unknown_class(raw, ClassName::Folder);
        self.report.problems.push(format!("{key}: unknown class `{raw}`, read as a Folder"));
        let display = record_name(doc).map_or_else(|| folder_display_name(fallback), str::to_owned);
        let world = parent_world.mul_transform(self.rule.folder_pose(ClassName::Folder, doc));
        let id = self.dm.create_scene("Folder", &display, Some(parent), Vec::new());
        self.seed_file_data(id, doc);
        self.bind(key, id);
        let posed = self.rule.folder_is_posed(ClassName::Folder);
        self.note(id, key, ClassName::Folder, posed, false, world, world);
    }

    /// Terrain data: not an instance. Its `Layers` folder's layer instances
    /// load beside it, under its parent.
    fn terrain_dir(&mut self, path: &str, parent: InstanceId, parent_world: GlobalTransform, service: &str) {
        let index = self.index;
        let layers = join(path, crate::terrain::layer_instances::LAYERS_FOLDER);
        let Some(entry) = index.dir(&layers) else { return };
        for name in &entry.dirs {
            if !skip_dir(name) {
                self.folder(&join(&layers, name), name, parent, parent_world, service);
            }
        }
    }

    /// A `SoulScript` folder: one script, keyed by its source file, named by
    /// the folder's file (else the folder) and given its attributes and tags.
    /// Studio reads nothing else it holds. Without a source it is a Folder,
    /// built as the general branch builds one.
    fn script_folder(
        &mut self,
        path: &str,
        name: &str,
        doc: &toml::Value,
        parent: InstanceId,
        parent_world: GlobalTransform,
        service: &str,
    ) {
        let named = crate::class_schema::get_section_insensitive(doc, "script")
            .and_then(|s| crate::class_schema::get_section_insensitive(s, "source"))
            .and_then(|s| s.as_str())
            .filter(|s| !s.trim().is_empty());
        // A named source that is missing is no source; only an unnamed one is
        // looked for.
        let source: Option<(String, usize)> = match named {
            Some(rel) => {
                let p = join_rel(path, rel);
                self.index.file(&p).map(|i| (p, i))
            }
            None => self
                .index
                .dir(path)
                .and_then(|e| e.files.iter().find(|(f, _)| is_script_file(f)))
                .map(|(f, &i)| (join(path, f), i)),
        };
        let Some((src_key, i)) = source else {
            let display = record_name(doc).map_or_else(|| folder_display_name(name), str::to_owned);
            let id = self.dm.create_scene("Folder", &display, Some(parent), Vec::new());
            self.seed_file_data(id, doc);
            self.bind(path, id);
            let world = parent_world.mul_transform(self.rule.folder_pose(ClassName::Folder, doc));
            let posed = self.rule.folder_is_posed(ClassName::Folder);
            self.note(id, &format!("{path}/_instance.toml"), ClassName::Folder, posed, false, world, world);
            self.walk(path, id, world, service, None);
            return;
        };
        let file = src_key.rsplit('/').next().unwrap_or(&src_key).to_string();
        let own = record_name(doc).unwrap_or(name);
        let (class, name) = if is_luau_file(&file) {
            (luau_script_class(&file, service).to_string(), strip_script_suffix(own))
        } else {
            ("SoulScript".to_string(), own.to_string())
        };
        let code = String::from_utf8_lossy(self.index.bytes(i)).into_owned();
        let mut props = vec![("Source".to_string(), DmValue::String(code))];
        props.extend(record_class_props(&class, doc));
        let id = self.dm.create_scene(&class, &name, Some(parent), props);
        self.seed_file_data(id, doc);
        self.bind(&src_key, id);
        self.note(id, &format!("{path}/_instance.toml"), ClassName::SoulScript, false, false, parent_world, parent_world);
    }

    /// A bare script file.
    fn bare_script(&mut self, key: &str, file: &str, i: usize, parent: InstanceId, service: &str) {
        let stem = file_stem(file);
        let (class, name) = if is_luau_file(file) {
            (luau_script_class(file, service).to_string(), strip_script_suffix(stem))
        } else {
            ("SoulScript".to_string(), stem.to_string())
        };
        let code = String::from_utf8_lossy(self.index.bytes(i)).into_owned();
        let id = self.dm.create_scene(&class, &name, Some(parent), vec![("Source".to_string(), DmValue::String(code))]);
        self.bind(key, id);
    }

    /// The first script file directly in `path` with any source, and its
    /// name: what Studio's seed folds into an empty Luau script folder.
    fn child_source(&self, path: &str) -> Option<(String, String)> {
        let entry = self.index.dir(path)?;
        entry.files.iter().find_map(|(file, &i)| {
            (is_script_file(file) && !self.index.bytes(i).is_empty())
                .then(|| (String::from_utf8_lossy(self.index.bytes(i)).into_owned(), file.clone()))
        })
    }

    /// Make an instance from a read file: its properties, a script's source,
    /// `Disabled` for a script its section switches off, then its attributes
    /// and tags.
    fn create(&mut self, rec: &RecordInstance, parent: InstanceId, source: Option<String>) -> InstanceId {
        let mut props = rec.props.clone();
        if let Some(code) = source {
            props.push(("Source".to_string(), DmValue::String(code)));
        }
        // Studio's seed: a Script or LocalScript whose section says
        // `enabled = false` is a disabled template, never started where it sits.
        if matches!(rec.class.as_str(), "Script" | "LocalScript")
            && rec.attributes.iter().any(|(k, v)| k == "enabled" && *v == DmValue::Bool(false))
        {
            props.push(("Disabled".to_string(), DmValue::Bool(true)));
        }
        let id = self.dm.create_scene(&rec.class, &rec.name, Some(parent), props);
        for (k, v) in &rec.attributes {
            self.dm.seed_attribute(id, k, v.clone());
        }
        for tag in &rec.tags {
            self.dm.seed_tag(id, tag);
        }
        if !reads_own_properties(&rec.class) {
            *self.report.unread_properties.entry(rec.class.clone()).or_default() += 1;
        }
        self.report.problems.extend(rec.problems.iter().cloned());
        id
    }

    /// Note an instance read from `key` for [`posed_records`], linked to the
    /// nearest ancestor read from a file.
    #[allow(clippy::too_many_arguments)]
    fn note(
        &mut self,
        id: InstanceId,
        key: &str,
        class: ClassName,
        posed: bool,
        from_file: bool,
        world: GlobalTransform,
        frame: GlobalTransform,
    ) {
        if self.posed.is_none() {
            return;
        }
        let mut up = self.dm.parent(id);
        let parent = loop {
            match up {
                Some(p) => match self.file_keys.get(&p) {
                    Some(k) => break Some(k.clone()),
                    None => up = self.dm.parent(p),
                },
                None => break None,
            }
        };
        if let Some(list) = self.posed.as_mut() {
            list.push(PosedRecord { key: key.to_string(), parent, class, posed, from_file, world, frame });
        }
        self.file_keys.insert(id, key.to_string());
    }

    /// A file's attributes and tags on an instance built without
    /// [`record_props`], as Studio's loader branches fill them.
    fn seed_file_data(&mut self, id: InstanceId, doc: &toml::Value) {
        for (name, value) in record_attribute_values(doc) {
            if let Some(dv) = attribute_to_dm(&value) {
                self.dm.seed_attribute(id, &name, dv);
            }
        }
        for tag in record_tags(doc) {
            self.dm.seed_tag(id, &tag);
        }
    }

    fn bind(&mut self, key: &str, id: InstanceId) {
        self.keys.push((key.to_string(), id));
        self.report.instances += 1;
    }

    fn parse(&mut self, key: &str, i: usize) -> Option<toml::Value> {
        let text = match std::str::from_utf8(self.index.bytes(i)) {
            Ok(t) => t,
            Err(e) => {
                self.report.problems.push(format!("{key}: {e}"));
                return None;
            }
        };
        match text.parse::<toml::Value>() {
            Ok(doc) => Some(doc),
            Err(e) => {
                self.report.problems.push(format!("{key}: {e}"));
                None
            }
        }
    }

    fn unread(&mut self, file: &str) {
        let kind = match file.rfind('.') {
            Some(i) if i > 0 => file[i + 1..].to_lowercase(),
            _ => file.to_string(),
        };
        *self.report.unread_files.entry(kind).or_default() += 1;
    }
}

// ── Disk ───────────────────────────────────────────────────────────────────

/// Names Studio never loads or publishes: hidden entries, the database and
/// its backups, temporaries and folders the loader skips.
fn skip_name(name: &str) -> bool {
    name.starts_with('.')
        || name.starts_with("world.fjalldb")
        || name.starts_with("header.bin")
        || name.ends_with(".tmp")
        || name.ends_with(".lock")
        || matches!(name, "Thumbs.db" | "desktop.ini" | "node_modules" | "target" | "trash")
}

/// Whether the reader looks inside a file: instance and service files and
/// script sources. Everything else only needs to exist.
fn needs_bytes(name: &str) -> bool {
    name.ends_with(".toml") || is_script_file(name)
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>, problems: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            problems.push(format!("{}: {e}", dir.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if skip_name(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            collect(base, &path, out, problems);
        } else if kind.is_file() {
            let rel = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let bytes = if needs_bytes(&name) {
                match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => {
                        problems.push(format!("{rel}: {e}"));
                        continue;
                    }
                }
            } else {
                Vec::new()
            };
            out.push((rel, bytes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(path: &str, text: &str) -> (String, Vec<u8>) {
        (path.to_string(), text.as_bytes().to_vec())
    }

    fn part(name: &str, pos: [f32; 3], scale: [f32; 3]) -> String {
        format!(
            "[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\n\n\
             [transform]\nposition = [{}, {}, {}]\nscale = [{}, {}, {}]\n\n[properties]\nanchored = true\n",
            pos[0], pos[1], pos[2], scale[0], scale[1], scale[2]
        )
    }

    fn world() -> Vec<(String, Vec<u8>)> {
        vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec("Workspace/Ball/_instance.toml", &part("Ball", [0.0, 1.0, 0.0], [1.0, 1.0, 1.0])),
            rec("Workspace/Wall.instance.toml", &part("Wall", [5.0, 0.0, 0.0], [1.0, 4.0, 20.0])),
            // A flat duplicate of the Ball folder: the folder wins.
            rec("Workspace/Ball.instance.toml", &part("Ghost", [9.0, 9.0, 9.0], [1.0, 1.0, 1.0])),
            // A Model named, posed and described by its file.
            rec(
                "Workspace/court/_instance.toml",
                "tags = [\"arena\"]\n\n[metadata]\nclass_name = \"Model\"\nname = \"Arena\"\n\n\
                 [transform]\nposition = [100.0, 0.0, 0.0]\n\n[attributes]\nlit = true\n\n\
                 [model]\nworld_pivot = { position = [100.0, 1.0, 0.0], rotation = [0.0, 0.0, 0.0, 1.0] }\n",
            ),
            rec("Workspace/court/Net/_instance.toml", &part("Net", [0.0, 2.0, 0.0], [1.0, 1.0, 1.0])),
            // A part nested in a part sits on its parent's pose, size included.
            rec("Workspace/Base/_instance.toml", &part("Base", [0.0, 0.0, 10.0], [2.0, 1.0, 2.0])),
            rec("Workspace/Base/Flag/_instance.toml", &part("Flag", [1.0, 1.0, 0.0], [0.2, 2.0, 0.2])),
            rec("Workspace/Props/Cone.instance.toml", &part("Cone", [1.0, 0.0, 1.0], [1.0, 1.0, 1.0])),
            rec("Workspace/Props/model.glb", ""),
            rec(
                "Workspace/Mover/_instance.toml",
                "[metadata]\nclass_name = \"SoulScript\"\n\n[script]\nsource = \"\"\n",
            ),
            rec("Workspace/Mover/Mover.server.luau", "print('move')"),
            rec(
                "Workspace/Clock/_instance.toml",
                "[metadata]\nclass_name = \"LuauScript\"\n\n[script]\nenabled = false\n",
            ),
            rec("Workspace/Clock/script.luau", "print('tick')"),
            rec("StarterPlayer/Hud.client.luau", "print('hud')"),
            rec("ReplicatedStorage/Util.luau", "return {}"),
            rec(
                "Workspace/Camera/_instance.toml",
                "[metadata]\nclass_name = \"Camera\"\n\n[transform]\nposition = [0.0, 30.0, 0.0]\n\n\
                 [camera]\ncamera_type = \"Scriptable\"\n",
            ),
            // Terrain data, and one of its layers.
            rec("Workspace/Terrain/_terrain.toml", "[terrain]\nchunk_size = 32\n"),
            rec(
                "Workspace/Terrain/Layers/Road/_instance.toml",
                "[metadata]\nclass_name = \"TerrainSpline\"\nname = \"Road\"\n",
            ),
        ]
    }

    #[cfg(feature = "units_v1")]
    #[test]
    fn starter_player_carries_its_character_settings() {
        let mut records = world();
        records.push(rec(
            "StarterPlayer/_service.toml",
            "[service]\nclass_name = \"StarterPlayer\"\n\n[properties]\ncharacter_walk_speed = 16.0\ncharacter_use_jump_power = false\n",
        ));
        let tree = read_space_records(&records);
        let sp = tree.dm.find_service("StarterPlayer").expect("StarterPlayer");
        let want = 16.0 * crate::units::Unit::Stud.to_meters();
        assert!(
            matches!(tree.dm.get_prop(sp, "CharacterWalkSpeed"), Some(DmValue::Number(n)) if (n - want).abs() < 1e-9),
            "{:?}",
            tree.dm.get_prop(sp, "CharacterWalkSpeed")
        );
        assert_eq!(tree.dm.get_prop(sp, "CharacterUseJumpPower"), Some(DmValue::Bool(false)), "as the file sets it");
    }

    #[test]
    fn a_roblox_script_folder_carries_its_origin() {
        let mut records = world();
        records.push(rec(
            "ServerScriptService/Rb/_instance.toml",
            "[metadata]\nclass_name = \"SoulScript\"\n\n[script]\norigin = \"roblox\"\n",
        ));
        records.push(rec("ServerScriptService/Rb/Rb.server.luau", "print('rb')"));
        let tree = read_space_records(&records);
        let rb = find(&tree, "ServerScriptService.Rb");
        assert_eq!(tree.dm.class_of(rb), Some("Script"));
        assert_eq!(tree.dm.get_prop(rb, "ScriptOrigin"), Some(DmValue::String("roblox".into())));
        let mover = find(&tree, "Workspace.Mover");
        assert_eq!(tree.dm.get_prop(mover, "ScriptOrigin"), None, "a script without an origin carries none");
    }

    #[test]
    fn a_flat_file_of_an_unknown_class_is_a_folder_like_its_folder_form() {
        let mut records = world();
        records.push(rec(
            "Workspace/gizmo.instance.toml",
            "tags = [\"odd\"]\n\n[metadata]\nclass_name = \"Gizmo9000\"\n\n[transform]\nposition = [1.0, 2.0, 3.0]\n\n\
             [attributes]\nlevel = 3\n\n[gizmo]\nspin = 4\n",
        ));
        let tree = read_space_records(&records);
        let gizmo = find(&tree, "Workspace.Gizmo");
        assert_eq!(tree.dm.class_of(gizmo), Some("Folder"));
        assert_eq!(key_of(&tree, gizmo), "Workspace/gizmo.instance.toml");
        assert_eq!(tree.dm.get_attribute(gizmo, "level"), Some(DmValue::Number(3.0)));
        assert_eq!(tree.dm.get_attribute(gizmo, "spin"), None, "its class's section is not attributes");
        assert!(
            tree.report.problems.iter().any(|p| p.contains("unknown class `Gizmo9000`")),
            "{:?}",
            tree.report.problems
        );
    }

    fn find(tree: &SceneTree, path: &str) -> InstanceId {
        let dm = &tree.dm;
        let mut cur = dm.root();
        for name in path.split('.') {
            cur = dm.find_first_child(cur, name, false).unwrap_or_else(|| panic!("no {name} in {path}"));
        }
        cur
    }

    fn key_of(tree: &SceneTree, id: InstanceId) -> &str {
        tree.keys.iter().find(|(_, i)| *i == id).map(|(k, _)| k.as_str()).expect("instance has a key")
    }

    fn position(tree: &SceneTree, id: InstanceId) -> Vec3 {
        match tree.dm.get_prop(id, "CFrame") {
            Some(DmValue::CFrame(cf)) => cf.position.to_vec3(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_hierarchy_and_keys_are_studios() {
        let tree = read_space_records(&world());
        assert!(tree.report.problems.is_empty(), "{:?}", tree.report.problems);
        let ball = find(&tree, "Workspace.Ball");
        assert_eq!(key_of(&tree, ball), "Workspace/Ball/_instance.toml");
        assert!(tree.dm.find_first_child(tree.dm.root(), "Ghost", true).is_none(), "the flat duplicate lost to its folder");
        assert_eq!(key_of(&tree, find(&tree, "Workspace.Wall")), "Workspace/Wall.instance.toml");
        let props = find(&tree, "Workspace.Props");
        assert_eq!(tree.dm.class_of(props), Some("Folder"), "a plain directory is a Folder");
        assert_eq!(key_of(&tree, props), "Workspace/Props");
        assert_eq!(key_of(&tree, find(&tree, "Workspace.Props.Cone")), "Workspace/Props/Cone.instance.toml");
        assert_eq!(tree.report.unread_files.get("glb"), Some(&1));
        assert!(tree.dm.find_service("Workspace").is_some());
        assert!(tree.keys.iter().all(|(k, _)| k != "Workspace"), "services are bound by class");
    }

    #[test]
    fn a_models_folder_takes_its_name_pose_and_data_from_its_file() {
        let tree = read_space_records(&world());
        let arena = find(&tree, "Workspace.Arena");
        assert_eq!(tree.dm.class_of(arena), Some("Model"));
        assert_eq!(key_of(&tree, arena), "Workspace/court", "keyed by its folder");
        assert_eq!(tree.dm.attributes(arena), vec![("lit".to_string(), DmValue::Bool(true))]);
        assert_eq!(tree.dm.tags_of(arena), vec!["arena".to_string()]);
        assert!(matches!(tree.dm.get_prop(arena, "WorldPivot"), Some(DmValue::CFrame(_))), "its stored pivot");
        // Its pose places its child.
        let net = find(&tree, "Workspace.Arena.Net");
        assert!((position(&tree, net) - Vec3::new(100.0, 2.0, 0.0)).length() < 1e-4, "{}", position(&tree, net));
    }

    #[test]
    fn a_part_in_a_part_sits_on_its_parents_pose_size_included() {
        let tree = read_space_records(&world());
        let flag = find(&tree, "Workspace.Base.Flag");
        // Base at (0,0,10), size (2,1,2): the child's (1,1,0) scales to (2,1,0).
        assert!((position(&tree, flag) - Vec3::new(2.0, 1.0, 10.0)).length() < 1e-4, "{}", position(&tree, flag));
    }

    #[test]
    fn an_animation_folder_carries_its_clip() {
        let records = vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec(
                "Workspace/Walk/_instance.toml",
                "[metadata]\nclass_name = \"Animation\"\n\n[properties]\nanimation_id = \"rbxassetid://507\"\n",
            ),
            rec(
                "Workspace/Run.instance.toml",
                "[metadata]\nclass_name = \"Animation\"\nname = \"Run\"\n\n[properties]\nanimation_id = \"rbxassetid://913\"\n",
            ),
        ];
        let tree = read_space_records(&records);
        let clip = |path: &str| tree.dm.get_prop(find(&tree, path), "AnimationId");
        assert_eq!(clip("Workspace.Walk"), Some(DmValue::String("rbxassetid://507".into())));
        // A flat Animation file keeps the class default, as in Studio, whose
        // typed instance definition has no clip field yet.
        assert_eq!(clip("Workspace.Run"), Some(DmValue::String(String::new())));
    }

    #[test]
    fn a_part_class_folder_is_a_part_and_loads_only_its_subfolders() {
        // No `[asset]`, as the importer writes seats: a part class draws the
        // default block.
        let seat = part("Chair", [0.0, 1.0, 0.0], [2.0, 1.0, 2.0])
            .replace("class_name = \"Part\"", "class_name = \"VehicleSeat\"");
        let records = vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec("Workspace/Chair/_instance.toml", &seat),
            rec("Workspace/Chair/csg.glb", ""),
            rec("Workspace/Chair/Stray.instance.toml", &part("Stray", [0.0, 0.0, 0.0], [1.0, 1.0, 1.0])),
            rec("Workspace/Chair/Hud/_instance.toml", "[metadata]\nclass_name = \"Folder\"\n"),
        ];
        let tree = read_space_records(&records);
        let chair = find(&tree, "Workspace.Chair");
        assert_eq!(tree.dm.class_of(chair), Some("VehicleSeat"));
        assert!((position(&tree, chair) - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4, "a seat is a part");
        assert!(tree.dm.find_first_child(chair, "Stray", false).is_none(), "a part folder's files are its assets");
        assert!(tree.dm.find_first_child(chair, "Hud", false).is_some(), "its subfolders load");
        assert_eq!(tree.report.unread_files.get("glb"), Some(&1));
    }

    #[test]
    fn under_the_parent_pose_rule_a_child_ignores_its_parents_size() {
        let mut records = world();
        records.push(rec("space.toml", "[space]\ntransform_rule = \"parent_pose\"\n"));
        let tree = read_space_records(&records);
        let flag = find(&tree, "Workspace.Base.Flag");
        // Base at (0,0,10), size (2,1,2): the child's (1,1,0) stays (1,1,0).
        assert!((position(&tree, flag) - Vec3::new(1.0, 1.0, 10.0)).length() < 1e-4, "{}", position(&tree, flag));
    }

    #[test]
    fn scripts_load_as_studio_loads_them() {
        let tree = read_space_records(&world());
        let dm = &tree.dm;

        // A SoulScript folder is one script, keyed by its source file.
        let mover = find(&tree, "Workspace.Mover");
        assert_eq!(dm.class_of(mover), Some("Script"), "a .server.luau source is a Script");
        assert_eq!(key_of(&tree, mover), "Workspace/Mover/Mover.server.luau");
        assert_eq!(dm.get_prop(mover, "Source"), Some(DmValue::String("print('move')".into())));
        assert!(dm.children(mover).is_empty(), "nothing else in a script folder loads");

        // A Luau script folder folds its file's source, and its file's
        // `[script] enabled = false` makes it a disabled template.
        let clock = find(&tree, "Workspace.Clock");
        assert_eq!(dm.class_of(clock), Some("Script"));
        assert_eq!(key_of(&tree, clock), "Workspace/Clock");
        assert_eq!(dm.get_prop(clock, "Source"), Some(DmValue::String("print('tick')".into())));
        assert!(dm.children(clock).is_empty(), "the folded file is not an instance");
        assert_eq!(dm.get_prop(clock, "Disabled"), Some(DmValue::Bool(true)));

        // Bare files by their Rojo names and service.
        let hud = find(&tree, "StarterPlayer.Hud");
        assert_eq!(dm.class_of(hud), Some("LocalScript"));
        assert_eq!(key_of(&tree, hud), "StarterPlayer/Hud.client.luau");
        assert_eq!(dm.class_of(find(&tree, "ReplicatedStorage.Util")), Some("ModuleScript"));
    }

    #[test]
    fn terrain_data_is_not_an_instance_and_the_handle_is_made_once() {
        let tree = read_space_records(&world());
        let dm = &tree.dm;
        let ws = dm.find_service("Workspace").unwrap();
        let terrains: Vec<_> = dm.children(ws).iter().filter(|c| dm.class_of(**c) == Some("Terrain")).collect();
        assert_eq!(terrains.len(), 1, "exactly one Terrain");
        assert_eq!(dm.children(ws).first(), terrains.first().copied(), "first among the Workspace's children");
        assert!(tree.keys.iter().all(|(k, _)| !k.starts_with("Workspace/Terrain/_")), "the data folder is no instance");
        let road = find(&tree, "Workspace.Road");
        assert_eq!(dm.class_of(road), Some("TerrainSpline"), "a layer loads beside the terrain folder");
        assert_eq!(key_of(&tree, road), "Workspace/Terrain/Layers/Road/_instance.toml");
    }

    #[test]
    fn a_loaded_world_reaches_the_apply_step_as_spawns_only() {
        let mut tree = read_space_records(&world());
        let dm = &mut tree.dm;
        assert!(dm.take_dirty().is_empty(), "loading marks nothing dirty");
        let spawns = dm.take_spawns();
        let ball = dm.find_first_child(dm.find_service("Workspace").unwrap(), "Ball", false).unwrap();
        assert!(spawns.contains(&ball));
        assert!(spawns.iter().all(|id| !crate::datamodel::is_service_class(dm.class_of(*id).unwrap_or(""))));
        assert!(dm.take_spawns().is_empty(), "each instance spawns once");
    }

    #[test]
    fn the_camera_is_the_spaces() {
        let tree = read_space_records(&world());
        let cam = tree.dm.current_camera().expect("the Space's camera");
        assert_eq!(
            tree.dm.get_prop(cam, "CameraType").as_ref().and_then(DmValue::as_enum_name),
            Some("Scriptable")
        );
        assert!((position(&tree, cam) - Vec3::new(0.0, 30.0, 0.0)).length() < 1e-4, "the camera keeps its pose");
    }

    #[test]
    fn a_space_on_disk_reads_like_its_records() {
        let root = std::env::temp_dir().join(format!("eustress_tree_read_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, bytes) in world() {
            let file = root.join(&path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, bytes).unwrap();
        }
        std::fs::create_dir_all(root.join(".eustress")).unwrap();
        std::fs::write(root.join(".eustress/output.log"), b"noise").unwrap();

        let from_disk = read_space_dir(&root).expect("the Space reads");
        let _ = std::fs::remove_dir_all(&root);
        let from_records = read_space_records(&world());

        let mut a: Vec<&str> = from_disk.keys.iter().map(|(k, _)| k.as_str()).collect();
        let mut b: Vec<&str> = from_records.keys.iter().map(|(k, _)| k.as_str()).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        assert_eq!(from_disk.report.instances, from_records.report.instances);
    }

    #[test]
    fn posed_records_keep_one_structure_under_either_rule() {
        let legacy = posed_records(&world(), TransformRule::Legacy);
        let parent_pose = posed_records(&world(), TransformRule::ParentPose);
        let shape = |list: &[PosedRecord]| list.iter().map(|r| (r.key.clone(), r.parent.clone())).collect::<Vec<_>>();
        assert!(!legacy.is_empty());
        assert_eq!(shape(&legacy), shape(&parent_pose));
        let flag = legacy.iter().find(|r| r.key == "Workspace/Base/Flag/_instance.toml").expect("the flag is posed");
        assert_eq!(flag.parent.as_deref(), Some("Workspace/Base/_instance.toml"));
        assert!(flag.posed && flag.from_file, "a part is posed and built from its file");
        assert!(legacy.iter().all(|r| !r.key.starts_with("Workspace/Props/_")), "a folder with no file is left out");
    }

    #[test]
    fn a_folder_without_a_file_passes_its_parents_frame_through() {
        let records = vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec("Workspace/A/_instance.toml", &part("A", [0.0, 0.0, 10.0], [2.0, 1.0, 2.0])),
            rec("Workspace/A/Group/Flag/_instance.toml", &part("Flag", [1.0, 1.0, 0.0], [0.2, 2.0, 0.2])),
            rec("Workspace/B/_instance.toml", &part("B", [0.0, 0.0, 10.0], [2.0, 1.0, 2.0])),
            rec("Workspace/B/Flag/_instance.toml", &part("Flag", [1.0, 1.0, 0.0], [0.2, 2.0, 0.2])),
        ];
        for rule in [TransformRule::Legacy, TransformRule::ParentPose] {
            let posed = posed_records(&records, rule);
            let get = |key: &str| posed.iter().find(|r| r.key == key).unwrap_or_else(|| panic!("{key} under {rule:?}"));
            let through = get("Workspace/A/Group/Flag/_instance.toml");
            let direct = get("Workspace/B/Flag/_instance.toml");
            assert_eq!(through.parent.as_deref(), Some("Workspace/A/_instance.toml"), "the fileless folder is skipped");
            assert!((through.world.translation() - direct.world.translation()).length() < 1e-5, "{rule:?}");
        }
    }

    #[test]
    fn an_attachment_carries_its_part_local_cframe() {
        let records = vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec("Workspace/A/_instance.toml", &part("A", [0.0, 0.0, 5.0], [2.0, 1.0, 2.0])),
            rec(
                "Workspace/A/Grip/_instance.toml",
                "[metadata]\nclass_name = \"Attachment\"\n\n[transform]\nposition = [1.0, 0.0, 0.0]\n",
            ),
        ];
        let tree = read_space_records(&records);
        let grip = find(&tree, "Workspace.A.Grip");
        match tree.dm.get_prop(grip, "CFrame") {
            Some(DmValue::CFrame(cf)) => assert!((cf.position.to_vec3() - Vec3::new(1.0, 0.0, 0.0)).length() < 1e-5),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_attachment_is_posed_only_under_the_parent_pose_rule() {
        let records = vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec("Workspace/A/_instance.toml", &part("A", [0.0, 0.0, 0.0], [2.0, 1.0, 2.0])),
            rec(
                "Workspace/A/Grip/_instance.toml",
                "[metadata]\nclass_name = \"Attachment\"\n\n[transform]\nposition = [1.0, 0.0, 0.0]\n",
            ),
        ];
        let grip = |rule: TransformRule| {
            posed_records(&records, rule).into_iter().find(|r| r.key == "Workspace/A/Grip/_instance.toml").expect("the grip")
        };
        assert!(!grip(TransformRule::Legacy).posed);
        assert!(grip(TransformRule::ParentPose).posed);
        assert!(!grip(TransformRule::ParentPose).from_file, "the general branch ignores its file scale");
    }

    #[test]
    fn a_keyframe_sequence_folder_carries_its_settings() {
        let records = vec![
            rec("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            rec(
                "Workspace/Wave/_instance.toml",
                "[metadata]\nclass_name = \"KeyframeSequence\"\n\n[keyframe_sequence]\nloop = false\npriority = \"Action2\"\n",
            ),
        ];
        let tree = read_space_records(&records);
        let wave = find(&tree, "Workspace.Wave");
        assert_eq!(tree.dm.class_of(wave), Some("KeyframeSequence"));
        assert_eq!(tree.dm.get_prop(wave, "Loop"), Some(DmValue::Bool(false)));
        assert_eq!(
            tree.dm.get_prop(wave, "Priority").as_ref().and_then(DmValue::as_enum_name),
            Some("Action2")
        );
    }

    #[test]
    fn a_folder_without_workspace_is_not_a_space() {
        let tmp = std::env::temp_dir().join(format!("eustress_tree_read_neg_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        assert!(read_space_dir(&tmp).is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
