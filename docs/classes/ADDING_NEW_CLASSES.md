# 📘 Guide: Adding New Roblox-Compatible Classes to EustressEngine

**Purpose:** Maintain full parity across all systems when adding new classes.

---

## 🎯 **Overview**

Every new class must be integrated into **6 core systems**:

1. **Class Definition** (`src/classes.rs`)
2. **PropertyAccess Implementation** (`src/properties.rs`)
3. **Serialization** (`src/serialization/scene.rs`)
4. **Explorer UI** (`src/ui/class_icons.rs`, `src/ui/explorer.rs`)
5. **Dynamic Properties Panel** (`src/ui/dynamic_properties.rs`)
6. **Command System** (`src/commands/property_command.rs`)

---

## 📋 **Step-by-Step Process**

### **Step 1: Define the Class** (`src/classes.rs`)

**Location:** `src/classes.rs` (top section with other classes)

**Template:**
```rust
/// [Class Name] - [Brief description]
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct ClassName {
    // Instance properties (inherited)
    pub name: String,
    pub class_name: String,
    pub parent: Option<Entity>,
    pub children: Vec<Entity>,
    
    // Class-specific properties (group by category)
    // === Behavior ===
    pub enabled: bool,
    pub property_a: f32,
    
    // === Appearance ===
    pub color: [f32; 3],
    pub transparency: f32,
    
    // === Data ===
    pub custom_data: String,
}

impl Default for ClassName {
    fn default() -> Self {
        Self {
            name: String::new(),
            class_name: "ClassName".to_string(),
            parent: None,
            children: Vec::new(),
            
            // Set sensible defaults
            enabled: true,
            property_a: 1.0,
            color: [1.0, 1.0, 1.0],
            transparency: 0.0,
            custom_data: String::new(),
        }
    }
}
```

**Property Count Target:** ~15-50 properties depending on class complexity
- GUI elements: 30-50 properties
- Physical objects: 15-30 properties
- Utility objects: 10-20 properties

---

### **Step 2: Implement PropertyAccess** (`src/properties.rs`)

**Location:** `src/properties.rs` (with other PropertyAccess implementations)

**Template:**
```rust
impl PropertyAccess for ClassName {
    fn list_properties(&self) -> Vec<PropertyDescriptor> {
        vec![
            // Instance properties (standard for all classes)
            PropertyDescriptor {
                name: "Name".to_string(),
                property_type: PropertyType::String,
                category: "Data".to_string(),
                read_only: false,
                description: "Object name".to_string(),
            },
            PropertyDescriptor {
                name: "ClassName".to_string(),
                property_type: PropertyType::String,
                category: "Data".to_string(),
                read_only: true,
                description: "Object class type".to_string(),
            },
            
            // Class-specific properties
            PropertyDescriptor {
                name: "Enabled".to_string(),
                property_type: PropertyType::Bool,
                category: "Behavior".to_string(),
                read_only: false,
                description: "Whether the object is active".to_string(),
            },
            PropertyDescriptor {
                name: "PropertyA".to_string(),
                property_type: PropertyType::Float,
                category: "Behavior".to_string(),
                read_only: false,
                description: "Description of PropertyA".to_string(),
            },
            PropertyDescriptor {
                name: "Color".to_string(),
                property_type: PropertyType::Color,
                category: "Appearance".to_string(),
                read_only: false,
                description: "RGB color values".to_string(),
            },
            // ... add all properties
        ]
    }
    
    fn get_property(&self, name: &str) -> Option<PropertyValue> {
        match name {
            "Name" => Some(PropertyValue::String(self.name.clone())),
            "ClassName" => Some(PropertyValue::String(self.class_name.clone())),
            "Enabled" => Some(PropertyValue::Bool(self.enabled)),
            "PropertyA" => Some(PropertyValue::Float(self.property_a)),
            "Color" => Some(PropertyValue::Color(self.color)),
            "Transparency" => Some(PropertyValue::Float(self.transparency)),
            "CustomData" => Some(PropertyValue::String(self.custom_data.clone())),
            _ => None,
        }
    }
    
    fn set_property(&mut self, name: &str, value: PropertyValue) -> Result<(), String> {
        match name {
            "Name" => {
                if let PropertyValue::String(v) = value {
                    self.name = v;
                    Ok(())
                } else {
                    Err("Name must be a string".to_string())
                }
            }
            "Enabled" => {
                if let PropertyValue::Bool(v) = value {
                    self.enabled = v;
                    Ok(())
                } else {
                    Err("Enabled must be a bool".to_string())
                }
            }
            "PropertyA" => {
                if let PropertyValue::Float(v) = value {
                    self.property_a = v;
                    Ok(())
                } else {
                    Err("PropertyA must be a float".to_string())
                }
            }
            "Color" => {
                if let PropertyValue::Color(v) = value {
                    self.color = v;
                    Ok(())
                } else {
                    Err("Color must be a color array".to_string())
                }
            }
            // ... handle all properties
            "ClassName" => Err("ClassName is read-only".to_string()),
            _ => Err(format!("Property '{}' not found", name)),
        }
    }
    
    fn has_property(&self, name: &str) -> bool {
        matches!(name, "Name" | "ClassName" | "Enabled" | "PropertyA" | "Color" | "Transparency" | "CustomData")
    }
}
```

**Categories to use:**
- `"Data"` - Name, ClassName, Parent, etc.
- `"Behavior"` - Functional properties (Enabled, Active, etc.)
- `"Appearance"` - Visual properties (Color, Transparency, Size, etc.)
- `"Transform"` - Position, Rotation, Scale (for 3D objects)
- `"Physics"` - Mass, Friction, etc.
- `"Lighting"` - Brightness, Range, etc.
- `"Text"` - Font, Text, TextSize, etc.
- `"Advanced"` - Technical/debug properties

---

### **Step 3: Add to Serialization** (`src/serialization/scene.rs`)

**Three sub-steps required:**

#### **3a. Property Collection (Save)**

**Location:** `collect_properties()` function in `save_scene()`

```rust
// In save_scene() function, add to match statement:
if let Some(class_name_component) = world.get::<ClassName>(entity) {
    // Collect all properties using PropertyAccess
    for descriptor in class_name_component.list_properties() {
        if let Some(value) = class_name_component.get_property(&descriptor.name) {
            properties.insert(descriptor.name.clone(), property_value_to_json(&value));
        }
    }
}
```

#### **3b. Spawn Helper (Load)**

**Location:** `spawn_entity_from_data()` function

```rust
// Add to match class_name statement:
"ClassName" => {
    let class_name = classname_from_properties(properties);
    commands.entity(entity).insert(class_name);
}
```

#### **3c. Property Reconstruction Function**

**Location:** Bottom of `scene.rs` with other reconstruction functions

```rust
/// Reconstruct ClassName from properties
fn classname_from_properties(props: &HashMap<String, serde_json::Value>) -> ClassName {
    let mut class_name = ClassName::default();
    
    // Instance properties
    if let Some(name) = props.get("Name").and_then(|v| v.as_str()) {
        let _ = class_name.set_property("Name", PropertyValue::String(name.to_string()));
    }
    
    // Class-specific properties
    if let Some(enabled) = props.get("Enabled").and_then(|v| v.as_bool()) {
        let _ = class_name.set_property("Enabled", PropertyValue::Bool(enabled));
    }
    
    if let Some(property_a) = props.get("PropertyA").and_then(|v| v.as_f64()) {
        let _ = class_name.set_property("PropertyA", PropertyValue::Float(property_a as f32));
    }
    
    if let Some(color_arr) = props.get("Color").and_then(|v| v.as_array()) {
        if color_arr.len() == 3 {
            let color = [
                color_arr[0].as_f64().unwrap_or(1.0) as f32,
                color_arr[1].as_f64().unwrap_or(1.0) as f32,
                color_arr[2].as_f64().unwrap_or(1.0) as f32,
            ];
            let _ = class_name.set_property("Color", PropertyValue::Color(color));
        }
    }
    
    // ... reconstruct all properties
    
    class_name
}
```

---

### **Step 4: Add Explorer Icon** (`src/ui/class_icons.rs`)

**Location:** `src/ui/class_icons.rs`

#### **4a. Add to get_class_icon() function**

```rust
pub fn get_class_icon(class_name: &str) -> &'static str {
    match class_name {
        // ... existing classes
        "ClassName" => "🔷",  // Choose appropriate emoji
        _ => "📦",
    }
}
```

#### **4b. Add to get_class_color() function**

```rust
pub fn get_class_color(class_name: &str) -> egui::Color32 {
    match class_name {
        // ... existing classes
        "ClassName" => egui::Color32::from_rgb(100, 150, 255),  // Choose color
        _ => egui::Color32::from_rgb(150, 150, 150),
    }
}
```

**Icon Selection Guide:**
- GUI Elements: 🖼️ 📊 📋 🔲 📝 🎨
- 3D Objects: 🔷 🔶 ⬛ ⬜ 🟦 🟧
- Lights: 💡 🔦 🌟 ✨
- Effects: ✨ 💫 🌀 🔥 💧
- Constraints: 🔗 ⚙️ 🔧
- Containers: 📁 📦 🗂️

**Color Selection Guide:**
- Instance/Containers: Gray (120, 120, 120)
- Parts: Blue (80, 120, 200)
- GUI: Cyan (0, 200, 200)
- Lights: Yellow (255, 200, 100)
- Effects: Purple (150, 100, 200)
- Constraints: Orange (255, 150, 50)

---

### **Step 5: Update Dynamic Properties Panel** (`src/ui/dynamic_properties.rs`)

**Location:** `src/ui/dynamic_properties.rs`

#### **5a. Add render method in impl block**

```rust
fn render_classname_properties(
    &mut self,
    ui: &mut egui::Ui,
    world: &mut World,
    entity: Entity,
    command_history: &mut crate::commands::CommandHistory,
) {
    self.render_component_properties::<ClassName>(ui, world, entity, "ClassName", command_history);
}
```

#### **5b. Call in show() method**

```rust
// In show() method, add to the ScrollArea:
self.render_classname_properties(ui, world, entity, command_history);
```

---

### **Step 6: Update Command System** (`src/commands/property_command.rs`)

**Location:** `set_property()` method in PropertyCommand

```rust
// Add to the component type checks in set_property():
if let Some(mut class_name) = world.get_mut::<ClassName>(self.entity) {
    if class_name.has_property(&self.property_name) {
        return class_name.set_property(&self.property_name, value);
    }
}
```

---

## ✅ **Verification Checklist**

After adding a new class, verify:

- [ ] **Class compiles** without errors
- [ ] **PropertyAccess implemented** for all properties
- [ ] **Save works** - Create instance, save scene, check JSON
- [ ] **Load works** - Load saved scene, verify properties
- [ ] **Roundtrip fidelity** - Save → Load → Save, compare JSON files
- [ ] **Properties panel shows** all properties with correct widgets
- [ ] **Property editing works** - Change values in UI
- [ ] **Undo/redo works** - Edit property, undo, redo
- [ ] **Explorer icon shows** with correct color
- [ ] **Search/filter works** in Explorer and Properties
- [ ] **Command history tracks** property changes

---

## 📊 **Code Statistics Per Class**

**Expected lines per class:**
- Class definition: ~50-100 lines
- PropertyAccess: ~100-200 lines
- Serialization (save): ~10 lines
- Serialization (spawn): ~5 lines
- Serialization (reconstruct): ~50-100 lines
- UI icons: ~2 lines
- Properties panel: ~5 lines
- Command system: ~5 lines

**Total: ~227-522 lines per class**

---

## 🔄 **Property Type Reference**

**PropertyType enum values:**
```rust
PropertyType::String     // Text input
PropertyType::Float      // Drag value or slider
PropertyType::Int        // Integer drag value
PropertyType::Bool       // Checkbox
PropertyType::Vector3    // 3x float drag values
PropertyType::Color      // RGB color picker
PropertyType::Transform  // Full transform editor
PropertyType::Enum       // Dropdown/ComboBox
```

**PropertyValue enum values:**
```rust
PropertyValue::String(String)
PropertyValue::Float(f32)
PropertyValue::Int(i32)
PropertyValue::Bool(bool)
PropertyValue::Vector3([f32; 3])
PropertyValue::Color([f32; 3])
PropertyValue::Transform(Transform)
PropertyValue::Enum(String)
```

---

## 🎨 **Category Organization**

**Standard categories for PropertyDescriptor:**

1. **Data** - Identity properties (Name, ClassName, Parent)
2. **Behavior** - Functional toggles (Enabled, Active, Locked)
3. **Appearance** - Visual properties (Color, Transparency, Material)
4. **Transform** - Position, Rotation, Scale
5. **Physics** - Mass, Friction, Elasticity, Velocity
6. **Lighting** - Brightness, Range, Shadows
7. **Text** - Font, TextSize, Text content
8. **Advanced** - Technical properties (CastShadow, Archivable)

---

## 🚀 **Performance Considerations**

1. **PropertyAccess should be lightweight** - No heavy computation in get/set
2. **Use Reflect** for Bevy integration (enables inspector plugins)
3. **Default values should be sensible** - Match Roblox Studio defaults
4. **String properties should use String::new()** not empty strings for efficiency
5. **Collections (Vec) should be pre-allocated** if size is known

---

## 📝 **Example: Complete Minimal Class**

See next section for BillboardGui and TextLabel implementations as complete examples.

---

## 🔗 **Related Files**

- `src/classes.rs` - Class definitions
- `src/properties.rs` - PropertyAccess implementations
- `src/serialization/scene.rs` - Save/load logic
- `src/ui/class_icons.rs` - Explorer icons
- `src/ui/dynamic_properties.rs` - Properties panel
- `src/commands/property_command.rs` - Undo/redo support

---

**This guide ensures every new class has full parity across all systems!** 🎉
