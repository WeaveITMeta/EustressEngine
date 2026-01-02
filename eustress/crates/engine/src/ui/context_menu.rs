use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::ui::{MenuActionEvent, SpawnPartEvent};
use crate::keybindings::Action;
use crate::classes::{PartType, ClassName, Instance};
use eustress_common::classes::{
    ScreenGui, SurfaceGui, Frame, ScrollingFrame, VideoFrame, DocumentFrame, WebFrame,
    ImageLabel, ImageButton, TextButton,
};
use crate::ui::explorer::ServiceType;
use crate::soul::SoulScriptData;
use crate::play_mode::{PlayModeState, SpawnedDuringPlayMode};

// ============================================================================
// Context Menu State
// ============================================================================

/// Context for what was right-clicked
#[derive(Debug, Clone, Default)]
pub enum ContextTarget {
    #[default]
    Viewport,                           // Right-clicked in 3D viewport
    Service(ServiceType),               // Right-clicked on a service (Workspace, Lighting, etc.)
    Entity(Entity, ClassName),          // Right-clicked on an entity with its class
}

/// Resource tracking context menu state
#[derive(Resource)]
pub struct ContextMenuState {
    pub open: bool,
    pub position: egui::Pos2,
    pub target: ContextTarget,
    pub right_click_start: Option<egui::Pos2>,
    pub drag_threshold: f32,
    pub insert_submenu_open: bool,
    /// Insert panel state - opens a side panel like Explorer Insert
    pub insert_panel_open: bool,
    /// Insert panel position (anchored to context menu position)
    pub insert_panel_pos: egui::Pos2,
    /// Parent entity for insert (None = Workspace root)
    pub insert_panel_parent: Option<Entity>,
    /// Selected category in insert panel
    pub insert_panel_category: Option<InsertCategory>,
}

impl Default for ContextMenuState {
    fn default() -> Self {
        Self {
            open: false,
            position: egui::Pos2::ZERO,
            target: ContextTarget::Viewport,
            right_click_start: None,
            drag_threshold: 5.0,
            insert_submenu_open: false,
            insert_panel_open: false,
            insert_panel_pos: egui::Pos2::ZERO,
            insert_panel_parent: None,
            insert_panel_category: None,
        }
    }
}

// ============================================================================
// Insert Menu Categories
// ============================================================================

/// Categories for the Insert Object submenu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertCategory {
    Part,
    Model,
    Light,
    Effect,
    GUI,
    Script,
    Constraint,
    Other,
}

impl InsertCategory {
    pub fn name(&self) -> &'static str {
        match self {
            InsertCategory::Part => "Part",
            InsertCategory::Model => "Model",
            InsertCategory::Light => "Light",
            InsertCategory::Effect => "Effect",
            InsertCategory::GUI => "GUI",
            InsertCategory::Script => "Script",
            InsertCategory::Constraint => "Constraint",
            InsertCategory::Other => "Other",
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct ContextMenuPlugin;

impl Plugin for ContextMenuPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<ContextMenuState>()
            .add_message::<InsertObjectEvent>()
            .add_systems(Update, (
                handle_right_click_to_open,
                show_context_menu,
                process_explorer_insert_actions,
                process_explorer_reparent_actions,
                handle_insert_object_events,
            ).chain());
    }
}

/// Process insert actions from Explorer context menus
fn process_explorer_insert_actions(
    mut explorer_state: ResMut<super::ExplorerState>,
    mut insert_events: MessageWriter<InsertObjectEvent>,
) {
    // Check if Explorer set an insert_class
    if let Some(class_name) = explorer_state.insert_class.take() {
        let parent = explorer_state.insert_parent.take();
        
        info!("Explorer insert: {:?} into {:?}", class_name, parent);
        
        insert_events.write(InsertObjectEvent {
            class_name,
            parent,
            position: Vec3::ZERO, // Will be positioned relative to parent or at origin
        });
    }
}

/// Process reparent actions from Explorer drag-and-drop
fn process_explorer_reparent_actions(
    mut explorer_state: ResMut<super::ExplorerState>,
    mut commands: Commands,
) {
    // Check if Explorer set a pending reparent operation
    if let Some((child_entity, new_parent_entity)) = explorer_state.pending_reparent.take() {
        info!("🔗 Reparenting {:?} to {:?}", child_entity, new_parent_entity);
        
        // Remove old parent relationship and set new parent using ChildOf component
        commands.entity(child_entity)
            .remove::<ChildOf>()
            .insert(ChildOf(new_parent_entity));
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if a class is a valid GUI container that can have GUI children
/// Valid containers: StarterGui (service), ScreenGui, BillboardGui, SurfaceGui, Frame, ScrollingFrame
fn is_valid_gui_container(class: ClassName) -> bool {
    matches!(class,
        ClassName::ScreenGui | ClassName::BillboardGui | ClassName::SurfaceGui |
        ClassName::Frame | ClassName::ScrollingFrame | ClassName::ViewportFrame
    )
}

// ============================================================================
// Events
// ============================================================================

/// Message to insert an object
#[derive(Message, Debug, Clone)]
pub struct InsertObjectEvent {
    pub class_name: ClassName,
    pub parent: Option<Entity>,
    pub position: Vec3,
}

// ============================================================================
// Systems
// ============================================================================

fn handle_right_click_to_open(
    mouse: Res<ButtonInput<MouseButton>>,
    mut egui_ctx: EguiContexts,
    mut context_menu: ResMut<ContextMenuState>,
) {
    let Ok(ctx) = egui_ctx.ctx_mut() else { return; };
    
    // Don't open if egui already wants pointer input
    if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
        return;
    }
    
    // Track right-click press
    if mouse.just_pressed(MouseButton::Right) {
        if let Some(pos) = ctx.pointer_latest_pos() {
            context_menu.right_click_start = Some(pos);
        }
    }
    
    // On right-click release, check if it was a drag or a click
    if mouse.just_released(MouseButton::Right) {
        if let Some(start_pos) = context_menu.right_click_start {
            if let Some(current_pos) = ctx.pointer_latest_pos() {
                let delta = current_pos - start_pos;
                let distance = (delta.x * delta.x + delta.y * delta.y).sqrt();
                
                if distance < context_menu.drag_threshold {
                    context_menu.position = current_pos;
                    context_menu.target = ContextTarget::Viewport;
                    context_menu.open = true;
                    context_menu.insert_submenu_open = false;
                }
            }
            context_menu.right_click_start = None;
        }
    }
}

fn show_context_menu(
    mut egui_ctx: EguiContexts,
    mut context_menu: ResMut<ContextMenuState>,
    mut menu_events: MessageWriter<MenuActionEvent>,
    mut spawn_events: MessageWriter<SpawnPartEvent>,
    mut insert_events: MessageWriter<InsertObjectEvent>,
    mut undo_events: MessageWriter<crate::undo::UndoEvent>,
    mut redo_events: MessageWriter<crate::undo::RedoEvent>,
) {
    if !context_menu.open {
        return;
    }
    
    let Ok(ctx) = egui_ctx.ctx_mut() else { return; };
    
    let target = context_menu.target.clone();
    let menu_pos = context_menu.position;
    let popup_id = egui::Id::new("viewport_context_menu");
    
    // Show popup menu at the click position
    egui::Area::new(popup_id)
        .fixed_pos(menu_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::menu(&ctx.style())
                .inner_margin(egui::Margin::symmetric(4, 6))
                .show(ui, |ui| {
                    ui.set_min_width(240.0);
                    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 2.0);
                    ui.style_mut().spacing.button_padding = egui::vec2(8.0, 4.0);
                    
                    // Render menu based on context
                    match &target {
                        ContextTarget::Viewport => {
                            render_viewport_menu(ui, &mut context_menu, &mut menu_events, &mut spawn_events, &mut insert_events, &mut undo_events, &mut redo_events);
                        }
                        ContextTarget::Service(service) => {
                            render_service_menu(ui, *service, &mut context_menu, &mut menu_events, &mut insert_events);
                        }
                        ContextTarget::Entity(entity, class) => {
                            render_entity_menu(ui, *entity, *class, &mut context_menu, &mut menu_events, &mut insert_events, &mut undo_events, &mut redo_events);
                        }
                    }
                });
        });
    
    // Close on click outside or Escape
    let dominated = ctx.is_pointer_over_area();
    if ctx.input(|i| i.pointer.any_click()) && !dominated {
        context_menu.open = false;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        context_menu.open = false;
    }
    
    // ════════════════════════════════════════════════════════════════════════
    // Insert Panel (side panel like Explorer Insert)
    // ════════════════════════════════════════════════════════════════════════
    
    if context_menu.insert_panel_open {
        render_insert_panel(ctx, &mut context_menu, &mut spawn_events, &mut insert_events);
    }
}

/// Render the Insert panel as a side panel (like Explorer Insert)
fn render_insert_panel(
    ctx: &egui::Context,
    context_menu: &mut ResMut<ContextMenuState>,
    spawn_events: &mut MessageWriter<SpawnPartEvent>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
) {
    let panel_pos = context_menu.insert_panel_pos;
    let parent = context_menu.insert_panel_parent;
    let popup_id = egui::Id::new("insert_panel");
    
    egui::Area::new(popup_id)
        .fixed_pos(panel_pos)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::menu(&ctx.style())
                .inner_margin(egui::Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.set_min_width(220.0);
                    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 4.0);
                    
                    // Header
                    ui.label(egui::RichText::new("Insert Object").strong().size(14.0));
                    ui.separator();
                    
                    // Category buttons on left, items on right
                    ui.horizontal(|ui| {
                        // Categories column
                        ui.vertical(|ui| {
                            ui.set_min_width(80.0);
                            
                            let categories = [
                                InsertCategory::Part,
                                InsertCategory::Model,
                                InsertCategory::Light,
                                InsertCategory::Effect,
                                InsertCategory::GUI,
                                InsertCategory::Script,
                                InsertCategory::Constraint,
                                InsertCategory::Other,
                            ];
                            
                            for cat in categories {
                                let is_selected = context_menu.insert_panel_category == Some(cat);
                                let btn = egui::Button::new(egui::RichText::new(cat.name()).size(12.0))
                                    .min_size(egui::vec2(75.0, 24.0))
                                    .fill(if is_selected { 
                                        egui::Color32::from_rgb(60, 100, 150) 
                                    } else { 
                                        egui::Color32::TRANSPARENT 
                                    });
                                
                                if ui.add(btn).clicked() {
                                    context_menu.insert_panel_category = Some(cat);
                                }
                            }
                        });
                        
                        ui.separator();
                        
                        // Items column
                        ui.vertical(|ui| {
                            ui.set_min_width(120.0);
                            
                            if let Some(cat) = context_menu.insert_panel_category {
                                render_insert_category_items(ui, cat, parent, spawn_events, insert_events, &mut context_menu.insert_panel_open);
                            } else {
                                ui.label(egui::RichText::new("Select a category").small().color(egui::Color32::GRAY));
                            }
                        });
                    });
                });
        });
    
    // Close on click outside or Escape
    if ctx.input(|i| i.pointer.any_click()) && !ctx.is_pointer_over_area() {
        context_menu.insert_panel_open = false;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        context_menu.insert_panel_open = false;
    }
}

/// Render items for a specific insert category
fn render_insert_category_items(
    ui: &mut egui::Ui,
    category: InsertCategory,
    parent: Option<Entity>,
    spawn_events: &mut MessageWriter<SpawnPartEvent>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
    panel_open: &mut bool,
) {
    match category {
        InsertCategory::Part => {
            if ui.button("Block").clicked() {
                spawn_events.write(SpawnPartEvent { part_type: PartType::Block, position: Vec3::new(0.0, 5.0, 0.0) });
                *panel_open = false;
            }
            if ui.button("Sphere").clicked() {
                spawn_events.write(SpawnPartEvent { part_type: PartType::Ball, position: Vec3::new(0.0, 5.0, 0.0) });
                *panel_open = false;
            }
            if ui.button("Cylinder").clicked() {
                spawn_events.write(SpawnPartEvent { part_type: PartType::Cylinder, position: Vec3::new(0.0, 5.0, 0.0) });
                *panel_open = false;
            }
            if ui.button("Wedge").clicked() {
                spawn_events.write(SpawnPartEvent { part_type: PartType::Wedge, position: Vec3::new(0.0, 5.0, 0.0) });
                *panel_open = false;
            }
            if ui.button("Corner Wedge").clicked() {
                spawn_events.write(SpawnPartEvent { part_type: PartType::CornerWedge, position: Vec3::new(0.0, 5.0, 0.0) });
                *panel_open = false;
            }
        }
        InsertCategory::Model => {
            render_panel_insert_item(ui, "Model", ClassName::Model, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "Folder", ClassName::Folder, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "MeshPart", ClassName::MeshPart, insert_events, parent, panel_open);
        }
        InsertCategory::Light => {
            render_panel_insert_item(ui, "PointLight", ClassName::PointLight, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "SpotLight", ClassName::SpotLight, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "SurfaceLight", ClassName::SurfaceLight, insert_events, parent, panel_open);
        }
        InsertCategory::Effect => {
            render_panel_insert_item(ui, "ParticleEmitter", ClassName::ParticleEmitter, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "Beam", ClassName::Beam, insert_events, parent, panel_open);
        }
        InsertCategory::GUI => {
            render_panel_insert_item(ui, "ScreenGui", ClassName::ScreenGui, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "BillboardGui", ClassName::BillboardGui, insert_events, parent, panel_open);
            ui.separator();
            render_panel_insert_item(ui, "Frame", ClassName::Frame, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "ScrollingFrame", ClassName::ScrollingFrame, insert_events, parent, panel_open);
            ui.separator();
            render_panel_insert_item(ui, "TextLabel", ClassName::TextLabel, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "TextButton", ClassName::TextButton, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "ImageLabel", ClassName::ImageLabel, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "ImageButton", ClassName::ImageButton, insert_events, parent, panel_open);
        }
        InsertCategory::Script => {
            render_panel_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, parent, panel_open);
        }
        InsertCategory::Constraint => {
            render_panel_insert_item(ui, "Attachment", ClassName::Attachment, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "WeldConstraint", ClassName::WeldConstraint, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "Motor6D", ClassName::Motor6D, insert_events, parent, panel_open);
        }
        InsertCategory::Other => {
            render_panel_insert_item(ui, "Sound", ClassName::Sound, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "Decal", ClassName::Decal, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "SpecialMesh", ClassName::SpecialMesh, insert_events, parent, panel_open);
            render_panel_insert_item(ui, "SpawnLocation", ClassName::SpawnLocation, insert_events, parent, panel_open);
        }
    }
}

/// Helper for insert panel items
fn render_panel_insert_item(
    ui: &mut egui::Ui,
    label: &str,
    class: ClassName,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
    parent: Option<Entity>,
    panel_open: &mut bool,
) {
    if ui.button(label).clicked() {
        insert_events.write(InsertObjectEvent {
            class_name: class,
            parent,
            position: Vec3::new(0.0, 5.0, 0.0),
        });
        *panel_open = false;
    }
}

// ============================================================================
// Menu Rendering Functions
// ============================================================================

fn render_viewport_menu(
    ui: &mut egui::Ui,
    context_menu: &mut ResMut<ContextMenuState>,
    menu_events: &mut MessageWriter<MenuActionEvent>,
    spawn_events: &mut MessageWriter<SpawnPartEvent>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
    undo_events: &mut MessageWriter<crate::undo::UndoEvent>,
    redo_events: &mut MessageWriter<crate::undo::RedoEvent>,
) {
    // Edit actions
    render_menu_item(ui, "Undo", "Ctrl+Z", || {
        undo_events.write(crate::undo::UndoEvent);
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Redo", "Ctrl+Y", || {
        redo_events.write(crate::undo::RedoEvent);
    }, &mut context_menu.open);
    
    ui.separator();
    
    render_menu_item(ui, "Copy", "Ctrl+C", || {
        menu_events.write(MenuActionEvent::new(Action::Copy));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Paste", "Ctrl+V", || {
        menu_events.write(MenuActionEvent::new(Action::Paste));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Duplicate", "Ctrl+D", || {
        menu_events.write(MenuActionEvent::new(Action::Duplicate));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Delete", "Del", || {
        menu_events.write(MenuActionEvent::new(Action::Delete));
    }, &mut context_menu.open);
    
    ui.separator();
    
    render_menu_item(ui, "Select All", "Ctrl+A", || {
        menu_events.write(MenuActionEvent::new(Action::SelectAll));
    }, &mut context_menu.open);
    
    ui.separator();
    
    render_menu_item(ui, "Group", "Ctrl+G", || {
        menu_events.write(MenuActionEvent::new(Action::Group));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Ungroup", "Ctrl+U", || {
        menu_events.write(MenuActionEvent::new(Action::Ungroup));
    }, &mut context_menu.open);
    
    ui.separator();
    
    // Insert submenu
    render_insert_submenu(ui, context_menu, spawn_events, insert_events, None);
    
    ui.separator();
    
    render_menu_item(ui, "Zoom to Selection", "F", || {
        menu_events.write(MenuActionEvent::new(Action::FocusSelection));
    }, &mut context_menu.open);
}

fn render_service_menu(
    ui: &mut egui::Ui,
    service: ServiceType,
    context_menu: &mut ResMut<ContextMenuState>,
    menu_events: &mut MessageWriter<MenuActionEvent>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
) {
    // Service-specific header
    ui.label(egui::RichText::new(service.name()).strong().color(service.color()));
    ui.separator();
    
    // Insert Object submenu - filtered by what this service accepts
    render_service_insert_submenu(ui, service, context_menu, insert_events);
    
    ui.separator();
    
    render_menu_item(ui, "Expand All", "", || {
        // TODO: Expand all children
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Collapse All", "", || {
        // TODO: Collapse all children
    }, &mut context_menu.open);
}

fn render_entity_menu(
    ui: &mut egui::Ui,
    entity: Entity,
    class: ClassName,
    context_menu: &mut ResMut<ContextMenuState>,
    menu_events: &mut MessageWriter<MenuActionEvent>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
    undo_events: &mut MessageWriter<crate::undo::UndoEvent>,
    redo_events: &mut MessageWriter<crate::undo::RedoEvent>,
) {
    // Entity header
    ui.label(egui::RichText::new(class.as_str()).strong());
    ui.separator();
    
    render_menu_item(ui, "Copy", "Ctrl+C", || {
        menu_events.write(MenuActionEvent::new(Action::Copy));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Paste", "Ctrl+V", || {
        menu_events.write(MenuActionEvent::new(Action::Paste));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Duplicate", "Ctrl+D", || {
        menu_events.write(MenuActionEvent::new(Action::Duplicate));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Delete", "Del", || {
        menu_events.write(MenuActionEvent::new(Action::Delete));
    }, &mut context_menu.open);
    
    ui.separator();
    
    render_menu_item(ui, "Rename", "F2", || {
        // TODO: Start rename
    }, &mut context_menu.open);
    
    ui.separator();
    
    render_menu_item(ui, "Group", "Ctrl+G", || {
        menu_events.write(MenuActionEvent::new(Action::Group));
    }, &mut context_menu.open);
    
    render_menu_item(ui, "Ungroup", "Ctrl+U", || {
        menu_events.write(MenuActionEvent::new(Action::Ungroup));
    }, &mut context_menu.open);
    
    ui.separator();
    
    // Insert Object submenu - filtered by what can be parented to this class
    render_entity_insert_submenu(ui, entity, class, context_menu, insert_events);
    
    ui.separator();
    
    render_menu_item(ui, "Zoom to", "F", || {
        menu_events.write(MenuActionEvent::new(Action::FocusSelection));
    }, &mut context_menu.open);
}

// ============================================================================
// Insert Submenus
// ============================================================================

fn render_insert_submenu(
    ui: &mut egui::Ui,
    context_menu: &mut ResMut<ContextMenuState>,
    _spawn_events: &mut MessageWriter<SpawnPartEvent>,
    _insert_events: &mut MessageWriter<InsertObjectEvent>,
    parent: Option<Entity>,
) {
    // Instead of nested submenus, open a side panel like Explorer Insert
    if ui.add(egui::Button::new(egui::RichText::new("Insert...  ▶").size(13.0))
        .min_size(egui::vec2(200.0, 22.0))
        .frame(false)).clicked() 
    {
        // Open the insert panel at the menu position
        context_menu.insert_panel_open = true;
        context_menu.insert_panel_pos = context_menu.position + egui::vec2(220.0, 0.0);
        context_menu.insert_panel_parent = parent;
        context_menu.insert_panel_category = None;
        context_menu.open = false; // Close the context menu
    }
}

fn render_service_insert_submenu(
    ui: &mut egui::Ui,
    service: ServiceType,
    context_menu: &mut ResMut<ContextMenuState>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
) {
    ui.menu_button("Insert...", |ui| {
        ui.set_min_width(180.0);
        ui.style_mut().spacing.button_padding = egui::vec2(8.0, 4.0);
        
        match service {
            ServiceType::Workspace => {
                // Workspace accepts Parts, Models, Folders, Scripts, etc.
                ui.menu_button("Part", |ui| {
                    render_workspace_part_items(ui, insert_events, &mut context_menu.open);
                });
                render_insert_item(ui, "Model", ClassName::Model, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "SpawnLocation", ClassName::SpawnLocation, insert_events, None, &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, None, &mut context_menu.open);
            }
            ServiceType::Lighting => {
                // Lighting accepts Sky, Atmosphere, lights
                render_insert_item(ui, "Sky", ClassName::Sky, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Atmosphere", ClassName::Atmosphere, insert_events, None, &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "PointLight", ClassName::PointLight, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "SpotLight", ClassName::SpotLight, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "SurfaceLight", ClassName::SurfaceLight, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "DirectionalLight", ClassName::DirectionalLight, insert_events, None, &mut context_menu.open);
            }
            ServiceType::ServerStorage => {
                // Storage accepts Models, Folders
                render_insert_item(ui, "Model", ClassName::Model, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
            }
            ServiceType::SoulService => {
                // Soul scripts only
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
            }
            ServiceType::StarterGui => {
                // GUI elements
                render_insert_item(ui, "ScreenGui", ClassName::ScreenGui, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "BillboardGui", ClassName::BillboardGui, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, None, &mut context_menu.open);
            }
            ServiceType::StarterPack => {
                // Tools and scripts
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, None, &mut context_menu.open);
            }
            ServiceType::StarterPlayer => {
                // Player scripts
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, None, &mut context_menu.open);
            }
            ServiceType::SoundService => {
                // Sounds
                render_insert_item(ui, "Sound", ClassName::Sound, insert_events, None, &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
            }
            ServiceType::Teams => {
                // Teams (placeholder)
                ui.label("(No insertable objects)");
            }
            _ => {
                // Default - Folder only
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, None, &mut context_menu.open);
            }
        }
    });
}

fn render_entity_insert_submenu(
    ui: &mut egui::Ui,
    parent_entity: Entity,
    parent_class: ClassName,
    context_menu: &mut ResMut<ContextMenuState>,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
) {
    ui.menu_button("Insert...", |ui| {
        ui.set_min_width(180.0);
        ui.style_mut().spacing.button_padding = egui::vec2(8.0, 4.0);
        
        match parent_class {
            // Parts can have attachments, constraints, effects, decals
            ClassName::Part | ClassName::MeshPart | ClassName::SpawnLocation => {
                render_insert_item(ui, "Attachment", ClassName::Attachment, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "WeldConstraint", ClassName::WeldConstraint, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "Motor6D", ClassName::Motor6D, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Decal", ClassName::Decal, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "SpecialMesh", ClassName::SpecialMesh, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "ParticleEmitter", ClassName::ParticleEmitter, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "PointLight", ClassName::PointLight, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "SpotLight", ClassName::SpotLight, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "SurfaceLight", ClassName::SurfaceLight, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Sound", ClassName::Sound, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "BillboardGui", ClassName::BillboardGui, insert_events, Some(parent_entity), &mut context_menu.open);
            }
            // Models can have parts, other models, scripts
            ClassName::Model => {
                ui.menu_button("Part", |ui| {
                    render_workspace_part_items(ui, insert_events, &mut context_menu.open);
                });
                render_insert_item(ui, "Model", ClassName::Model, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Humanoid", ClassName::Humanoid, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "Animator", ClassName::Animator, insert_events, Some(parent_entity), &mut context_menu.open);
            }
            // Folders can have anything
            ClassName::Folder => {
                ui.menu_button("Part", |ui| {
                    render_workspace_part_items(ui, insert_events, &mut context_menu.open);
                });
                render_insert_item(ui, "Model", ClassName::Model, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                render_insert_item(ui, "Soul Script", ClassName::SoulScript, insert_events, Some(parent_entity), &mut context_menu.open);
            }
            // Humanoid can have Animator
            ClassName::Humanoid => {
                render_insert_item(ui, "Animator", ClassName::Animator, insert_events, Some(parent_entity), &mut context_menu.open);
            }
            // BillboardGui/ScreenGui can have GUI children
            ClassName::BillboardGui | ClassName::ScreenGui => {
                ui.label(egui::RichText::new("Containers").small().color(egui::Color32::GRAY));
                render_insert_item(ui, "Frame", ClassName::Frame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "ScrollingFrame", ClassName::ScrollingFrame, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                ui.label(egui::RichText::new("Media").small().color(egui::Color32::GRAY));
                render_insert_item(ui, "VideoFrame", ClassName::VideoFrame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "DocumentFrame", ClassName::DocumentFrame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "ImageLabel", ClassName::ImageLabel, insert_events, Some(parent_entity), &mut context_menu.open);
                ui.separator();
                ui.label(egui::RichText::new("Interactive").small().color(egui::Color32::GRAY));
                render_insert_item(ui, "TextLabel", ClassName::TextLabel, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "TextButton", ClassName::TextButton, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "ImageButton", ClassName::ImageButton, insert_events, Some(parent_entity), &mut context_menu.open);
            }
            // Frame/ScrollingFrame can have GUI children
            ClassName::Frame | ClassName::ScrollingFrame => {
                render_insert_item(ui, "Frame", ClassName::Frame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "ScrollingFrame", ClassName::ScrollingFrame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "VideoFrame", ClassName::VideoFrame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "DocumentFrame", ClassName::DocumentFrame, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "ImageLabel", ClassName::ImageLabel, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "TextLabel", ClassName::TextLabel, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "TextButton", ClassName::TextButton, insert_events, Some(parent_entity), &mut context_menu.open);
                render_insert_item(ui, "ImageButton", ClassName::ImageButton, insert_events, Some(parent_entity), &mut context_menu.open);
            }
            // Default - limited options
            _ => {
                render_insert_item(ui, "Folder", ClassName::Folder, insert_events, Some(parent_entity), &mut context_menu.open);
            }
        }
    });
}

// ============================================================================
// Helper Functions
// ============================================================================

fn render_menu_item<F: FnOnce()>(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    action: F,
    open: &mut bool,
) {
    // Use fixed-width format for proper right-aligned shortcuts
    let button_text = if shortcut.is_empty() {
        egui::RichText::new(label).size(13.0)
    } else {
        let full_text = format!("{:<18}{:>10}", label, shortcut);
        egui::RichText::new(full_text).size(13.0).family(egui::FontFamily::Monospace)
    };
    
    let button = egui::Button::new(button_text)
        .min_size(egui::vec2(220.0, 26.0))
        .frame(false);
    
    let response = ui.add(button);
    
    if response.clicked() {
        action();
        *open = false;
    }
}

fn render_insert_item(
    ui: &mut egui::Ui,
    label: &str,
    class: ClassName,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
    parent: Option<Entity>,
    open: &mut bool,
) {
    let button = egui::Button::new(
        egui::RichText::new(label).size(13.0)
    )
    .min_size(egui::vec2(150.0, 22.0))
    .frame(false);
    
    if ui.add(button).clicked() {
        insert_events.write(InsertObjectEvent {
            class_name: class,
            parent,
            position: Vec3::new(0.0, 5.0, 0.0),
        });
        *open = false;
    }
}

/// Helper to render a hoverable menu item for part spawning
fn render_part_menu_item(
    ui: &mut egui::Ui,
    label: &str,
    part_type: PartType,
    spawn_events: &mut MessageWriter<SpawnPartEvent>,
    open: &mut bool,
) {
    let button = egui::Button::new(
        egui::RichText::new(label).size(13.0)
    )
    .min_size(egui::vec2(150.0, 22.0))
    .frame(false);
    
    if ui.add(button).clicked() {
        spawn_events.write(SpawnPartEvent { part_type, position: Vec3::new(0.0, 5.0, 0.0) });
        *open = false;
    }
}

fn render_part_items(
    ui: &mut egui::Ui,
    spawn_events: &mut MessageWriter<SpawnPartEvent>,
    context_menu: &mut ResMut<ContextMenuState>,
) {
    render_part_menu_item(ui, "Block", PartType::Block, spawn_events, &mut context_menu.open);
    render_part_menu_item(ui, "Sphere", PartType::Ball, spawn_events, &mut context_menu.open);
    render_part_menu_item(ui, "Cylinder", PartType::Cylinder, spawn_events, &mut context_menu.open);
    render_part_menu_item(ui, "Wedge", PartType::Wedge, spawn_events, &mut context_menu.open);
    render_part_menu_item(ui, "CornerWedge", PartType::CornerWedge, spawn_events, &mut context_menu.open);
}

fn render_workspace_part_items(
    ui: &mut egui::Ui,
    insert_events: &mut MessageWriter<InsertObjectEvent>,
    open: &mut bool,
) {
    render_insert_item(ui, "Block", ClassName::Part, insert_events, None, open);
    render_insert_item(ui, "Sphere", ClassName::Part, insert_events, None, open);
    render_insert_item(ui, "Cylinder", ClassName::Part, insert_events, None, open);
    render_insert_item(ui, "Wedge", ClassName::Part, insert_events, None, open);
    render_insert_item(ui, "CornerWedge", ClassName::Part, insert_events, None, open);
}

// ============================================================================
// Event Handler System
// ============================================================================

fn handle_insert_object_events(
    mut events: MessageReader<InsertObjectEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    play_mode_state: Res<State<PlayModeState>>,
    selection_manager: Res<super::BevySelectionManager>,
    instance_query: Query<&Instance>,
    mut notifications: ResMut<crate::notifications::NotificationManager>,
) {
    use crate::classes::*;
    use crate::spawn::*;
    
    let is_playing = *play_mode_state.get() != PlayModeState::Editing;
    
    for event in events.read() {
        info!("Inserting object: {:?} at {:?}", event.class_name, event.position);
        
        let instance = Instance {
            name: event.class_name.as_str().to_string(),
            class_name: event.class_name,
            archivable: true,
            id: 0,
        };
        
        // Track spawned entity for auto-selection
        let mut spawned_entity: Option<Entity> = None;
        
        match event.class_name {
            ClassName::Part => {
                let part = Part::default();
                let mut base_part = BasePart::default();
                base_part.cframe = Transform::from_translation(event.position);
                let entity = spawn_part(&mut commands, &mut meshes, &mut materials, instance, base_part, part);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
                spawned_entity = Some(entity);
            }
            ClassName::MeshPart => {
                let mesh_part = MeshPart::default();
                let mut base_part = BasePart::default();
                base_part.cframe = Transform::from_translation(event.position);
                let entity = spawn_mesh_part(&mut commands, &mut meshes, &asset_server, &mut materials, instance, base_part, mesh_part);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
                spawned_entity = Some(entity);
            }
            ClassName::UnionOperation => {
                // UnionOperation is a special Part that represents CSG union
                let mut base_part = BasePart::default();
                base_part.cframe = Transform::from_translation(event.position);
                base_part.color = Color::srgb(0.6, 0.6, 0.8);
                let union_op = UnionOperation::default();
                let entity = spawn_union(&mut commands, &mut meshes, &mut materials, instance, base_part, union_op);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
                spawned_entity = Some(entity);
            }
            ClassName::Seat => {
                // Seat is a Part with seating functionality
                let mut part = Part::default();
                part.shape = PartType::Block;
                let mut base_part = BasePart::default();
                base_part.size = Vec3::new(2.0, 0.6, 2.0);
                base_part.cframe = Transform::from_translation(event.position);
                base_part.color = Color::srgb(0.4, 0.3, 0.2);
                let entity = spawn_part(&mut commands, &mut meshes, &mut materials, instance, base_part, part);
                commands.entity(entity).insert(Seat::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
                spawned_entity = Some(entity);
            }
            ClassName::VehicleSeat => {
                // VehicleSeat is a Part with vehicle control functionality
                let mut part = Part::default();
                part.shape = PartType::Block;
                let mut base_part = BasePart::default();
                base_part.size = Vec3::new(2.0, 0.6, 2.0);
                base_part.cframe = Transform::from_translation(event.position);
                base_part.color = Color::srgb(0.2, 0.2, 0.3);
                let entity = spawn_part(&mut commands, &mut meshes, &mut materials, instance, base_part, part);
                commands.entity(entity).insert(VehicleSeat::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
                spawned_entity = Some(entity);
            }
            ClassName::Model => {
                let entity = spawn_model(&mut commands, instance, Model::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::Folder => {
                let entity = spawn_folder(&mut commands, instance);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::PointLight => {
                let transform = Transform::from_translation(event.position);
                let entity = spawn_point_light(&mut commands, instance, EustressPointLight::default(), transform);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::SpotLight => {
                let transform = Transform::from_translation(event.position);
                let entity = spawn_spot_light(&mut commands, instance, EustressSpotLight::default(), transform);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::SurfaceLight => {
                let entity = spawn_surface_light(&mut commands, instance, SurfaceLight::default(), Entity::PLACEHOLDER);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::Sound => {
                // Sound needs asset server - simplified for now
                let name = instance.name.clone();
                let entity = commands.spawn((
                    Transform::from_translation(event.position),
                    Visibility::default(),
                    instance,
                    Sound::default(),
                    Name::new(name),
                )).id();
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::ParticleEmitter => {
                let entity = spawn_particle_emitter(&mut commands, instance, ParticleEmitter::default(), Entity::PLACEHOLDER);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::Beam => {
                let entity = spawn_beam(&mut commands, instance, Beam::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::Attachment => {
                let entity = spawn_attachment(&mut commands, instance, Attachment::default(), Entity::PLACEHOLDER);
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::WeldConstraint => {
                let entity = spawn_weld_constraint(&mut commands, instance, WeldConstraint::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::Motor6D => {
                let entity = spawn_motor6d(&mut commands, instance, Motor6D::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::BillboardGui => {
                // BillboardGui requires a Part or Attachment parent (3D object for 2D projection)
                // It does NOT work with StarterGui or ScreenGui
                let valid_parent = if let Some(parent) = event.parent {
                    // Check if parent is a valid 3D type (Part, MeshPart, or Attachment)
                    if let Ok(parent_instance) = instance_query.get(parent) {
                        matches!(parent_instance.class_name, 
                            ClassName::Part | ClassName::MeshPart | ClassName::SpawnLocation |
                            ClassName::Seat | ClassName::VehicleSeat | ClassName::Attachment |
                            ClassName::UnionOperation)
                    } else {
                        false
                    }
                } else {
                    false
                };
                
                if valid_parent {
                    let entity = spawn_billboard_gui(&mut commands, instance, BillboardGui::default());
                    if let Some(parent) = event.parent {
                        commands.entity(entity).insert(ChildOf(parent));
                    }
                    spawned_entity = Some(entity);
                } else {
                    // Show error notification - BillboardGui needs a Part or Attachment parent
                    notifications.error("BillboardGui requires a Part or Attachment parent (3D object)");
                }
            }
            ClassName::TextLabel => {
                let entity = spawn_text_label(&mut commands, instance, TextLabel::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::SpecialMesh => {
                spawn_special_mesh(&mut commands, instance, SpecialMesh::default(), Entity::PLACEHOLDER);
            }
            ClassName::Humanoid => {
                spawn_humanoid(&mut commands, instance, Humanoid::default(), Entity::PLACEHOLDER);
            }
            ClassName::Sky => {
                spawn_sky(&mut commands, instance, Sky::default());
            }
            ClassName::Atmosphere => {
                spawn_atmosphere(&mut commands, instance, Atmosphere::default());
            }
            ClassName::SpawnLocation => {
                // SpawnLocation is a Part with special properties
                // Size: 2m x 0.2m x 2m (flat platform for spawning)
                let mut part = Part::default();
                part.shape = PartType::Block;
                let mut base_part = BasePart::default();
                base_part.size = Vec3::new(2.0, 0.2, 2.0);
                base_part.color = Color::srgb(0.2, 0.8, 0.2);
                base_part.cframe = Transform::from_translation(event.position);
                let entity = spawn_part(&mut commands, &mut meshes, &mut materials, instance, base_part, part);
                if is_playing {
                    commands.entity(entity).insert(SpawnedDuringPlayMode);
                }
                spawned_entity = Some(entity);
            }
            ClassName::SoulScript => {
                // Soul scripts are markdown files that compile to Rust
                let name = instance.name.clone();
                commands.spawn((
                    Transform::default(),
                    Visibility::default(),
                    instance,
                    SoulScriptData::default(),
                    Name::new(name),
                ));
            }
            ClassName::ScreenGui => {
                let entity = spawn_screen_gui(&mut commands, instance, ScreenGui::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::SurfaceGui => {
                // SurfaceGui requires a Part or Attachment parent
                let valid_parent = if let Some(parent) = event.parent {
                    // Check if parent is a valid type (Part, MeshPart, or Attachment)
                    if let Ok(parent_instance) = instance_query.get(parent) {
                        matches!(parent_instance.class_name, 
                            ClassName::Part | ClassName::MeshPart | ClassName::SpawnLocation |
                            ClassName::Seat | ClassName::VehicleSeat | ClassName::Attachment |
                            ClassName::UnionOperation)
                    } else {
                        false
                    }
                } else {
                    false
                };
                
                if valid_parent {
                    let entity = spawn_surface_gui(&mut commands, instance, SurfaceGui::default());
                    if let Some(parent) = event.parent {
                        commands.entity(entity).insert(ChildOf(parent));
                    }
                    spawned_entity = Some(entity);
                } else {
                    // Show error notification - SurfaceGui needs a Part or Attachment parent
                    notifications.error("SurfaceGui requires a Part or Attachment parent");
                }
            }
            ClassName::Frame => {
                let entity = spawn_frame(&mut commands, instance, Frame::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::ScrollingFrame => {
                let entity = spawn_scrolling_frame(&mut commands, instance, ScrollingFrame::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::VideoFrame => {
                let entity = spawn_video_frame(&mut commands, instance, VideoFrame::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::DocumentFrame => {
                let entity = spawn_document_frame(&mut commands, instance, DocumentFrame::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::WebFrame => {
                let entity = spawn_web_frame(&mut commands, instance, WebFrame::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::ImageLabel => {
                let entity = spawn_image_label(&mut commands, &asset_server, instance, ImageLabel::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::TextButton => {
                let entity = spawn_text_button(&mut commands, instance, TextButton::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::ImageButton => {
                let entity = spawn_image_button(&mut commands, &asset_server, instance, ImageButton::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::TextBox => {
                let entity = spawn_text_box(&mut commands, instance, TextBox::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::ViewportFrame => {
                let entity = spawn_viewport_frame(&mut commands, instance, ViewportFrame::default());
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            ClassName::Team => {
                let name = instance.name.clone();
                let entity = commands.spawn((
                    instance,
                    Team::default(),
                    Name::new(name),
                )).id();
                if let Some(parent) = event.parent {
                    commands.entity(entity).insert(ChildOf(parent));
                }
                spawned_entity = Some(entity);
            }
            _ => {
                warn!("Insert not implemented for {:?}", event.class_name);
            }
        }
        
        // Auto-select the spawned entity
        if let Some(entity) = spawned_entity {
            let entity_id = format!("{}v{}", entity.index(), entity.generation());
            info!("Auto-selecting inserted entity: {}", entity_id);
            let sm = selection_manager.0.write();
            sm.clear();
            sm.select(entity_id);
        }
    }
}
