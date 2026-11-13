//! # Soul Client Runtime
//!
//! Full script execution on the client using shared Rune + Luau runtimes
//! from eustress_common. Mirrors engine capabilities for play mode parity.
//!
//! ## Architecture
//! - Rune scripts: the open world's `SoulScript`s, compiled and run as
//!   Studio's Play runs them by `common::soul::rune_play::TreeRunePlugin`
//!   (all of them played solo; only this player's own when joined to a host,
//!   which runs the rest), against `player_rune_modules`
//! - Luau scripts: a Space's scripts run in the Play runtime's own VM
//!   (`common::tree_scripts`), made when a session begins; this plugin makes
//!   none, so a bare launch builds no Luau VM
//! - GUI bridge: GuiBridgePlugin applies script commands to GuiElementDisplay
//! - Physics bridge: Avian3D force/velocity commands from scripts

pub mod runtime;
pub mod hot_reload;

pub use runtime::*;
pub use hot_reload::*;

use bevy::prelude::*;
use avian3d::prelude::Gravity;
use eustress_common::soul::{SoulConfig, ScriptRegistry, SoulPlugin as CommonSoulPlugin};
use eustress_common::soul::rune_runtime::{RuneRuntimeState, RuneModuleRegistry};
use eustress_common::soul::rune_play::{player_rune_modules, RuneScope, TreeRunePlugin};

// ============================================================================
// Client Soul Plugin — full script execution
// ============================================================================

/// Soul scripting plugin for Eustress Client.
/// Provides Rune script execution with GUI and physics bridges.
pub struct ClientSoulPlugin;

impl Plugin for ClientSoulPlugin {
    fn build(&self, app: &mut App) {
        // Add common Soul plugin first (config, registry, types)
        app.add_plugins(CommonSoulPlugin);

        // GUI bridge — applies gui_set_text etc. to GuiElementDisplay components
        app.add_plugins(eustress_common::gui::GuiBridgePlugin);

        // No `MluaPlugin`: it built a Luau VM on the first frame for a queue
        // nothing in the Player fills. A Space's scripts run in the Play
        // runtime's own VM (`common::tree_scripts`).

        // Rune runtime resources
        app.init_resource::<RuneRuntimeState>()
            .init_resource::<RuneModuleRegistry>()
            .init_resource::<SoulRuntime>()
            .init_resource::<HotReloadWatcher>();

        // Register client Rune modules at startup, and pick which of a
        // world's Rune scripts this Player runs: all of them solo, only this
        // player's own when joined (the host runs the rest).
        app.add_systems(Startup, (register_client_rune_modules, pick_rune_scope));

        // The open world's Rune scripts, driven once per frame among the Play
        // frame's scripts. The only Rune driver here: a second one over the
        // same `RuneRuntimeState` would run every `on_update` twice.
        app.add_plugins(TreeRunePlugin);

        // Script execution events
        app.add_message::<ScriptExecuteEvent>()
            .add_message::<ScriptReloadEvent>();

        app.add_systems(Update, (execute_scripts, watch_hot_reload));

        info!("ClientSoulPlugin initialized — Rune + GUI bridge ready");
    }
}

/// Register the Rune modules a Player's scripts compile against: the shared
/// Player set (`eustress::dm`, `eustress::Vector3`, the GUI and log
/// functions, `event_bus`).
fn register_client_rune_modules(
    mut module_registry: ResMut<RuneModuleRegistry>,
) {
    let modules = player_rune_modules();
    let count = modules.len();
    for module in modules {
        module_registry.add_module(module);
    }
    info!("Registered {count} Rune module(s) for the Player");
}

/// Joined to a host, only this player's own Rune runs here; played solo
/// (a published simulation, or a local Space), all of it.
fn pick_rune_scope(mut commands: Commands, join: Option<Res<crate::systems::net_play::JoinTarget>>) {
    commands.insert_resource(if join.is_some() { RuneScope::ClientOnly } else { RuneScope::Everything });
}

// ============================================================================
// Events
// ============================================================================

/// Event: Execute a script
#[derive(Event, Message, Debug, Clone)]
pub struct ScriptExecuteEvent {
    /// Script ID
    pub script_id: String,
    /// Execution context
    pub context: ExecutionContext,
}

/// Event: Script reloaded
#[derive(Event, Message, Debug, Clone)]
pub struct ScriptReloadEvent {
    /// Script ID
    pub script_id: String,
    /// Was reload successful?
    pub success: bool,
}

/// Execution context
#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    /// Triggering entity (if any)
    pub trigger_entity: Option<Entity>,
    /// Event name
    pub event_name: Option<String>,
    /// Custom data
    pub data: std::collections::HashMap<String, String>,
}

// ============================================================================
// Systems
// ============================================================================

/// Execute scripts from event queue
fn execute_scripts(
    mut events: MessageReader<ScriptExecuteEvent>,
    mut _runtime: ResMut<SoulRuntime>,
    _registry: Res<ScriptRegistry>,
) {
    for event in events.read() {
        info!("📜 Executing script '{}' (context: {:?})", event.script_id, event.context.event_name);
        // TODO: Look up script in registry, compile if needed, execute via RuneRuntimeState
    }
}

/// Watch for hot reload
fn watch_hot_reload(
    mut watcher: ResMut<HotReloadWatcher>,
    mut reload_events: MessageWriter<ScriptReloadEvent>,
    config: Res<SoulConfig>,
) {
    if !config.hot_reload {
        return;
    }

    for changed in watcher.poll_changes() {
        info!("🔄 Hot reloading script: {}", changed);
        reload_events.write(ScriptReloadEvent {
            script_id: changed,
            success: true,
        });
    }
}

// ============================================================================
// Client Physics Bridge — Avian3D integration for scripts
// ============================================================================

/// Plugin that bridges script physics commands to Avian3D.
/// Mirrors the engine's RunePhysicsBridgePlugin.
pub struct ClientPhysicsBridgePlugin;

impl Plugin for ClientPhysicsBridgePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            apply_client_physics_commands,
            sync_client_gravity,
        ));
    }
}

/// Apply physics commands from scripts to Avian3D entities.
/// Currently a stub — will be wired when PhysicsCommand moves to common.
fn apply_client_physics_commands() {
    // Physics commands would come from a shared thread-local in common
    // For now this is a no-op until PhysicsCommand is migrated
}

/// Sync gravity from script thread-local to Avian3D.
fn sync_client_gravity(
    mut _gravity: ResMut<Gravity>,
) {
    // Will be wired when PhysicsCommand moves to common
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bare launch builds no Luau VM: the soul plugin no longer adds
    /// `MluaPlugin`, whose state, and the VM made for it on the first frame,
    /// never appear. Adding it back fails this test.
    #[test]
    fn the_soul_plugin_adds_no_luau_runtime() {
        let mut app = App::new();
        app.add_plugins(ClientSoulPlugin);
        assert!(app.world().contains_resource::<RuneRuntimeState>(), "the plugin was built");
        assert!(
            !app.world().contains_resource::<eustress_common::mlua::MluaRuntimeState>(),
            "a bare launch must build no Luau VM"
        );
    }

    /// A Player played solo runs every Rune script in the world; joined to a
    /// host, only its own (the host runs the rest).
    #[test]
    fn a_join_runs_only_this_players_rune() {
        let scope_after = |join: bool| {
            let mut app = App::new();
            if join {
                let link = eustress_networking::JoinLink::parse("eustress://join/127.0.0.1:7777").expect("a link");
                app.insert_resource(crate::systems::net_play::JoinTarget::new(link, "Ada".into(), None, |_| None));
            }
            app.add_systems(Startup, pick_rune_scope);
            app.update();
            *app.world().resource::<RuneScope>()
        };
        assert_eq!(scope_after(false), RuneScope::Everything);
        assert_eq!(scope_after(true), RuneScope::ClientOnly);
    }

    /// The Player's soul plugin drives a world's Rune once a frame, against
    /// the Player's modules: the Rune frame runs `on_update` exactly once per
    /// frame (a second driver over the same runtime would run it twice), and
    /// `eustress::dm` and `eustress::Vector3` resolve.
    #[test]
    fn the_player_runs_a_worlds_rune_once_a_frame() {
        use eustress_common::datamodel::DmValue;
        use eustress_common::play_session::PlayDataModel;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(ClientSoulPlugin);
        let script = r#"
use eustress::dm;
use eustress::Vector3;
pub fn on_update(dt) {
    let b = dm::find_path("Workspace.Box");
    dm::set_attribute_number(b, "Frames", dm::get_attribute_number(b, "Frames") + 1.0);
    dm::set_vector3(b, "Size", Vector3::new(2.0, 2.0, 2.0));
    eustress::log_info("tick");
}
"#;
        let records: Vec<(String, Vec<u8>)> = [
            ("SoulService/_service.toml", "[service]\nclass_name = \"SoulService\"\n"),
            ("SoulService/Counter.rune", script),
            ("Workspace/_service.toml", "[service]\nclass_name = \"Workspace\"\n"),
            (
                "Workspace/Box.part.toml",
                "[metadata]\nclass_name = \"Part\"\nname = \"Box\"\n\n[properties]\nanchored = true\n\n\
                 [transform]\nposition = [0.0, 1.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n",
            ),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.as_bytes().to_vec()))
        .collect();
        let world = eustress_common::tree_read::open_player_world(&records, "Tester");
        let dm = eustress_common::datamodel::new_shared();
        *dm.lock() = world.dm;
        app.update(); // Startup: modules registered, scope picked.
        app.insert_resource(PlayDataModel { dm: dm.clone() });
        for _ in 0..5 {
            app.update();
        }
        let g = dm.lock();
        let ws = g.find_service("Workspace").expect("Workspace");
        let b = g.find_first_child(ws, "Box", false).expect("the box");
        let out: Vec<String> = g.output.iter().map(|l| format!("[{}] {}", l.source, l.text)).collect();
        assert_eq!(g.get_attribute(b, "Frames"), Some(DmValue::Number(5.0)), "{out:?}");
        let size = g.get_prop(b, "Size").and_then(|v| v.as_vector3()).expect("a size");
        assert_eq!((size.x, size.y, size.z), (2.0, 2.0, 2.0));
        assert!(out.iter().any(|l| l == "[SoulService.Counter] Started"), "{out:?}");
        // A second driver would call on_update again each frame: its tree
        // writes are no-ops (nothing published), but its log lines are not.
        let ticks = out.iter().filter(|l| *l == "[SoulService.Counter] tick").count();
        assert_eq!(ticks, 5, "one on_update per frame: {out:?}");
    }
}
