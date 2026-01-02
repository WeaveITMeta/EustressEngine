"""
Blender script to merge FBX animations into the Y-Bot/X-Bot GLB files.
This embeds all animations into the model file so Bevy can load them correctly.

Run with: blender --background --python merge_animations.py
"""
import bpy
import os

script_dir = os.path.dirname(os.path.abspath(__file__))
characters_dir = os.path.dirname(script_dir)
animations_dir = os.path.join(characters_dir, "animations", "scripts")

# Animation files to merge (FBX name, action name for Bevy)
ANIMATIONS = [
    ("Female Idle.fbx", "Idle"),
    ("Female Walking.fbx", "Walk"),
    ("Female Running.fbx", "Run"),
    ("Sprint.fbx", "Sprint"),
    ("Female Jump.fbx", "Jump"),
]

def merge_animations_into_model(model_glb_path, output_glb_path, animation_files):
    """Load model, import animations from FBX files, export combined GLB."""
    
    # Clear scene
    bpy.ops.wm.read_factory_settings(use_empty=True)
    
    # Import the base model
    print(f"Importing model: {model_glb_path}")
    bpy.ops.import_scene.gltf(filepath=model_glb_path)
    
    # Find the armature
    armature = None
    for obj in bpy.context.scene.objects:
        if obj.type == 'ARMATURE':
            armature = obj
            break
    
    if not armature:
        print("ERROR: No armature found in model!")
        return
    
    print(f"Found armature: {armature.name}")
    
    # Store all actions
    actions = []
    
    # Import each animation FBX
    for fbx_name, action_name in animation_files:
        fbx_path = os.path.join(animations_dir, fbx_name)
        if not os.path.exists(fbx_path):
            print(f"  Skipping (not found): {fbx_name}")
            continue
        
        print(f"Importing animation: {fbx_name}")
        
        # Import FBX
        bpy.ops.import_scene.fbx(
            filepath=fbx_path,
            use_anim=True,
            ignore_leaf_bones=False,
        )
        
        # Find the imported armature and its action
        for obj in bpy.context.scene.objects:
            if obj.type == 'ARMATURE' and obj != armature:
                if obj.animation_data and obj.animation_data.action:
                    action = obj.animation_data.action
                    action.name = action_name
                    actions.append(action)
                    print(f"  Found action: {action_name} ({action.frame_range[0]}-{action.frame_range[1]})")
                
                # Delete the imported armature (we only want the action)
                bpy.data.objects.remove(obj, do_unlink=True)
                break
    
    # Ensure the main armature has animation data
    if not armature.animation_data:
        armature.animation_data_create()
    
    # Push all actions to NLA tracks
    for i, action in enumerate(actions):
        track = armature.animation_data.nla_tracks.new()
        track.name = action.name
        strip = track.strips.new(action.name, int(action.frame_range[0]), action)
        strip.name = action.name
        print(f"Added NLA track: {action.name}")
    
    # Set the first action as active (for preview)
    if actions:
        armature.animation_data.action = actions[0]
    
    # Select all objects for export
    bpy.ops.object.select_all(action='SELECT')
    bpy.context.view_layer.objects.active = armature
    
    # Export as GLB with all animations
    print(f"Exporting: {output_glb_path}")
    bpy.ops.export_scene.gltf(
        filepath=output_glb_path,
        export_format='GLB',
        export_animations=True,
        export_animation_mode='NLA_TRACKS',  # Export all NLA tracks as separate animations
        export_skins=True,
        export_morph=True,
        export_apply=False,
    )
    print(f"Done! Exported {len(actions)} animations")


# Process Y-Bot (female)
ybot_input = os.path.join(characters_dir, "y_bot.glb")
ybot_output = os.path.join(characters_dir, "y_bot_animated.glb")
merge_animations_into_model(ybot_input, ybot_output, ANIMATIONS)

print("\nMerge complete!")
print(f"Output: {ybot_output}")
print("Update skinned_character.rs to use 'y_bot_animated.glb' instead of 'y_bot.glb'")
