//! # Instance API
//!
//! Roblox-compatible Instance API for scripting.
//! Provides entity creation, hierarchy management, and property access.
//!
//! ## Table of Contents
//!
//! 1. **Instance** — Base class for all entities
//! 2. **InstanceRef** — Reference to an instance (like Roblox Instance)
//! 3. **InstanceRegistry** — Global registry mapping entity IDs to instances
//! 4. **Property Access** — Get/set properties by name

use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;

use super::types::{Vector3, CFrame, Color3};
use super::events::{Signal, SignalArg, PropertyChangedSignal};

// ============================================================================
// 1. Instance Properties
// ============================================================================

/// Property value that can be stored on an instance
#[derive(Debug, Clone)]
pub enum PropertyValue {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Vector3(Vector3),
    CFrame(CFrame),
    Color3(Color3),
    EntityRef(u64),
    EnumValue(String, String), // (EnumType, Value)
}

impl Default for PropertyValue {
    fn default() -> Self {
        PropertyValue::None
    }
}

impl PropertyValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropertyValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            PropertyValue::Int(v) => Some(*v),
            PropertyValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            PropertyValue::Float(v) => Some(*v),
            PropertyValue::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            PropertyValue::String(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_vector3(&self) -> Option<&Vector3> {
        match self {
            PropertyValue::Vector3(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_cframe(&self) -> Option<&CFrame> {
        match self {
            PropertyValue::CFrame(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_color3(&self) -> Option<&Color3> {
        match self {
            PropertyValue::Color3(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_entity_ref(&self) -> Option<u64> {
        match self {
            PropertyValue::EntityRef(v) => Some(*v),
            _ => None,
        }
    }
}

impl From<bool> for PropertyValue {
    fn from(v: bool) -> Self { PropertyValue::Bool(v) }
}

impl From<i64> for PropertyValue {
    fn from(v: i64) -> Self { PropertyValue::Int(v) }
}

impl From<i32> for PropertyValue {
    fn from(v: i32) -> Self { PropertyValue::Int(v as i64) }
}

impl From<f64> for PropertyValue {
    fn from(v: f64) -> Self { PropertyValue::Float(v) }
}

impl From<f32> for PropertyValue {
    fn from(v: f32) -> Self { PropertyValue::Float(v as f64) }
}

impl From<String> for PropertyValue {
    fn from(v: String) -> Self { PropertyValue::String(v) }
}

impl From<&str> for PropertyValue {
    fn from(v: &str) -> Self { PropertyValue::String(v.to_string()) }
}

impl From<Vector3> for PropertyValue {
    fn from(v: Vector3) -> Self { PropertyValue::Vector3(v) }
}

impl From<CFrame> for PropertyValue {
    fn from(v: CFrame) -> Self { PropertyValue::CFrame(v) }
}

impl From<Color3> for PropertyValue {
    fn from(v: Color3) -> Self { PropertyValue::Color3(v) }
}

// ============================================================================
// 2. Instance — Base class for all entities
// ============================================================================

/// Instance data stored in the registry
pub struct InstanceData {
    /// Unique entity ID (maps to Bevy Entity)
    pub entity_id: u64,
    /// Instance name
    pub name: String,
    /// Class name (Part, Model, Script, etc.)
    pub class_name: String,
    /// Parent entity ID (0 = no parent)
    pub parent_id: u64,
    /// Child entity IDs
    pub children: Vec<u64>,
    /// Custom properties
    pub properties: HashMap<String, PropertyValue>,
    /// Property changed signals
    pub property_signals: HashMap<String, PropertyChangedSignal>,
    /// Archivable flag
    pub archivable: bool,
    /// Changed signal (fires when any property changes)
    pub changed: Signal<String>,
    /// ChildAdded signal
    pub child_added: Signal<u64>,
    /// ChildRemoved signal
    pub child_removed: Signal<u64>,
    /// DescendantAdded signal
    pub descendant_added: Signal<u64>,
    /// DescendantRemoving signal
    pub descendant_removing: Signal<u64>,
    /// AncestryChanged signal
    pub ancestry_changed: Signal<(u64, u64)>, // (child, parent)
    /// Destroying signal
    pub destroying: Signal<()>,
}

impl InstanceData {
    pub fn new(entity_id: u64, class_name: &str, name: &str) -> Self {
        Self {
            entity_id,
            name: name.to_string(),
            class_name: class_name.to_string(),
            parent_id: 0,
            children: Vec::new(),
            properties: HashMap::new(),
            property_signals: HashMap::new(),
            archivable: true,
            changed: Signal::new(),
            child_added: Signal::new(),
            child_removed: Signal::new(),
            descendant_added: Signal::new(),
            descendant_removing: Signal::new(),
            ancestry_changed: Signal::new(),
            destroying: Signal::new(),
        }
    }

    /// Get a property value
    pub fn get_property(&self, name: &str) -> Option<&PropertyValue> {
        self.properties.get(name)
    }

    /// Set a property value
    pub fn set_property(&mut self, name: &str, value: PropertyValue) {
        let old_value = self.properties.insert(name.to_string(), value.clone());
        
        // Fire property changed signal if it exists
        if let Some(signal) = self.property_signals.get(name) {
            signal.fire(SignalArg::String(name.to_string()));
        }
        
        // Fire general changed signal
        self.changed.fire(name.to_string());
    }

    /// Get or create a property changed signal
    pub fn get_property_changed_signal(&mut self, property_name: &str) -> PropertyChangedSignal {
        self.property_signals
            .entry(property_name.to_string())
            .or_insert_with(|| PropertyChangedSignal::new(property_name))
            .clone()
    }
}

// ============================================================================
// 3. InstanceRef — Reference to an instance
// ============================================================================

/// A reference to an instance that can be passed to scripts.
/// This is the primary way scripts interact with entities.
#[derive(Clone)]
pub struct InstanceRef {
    entity_id: u64,
    registry: Arc<RwLock<InstanceRegistry>>,
}

impl InstanceRef {
    /// Create a new instance reference
    pub fn new(entity_id: u64, registry: Arc<RwLock<InstanceRegistry>>) -> Self {
        Self { entity_id, registry }
    }

    /// Get the entity ID
    pub fn entity_id(&self) -> u64 {
        self.entity_id
    }

    /// Check if the instance still exists
    pub fn exists(&self) -> bool {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).is_some()
    }

    /// Get the instance name
    pub fn name(&self) -> Option<String> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.name.clone())
    }

    /// Set the instance name
    pub fn set_name(&self, name: &str) {
        let mut registry = self.registry.write().unwrap();
        if let Some(instance) = registry.get_mut(self.entity_id) {
            instance.name = name.to_string();
            instance.changed.fire("Name".to_string());
        }
    }

    /// Get the class name
    pub fn class_name(&self) -> Option<String> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.class_name.clone())
    }

    /// Check if instance is of a specific class (or inherits from it)
    pub fn is_a(&self, class_name: &str) -> bool {
        let registry = self.registry.read().unwrap();
        if let Some(instance) = registry.get(self.entity_id) {
            // Direct match
            if instance.class_name == class_name {
                return true;
            }
            // Check inheritance (simplified - would need class hierarchy)
            match class_name {
                "Instance" => true,
                "BasePart" => matches!(instance.class_name.as_str(), 
                    "Part" | "MeshPart" | "WedgePart" | "CornerWedgePart" | "TrussPart" | "SpawnLocation" | "Seat" | "VehicleSeat" | "Terrain"),
                "PVInstance" => matches!(instance.class_name.as_str(),
                    "Part" | "MeshPart" | "Model" | "BasePart" | "PVInstance"),
                "GuiObject" => matches!(instance.class_name.as_str(),
                    "Frame" | "TextLabel" | "TextButton" | "TextBox" | "ImageLabel" | "ImageButton" | "ScrollingFrame" | "ViewportFrame"),
                "GuiBase2d" => matches!(instance.class_name.as_str(),
                    "ScreenGui" | "BillboardGui" | "SurfaceGui" | "Frame" | "TextLabel" | "TextButton"),
                "LuaSourceContainer" => matches!(instance.class_name.as_str(),
                    "Script" | "LocalScript" | "ModuleScript"),
                "Light" => matches!(instance.class_name.as_str(),
                    "PointLight" | "SpotLight" | "SurfaceLight"),
                "Constraint" => matches!(instance.class_name.as_str(),
                    "WeldConstraint" | "Motor6D" | "HingeConstraint" | "PrismaticConstraint" | "BallSocketConstraint" | "SpringConstraint" | "RopeConstraint"),
                _ => false,
            }
        } else {
            false
        }
    }

    /// Get parent instance
    pub fn parent(&self) -> Option<InstanceRef> {
        let registry = self.registry.read().unwrap();
        if let Some(instance) = registry.get(self.entity_id) {
            if instance.parent_id != 0 {
                return Some(InstanceRef::new(instance.parent_id, self.registry.clone()));
            }
        }
        None
    }

    /// Set parent instance
    pub fn set_parent(&self, parent: Option<&InstanceRef>) {
        let new_parent_id = parent.map(|p| p.entity_id).unwrap_or(0);
        let old_parent_id;
        
        {
            let mut registry = self.registry.write().unwrap();
            
            // Get old parent
            old_parent_id = registry.get(self.entity_id)
                .map(|i| i.parent_id)
                .unwrap_or(0);
            
            if old_parent_id == new_parent_id {
                return;
            }
            
            // Remove from old parent's children
            if old_parent_id != 0 {
                if let Some(old_parent) = registry.get_mut(old_parent_id) {
                    old_parent.children.retain(|&id| id != self.entity_id);
                    old_parent.child_removed.fire(self.entity_id);
                }
            }
            
            // Add to new parent's children
            if new_parent_id != 0 {
                if let Some(new_parent) = registry.get_mut(new_parent_id) {
                    new_parent.children.push(self.entity_id);
                    new_parent.child_added.fire(self.entity_id);
                }
            }
            
            // Update this instance's parent
            if let Some(instance) = registry.get_mut(self.entity_id) {
                instance.parent_id = new_parent_id;
                instance.ancestry_changed.fire((self.entity_id, new_parent_id));
            }
        }
    }

    /// Get children
    pub fn get_children(&self) -> Vec<InstanceRef> {
        let registry = self.registry.read().unwrap();
        if let Some(instance) = registry.get(self.entity_id) {
            instance.children.iter()
                .map(|&id| InstanceRef::new(id, self.registry.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get all descendants (recursive)
    pub fn get_descendants(&self) -> Vec<InstanceRef> {
        let mut result = Vec::new();
        let mut stack = self.get_children();
        
        while let Some(child) = stack.pop() {
            result.push(child.clone());
            stack.extend(child.get_children());
        }
        
        result
    }

    /// Find first child with name
    pub fn find_first_child(&self, name: &str, recursive: bool) -> Option<InstanceRef> {
        let registry = self.registry.read().unwrap();
        
        if let Some(instance) = registry.get(self.entity_id) {
            // Check direct children
            for &child_id in &instance.children {
                if let Some(child) = registry.get(child_id) {
                    if child.name == name {
                        return Some(InstanceRef::new(child_id, self.registry.clone()));
                    }
                }
            }
            
            // Recursive search
            if recursive {
                for &child_id in &instance.children {
                    let child_ref = InstanceRef::new(child_id, self.registry.clone());
                    if let Some(found) = child_ref.find_first_child(name, true) {
                        return Some(found);
                    }
                }
            }
        }
        
        None
    }

    /// Find first child of class
    pub fn find_first_child_of_class(&self, class_name: &str) -> Option<InstanceRef> {
        let registry = self.registry.read().unwrap();
        
        if let Some(instance) = registry.get(self.entity_id) {
            for &child_id in &instance.children {
                if let Some(child) = registry.get(child_id) {
                    if child.class_name == class_name {
                        return Some(InstanceRef::new(child_id, self.registry.clone()));
                    }
                }
            }
        }
        
        None
    }

    /// Find first child which is a (inherits from) class
    pub fn find_first_child_which_is_a(&self, class_name: &str) -> Option<InstanceRef> {
        for child in self.get_children() {
            if child.is_a(class_name) {
                return Some(child);
            }
        }
        None
    }

    /// Find first ancestor with name
    pub fn find_first_ancestor(&self, name: &str) -> Option<InstanceRef> {
        let mut current = self.parent();
        while let Some(parent) = current {
            if parent.name().as_deref() == Some(name) {
                return Some(parent);
            }
            current = parent.parent();
        }
        None
    }

    /// Find first ancestor of class
    pub fn find_first_ancestor_of_class(&self, class_name: &str) -> Option<InstanceRef> {
        let mut current = self.parent();
        while let Some(parent) = current {
            if parent.class_name().as_deref() == Some(class_name) {
                return Some(parent);
            }
            current = parent.parent();
        }
        None
    }

    /// Find first descendant with name
    pub fn find_first_descendant(&self, name: &str) -> Option<InstanceRef> {
        self.find_first_child(name, true)
    }

    /// Check if this is a descendant of another instance
    pub fn is_descendant_of(&self, ancestor: &InstanceRef) -> bool {
        let mut current = self.parent();
        while let Some(parent) = current {
            if parent.entity_id == ancestor.entity_id {
                return true;
            }
            current = parent.parent();
        }
        false
    }

    /// Check if this is an ancestor of another instance
    pub fn is_ancestor_of(&self, descendant: &InstanceRef) -> bool {
        descendant.is_descendant_of(self)
    }

    /// Get full name (path from root)
    pub fn get_full_name(&self) -> String {
        let mut parts = vec![self.name().unwrap_or_default()];
        let mut current = self.parent();
        
        while let Some(parent) = current {
            parts.push(parent.name().unwrap_or_default());
            current = parent.parent();
        }
        
        parts.reverse();
        parts.join(".")
    }

    /// Get a property value
    pub fn get_property(&self, name: &str) -> Option<PropertyValue> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id)
            .and_then(|i| i.properties.get(name).cloned())
    }

    /// Set a property value
    pub fn set_property(&self, name: &str, value: PropertyValue) {
        let mut registry = self.registry.write().unwrap();
        if let Some(instance) = registry.get_mut(self.entity_id) {
            instance.set_property(name, value);
        }
    }

    /// Get attribute (same as property in our implementation)
    pub fn get_attribute(&self, name: &str) -> Option<PropertyValue> {
        self.get_property(name)
    }

    /// Set attribute
    pub fn set_attribute(&self, name: &str, value: PropertyValue) {
        self.set_property(name, value);
    }

    /// Get all attributes
    pub fn get_attributes(&self) -> HashMap<String, PropertyValue> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id)
            .map(|i| i.properties.clone())
            .unwrap_or_default()
    }

    /// Clone the instance (deep copy)
    pub fn clone_instance(&self) -> Option<InstanceRef> {
        let registry_arc = self.registry.clone();
        
        // Extract data we need while holding the lock, then release it
        let (class_name, name, properties, archivable, children_to_clone) = {
            let registry = registry_arc.read().unwrap();
            let source = registry.get(self.entity_id)?;
            if !source.archivable {
                return None;
            }
            (
                source.class_name.clone(),
                source.name.clone(),
                source.properties.clone(),
                source.archivable,
                source.children.clone(),
            )
        };
        
        // Get new ID with write lock
        let new_id = {
            let mut registry = registry_arc.write().unwrap();
            registry.next_entity_id()
        };
        
        let mut new_instance = InstanceData::new(new_id, &class_name, &name);
        new_instance.properties = properties;
        new_instance.archivable = archivable;
        
        let new_ref = {
            let mut registry = registry_arc.write().unwrap();
            registry.insert(new_instance);
            InstanceRef::new(new_id, registry_arc.clone())
        };
        
        for child_id in children_to_clone {
            let child_ref = InstanceRef::new(child_id, registry_arc.clone());
            if let Some(cloned_child) = child_ref.clone_instance() {
                cloned_child.set_parent(Some(&new_ref));
            }
        }
        
        Some(new_ref)
    }

    /// Destroy the instance and all descendants
    pub fn destroy(&self) {
        // Fire destroying signal
        {
            let registry = self.registry.read().unwrap();
            if let Some(instance) = registry.get(self.entity_id) {
                instance.destroying.fire(());
            }
        }
        
        // Destroy children first
        let children = self.get_children();
        for child in children {
            child.destroy();
        }
        
        // Remove from parent
        self.set_parent(None);
        
        // Remove from registry
        let mut registry = self.registry.write().unwrap();
        registry.remove(self.entity_id);
    }

    /// Clear all children
    pub fn clear_all_children(&self) {
        let children = self.get_children();
        for child in children {
            child.destroy();
        }
    }

    /// Get Changed signal
    pub fn changed(&self) -> Option<Signal<String>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.changed.clone())
    }

    /// Get ChildAdded signal
    pub fn child_added(&self) -> Option<Signal<u64>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.child_added.clone())
    }

    /// Get ChildRemoved signal
    pub fn child_removed(&self) -> Option<Signal<u64>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.child_removed.clone())
    }

    /// Get DescendantAdded signal
    pub fn descendant_added(&self) -> Option<Signal<u64>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.descendant_added.clone())
    }

    /// Get DescendantRemoving signal
    pub fn descendant_removing(&self) -> Option<Signal<u64>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.descendant_removing.clone())
    }

    /// Get AncestryChanged signal
    pub fn ancestry_changed(&self) -> Option<Signal<(u64, u64)>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.ancestry_changed.clone())
    }

    /// Get Destroying signal
    pub fn destroying(&self) -> Option<Signal<()>> {
        let registry = self.registry.read().unwrap();
        registry.get(self.entity_id).map(|i| i.destroying.clone())
    }

    /// Get property changed signal for a specific property
    pub fn get_property_changed_signal(&self, property_name: &str) -> Option<PropertyChangedSignal> {
        let mut registry = self.registry.write().unwrap();
        registry.get_mut(self.entity_id)
            .map(|i| i.get_property_changed_signal(property_name))
    }
}

impl std::fmt::Debug for InstanceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "InstanceRef({})", self.entity_id)
    }
}

impl PartialEq for InstanceRef {
    fn eq(&self, other: &Self) -> bool {
        self.entity_id == other.entity_id
    }
}

// ============================================================================
// 4. InstanceRegistry — Global registry
// ============================================================================

/// Global registry of all instances
pub struct InstanceRegistry {
    instances: HashMap<u64, InstanceData>,
    next_id: u64,
    /// Callback to create Bevy entity (set by engine)
    create_entity_callback: Option<Box<dyn Fn(&str, &str) -> u64 + Send + Sync>>,
    /// Callback to destroy Bevy entity (set by engine)
    destroy_entity_callback: Option<Box<dyn Fn(u64) + Send + Sync>>,
}

impl Default for InstanceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceRegistry {
    pub fn new() -> Self {
        Self {
            instances: HashMap::new(),
            next_id: 1,
            create_entity_callback: None,
            destroy_entity_callback: None,
        }
    }

    /// Set the callback for creating Bevy entities
    pub fn set_create_entity_callback<F>(&mut self, callback: F)
    where
        F: Fn(&str, &str) -> u64 + Send + Sync + 'static,
    {
        self.create_entity_callback = Some(Box::new(callback));
    }

    /// Set the callback for destroying Bevy entities
    pub fn set_destroy_entity_callback<F>(&mut self, callback: F)
    where
        F: Fn(u64) + Send + Sync + 'static,
    {
        self.destroy_entity_callback = Some(Box::new(callback));
    }

    /// Get next entity ID
    pub fn next_entity_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Create a new instance
    pub fn create(&mut self, class_name: &str, name: Option<&str>) -> u64 {
        let name = name.unwrap_or(class_name);
        
        // Use callback if available, otherwise generate ID
        let entity_id = if let Some(callback) = &self.create_entity_callback {
            callback(class_name, name)
        } else {
            self.next_entity_id()
        };
        
        let instance = InstanceData::new(entity_id, class_name, name);
        self.instances.insert(entity_id, instance);
        
        entity_id
    }

    /// Insert an existing instance
    pub fn insert(&mut self, instance: InstanceData) {
        self.instances.insert(instance.entity_id, instance);
    }

    /// Get an instance by ID
    pub fn get(&self, entity_id: u64) -> Option<&InstanceData> {
        self.instances.get(&entity_id)
    }

    /// Get a mutable instance by ID
    pub fn get_mut(&mut self, entity_id: u64) -> Option<&mut InstanceData> {
        self.instances.get_mut(&entity_id)
    }

    /// Remove an instance
    pub fn remove(&mut self, entity_id: u64) -> Option<InstanceData> {
        let instance = self.instances.remove(&entity_id);
        
        // Call destroy callback if available
        if instance.is_some() {
            if let Some(callback) = &self.destroy_entity_callback {
                callback(entity_id);
            }
        }
        
        instance
    }

    /// Find instance by name (first match)
    pub fn find_by_name(&self, name: &str) -> Option<u64> {
        self.instances.iter()
            .find(|(_, i)| i.name == name)
            .map(|(&id, _)| id)
    }

    /// Find all instances by class name
    pub fn find_by_class(&self, class_name: &str) -> Vec<u64> {
        self.instances.iter()
            .filter(|(_, i)| i.class_name == class_name)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get all instance IDs
    pub fn all_ids(&self) -> Vec<u64> {
        self.instances.keys().copied().collect()
    }

    /// Get instance count
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Clear all instances
    pub fn clear(&mut self) {
        self.instances.clear();
    }
}

// ============================================================================
// 5. Instance Factory — Create instances with proper setup
// ============================================================================

/// Factory for creating instances with proper class defaults
pub struct InstanceFactory {
    registry: Arc<RwLock<InstanceRegistry>>,
}

impl InstanceFactory {
    pub fn new(registry: Arc<RwLock<InstanceRegistry>>) -> Self {
        Self { registry }
    }

    /// Create a new instance of the given class
    pub fn create(&self, class_name: &str) -> InstanceRef {
        let entity_id = {
            let mut registry = self.registry.write().unwrap();
            registry.create(class_name, None)
        };
        
        // Set default properties based on class
        let instance_ref = InstanceRef::new(entity_id, self.registry.clone());
        self.apply_class_defaults(&instance_ref, class_name);
        
        instance_ref
    }

    /// Create a new instance with a specific name
    pub fn create_with_name(&self, class_name: &str, name: &str) -> InstanceRef {
        let entity_id = {
            let mut registry = self.registry.write().unwrap();
            registry.create(class_name, Some(name))
        };
        
        let instance_ref = InstanceRef::new(entity_id, self.registry.clone());
        self.apply_class_defaults(&instance_ref, class_name);
        
        instance_ref
    }

    /// Apply default properties for a class
    fn apply_class_defaults(&self, instance: &InstanceRef, class_name: &str) {
        match class_name {
            "Part" | "MeshPart" | "WedgePart" | "CornerWedgePart" => {
                instance.set_property("Size", Vector3::new(4.0, 1.0, 2.0).into());
                instance.set_property("Position", Vector3::ZERO.into());
                instance.set_property("Orientation", Vector3::ZERO.into());
                instance.set_property("Anchored", false.into());
                instance.set_property("CanCollide", true.into());
                instance.set_property("CanTouch", true.into());
                instance.set_property("CanQuery", true.into());
                instance.set_property("Transparency", 0.0.into());
                instance.set_property("Color", Color3::from_rgb(163, 162, 165).into());
                instance.set_property("Material", PropertyValue::EnumValue("Material".into(), "Plastic".into()));
                instance.set_property("CastShadow", true.into());
            }
            "Model" => {
                instance.set_property("PrimaryPart", PropertyValue::None);
            }
            "Script" | "LocalScript" => {
                instance.set_property("Source", "".into());
                instance.set_property("Enabled", true.into());
            }
            "ModuleScript" => {
                instance.set_property("Source", "return {}".into());
            }
            "PointLight" => {
                instance.set_property("Brightness", 1.0.into());
                instance.set_property("Color", Color3::WHITE.into());
                instance.set_property("Range", 8.0.into());
                instance.set_property("Shadows", true.into());
                instance.set_property("Enabled", true.into());
            }
            "SpotLight" => {
                instance.set_property("Brightness", 1.0.into());
                instance.set_property("Color", Color3::WHITE.into());
                instance.set_property("Range", 16.0.into());
                instance.set_property("Angle", 90.0.into());
                instance.set_property("Shadows", true.into());
                instance.set_property("Enabled", true.into());
            }
            "Sound" => {
                instance.set_property("SoundId", "".into());
                instance.set_property("Volume", 0.5.into());
                instance.set_property("Playing", false.into());
                instance.set_property("Looped", false.into());
                instance.set_property("PlaybackSpeed", 1.0.into());
                instance.set_property("TimePosition", 0.0.into());
            }
            "Frame" | "TextLabel" | "TextButton" | "ImageLabel" | "ImageButton" => {
                instance.set_property("Visible", true.into());
                instance.set_property("BackgroundTransparency", 0.0.into());
                instance.set_property("BackgroundColor3", Color3::WHITE.into());
            }
            "TextLabel" | "TextButton" | "TextBox" => {
                instance.set_property("Text", "".into());
                instance.set_property("TextColor3", Color3::BLACK.into());
                instance.set_property("TextSize", 14.0.into());
            }
            _ => {}
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_registry() -> Arc<RwLock<InstanceRegistry>> {
        Arc::new(RwLock::new(InstanceRegistry::new()))
    }

    #[test]
    fn test_instance_creation() {
        let registry = create_test_registry();
        let factory = InstanceFactory::new(registry.clone());
        
        let part = factory.create("Part");
        assert!(part.exists());
        assert_eq!(part.class_name(), Some("Part".to_string()));
    }

    #[test]
    fn test_instance_hierarchy() {
        let registry = create_test_registry();
        let factory = InstanceFactory::new(registry.clone());
        
        let model = factory.create_with_name("Model", "TestModel");
        let part = factory.create_with_name("Part", "TestPart");
        
        part.set_parent(Some(&model));
        
        assert_eq!(part.parent().unwrap().entity_id(), model.entity_id());
        assert_eq!(model.get_children().len(), 1);
        assert!(part.is_descendant_of(&model));
    }

    #[test]
    fn test_find_first_child() {
        let registry = create_test_registry();
        let factory = InstanceFactory::new(registry.clone());
        
        let model = factory.create_with_name("Model", "TestModel");
        let part1 = factory.create_with_name("Part", "Part1");
        let part2 = factory.create_with_name("Part", "Part2");
        
        part1.set_parent(Some(&model));
        part2.set_parent(Some(&model));
        
        let found = model.find_first_child("Part2", false);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), Some("Part2".to_string()));
    }

    #[test]
    fn test_instance_destroy() {
        let registry = create_test_registry();
        let factory = InstanceFactory::new(registry.clone());
        
        let model = factory.create_with_name("Model", "TestModel");
        let part = factory.create_with_name("Part", "TestPart");
        
        part.set_parent(Some(&model));
        part.destroy();
        
        assert!(!part.exists());
        assert_eq!(model.get_children().len(), 0);
    }

    #[test]
    fn test_is_a() {
        let registry = create_test_registry();
        let factory = InstanceFactory::new(registry.clone());
        
        let part = factory.create("Part");
        
        assert!(part.is_a("Part"));
        assert!(part.is_a("BasePart"));
        assert!(part.is_a("Instance"));
        assert!(!part.is_a("Model"));
    }

    #[test]
    fn test_properties() {
        let registry = create_test_registry();
        let factory = InstanceFactory::new(registry.clone());
        
        let part = factory.create("Part");
        
        // Check default property
        let size = part.get_property("Size");
        assert!(size.is_some());
        
        // Set custom property
        part.set_property("CustomProp", PropertyValue::String("test".into()));
        let custom = part.get_property("CustomProp");
        assert_eq!(custom.and_then(|p| p.as_string().map(|s| s.to_string())), Some("test".to_string()));
    }
}
