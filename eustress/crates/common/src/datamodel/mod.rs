//! # Live DataModel
//!
//! The script-facing instance tree of a running Space: `game`, its services,
//! and every instance below them, with Roblox semantics (Name, ClassName,
//! Parent, typed properties, attributes, tags). Luau and Rune both bind to
//! this one tree, so a part moved by a Rune script reads back moved in Luau
//! the same frame, and a signal fired by one language reaches the other.
//!
//! ## Frame protocol
//!
//! The engine owns the ECS; scripts own nothing but this tree. Once per frame:
//!
//! 1. **Pull** (engine): physics-driven poses, input, the mouse ray and hit,
//!    camera state, collisions and GUI clicks are written in with the
//!    `*_from_engine` / `push_event` calls. Those writes never mark anything
//!    dirty.
//! 2. **Scripts** run. Every property write goes through [`DataModel::set_prop`],
//!    which stores the value and marks it dirty; `Instance.new`, `Clone`,
//!    `Parent =` and `Destroy` restructure the tree immediately, so reads
//!    later in the same frame see the new state.
//! 3. **Apply** (engine): [`DataModel::take_spawns`], [`DataModel::take_reparents`],
//!    [`DataModel::take_dirty`], [`DataModel::take_despawns`] and the command
//!    queues are drained and written into the ECS.
//!
//! ## Identity
//!
//! An [`InstanceId`] is a slot index plus a generation, so a handle a script
//! kept after `Destroy` reads as dead instead of aliasing whatever reuses
//! the slot. Instances bound to an ECS entity carry its bits in
//! [`LiveInstance::entity`].

mod classes;
mod value;

pub use classes::*;
pub use value::*;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use crate::scripting::{CFrame, Vector2, Vector3};

/// The tree, shared between the engine systems and both script runtimes.
pub type SharedDataModel = Arc<parking_lot::Mutex<DataModel>>;

/// A fresh, empty tree behind the shared lock.
pub fn new_shared() -> SharedDataModel {
    Arc::new(parking_lot::Mutex::new(DataModel::new()))
}

/// The tree of the running Play session, reachable from any thread. Rune's
/// native functions take no Bevy parameters, so this (not a thread-local)
/// is how they find it.
static ACTIVE: std::sync::RwLock<Option<SharedDataModel>> = std::sync::RwLock::new(None);

/// Publish (or clear, with `None`) the running session's tree.
pub fn set_active(dm: Option<SharedDataModel>) {
    if let Ok(mut slot) = ACTIVE.write() {
        *slot = dm;
    }
}

/// The running session's tree, if a Play session is live.
pub fn active() -> Option<SharedDataModel> {
    ACTIVE.read().ok().and_then(|slot| slot.clone())
}

// ============================================================================
// Identity
// ============================================================================

/// Slot index (low 32 bits) plus generation (high 32 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InstanceId(pub u64);

impl InstanceId {
    fn new(index: u32, generation: u32) -> Self {
        Self(((generation as u64) << 32) | index as u64)
    }
    pub fn index(self) -> u32 {
        self.0 as u32
    }
    pub fn generation(self) -> u32 {
        (self.0 >> 32) as u32
    }
}

/// Where an instance came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Seeded from an entity the Space loaded.
    Scene,
    /// Made by a script (`Instance.new`, `Clone`), or synthesised by the
    /// engine for Play (players, characters).
    Script,
}

/// One instance.
#[derive(Debug, Clone)]
pub struct LiveInstance {
    pub id: InstanceId,
    pub class_name: String,
    pub name: String,
    pub parent: Option<InstanceId>,
    pub children: Vec<InstanceId>,
    pub props: HashMap<String, DmValue>,
    pub attributes: BTreeMap<String, DmValue>,
    pub tags: BTreeSet<String>,
    /// Bits of the bound ECS entity, if any.
    pub entity: Option<u64>,
    pub origin: Origin,
    /// For a `Clone`, the instance it was copied from. The engine copies the
    /// source entity's components when it spawns the clone, so meshes and
    /// materials carry over even when no property names them.
    pub clone_of: Option<InstanceId>,
    pub archivable: bool,
    pub destroyed: bool,
    /// Set by `Destroy`: the Parent can no longer change.
    pub parent_locked: bool,
    /// Properties written by scripts since the engine last applied them.
    pub dirty: Vec<String>,
    in_dirty_list: bool,
    /// A script listens to `Changed` / `GetPropertyChangedSignal` here, so
    /// every write also queues a [`DmEvent::Changed`].
    pub watch_changes: bool,
    queued_spawn: bool,
}

impl LiveInstance {
    fn new(id: InstanceId, class_name: &str, name: &str, origin: Origin) -> Self {
        Self {
            id,
            class_name: class_name.to_string(),
            name: name.to_string(),
            parent: None,
            children: Vec::new(),
            props: HashMap::default(),
            attributes: BTreeMap::new(),
            tags: BTreeSet::new(),
            entity: None,
            origin,
            clone_of: None,
            archivable: true,
            destroyed: false,
            parent_locked: false,
            dirty: Vec::new(),
            in_dirty_list: false,
            watch_changes: false,
            queued_spawn: false,
        }
    }

    pub fn is_a(&self, base: &str) -> bool {
        class_is_a(&self.class_name, base)
    }

    pub fn prop(&self, name: &str) -> Option<&DmValue> {
        self.props.get(name)
    }

    pub fn cframe(&self) -> Option<CFrame> {
        self.props.get("CFrame").and_then(DmValue::as_cframe)
    }
}

struct Slot {
    generation: u32,
    inst: Option<LiveInstance>,
}

// ============================================================================
// Events and commands
// ============================================================================

/// Something scripts may react to. Queued, then fired by each runtime at
/// its next resumption point (Roblox's deferred signal behaviour).
#[derive(Debug, Clone, PartialEq)]
pub enum DmEvent {
    ChildAdded { parent: InstanceId, child: InstanceId },
    ChildRemoved { parent: InstanceId, child: InstanceId },
    DescendantAdded { ancestor: InstanceId, descendant: InstanceId },
    DescendantRemoving { ancestor: InstanceId, descendant: InstanceId },
    AncestryChanged { id: InstanceId, parent: Option<InstanceId> },
    Destroying { id: InstanceId },
    Changed { id: InstanceId, prop: String },
    AttributeChanged { id: InstanceId, name: String },
    TagAdded { id: InstanceId, tag: String },
    TagRemoved { id: InstanceId, tag: String },
    Touched { part: InstanceId, other: InstanceId },
    TouchEnded { part: InstanceId, other: InstanceId },
    PlayerAdded { player: InstanceId },
    PlayerRemoving { player: InstanceId },
    CharacterAdded { player: InstanceId, character: InstanceId },
    CharacterRemoving { player: InstanceId, character: InstanceId },
    /// A Humanoid's Health reached zero.
    Died { humanoid: InstanceId },
    MoveToFinished { humanoid: InstanceId, reached: bool },
    /// A GuiButton was clicked (MouseButton1Click / Activated).
    GuiActivated { button: InstanceId },
    /// A BindableEvent fired from either language. Luau delivers its own
    /// fires directly (tables and functions intact), so it skips events
    /// marked `from_luau`; Rune reads them all.
    Fired { event: InstanceId, args: Vec<DmValue>, from_luau: bool },
}

/// Output routed to the Studio Output panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub struct OutputLine {
    pub level: OutputLevel,
    pub source: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundAction {
    Play,
    Stop,
    Pause,
    Resume,
}

#[derive(Debug, Clone)]
pub struct SoundCommand {
    pub sound: InstanceId,
    pub action: SoundAction,
}

#[derive(Debug, Clone)]
pub enum HumanoidCommand {
    /// `Humanoid:Move(direction)`: walk this way until told otherwise.
    Move { humanoid: InstanceId, direction: Vector3 },
    /// `Humanoid:MoveTo(point)`.
    MoveTo { humanoid: InstanceId, target: Vector3 },
    Jump { humanoid: InstanceId },
}

#[derive(Debug, Clone)]
pub enum PhysicsCommand {
    ApplyImpulse { part: InstanceId, impulse: Vector3 },
    ApplyAngularImpulse { part: InstanceId, impulse: Vector3 },
}

// ============================================================================
// Per-frame state the engine writes before scripts run
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct FrameState {
    /// Seconds since Play started (Roblox `time()`).
    pub time: f64,
    /// Seconds since the previous frame.
    pub dt: f64,
    pub frame: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPhase {
    Began,
    Changed,
    Ended,
}

/// One input transition this frame, in Roblox names.
#[derive(Debug, Clone)]
pub struct InputEvent {
    pub phase: InputPhase,
    /// `Keyboard`, `MouseButton1`, `MouseButton2`, `MouseButton3`,
    /// `MouseMovement`, `MouseWheel`.
    pub input_type: String,
    /// `W`, `LeftShift`, ... or `Unknown` for mouse input.
    pub key_code: String,
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
    /// Wheel steps for `MouseWheel` (positive = away from the user).
    pub wheel: f64,
    /// True when the input landed on a GUI element or an editor panel.
    pub game_processed: bool,
}

#[derive(Debug, Clone)]
pub struct InputState {
    pub keys: BTreeSet<String>,
    pub buttons: BTreeSet<String>,
    /// Cursor in viewport pixels, origin top-left.
    pub mouse_x: f64,
    pub mouse_y: f64,
    pub mouse_dx: f64,
    pub mouse_dy: f64,
    pub wheel: f64,
    pub viewport_w: f64,
    pub viewport_h: f64,
    /// The 3D viewport holds keyboard focus (typing in a panel does not
    /// reach gameplay).
    pub viewport_focused: bool,
    pub events: Vec<InputEvent>,
    pub mouse_icon_enabled: bool,
    /// `Default`, `LockCenter`, `LockCurrentPosition`.
    pub mouse_behavior: String,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
            mouse_x: 0.0,
            mouse_y: 0.0,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            wheel: 0.0,
            viewport_w: 1280.0,
            viewport_h: 720.0,
            viewport_focused: true,
            events: Vec::new(),
            mouse_icon_enabled: true,
            mouse_behavior: "Default".into(),
        }
    }
}

/// Where the cursor's ray meets the world, recomputed by the engine each
/// frame (Roblox `Mouse.Hit` / `Mouse.Target`).
#[derive(Debug, Clone, Default)]
pub struct MouseState {
    pub ray_origin: Vector3,
    pub ray_direction: Vector3,
    pub has_hit: bool,
    pub hit_position: Vector3,
    pub hit_normal: Vector3,
    pub target: Option<InstanceId>,
    /// `Mouse.TargetFilter`: this instance and its descendants are ignored.
    pub target_filter: Option<InstanceId>,
    pub icon: String,
}

// ============================================================================
// The tree
// ============================================================================

pub struct DataModel {
    slots: Vec<Slot>,
    free: Vec<u32>,
    root: InstanceId,
    by_entity: HashMap<u64, InstanceId>,
    services: HashMap<String, InstanceId>,
    tag_index: HashMap<String, BTreeSet<InstanceId>>,
    dirty_list: Vec<InstanceId>,
    spawn_queue: Vec<InstanceId>,
    reparent_queue: Vec<InstanceId>,
    despawn_queue: Vec<u64>,
    garbage: Vec<InstanceId>,
    events: Vec<DmEvent>,
    /// Sequence number of `events[0]`.
    event_base: u64,

    pub frame: FrameState,
    pub input: InputState,
    pub mouse: MouseState,
    pub local_player: Option<InstanceId>,
    pub output: Vec<OutputLine>,
    pub sound_commands: Vec<SoundCommand>,
    pub particle_emits: Vec<(InstanceId, u32)>,
    pub humanoid_commands: Vec<HumanoidCommand>,
    pub physics_commands: Vec<PhysicsCommand>,
    /// Parts a script listens to `Touched` / `TouchEnded` on. Physics only
    /// reports contacts for colliders that ask for them, so the engine turns
    /// reporting on for each part queued here.
    pub touch_watch: Vec<InstanceId>,
    /// Bumped on every structural change, so caches keyed on the tree
    /// shape (the Rune snapshot, name lookups) know when to rebuild.
    pub structure_version: u64,
}

impl Default for DataModel {
    fn default() -> Self {
        Self::new()
    }
}

impl DataModel {
    /// A tree holding only the `game` root.
    pub fn new() -> Self {
        let mut dm = Self {
            slots: Vec::new(),
            free: Vec::new(),
            root: InstanceId(0),
            by_entity: HashMap::default(),
            services: HashMap::default(),
            tag_index: HashMap::default(),
            dirty_list: Vec::new(),
            spawn_queue: Vec::new(),
            reparent_queue: Vec::new(),
            despawn_queue: Vec::new(),
            garbage: Vec::new(),
            events: Vec::new(),
            event_base: 0,
            frame: FrameState::default(),
            input: InputState::default(),
            mouse: MouseState::default(),
            local_player: None,
            output: Vec::new(),
            sound_commands: Vec::new(),
            particle_emits: Vec::new(),
            humanoid_commands: Vec::new(),
            physics_commands: Vec::new(),
            touch_watch: Vec::new(),
            structure_version: 0,
        };
        let root = dm.alloc("DataModel", "Game", Origin::Scene);
        dm.root = root;
        dm
    }

    fn alloc(&mut self, class: &str, name: &str, origin: Origin) -> InstanceId {
        let (index, generation) = if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1).max(1);
            (index, slot.generation)
        } else {
            self.slots.push(Slot { generation: 1, inst: None });
            ((self.slots.len() - 1) as u32, 1)
        };
        let id = InstanceId::new(index, generation);
        self.slots[index as usize].inst = Some(LiveInstance::new(id, class, name, origin));
        id
    }

    // ── Identity ───────────────────────────────────────────────────────────

    /// The `game` instance.
    pub fn root(&self) -> InstanceId {
        self.root
    }

    pub fn get(&self, id: InstanceId) -> Option<&LiveInstance> {
        let slot = self.slots.get(id.index() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.inst.as_ref()
    }

    pub fn get_mut(&mut self, id: InstanceId) -> Option<&mut LiveInstance> {
        let slot = self.slots.get_mut(id.index() as usize)?;
        if slot.generation != id.generation() {
            return None;
        }
        slot.inst.as_mut()
    }

    /// Alive and not destroyed.
    pub fn exists(&self, id: InstanceId) -> bool {
        self.get(id).map_or(false, |i| !i.destroyed)
    }

    pub fn class_of(&self, id: InstanceId) -> Option<&str> {
        self.get(id).map(|i| i.class_name.as_str())
    }

    pub fn name_of(&self, id: InstanceId) -> Option<&str> {
        self.get(id).map(|i| i.name.as_str())
    }

    pub fn is_a(&self, id: InstanceId, base: &str) -> bool {
        self.get(id).map_or(false, |i| i.is_a(base))
    }

    /// Number of live instances (for diagnostics).
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.inst.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() <= 1
    }

    // ── Creation ───────────────────────────────────────────────────────────

    /// `Instance.new(class)`: a detached instance with the class defaults.
    /// It reaches the world when a script parents it into the tree.
    pub fn create(&mut self, class: &str) -> InstanceId {
        let id = self.alloc(class, class, Origin::Script);
        let defaults = default_properties(class);
        if let Some(inst) = self.get_mut(id) {
            for (k, v) in defaults {
                inst.props.insert(k.to_string(), v);
            }
        }
        id
    }

    /// Engine seeding: an instance already present in the ECS. `props`
    /// replace the class defaults; the instance is linked under `parent`
    /// without queueing a spawn (the entity exists).
    pub fn create_bound(
        &mut self,
        class: &str,
        name: &str,
        entity_bits: u64,
        parent: Option<InstanceId>,
        props: Vec<(String, DmValue)>,
    ) -> InstanceId {
        let id = self.alloc(class, name, Origin::Scene);
        let defaults = default_properties(class);
        if let Some(inst) = self.get_mut(id) {
            for (k, v) in defaults {
                inst.props.insert(k.to_string(), v);
            }
            for (k, v) in props {
                inst.props.insert(k, v);
            }
            inst.entity = Some(entity_bits);
        }
        self.by_entity.insert(entity_bits, id);
        if let Some(p) = parent {
            self.link(id, p);
        }
        if class_is_service_like(class) {
            self.services.entry(class.to_string()).or_insert(id);
        }
        self.structure_version += 1;
        id
    }

    /// An instance that exists only in the tree (services with no entity,
    /// Play-time players). Linked immediately, never spawned.
    pub fn create_virtual(&mut self, class: &str, name: &str, parent: Option<InstanceId>) -> InstanceId {
        let id = self.alloc(class, name, Origin::Script);
        let defaults = default_properties(class);
        if let Some(inst) = self.get_mut(id) {
            for (k, v) in defaults {
                inst.props.insert(k.to_string(), v);
            }
        }
        if let Some(p) = parent {
            self.link(id, p);
        }
        if class_is_service_like(class) {
            self.services.entry(class.to_string()).or_insert(id);
        }
        self.structure_version += 1;
        id
    }

    /// `game:GetService(name)`. Creates a virtual service the first time a
    /// script asks for one the Space does not have, like Roblox does.
    pub fn get_service(&mut self, class: &str) -> Option<InstanceId> {
        if let Some(id) = self.services.get(class).copied() {
            if self.exists(id) {
                return Some(id);
            }
        }
        if !is_service_class(class) {
            return None;
        }
        let root = self.root;
        let id = self.create_virtual(class, class, Some(root));
        self.services.insert(class.to_string(), id);
        Some(id)
    }

    /// `game:FindService(name)`: never creates.
    pub fn find_service(&self, class: &str) -> Option<InstanceId> {
        self.services.get(class).copied().filter(|id| self.exists(*id))
    }

    /// Point a service name at an existing instance (the engine does this
    /// for service entities while seeding).
    pub fn register_service(&mut self, class: &str, id: InstanceId) {
        self.services.insert(class.to_string(), id);
    }

    pub fn bind_entity(&mut self, id: InstanceId, entity_bits: u64) {
        if let Some(inst) = self.get_mut(id) {
            if let Some(old) = inst.entity.replace(entity_bits) {
                self.by_entity.remove(&old);
            }
        }
        self.by_entity.insert(entity_bits, id);
    }

    pub fn unbind_entity(&mut self, id: InstanceId) -> Option<u64> {
        let bits = self.get_mut(id)?.entity.take()?;
        self.by_entity.remove(&bits);
        Some(bits)
    }

    pub fn by_entity(&self, entity_bits: u64) -> Option<InstanceId> {
        self.by_entity.get(&entity_bits).copied().filter(|id| self.exists(*id))
    }

    pub fn entity_of(&self, id: InstanceId) -> Option<u64> {
        self.get(id).and_then(|i| i.entity)
    }

    // ── Hierarchy reads ────────────────────────────────────────────────────

    pub fn parent(&self, id: InstanceId) -> Option<InstanceId> {
        self.get(id).and_then(|i| i.parent)
    }

    pub fn children(&self, id: InstanceId) -> &[InstanceId] {
        self.get(id).map(|i| i.children.as_slice()).unwrap_or(&[])
    }

    /// Every descendant, depth-first, parents before children.
    pub fn descendants(&self, id: InstanceId) -> Vec<InstanceId> {
        let mut out = Vec::new();
        let mut stack: Vec<InstanceId> = self.children(id).iter().rev().copied().collect();
        while let Some(next) = stack.pop() {
            out.push(next);
            for c in self.children(next).iter().rev() {
                stack.push(*c);
            }
        }
        out
    }

    pub fn is_descendant_of(&self, id: InstanceId, ancestor: InstanceId) -> bool {
        let mut cur = self.parent(id);
        let mut guard = 0;
        while let Some(p) = cur {
            if p == ancestor {
                return true;
            }
            cur = self.parent(p);
            guard += 1;
            if guard > 4096 {
                break;
            }
        }
        false
    }

    /// In the running tree (a descendant of `game`).
    pub fn in_tree(&self, id: InstanceId) -> bool {
        id == self.root || self.is_descendant_of(id, self.root)
    }

    /// The service this instance lives under, if any.
    pub fn service_of(&self, id: InstanceId) -> Option<InstanceId> {
        let mut cur = id;
        let mut guard = 0;
        loop {
            let parent = self.parent(cur)?;
            if parent == self.root {
                return Some(cur);
            }
            cur = parent;
            guard += 1;
            if guard > 4096 {
                return None;
            }
        }
    }

    /// Under Workspace: rendered and simulated.
    pub fn in_workspace(&self, id: InstanceId) -> bool {
        self.service_of(id)
            .and_then(|s| self.class_of(s).map(|c| c == "Workspace"))
            .unwrap_or(false)
    }

    pub fn find_first_child(&self, id: InstanceId, name: &str, recursive: bool) -> Option<InstanceId> {
        if recursive {
            return self.descendants(id).into_iter().find(|d| self.name_of(*d) == Some(name));
        }
        self.children(id).iter().copied().find(|c| self.name_of(*c) == Some(name))
    }

    pub fn find_first_child_of_class(&self, id: InstanceId, class: &str, recursive: bool) -> Option<InstanceId> {
        let pred = |c: &InstanceId| self.class_of(*c) == Some(class);
        if recursive {
            return self.descendants(id).into_iter().find(|c| pred(c));
        }
        self.children(id).iter().copied().find(|c| pred(c))
    }

    pub fn find_first_child_which_is_a(&self, id: InstanceId, base: &str, recursive: bool) -> Option<InstanceId> {
        let pred = |c: &InstanceId| self.is_a(*c, base);
        if recursive {
            return self.descendants(id).into_iter().find(|c| pred(c));
        }
        self.children(id).iter().copied().find(|c| pred(c))
    }

    pub fn find_first_ancestor(&self, id: InstanceId, name: &str) -> Option<InstanceId> {
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            if self.name_of(p) == Some(name) {
                return Some(p);
            }
            cur = self.parent(p);
        }
        None
    }

    pub fn find_first_ancestor_which_is_a(&self, id: InstanceId, base: &str) -> Option<InstanceId> {
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            if self.is_a(p, base) {
                return Some(p);
            }
            cur = self.parent(p);
        }
        None
    }

    /// `Instance:GetFullName()`: `Workspace.Map.Wall`.
    pub fn full_name(&self, id: InstanceId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(c) = cur {
            if c == self.root {
                break;
            }
            if let Some(n) = self.name_of(c) {
                parts.push(n.to_string());
            }
            cur = self.parent(c);
        }
        parts.reverse();
        parts.join(".")
    }

    // ── Hierarchy writes ───────────────────────────────────────────────────

    fn link(&mut self, child: InstanceId, parent: InstanceId) {
        if let Some(p) = self.get_mut(parent) {
            p.children.push(child);
        }
        if let Some(c) = self.get_mut(child) {
            c.parent = Some(parent);
        }
    }

    fn unlink(&mut self, child: InstanceId) -> Option<InstanceId> {
        let old = self.get_mut(child)?.parent.take()?;
        if let Some(p) = self.get_mut(old) {
            p.children.retain(|c| *c != child);
        }
        Some(old)
    }

    /// `instance.Parent = new_parent` (`None` detaches).
    pub fn set_parent(&mut self, child: InstanceId, new_parent: Option<InstanceId>) -> Result<(), String> {
        let (locked, destroyed, old_parent, name) = match self.get(child) {
            Some(c) => (c.parent_locked, c.destroyed, c.parent, c.name.clone()),
            None => return Err("cannot set the Parent of a destroyed instance".into()),
        };
        if child == self.root {
            return Err("the Parent of game cannot be changed".into());
        }
        if locked || destroyed {
            return Err(format!("The Parent property of {} is locked", name));
        }
        if old_parent == new_parent {
            return Ok(());
        }
        if let Some(np) = new_parent {
            if !self.exists(np) {
                return Err(format!("cannot parent {} to a destroyed instance", name));
            }
            if np == child || self.is_descendant_of(np, child) {
                return Err(format!(
                    "Attempt to set parent of {} to {} would result in circular reference",
                    name,
                    self.name_of(np).unwrap_or("?")
                ));
            }
        }

        let was_in_tree = self.in_tree(child);
        let subtree: Vec<InstanceId> = std::iter::once(child).chain(self.descendants(child)).collect();

        if let Some(old) = self.unlink(child) {
            self.events.push(DmEvent::ChildRemoved { parent: old, child });
            let mut anc = Some(old);
            while let Some(a) = anc {
                self.events.push(DmEvent::DescendantRemoving { ancestor: a, descendant: child });
                anc = self.parent(a);
            }
        }
        if let Some(np) = new_parent {
            self.link(child, np);
            self.events.push(DmEvent::ChildAdded { parent: np, child });
            let mut anc = Some(np);
            while let Some(a) = anc {
                for d in &subtree {
                    self.events.push(DmEvent::DescendantAdded { ancestor: a, descendant: *d });
                }
                anc = self.parent(a);
            }
        }
        for d in &subtree {
            let p = self.parent(*d);
            self.events.push(DmEvent::AncestryChanged { id: *d, parent: p });
        }

        let now_in_tree = self.in_tree(child);
        for d in &subtree {
            let (bound, queued) = match self.get(*d) {
                Some(i) => (i.entity.is_some(), i.queued_spawn),
                None => continue,
            };
            if bound {
                // The entity follows its instance: the engine re-links it and
                // re-derives visibility and physics from the new ancestry.
                if !self.reparent_queue.contains(d) {
                    self.reparent_queue.push(*d);
                }
            } else if now_in_tree && !queued {
                if let Some(i) = self.get_mut(*d) {
                    i.queued_spawn = true;
                }
                self.spawn_queue.push(*d);
            }
        }
        let _ = was_in_tree;
        self.structure_version += 1;
        Ok(())
    }

    pub fn rename(&mut self, id: InstanceId, name: &str) -> Result<(), String> {
        let watch = {
            let inst = self.get_mut(id).ok_or("instance is destroyed")?;
            inst.name = name.to_string();
            inst.watch_changes
        };
        self.mark_dirty(id, "Name");
        if watch {
            self.events.push(DmEvent::Changed { id, prop: "Name".into() });
        }
        self.structure_version += 1;
        Ok(())
    }

    /// `Instance:Destroy()`: the instance and all its descendants leave the
    /// tree for good. Their slots are released at the end of the frame, so a
    /// `Destroying` handler can still read them.
    pub fn destroy(&mut self, id: InstanceId) {
        if id == self.root || !self.exists(id) {
            return;
        }
        let subtree: Vec<InstanceId> = std::iter::once(id).chain(self.descendants(id)).collect();
        for d in subtree.iter().rev() {
            self.events.push(DmEvent::Destroying { id: *d });
        }
        if let Some(old) = self.unlink(id) {
            self.events.push(DmEvent::ChildRemoved { parent: old, child: id });
        }
        for d in &subtree {
            let (entity, tags) = match self.get_mut(*d) {
                Some(i) => {
                    i.destroyed = true;
                    i.parent_locked = true;
                    (i.entity.take(), std::mem::take(&mut i.tags))
                }
                None => continue,
            };
            for t in tags {
                if let Some(set) = self.tag_index.get_mut(&t) {
                    set.remove(d);
                }
            }
            if let Some(bits) = entity {
                self.by_entity.remove(&bits);
                self.despawn_queue.push(bits);
            }
            self.garbage.push(*d);
        }
        // A destroyed subtree keeps its internal parent links so descendants
        // still resolve `.Parent` inside their own Destroying handlers.
        self.structure_version += 1;
    }

    /// `Instance:ClearAllChildren()`.
    pub fn clear_all_children(&mut self, id: InstanceId) {
        let kids: Vec<InstanceId> = self.children(id).to_vec();
        for k in kids {
            self.destroy(k);
        }
    }

    /// `Instance:Clone()`: a detached deep copy of every archivable
    /// instance in the subtree. `None` when the instance itself is not
    /// archivable (Roblox returns nil).
    pub fn clone_instance(&mut self, id: InstanceId) -> Option<InstanceId> {
        if !self.exists(id) || !self.get(id)?.archivable {
            return None;
        }
        let root_copy = self.clone_one(id)?;
        let mut stack: Vec<(InstanceId, InstanceId)> = vec![(id, root_copy)];
        while let Some((src, dst)) = stack.pop() {
            let kids: Vec<InstanceId> = self.children(src).to_vec();
            for k in kids {
                if !self.get(k).map_or(false, |i| i.archivable) {
                    continue;
                }
                if let Some(copy) = self.clone_one(k) {
                    self.link(copy, dst);
                    stack.push((k, copy));
                }
            }
        }
        // Remap instance references inside the copy (PrimaryPart pointing at
        // a part of the same model must point at the copied part).
        let mut map: HashMap<InstanceId, InstanceId> = HashMap::default();
        let src_all: Vec<InstanceId> = std::iter::once(id).chain(self.descendants(id)).collect();
        let dst_all: Vec<InstanceId> = std::iter::once(root_copy).chain(self.descendants(root_copy)).collect();
        for d in &dst_all {
            if let Some(src) = self.get(*d).and_then(|i| i.clone_of) {
                map.insert(src, *d);
            }
        }
        let _ = src_all;
        for d in &dst_all {
            if let Some(inst) = self.get_mut(*d) {
                for v in inst.props.values_mut() {
                    if let DmValue::Instance(r) = v {
                        if let Some(n) = map.get(r) {
                            *r = *n;
                        }
                    }
                }
            }
        }
        self.structure_version += 1;
        Some(root_copy)
    }

    fn clone_one(&mut self, src: InstanceId) -> Option<InstanceId> {
        let s = self.get(src)?.clone();
        let copy = self.alloc(&s.class_name, &s.name, Origin::Script);
        let tags = s.tags.clone();
        if let Some(inst) = self.get_mut(copy) {
            inst.props = s.props;
            inst.attributes = s.attributes;
            inst.tags = s.tags;
            inst.archivable = s.archivable;
            inst.clone_of = Some(src);
        }
        for t in tags {
            self.tag_index.entry(t).or_default().insert(copy);
        }
        Some(copy)
    }

    // ── Properties ─────────────────────────────────────────────────────────

    /// A property as scripts read it, including the derived ones
    /// (`Position` and `Orientation` from `CFrame`, `Velocity`, ...).
    pub fn get_prop(&self, id: InstanceId, name: &str) -> Option<DmValue> {
        let inst = self.get(id)?;
        match name {
            "Name" => return Some(DmValue::String(inst.name.clone())),
            "ClassName" => return Some(DmValue::String(inst.class_name.clone())),
            "Parent" => {
                return Some(match inst.parent {
                    Some(p) if !inst.destroyed => DmValue::Instance(p),
                    _ => DmValue::Nil,
                })
            }
            "Archivable" => return Some(DmValue::Bool(inst.archivable)),
            _ => {}
        }
        if has_pose(&inst.class_name) {
            match name {
                "Position" => return inst.cframe().map(|cf| DmValue::Vector3(cf.position)),
                "Orientation" => {
                    return inst.cframe().map(|cf| {
                        let (rx, ry, rz) = cf.to_euler_angles_yxz();
                        DmValue::Vector3(Vector3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees()))
                    })
                }
                "Rotation" if is_base_part(&inst.class_name) => {
                    return inst.cframe().map(|cf| {
                        let (rx, ry, rz) = cf.to_euler_angles_xyz();
                        DmValue::Vector3(Vector3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees()))
                    })
                }
                "WorldPosition" => return inst.cframe().map(|cf| DmValue::Vector3(cf.position)),
                _ => {}
            }
        }
        if is_base_part(&inst.class_name) {
            match name {
                "Velocity" => return inst.props.get("AssemblyLinearVelocity").cloned(),
                "RotVelocity" => return inst.props.get("AssemblyAngularVelocity").cloned(),
                _ => {}
            }
        }
        inst.props.get(name).cloned()
    }

    /// A script write. Normalises the value (enum strings, `Position` into
    /// `CFrame`, ...), stores it, and marks it for the engine.
    pub fn set_prop(&mut self, id: InstanceId, name: &str, value: DmValue) -> Result<(), String> {
        let class = match self.get(id) {
            Some(i) if !i.destroyed => i.class_name.clone(),
            Some(_) => return Ok(()), // writes to a destroyed instance are ignored, like Roblox
            None => return Err("attempt to write a property of a destroyed instance".into()),
        };
        match name {
            "Name" => {
                let n = match &value {
                    DmValue::String(s) => s.clone(),
                    other => other.display(),
                };
                return self.rename(id, &n);
            }
            "Parent" => {
                return match value {
                    DmValue::Nil => self.set_parent(id, None),
                    DmValue::Instance(p) => self.set_parent(id, Some(p)),
                    other => Err(format!("Parent must be an Instance or nil, got {}", other.type_name())),
                };
            }
            "ClassName" => return Err("ClassName is read-only".into()),
            "Archivable" => {
                let b = value.as_bool().ok_or("Archivable expects a boolean")?;
                if let Some(i) = self.get_mut(id) {
                    i.archivable = b;
                }
                return Ok(());
            }
            _ => {}
        }

        // Derived pose properties write through to CFrame.
        if has_pose(&class) {
            match name {
                "Position" | "WorldPosition" => {
                    let pos = value.as_vector3().ok_or_else(|| type_error(name, "Vector3", &value))?;
                    let mut cf = self.get(id).and_then(|i| i.cframe()).unwrap_or_default();
                    cf.position = pos;
                    return self.store(id, "CFrame", DmValue::CFrame(cf));
                }
                "Orientation" => {
                    let deg = value.as_vector3().ok_or_else(|| type_error(name, "Vector3", &value))?;
                    let pos = self.get(id).and_then(|i| i.cframe()).map(|c| c.position).unwrap_or_default();
                    let mut cf = CFrame::from_euler_angles_yxz(
                        deg.x.to_radians(),
                        deg.y.to_radians(),
                        deg.z.to_radians(),
                    );
                    cf.position = pos;
                    return self.store(id, "CFrame", DmValue::CFrame(cf));
                }
                "Rotation" if is_base_part(&class) => {
                    let deg = value.as_vector3().ok_or_else(|| type_error(name, "Vector3", &value))?;
                    let pos = self.get(id).and_then(|i| i.cframe()).map(|c| c.position).unwrap_or_default();
                    let mut cf = CFrame::from_euler_angles_xyz(
                        deg.x.to_radians(),
                        deg.y.to_radians(),
                        deg.z.to_radians(),
                    );
                    cf.position = pos;
                    return self.store(id, "CFrame", DmValue::CFrame(cf));
                }
                _ => {}
            }
        }
        if is_base_part(&class) {
            match name {
                "Velocity" => return self.store(id, "AssemblyLinearVelocity", value),
                "RotVelocity" => return self.store(id, "AssemblyAngularVelocity", value),
                _ => {}
            }
        }

        // Enum-typed properties accept the enum item or its name.
        let normalized = match (enum_type_of(name), &value) {
            (Some(ty), DmValue::String(s)) => Some(DmValue::Enum(EnumItem::parse(s, ty))),
            (Some(ty), DmValue::Enum(e)) if e.enum_type.is_empty() => {
                Some(DmValue::Enum(EnumItem::new(ty, e.name.clone())))
            }
            _ => None,
        };
        let value = normalized.unwrap_or(value);

        // Type check against the current value, so `part.Size = 5` fails the
        // way it does in Roblox instead of corrupting the part.
        if let Some(current) = self.get(id).and_then(|i| i.props.get(name)) {
            if !compatible(current, &value) {
                return Err(type_error(name, current.type_name(), &value));
            }
        }
        let value = match value {
            // Clamp the usual unit-interval properties.
            DmValue::Number(n) if matches!(name, "Transparency" | "Reflectance" | "BackgroundTransparency"
                | "TextTransparency" | "ImageTransparency") => DmValue::Number(n.clamp(0.0, 1.0)),
            other => other,
        };
        self.store(id, name, value)
    }

    fn store(&mut self, id: InstanceId, name: &str, value: DmValue) -> Result<(), String> {
        let (watch, changed, died) = {
            let inst = self.get_mut(id).ok_or("instance is destroyed")?;
            let changed = inst.props.get(name) != Some(&value);
            if !changed {
                // Writing the value a property already has is a no-op, as in
                // Roblox: no Changed event and nothing for the engine to apply
                // (a script refreshing a colour every frame costs nothing).
                return Ok(());
            }
            // A Humanoid dies once, on the write that takes Health to zero.
            let died = inst.class_name == "Humanoid"
                && name == "Health"
                && inst.props.get("Health").and_then(DmValue::as_number).map_or(false, |h| h > 0.0)
                && value.as_number().map_or(false, |h| h <= 0.0);
            inst.props.insert(name.to_string(), value);
            (inst.watch_changes, changed, died)
        };
        if died {
            self.events.push(DmEvent::Died { humanoid: id });
        }
        self.mark_dirty(id, name);
        if watch && changed {
            self.events.push(DmEvent::Changed { id, prop: name.to_string() });
            if name == "CFrame" {
                self.events.push(DmEvent::Changed { id, prop: "Position".into() });
                self.events.push(DmEvent::Changed { id, prop: "Orientation".into() });
            }
        }
        Ok(())
    }

    /// Engine-side write: physics poses, avatar state. Never marks dirty,
    /// so it is never written back to the ECS.
    pub fn set_prop_from_engine(&mut self, id: InstanceId, name: &str, value: DmValue) {
        let watch = match self.get_mut(id) {
            Some(inst) => {
                if inst.dirty.iter().any(|d| d == name) {
                    // A script wrote this property this frame; its write wins.
                    return;
                }
                let changed = inst.props.get(name) != Some(&value);
                inst.props.insert(name.to_string(), value);
                inst.watch_changes && changed
            }
            None => return,
        };
        if watch {
            self.events.push(DmEvent::Changed { id, prop: name.to_string() });
        }
    }

    fn mark_dirty(&mut self, id: InstanceId, name: &str) {
        let push = match self.get_mut(id) {
            Some(inst) => {
                if !inst.dirty.iter().any(|d| d == name) {
                    inst.dirty.push(name.to_string());
                }
                if inst.in_dirty_list {
                    false
                } else {
                    inst.in_dirty_list = true;
                    true
                }
            }
            None => false,
        };
        if push {
            self.dirty_list.push(id);
        }
    }

    /// Start queueing `Changed` events for this instance.
    pub fn watch_changes(&mut self, id: InstanceId) {
        if let Some(i) = self.get_mut(id) {
            i.watch_changes = true;
        }
    }

    // ── Attributes and tags ────────────────────────────────────────────────

    pub fn get_attribute(&self, id: InstanceId, name: &str) -> Option<DmValue> {
        self.get(id)?.attributes.get(name).cloned()
    }

    pub fn set_attribute(&mut self, id: InstanceId, name: &str, value: DmValue) -> Result<(), String> {
        if matches!(value, DmValue::Instance(_)) {
            return Err("attributes cannot hold Instances".into());
        }
        let changed = {
            let inst = self.get_mut(id).ok_or("instance is destroyed")?;
            if value.is_nil() {
                inst.attributes.remove(name).is_some()
            } else {
                inst.attributes.insert(name.to_string(), value.clone()) != Some(value)
            }
        };
        if changed {
            self.events.push(DmEvent::AttributeChanged { id, name: name.to_string() });
            self.mark_dirty(id, "__attributes");
        }
        Ok(())
    }

    pub fn attributes(&self, id: InstanceId) -> Vec<(String, DmValue)> {
        self.get(id)
            .map(|i| i.attributes.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Engine seeding of authored attributes (not dirty, no event).
    pub fn seed_attribute(&mut self, id: InstanceId, name: &str, value: DmValue) {
        if let Some(i) = self.get_mut(id) {
            i.attributes.insert(name.to_string(), value);
        }
    }

    pub fn add_tag(&mut self, id: InstanceId, tag: &str) {
        let added = self.get_mut(id).map_or(false, |i| i.tags.insert(tag.to_string()));
        if added {
            self.tag_index.entry(tag.to_string()).or_default().insert(id);
            self.events.push(DmEvent::TagAdded { id, tag: tag.to_string() });
            self.mark_dirty(id, "__tags");
        }
    }

    /// Engine seeding of authored tags (not dirty, no event).
    pub fn seed_tag(&mut self, id: InstanceId, tag: &str) {
        if self.get_mut(id).map_or(false, |i| i.tags.insert(tag.to_string())) {
            self.tag_index.entry(tag.to_string()).or_default().insert(id);
        }
    }

    pub fn remove_tag(&mut self, id: InstanceId, tag: &str) {
        let removed = self.get_mut(id).map_or(false, |i| i.tags.remove(tag));
        if removed {
            if let Some(set) = self.tag_index.get_mut(tag) {
                set.remove(&id);
            }
            self.events.push(DmEvent::TagRemoved { id, tag: tag.to_string() });
            self.mark_dirty(id, "__tags");
        }
    }

    pub fn has_tag(&self, id: InstanceId, tag: &str) -> bool {
        self.get(id).map_or(false, |i| i.tags.contains(tag))
    }

    /// `CollectionService:GetTagged(tag)`: instances in the tree only.
    pub fn tagged(&self, tag: &str) -> Vec<InstanceId> {
        self.tag_index
            .get(tag)
            .map(|set| set.iter().copied().filter(|id| self.exists(*id) && self.in_tree(*id)).collect())
            .unwrap_or_default()
    }

    pub fn tags_of(&self, id: InstanceId) -> Vec<String> {
        self.get(id).map(|i| i.tags.iter().cloned().collect()).unwrap_or_default()
    }

    /// Every live instance of exactly `class`, in the tree or not. A full
    /// scan: cache the result against [`Self::structure_version`].
    pub fn ids_of_class(&self, class: &str) -> Vec<InstanceId> {
        self.slots
            .iter()
            .filter_map(|s| s.inst.as_ref())
            .filter(|i| !i.destroyed && i.class_name == class)
            .map(|i| i.id)
            .collect()
    }

    // ── Events ─────────────────────────────────────────────────────────────

    pub fn push_event(&mut self, event: DmEvent) {
        self.events.push(event);
    }

    /// Sequence number one past the newest event.
    pub fn event_cursor(&self) -> u64 {
        self.event_base + self.events.len() as u64
    }

    /// Events at or after `cursor`, and the cursor to use next time.
    pub fn events_since(&self, cursor: u64) -> (Vec<DmEvent>, u64) {
        let start = cursor.saturating_sub(self.event_base) as usize;
        let slice = if start < self.events.len() { self.events[start..].to_vec() } else { Vec::new() };
        (slice, self.event_cursor())
    }

    /// Drop events every reader has seen (`min_cursor` is the slowest reader).
    pub fn trim_events(&mut self, min_cursor: u64) {
        let n = min_cursor.saturating_sub(self.event_base) as usize;
        let n = n.min(self.events.len());
        if n > 0 {
            self.events.drain(..n);
            self.event_base += n as u64;
        }
    }

    // ── Output and commands ────────────────────────────────────────────────

    pub fn print(&mut self, level: OutputLevel, source: &str, text: impl Into<String>) {
        self.output.push(OutputLine { level, source: source.to_string(), text: text.into() });
    }

    // ── Engine drains ──────────────────────────────────────────────────────

    /// Instances that entered the tree without an entity, parents first.
    pub fn take_spawns(&mut self) -> Vec<InstanceId> {
        let q = std::mem::take(&mut self.spawn_queue);
        let mut out = Vec::with_capacity(q.len());
        for id in q {
            if let Some(i) = self.get_mut(id) {
                i.queued_spawn = false;
                if !i.destroyed && i.entity.is_none() {
                    out.push(id);
                }
            }
        }
        out.retain(|id| self.in_tree(*id));
        out
    }

    /// Bound instances whose parent changed.
    pub fn take_reparents(&mut self) -> Vec<InstanceId> {
        let q = std::mem::take(&mut self.reparent_queue);
        q.into_iter().filter(|id| self.exists(*id)).collect()
    }

    /// Entities to despawn (destroyed instances).
    pub fn take_despawns(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.despawn_queue)
    }

    /// Instances with properties written since the last call, with the
    /// property names. Values are read with [`DataModel::get_prop`].
    pub fn take_dirty(&mut self) -> Vec<(InstanceId, Vec<String>)> {
        let list = std::mem::take(&mut self.dirty_list);
        let mut out = Vec::with_capacity(list.len());
        for id in list {
            if let Some(i) = self.get_mut(id) {
                i.in_dirty_list = false;
                let d = std::mem::take(&mut i.dirty);
                if !i.destroyed && !d.is_empty() {
                    out.push((id, d));
                }
            }
        }
        out
    }

    /// Forget pending writes on one instance: for values the engine set up
    /// itself (a new character's props) that must not be applied back.
    pub fn take_dirty_of(&mut self, id: InstanceId) -> Vec<String> {
        match self.get_mut(id) {
            Some(i) => std::mem::take(&mut i.dirty),
            None => Vec::new(),
        }
    }

    /// Release destroyed slots. Call once per frame after every reader has
    /// handled this frame's events.
    pub fn end_frame(&mut self) {
        for id in std::mem::take(&mut self.garbage) {
            let idx = id.index() as usize;
            if let Some(slot) = self.slots.get_mut(idx) {
                if slot.generation == id.generation() && slot.inst.as_ref().map_or(false, |i| i.destroyed) {
                    slot.inst = None;
                    self.free.push(id.index());
                }
            }
        }
        self.input.events.clear();
    }

    // ── Camera maths ───────────────────────────────────────────────────────

    /// The current camera instance (`workspace.CurrentCamera`).
    pub fn current_camera(&self) -> Option<InstanceId> {
        let ws = self.find_service("Workspace")?;
        match self.get(ws)?.props.get("CurrentCamera") {
            Some(DmValue::Instance(id)) if self.exists(*id) => Some(*id),
            _ => self.find_first_child_of_class(ws, "Camera", false),
        }
    }

    /// `Camera:ViewportPointToRay(x, y)`: origin and unit direction through a
    /// viewport pixel, for perspective and orthographic cameras alike.
    pub fn viewport_point_to_ray(&self, x: f64, y: f64) -> (Vector3, Vector3) {
        let view = self.camera_view();
        let ndc_x = 2.0 * x / view.width.max(1.0) - 1.0;
        let ndc_y = 1.0 - 2.0 * y / view.height.max(1.0);
        let aspect = view.width.max(1.0) / view.height.max(1.0);
        if view.orthographic {
            let half_h = view.ortho_size * 0.5;
            let half_w = half_h * aspect;
            let origin = view.cframe.point_to_world_space(Vector3::new(ndc_x * half_w, ndc_y * half_h, 0.0));
            (origin, view.cframe.look_vector())
        } else {
            let tan_half = (view.fov_deg.to_radians() * 0.5).tan();
            let local = Vector3::new(ndc_x * tan_half * aspect, ndc_y * tan_half, -1.0).unit();
            (view.cframe.position, view.cframe.vector_to_world_space(local))
        }
    }

    /// `Camera:WorldToViewportPoint(p)`: pixel x, y, the depth along the
    /// view axis, and whether the point is on screen.
    pub fn world_to_viewport_point(&self, p: Vector3) -> (Vector3, bool) {
        let view = self.camera_view();
        let local = view.cframe.point_to_object_space(p);
        let depth = -local.z;
        let aspect = view.width.max(1.0) / view.height.max(1.0);
        let (ndc_x, ndc_y) = if view.orthographic {
            let half_h = view.ortho_size * 0.5;
            (local.x / (half_h * aspect), local.y / half_h)
        } else {
            let tan_half = (view.fov_deg.to_radians() * 0.5).tan();
            if depth.abs() < 1e-9 {
                (0.0, 0.0)
            } else {
                (local.x / (depth * tan_half * aspect), local.y / (depth * tan_half))
            }
        };
        let sx = (ndc_x + 1.0) * 0.5 * view.width;
        let sy = (1.0 - ndc_y) * 0.5 * view.height;
        let on_screen = depth > 0.0 && ndc_x.abs() <= 1.0 && ndc_y.abs() <= 1.0;
        (Vector3::new(sx, sy, depth), on_screen)
    }

    /// Everything the ray maths needs about the current camera.
    pub fn camera_view(&self) -> CameraView {
        let mut view = CameraView {
            cframe: CFrame::new(0.0, 20.0, 20.0),
            fov_deg: 70.0,
            orthographic: false,
            ortho_size: 40.0,
            width: self.input.viewport_w,
            height: self.input.viewport_h,
        };
        if let Some(cam) = self.current_camera().and_then(|c| self.get(c)) {
            if let Some(cf) = cam.cframe() {
                view.cframe = cf;
            }
            if let Some(f) = cam.props.get("FieldOfView").and_then(DmValue::as_number) {
                view.fov_deg = f;
            }
            if let Some(s) = cam.props.get("OrthographicSize").and_then(DmValue::as_number) {
                view.ortho_size = s.max(0.01);
            }
            view.orthographic = cam.props.get("Projection").and_then(DmValue::as_enum_name) == Some("Orthographic");
        }
        view
    }
}

/// The camera numbers [`DataModel::viewport_point_to_ray`] works from.
#[derive(Debug, Clone, Copy)]
pub struct CameraView {
    pub cframe: CFrame,
    pub fov_deg: f64,
    pub orthographic: bool,
    pub ortho_size: f64,
    pub width: f64,
    pub height: f64,
}

/// Classes whose pose lives in a `CFrame` property.
fn has_pose(class: &str) -> bool {
    is_base_part(class) || matches!(class, "Camera" | "Attachment")
}

/// Services and the service-like folders the loader creates.
fn class_is_service_like(class: &str) -> bool {
    is_service_class(class)
}

/// Whether `value` may replace `current`.
fn compatible(current: &DmValue, value: &DmValue) -> bool {
    use DmValue::*;
    match (current, value) {
        (Nil, _) | (_, Nil) => true,
        (Instance(_), Instance(_)) => true,
        (Number(_), Number(_)) => true,
        (Number(_), Bool(_)) => false,
        (Bool(_), Bool(_)) => true,
        (String(_), String(_)) => true,
        (Enum(_), Enum(_)) | (Enum(_), String(_)) => true,
        (Vector2(_), Vector2(_)) => true,
        (Vector3(_), Vector3(_)) => true,
        (CFrame(_), CFrame(_)) => true,
        (Color3(_), Color3(_)) => true,
        (UDim(_), UDim(_)) => true,
        (UDim2(_), UDim2(_)) => true,
        (NumberRange(_), NumberRange(_)) | (NumberRange(_), Number(_)) => true,
        (NumberSequence(_), NumberSequence(_)) | (NumberSequence(_), Number(_)) => true,
        (ColorSequence(_), ColorSequence(_)) | (ColorSequence(_), Color3(_)) => true,
        _ => false,
    }
}

fn type_error(prop: &str, expected: &str, got: &DmValue) -> String {
    format!("Unable to assign property {}. {} expected, got {}", prop, expected, got.type_name())
}

/// Screen-space helpers shared by the bindings.
pub fn vector2(x: f64, y: f64) -> Vector2 {
    Vector2::new(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::Color3;

    fn part(dm: &mut DataModel, name: &str) -> InstanceId {
        let id = dm.create("Part");
        dm.rename(id, name).unwrap();
        id
    }

    #[test]
    fn create_parent_and_find() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let p = part(&mut dm, "Wall");
        assert!(!dm.in_tree(p));
        dm.set_parent(p, Some(ws)).unwrap();
        assert!(dm.in_tree(p));
        assert!(dm.in_workspace(p));
        assert_eq!(dm.find_first_child(ws, "Wall", false), Some(p));
        assert_eq!(dm.take_spawns(), vec![p]);
        assert_eq!(dm.full_name(p), "Workspace.Wall");
    }

    #[test]
    fn position_writes_through_cframe_and_marks_dirty() {
        let mut dm = DataModel::new();
        let p = dm.create("Part");
        dm.set_prop(p, "Position", DmValue::Vector3(Vector3::new(1.0, 2.0, 3.0))).unwrap();
        assert_eq!(dm.get_prop(p, "Position"), Some(DmValue::Vector3(Vector3::new(1.0, 2.0, 3.0))));
        let dirty = dm.take_dirty();
        assert_eq!(dirty.len(), 1);
        assert!(dirty[0].1.iter().any(|n| n == "CFrame"));
        assert!(dm.take_dirty().is_empty());
    }

    #[test]
    fn type_errors_like_roblox() {
        let mut dm = DataModel::new();
        let p = dm.create("Part");
        assert!(dm.set_prop(p, "Size", DmValue::Number(5.0)).is_err());
        assert!(dm.set_prop(p, "Color", DmValue::Color3(Color3::new(1.0, 0.0, 0.0))).is_ok());
        dm.set_prop(p, "Material", DmValue::String("Neon".into())).unwrap();
        assert_eq!(
            dm.get_prop(p, "Material"),
            Some(DmValue::Enum(EnumItem::new("Material", "Neon")))
        );
    }

    #[test]
    fn circular_parent_rejected() {
        let mut dm = DataModel::new();
        let a = dm.create("Model");
        let b = dm.create("Model");
        dm.set_parent(b, Some(a)).unwrap();
        assert!(dm.set_parent(a, Some(b)).is_err());
    }

    #[test]
    fn destroy_frees_after_end_frame_and_locks_parent() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let m = dm.create("Model");
        let p = dm.create("Part");
        dm.set_parent(p, Some(m)).unwrap();
        dm.set_parent(m, Some(ws)).unwrap();
        dm.bind_entity(p, 42);
        dm.destroy(m);
        assert!(!dm.exists(m));
        assert!(dm.get(p).is_some(), "still readable until end_frame");
        assert!(dm.set_parent(m, Some(ws)).is_err());
        assert_eq!(dm.take_despawns(), vec![42]);
        dm.end_frame();
        assert!(dm.get(p).is_none());
        let q = dm.create("Part");
        assert_ne!(q, p, "a reused slot gets a new generation");
    }

    #[test]
    fn clone_is_detached_deep_and_remaps_refs() {
        let mut dm = DataModel::new();
        let m = dm.create("Model");
        let root = dm.create("Part");
        dm.rename(root, "HumanoidRootPart").unwrap();
        dm.set_parent(root, Some(m)).unwrap();
        dm.set_prop(m, "PrimaryPart", DmValue::Instance(root)).unwrap();
        let c = dm.clone_instance(m).unwrap();
        assert_eq!(dm.parent(c), None);
        let croot = dm.find_first_child(c, "HumanoidRootPart", false).unwrap();
        assert_ne!(croot, root);
        assert_eq!(dm.get_prop(c, "PrimaryPart"), Some(DmValue::Instance(croot)));
        assert_eq!(dm.get(croot).unwrap().clone_of, Some(root));
    }

    #[test]
    fn engine_writes_do_not_override_script_writes() {
        let mut dm = DataModel::new();
        let p = dm.create("Part");
        dm.set_prop(p, "Position", DmValue::Vector3(Vector3::new(5.0, 0.0, 0.0))).unwrap();
        dm.set_prop_from_engine(p, "CFrame", DmValue::CFrame(CFrame::new(0.0, 0.0, 0.0)));
        assert_eq!(dm.get_prop(p, "Position"), Some(DmValue::Vector3(Vector3::new(5.0, 0.0, 0.0))));
    }

    #[test]
    fn tags_index() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let p = dm.create("Part");
        dm.add_tag(p, "Zombie");
        assert!(dm.tagged("Zombie").is_empty(), "not in the tree yet");
        dm.set_parent(p, Some(ws)).unwrap();
        assert_eq!(dm.tagged("Zombie"), vec![p]);
        dm.destroy(p);
        assert!(dm.tagged("Zombie").is_empty());
    }

    #[test]
    fn orthographic_ray_is_parallel() {
        let mut dm = DataModel::new();
        let ws = dm.get_service("Workspace").unwrap();
        let cam = dm.create("Camera");
        dm.set_parent(cam, Some(ws)).unwrap();
        dm.set_prop(ws, "CurrentCamera", DmValue::Instance(cam)).unwrap();
        // Straight down with north (-Z) at the top of the screen.
        let cf = CFrame::look_at(
            Vector3::new(0.0, 50.0, 0.0),
            Vector3::new(0.0, 0.0, 0.0),
            Some(Vector3::new(0.0, 0.0, -1.0)),
        );
        dm.set_prop(cam, "CFrame", DmValue::CFrame(cf)).unwrap();
        dm.set_prop(cam, "Projection", DmValue::String("Orthographic".into())).unwrap();
        dm.set_prop(cam, "OrthographicSize", DmValue::Number(20.0)).unwrap();
        dm.input.viewport_w = 200.0;
        dm.input.viewport_h = 100.0;
        let (o1, d1) = dm.viewport_point_to_ray(0.0, 0.0);
        let (o2, d2) = dm.viewport_point_to_ray(200.0, 100.0);
        assert!(d1.fuzzy_eq(&d2, 1e-9));
        assert!(d1.y < -0.99);
        // 20 m tall view on a 2:1 viewport is 40 m wide.
        assert!(((o2.x - o1.x).abs() - 40.0).abs() < 1e-6);
        let (p, on) = dm.world_to_viewport_point(Vector3::new(0.0, 0.0, 0.0));
        assert!(on);
        assert!((p.x - 100.0).abs() < 1e-6 && (p.y - 50.0).abs() < 1e-3);
    }
}
