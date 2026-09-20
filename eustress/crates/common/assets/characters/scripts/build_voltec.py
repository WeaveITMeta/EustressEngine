"""Build Voltec Supreme v2: manufactured armor, skin and Mixamo motion.
Blender 4.4: blender --background --python build_voltec.py
"""
import bpy
import re
import os
import json
import shutil
import sys
from pathlib import Path
from mathutils import Vector
ROOT = Path(__file__).resolve().parents[1]
REVIEW = ROOT / 'voltec_review'
REVIEW.mkdir(exist_ok=True)
sys.path.insert(0, str(Path(__file__).resolve().parent))
from voltec_geometry import build, material
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=str(ROOT / 'y_bot.glb'))
rig = next(o for o in bpy.context.scene.objects if o.type == 'ARMATURE')
rig.name = 'Armature'
def key(name):
    return re.sub(r'[^a-z0-9]', '', re.sub(r'_\d+$', '', name.split(':')[-1]).lower())
bones = {key(b.name): b for b in rig.data.bones}
parts = build(rig, bones)

# Bake each Mixamo motion onto THIS armature using rest-relative rotations.
# Copying fcurve names alone fails when FBX/GLB bone frames differ.
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from mixamo_bake import bake_mixamo, install_tracks
scene=bpy.context.scene
# Merge loose armor pieces into one skinned mesh with material primitives.
# Vertex groups retain the independent rigid bone assignments.
bpy.ops.object.select_all(action='DESELECT')
for o in parts:o.select_set(True)
bpy.context.view_layer.objects.active=parts[0]
bpy.ops.object.join()
parts=[bpy.context.object];parts[0].name='Voltec Supreme • armor and chassis'
actions=bake_mixamo(rig,ROOT)
install_tracks(rig,actions)
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
for track in rig.animation_data.nla_tracks:track.mute=False
bpy.ops.export_scene.gltf(filepath=str(ROOT/'voltec_supreme.glb'),export_format='GLB',use_selection=True,export_animations=True,export_animation_mode='NLA_TRACKS',export_skins=True,export_yup=True,export_force_sampling=True)
# Export only armature + active action into the runtime's single-clip files.
for track in rig.animation_data.nla_tracks:track.mute=True
for action,filename,end in actions:
    rig.animation_data.action=action;scene.frame_start=0;scene.frame_end=end
    bpy.ops.object.select_all(action='DESELECT');rig.select_set(True)
    bpy.ops.export_scene.gltf(filepath=str(ROOT/'animations'/('robot_'+filename+'.glb')),export_format='GLB',use_selection=True,export_animations=True,export_animation_mode='ACTIVE_ACTIONS',export_skins=True,export_yup=True,export_force_sampling=True)
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
