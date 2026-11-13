//! # Rune on a Player's Play tree
//!
//! A Player keeps the world it plays as a [`PlayDataModel`] tree (see
//! [`crate::tree_scripts`], which runs the tree's LocalScripts). This runs
//! the tree's Rune scripts (`SoulScript`s) the way Studio's Play runs them
//! (`engine::soul::rune_play`), so a published Space whose logic is Rune
//! works in the Player.
//!
//! ## What is shared with Studio, and what is not
//!
//! - The compile and the lifecycle are [`super::rune_runtime`]'s, which
//!   Studio's driver calls too: [`compile_scripts`] at the session's start,
//!   then one [`call_script_frame`] (`on_init`, `on_ready` a frame later,
//!   `on_update(dt)`) per frame, and [`call_script_exit`] when the session
//!   ends.
//! - The DataModel bridge, `eustress::dm`, is [`super::rune_datamodel`],
//!   the module Studio installs, over the tree [`crate::datamodel::active`]
//!   names. This driver publishes the Player's tree there only while it runs
//!   the scripts, inside the one system that runs them, as Studio installs
//!   its bridges.
//! - The modules a Player's script can call are [`player_rune_modules`]:
//!   `eustress::dm`, `eustress::Vector3`, the `eustress` GUI and log
//!   functions, and `event_bus`. Studio's own ECS module (its `Instance`
//!   handles, sim values, Space files, HTTP, DataStore, spatial queries,
//!   physics) and `eustress::terrain` stay Studio-only: they reach the
//!   engine's world and the Space's files, which a Player does not hand to a
//!   downloaded script. Nothing here gives a script the file system or the
//!   network.
//!
//! ## Which scripts run ([`RuneScope`])
//!
//! Studio's Play compiles every Rune script the Space holds, wherever it
//! sits, once, as the session starts, and runs them all on the one machine.
//!
//! - Played solo ([`RuneScope::Everything`]) the Player is that one machine:
//!   every `SoulScript` in the tree is compiled once as the tree opens, as
//!   Studio does. One whose recorded file is not `.rune` or `.soul` is
//!   skipped, as Studio skips it.
//! - Joined to a host ([`RuneScope::ClientOnly`]) the host runs the
//!   session's Rune, and its results arrive by replication. Here only the
//!   Rune that belongs to this player runs: scripts in `ReplicatedFirst`, in
//!   `StarterPlayer` / `StarterPlayerScripts` (not the
//!   `StarterCharacterScripts` templates), and in this player's own subtree,
//!   which is where a LocalScript runs too ([`crate::tree_scripts`]). The
//!   host replicates this player's containers after the join, so that list is
//!   looked at again whenever the tree's shape changes.
//!
//! ## Where this sits in the frame
//!
//! In `PlayScriptSet::Scripts`, after the LocalScripts' VM, on the main
//! thread: Rune's per-script state lives in thread-locals, as in Studio. A
//! script's writes to the tree draw in `Apply` the same frame. A script's
//! output, its start and its errors go to the tree's Output, which a Player
//! writes to its log.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use super::rune_runtime::{
    call_script_exit, call_script_frame, compile_more_scripts, compile_scripts, RuneModuleRegistry,
    RuneRuntimeState, ScriptError, ScriptSource,
};
use crate::datamodel::{self, DataModel, InstanceId, OutputLevel, SharedDataModel};
use crate::play_session::{PlayDataModel, PlayScriptSet};

#[cfg(test)]
mod tests;

/// Which of a tree's Rune scripts this Player runs (see the module docs).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RuneScope {
    /// Played solo: every Rune script, as Studio's Play runs them.
    #[default]
    Everything,
    /// Joined to a host, which runs the session's Rune: only the scripts
    /// that belong to this player.
    ClientOnly,
}

/// A Rune script found in a tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeRuneScript {
    pub instance: InstanceId,
    /// Its `Name`, which its Output lines and errors carry, as in Studio.
    pub name: String,
    /// Its full name (`SoulService.StationMind`).
    pub chunk_name: String,
    pub source: String,
    /// Its source file, Space-relative, when the tree recorded one.
    pub file: Option<String>,
}

/// A `SoulScript` a tree holds that does not run, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedRuneScript {
    pub chunk_name: String,
    pub reason: String,
}

/// The Rune scripts in `g` this Player runs under `scope`, ordered by full
/// name so starts are deterministic, and the ones it skips with the reason.
///
/// Studio's rule for what compiles (`compile_scripts_on_play`): a script
/// with source, whose file (when it has one) is `.rune` or `.soul`.
pub fn tree_rune_scripts(g: &DataModel, scope: RuneScope) -> (Vec<TreeRuneScript>, Vec<SkippedRuneScript>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();

    // The local player may sit outside the tree walk, so walk its subtree too.
    let mut stack = vec![g.root()];
    if let Some(p) = g.local_player {
        stack.push(p);
    }
    let mut visited = HashSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        for c in g.children(id) {
            stack.push(*c);
        }
        if g.class_of(id) != Some("SoulScript") {
            continue;
        }
        let chunk_name = g.full_name(id);
        if scope == RuneScope::ClientOnly && !belongs_to_this_player(g, id) {
            continue;
        }
        let source = g.get_prop(id, "Source").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        if source.trim().is_empty() {
            skipped.push(SkippedRuneScript { chunk_name, reason: "its source is empty".into() });
            continue;
        }
        let file = g.script_file(id).map(str::to_string);
        if let Some(f) = &file {
            let ext = std::path::Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rune" | "soul") {
                skipped.push(SkippedRuneScript { chunk_name, reason: format!("its file is .{ext}, not .rune or .soul") });
                continue;
            }
        }
        found.push(TreeRuneScript {
            instance: id,
            name: g.name_of(id).unwrap_or("").to_string(),
            chunk_name,
            source,
            file,
        });
    }
    found.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    skipped.sort_by(|a, b| a.chunk_name.cmp(&b.chunk_name));
    (found, skipped)
}

/// Whether a Rune script is this player's own: in its subtree, in
/// `ReplicatedFirst`, or in `StarterPlayer` / `StarterPlayerScripts` outside
/// the `StarterCharacterScripts` templates.
fn belongs_to_this_player(g: &DataModel, id: InstanceId) -> bool {
    if g.local_player.is_some_and(|p| g.is_descendant_of(id, p)) {
        return true;
    }
    let service = g.service_of(id).and_then(|s| g.class_of(s)).unwrap_or_default();
    match service {
        "ReplicatedFirst" | "StarterPlayerScripts" => true,
        "StarterPlayer" => g.find_first_ancestor(id, "StarterCharacterScripts").is_none(),
        _ => false,
    }
}

/// The Rune modules a Player's scripts compile against (see the module
/// docs for what is left out, and why).
pub fn player_rune_modules() -> Vec<rune::Module> {
    let mut modules = Vec::new();
    let mut add = |what: &str, built: Result<rune::Module, rune::ContextError>| match built {
        Ok(m) => modules.push(m),
        Err(e) => error!("Failed to create the {what} Rune module: {e}"),
    };
    add("eustress::dm", super::rune_datamodel::create_datamodel_module());
    add("eustress::Vector3", super::rune_values::create_values_module());
    // The `eustress` GUI and `log_*` functions.
    add("eustress GUI", super::rune_gui_module::create_gui_module());
    add("event_bus", crate::events::event_bus_rune_module());
    modules
}

/// The session the driver runs scripts for.
#[derive(Resource, Default)]
pub struct TreeRune {
    /// The tree the running scripts belong to, and its session number.
    tree: Option<(SharedDataModel, u64)>,
    /// Runtime key to instance, and back.
    by_key: HashMap<u32, TreeRuneScript>,
    by_instance: HashMap<InstanceId, u32>,
    next_key: u32,
    /// The tree shape the script list was last read at (joined only).
    seen_shape: u64,
}

impl TreeRune {
    /// The scripts compiled for the running session, by full name.
    pub fn running(&self) -> Vec<String> {
        let mut names: Vec<String> = self.by_key.values().map(|s| s.chunk_name.clone()).collect();
        names.sort();
        names
    }

    fn key_for(&mut self, script: &TreeRuneScript) -> u32 {
        self.next_key += 1;
        let key = self.next_key;
        self.by_key.insert(key, script.clone());
        self.by_instance.insert(script.instance, key);
        key
    }

    /// The script a log line or an error names, by its `Name`.
    fn script_named(&self, name: &str) -> Option<&TreeRuneScript> {
        let mut keys: Vec<&u32> = self.by_key.keys().collect();
        keys.sort();
        keys.into_iter().map(|k| &self.by_key[k]).find(|s| s.name == name)
    }
}

/// The system set the Rune frame runs in, inside `PlayScriptSet::Scripts`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunTreeRune;

/// Runs a [`PlayDataModel`]'s Rune scripts (see the module docs). Needs a
/// [`RuneModuleRegistry`] holding the modules to compile against
/// ([`player_rune_modules`]); a [`RuneScope`] resource picks the scripts,
/// [`RuneScope::Everything`] when there is none.
pub struct TreeRunePlugin;

impl Plugin for TreeRunePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RuneRuntimeState>()
            .init_resource::<RuneModuleRegistry>()
            .init_resource::<TreeRune>()
            .configure_sets(Update, RunTreeRune.in_set(PlayScriptSet::Scripts));
        // After the LocalScripts, as Studio runs Rune after Luau: Rune reads
        // what Luau wrote this frame.
        #[cfg(feature = "mlua")]
        app.configure_sets(Update, RunTreeRune.after(crate::tree_scripts::RunTreeScripts));
        app.add_systems(Update, drive_tree_rune.in_set(RunTreeRune))
            // The Play frame's sets run only while a tree is open, so the
            // world closing is noticed outside them.
            .add_systems(Update, stop_tree_rune_when_closed.run_if(not(resource_exists::<PlayDataModel>)))
            .add_systems(Last, stop_tree_rune_on_exit);
    }
}

/// No world open: the last world's scripts stop with it.
fn stop_tree_rune_when_closed(
    mut state: ResMut<TreeRune>,
    mut runtime: ResMut<RuneRuntimeState>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    stop_session(&mut state, &mut runtime);
}

/// One Rune frame over the open tree: start a new tree's scripts (ending the
/// last tree's), pick up this player's scripts as they arrive when joined,
/// run the frame with the tree published to `eustress::dm`, and report.
#[allow(clippy::too_many_arguments)]
fn drive_tree_rune(
    tree: Option<Res<PlayDataModel>>,
    scope: Option<Res<RuneScope>>,
    mut state: ResMut<TreeRune>,
    mut runtime: ResMut<RuneRuntimeState>,
    registry: Res<RuneModuleRegistry>,
    time: Res<Time>,
    bus: Option<Res<crate::events::EventBusResource>>,
    // Rune keeps per-script state in thread-locals: one thread, every frame.
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    let scope = scope.as_deref().copied().unwrap_or_default();
    let Some(tree) = tree else {
        // No world open: the last world's scripts stop with it.
        stop_session(&mut state, &mut runtime);
        return;
    };

    // A new tree is a new session; the scripts of the last one stop first.
    let session = datamodel::session_of(&tree.dm);
    if state.tree.as_ref().map(|(_, s)| *s) != Some(session) {
        stop_session(&mut state, &mut runtime);
        start_session(&mut state, &mut runtime, &registry, &tree.dm, session, scope);
    } else if scope == RuneScope::ClientOnly {
        pick_up_arrivals(&mut state, &mut runtime, &registry, &tree.dm);
    }

    if !runtime.compiled.is_empty() {
        datamodel::set_active(Some(tree.dm.clone()));
        if let Some(bus) = bus.as_deref() {
            crate::events::set_event_bus_for_rune(bus.0.clone());
        }
        call_script_frame(&mut runtime, time.delta_secs() as f64);
        crate::events::clear_event_bus_for_rune();
        datamodel::set_active(None);
    }
    report(&state, &mut runtime, &tree.dm);
}

/// Compile the tree's scripts for a new session and say what was found.
fn start_session(
    state: &mut TreeRune,
    runtime: &mut RuneRuntimeState,
    registry: &RuneModuleRegistry,
    dm: &SharedDataModel,
    session: u64,
    scope: RuneScope,
) {
    let (scripts, skipped, shape) = {
        let g = dm.lock();
        let (scripts, skipped) = tree_rune_scripts(&g, scope);
        (scripts, skipped, g.structure_version)
    };
    state.tree = Some((dm.clone(), session));
    state.seen_shape = shape;
    {
        let mut g = dm.lock();
        for s in &skipped {
            g.print(OutputLevel::Warn, "Rune", format!("{} does not run: {}", s.chunk_name, s.reason));
        }
    }
    let sources: Vec<ScriptSource> = scripts.iter().map(|s| source_for(state.key_for(s), s)).collect();
    let started = bevy::platform::time::Instant::now();
    compile_scripts(runtime, registry, &sources);
    if !sources.is_empty() {
        info!(
            "Rune: {} of {} script(s) compiled in {:.1} ms ({:?})",
            runtime.compiled.len(),
            sources.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            scope
        );
    }
}

/// Joined: compile this player's scripts that arrived since the last look,
/// and stop the ones that left the tree.
fn pick_up_arrivals(state: &mut TreeRune, runtime: &mut RuneRuntimeState, registry: &RuneModuleRegistry, dm: &SharedDataModel) {
    let (scripts, shape) = {
        let g = dm.lock();
        if g.structure_version == state.seen_shape {
            return;
        }
        (tree_rune_scripts(&g, RuneScope::ClientOnly).0, g.structure_version)
    };
    state.seen_shape = shape;
    let present: HashSet<InstanceId> = scripts.iter().map(|s| s.instance).collect();
    let gone: Vec<(InstanceId, u32)> =
        state.by_instance.iter().filter(|(i, _)| !present.contains(i)).map(|(i, k)| (*i, *k)).collect();
    for (instance, key) in gone {
        state.by_instance.remove(&instance);
        if let Some(script) = state.by_key.remove(&key) {
            dm.lock().print_lifecycle(OutputLevel::Info, None, &script.chunk_name, "Stopped");
        }
        runtime.compiled.remove(&key);
        runtime.initialized.remove(&key);
        runtime.initialized.remove(&(key + 1_000_000));
    }
    let new: Vec<TreeRuneScript> = scripts.into_iter().filter(|s| !state.by_instance.contains_key(&s.instance)).collect();
    let sources: Vec<ScriptSource> = new.iter().map(|s| source_for(state.key_for(s), s)).collect();
    compile_more_scripts(runtime, registry, &sources);
}

fn source_for(key: u32, s: &TreeRuneScript) -> ScriptSource {
    ScriptSource { entity_index: key, name: s.name.clone(), source: s.source.clone(), file: s.file.clone() }
}

/// End the running session: `on_exit` with its tree published, then clear.
fn stop_session(state: &mut TreeRune, runtime: &mut RuneRuntimeState) {
    let Some((dm, _)) = state.tree.take() else { return };
    if !runtime.compiled.is_empty() {
        datamodel::set_active(Some(dm.clone()));
        call_script_exit(runtime);
        datamodel::set_active(None);
        info!("Rune: the session's scripts stopped");
    }
    report(state, runtime, &dm);
    runtime.compiled.clear();
    runtime.initialized.clear();
    runtime.last_errors.clear();
    state.by_key.clear();
    state.by_instance.clear();
}

/// The app is closing: the running scripts get their `on_exit`.
fn stop_tree_rune_on_exit(
    mut exits: MessageReader<AppExit>,
    mut state: ResMut<TreeRune>,
    mut runtime: ResMut<RuneRuntimeState>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    if exits.read().next().is_some() {
        stop_session(&mut state, &mut runtime);
    }
}

/// The frame's script log lines and errors into the tree's Output, where the
/// Player's log reads them: a start as Luau's start reads ("Started"), a
/// `log_*` line under its script's full name, and each error on the script.
fn report(state: &TreeRune, runtime: &mut RuneRuntimeState, dm: &SharedDataModel) {
    let logs = crate::gui::drain_script_logs();
    let errors: Vec<ScriptError> = std::mem::take(&mut runtime.last_errors);
    if logs.is_empty() && errors.is_empty() {
        return;
    }
    let mut g = dm.lock();
    for entry in logs {
        let script = entry.script.as_deref().and_then(|n| state.script_named(n));
        let instance = script.map(|s| s.instance).filter(|i| g.exists(*i));
        let source = script.map_or("Rune", |s| s.chunk_name.as_str());
        if entry.message == "started" {
            g.print_lifecycle(OutputLevel::Info, instance, source, "Started");
            continue;
        }
        let level = match entry.level {
            crate::gui::ScriptLogLevel::Info => OutputLevel::Info,
            crate::gui::ScriptLogLevel::Warn => OutputLevel::Warn,
            crate::gui::ScriptLogLevel::Error => OutputLevel::Error,
        };
        let text = entry.message.strip_prefix("[Rune] ").unwrap_or(&entry.message).to_string();
        g.print_from(level, instance, source, text, entry.line.unwrap_or(0), entry.stack);
    }
    for error in errors {
        let script = state.script_named(&error.script);
        let instance = script.map(|s| s.instance).filter(|i| g.exists(*i));
        let source = script.map_or_else(|| error.script.clone(), |s| s.chunk_name.clone());
        if error.stack.is_empty() {
            // A compile error: one formatted diagnostic per line, as Studio's
            // Output shows it.
            for line in error.message.lines().filter(|l| !l.trim().is_empty()) {
                g.print_from(OutputLevel::Error, instance, &source, line.to_string(), error.line.unwrap_or(0), Vec::new());
            }
        } else {
            g.print_from(OutputLevel::Error, instance, &source, error.message.clone(), error.line.unwrap_or(0), error.stack);
        }
    }
}
