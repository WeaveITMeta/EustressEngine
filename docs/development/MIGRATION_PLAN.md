# Long-term Migration Plan: PartData → Roblox Class System

## Overview

This document outlines the complete migration strategy from the current `PartData`-based system to the new Roblox-compatible class system with full property access.

---

## Current State Analysis

### Existing System (`parts.rs` + `ui/properties.rs`)

**Current Architecture:**
```
PartManager (Arc<Mutex<HashMap<u32, PartData>>>)
    ↓
PartData (struct with hardcoded fields)
    ↓
UI Properties Panel (hardcoded widgets per field)
```

**PartData Structure:**
```rust
pub struct PartData {
    pub id: u32,
    pub name: String,
    pub part_type: PartType,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 4],
    pub material: Material,
    pub anchored: bool,
    pub transparency: f32,
    pub can_collide: bool,
    pub parent: Option<u32>,
    pub locked: bool,
}
```

**Issues:**
- Flat structure (no class hierarchy)
- No dynamic property access
- Hardcoded UI rendering
- Limited to ~13 properties
- No support for Attachments, Constraints, Animation, etc.
- Manual sync between PartData and Bevy components

---

## Target State (New Class System)

### New Architecture

**Target Architecture:**
```
Instance (base class)
    ↓
BasePart (50+ properties via PropertyAccess trait)
    ↓
Part/MeshPart/Model/etc. (25 classes total)
    ↓
Dynamic UI (auto-generated from PropertyDescriptor)
```

**Component Composition:**
```rust
// Entity with multiple components
commands.spawn((
    Instance { ... },      // Identity & hierarchy
    BasePart { ... },      // Transform, appearance, physics
    Part { ... },          // Shape type
    Name::new("MyCube"),   // Bevy name
    // Optional: Attachment, WeldConstraint, ParticleEmitter, etc.
));
```

**Benefits:**
- Hierarchical class system (like Roblox)
- Dynamic property access (get/set/list)
- Auto-generated UI from metadata
- Support for 25+ class types
- Direct Bevy ECS integration
- Extensible for future classes

---

## Migration Strategy: 3-Phase Approach

### Phase 1: Compatibility Layer (Weeks 1-2)

**Goal:** Make both systems coexist without breaking existing functionality.

**Steps:**

1. **Create PartData → Class Converter**
```rust
// src/compatibility.rs
pub fn part_data_to_components(data: &PartData) -> (Instance, BasePart, Part) {
    let instance = Instance {
        name: data.name.clone(),
        class_name: ClassName::Part,
        archivable: true,
        id: data.id,
    };
    
    let base_part = BasePart {
        cframe: Transform {
            translation: Vec3::from_slice(&data.position),
            rotation: Quat::from_euler(
                EulerRot::XYZ,
                data.rotation[0].to_radians(),
                data.rotation[1].to_radians(),
                data.rotation[2].to_radians(),
            ),
            scale: Vec3::ONE,
        },
        size: Vec3::from_slice(&data.size),
        color: Color::srgba(data.color[0], data.color[1], data.color[2], data.color[3]),
        material: data.material,
        anchored: data.anchored,
        transparency: data.transparency,
        can_collide: data.can_collide,
        locked: data.locked,
        ..default()
    };
    
    let part = Part {
        shape: match data.part_type {
            PartType::Cube => PartType::Block,
            PartType::Sphere => PartType::Ball,
            // ... other mappings
        },
    };
    
    (instance, base_part, part)
}

pub fn components_to_part_data(
    instance: &Instance,
    base_part: &BasePart,
    part: &Part,
) -> PartData {
    PartData {
        id: instance.id,
        name: instance.name.clone(),
        part_type: match part.shape {
            PartType::Block => PartType::Cube,
            PartType::Ball => PartType::Sphere,
            // ... reverse mappings
        },
        position: base_part.cframe.translation.to_array(),
        rotation: {
            let (x, y, z) = base_part.cframe.rotation.to_euler(EulerRot::XYZ);
            [x.to_degrees(), y.to_degrees(), z.to_degrees()]
        },
        size: base_part.size.to_array(),
        color: [
            base_part.color.r(),
            base_part.color.g(),
            base_part.color.b(),
            base_part.color.a(),
        ],
        material: base_part.material,
        anchored: base_part.anchored,
        transparency: base_part.transparency,
        can_collide: base_part.can_collide,
        parent: None, // Extracted from Bevy Parent component
        locked: base_part.locked,
    }
}
```

2. **Dual-Mode Rendering System**
```rust
// src/rendering.rs - Updated
fn spawn_new_parts(
    mut commands: Commands,
    part_manager: Res<BevyPartManager>,
    mut part_entities: ResMut<PartEntities>,
    existing_parts: Query<&PartEntity>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    use_new_system: Res<MigrationConfig>,  // NEW: Feature flag
) {
    let parts = match part_manager.0.read().list_parts() {
        Ok(parts) => parts,
        Err(e) => return,
    };
    
    for part in parts {
        if use_new_system.enabled {
            // NEW SYSTEM: Use class components
            let (instance, base_part, part_comp) = part_data_to_components(&part);
            spawn_part(&mut commands, &mut meshes, &mut materials, 
                       instance, base_part, part_comp);
        } else {
            // OLD SYSTEM: Current PartData approach
            spawn_part_legacy(&mut commands, &mut meshes, &mut materials, &part);
        }
    }
}
```

3. **Add Migration Config Resource**
```rust
// src/migration.rs
#[derive(Resource)]
pub struct MigrationConfig {
    pub enabled: bool,
    pub migrate_on_save: bool,
    pub preserve_legacy: bool,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            enabled: false,          // Start with old system
            migrate_on_save: true,   // Convert when saving
            preserve_legacy: false,  // Don't keep old data
        }
    }
}
```

**Deliverables Phase 1:**
- ✅ Converter functions (PartData ↔ Components)
- ✅ Dual-mode rendering system
- ✅ Feature flag for gradual rollout
- ✅ No breaking changes to existing code
- ✅ All tests still pass

---

### Phase 2: Gradual Feature Adoption (Weeks 3-6)

**Goal:** Incrementally migrate features to new system while maintaining stability.

#### Week 3: Properties Panel Migration

**Update UI to use PropertyAccess trait:**

```rust
// src/ui/properties.rs - Enhanced
impl PropertiesPanel {
    pub fn show_content(
        ui: &mut egui::Ui,
        part_manager: &BevyPartManager,
        selection_manager: &BevySelectionManager,
        class_query: Query<(Entity, &Instance, &BasePart, Option<&Part>)>,  // NEW
        migration_config: Res<MigrationConfig>,  // NEW
    ) {
        let selected = selection_manager.0.read().get_selected();
        
        if let Some(id_str) = selected.first() {
            if let Ok(part_id) = id_str.parse::<u32>() {
                if migration_config.enabled {
                    // NEW SYSTEM: Use PropertyAccess
                    Self::show_properties_new_system(ui, class_query, part_id);
                } else {
                    // OLD SYSTEM: Hardcoded fields
                    Self::show_properties_legacy(ui, part_manager, part_id);
                }
            }
        }
    }
    
    fn show_properties_new_system(
        ui: &mut egui::Ui,
        class_query: Query<(Entity, &Instance, &BasePart, Option<&Part>)>,
        part_id: u32,
    ) {
        // Find entity by ID
        for (entity, instance, base_part, part) in &class_query {
            if instance.id == part_id {
                // Render Instance properties
                Self::render_class_properties(ui, "Instance", instance);
                
                // Render BasePart properties
                Self::render_class_properties(ui, "BasePart", base_part);
                
                // Render Part properties if present
                if let Some(part) = part {
                    Self::render_class_properties(ui, "Part", part);
                }
                
                break;
            }
        }
    }
    
    fn render_class_properties<T: PropertyAccess>(
        ui: &mut egui::Ui,
        category: &str,
        component: &T,
    ) {
        ui.collapsing(category, |ui| {
            let properties = component.list_properties();
            
            // Group by category
            let mut categories: HashMap<String, Vec<PropertyDescriptor>> = HashMap::new();
            for prop in properties {
                categories.entry(prop.category.clone()).or_default().push(prop);
            }
            
            // Render each category
            for (cat, props) in categories {
                ui.label(format!("== {} ==", cat));
                for prop in props {
                    Self::render_property_widget(ui, component, &prop);
                }
                ui.separator();
            }
        });
    }
    
    fn render_property_widget<T: PropertyAccess>(
        ui: &mut egui::Ui,
        component: &mut T,
        descriptor: &PropertyDescriptor,
    ) {
        if descriptor.read_only {
            // Display-only
            if let Some(value) = component.get_property(&descriptor.name) {
                ui.horizontal(|ui| {
                    ui.label(&descriptor.name);
                    ui.label(format!("{:?}", value));
                });
            }
        } else {
            // Editable widget based on type
            match descriptor.property_type.as_str() {
                "Vector3" => {
                    if let Some(PropertyValue::Vector3(mut v)) = component.get_property(&descriptor.name) {
                        ui.horizontal(|ui| {
                            ui.label(&descriptor.name);
                            let changed = ui.add(egui::DragValue::new(&mut v.x).prefix("X:")).changed()
                                | ui.add(egui::DragValue::new(&mut v.y).prefix("Y:")).changed()
                                | ui.add(egui::DragValue::new(&mut v.z).prefix("Z:")).changed();
                            
                            if changed {
                                let _ = component.set_property(&descriptor.name, PropertyValue::Vector3(v));
                            }
                        });
                    }
                }
                "Color" => {
                    if let Some(PropertyValue::Color(c)) = component.get_property(&descriptor.name) {
                        let mut color32 = egui::Color32::from_rgba_premultiplied(
                            (c.r() * 255.0) as u8,
                            (c.g() * 255.0) as u8,
                            (c.b() * 255.0) as u8,
                            (c.a() * 255.0) as u8,
                        );
                        
                        ui.horizontal(|ui| {
                            ui.label(&descriptor.name);
                            if ui.color_edit_button_srgba(&mut color32).changed() {
                                let new_color = Color::rgba_u8(
                                    color32.r(), color32.g(), color32.b(), color32.a()
                                );
                                let _ = component.set_property(&descriptor.name, PropertyValue::Color(new_color));
                            }
                        });
                    }
                }
                "bool" => {
                    if let Some(PropertyValue::Bool(mut b)) = component.get_property(&descriptor.name) {
                        if ui.checkbox(&mut b, &descriptor.name).changed() {
                            let _ = component.set_property(&descriptor.name, PropertyValue::Bool(b));
                        }
                    }
                }
                "float" => {
                    if let Some(PropertyValue::Float(mut f)) = component.get_property(&descriptor.name) {
                        ui.horizontal(|ui| {
                            ui.label(&descriptor.name);
                            if ui.add(egui::DragValue::new(&mut f).speed(0.1)).changed() {
                                let _ = component.set_property(&descriptor.name, PropertyValue::Float(f));
                            }
                        });
                    }
                }
                "string" => {
                    if let Some(PropertyValue::String(mut s)) = component.get_property(&descriptor.name) {
                        ui.horizontal(|ui| {
                            ui.label(&descriptor.name);
                            if ui.text_edit_singleline(&mut s).changed() {
                                let _ = component.set_property(&descriptor.name, PropertyValue::String(s));
                            }
                        });
                    }
                }
                "Enum" => {
                    if let Some(PropertyValue::Enum(current)) = component.get_property(&descriptor.name) {
                        ui.horizontal(|ui| {
                            ui.label(&descriptor.name);
                            // ComboBox for enum selection
                            // Would need enum variants - could enhance PropertyDescriptor
                            ui.label(&current);
                        });
                    }
                }
                _ => {
                    ui.label(format!("{}: (unsupported type {})", descriptor.name, descriptor.property_type));
                }
            }
        }
    }
}
```

#### Week 4: Explorer Panel Migration

**Update Explorer to show class hierarchy:**

```rust
// src/ui/explorer.rs - Enhanced
impl ExplorerPanel {
    fn render_hierarchy(
        ui: &mut egui::Ui,
        class_query: &Query<(Entity, &Instance, Option<&Children>)>,
        parent_id: Option<u32>,
        selected: &HashSet<String>,
        expanded: &mut ExplorerExpanded,
    ) {
        for (entity, instance, children) in class_query {
            // Filter by parent
            let matches_parent = match (parent_id, get_parent_id(entity)) {
                (None, None) => true,  // Root items
                (Some(p1), Some(p2)) if p1 == p2 => true,
                _ => false,
            };
            
            if !matches_parent {
                continue;
            }
            
            let has_children = children.is_some();
            let is_selected = selected.contains(&instance.id.to_string());
            
            ui.horizontal(|ui| {
                // Indent based on depth
                ui.add_space(depth_level * 16.0);
                
                // Expand/collapse for containers
                if has_children {
                    if ui.button(if expanded.is_expanded(instance.id) { "▼" } else { "▶" }).clicked() {
                        expanded.toggle(instance.id);
                    }
                }
                
                // Class icon
                let icon = match instance.class_name {
                    ClassName::Part => "📦",
                    ClassName::Model => "📁",
                    ClassName::Folder => "🗂️",
                    ClassName::Attachment => "📍",
                    _ => "⚙️",
                };
                ui.label(icon);
                
                // Name (with class type)
                let label = if is_selected {
                    format!("{} ({})", instance.name, instance.class_name.as_str())
                } else {
                    instance.name.clone()
                };
                
                if ui.selectable_label(is_selected, label).clicked() {
                    // Handle selection
                }
            });
            
            // Recurse for children
            if has_children && expanded.is_expanded(instance.id) {
                Self::render_hierarchy(ui, class_query, Some(instance.id), selected, expanded);
            }
        }
    }
}
```

#### Week 5: Serialization Migration

**Implement new save/load format:**

```rust
// src/serialization.rs
#[derive(Serialize, Deserialize)]
pub struct SceneFile {
    pub version: u32,  // 2 = new class system
    pub instances: Vec<SavedInstance>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedInstance {
    pub id: u32,
    pub class_name: ClassName,
    pub instance_data: Instance,
    pub base_part_data: Option<BasePart>,
    pub part_data: Option<Part>,
    pub model_data: Option<Model>,
    // ... other class components
}

pub fn save_scene(path: &Path, query: Query<(Entity, &Instance, Option<&BasePart>, Option<&Part>)>) -> Result<()> {
    let mut instances = Vec::new();
    
    for (entity, instance, base_part, part) in &query {
        instances.push(SavedInstance {
            id: instance.id,
            class_name: instance.class_name.clone(),
            instance_data: instance.clone(),
            base_part_data: base_part.cloned(),
            part_data: part.cloned(),
            model_data: None,  // Query separately
        });
    }
    
    let scene = SceneFile {
        version: 2,
        instances,
    };
    
    let json = serde_json::to_string_pretty(&scene)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_scene(path: &Path) -> Result<SceneFile> {
    let json = std::fs::read_to_string(path)?;
    let scene: SceneFile = serde_json::from_str(&json)?;
    
    if scene.version == 1 {
        // Old format - convert
        convert_legacy_scene(scene)
    } else {
        Ok(scene)
    }
}
```

#### Week 6: Command System Migration

**Update commands to use new classes:**

```rust
// src/commands.rs - Updated
impl TransformManager {
    pub fn move_part(&self, part_id: u32, delta: Vec3) -> Result<()> {
        // Query for BasePart component instead of PartData
        // Update via PropertyAccess
        Ok(())
    }
}
```

**Deliverables Phase 2:**
- ✅ Dynamic UI from PropertyAccess
- ✅ Explorer shows class hierarchy
- ✅ New serialization format (backward compatible)
- ✅ Commands use new system
- ✅ Feature flag can be toggled without breaking

---

### Phase 3: Full Cutover & Cleanup (Weeks 7-8)

**Goal:** Remove legacy code, finalize migration, optimize.

#### Week 7: Deprecation & Cleanup

1. **Mark Legacy Code as Deprecated**
```rust
#[deprecated(note = "Use Instance + BasePart + Part components instead")]
pub struct PartData { ... }

#[deprecated(note = "Use spawn_part() from classes.rs")]
pub fn spawn_part_legacy(...) { ... }
```

2. **Remove PartManager HashMap**
```rust
// Delete: Arc<Mutex<HashMap<u32, PartData>>>
// Replace with: ECS queries directly
```

3. **Consolidate Rendering**
```rust
// Remove dual-mode system
// Keep only new class-based spawning
fn spawn_new_parts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    scene: Res<LoadedScene>,  // New scene format
) {
    for saved_instance in &scene.instances {
        match saved_instance.class_name {
            ClassName::Part => {
                spawn_part(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    saved_instance.instance_data.clone(),
                    saved_instance.base_part_data.unwrap(),
                    saved_instance.part_data.unwrap(),
                );
            }
            ClassName::Model => {
                spawn_model(
                    &mut commands,
                    saved_instance.instance_data.clone(),
                    saved_instance.model_data.unwrap(),
                );
            }
            // ... other class types
            _ => {}
        }
    }
}
```

4. **Update All Systems**
```rust
// Old: Query PartManager
fn old_system(part_manager: Res<BevyPartManager>) {
    let parts = part_manager.0.read().list_parts();
    // ...
}

// New: Query ECS components
fn new_system(query: Query<(Entity, &Instance, &BasePart, &Part)>) {
    for (entity, instance, base_part, part) in &query {
        // Direct access to components
    }
}
```

#### Week 8: Performance Optimization

1. **Component Storage Optimization**
```rust
// Use Bevy's component storage efficiently
#[derive(Component)]
#[component(storage = "SparseSet")]  // For frequently added/removed
pub struct Attachment { ... }

#[derive(Component)]
#[component(storage = "Table")]  // For stable components
pub struct BasePart { ... }
```

2. **Query Caching**
```rust
// Cache common queries as resources
#[derive(Resource)]
pub struct CachedQueries {
    pub parts: Vec<Entity>,
    pub models: Vec<Entity>,
    pub selected: Vec<Entity>,
}
```

3. **Batch Operations**
```rust
// Batch property updates
fn batch_update_colors(
    mut query: Query<&mut BasePart>,
    updates: Vec<(u32, Color)>,
) {
    for (id, color) in updates {
        // Apply all at once
    }
}
```

**Deliverables Phase 3:**
- ✅ Legacy code removed
- ✅ Single rendering path
- ✅ Optimized component storage
- ✅ Full test coverage
- ✅ Documentation updated
- ✅ Migration guide for users

---

## Testing Strategy

### Regression Testing

**Test Suite Coverage:**
```rust
#[cfg(test)]
mod migration_tests {
    #[test]
    fn test_part_data_conversion() {
        let old_data = PartData { ... };
        let (instance, base_part, part) = part_data_to_components(&old_data);
        let converted_back = components_to_part_data(&instance, &base_part, &part);
        assert_eq!(old_data, converted_back);
    }
    
    #[test]
    fn test_property_access() {
        let mut base_part = BasePart::default();
        
        // Test get
        let pos = base_part.get_property("Position").unwrap();
        
        // Test set
        base_part.set_property("Position", PropertyValue::Vector3(Vec3::new(1.0, 2.0, 3.0))).unwrap();
        
        // Test validation
        assert!(base_part.set_property("Size", PropertyValue::Vector3(Vec3::ZERO)).is_err());
    }
    
    #[test]
    fn test_ui_rendering() {
        // Test that UI can render all property types
    }
}
```

### Integration Testing

**End-to-End Scenarios:**
1. Create part → Save → Load → Verify
2. Edit properties → Undo → Redo → Verify
3. Multi-select → Batch edit → Verify
4. Hierarchy operations → Parent/unparent → Verify

---

## Rollback Plan

**If Migration Fails:**

1. **Immediate Rollback**
```rust
// Set feature flag to false
migration_config.enabled = false;
```

2. **Data Recovery**
```rust
// Convert new format back to legacy
fn emergency_convert_to_legacy(new_scene: SceneFile) -> Vec<PartData> {
    new_scene.instances.into_iter()
        .filter_map(|inst| {
            if let (Some(bp), Some(p)) = (inst.base_part_data, inst.part_data) {
                Some(components_to_part_data(&inst.instance_data, &bp, &p))
            } else {
                None
            }
        })
        .collect()
}
```

3. **Restore Snapshot**
```rust
// Auto-backup before migration
fn backup_before_migration() {
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    std::fs::copy("scene.json", format!("scene_backup_{}.json", timestamp));
}
```

---

## Success Metrics

**Migration Complete When:**
- ✅ All 25 classes functional
- ✅ PropertyAccess works for all classes
- ✅ UI auto-generates from descriptors
- ✅ Legacy code removed
- ✅ Performance equal or better than before
- ✅ Test coverage >90%
- ✅ No user-reported regressions for 2 weeks
- ✅ Documentation complete

**Performance Targets:**
- Frame time: <16ms (60 FPS)
- Property updates: <1ms
- UI rendering: <5ms
- Scene load time: <2s for 1000 parts

---

## Timeline Summary

| Phase | Duration | Key Deliverable |
|-------|----------|-----------------|
| Phase 1 | 2 weeks | Compatibility layer |
| Phase 2 | 4 weeks | Feature migration |
| Phase 3 | 2 weeks | Cleanup & optimization |
| **Total** | **8 weeks** | Full migration complete |

---

## Risk Mitigation

**High Risks:**
1. **Data Loss** → Auto-backups before every migration
2. **Performance Regression** → Continuous profiling
3. **UI Breakage** → Extensive UI testing
4. **User Confusion** → In-app migration guide

**Medium Risks:**
1. **Third-party Plugin Breakage** → Deprecation warnings
2. **Save Format Incompatibility** → Version detection + conversion
3. **Memory Leaks** → Valgrind/sanitizers in CI

---

## Post-Migration Benefits

**Developer Experience:**
- 🎯 Add new classes in ~50 lines of code
- 🎯 UI auto-generates from PropertyDescriptor
- 🎯 No manual sync between data structures
- 🎯 Type-safe property access

**User Experience:**
- 🎯 All 25 Roblox classes available
- 🎯 Professional property panel
- 🎯 Faster performance (ECS queries)
- 🎯 Better undo/redo integration

**Codebase Health:**
- 🎯 Reduced coupling (ECS vs HashMap)
- 🎯 Better testability
- 🎯 Easier to extend
- 🎯 Fewer bugs (type safety)

---

## Conclusion

The migration from `PartData` to the Roblox class system is a **high-value, medium-risk** initiative that will:
- Unlock 25 Roblox-compatible classes
- Enable dynamic UI generation
- Improve performance and maintainability
- Position Eustress Engine for future growth

**Recommended Approach:** Gradual 3-phase migration over 8 weeks with feature flags and continuous testing.

**Next Steps:**
1. Review and approve migration plan
2. Create Phase 1 tasks in project tracker
3. Set up auto-backup system
4. Begin compatibility layer implementation
