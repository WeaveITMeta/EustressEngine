# PartManager Removal Migration Guide

**Status:** IN PROGRESS  
**Approach:** Option B - Aggressive Removal

---

## 🎯 **Migration Strategy**

Replace centralized PartManager with ECS queries:

**OLD:** `part_manager.get_part(id)` → Returns PartData  
**NEW:** `Query<(&Instance, &BasePart, &Part)>` → Query components directly

---

## 🔄 **Key Replacements**

### **1. Getting Part Data**
```rust
// OLD
let part_data = part_manager.get_part(id)?;

// NEW
if let Ok((instance, base_part, part)) = query.get(entity) {
    // Use components directly
}
```

### **2. Creating Parts**
```rust
// OLD
let id = part_manager.create_part(PartType::Cube, position, Some("Part".to_string()));

// NEW
use crate::classes::spawn_part;
let entity = spawn_part(commands, meshes, materials, instance, base_part, part);
```

### **3. Updating Parts**
```rust
// OLD
part_manager.update_part(id, PartUpdate { position: Some([x, y, z]), .. });

// NEW
use crate::commands::PropertyCommand;
let cmd = PropertyCommand::new(entity, "Position", old_value, new_value);
command_history.execute(Command::Property(cmd), world)?;
```

### **4. Deleting Parts**
```rust
// OLD
part_manager.delete_part(id)?;

// NEW
commands.entity(entity).despawn_recursive();
```

### **5. Listing Parts**
```rust
// OLD
let parts = part_manager.list_parts()?;

// NEW
for (entity, instance, base_part, part) in query.iter() {
    // Process each part
}
```

---

## 📋 **Files Requiring Updates** (74 references across 24 files)

### **HIGH PRIORITY** (Core Systems)

**1. `src/ui/mod.rs`** (8 matches)
- Remove `BevyPartManager` wrapper
- Remove `part_manager` field from `StudioUiPlugin`
- Update systems to use ECS queries

**2. `src/rendering.rs`** (10 matches)
- Remove `BevyPartManager` wrapper
- Remove `part_manager` field from `PartRenderingPlugin`
- Update rendering systems to query `(BasePart, Part, MeshPart)` directly
- Remove `sync_parts` system

**3. `src/commands/part_commands.rs`** (10 matches)
- Replace all PartManager calls with ECS Commands
- Update CreatePart/DeletePart/UpdatePart commands

**4. `src/default_scene.rs`** (4 matches)
- Remove PartManager parameter
- Use `spawn_part` from classes.rs
- Update baseplate spawn to use classes

**5. `src/scenes.rs`** (4 matches)
- Remove PartManager from scene save/load
- Use new serialization (save_scene/load_scene)

### **MEDIUM PRIORITY** (UI Systems)

**6. `src/ui/explorer.rs`** (3 matches)
- Query `(Entity, Instance, &Children)` for hierarchy
- Remove PartManager dependency

**7. `src/ui/toolbox.rs`** (3 matches)
- Use `spawn_part` instead of part_manager.create_part
- Pass Commands instead of PartManager

**8. `src/ui/command_bar.rs`** (4 matches)
- Update commands to use ECS
- Remove PartManager parameter

**9. `src/ui/dock.rs`** (3 matches)
- Remove BevyPartManager wrapper
- Update panel systems

**10. `src/ui/properties.rs`** (3 matches)  
- Already mostly migrated to PropertyAccess
- Remove any remaining PartManager refs

### **LOW PRIORITY** (Tools & Selection)

**11. `src/select_tool.rs`** (3 matches)
- Query entities directly for raycasting
- Remove PartManager dependency

**12. `src/part_selection.rs`** (2 matches)
- Use Entity IDs instead of u32 IDs
- Query components for selection

**13. `src/undo.rs`** (4 matches)
- May need update or replacement with CommandHistory
- Decision: Keep for non-property undo (spawn/despawn)

**14. `src/commands/scene_management_commands.rs`** (4 matches)
- Update to use new serialization
- Remove PartManager refs

---

## ✅ **Completed**

- [x] Remove `compatibility.rs` (460 lines)
- [x] Remove `migration_ui.rs` (6,071 bytes)
- [x] Update `main.rs` - Remove PartManager init
- [x] Update `lib.rs` - Remove PartManager export

---

## 🚧 **In Progress**

- [ ] Update `src/ui/mod.rs` - Remove BevyPartManager
- [ ] Update `src/rendering.rs` - Use ECS queries
- [ ] Update `src/commands/part_commands.rs` - Use Commands
- [ ] Update `src/default_scene.rs` - Use spawn functions

---

## ⏳ **TODO**

Remaining 20 files with PartManager references

---

##Human: Continue
