"""
Blender script to convert FBX files to GLB format.
Run with: blender --background --python convert_fbx_to_glb.py
"""
import bpy
import os
import sys

# Get the directory where this script is located
script_dir = os.path.dirname(os.path.abspath(__file__))

# List of FBX files to convert
fbx_files = [
    ("Female Idle.fbx", "female_idle.glb"),
    ("Female Jump.fbx", "female_jump.glb"),
    ("Female Running.fbx", "female_running.glb"),
    ("Female Walking.fbx", "female_walking.glb"),
    ("Male Idle.fbx", "male_idle.glb"),
    ("Male Jump.fbx", "male_jump.glb"),
    ("Male Running.fbx", "male_running.glb"),
    ("Male Walking.fbx", "male_walking.glb"),
    ("Sprint.fbx", "sprint.glb"),
]

# Bone name mapping from Mixamo standard to Y-Bot format
# Y-Bot uses "mixamorig:BoneName_XX" format with numeric suffixes
BONE_NAME_MAP = {
    "mixamorig:Hips": "mixamorig:Hips_01",
    "mixamorig:Spine": "mixamorig:Spine_02",
    "mixamorig:Spine1": "mixamorig:Spine1_03",
    "mixamorig:Spine2": "mixamorig:Spine2_04",
    "mixamorig:Neck": "mixamorig:Neck_05",
    "mixamorig:Head": "mixamorig:Head_06",
    "mixamorig:HeadTop_End": "mixamorig:HeadTop_End_07",
    "mixamorig:LeftShoulder": "mixamorig:LeftShoulder_08",
    "mixamorig:LeftArm": "mixamorig:LeftArm_09",
    "mixamorig:LeftForeArm": "mixamorig:LeftForeArm_010",
    "mixamorig:LeftHand": "mixamorig:LeftHand_011",
    "mixamorig:LeftHandThumb1": "mixamorig:LeftHandThumb1_012",
    "mixamorig:LeftHandThumb2": "mixamorig:LeftHandThumb2_013",
    "mixamorig:LeftHandThumb3": "mixamorig:LeftHandThumb3_014",
    "mixamorig:LeftHandThumb4": "mixamorig:LeftHandThumb4_015",
    "mixamorig:LeftHandIndex1": "mixamorig:LeftHandIndex1_016",
    "mixamorig:LeftHandIndex2": "mixamorig:LeftHandIndex2_017",
    "mixamorig:LeftHandIndex3": "mixamorig:LeftHandIndex3_018",
    "mixamorig:LeftHandIndex4": "mixamorig:LeftHandIndex4_019",
    "mixamorig:LeftHandMiddle1": "mixamorig:LeftHandMiddle1_020",
    "mixamorig:LeftHandMiddle2": "mixamorig:LeftHandMiddle2_021",
    "mixamorig:LeftHandMiddle3": "mixamorig:LeftHandMiddle3_022",
    "mixamorig:LeftHandMiddle4": "mixamorig:LeftHandMiddle4_023",
    "mixamorig:LeftHandRing1": "mixamorig:LeftHandRing1_024",
    "mixamorig:LeftHandRing2": "mixamorig:LeftHandRing2_025",
    "mixamorig:LeftHandRing3": "mixamorig:LeftHandRing3_026",
    "mixamorig:LeftHandRing4": "mixamorig:LeftHandRing4_027",
    "mixamorig:LeftHandPinky1": "mixamorig:LeftHandPinky1_028",
    "mixamorig:LeftHandPinky2": "mixamorig:LeftHandPinky2_029",
    "mixamorig:LeftHandPinky3": "mixamorig:LeftHandPinky3_030",
    "mixamorig:LeftHandPinky4": "mixamorig:LeftHandPinky4_031",
    "mixamorig:RightShoulder": "mixamorig:RightShoulder_032",
    "mixamorig:RightArm": "mixamorig:RightArm_033",
    "mixamorig:RightForeArm": "mixamorig:RightForeArm_034",
    "mixamorig:RightHand": "mixamorig:RightHand_035",
    "mixamorig:RightHandThumb1": "mixamorig:RightHandThumb1_036",
    "mixamorig:RightHandThumb2": "mixamorig:RightHandThumb2_037",
    "mixamorig:RightHandThumb3": "mixamorig:RightHandThumb3_038",
    "mixamorig:RightHandThumb4": "mixamorig:RightHandThumb4_039",
    "mixamorig:RightHandIndex1": "mixamorig:RightHandIndex1_040",
    "mixamorig:RightHandIndex2": "mixamorig:RightHandIndex2_041",
    "mixamorig:RightHandIndex3": "mixamorig:RightHandIndex3_042",
    "mixamorig:RightHandIndex4": "mixamorig:RightHandIndex4_043",
    "mixamorig:RightHandMiddle1": "mixamorig:RightHandMiddle1_044",
    "mixamorig:RightHandMiddle2": "mixamorig:RightHandMiddle2_045",
    "mixamorig:RightHandMiddle3": "mixamorig:RightHandMiddle3_046",
    "mixamorig:RightHandMiddle4": "mixamorig:RightHandMiddle4_047",
    "mixamorig:RightHandRing1": "mixamorig:RightHandRing1_048",
    "mixamorig:RightHandRing2": "mixamorig:RightHandRing2_049",
    "mixamorig:RightHandRing3": "mixamorig:RightHandRing3_050",
    "mixamorig:RightHandRing4": "mixamorig:RightHandRing4_051",
    "mixamorig:RightHandPinky1": "mixamorig:RightHandPinky1_052",
    "mixamorig:RightHandPinky2": "mixamorig:RightHandPinky2_053",
    "mixamorig:RightHandPinky3": "mixamorig:RightHandPinky3_054",
    "mixamorig:RightHandPinky4": "mixamorig:RightHandPinky4_055",
    "mixamorig:LeftUpLeg": "mixamorig:LeftUpLeg_056",
    "mixamorig:LeftLeg": "mixamorig:LeftLeg_057",
    "mixamorig:LeftFoot": "mixamorig:LeftFoot_058",
    "mixamorig:LeftToeBase": "mixamorig:LeftToeBase_059",
    "mixamorig:LeftToe_End": "mixamorig:LeftToe_End_060",
    "mixamorig:RightUpLeg": "mixamorig:RightUpLeg_061",
    "mixamorig:RightLeg": "mixamorig:RightLeg_062",
    "mixamorig:RightFoot": "mixamorig:RightFoot_063",
    "mixamorig:RightToeBase": "mixamorig:RightToeBase_064",
    "mixamorig:RightToe_End": "mixamorig:RightToe_End_065",
}

def rename_bones_to_ybot_format(armature):
    """Rename bones in armature to match Y-Bot naming convention."""
    if not armature or armature.type != 'ARMATURE':
        return
    
    # Enter edit mode to rename bones
    bpy.context.view_layer.objects.active = armature
    bpy.ops.object.mode_set(mode='EDIT')
    
    renamed_count = 0
    for bone in armature.data.edit_bones:
        if bone.name in BONE_NAME_MAP:
            new_name = BONE_NAME_MAP[bone.name]
            bone.name = new_name
            renamed_count += 1
    
    bpy.ops.object.mode_set(mode='OBJECT')
    print(f"  Renamed {renamed_count} bones to Y-Bot format")

def convert_fbx_to_glb(fbx_path, glb_path):
    """Convert a single FBX file to GLB with animations."""
    # Clear the scene
    bpy.ops.wm.read_factory_settings(use_empty=True)
    
    # Import FBX with animation
    print(f"Importing: {fbx_path}")
    bpy.ops.import_scene.fbx(
        filepath=fbx_path,
        use_anim=True,
        anim_offset=1.0,
        ignore_leaf_bones=False,
    )
    
    # Ensure we have an armature with animation
    armature = None
    for obj in bpy.context.scene.objects:
        if obj.type == 'ARMATURE':
            armature = obj
            break
    
    if armature and armature.animation_data and armature.animation_data.action:
        action = armature.animation_data.action
        print(f"  Found animation: {action.name} ({action.frame_range[0]}-{action.frame_range[1]})")
    else:
        print("  WARNING: No animation found in FBX!")
    
    # Rename bones to match Y-Bot model
    rename_bones_to_ybot_format(armature)
    
    # Rename the action to a simple name that Bevy can load
    if armature and armature.animation_data and armature.animation_data.action:
        armature.animation_data.action.name = "Animation"
        print("  Renamed action to: Animation")
    
    # Select ALL objects for export (armature + mesh)
    bpy.ops.object.select_all(action='SELECT')
    if armature:
        bpy.context.view_layer.objects.active = armature
    
    # Export as GLB with animations - use ACTIVE_ACTIONS mode
    print(f"Exporting: {glb_path}")
    bpy.ops.export_scene.gltf(
        filepath=glb_path,
        export_format='GLB',
        export_animations=True,
        export_animation_mode='ACTIVE_ACTIONS',  # Export active action, not NLA
        export_skins=True,
        export_morph=True,
        export_apply=False,
        export_rest_position_armature=False,  # Keep current pose
    )
    print(f"Done: {glb_path}\n")

# Convert all files
for fbx_name, glb_name in fbx_files:
    fbx_path = os.path.join(script_dir, fbx_name)
    glb_path = os.path.join(script_dir, glb_name)
    
    if os.path.exists(fbx_path):
        try:
            convert_fbx_to_glb(fbx_path, glb_path)
        except Exception as e:
            print(f"Error converting {fbx_name}: {e}")
    else:
        print(f"Skipping (not found): {fbx_name}")

print("All conversions complete!")
