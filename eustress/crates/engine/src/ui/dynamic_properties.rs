// Dynamic Properties Panel - Auto-generates UI from PropertyAccess
// Phase 2, Week 1: Properties Panel Migration
// Phase 3: Added Attributes, Tags, and Parameters sections

use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashMap;

use crate::properties::{PropertyAccess, PropertyDescriptor};
use crate::classes::*;
use super::property_widgets::*;
use super::attributes_ui::{AttributesPanelState, render_attributes_panel};

/// Dynamic properties panel state
#[derive(Resource, Default)]
pub struct DynamicPropertiesPanel {
    pub selected_entity: Option<Entity>,
    pub selected_service: Option<super::explorer::ServiceType>,
    pub search_filter: String,
    pub show_advanced: bool,
    pub collapsed_categories: HashMap<String, bool>,
    pub validation_errors: HashMap<String, String>,
    /// State for Attributes/Tags/Parameters UI sections
    pub attributes_state: AttributesPanelState,
}

impl DynamicPropertiesPanel {
    /// Show the dynamic properties panel
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        command_history: &mut crate::commands::CommandHistory,
    ) {
        // Check if a service is selected (takes priority over entity)
        if let Some(service) = self.selected_service {
            super::service_properties::render_service_properties(ui, service, world);
            return;
        }
        
        ui.heading("Properties");
        
        // Search bar
        ui.horizontal(|ui| {
            ui.label("🔍");
            ui.add(
                egui::TextEdit::singleline(&mut self.search_filter)
                    .hint_text("Search properties...")
                    .desired_width(200.0)
            );
            
            if ui.button("Clear").clicked() {
                self.search_filter.clear();
            }
        });
        
        ui.separator();
        
        // Advanced toggle
        ui.checkbox(&mut self.show_advanced, "Show Advanced");
        
        ui.separator();
        
        // Get selected entity
        let Some(entity) = self.selected_entity else {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label("❓ No entity selected");
                ui.add_space(10.0);
                ui.label("Click on an object in the viewport\nor a service in the Explorer\nto view its properties");
            });
            return;
        };
        
        // Entity ID removed from display - not useful for users
        
        // Check if entity exists
        if !world.entities().contains(entity) {
            ui.label("Selected entity no longer exists");
            warn!("📋 Properties panel: Entity {:?} no longer exists", entity);
            self.selected_entity = None;
            return;
        }
        
        // Debug: Log what entity is selected
        if let Some(inst) = world.get::<Instance>(entity) {
            info!("📋 DynamicPropertiesPanel.show: Selected entity={:?}, class={:?}", entity, inst.class_name);
        } else {
            info!("📋 DynamicPropertiesPanel.show: Selected entity={:?} (no Instance component)", entity);
        }
        
        // Render properties based on components
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.render_instance_properties(ui, world, entity, command_history);
                self.render_basepart_properties(ui, world, entity, command_history);
                self.render_part_properties(ui, world, entity, command_history);
                self.render_meshpart_properties(ui, world, entity, command_history);
                self.render_model_properties(ui, world, entity, command_history);
                self.render_humanoid_properties(ui, world, entity, command_history);
                self.render_camera_properties(ui, world, entity, command_history);
                self.render_light_properties(ui, world, entity, command_history);
                self.render_sound_properties(ui, world, entity, command_history);
                self.render_constraint_properties(ui, world, entity, command_history);
                self.render_billboardgui_properties(ui, world, entity, command_history);
                self.render_textlabel_properties(ui, world, entity, command_history);
                self.render_screengui_properties(ui, world, entity, command_history);
                self.render_frame_properties(ui, world, entity, command_history);
                self.render_textbox_properties(ui, world, entity, command_history);
                self.render_textbutton_properties(ui, world, entity, command_history);
                self.render_imagebutton_properties(ui, world, entity, command_history);
                self.render_imagelabel_properties(ui, world, entity, command_history);
                self.render_scrollingframe_properties(ui, world, entity, command_history);
                self.render_surfacegui_properties(ui, world, entity, command_history);
                self.render_sky_properties(ui, world, entity, command_history);
                self.render_atmosphere_properties(ui, world, entity, command_history);
                self.render_sun_properties(ui, world, entity, command_history);
                self.render_moon_properties(ui, world, entity, command_history);
                
                // Render Attributes, Tags, and Parameters sections
                ui.separator();
                render_attributes_panel(ui, entity, world, &mut self.attributes_state);
            });
    }
    
    /// Render properties for a component that implements PropertyAccess
    fn render_component_properties<T: Component + PropertyAccess>(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        entity: Entity,
        component_name: &str,
        command_history: &mut crate::commands::CommandHistory,
    ) {
        // Get component
        let Some(component) = world.get::<T>(entity) else {
            // Debug: Log when component is not found for GUI types
            if component_name == "BillboardGui" || component_name == "TextLabel" {
                debug!("📋 Component '{}' not found on entity {:?}", component_name, entity);
            }
            return;
        };
        
        // Debug: Log when component IS found for GUI types
        if component_name == "BillboardGui" || component_name == "TextLabel" {
            info!("📋 Found component '{}' on entity {:?}, listing properties", component_name, entity);
        }
        
        // Get all properties
        let properties = component.list_properties();
        
        // Filter properties
        let filtered: Vec<_> = properties.into_iter()
            .filter(|p| self.matches_filter(&p.name))
            .collect();
        
        if filtered.is_empty() {
            return;
        }
        
        // Group by category
        let mut by_category: HashMap<String, Vec<PropertyDescriptor>> = HashMap::new();
        for prop in filtered {
            by_category.entry(prop.category.clone())
                .or_default()
                .push(prop);
        }
        
        // Render component section with unique ID
        let component_id = egui::Id::new(format!("comp_{}_{:?}", component_name, entity));
        
        egui::CollapsingHeader::new(component_name)
            .id_salt(component_id)
            .default_open(true)
            .show(ui, |ui| {
                // Sort categories for consistent display order
                let mut categories: Vec<_> = by_category.into_iter().collect();
                categories.sort_by(|a, b| a.0.cmp(&b.0));
                
                for (category, props) in categories {
                    self.render_category(ui, world, entity, &category, props, command_history);
                }
            });
    }
    
    /// Render a category of properties
    fn render_category(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        entity: Entity,
        category: &str,
        properties: Vec<PropertyDescriptor>,
        command_history: &mut crate::commands::CommandHistory,
    ) {
        let is_collapsed = self.collapsed_categories.get(category).copied().unwrap_or(false);
        
        // Use unique ID per category to avoid conflicts
        let category_id = egui::Id::new(format!("cat_{}_{:?}", category, entity));
        
        // Sort properties alphabetically for consistent display
        let mut sorted_props = properties;
        sorted_props.sort_by(|a, b| a.name.cmp(&b.name));
        
        let header = egui::CollapsingHeader::new(category)
            .id_salt(category_id)  // Explicit ID to prevent conflicts
            .default_open(!is_collapsed)
            .show(ui, |ui| {
                for descriptor in sorted_props {
                    self.render_property(ui, world, entity, &descriptor, command_history);
                }
            });
        
        self.collapsed_categories.insert(category.to_string(), !header.fully_open());
    }
    
    /// Render a single property
    fn render_property(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut World,
        entity: Entity,
        descriptor: &PropertyDescriptor,
        command_history: &mut crate::commands::CommandHistory,
    ) {
        // Get current value (need to check each component type)
        let value = self.get_property_value(world, entity, &descriptor.name);
        
        let Some(current_value) = value else {
            return;
        };
        
        // Get validation error if any
        let error = self.validation_errors.get(&descriptor.name).map(|s| s.as_str());
        
        // Render widget
        if let Some(new_value) = property_widget_with_validation(ui, descriptor, current_value.clone(), error) {
            // Create and execute command for undo/redo
            let command = crate::commands::PropertyCommand::new(
                entity,
                descriptor.name.clone(),
                current_value,
                new_value,
            );
            
            if let Err(e) = command_history.execute(crate::commands::Command::Property(command), world) {
                self.validation_errors.insert(descriptor.name.clone(), e);
            } else {
                self.validation_errors.remove(&descriptor.name);
            }
        }
        
        ui.add_space(4.0);
    }
    
    /// Get property value from entity (checks all component types)
    fn get_property_value(
        &self,
        world: &World,
        entity: Entity,
        property_name: &str,
    ) -> Option<crate::properties::PropertyValue> {
        // Check Instance
        if let Some(instance) = world.get::<Instance>(entity) {
            if let Some(val) = instance.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check BasePart
        if let Some(base_part) = world.get::<BasePart>(entity) {
            if let Some(val) = base_part.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Part
        if let Some(part) = world.get::<Part>(entity) {
            if let Some(val) = part.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check MeshPart
        if let Some(mesh_part) = world.get::<MeshPart>(entity) {
            if let Some(val) = mesh_part.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Model
        if let Some(model) = world.get::<Model>(entity) {
            if let Some(val) = model.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Humanoid
        if let Some(humanoid) = world.get::<Humanoid>(entity) {
            if let Some(val) = humanoid.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check BillboardGui
        if let Some(billboard_gui) = world.get::<BillboardGui>(entity) {
            if let Some(val) = billboard_gui.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check TextLabel
        if let Some(text_label) = world.get::<TextLabel>(entity) {
            if let Some(val) = text_label.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check ScreenGui
        if let Some(screen_gui) = world.get::<ScreenGui>(entity) {
            if let Some(val) = screen_gui.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Frame
        if let Some(frame) = world.get::<Frame>(entity) {
            if let Some(val) = frame.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check TextBox
        if let Some(text_box) = world.get::<TextBox>(entity) {
            if let Some(val) = text_box.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check TextButton
        if let Some(text_button) = world.get::<TextButton>(entity) {
            if let Some(val) = text_button.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check ImageButton
        if let Some(image_button) = world.get::<ImageButton>(entity) {
            if let Some(val) = image_button.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check ImageLabel
        if let Some(image_label) = world.get::<ImageLabel>(entity) {
            if let Some(val) = image_label.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check ScrollingFrame
        if let Some(scrolling_frame) = world.get::<ScrollingFrame>(entity) {
            if let Some(val) = scrolling_frame.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check SurfaceGui
        if let Some(surface_gui) = world.get::<SurfaceGui>(entity) {
            if let Some(val) = surface_gui.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Camera
        if let Some(camera) = world.get::<EustressCamera>(entity) {
            if let Some(val) = camera.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check PointLight
        if let Some(point_light) = world.get::<EustressPointLight>(entity) {
            if let Some(val) = point_light.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check SpotLight
        if let Some(spot_light) = world.get::<EustressSpotLight>(entity) {
            if let Some(val) = spot_light.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check SurfaceLight
        if let Some(surface_light) = world.get::<SurfaceLight>(entity) {
            if let Some(val) = surface_light.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Sound
        if let Some(sound) = world.get::<Sound>(entity) {
            if let Some(val) = sound.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check WeldConstraint
        if let Some(weld) = world.get::<WeldConstraint>(entity) {
            if let Some(val) = weld.get_property(property_name) {
                return Some(val);
            }
        }
        
        // Check Motor6D
        if let Some(motor) = world.get::<Motor6D>(entity) {
            if let Some(val) = motor.get_property(property_name) {
                return Some(val);
            }
        }
        
        None
    }
    
    /// Set property value on entity
    fn set_property_value(
        &mut self,
        world: &mut World,
        entity: Entity,
        property_name: &str,
        value: crate::properties::PropertyValue,
    ) -> Result<(), String> {
        // Try each component type
        if let Some(mut instance) = world.get_mut::<Instance>(entity) {
            if instance.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        if let Some(mut base_part) = world.get_mut::<BasePart>(entity) {
            if base_part.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        if let Some(mut part) = world.get_mut::<Part>(entity) {
            if part.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        if let Some(mut mesh_part) = world.get_mut::<MeshPart>(entity) {
            if mesh_part.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        if let Some(mut model) = world.get_mut::<Model>(entity) {
            if model.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        if let Some(mut humanoid) = world.get_mut::<Humanoid>(entity) {
            if humanoid.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // BillboardGui
        if let Some(mut billboard_gui) = world.get_mut::<BillboardGui>(entity) {
            if billboard_gui.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // TextLabel
        if let Some(mut text_label) = world.get_mut::<TextLabel>(entity) {
            if text_label.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // ScreenGui
        if let Some(mut screen_gui) = world.get_mut::<ScreenGui>(entity) {
            if screen_gui.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // Frame
        if let Some(mut frame) = world.get_mut::<Frame>(entity) {
            if frame.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // TextBox
        if let Some(mut text_box) = world.get_mut::<TextBox>(entity) {
            if text_box.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // TextButton
        if let Some(mut text_button) = world.get_mut::<TextButton>(entity) {
            if text_button.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // ImageButton
        if let Some(mut image_button) = world.get_mut::<ImageButton>(entity) {
            if image_button.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // ImageLabel
        if let Some(mut image_label) = world.get_mut::<ImageLabel>(entity) {
            if image_label.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // ScrollingFrame
        if let Some(mut scrolling_frame) = world.get_mut::<ScrollingFrame>(entity) {
            if scrolling_frame.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // SurfaceGui
        if let Some(mut surface_gui) = world.get_mut::<SurfaceGui>(entity) {
            if surface_gui.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // Camera
        if let Some(mut camera) = world.get_mut::<EustressCamera>(entity) {
            if camera.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // PointLight
        if let Some(mut point_light) = world.get_mut::<EustressPointLight>(entity) {
            if point_light.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // SpotLight
        if let Some(mut spot_light) = world.get_mut::<EustressSpotLight>(entity) {
            if spot_light.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // SurfaceLight
        if let Some(mut surface_light) = world.get_mut::<SurfaceLight>(entity) {
            if surface_light.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // Sound
        if let Some(mut sound) = world.get_mut::<Sound>(entity) {
            if sound.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // WeldConstraint
        if let Some(mut weld) = world.get_mut::<WeldConstraint>(entity) {
            if weld.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        // Motor6D
        if let Some(mut motor) = world.get_mut::<Motor6D>(entity) {
            if motor.set_property(property_name, value.clone()).is_ok() {
                return Ok(());
            }
        }
        
        Err(format!("Property '{}' not found on entity", property_name))
    }
    
    /// Check if property name matches filter
    fn matches_filter(&self, property_name: &str) -> bool {
        if self.search_filter.is_empty() {
            return true;
        }
        property_name.to_lowercase().contains(&self.search_filter.to_lowercase())
    }
    
    // Convenience methods for rendering specific components
    
    fn render_instance_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<Instance>(ui, world, entity, "Instance", command_history);
    }
    
    fn render_basepart_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<BasePart>(ui, world, entity, "BasePart", command_history);
    }
    
    fn render_part_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<Part>(ui, world, entity, "Part", command_history);
    }
    
    fn render_meshpart_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<MeshPart>(ui, world, entity, "MeshPart", command_history);
    }
    
    fn render_model_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<Model>(ui, world, entity, "Model", command_history);
    }
    
    fn render_humanoid_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<Humanoid>(ui, world, entity, "Humanoid", command_history);
    }
    
    fn render_camera_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<EustressCamera>(ui, world, entity, "Camera", command_history);
    }
    
    fn render_light_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<EustressPointLight>(ui, world, entity, "PointLight", command_history);
        self.render_component_properties::<EustressSpotLight>(ui, world, entity, "SpotLight", command_history);
        self.render_component_properties::<SurfaceLight>(ui, world, entity, "SurfaceLight", command_history);
    }
    
    fn render_sound_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<Sound>(ui, world, entity, "Sound", command_history);
    }
    
    fn render_constraint_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<WeldConstraint>(ui, world, entity, "WeldConstraint", command_history);
        self.render_component_properties::<Motor6D>(ui, world, entity, "Motor6D", command_history);
    }
    
    fn render_billboardgui_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        // Debug: Check what's on this entity
        let has_billboard = world.get::<BillboardGui>(entity).is_some();
        let has_instance = world.get::<Instance>(entity).is_some();
        
        if has_instance {
            if let Some(inst) = world.get::<Instance>(entity) {
                info!("📋 render_billboardgui_properties: entity={:?}, class={:?}, has_billboard_component={}", 
                    entity, inst.class_name, has_billboard);
            }
        } else {
            info!("📋 render_billboardgui_properties: entity={:?} has NO Instance component!", entity);
        }
        
        self.render_component_properties::<BillboardGui>(ui, world, entity, "BillboardGui", command_history);
    }
    
    fn render_textlabel_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        // Debug: Check what's on this entity
        let has_textlabel = world.get::<TextLabel>(entity).is_some();
        let has_instance = world.get::<Instance>(entity).is_some();
        
        if has_instance {
            if let Some(inst) = world.get::<Instance>(entity) {
                info!("📋 render_textlabel_properties: entity={:?}, class={:?}, has_textlabel_component={}", 
                    entity, inst.class_name, has_textlabel);
            }
        }
        
        self.render_component_properties::<TextLabel>(ui, world, entity, "TextLabel", command_history);
    }
    
    fn render_screengui_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<ScreenGui>(ui, world, entity, "ScreenGui", command_history);
    }
    
    fn render_frame_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<Frame>(ui, world, entity, "Frame", command_history);
    }
    
    fn render_textbox_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<TextBox>(ui, world, entity, "TextBox", command_history);
    }
    
    fn render_textbutton_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<TextButton>(ui, world, entity, "TextButton", command_history);
    }
    
    fn render_imagebutton_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<ImageButton>(ui, world, entity, "ImageButton", command_history);
    }
    
    fn render_imagelabel_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<ImageLabel>(ui, world, entity, "ImageLabel", command_history);
    }
    
    fn render_scrollingframe_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<ScrollingFrame>(ui, world, entity, "ScrollingFrame", command_history);
    }
    
    fn render_surfacegui_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, command_history: &mut crate::commands::CommandHistory) {
        self.render_component_properties::<SurfaceGui>(ui, world, entity, "SurfaceGui", command_history);
    }
    
    fn render_sky_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, _command_history: &mut crate::commands::CommandHistory) {
        // Sky doesn't implement PropertyAccess yet, so render manually
        let Some(sky) = world.get::<Sky>(entity) else { return };
        
        egui::CollapsingHeader::new("Sky")
            .default_open(true)
            .show(ui, |ui| {
                ui.label(format!("Star Count: {}", sky.star_count));
                ui.label(format!("Celestial Bodies: {}", if sky.celestial_bodies_shown { "Shown" } else { "Hidden" }));
                ui.separator();
                ui.label("Skybox Textures:");
                ui.indent("skybox_textures", |ui| {
                    ui.label(format!("Top: {}", if sky.skybox_textures.up.is_empty() { "(procedural)" } else { &sky.skybox_textures.up }));
                    ui.label(format!("Bottom: {}", if sky.skybox_textures.down.is_empty() { "(procedural)" } else { &sky.skybox_textures.down }));
                    ui.label(format!("Front: {}", if sky.skybox_textures.front.is_empty() { "(procedural)" } else { &sky.skybox_textures.front }));
                    ui.label(format!("Back: {}", if sky.skybox_textures.back.is_empty() { "(procedural)" } else { &sky.skybox_textures.back }));
                    ui.label(format!("Left: {}", if sky.skybox_textures.left.is_empty() { "(procedural)" } else { &sky.skybox_textures.left }));
                    ui.label(format!("Right: {}", if sky.skybox_textures.right.is_empty() { "(procedural)" } else { &sky.skybox_textures.right }));
                });
            });
    }
    
    fn render_atmosphere_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, _command_history: &mut crate::commands::CommandHistory) {
        // Atmosphere doesn't implement PropertyAccess yet, so render manually
        let Some(mut atmosphere) = world.get_mut::<Atmosphere>(entity) else { return };
        
        egui::CollapsingHeader::new("Atmosphere")
            .default_open(true)
            .show(ui, |ui| {
                // Density slider
                ui.horizontal(|ui| {
                    ui.label("Density:");
                    ui.add(egui::Slider::new(&mut atmosphere.density, 0.0..=1.0));
                });
                
                // Offset slider
                ui.horizontal(|ui| {
                    ui.label("Offset:");
                    ui.add(egui::Slider::new(&mut atmosphere.offset, -1.0..=1.0));
                });
                
                // Glare slider
                ui.horizontal(|ui| {
                    ui.label("Glare:");
                    ui.add(egui::Slider::new(&mut atmosphere.glare, 0.0..=1.0));
                });
                
                // Haze slider
                ui.horizontal(|ui| {
                    ui.label("Haze:");
                    ui.add(egui::Slider::new(&mut atmosphere.haze, 0.0..=1.0));
                });
                
                ui.separator();
                
                // Color display
                ui.label("Color:");
                let color = egui::Color32::from_rgba_unmultiplied(
                    (atmosphere.color[0] * 255.0) as u8,
                    (atmosphere.color[1] * 255.0) as u8,
                    (atmosphere.color[2] * 255.0) as u8,
                    (atmosphere.color[3] * 255.0) as u8,
                );
                let mut color_arr = [atmosphere.color[0], atmosphere.color[1], atmosphere.color[2]];
                if ui.color_edit_button_rgb(&mut color_arr).changed() {
                    atmosphere.color = [color_arr[0], color_arr[1], color_arr[2], 1.0];
                }
                
                // Decay color display
                ui.label("Decay:");
                let mut decay_arr = [atmosphere.decay[0], atmosphere.decay[1], atmosphere.decay[2]];
                if ui.color_edit_button_rgb(&mut decay_arr).changed() {
                    atmosphere.decay = [decay_arr[0], decay_arr[1], decay_arr[2], 1.0];
                }
                
                ui.separator();
                
                // Presets
                ui.label("Presets:");
                ui.horizontal(|ui| {
                    if ui.button("Clear Day").clicked() {
                        *atmosphere = Atmosphere::clear_day();
                    }
                    if ui.button("Sunset").clicked() {
                        *atmosphere = Atmosphere::sunset();
                    }
                    if ui.button("Foggy").clicked() {
                        *atmosphere = Atmosphere::foggy();
                    }
                });
            });
    }
    
    fn render_sun_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, _command_history: &mut crate::commands::CommandHistory) {
        // Sun class properties (from crate::classes::Sun)
        let Some(mut sun) = world.get_mut::<crate::classes::Sun>(entity) else { return };
        
        egui::CollapsingHeader::new("Sun")
            .default_open(true)
            .show(ui, |ui| {
                // Enabled checkbox
                ui.checkbox(&mut sun.enabled, "Enabled");
                
                // Time of day slider
                ui.horizontal(|ui| {
                    ui.label("Time of Day:");
                    ui.add(egui::Slider::new(&mut sun.time_of_day, 0.0..=24.0).suffix("h"));
                });
                
                // Cycle speed slider
                ui.horizontal(|ui| {
                    ui.label("Cycle Speed:");
                    ui.add(egui::Slider::new(&mut sun.cycle_speed, 0.0..=120.0));
                });
                
                ui.checkbox(&mut sun.cycle_paused, "Cycle Paused");
                
                ui.separator();
                
                // Angular size slider (degrees)
                ui.horizontal(|ui| {
                    ui.label("Angular Size:");
                    ui.add(egui::Slider::new(&mut sun.angular_size, 0.1..=90.0).suffix("°"));
                });
                
                // Noon intensity slider
                ui.horizontal(|ui| {
                    ui.label("Noon Intensity:");
                    ui.add(egui::Slider::new(&mut sun.noon_intensity, 0.0..=150000.0).suffix(" lux"));
                });
                
                ui.separator();
                
                // Noon color picker
                ui.horizontal(|ui| {
                    ui.label("Noon Color:");
                    let mut color_arr = [sun.noon_color[0], sun.noon_color[1], sun.noon_color[2]];
                    if ui.color_edit_button_rgb(&mut color_arr).changed() {
                        sun.noon_color = [color_arr[0], color_arr[1], color_arr[2], 1.0];
                    }
                });
                
                // Horizon color picker
                ui.horizontal(|ui| {
                    ui.label("Horizon Color:");
                    let mut color_arr = [sun.horizon_color[0], sun.horizon_color[1], sun.horizon_color[2]];
                    if ui.color_edit_button_rgb(&mut color_arr).changed() {
                        sun.horizon_color = [color_arr[0], color_arr[1], color_arr[2], 1.0];
                    }
                });
                
                ui.separator();
                
                ui.checkbox(&mut sun.cast_shadows, "Cast Shadows");
                
                ui.horizontal(|ui| {
                    ui.label("Shadow Softness:");
                    ui.add(egui::Slider::new(&mut sun.shadow_softness, 0.0..=1.0));
                });
            });
    }
    
    fn render_moon_properties(&mut self, ui: &mut egui::Ui, world: &mut World, entity: Entity, _command_history: &mut crate::commands::CommandHistory) {
        // Moon class properties (from crate::classes::Moon)
        let Some(mut moon) = world.get_mut::<crate::classes::Moon>(entity) else { return };
        
        egui::CollapsingHeader::new("Moon")
            .default_open(true)
            .show(ui, |ui| {
                // Enabled checkbox
                ui.checkbox(&mut moon.enabled, "Enabled");
                
                // Lunar day slider (0-29.53 synodic month)
                ui.horizontal(|ui| {
                    ui.label("Lunar Day:");
                    ui.add(egui::Slider::new(&mut moon.lunar_day, 0.0..=29.53));
                });
                
                ui.checkbox(&mut moon.sync_with_sun, "Sync with Sun");
                
                ui.separator();
                
                // Angular size slider (degrees)
                ui.horizontal(|ui| {
                    ui.label("Angular Size:");
                    ui.add(egui::Slider::new(&mut moon.angular_size, 0.1..=90.0).suffix("°"));
                });
                
                // Full intensity slider
                ui.horizontal(|ui| {
                    ui.label("Full Intensity:");
                    ui.add(egui::Slider::new(&mut moon.full_intensity, 0.0..=5.0).suffix(" lux"));
                });
                
                ui.separator();
                
                // Color picker
                ui.horizontal(|ui| {
                    ui.label("Color:");
                    let mut color_arr = [moon.color[0], moon.color[1], moon.color[2]];
                    if ui.color_edit_button_rgb(&mut color_arr).changed() {
                        moon.color = [color_arr[0], color_arr[1], color_arr[2], 1.0];
                    }
                });
                
                // Glow color picker
                ui.horizontal(|ui| {
                    ui.label("Glow Color:");
                    let mut color_arr = [moon.glow_color[0], moon.glow_color[1], moon.glow_color[2]];
                    if ui.color_edit_button_rgb(&mut color_arr).changed() {
                        moon.glow_color = [color_arr[0], color_arr[1], color_arr[2], moon.glow_color[3]];
                    }
                });
                
                ui.separator();
                
                ui.checkbox(&mut moon.cast_shadows, "Cast Shadows");
                
                ui.horizontal(|ui| {
                    ui.label("Glow Intensity:");
                    ui.add(egui::Slider::new(&mut moon.glow_intensity, 0.0..=1.0));
                });
            });
    }
}

/// Plugin for dynamic properties panel
pub struct DynamicPropertiesPlugin;

impl Plugin for DynamicPropertiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DynamicPropertiesPanel>();
    }
}
