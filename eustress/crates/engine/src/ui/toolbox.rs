#![allow(dead_code)]

use bevy_egui::egui::{self, Color32, Pos2, Stroke, Vec2};
use bevy::prelude::*;
use bevy::log::info;
use crate::classes::PartType as ClassPartType;

/// Toolbox state resource
#[derive(Resource)]
pub struct ToolboxState {
    pub show: bool,
    pub search_query: String,
    pub selected_item: Option<ToolboxItemType>,
}

impl Default for ToolboxState {
    fn default() -> Self {
        Self {
            show: true,
            search_query: String::new(),
            selected_item: None,
        }
    }
}

/// Icon type for toolbox entries
#[derive(Clone, Copy)]
pub enum ToolboxIcon {
    Block,
    Ball,
    Cylinder,
    Wedge,
    CornerWedge,
    Cone,
    SpawnPoint,
    PointLight,
    SpotLight,
}

/// What type of item to spawn
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolboxItemType {
    Part(ClassPartType),
    SpawnPoint,
    PointLight,
    SpotLight,
}

/// Catalog entry for the toolbox
#[derive(Clone)]
pub struct ToolboxCatalogEntry {
    pub item_type: ToolboxItemType,
    pub name: &'static str,
    pub icon: ToolboxIcon,
    pub description: &'static str,
    pub category: &'static str,
}

/// Get the catalog of available items
pub fn get_toolbox_catalog() -> Vec<ToolboxCatalogEntry> {
    vec![
        // Basic Shapes
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::Part(ClassPartType::Block),
            name: "Block",
            icon: ToolboxIcon::Block,
            description: "Basic building block - the most common part",
            category: "Basic",
        },
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::Part(ClassPartType::Ball),
            name: "Ball",
            icon: ToolboxIcon::Ball,
            description: "Round sphere - great for decorations",
            category: "Basic",
        },
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::Part(ClassPartType::Cylinder),
            name: "Cylinder",
            icon: ToolboxIcon::Cylinder,
            description: "Cylindrical shape - pillars and poles",
            category: "Basic",
        },
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::Part(ClassPartType::Wedge),
            name: "Wedge",
            icon: ToolboxIcon::Wedge,
            description: "Triangular wedge - ramps and roofs",
            category: "Basic",
        },
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::Part(ClassPartType::CornerWedge),
            name: "Corner Wedge",
            icon: ToolboxIcon::CornerWedge,
            description: "Corner wedge - roof corners",
            category: "Basic",
        },
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::Part(ClassPartType::Cone),
            name: "Cone",
            icon: ToolboxIcon::Cone,
            description: "Cone shape - decorative element",
            category: "Basic",
        },
        // Gameplay
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::SpawnPoint,
            name: "Spawn Point",
            icon: ToolboxIcon::SpawnPoint,
            description: "Player spawn location",
            category: "Gameplay",
        },
        // Lighting
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::PointLight,
            name: "Point Light",
            icon: ToolboxIcon::PointLight,
            description: "Omnidirectional light source",
            category: "Lighting",
        },
        ToolboxCatalogEntry {
            item_type: ToolboxItemType::SpotLight,
            name: "Spot Light",
            icon: ToolboxIcon::SpotLight,
            description: "Directional cone light",
            category: "Lighting",
        },
    ]
}

// Keep old function for backwards compatibility
pub fn get_part_catalog() -> Vec<ToolboxCatalogEntry> {
    get_toolbox_catalog()
}

// ============================================================================
// Toolbox Icon Drawing Functions
// ============================================================================

/// Draw a toolbox icon centered in the given rect
pub fn draw_toolbox_icon(painter: &egui::Painter, rect: egui::Rect, icon: ToolboxIcon) {
    let cx = rect.center().x;
    let cy = rect.center().y - 8.0; // Offset up to leave room for text
    let size = 28.0;
    
    match icon {
        ToolboxIcon::Block => draw_block_icon(painter, cx, cy, size),
        ToolboxIcon::Ball => draw_ball_icon(painter, cx, cy, size),
        ToolboxIcon::Cylinder => draw_cylinder_icon(painter, cx, cy, size),
        ToolboxIcon::Wedge => draw_wedge_icon(painter, cx, cy, size),
        ToolboxIcon::CornerWedge => draw_corner_wedge_icon(painter, cx, cy, size),
        ToolboxIcon::Cone => draw_cone_icon(painter, cx, cy, size),
        ToolboxIcon::SpawnPoint => draw_spawn_icon(painter, cx, cy, size),
        ToolboxIcon::PointLight => draw_pointlight_icon(painter, cx, cy, size),
        ToolboxIcon::SpotLight => draw_spotlight_icon(painter, cx, cy, size),
    }
}

fn draw_block_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let s = size * 0.4;
    let color = Color32::from_rgb(100, 149, 237); // Cornflower blue
    let dark = Color32::from_rgb(60, 90, 150);
    let light = Color32::from_rgb(140, 180, 255);
    
    // Top face
    let top = vec![
        Pos2::new(cx, cy - s * 0.6),
        Pos2::new(cx + s * 0.8, cy - s * 0.2),
        Pos2::new(cx, cy + s * 0.2),
        Pos2::new(cx - s * 0.8, cy - s * 0.2),
    ];
    painter.add(egui::Shape::convex_polygon(top, light, Stroke::new(1.0, dark)));
    
    // Left face
    let left = vec![
        Pos2::new(cx - s * 0.8, cy - s * 0.2),
        Pos2::new(cx, cy + s * 0.2),
        Pos2::new(cx, cy + s * 0.8),
        Pos2::new(cx - s * 0.8, cy + s * 0.4),
    ];
    painter.add(egui::Shape::convex_polygon(left, color, Stroke::new(1.0, dark)));
    
    // Right face
    let right = vec![
        Pos2::new(cx + s * 0.8, cy - s * 0.2),
        Pos2::new(cx + s * 0.8, cy + s * 0.4),
        Pos2::new(cx, cy + s * 0.8),
        Pos2::new(cx, cy + s * 0.2),
    ];
    painter.add(egui::Shape::convex_polygon(right, dark, Stroke::new(1.0, dark)));
}

fn draw_ball_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let r = size * 0.35;
    let color = Color32::from_rgb(231, 76, 60); // Red
    let highlight = Color32::from_rgb(255, 150, 140);
    
    // Main sphere
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    
    // Highlight
    painter.circle_filled(Pos2::new(cx - r * 0.3, cy - r * 0.3), r * 0.25, highlight);
}

fn draw_cylinder_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let s = size * 0.35;
    let color = Color32::from_rgb(46, 204, 113); // Green
    let dark = Color32::from_rgb(30, 140, 80);
    let light = Color32::from_rgb(80, 230, 150);
    
    // Body
    painter.rect_filled(
        egui::Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(s * 1.4, s * 1.2)),
        0.0, color
    );
    
    // Top ellipse
    painter.add(egui::Shape::ellipse_filled(
        Pos2::new(cx, cy - s * 0.6),
        Vec2::new(s * 0.7, s * 0.25),
        light
    ));
    painter.add(egui::Shape::ellipse_stroke(
        Pos2::new(cx, cy - s * 0.6),
        Vec2::new(s * 0.7, s * 0.25),
        Stroke::new(1.0, dark)
    ));
    
    // Bottom ellipse (partial)
    painter.add(egui::Shape::ellipse_stroke(
        Pos2::new(cx, cy + s * 0.6),
        Vec2::new(s * 0.7, s * 0.25),
        Stroke::new(1.0, dark)
    ));
    
    // Side lines
    painter.line_segment(
        [Pos2::new(cx - s * 0.7, cy - s * 0.6), Pos2::new(cx - s * 0.7, cy + s * 0.6)],
        Stroke::new(1.0, dark)
    );
    painter.line_segment(
        [Pos2::new(cx + s * 0.7, cy - s * 0.6), Pos2::new(cx + s * 0.7, cy + s * 0.6)],
        Stroke::new(1.0, dark)
    );
}

fn draw_wedge_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let s = size * 0.4;
    let color = Color32::from_rgb(155, 89, 182); // Purple
    let dark = Color32::from_rgb(100, 50, 130);
    let light = Color32::from_rgb(190, 130, 210);
    
    // Front triangle face
    let front = vec![
        Pos2::new(cx - s * 0.7, cy + s * 0.5),
        Pos2::new(cx + s * 0.7, cy + s * 0.5),
        Pos2::new(cx + s * 0.7, cy - s * 0.5),
    ];
    painter.add(egui::Shape::convex_polygon(front, color, Stroke::new(1.0, dark)));
    
    // Top slope
    let top = vec![
        Pos2::new(cx - s * 0.7, cy + s * 0.5),
        Pos2::new(cx + s * 0.7, cy - s * 0.5),
        Pos2::new(cx + s * 0.3, cy - s * 0.7),
        Pos2::new(cx - s * 1.1, cy + s * 0.3),
    ];
    painter.add(egui::Shape::convex_polygon(top, light, Stroke::new(1.0, dark)));
}

fn draw_corner_wedge_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let s = size * 0.4;
    let color = Color32::from_rgb(241, 196, 15); // Yellow
    let dark = Color32::from_rgb(180, 140, 10);
    let light = Color32::from_rgb(255, 220, 80);
    
    // Base triangle
    let base = vec![
        Pos2::new(cx - s * 0.8, cy + s * 0.5),
        Pos2::new(cx + s * 0.6, cy + s * 0.5),
        Pos2::new(cx + s * 0.6, cy - s * 0.3),
    ];
    painter.add(egui::Shape::convex_polygon(base, color, Stroke::new(1.0, dark)));
    
    // Top face
    let top = vec![
        Pos2::new(cx - s * 0.8, cy + s * 0.5),
        Pos2::new(cx + s * 0.6, cy - s * 0.3),
        Pos2::new(cx, cy - s * 0.6),
    ];
    painter.add(egui::Shape::convex_polygon(top, light, Stroke::new(1.0, dark)));
}

fn draw_cone_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let s = size * 0.4;
    let color = Color32::from_rgb(230, 126, 34); // Orange
    let dark = Color32::from_rgb(160, 80, 20);
    
    // Cone body
    let cone = vec![
        Pos2::new(cx, cy - s * 0.8),
        Pos2::new(cx + s * 0.7, cy + s * 0.5),
        Pos2::new(cx - s * 0.7, cy + s * 0.5),
    ];
    painter.add(egui::Shape::convex_polygon(cone, color, Stroke::new(1.0, dark)));
    
    // Base ellipse
    painter.add(egui::Shape::ellipse_filled(
        Pos2::new(cx, cy + s * 0.5),
        Vec2::new(s * 0.7, s * 0.2),
        dark
    ));
}

fn draw_spawn_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let color = Color32::from_rgb(52, 152, 219); // Blue
    let r = size * 0.35;
    
    // Outer ring
    painter.circle_stroke(Pos2::new(cx, cy), r, Stroke::new(2.0, color));
    // Middle ring
    painter.circle_stroke(Pos2::new(cx, cy), r * 0.6, Stroke::new(1.5, color));
    // Center dot
    painter.circle_filled(Pos2::new(cx, cy), r * 0.2, color);
    
    // Crosshairs
    painter.line_segment([Pos2::new(cx, cy - r * 1.2), Pos2::new(cx, cy - r * 0.8)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx, cy + r * 0.8), Pos2::new(cx, cy + r * 1.2)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx - r * 1.2, cy), Pos2::new(cx - r * 0.8, cy)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx + r * 0.8, cy), Pos2::new(cx + r * 1.2, cy)], Stroke::new(1.5, color));
}

fn draw_pointlight_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let color = Color32::from_rgb(255, 235, 59); // Yellow
    let r = size * 0.2;
    
    // Glow center
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    
    // Rays
    for i in 0..8 {
        let angle = (i as f32) * std::f32::consts::PI / 4.0;
        let inner = r * 1.4;
        let outer = r * 2.2;
        painter.line_segment([
            Pos2::new(cx + angle.cos() * inner, cy + angle.sin() * inner),
            Pos2::new(cx + angle.cos() * outer, cy + angle.sin() * outer),
        ], Stroke::new(2.0, color));
    }
}

fn draw_spotlight_icon(painter: &egui::Painter, cx: f32, cy: f32, size: f32) {
    let color = Color32::from_rgb(255, 235, 59); // Yellow
    let dark = Color32::from_rgb(100, 100, 100);
    let s = size * 0.4;
    
    // Housing
    painter.rect_filled(
        egui::Rect::from_center_size(Pos2::new(cx, cy - s * 0.6), Vec2::new(s * 0.8, s * 0.4)),
        2.0, dark
    );
    
    // Light cone
    let cone = vec![
        Pos2::new(cx - s * 0.3, cy - s * 0.4),
        Pos2::new(cx + s * 0.3, cy - s * 0.4),
        Pos2::new(cx + s * 0.8, cy + s * 0.7),
        Pos2::new(cx - s * 0.8, cy + s * 0.7),
    ];
    painter.add(egui::Shape::convex_polygon(
        cone,
        Color32::from_rgba_unmultiplied(255, 235, 59, 80),
        Stroke::NONE
    ));
    
    // Light source
    painter.circle_filled(Pos2::new(cx, cy - s * 0.4), s * 0.15, color);
}

/// Toolbox panel - Roblox-style part picker
pub struct ToolboxPanel;

impl ToolboxPanel {
    /// Show toolbox content for dock system (DEPRECATED - use show_content_simple instead)
    #[allow(dead_code)]
    pub fn show_content(
        ui: &mut egui::Ui,
        state: &mut ToolboxState,
        notifications: &mut crate::notifications::NotificationManager,
        world: &mut World,
    ) {
        // Search bar with proper spacing
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(egui::TextEdit::singleline(&mut state.search_query)
                .hint_text("Search parts...")
                .desired_width(ui.available_width() - 28.0));
            
            if ui.button("X").on_hover_text("Clear search").clicked() {
                state.search_query.clear();
            }
        });
        
        ui.add_space(4.0);
        
        // Part grid
        egui::ScrollArea::vertical().show(ui, |ui| {
            let catalog = get_toolbox_catalog();
            let filtered_items: Vec<_> = catalog.iter()
                .filter(|entry| {
                    if state.search_query.is_empty() {
                        true
                    } else {
                        entry.name.to_lowercase().contains(&state.search_query.to_lowercase()) ||
                        entry.description.to_lowercase().contains(&state.search_query.to_lowercase())
                    }
                })
                .collect();
            
            if filtered_items.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.weak("No parts found");
                });
            } else {
                // Fixed square button size
                let button_size = 64.0;
                let button_vec = egui::vec2(button_size, button_size);
                let spacing = 6.0;
                let available_width = ui.available_width() - 16.0;
                let cols = ((available_width + spacing) / (button_size + spacing)).floor().max(2.0) as usize;
                
                for chunk in filtered_items.chunks(cols) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = spacing;
                        for entry in chunk {
                            // Allocate button space
                            let (rect, response) = ui.allocate_exact_size(button_vec, egui::Sense::click());
                            
                            // Draw button background
                            let bg_color = if response.hovered() {
                                Color32::from_rgb(70, 70, 80)
                            } else {
                                Color32::from_rgb(50, 50, 55)
                            };
                            ui.painter().rect_filled(rect, 4.0, bg_color);
                            ui.painter().rect_stroke(rect, egui::CornerRadius::same(4), Stroke::new(1.0, Color32::from_rgb(80, 80, 90)), egui::StrokeKind::Outside);
                            
                            // Draw the vector icon centered in the upper portion
                            let icon_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(rect.width(), rect.height() - 16.0)
                            );
                            draw_toolbox_icon(ui.painter(), icon_rect, entry.icon);
                            
                            // Draw the name centered at the bottom
                            let text_pos = Pos2::new(rect.center().x, rect.max.y - 8.0);
                            ui.painter().text(
                                text_pos,
                                egui::Align2::CENTER_CENTER,
                                entry.name,
                                egui::FontId::proportional(9.0),
                                Color32::from_rgb(200, 200, 200),
                            );
                            
                            // Handle click - only spawn parts via this method
                            if response.clicked() {
                                if let ToolboxItemType::Part(part_type) = entry.item_type {
                                    world.write_message(super::SpawnPartEvent {
                                        part_type,
                                        position: Vec3::new(0.0, 0.0, 0.0),
                                    });
                                    notifications.success(format!("Added {}", entry.name));
                                }
                            }
                            
                            // Drag source for drag-and-drop
                            if response.drag_started() {
                                state.selected_item = Some(entry.item_type);
                            }
                            
                            // Handle hover tooltip (must be last as it consumes response)
                            response.on_hover_text(entry.description);
                        }
                    });
                    ui.add_space(spacing);
                }
            }
        });
        
        ui.add_space(4.0);
        ui.separator();
        
        // Instructions
        ui.horizontal(|ui| {
            ui.weak("Click to insert at origin");
        });
    }
    
    /// Show as standalone side panel (DEPRECATED - use dock system instead)
    #[allow(dead_code)]
    pub fn show_as_panel(
        _ctx: &egui::Context,
        _state: &mut ToolboxState,
        _notifications: &mut crate::notifications::NotificationManager,
    ) {
        // DEPRECATED: Now using dock system with show_content() which requires World access
        // This method is no longer functional after adding spawn events
    }
    
    /// Simplified toolbox content using action queue for spawning
    pub fn show_content_simple(
        ui: &mut egui::Ui,
        state: &mut ToolboxState,
        action_queue: &mut super::world_view::UIActionQueue,
    ) {
        // Add padding
        ui.add_space(4.0);
        
        // Search bar with proper spacing
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(egui::TextEdit::singleline(&mut state.search_query)
                .hint_text("🔍 Search...")
                .desired_width(ui.available_width() - 40.0));
            
            if ui.button("✕").on_hover_text("Clear search").clicked() {
                state.search_query.clear();
            }
        });
        
        ui.add_space(8.0);
        
        // Part grid with fixed button sizes
        egui::ScrollArea::vertical()
            .id_salt("toolbox_scroll")
            .show(ui, |ui| {
            let catalog = get_toolbox_catalog();
            let filtered_items: Vec<_> = catalog.iter()
                .filter(|entry| {
                    if state.search_query.is_empty() {
                        true
                    } else {
                        entry.name.to_lowercase().contains(&state.search_query.to_lowercase()) ||
                        entry.description.to_lowercase().contains(&state.search_query.to_lowercase())
                    }
                })
                .collect();
            
            if filtered_items.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.weak("No items found");
                });
            } else {
                // Fixed square button size (doesn't change with panel width)
                let button_size = 64.0;
                let button_vec = egui::vec2(button_size, button_size);
                let spacing = 6.0;
                
                // Calculate how many columns fit
                let available_width = ui.available_width() - 16.0; // Account for padding
                let cols = ((available_width + spacing) / (button_size + spacing)).floor().max(2.0) as usize;
                
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        for chunk in filtered_items.chunks(cols) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = spacing;
                                for entry in chunk {
                                    // Capture entry data before the closure
                                    let item_type = entry.item_type;
                                    let item_name = entry.name;
                                    let item_icon = entry.icon;
                                    let item_desc = entry.description;
                                    
                                    // Use push_id to ensure each button has a unique ID
                                    ui.push_id(item_name, |ui| {
                                        // Allocate fixed square button space
                                        let (rect, response) = ui.allocate_exact_size(button_vec, egui::Sense::click());
                                        
                                        // Draw button background
                                        let bg_color = if response.hovered() {
                                            Color32::from_rgb(70, 70, 80)
                                        } else {
                                            Color32::from_rgb(50, 50, 55)
                                        };
                                        ui.painter().rect_filled(rect, 4.0, bg_color);
                                        ui.painter().rect_stroke(rect, egui::CornerRadius::same(4), Stroke::new(1.0, Color32::from_rgb(80, 80, 90)), egui::StrokeKind::Outside);
                                        
                                        // Draw the vector icon centered in the upper portion
                                        let icon_rect = egui::Rect::from_min_size(
                                            rect.min,
                                            egui::vec2(rect.width(), rect.height() - 16.0)
                                        );
                                        draw_toolbox_icon(ui.painter(), icon_rect, item_icon);
                                        
                                        // Draw the name centered at the bottom
                                        let text_pos = Pos2::new(rect.center().x, rect.max.y - 8.0);
                                        ui.painter().text(
                                            text_pos,
                                            egui::Align2::CENTER_CENTER,
                                            item_name,
                                            egui::FontId::proportional(9.0),
                                            Color32::from_rgb(200, 200, 200),
                                        );
                                        
                                        // Handle click - spawn based on item type
                                        if response.clicked() {
                                            let spawn_pos = Vec3::new(0.0, 0.0, 0.0);
                                            info!("🔧 Toolbox: Clicked on '{}' -> {:?}", item_name, item_type);
                                            match item_type {
                                                ToolboxItemType::Part(part_type) => {
                                                    info!("🔧 Toolbox: Spawning Part with type {:?}", part_type);
                                                    action_queue.push(super::world_view::UIAction::SpawnPart {
                                                        part_type,
                                                        position: spawn_pos,
                                                    });
                                                }
                                                ToolboxItemType::SpawnPoint => {
                                                    action_queue.push(super::world_view::UIAction::SpawnSpawnPoint {
                                                        position: spawn_pos,
                                                    });
                                                }
                                                ToolboxItemType::PointLight => {
                                                    action_queue.push(super::world_view::UIAction::SpawnPointLight {
                                                        position: spawn_pos,
                                                    });
                                                }
                                                ToolboxItemType::SpotLight => {
                                                    action_queue.push(super::world_view::UIAction::SpawnSpotLight {
                                                        position: spawn_pos,
                                                    });
                                                }
                                            }
                                            state.selected_item = Some(item_type);
                                        }
                                        
                                        // Handle hover tooltip
                                        response.on_hover_text(item_desc);
                                    });
                                }
                            });
                            ui.add_space(spacing);
                        }
                    });
                });
            }
        });
        
        ui.add_space(4.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.weak("Click to insert at origin");
        });
    }
}
