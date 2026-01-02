# Phase 3: Legacy System Removal Analysis

**Date:** November 14, 2025  
**Status:** Planning Phase

---

## 🎯 **Objective**

Remove all legacy PartData systems and complete migration to new PropertyAccess-based class system.

---

## 📋 **Legacy Code Inventory**

### **Files to DELETE:**

1. **`src/compatibility.rs`** (460 lines)
   - PartData ↔ Class component converters
   - Old/New PartType mapping
   - Batch conversion utilities
   - **Action:** Full deletion

2. **`src/migration_ui.rs`** (6071 bytes)
   - F9 toggle between old/new systems
   - Migration status UI
   - System switcher
   - **Action:** Full deletion

### **Files to REFACTOR:**

1. **`src/parts.rs`** (317 lines)
   - **Keep:** PartType enum (if used by classes)
   - **Keep:** Material enum (used by BasePart)
   - **Delete:** Legacy Part struct (lines 62-85)
   - **Delete:** PartData struct
   - **Delete:** PartManager references
   - **Rename to:** `src/enums.rs` or merge into `classes.rs`

2. **`src/rendering.rs`** (13732 bytes)
   - **Update:** Query for BasePart/Part/MeshPart instead of PartData
   - **Remove:** PartData rendering paths
   - **Update:** Material mapping to use classes

3. **`src/default_scene.rs`** (5021 bytes)
   - **Update:** Spawn classes instead of PartData
   - **Remove:** PartManager calls
   - **Use:** spawn_part, spawn_model functions from classes

4. **`src/part_selection.rs`** (8595 bytes)
   - **Update:** Query classes (Instance, BasePart)
   - **Remove:** PartData selection logic
   - **Update:** Selection to use Entity + PropertyAccess

5. **`src/move_tool.rs`** (14277 bytes)
   - **Update:** Modify Transform via PropertyAccess
   - **Remove:** PartData mutation
   - **Integrate:** PropertyCommand for undo/redo

6. **`src/rotate_tool.rs`** (8452 bytes)
   - **Update:** Use PropertyAccess for rotation
   - **Remove:** PartData rotation logic
   - **Integrate:** PropertyCommand

7. **`src/scale_tool.rs`** (19237 bytes)
   - **Update:** Use PropertyAccess for size
   - **Remove:** PartData scaling
   - **Integrate:** PropertyCommand

8. **`src/select_tool.rs`** (11637 bytes)
   - **Update:** Query classes for raycasting
   - **Remove:** PartData hit detection
   - **Update:** Selection to use Instance component

9. **`src/undo.rs`** (12680 bytes)
   - **Evaluate:** May be replaced by CommandHistory
   - **Option A:** Keep for non-property undo (delete, spawn)
   - **Option B:** Merge into CommandHistory
   - **Decision needed:** Review current usage

10. **`src/scenes.rs`** (8067 bytes)
    - **Update:** Use new serialization (save_scene/load_scene)
    - **Remove:** Old scene format
    - **Update:** Scene management to use PropertyAccess

11. **`src/lib.rs`** (758 bytes)
    - **Remove:** `pub mod parts;` (or rename to enums)
    - **Remove:** `pub use parts::PartManager;` (if exists)
    - **Keep:** New class/property exports

---

## 🔄 **System Update Requirements**

### **1. Rendering System**

**Current:** Queries `PartData` and renders meshes  
**Target:** Query `(BasePart, Part)` or `(BasePart, MeshPart)`

```rust
// OLD
fn render_parts(query: Query<&PartData>) { ... }

// NEW
fn render_parts(
    query: Query<(&BasePart, &Part, &Instance)>,
    meshparts: Query<(&BasePart, &MeshPart, &Instance)>,
) { ... }
```

### **2. Transform Tools (Move/Rotate/Scale)**

**Current:** Direct mutation of PartData  
**Target:** PropertyCommand through CommandHistory

```rust
// OLD
part_data.position = new_position;

// NEW
let old_pos = base_part.get_property("Position").unwrap();
let cmd = PropertyCommand::new(entity, "Position", old_pos, new_value);
command_history.execute(Command::Property(cmd), world)?;
```

### **3. Selection System**

**Current:** Stores PartData IDs  
**Target:** Store Entity + query components

```rust
// OLD
selection_manager.select(part_data.id);

// NEW
selection_manager.select(entity);
// Query Instance/BasePart when needed
```

### **4. Scene Management**

**Current:** Custom PartData JSON format  
**Target:** Use PropertyAccess serialization

```rust
// OLD
save_partdata_scene(parts, path);

// NEW (already implemented)
save_scene(world, path, metadata);
```

---

## 📊 **Impact Analysis**

### **High Impact (Major Refactor):**
- Transform tools (move/rotate/scale) - ~42,000 bytes total
- Rendering system - 13,732 bytes
- Selection system - 8,595 bytes

### **Medium Impact (Update Queries):**
- Default scene - 5,021 bytes
- Scene management - 8,067 bytes
- Select tool - 11,637 bytes

### **Low Impact (Minor Changes):**
- lib.rs exports - 758 bytes
- Documentation updates

---

## ⚠️ **Risks & Mitigation**

### **Risk 1: Breaking Existing Scenes**
- **Impact:** Users can't load old scenes
- **Mitigation:** Provide conversion tool or support both formats temporarily
- **Solution:** Phase 3.5 - Scene converter before full deletion

### **Risk 2: Performance Regression**
- **Impact:** PropertyAccess slower than direct field access
- **Mitigation:** Profile and optimize (Phase 3 Week 3)
- **Solution:** Cache property descriptors, batch updates

### **Risk 3: Incomplete Migration**
- **Impact:** Some systems still reference old code
- **Mitigation:** Grep for all PartData references before deletion
- **Solution:** Compile with warnings as errors

---

## 🗓️ **Phased Deletion Schedule**

### **Week 1: Core Systems** (40%)
**Day 1-2:**
- Remove compatibility.rs
- Remove migration_ui.rs  
- Update rendering.rs

**Day 3-4:**
- Update move_tool.rs
- Update rotate_tool.rs
- Update scale_tool.rs

**Day 5:**
- Testing & fixes

### **Week 2: Scene & Selection** (30%)
**Day 1-2:**
- Update default_scene.rs
- Update scenes.rs
- Remove old scene format

**Day 3-4:**
- Update part_selection.rs
- Update select_tool.rs
- Update selection_box.rs

**Day 5:**
- Testing & fixes

### **Week 3: Optimization & Polish** (30%)
**Day 1-2:**
- Profile PropertyAccess performance
- Optimize hot paths
- Cache property descriptors

**Day 3-4:**
- Final cleanup (parts.rs → enums.rs)
- Remove all PartData references
- Update documentation

**Day 5:**
- Full integration testing
- Stress tests
- Production validation

---

## ✅ **Success Criteria**

**Code Quality:**
- [ ] Zero references to `PartData` in codebase
- [ ] Zero references to `PartManager`
- [ ] All systems use PropertyAccess
- [ ] No F9 toggle code remains

**Functionality:**
- [ ] All transform tools work with PropertyCommand
- [ ] Undo/redo works for all operations
- [ ] Save/load works perfectly
- [ ] Rendering works for all class types
- [ ] Selection works for all entities

**Performance:**
- [ ] <5% overhead vs direct access
- [ ] 60 FPS with 1000+ entities
- [ ] <100ms for save/load (100 entities)
- [ ] <1ms for property get/set

**Testing:**
- [ ] All unit tests pass
- [ ] Integration tests pass
- [ ] Manual smoke tests pass
- [ ] Large scene stress tests pass

---

## 🚀 **Quick Start Checklist**

Before starting Phase 3, ensure:

1. [ ] Phase 2 is fully tested
2. [ ] BillboardGui/TextLabel working
3. [ ] All existing scenes backed up
4. [ ] Git branch created: `phase3-legacy-removal`
5. [ ] Team notified of breaking changes
6. [ ] Documentation updated

---

**Estimated Total Effort:** 15-20 days (3-4 weeks)  
**Lines to Delete:** ~6,500+ lines  
**Lines to Refactor:** ~50,000+ lines  
**Risk Level:** High (breaking changes)

---

**Ready to proceed?** Start with Week 1, Day 1: Remove compatibility.rs
