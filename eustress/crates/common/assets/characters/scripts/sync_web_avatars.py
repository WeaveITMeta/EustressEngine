"""Blender: build animated X/Y Bot website previews and copy the Voltec GLB.
Run after build_voltec.py. Engine X/Y source assets remain untouched.
"""
import bpy
import sys
import json
import shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(Path(__file__).resolve().parent))
from mixamo_bake import bake_mixamo, install_tracks
WEB=ROOT.parents[2]/'web/assets/characters'
WEB.mkdir(parents=True,exist_ok=True)
catalog=[]
for identity,body,label,sex,prefix in [('M','y_bot','Y Bot','Male','male'),('F','x_bot','X Bot','Female','female'),('R','voltec_supreme','Voltec Supreme',None,'robot')]:
    catalog.append(dict(id=body,label=label,identity=identity,body_asset=f'bundled://characters/{body}.glb',
        animations=[f'bundled://characters/animations/{prefix}_{m}.glb' for m in ['idle','walking','running','jump']],bone_aliases=[]))
    if sex:
        bpy.ops.wm.read_factory_settings(use_empty=True)
        bpy.ops.import_scene.gltf(filepath=str(ROOT/(body+'.glb')))
        rig=next(o for o in bpy.context.scene.objects if o.type=='ARMATURE')
        original=list(bpy.context.scene.objects)
        actions=bake_mixamo(rig,ROOT,sex);install_tracks(rig,actions)
        bpy.ops.object.select_all(action='DESELECT')
        for o in original:o.select_set(True)
        for track in rig.animation_data.nla_tracks:track.mute=False
        bpy.ops.export_scene.gltf(filepath=str(WEB/(body+'.glb')),export_format='GLB',use_selection=True,
            export_animations=True,export_animation_mode='NLA_TRACKS',export_skins=True,export_yup=True,export_force_sampling=True)
    else:
        shutil.copyfile(ROOT/(body+'.glb'),WEB/(body+'.glb'))
catalog_path=WEB/'rigs.json'
if catalog_path.exists():
    existing=json.loads(catalog_path.read_text(encoding='utf-8'))
    builtin_ids={r['id'] for r in catalog}
    catalog.extend(r for r in existing if r['id'] not in builtin_ids)
catalog_path.write_text(json.dumps(catalog,indent=2)+'\n',encoding='utf-8')
print('WEB AVATARS READY',flush=True)
