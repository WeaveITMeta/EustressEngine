// ============================================================================
// Slint Overlay Thread — Borderless UI Shell Over Native Bevy Window
// ============================================================================
//
// Architecture (Native Dual-Window with Overlay Composition):
//   - Bevy runs NATIVELY on the main thread (full GPU window, native input)
//   - Slint runs in THIS BACKGROUND THREAD with a borderless overlay window
//   - The overlay viewport center is transparent so Bevy shows through
//   - UI panels (explorer, properties, toolbar, output) surround the viewport
//   - Communication is data-only via SlintBridge (Bevy→Slint) and
//     SlintActionBridge (Slint→Bevy). No texture bridge, no input forwarding.
//   - Win32 APIs (in native_compose.rs) link the windows for move/minimize/alt-tab
//
// Table of Contents:
//   - run_slint_overlay_loop(): entry point — creates borderless overlay, wires callbacks
//   - wire_callbacks(): connects all ui.on_*() to SlintActionBridge
//   - apply_bridge_state(): reads BridgeState snapshot and applies to StudioWindow
// ============================================================================

use super::slint_bridge::{SlintBridge, SlintActionBridge, BridgeState};
use super::slint_ui::SlintAction;

// Import generated Slint types
slint::include_modules!();

/// Run the Slint overlay event loop in a background thread.
/// Creates a borderless overlay window that frames UI panels around the Bevy viewport.
///
/// - `bridge`: shared state written by Bevy sync systems, read here via timer
/// - `action_bridge`: actions from Slint callbacks, read by Bevy drain system
pub fn run_slint_overlay_loop(
    bridge: SlintBridge,
    action_bridge: SlintActionBridge,
) -> Result<(), slint::PlatformError> {
    // Register the custom Win32 platform BEFORE creating any Slint components.
    // This replaces Slint's default winit backend with a raw Win32 window +
    // software renderer, avoiding the winit event loop conflict with Bevy.
    #[cfg(target_os = "windows")]
    {
        let platform = super::win32_platform::Win32Platform::new();
        slint::platform::set_platform(Box::new(platform))
            .map_err(|e| slint::PlatformError::Other(format!("set_platform failed: {}", e).into()))?;
    }

    // Create the StudioWindow Slint component (uses our custom Win32 platform)
    let ui = StudioWindow::new()?;

    // Set initial UI state
    ui.set_dark_theme(true);
    ui.set_show_explorer(true);
    ui.set_show_properties(true);
    ui.set_show_output(true);
    ui.set_show_toolbox(true);

    // Wire all Slint callbacks → SlintActionBridge
    wire_callbacks(&ui, &action_bridge);

    // Bridge state sync timer: periodically read BridgeState from Bevy and apply
    // to the Slint UI. Runs at ~60 FPS (16ms) for responsive property updates.
    // This replaces the old rendering notifier approach that was tied to texture exchange.
    let bridge_sync_timer = slint::Timer::default();
    let ui_weak_sync = ui.as_weak();
    let bridge_for_sync = bridge.clone();
    bridge_sync_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
        if let Some(ui) = ui_weak_sync.upgrade() {
            let snapshot = bridge_for_sync.lock().take();
            if snapshot.has_changes() {
                apply_bridge_state(&ui, snapshot);
            }
        }
    });

    // Run the Slint event loop (blocks until overlay window is closed)
    ui.run()?;

    Ok(())
}

/// Wire all StudioWindow callbacks to push SlintActions into the SlintActionBridge.
/// This is the Slint → Bevy direction. Identical logic to the old setup_slint_overlay,
/// but uses SlintActionBridge (Arc<Mutex<Vec<SlintAction>>>) instead of SlintActionQueue.
fn wire_callbacks(ui: &StudioWindow, queue: &SlintActionBridge) {
    // File operations
    let q = queue.clone(); ui.on_new_universe(move || q.push(SlintAction::NewUniverse));
    let q = queue.clone(); ui.on_new_scene(move || q.push(SlintAction::NewScene));
    let q = queue.clone(); ui.on_open_scene(move || q.push(SlintAction::OpenScene));
    let q = queue.clone(); ui.on_save_scene(move || q.push(SlintAction::SaveScene));
    let q = queue.clone(); ui.on_save_scene_as(move || q.push(SlintAction::SaveSceneAs));
    let q = queue.clone(); ui.on_open_publish_dialog(move || q.push(SlintAction::OpenPublishDialog));
    let q = queue.clone(); ui.on_publish_as(move || q.push(SlintAction::OpenPublishAsDialog));
    let q = queue.clone();
    ui.on_publish(move |experience_name, description, genre, is_public, open_source, studio_editable, as_new| {
        q.push(SlintAction::Publish(super::file_dialogs::PublishRequest {
            experience_name: experience_name.to_string(),
            description: description.to_string(),
            genre: genre.to_string(),
            is_public,
            open_source,
            studio_editable,
            as_new,
        }))
    });

    // Edit operations
    let q = queue.clone(); ui.on_undo(move || q.push(SlintAction::Undo));
    let q = queue.clone(); ui.on_redo(move || q.push(SlintAction::Redo));
    let q = queue.clone(); ui.on_copy(move || q.push(SlintAction::Copy));
    let q = queue.clone(); ui.on_cut(move || q.push(SlintAction::Cut));
    let q = queue.clone(); ui.on_paste(move || q.push(SlintAction::Paste));
    let q = queue.clone(); ui.on_delete_selected(move || q.push(SlintAction::Delete));
    let q = queue.clone(); ui.on_duplicate(move || q.push(SlintAction::Duplicate));
    let q = queue.clone(); ui.on_select_all(move || q.push(SlintAction::SelectAll));

    // Tool selection
    let q = queue.clone(); ui.on_select_tool(move |tool| q.push(SlintAction::SelectTool(tool.to_string())));

    // Transform mode
    let q = queue.clone(); ui.on_set_transform_mode(move |mode| q.push(SlintAction::SetTransformMode(mode.to_string())));
    let q = queue.clone(); ui.on_toggle_snap(move || q.push(SlintAction::ToggleSnap));
    let q = queue.clone(); ui.on_set_snap_increment(move |val| q.push(SlintAction::SetSnapIncrement(val)));

    // View
    let q = queue.clone(); ui.on_set_view_mode(move |mode| q.push(SlintAction::SetViewMode(mode.to_string())));
    let q = queue.clone(); ui.on_focus_selected(move || q.push(SlintAction::FocusSelected));
    let q = queue.clone(); ui.on_toggle_wireframe(move || q.push(SlintAction::ToggleWireframe));
    let q = queue.clone(); ui.on_toggle_grid(move || q.push(SlintAction::ToggleGrid));

    // Play controls
    let q = queue.clone(); ui.on_play_solo(move || q.push(SlintAction::PlaySolo));
    let q = queue.clone(); ui.on_play_with_character(move || q.push(SlintAction::PlayWithCharacter));
    let q = queue.clone(); ui.on_pause(move || q.push(SlintAction::Pause));
    let q = queue.clone(); ui.on_stop(move || q.push(SlintAction::Stop));

    // Simulation settings
    let q = queue.clone(); ui.on_save_simulation_settings(move || q.push(SlintAction::SaveSimulationSettings));
    let q = queue.clone(); ui.on_add_sim_watchpoint(move || q.push(SlintAction::AddSimWatchpoint));
    let q = queue.clone(); ui.on_remove_sim_watchpoint(move |i| q.push(SlintAction::RemoveSimWatchpoint(i)));
    let q = queue.clone(); ui.on_add_sim_output_binding(move || q.push(SlintAction::AddSimOutputBinding));
    let q = queue.clone(); ui.on_remove_sim_output_binding(move |i| q.push(SlintAction::RemoveSimOutputBinding(i)));

    // Explorer (unified: entities + files)
    let q = queue.clone(); ui.on_select_node(move |id, node_type| q.push(SlintAction::SelectNode(id, node_type.to_string())));
    let q = queue.clone(); ui.on_expand_node(move |id, node_type| q.push(SlintAction::ExpandNode(id, node_type.to_string())));
    let q = queue.clone(); ui.on_collapse_node(move |id, node_type| q.push(SlintAction::CollapseNode(id, node_type.to_string())));
    let q = queue.clone(); ui.on_open_node(move |id, node_type| q.push(SlintAction::OpenNode(id, node_type.to_string())));
    let q = queue.clone(); ui.on_rename_node(move |id, name, node_type| q.push(SlintAction::RenameNode(id, name.to_string(), node_type.to_string())));
    let q = queue.clone(); ui.on_add_service(move || q.push(SlintAction::AddService));
    let q = queue.clone(); ui.on_expand_all(move || q.push(SlintAction::ExpandAll));
    let q = queue.clone(); ui.on_collapse_all(move || q.push(SlintAction::CollapseAll));

    // Properties
    let q = queue.clone(); ui.on_property_changed(move |key, val| q.push(SlintAction::PropertyChanged(key.to_string(), val.to_string())));
    let q = queue.clone(); ui.on_section_toggle(move |category| q.push(SlintAction::SectionToggle(category.to_string())));

    // Help icons
    let q = queue.clone(); ui.on_open_learn_url(move |url| q.push(SlintAction::OpenLearnUrl(url.to_string())));
    let q = queue.clone(); ui.on_help_icons_changed(move |val| q.push(SlintAction::ShowHelpIconsChanged(val)));
    let q = queue.clone(); ui.on_help_opens_in_tab_changed(move |val| q.push(SlintAction::HelpOpensInTabChanged(val)));

    // Command bar
    let q = queue.clone(); ui.on_execute_command(move |cmd| q.push(SlintAction::ExecuteCommand(cmd.to_string())));

    // Toolbox part insertion
    let q = queue.clone(); ui.on_insert_part(move |part_type| q.push(SlintAction::InsertPart(part_type.to_string())));

    // Ribbon menu actions
    let q = queue.clone(); ui.on_menu_action(move |action| q.push(SlintAction::MenuAction(action.to_string())));

    // Context menu
    let q = queue.clone(); ui.on_context_action(move |action| q.push(SlintAction::ContextAction(action.to_string())));

    // Terrain
    let q = queue.clone(); ui.on_generate_terrain(move |size| q.push(SlintAction::GenerateTerrain(size.to_string())));
    let q = queue.clone(); ui.on_toggle_terrain_edit_mode(move || q.push(SlintAction::ToggleTerrainEditMode));
    let q = queue.clone(); ui.on_set_terrain_brush(move |brush| q.push(SlintAction::SetTerrainBrush(brush.to_string())));
    let q = queue.clone(); ui.on_import_heightmap(move || q.push(SlintAction::ImportHeightmap));
    let q = queue.clone(); ui.on_export_heightmap(move || q.push(SlintAction::ExportHeightmap));

    // Asset Manager
    let q = queue.clone(); ui.on_asset_select(move |id| q.push(SlintAction::AssetSelect(id)));
    let q = queue.clone(); ui.on_asset_expand(move |id| q.push(SlintAction::AssetExpand(id)));
    let q = queue.clone(); ui.on_asset_import(move |kind| q.push(SlintAction::AssetImport(kind.to_string())));
    let q = queue.clone(); ui.on_asset_search(move |text| q.push(SlintAction::AssetSearch(text.to_string())));
    let q = queue.clone(); ui.on_asset_category_changed(move |cat| q.push(SlintAction::AssetCategoryChanged(cat.to_string())));

    // Network
    let q = queue.clone(); ui.on_start_server(move || q.push(SlintAction::StartServer));
    let q = queue.clone(); ui.on_stop_server(move || q.push(SlintAction::StopServer));
    let q = queue.clone(); ui.on_connect_forge(move || q.push(SlintAction::ConnectForge));
    let q = queue.clone(); ui.on_disconnect_forge(move || q.push(SlintAction::DisconnectForge));
    let q = queue.clone(); ui.on_allocate_forge_server(move || q.push(SlintAction::AllocateForgeServer));
    let q = queue.clone(); ui.on_spawn_synthetic_clients(move |count| q.push(SlintAction::SpawnSyntheticClients(count)));
    let q = queue.clone(); ui.on_disconnect_all_clients(move || q.push(SlintAction::DisconnectAllClients));

    // Data
    let q = queue.clone(); ui.on_open_global_sources(move || q.push(SlintAction::OpenGlobalSources));
    let q = queue.clone(); ui.on_open_domains(move || q.push(SlintAction::OpenDomains));
    let q = queue.clone(); ui.on_open_global_variables(move || q.push(SlintAction::OpenGlobalVariables));

    // MindSpace
    let q = queue.clone(); ui.on_toggle_mindspace(move || q.push(SlintAction::ToggleMindspace));
    let q = queue.clone(); ui.on_mindspace_add_label(move || q.push(SlintAction::MindspaceAddLabel));
    let q = queue.clone(); ui.on_mindspace_connect(move || q.push(SlintAction::MindspaceConnect));

    // Auth
    let q = queue.clone(); ui.on_login(move || q.push(SlintAction::Login));
    let q = queue.clone(); ui.on_logout(move || q.push(SlintAction::Logout));

    // Scripts
    let q = queue.clone(); ui.on_build_script(move |id| q.push(SlintAction::BuildScript(id)));
    let q = queue.clone(); ui.on_open_script(move |id| q.push(SlintAction::OpenScript(id)));

    // Center tab management
    let q = queue.clone(); ui.on_close_center_tab(move |idx| q.push(SlintAction::CloseCenterTab(idx)));
    let q = queue.clone(); ui.on_select_center_tab(move |idx| q.push(SlintAction::SelectCenterTab(idx)));
    let q = queue.clone(); ui.on_script_content_changed(move |text| q.push(SlintAction::ScriptContentChanged(text.to_string())));
    let q = queue.clone(); ui.on_reorder_center_tab(move |from, to| q.push(SlintAction::ReorderCenterTab(from, to)));
    let q = queue.clone(); ui.on_toggle_mode(move |idx| q.push(SlintAction::ToggleTabMode(idx)));

    // Web browser
    let q = queue.clone(); ui.on_open_web_tab(move |url| q.push(SlintAction::OpenWebTab(url.to_string())));
    let q = queue.clone(); ui.on_web_navigate(move |url| q.push(SlintAction::WebNavigate(url.to_string())));
    let q = queue.clone(); ui.on_web_go_back(move || q.push(SlintAction::WebGoBack));
    let q = queue.clone(); ui.on_web_go_forward(move || q.push(SlintAction::WebGoForward));
    let q = queue.clone(); ui.on_web_refresh(move || q.push(SlintAction::WebRefresh));

    // Settings
    let q = queue.clone(); ui.on_open_settings(move || q.push(SlintAction::ShowSettings));
    let q = queue.clone(); ui.on_open_find(move || q.push(SlintAction::ShowFind));

    // Layout
    let q = queue.clone(); ui.on_apply_layout_preset(move |preset| q.push(SlintAction::ApplyLayoutPreset(preset)));
    let q = queue.clone(); ui.on_save_layout_to_file(move || q.push(SlintAction::SaveLayoutToFile));
    let q = queue.clone(); ui.on_load_layout_from_file(move || q.push(SlintAction::LoadLayoutFromFile));
    let q = queue.clone(); ui.on_reset_layout_to_default(move || q.push(SlintAction::ResetLayoutToDefault));
    let q = queue.clone(); ui.on_toggle_theme_editor(move || q.push(SlintAction::ToggleThemeEditor));
    let q = queue.clone(); ui.on_apply_theme_settings(move |dark, hc, scale| q.push(SlintAction::ApplyThemeSettings(dark, hc, scale)));
    let q = queue.clone(); ui.on_detach_panel_to_window(move |panel| q.push(SlintAction::DetachPanelToWindow(panel.to_string())));

    // Viewport bounds
    let q = queue.clone(); ui.on_viewport_bounds_changed(move |x, y, w, h| q.push(SlintAction::ViewportBoundsChanged(x, y, w, h)));
    
    // Viewport pointer forwarding removed — Bevy handles native input in dual-window architecture.

    // Close
    let q = queue.clone(); ui.on_close_requested(move || q.push(SlintAction::CloseRequested));
    let q = queue.clone(); ui.on_force_exit(move || q.push(SlintAction::ForceExit));

    // Workshop Panel (System 0: Ideation)
    let q = queue.clone(); ui.on_workshop_send_message(move |text| q.push(SlintAction::WorkshopSendMessage(text.to_string())));
    let q = queue.clone(); ui.on_workshop_approve_mcp(move |id| q.push(SlintAction::WorkshopApproveMcp(id)));
    let q = queue.clone(); ui.on_workshop_skip_mcp(move |id| q.push(SlintAction::WorkshopSkipMcp(id)));
    let q = queue.clone(); ui.on_workshop_edit_mcp(move |id| q.push(SlintAction::WorkshopEditMcp(id)));
    let q = queue.clone(); ui.on_workshop_open_artifact(move |path| q.push(SlintAction::WorkshopOpenArtifact(path.to_string())));
    let q = queue.clone(); ui.on_workshop_start_pipeline(move || q.push(SlintAction::WorkshopStartPipeline));
    let q = queue.clone(); ui.on_workshop_pause_pipeline(move || q.push(SlintAction::WorkshopPausePipeline));
    let q = queue.clone(); ui.on_workshop_resume_pipeline(move || q.push(SlintAction::WorkshopResumePipeline));
    let q = queue.clone(); ui.on_workshop_cancel_pipeline(move || q.push(SlintAction::WorkshopCancelPipeline));
    let q = queue.clone(); ui.on_workshop_optimize_and_build(move || q.push(SlintAction::WorkshopOptimizeAndBuild));
}

/// Apply a BridgeState snapshot to the StudioWindow.
/// Only fields that are Some() get applied — None means "no change this frame".
/// This runs on the Slint main thread inside the rendering notifier.
fn apply_bridge_state(ui: &StudioWindow, state: BridgeState) {
    // ── Scalar properties (only set when changed) ──
    if let Some(v) = state.current_tool { ui.set_current_tool(v.into()); }
    if let Some(v) = state.transform_mode { ui.set_transform_mode(v.into()); }
    if let Some(v) = state.play_state { ui.set_play_state(v.into()); }
    if let Some(v) = state.current_fps { ui.set_current_fps(v); }
    if let Some(v) = state.current_frame_time { ui.set_current_frame_time(v); }
    if let Some(v) = state.current_entity_count { ui.set_current_entity_count(v); }
    if let Some(v) = state.active_tab_type { ui.set_active_tab_type(v.into()); }
    if let Some(v) = state.center_active_tab { ui.set_center_active_tab(v); }
    if let Some(v) = state.has_unsaved_changes { ui.set_has_unsaved_changes(v); }

    // ── Panel visibility ──
    if let Some(v) = state.show_explorer { ui.set_show_explorer(v); }
    if let Some(v) = state.show_properties { ui.set_show_properties(v); }
    if let Some(v) = state.show_output { ui.set_show_output(v); }
    if let Some(v) = state.show_terrain_editor { ui.set_show_terrain_editor(v); }
    if let Some(v) = state.show_network_panel { ui.set_show_network_panel(v); }
    if let Some(v) = state.show_mindspace_panel { ui.set_show_mindspace_panel(v); }
    if let Some(v) = state.show_exit_confirmation { ui.set_show_exit_confirmation(v); }
    if let Some(v) = state.show_help_icons { ui.set_show_help_icons(v); }
    if let Some(v) = state.help_opens_in_tab { ui.set_help_opens_in_tab(v); }

    // ── Terrain ──
    if let Some(v) = state.has_terrain { ui.set_has_terrain(v); }
    if let Some(v) = state.terrain_edit_mode { ui.set_terrain_edit_mode(v); }
    if let Some(v) = state.terrain_brush { ui.set_terrain_brush(v.into()); }
    if let Some(v) = state.terrain_size { ui.set_terrain_size(v.into()); }
    if let Some(v) = state.terrain_chunk_count { ui.set_terrain_chunk_count(v.into()); }

    // ── Editor settings ──
    if let Some(v) = state.grid_visible { ui.set_grid_visible(v); }
    if let Some(v) = state.grid_size { ui.set_grid_size(v); }
    if let Some(v) = state.snap_enabled { ui.set_snap_enabled(v); }
    if let Some(v) = state.snap_size { ui.set_snap_size(v); }

    // ── Account ──
    if let Some(v) = state.account_name { ui.set_account_name(v.into()); }
    if let Some(v) = state.account_status { ui.set_account_status(v.into()); }

    // ── Selection ──
    if let Some(v) = state.selected_count { ui.set_selected_count(v); }
    if let Some(v) = state.selected_class { ui.set_selected_class(v.into()); }
    if let Some(v) = state.selected_icon_name {
        if v.is_empty() {
            ui.set_selected_icon(slint::Image::default());
        } else {
            let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("assets").join("icons").join(format!("{}.svg", v));
            ui.set_selected_icon(slint::Image::load_from_path(&icon_path).unwrap_or_default());
        }
    }

    // ── Publish dialog ──
    if let Some(v) = state.show_publish_dialog { ui.set_show_publish_dialog(v); }
    if let Some(v) = state.publish_experience_name { ui.set_publish_experience_name(v.into()); }
    if let Some(v) = state.publish_description { ui.set_publish_description(v.into()); }
    if let Some(v) = state.publish_genre { ui.set_publish_genre(v.into()); }
    if let Some(v) = state.publish_target { ui.set_publish_target(v.into()); }
    if let Some(v) = state.publish_is_public { ui.set_publish_is_public(v); }
    if let Some(v) = state.publish_open_source { ui.set_publish_open_source(v); }
    if let Some(v) = state.publish_studio_editable { ui.set_publish_studio_editable(v); }
    if let Some(v) = state.publish_as_new { ui.set_publish_as_new(v); }
    if let Some(v) = state.publish_is_update { ui.set_publish_is_update(v); }

    // ── Workshop ──
    if let Some(v) = state.workshop_state { ui.set_workshop_pipeline_state(v.into()); }
    if let Some(v) = state.workshop_product_name { ui.set_workshop_product_name(v.into()); }
    if let Some(v) = state.workshop_artifact_count { ui.set_workshop_total_artifacts(v); }
    if let Some(v) = state.workshop_estimated_cost { ui.set_workshop_estimated_cost(v.into()); }
    if let Some(v) = state.workshop_api_key_valid { ui.set_workshop_api_key_valid(v); }

    // ── Model data (VecModel payloads) ──
    // Explorer tree nodes
    if let Some(nodes) = state.explorer_nodes {
        let slint_nodes: Vec<TreeNode> = nodes.into_iter().map(|n| TreeNode {
            id: n.entity_id as i32,
            name: n.name.into(),
            icon: load_icon_by_name(&n.icon),
            depth: n.depth,
            expandable: n.has_children,
            expanded: n.is_expanded,
            selected: n.is_selected,
            visible: true,
            node_type: n.node_type.into(),
            class_name: n.icon.into(),
            path: slint::SharedString::default(),
            is_directory: false,
            extension: slint::SharedString::default(),
            size: slint::SharedString::default(),
            modified: false,
        }).collect();
        let model = std::rc::Rc::new(slint::VecModel::from(slint_nodes));
        ui.set_tree_nodes(slint::ModelRc::from(model));
    }

    // Entity properties (for Properties panel)
    if let Some(props) = state.entity_properties {
        let slint_props: Vec<PropertyData> = props.into_iter().map(|p| PropertyData {
            name: p.name.as_str().into(),
            value: p.value.as_str().into(),
            property_type: p.property_type.as_str().into(),
            category: p.category.as_str().into(),
            editable: !p.is_readonly,
            options: slint::ModelRc::default(),
            is_header: p.is_header,
            section_collapsed: p.section_collapsed,
            x_value: p.x_value.as_str().into(),
            y_value: p.y_value.as_str().into(),
            z_value: p.z_value.as_str().into(),
            description: p.description.as_str().into(),
            learn_url: p.learn_url.as_str().into(),
        }).collect();
        let model = std::rc::Rc::new(slint::VecModel::from(slint_props));
        ui.set_entity_properties(slint::ModelRc::from(model));
    }

    // Output logs
    if let Some(logs) = state.output_logs {
        let slint_logs: Vec<LogData> = logs.into_iter().enumerate().map(|(i, l)| LogData {
            id: i as i32,
            level: l.level.into(),
            timestamp: l.timestamp.into(),
            message: l.text.into(),
            source: slint::SharedString::default(),
        }).collect();
        let model = std::rc::Rc::new(slint::VecModel::from(slint_logs));
        ui.set_output_logs(slint::ModelRc::from(model));
    }

    // Workshop messages
    if let Some(messages) = state.workshop_messages {
        let slint_messages: Vec<ChatMessage> = messages.into_iter().enumerate().map(|(i, m)| ChatMessage {
            id: i as i32,
            role: m.role.into(),
            content: m.content.into(),
            timestamp: m.timestamp.into(),
            mcp_endpoint: slint::SharedString::default(),
            mcp_method: slint::SharedString::default(),
            mcp_status: slint::SharedString::default(),
            artifact_path: slint::SharedString::default(),
            artifact_type: m.message_type.into(),
        }).collect();
        let model = std::rc::Rc::new(slint::VecModel::from(slint_messages));
        ui.set_workshop_messages(slint::ModelRc::from(model));
    }

    // Workshop pipeline steps
    if let Some(steps) = state.workshop_steps {
        let slint_steps: Vec<PipelineStepData> = steps.into_iter().enumerate().map(|(i, s)| PipelineStepData {
            index: i as i32,
            label: s.name.into(),
            status: s.status.into(),
            artifact_count: 0,
        }).collect();
        let model = std::rc::Rc::new(slint::VecModel::from(slint_steps));
        ui.set_workshop_pipeline_steps(slint::ModelRc::from(model));
    }

    // Center tabs
    if let Some(tabs) = state.center_tabs {
        let slint_tabs: Vec<CenterTab> = tabs.into_iter().map(|t| CenterTab {
            entity_id: t.entity_id,
            name: t.name.into(),
            tab_type: t.tab_type.into(),
            mode: slint::SharedString::default(),
            dirty: t.dirty,
            content: t.content.into(),
            url: t.url.into(),
            loading: t.loading,
            favicon: slint::Image::default(),
            can_go_back: false,
            can_go_forward: false,
        }).collect();
        let model = std::rc::Rc::new(slint::VecModel::from(slint_tabs));
        ui.set_center_tabs(slint::ModelRc::from(model));
    }
}

/// Load an SVG icon from the assets/icons directory by name.
/// Called on the Slint main thread where slint::Image is valid.
/// The name should match an SVG filename without extension (e.g., "part", "model", "folder").
fn load_icon_by_name(name: &str) -> slint::Image {
    if name.is_empty() {
        return slint::Image::default();
    }
    let icon_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("icons")
        .join(format!("{}.svg", name));
    slint::Image::load_from_path(&icon_path).unwrap_or_default()
}
