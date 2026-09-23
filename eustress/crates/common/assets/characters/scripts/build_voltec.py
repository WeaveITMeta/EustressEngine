"""Build Voltec Supreme: forged armour, skin and Mixamo motion.
Blender 4.4: blender --background --python build_voltec.py
"""
import bpy
import os
import json
import shutil
import time
import sys
from pathlib import Path
from mathutils import Vector
ROOT = Path(__file__).resolve().parents[1]
REVIEW = ROOT / 'voltec_review'
REVIEW.mkdir(exist_ok=True)
sys.path.insert(0, str(Path(__file__).resolve().parent))
from voltec_forge import (build, curl_fingers, limit_elbows, limit_knees, material, settle_idle, space_arms,
                          turn_palms, widen_skeleton)
from mixamo_bake import bake_mixamo, install_tracks
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=str(ROOT / 'y_bot.glb'))
# Only the donor's skeleton is kept; its body is replaced entirely.
for obj in list(bpy.context.scene.objects):
    if obj.type == 'MESH':
        bpy.data.objects.remove(obj, do_unlink=True)
rig = next(o for o in bpy.context.scene.objects if o.type == 'ARMATURE')
rig.name = 'Armature'
# Spread the limbs BEFORE baking: the bake is rest-relative, so it reproduces
# each clip exactly on the widened skeleton.
widen_skeleton(rig)
scene = bpy.context.scene
actions = bake_mixamo(rig, ROOT)
space_arms(rig, actions)
limit_knees(rig, actions)
turn_palms(rig, actions)
curl_fingers(rig, actions)
settle_idle(rig, actions)
limit_elbows(rig, actions)
install_tracks(rig, actions)
# The armour is authored against the idle pose and returned in bind space.
parts = build(rig, actions[0][0])
# Merge loose armor pieces into one skinned mesh with material primitives.
# Vertex groups retain the independent rigid bone assignments.
bpy.ops.object.select_all(action='DESELECT')
for o in parts:
    # Blender joins UV layers by name. The donor's UVMap previously stayed
    # active while generated armor coordinates landed in an inactive layer.
    # That made all mapped armor sample a single texel in Blender and glTF.
    if o.data.uv_layers:
        o.data.uv_layers.active.name='Surface UV'
        o.data.uv_layers.active.active_render=True
    o.select_set(True)
bpy.context.view_layer.objects.active=parts[0]
bpy.ops.object.join()
parts=[bpy.context.object];parts[0].name='Voltec Supreme • armor and chassis'
# Inspect the actual skinned geometry, not only the animation curves. Invalid
# donor end markers can leave weights valid while sending armor far off-body.
pose_bounds = {}
for action, filename, end in actions:
    rig.animation_data.action = action
    samples = []
    for frame in sorted({0, end//4, end//2, end*3//4, end}):
        scene.frame_set(frame)
        bpy.context.view_layer.update()
        evaluated = parts[0].evaluated_get(bpy.context.evaluated_depsgraph_get())
        evaluated_mesh = evaluated.to_mesh()
        coordinates = [evaluated.matrix_world @ v.co for v in evaluated_mesh.vertices]
        low = [min(p[i] for p in coordinates) for i in range(3)]
        high = [max(p[i] for p in coordinates) for i in range(3)]
        span = [high[i]-low[i] for i in range(3)]
        evaluated.to_mesh_clear()
        assert max(span) < 2.8, (action.name, frame, span)
        samples.append(dict(frame=frame, min=low, max=high, span=span))
    pose_bounds[action.name] = samples
(REVIEW/'pose_bounds.json').write_text(json.dumps(pose_bounds,indent=2)+'\n')
rig.animation_data.action = None
for pb in rig.pose.bones:pb.matrix_basis.identity()
scene.frame_set(0)
bpy.ops.object.select_all(action='DESELECT')
for o in parts+[rig]:o.select_set(True)
bpy.context.view_layer.objects.active=rig
def export_glb(path, **options):
    staged=path.with_name('.'+path.stem+'-'+str(os.getpid())+'.glb')
    bpy.ops.export_scene.gltf(filepath=str(staged), **options)
    if path.exists() and path.read_bytes()==staged.read_bytes():
        staged.unlink()
        return
    for attempt in range(6):
        try:
            os.replace(staged,path)
            return
        except OSError:
            if attempt==5:raise
            time.sleep(.3)
for track in rig.animation_data.nla_tracks:track.mute=False
export_glb(ROOT/'voltec_supreme.glb',export_format='GLB',use_selection=True,export_animations=True,export_animation_mode='NLA_TRACKS',export_skins=True,export_yup=True,export_force_sampling=True)
# Export only armature + active action into the runtime's single-clip files.
for track in rig.animation_data.nla_tracks:track.mute=True
for action,filename,end in actions:
    rig.animation_data.action=action;scene.frame_start=0;scene.frame_end=end
    bpy.ops.object.select_all(action='DESELECT');rig.select_set(True)
    export_glb(ROOT/'animations'/('robot_'+filename+'.glb'),export_format='GLB',use_selection=True,export_animations=True,export_animation_mode='ACTIVE_ACTIONS',export_skins=True,export_yup=True,export_force_sampling=True)
rig.animation_data.action=actions[0][0];scene.frame_set(20)

# Editable source and neutral review stage; nothing from this stage is in GLB.
world=bpy.data.worlds.new('Studio');scene.world=world;world.use_nodes=True
world.node_tree.nodes['Background'].inputs[0].default_value=(.055,.065,.085,1)
world.node_tree.nodes['Background'].inputs[1].default_value=.45
floor=material('Review floor',(.065,.08,.10),.4,.45)
bpy.ops.mesh.primitive_plane_add(size=200,location=(0,0,-.01));bpy.context.object.data.materials.append(floor)
def area(name,loc,power,color,size):
    data=bpy.data.lights.new(name,'AREA');data.energy=power;data.color=color;data.shape='DISK';data.size=size
    o=bpy.data.objects.new(name,data);scene.collection.objects.link(o);o.location=loc;o.rotation_euler=(Vector((0,0,1))-o.location).to_track_quat('-Z','Y').to_euler()
area('Key softbox',(2,-3,4),450,(.85,.92,1),3)
area('Warm fill',(-3,-1,2),250,(1,.85,.69),2)
area('Blue rim',(1,2,3),600,(.35,.6,1),2)
cam=bpy.data.cameras.new('Review camera');camera=bpy.data.objects.new('Review camera',cam);scene.collection.objects.link(camera);scene.camera=camera
camera.location=(1.7,-5.8,2.0);camera.rotation_euler=(Vector((0,0,.94))-camera.location).to_track_quat('-Z','Y').to_euler();cam.type='ORTHO';cam.ortho_scale=2.15
scene.render.engine='CYCLES';scene.cycles.samples=48;scene.cycles.use_denoising=True
scene.render.resolution_x=1000;scene.render.resolution_y=1200;scene.render.resolution_percentage=100
scene.view_settings.view_transform='AgX'
# Punchy keeps black black: plain AgX lifts deep shadows the engine's
# Reinhard and TonyMcMapface tonemappers leave dark.
scene.view_settings.look='AgX - Punchy'
bpy.context.preferences.filepaths.save_version=0
bpy.data.orphans_purge(do_recursive=True)
stage=ROOT/('.voltec-'+str(os.getpid())+'.blend')
bpy.ops.wm.save_as_mainfile(filepath=str(stage),compress=True)
shutil.copyfile(stage,ROOT/'voltec_supreme.blend')
stage.unlink()
scene.render.filepath=str(REVIEW/'voltec_supreme.png');bpy.ops.render.render(write_still=True)
camera.location=(0,-6,1.55);camera.rotation_euler=(Vector((0,0,.94))-camera.location).to_track_quat('-Z','Y').to_euler()
scene.render.filepath=str(REVIEW/'voltec_front.png');bpy.ops.render.render(write_still=True)
print('VOLTEC COMPLETE',flush=True)
