"""
Blender script to re-export Y-Bot with a clean hierarchy that matches animations.
The goal is to have the armature named "Armature" at the root, so bone paths match.

Run with: blender --background --python clean_ybot.py
"""
import bpy
import os

script_dir = os.path.dirname(os.path.abspath(__file__))
characters_dir = os.path.dirname(script_dir)

input_path = os.path.join(characters_dir, "y_bot.glb")
output_path = os.path.join(characters_dir, "y_bot_clean.glb")

print(f"Input: {input_path}")
print(f"Output: {output_path}")

# Clear scene
bpy.ops.wm.read_factory_settings(use_empty=True)

# Import the Y-Bot model
print("Importing Y-Bot...")
bpy.ops.import_scene.gltf(filepath=input_path)

# Note: Do NOT apply rotation here - let the GLTF exporter handle Y-up conversion
# and let the Rust code apply any needed rotation fix

# Find the armature
armature = None
for obj in bpy.context.scene.objects:
    if obj.type == 'ARMATURE':
        armature = obj
        print(f"Found armature: {obj.name}")
        break

if not armature:
    print("ERROR: No armature found!")
    exit(1)

# Rename armature to "Armature" to match animation files
armature.name = "Armature"
if armature.data:
    armature.data.name = "Armature"
print(f"Renamed armature to: {armature.name}")

# Remove _rootJoint bone and reparent its children to root
# This makes the hierarchy match: Armature -> mixamorig:Hips_01
bpy.context.view_layer.objects.active = armature
bpy.ops.object.mode_set(mode='EDIT')

root_joint = armature.data.edit_bones.get('_rootJoint')
if root_joint:
    # Get children of _rootJoint
    children = [b for b in armature.data.edit_bones if b.parent == root_joint]
    # Reparent children to None (root level)
    for child in children:
        child.parent = None
        print(f"  Reparented {child.name} to root")
    # Delete _rootJoint
    armature.data.edit_bones.remove(root_joint)
    print("Removed _rootJoint bone")

bpy.ops.object.mode_set(mode='OBJECT')

# Unparent armature from any parent hierarchy
if armature.parent:
    # Store world transform
    world_matrix = armature.matrix_world.copy()
    armature.parent = None
    armature.matrix_world = world_matrix
    print("Unparented armature from hierarchy")

# Find and keep mesh objects, parent them to armature
meshes = [obj for obj in bpy.context.scene.objects if obj.type == 'MESH']
print(f"Found {len(meshes)} mesh objects")

# Delete all other objects except armature and meshes
to_delete = []
for obj in bpy.context.scene.objects:
    if obj != armature and obj not in meshes:
        to_delete.append(obj)

for obj in to_delete:
    bpy.data.objects.remove(obj, do_unlink=True)
print(f"Deleted {len(to_delete)} unnecessary objects")

# Select all for export
bpy.ops.object.select_all(action='SELECT')
bpy.context.view_layer.objects.active = armature

# Select all for export
bpy.ops.object.select_all(action='SELECT')
bpy.context.view_layer.objects.active = armature

# Export as GLB with +Y up axis conversion
print(f"Exporting to: {output_path}")
bpy.ops.export_scene.gltf(
    filepath=output_path,
    export_format='GLB',
    export_animations=True,
    export_skins=True,
    export_morph=True,
    export_apply=False,
    export_yup=True,  # Convert to Y-up coordinate system
)

print("Done!")
print(f"\nNow update skinned_character.rs to use 'y_bot_clean.glb' instead of 'y_bot.glb'")
