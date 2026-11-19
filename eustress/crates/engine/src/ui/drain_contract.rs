//! The drain contract: `drain_slint_actions` is the one system that turns
//! Slint callbacks into Bevy state and messages, and Bevy skips a system
//! whose parameters fail validation. One missing registration (a required
//! resource, or a message the drain writes) skips the drain every frame, and
//! every Slint callback in Eustress Studio stops at once with one
//! `failed validation` log line. That has shipped twice.
//!
//! Two guards close that class:
//!
//! - [`guard_drain_contract`] runs at `PostStartup` in the Studio. It
//!   validates the drain's whole parameter set against the real `World`
//!   (taken from the function itself, so it cannot drift from the signature)
//!   and panics in debug builds, logs an `error!` in release builds, naming
//!   the parameter.
//! - The tests below build a headless `App` from
//!   [`register_drain_requirements`](super::slint_ui::register_drain_requirements)
//!   (the same function `SlintUiPlugin` calls) plus [`foreign_registrations`],
//!   which stands in for the other plugins the Studio mounts. Remove a
//!   registration from `register_drain_requirements` and
//!   `every_required_drain_param_is_registered` fails, naming it.
//!
//! Run: `cargo test -p eustress-engine --lib ui::drain_contract`.
//!
//! When the drain gains a parameter: register it in
//! `register_drain_requirements` if `SlintUiPlugin` owns it, or in
//! [`foreign_registrations`] if another plugin does.

use bevy::ecs::system::{SystemParamFunction, SystemState};
use bevy::prelude::*;

/// Validates every parameter of the system function `system` against
/// `world` without running it: the same check Bevy makes before each run.
/// `Err` is Bevy's message, which names the parameter (and the bundle field)
/// that failed.
pub(crate) fn validate_system_params<M, F>(_system: &F, world: &mut World) -> Result<(), String>
where
    M: 'static,
    F: SystemParamFunction<M>,
    F::Param: 'static,
{
    let mut state = SystemState::<F::Param>::new(world);
    state.get_mut(world).map(|_| ()).map_err(|error| error.to_string())
}

/// `PostStartup` guard for the Studio: every plugin has built and every
/// `Startup` system has run, so a parameter missing now is missing for good
/// and the drain would be skipped every frame.
pub(crate) fn guard_drain_contract(world: &mut World) {
    let why = match super::slint_ui::check_drain_params(world) {
        Ok(()) => {
            info!("drain contract: every drain_slint_actions parameter is registered");
            return;
        }
        Err(why) => why,
    };
    let report = format!(
        "drain contract broken: Bevy will skip drain_slint_actions every frame, so no \
         Studio button, menu or panel callback will do anything. Register the missing \
         parameter in register_drain_requirements (ui/slint_ui.rs) or in the plugin that \
         owns it. Bevy says: {why}"
    );
    #[cfg(debug_assertions)]
    {
        panic!("{report}");
    }
    #[cfg(not(debug_assertions))]
    {
        error!("{report}");
    }
}

/// What the Studio's other plugins (and `DefaultPlugins`) register that the
/// drain needs: everything the drain requires except what
/// `register_drain_requirements` owns. Kept apart from that function on
/// purpose, so the tests exercise the plugin's own registrations.
#[cfg(test)]
fn foreign_registrations(app: &mut App) {
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
    app.init_asset::<StandardMaterial>();
    app.add_message::<crate::scale_tool::ResizePartEvent>()
        .add_message::<crate::undo::UndoEvent>()
        .add_message::<crate::undo::RedoEvent>()
        .add_message::<crate::undo::UndoSingleEvent>()
        .add_message::<crate::undo::RevertToEvent>()
        .add_message::<crate::undo::HistoryJumpEvent>()
        .add_message::<bevy::app::AppExit>()
        .add_message::<crate::ui::SpawnPartEvent>()
        .add_message::<crate::ui::spawn_events::SpawnTerrainEvent>()
        .add_message::<crate::ui::spawn_events::ToggleTerrainEditEvent>()
        .add_message::<crate::ui::spawn_events::SetTerrainBrushEvent>()
        .add_message::<crate::ui::spawn_events::ImportTerrainEvent>()
        .add_message::<crate::ui::spawn_events::GenerateWorldEvent>()
        .add_message::<crate::ui::spawn_events::GenerateFlatTerrainEvent>()
        .add_message::<crate::workshop::WorkshopSendMessageEvent>()
        .add_message::<crate::workshop::WorkshopApproveMcpEvent>()
        .add_message::<crate::workshop::WorkshopSkipMcpEvent>()
        .add_message::<crate::workshop::WorkshopEditMcpEvent>()
        .add_message::<crate::workshop::WorkshopOpenArtifactEvent>()
        .add_message::<crate::workshop::OptimizeAndBuildEvent>()
        .add_message::<crate::workshop::WorkshopSetModeEvent>()
        .add_message::<crate::workshop::gauntlet::WorkshopSetGauntletEvent>()
        .add_message::<crate::studio_plugins::PluginActionEvent>()
        .add_message::<crate::modal_tool::ToolOptionChangedEvent>()
        .add_message::<crate::modal_tool::CancelModalToolEvent>()
        .add_message::<crate::modal_tool::ActivateModalToolEvent>()
        .add_message::<crate::align_distribute::AlignEntitiesEvent>()
        .add_message::<crate::align_distribute::DistributeEntitiesEvent>()
        .add_message::<crate::ui::floating_windows::DetachPanelEvent>()
        .add_message::<crate::ui::notifications::NotificationEvent>()
        .add_message::<crate::cad_plugin::CadInsertTemplateEvent>()
        .add_message::<crate::cad_plugin::CadSetVariableEvent>()
        .add_message::<crate::cad_plugin::CadExportGlbEvent>()
        .add_message::<crate::cad_plugin::CadTreeOpEvent>()
        .add_message::<crate::cad_plugin::CadSolveSketchEvent>()
        .add_message::<crate::cad_plugin::CadAddConstraintEvent>()
        .add_message::<crate::cad_plugin::CadSketchCanvasSetVisibleEvent>()
        .add_message::<crate::cad_assembly::CadCreateMateEvent>()
        .add_message::<crate::toast_undo::ToastUndoClickedEvent>()
        .add_message::<crate::toast_undo::ToastUndoDismissedEvent>()
        .add_message::<crate::camera_controller::FrameSelectionEvent>()
        .add_message::<crate::clipboard::PasteEvent>();
    app.insert_resource(State::new(crate::play_mode::PlayModeState::Editing));
    app.insert_resource(crate::notifications::NotificationManager::new());
    app.init_resource::<super::slint_ui::OutputConsole>();
    app.init_resource::<super::slint_ui::SlintActionQueue>();
}

#[cfg(test)]
mod tests {
    use super::super::slint_ui::{
        add_drain_system, check_drain_params, register_drain_requirements, OutputConsole, PendingInsertSelection,
        SelectedItem, SlintAction, SlintActionQueue, TerrainVisibility, UnifiedExplorerState,
    };
    use super::*;

    /// The Studio's drain as `SlintUiPlugin` registers it, headless.
    fn studio_drain_app() -> App {
        let mut app = App::new();
        foreign_registrations(&mut app);
        register_drain_requirements(&mut app);
        add_drain_system(&mut app);
        app
    }

    /// Every parameter `drain_slint_actions` declares validates against the
    /// World the plugin's registration path builds, so Bevy runs the drain.
    /// Then the same check on a World missing one required resource fails and
    /// names it, so the check is not vacuous.
    #[test]
    fn every_required_drain_param_is_registered() {
        let mut app = studio_drain_app();
        app.update();
        let checked = check_drain_params(app.world_mut());
        println!("drain contract, plugin registration path: {checked:?}");
        assert_eq!(checked, Ok(()), "drain_slint_actions would be skipped every frame");

        let mut app = studio_drain_app();
        app.world_mut().remove_resource::<TerrainVisibility>();
        let missing = check_drain_params(app.world_mut()).expect_err("TerrainVisibility removed, the check passed");
        println!("drain contract, TerrainVisibility removed: {missing}");
        assert!(missing.contains("terrain_visibility") || missing.contains("TerrainVisibility"), "{missing}");

        let mut app = studio_drain_app();
        app.world_mut().remove_resource::<PendingInsertSelection>();
        let missing = check_drain_params(app.world_mut()).expect_err("PendingInsertSelection removed, the check passed");
        println!("drain contract, PendingInsertSelection removed: {missing}");
        assert!(missing.contains("PendingInsertSelection"), "{missing}");

        // Without the plugin's registrations at all, the drain cannot run.
        let mut app = App::new();
        foreign_registrations(&mut app);
        assert!(check_drain_params(app.world_mut()).is_err(), "the plugin's registrations were not needed");
    }

    /// One queued action is taken off the queue in one Update and does what
    /// it says: a tag added to the selected instance is on that instance.
    #[test]
    fn drain_consumes_a_queued_action() {
        let mut app = studio_drain_app();
        let door = app.world_mut().spawn(eustress_common::classes::Instance::default()).id();
        let mut explorer = UnifiedExplorerState::default();
        explorer.selected = SelectedItem::Entity(door);
        app.insert_resource(explorer);
        app.world().resource::<SlintActionQueue>().push(SlintAction::TagAdded("DrainContract".into()));
        app.update();
        let left = app.world().resource::<SlintActionQueue>().drain();
        assert!(left.is_empty(), "the drain left {} action(s) queued", left.len());
        let tags = app.world().get::<eustress_common::attributes::Tags>(door).map(|t| t.0.clone());
        let output: Vec<String> = app.world().resource::<OutputConsole>().entries.iter().map(|e| e.message.clone()).collect();
        assert_eq!(tags, Some(vec!["DrainContract".to_string()]), "the tag never landed; Output: {output:?}");
    }

    /// `StudioState` is defined once in the workspace (`crate::ui::StudioState`,
    /// ui/mod.rs). A second definition splits the state the drain writes.
    #[test]
    fn studio_state_is_defined_exactly_once() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        collect_rust_files(&crates, &mut files);
        assert!(files.len() > 100, "found only {} .rs files under {}", files.len(), crates.display());
        let mut definitions = Vec::new();
        for file in &files {
            let Ok(text) = std::fs::read_to_string(file) else { continue };
            for (index, line) in text.lines().enumerate() {
                if defines_studio_state(line) {
                    definitions.push(format!("{}:{}", file.display(), index + 1));
                }
            }
        }
        println!("StudioState definitions: {definitions:?}");
        assert_eq!(definitions.len(), 1, "StudioState must have exactly one definition: {definitions:?}");
        assert!(definitions[0].replace('\\', "/").contains("engine/src/ui/mod.rs:"), "{definitions:?}");
    }

    /// The scan's own rule, checked on lines it must and must not count.
    #[test]
    fn the_definition_scan_counts_code_not_comments() {
        assert!(defines_studio_state("pub struct StudioState {"));
        assert!(defines_studio_state("    pub(crate) struct StudioState;"));
        assert!(defines_studio_state("struct StudioState<T> {"));
        assert!(defines_studio_state("pub enum StudioState {"));
        assert!(defines_studio_state("type StudioState = Foo;"));
        assert!(!defines_studio_state("// Do NOT add a local `struct StudioState` here"));
        assert!(!defines_studio_state("/// struct StudioState"));
        assert!(!defines_studio_state("pub struct StudioStateCache {"));
        assert!(!defines_studio_state("pub use super::StudioState;"));
        assert!(!defines_studio_state("let message = \"struct StudioState\";"));
        assert!(defines_studio_state("pub struct StudioState { name: String } // \"x\""));
    }

    fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name == "target" || name.starts_with("target-") || name == "node_modules" || name.starts_with('.') {
                    continue;
                }
                collect_rust_files(&path, out);
            } else if name.ends_with(".rs") {
                out.push(path);
            }
        }
    }

    /// A line that defines a type named exactly `StudioState`: code, not a
    /// comment, and not a longer name.
    fn defines_studio_state(line: &str) -> bool {
        let code = line.trim_start();
        if code.starts_with("//") || code.starts_with("/*") || code.starts_with('*') {
            return false;
        }
        // Drop string literals, so a test's or a message's text is not code.
        let mut code_only = String::with_capacity(code.len());
        let (mut in_string, mut escaped) = (false, false);
        for c in code.chars() {
            if in_string {
                match (escaped, c) {
                    (true, _) => escaped = false,
                    (false, '\\') => escaped = true,
                    (false, '"') => in_string = false,
                    _ => {}
                }
            } else if c == '"' {
                in_string = true;
                code_only.push(' ');
            } else {
                code_only.push(c);
            }
        }
        let code = code_only.split("//").next().unwrap_or("");
        let words: Vec<&str> = code.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
        words
            .windows(2)
            .any(|pair| matches!(pair[0], "struct" | "enum" | "type" | "union") && pair[1] == "StudioState")
    }
}
