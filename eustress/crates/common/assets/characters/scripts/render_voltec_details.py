"""Render chest, rear and running-pose inspections from the saved source."""
from pathlib import Path
import sys

import bpy
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[1]
bpy.ops.wm.open_mainfile(filepath=str(ROOT / 'voltec_supreme.blend'))
scene = bpy.context.scene
camera = scene.camera
rig = next(o for o in scene.objects if o.type == 'ARMATURE')
scene.cycles.samples = 48
scene.cycles.use_denoising = True
sole_light_data = bpy.data.lights.new('Sole inspection softbox', 'AREA')
sole_light_data.energy = 180
sole_light_data.shape = 'DISK'
sole_light_data.size = 1.4
sole_light = bpy.data.objects.new('Sole inspection softbox', sole_light_data)
scene.collection.objects.link(sole_light)
sole_light.location = (0, -.7, -1.5)
sole_light.rotation_euler = (Vector((0, 0, .1))-sole_light.location).to_track_quat('-Z','Y').to_euler()
requested = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
for name, location, target, scale, dimensions, action, frame in [
    ('voltec_chest', (.18, -4, 1.52), (0, -.01, 1.39), .77, (1200, 1000), 'Idle', 20),
    ('voltec_back', (-1.8, 5.8, 2), (0, 0, .94), 2.15, (1000, 1200), 'Idle', 20),
    ('voltec_side', (6, 0, 1.55), (0, 0, .94), 2.15, (1000, 1200), 'Idle', 20),
    ('voltec_stomach', (.4, -4, 1.28), (0, 0, 1.15), .69, (1100, 1000), 'Idle', 20),
    ('voltec_back_detail', (-.5, 4, 1.4), (0, .02, 1.22), .85, (1000, 1100), 'Idle', 20),
    ('voltec_boots', (.6, -2, .38), (0, -.04, .14), .58, (1100, 900), 'Idle', 20),
    ('voltec_soles', (.2, -1, -3), (0, -.04, .08), .55, (1000, 1000), 'Idle', 20),
    ('voltec_back_run', (-.5, 4, 1.4), (0, .02, 1.22), .9, (1000, 1100), 'Run', 19),
    ('voltec_seam_walk', (.4, -4, 1.35), (0, 0, 1.25), .64, (900, 900), 'Walk', 31),
    ('voltec_seam_run', (.4, -4, 1.35), (0, 0, 1.25), .64, (900, 900), 'Run', 19),
    ('voltec_seam_jump', (.4, -4, 1.35), (0, 0, 1.25), .64, (900, 900), 'Jump', 30),
    ('voltec_running', (2.8, -5, 2.05), (0, 0, .94), 2.30, (1000, 1200), 'Run', 10),
]:
    if requested and name not in requested:
        continue
    sole_light.hide_render = name != 'voltec_soles'
    for obj in scene.objects:
        if obj.type == 'MESH' and any(m and m.name == 'Review floor' for m in obj.data.materials):
            obj.hide_render = name == 'voltec_soles'
    rig.animation_data.action = bpy.data.actions[action]
    scene.frame_set(frame)
    offset = Vector((0, 0, 0))
    if action == 'Jump':
        hips = next(b for b in rig.pose.bones if 'hips' in b.name.lower())
        posed = (rig.matrix_world @ hips.matrix).translation
        rest = (rig.matrix_world @ hips.bone.matrix_local).translation
        offset.z = posed.z - rest.z
    camera.location = Vector(location) + offset
    camera.rotation_euler = (Vector(target) + offset - camera.location).to_track_quat('-Z', 'Y').to_euler()
    camera.data.ortho_scale = scale
    scene.render.resolution_x, scene.render.resolution_y = dimensions
    scene.render.filepath = str(ROOT / 'voltec_review' / (name + '.png'))
    bpy.ops.render.render(write_still=True)
print('DETAIL REVIEWS COMPLETE', flush=True)
