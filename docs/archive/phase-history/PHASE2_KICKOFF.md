# Phase 2 Kickoff: Feature Adoption

**Status:** Ready to Begin  
**Duration:** 4 weeks  
**Prerequisites:** ✅ Phase 1 Complete

---

## 🎯 Phase 2 Goals

**Objective:** Gradually migrate UI and core features to use the new class system while maintaining full backward compatibility.

**Success Criteria:**
- ✅ Properties Panel uses PropertyAccess
- ✅ Explorer shows class hierarchy
- ✅ New save format with versioning
- ✅ Commands use class components
- ✅ No regression in functionality
- ✅ Performance equal or better

---

## 📅 4-Week Sprint Plan

### **Week 1: Properties Panel Migration**

**Goal:** Dynamic property UI generation from PropertyAccess

#### **Tasks:**
1. **Property Widget System** (2 days)
   - Create widget factories for each PropertyValue type
   - String → TextEdit
   - Float → DragValue/Slider
   - Int → DragValue
   - Bool → Checkbox
   - Vector3 → 3x DragValue
   - Color → ColorPicker
   - Transform → Matrix editor
   - Enum → ComboBox

2. **Category Organization** (1 day)
   - Group properties by category
   - Collapsible sections
   - Search/filter box
   - "Advanced" toggle

3. **Dynamic Panel** (2 days)
   - Query selected entity
   - Check for class components
   - Generate UI from list_properties()
   - Handle set_property() on edit
   - Validation feedback (red border on error)

**Deliverables:**
```rust
// src/ui/dynamic_properties.rs
pub struct DynamicPropertiesPanel;
impl DynamicPropertiesPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, entity: Entity, world: &mut World);
}
```

**Testing:**
- Select entity with BasePart
- Properties panel shows ~50 properties
- Edit Size → Updates in viewport
- Edit Color → Updates in viewport
- Validation errors show messages

---

### **Week 2: Explorer Panel Enhancement**

**Goal:** Display class hierarchy and support class-based filtering

#### **Tasks:**
1. **Class Icons** (1 day)
   - Icon mapping for each ClassName
   - Use egui emoji or custom icons
   - Color-code by category

2. **Hierarchy Display** (2 days)
   - Show Instance.ClassName next to name
   - Indent based on parent-child
   - Filter by class type
   - Multi-select same class

3. **Context Menus** (2 days)
   - Right-click entity
   - Show class-specific actions
   - "Add Attachment" for BasePart
   - "Add Constraint" between parts
   - "Add Effect" (Sound, ParticleEmitter)

**Deliverables:**
```rust
// src/ui/class_explorer.rs
pub struct ClassExplorerPanel;
impl ClassExplorerPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, world: &World);
    fn show_entity_with_class(&mut self, ui: &mut egui::Ui, entity: Entity, instance: &Instance);
    fn class_icon(class_name: ClassName) -> &'static str;
}
```

**Testing:**
- Explorer shows "Part", "Model", "Humanoid" icons
- Filter to only "Part" entities
- Right-click Part → See "Add Attachment"
- Multi-select 3 Parts → Batch edit enabled

---

### **Week 3: New Save Format**

**Goal:** JSON-based save/load using PropertyAccess

#### **Tasks:**
1. **Serialization Schema** (1 day)
   ```json
   {
     "version": 2,
     "format": "roblox_classes",
     "entities": [
       {
         "id": 1,
         "class": "Part",
         "properties": {
           "Name": "RedCube",
           "Size": [4.0, 1.0, 2.0],
           "Color": [1.0, 0.0, 0.0],
           "Material": "SmoothPlastic"
         },
         "children": []
       }
     ]
   }
   ```

2. **Serializer** (2 days)
   - Traverse entity hierarchy
   - Query Instance + class components
   - Call list_properties() + get_property()
   - Build JSON structure
   - Include parent-child relationships

3. **Deserializer** (2 days)
   - Parse JSON
   - Spawn entities by class
   - Set properties via set_property()
   - Restore hierarchy
   - Handle missing properties gracefully

**Deliverables:**
```rust
// src/serialization.rs
pub fn save_scene_v2(world: &World, path: &Path) -> Result<(), String>;
pub fn load_scene_v2(world: &mut World, path: &Path) -> Result<(), String>;
pub fn detect_version(path: &Path) -> u32; // 1=legacy, 2=new
```

**Testing:**
- Create scene with 10 parts, 2 models, 1 light
- Save as JSON (v2)
- Clear scene
- Load JSON → Scene restored perfectly
- Load old .ron file → Still works (v1)

---

### **Week 4: Command System Migration**

**Goal:** Commands operate on class components

#### **Tasks:**
1. **Component-Based Commands** (2 days)
   - CreatePartCommand uses spawn_part()
   - DeleteCommand removes class components
   - TransformCommand edits BasePart.cframe
   - PropertyCommand uses set_property()

2. **Undo/Redo Enhancement** (1 day)
   - Store property values as PropertyValue
   - Undo → restore via set_property()
   - Works with any property
   - Batch undo for multi-select

3. **Integration** (2 days)
   - Update existing commands
   - Test with legacy and new system
   - Ensure F9 toggle doesn't break undo
   - Performance profiling

**Deliverables:**
```rust
// src/commands/property_command.rs
pub struct PropertyCommand {
    entity: Entity,
    property: String,
    old_value: PropertyValue,
    new_value: PropertyValue,
}
impl Command for PropertyCommand {
    fn execute(&mut self, world: &mut World);
    fn undo(&mut self, world: &mut World);
}
```

**Testing:**
- Create part
- Edit Size via Properties panel
- Undo → Size restored
- Redo → Size changed again
- Multi-select 5 parts → Batch color change → Undo all

---

## 🔧 Technical Implementation

### **Week 1: Property Widgets**

```rust
// src/ui/property_widgets.rs

pub fn property_widget(
    ui: &mut egui::Ui,
    descriptor: &PropertyDescriptor,
    value: PropertyValue,
) -> Option<PropertyValue> {
    match value {
        PropertyValue::String(s) => {
            let mut text = s;
            if ui.text_edit_singleline(&mut text).changed() {
                Some(PropertyValue::String(text))
            } else {
                None
            }
        }
        PropertyValue::Float(f) => {
            let mut val = f;
            if ui.add(egui::DragValue::new(&mut val).speed(0.1)).changed() {
                Some(PropertyValue::Float(val))
            } else {
                None
            }
        }
        PropertyValue::Color(color) => {
            let mut rgb = [color.r(), color.g(), color.b()];
            if ui.color_edit_button_rgb(&mut rgb).changed() {
                Some(PropertyValue::Color(Color::srgb(rgb[0], rgb[1], rgb[2])))
            } else {
                None
            }
        }
        PropertyValue::Vector3(v) => {
            let mut changed = false;
            let mut vec = v;
            ui.horizontal(|ui| {
                changed |= ui.add(egui::DragValue::new(&mut vec.x).prefix("X: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut vec.y).prefix("Y: ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut vec.z).prefix("Z: ")).changed();
            });
            if changed {
                Some(PropertyValue::Vector3(vec))
            } else {
                None
            }
        }
        PropertyValue::Bool(b) => {
            let mut val = b;
            if ui.checkbox(&mut val, "").changed() {
                Some(PropertyValue::Bool(val))
            } else {
                None
            }
        }
        PropertyValue::Enum(e) => {
            // Dropdown for enum values
            // Would need enum variants list
            None // Placeholder
        }
        _ => None,
    }
}
```

### **Week 2: Class Icons**

```rust
// src/ui/class_icons.rs

pub fn class_icon(class_name: ClassName) -> &'static str {
    match class_name {
        ClassName::Part => "🟦",
        ClassName::MeshPart => "🔷",
        ClassName::Model => "📦",
        ClassName::Humanoid => "🚶",
        ClassName::Camera => "📷",
        ClassName::PointLight => "💡",
        ClassName::SpotLight => "🔦",
        ClassName::SurfaceLight => "🔅",
        ClassName::Attachment => "📍",
        ClassName::WeldConstraint => "🔗",
        ClassName::Motor6D => "⚙️",
        ClassName::Sound => "🔊",
        ClassName::ParticleEmitter => "✨",
        ClassName::Beam => "➖",
        ClassName::Folder => "📁",
        ClassName::Terrain => "🏔️",
        ClassName::Sky => "☁️",
        _ => "📄",
    }
}

pub fn class_color(class_name: ClassName) -> Color32 {
    match class_name {
        ClassName::Part | ClassName::MeshPart => Color32::from_rgb(100, 150, 255),
        ClassName::Model | ClassName::Folder => Color32::from_rgb(255, 200, 100),
        ClassName::Humanoid => Color32::from_rgb(100, 255, 150),
        ClassName::PointLight | ClassName::SpotLight => Color32::from_rgb(255, 255, 100),
        _ => Color32::GRAY,
    }
}
```

### **Week 3: Serialization**

```rust
// src/serialization.rs

#[derive(Serialize, Deserialize)]
pub struct SceneV2 {
    version: u32,
    format: String,
    entities: Vec<EntityData>,
}

#[derive(Serialize, Deserialize)]
pub struct EntityData {
    id: u32,
    class: String,
    properties: HashMap<String, serde_json::Value>,
    children: Vec<u32>,
}

pub fn save_scene_v2(world: &World, path: &Path) -> Result<(), String> {
    let mut entities = Vec::new();
    
    // Query all entities with Instance
    let query = world.query::<(Entity, &Instance)>();
    for (entity, instance) in query.iter(world) {
        let mut properties = HashMap::new();
        
        // Get properties based on class
        if let Some(base_part) = world.get::<BasePart>(entity) {
            for desc in base_part.list_properties() {
                if let Some(value) = base_part.get_property(&desc.name) {
                    properties.insert(desc.name, property_to_json(value));
                }
            }
        }
        
        entities.push(EntityData {
            id: instance.id,
            class: instance.class_name.as_str().to_string(),
            properties,
            children: vec![], // TODO: Get from hierarchy
        });
    }
    
    let scene = SceneV2 {
        version: 2,
        format: "roblox_classes".to_string(),
        entities,
    };
    
    let json = serde_json::to_string_pretty(&scene)
        .map_err(|e| format!("Serialization failed: {}", e))?;
    
    std::fs::write(path, json)
        .map_err(|e| format!("File write failed: {}", e))
}
```

### **Week 4: Property Command**

```rust
// src/commands/property_command.rs

pub struct PropertyCommand {
    entity: Entity,
    property_name: String,
    old_value: PropertyValue,
    new_value: PropertyValue,
}

impl Command for PropertyCommand {
    fn execute(&mut self, world: &mut World) {
        if let Some(mut base_part) = world.get_mut::<BasePart>(self.entity) {
            let _ = base_part.set_property(&self.property_name, self.new_value.clone());
        }
    }
    
    fn undo(&mut self, world: &mut World) {
        if let Some(mut base_part) = world.get_mut::<BasePart>(self.entity) {
            let _ = base_part.set_property(&self.property_name, self.old_value.clone());
        }
    }
}

impl PropertyCommand {
    pub fn new(
        world: &World,
        entity: Entity,
        property_name: String,
        new_value: PropertyValue,
    ) -> Result<Self, String> {
        let old_value = world.get::<BasePart>(entity)
            .and_then(|bp| bp.get_property(&property_name))
            .ok_or("Property not found")?;
        
        Ok(Self {
            entity,
            property_name,
            old_value,
            new_value,
        })
    }
}
```

---

## 📊 Testing Strategy

### **Week 1: Properties Panel**
```
✓ Visual: All property types render correctly
✓ Interaction: Edits update components immediately
✓ Validation: Invalid values show error messages
✓ Performance: No lag with 50+ properties
✓ Categories: Collapsible sections work
```

### **Week 2: Explorer**
```
✓ Visual: Icons show for all classes
✓ Hierarchy: Indentation matches parent-child
✓ Filtering: Can filter by class type
✓ Context: Right-click menus work
✓ Multi-select: Batch operations work
```

### **Week 3: Serialization**
```
✓ Save: Scene saves to JSON correctly
✓ Load: Scene loads from JSON perfectly
✓ Roundtrip: Save→Load→Save produces identical files
✓ Compatibility: V1 files still load
✓ Error: Handles corrupted files gracefully
```

### **Week 4: Commands**
```
✓ Execute: Commands modify properties
✓ Undo: Restores previous values
✓ Redo: Reapplies changes
✓ Batch: Multi-entity commands work
✓ Performance: No slowdown vs legacy
```

---

## 🎯 Success Metrics

### **Performance**
| Metric | Target | Baseline |
|--------|--------|----------|
| Frame Time | <16ms | ~20ms |
| Property Update | <1ms | ~2ms |
| UI Render | <5ms | ~8ms |
| Scene Load (1000 parts) | <2s | ~5s |

### **Functionality**
- ✅ All legacy features work
- ✅ New features available
- ✅ F9 toggle still works
- ✅ No data loss
- ✅ Better performance

### **Usability**
- ✅ Properties easier to edit
- ✅ Explorer more informative
- ✅ Save/load faster
- ✅ Undo/redo more powerful

---

## 🛡️ Risk Mitigation

### **Risk: UI Performance**
**Mitigation:** Cache property lists, debounce updates, batch edits

### **Risk: Save Format Breaking**
**Mitigation:** Version detection, backward compatibility layer

### **Risk: Undo/Redo Complexity**
**Mitigation:** Generic PropertyCommand, extensive testing

### **Risk: User Confusion**
**Mitigation:** Tooltips, documentation, gradual rollout

---

## 📋 Deliverables Checklist

### **Code**
- [ ] `src/ui/dynamic_properties.rs`
- [ ] `src/ui/property_widgets.rs`
- [ ] `src/ui/class_explorer.rs`
- [ ] `src/ui/class_icons.rs`
- [ ] `src/serialization.rs`
- [ ] `src/commands/property_command.rs`

### **Documentation**
- [ ] Properties Panel User Guide
- [ ] Explorer Panel User Guide
- [ ] Save Format Specification
- [ ] Migration Progress Report

### **Testing**
- [ ] Unit tests for property widgets
- [ ] Integration tests for serialization
- [ ] UI tests for panels
- [ ] Performance benchmarks

---

## 🚀 Getting Started

### **Pre-Phase 2 Checklist**
- [x] Phase 1 complete (100%)
- [x] All tests passing
- [x] Documentation reviewed
- [x] F9 toggle working
- [ ] Performance baseline measured
- [ ] User feedback collected

### **Week 1 Kickoff**
1. Create `src/ui/dynamic_properties.rs`
2. Implement property widget system
3. Test with BasePart (~50 properties)
4. Integrate into main UI

---

## 📞 Support

**Questions?**
- Review Phase 1 docs for context
- Check `MIGRATION_PLAN.md` for full strategy
- See `IMPLEMENTATION_STATUS.md` for current state

**Ready to begin:** Once Phase 1 testing is complete and feedback incorporated.

---

**Phase 2 Timeline:** 4 weeks  
**Start Date:** TBD (after Phase 1 testing)  
**End Date:** TBD + 4 weeks  
**Next Phase:** Phase 3 (Optimization, 2 weeks)

---

🎯 **Phase 2 Goal:** Make the new class system the primary way to interact with Eustress Engine while maintaining full backward compatibility.
