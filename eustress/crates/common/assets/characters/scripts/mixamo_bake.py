"""Bake Mixamo FBX motions into an imported GLB's own bone rest frames."""
import bpy
import re
from mathutils import Matrix, Vector

def canonical(name):
    return re.sub(r'[^a-z0-9]', '', re.sub(r'_\d+$', '', name.split(':')[-1]).lower())

def bake_mixamo(rig, root, sex='Male'):
    scene=bpy.context.scene
    rig.animation_data_clear()
    actions=[]
    for motion,label,filename in [('Idle','Idle','idle'),('Walking','Walk','walking'),('Running','Run','running'),('Jump','Jump','jump')]:
        old=set(scene.objects)
        bpy.ops.import_scene.fbx(filepath=str(root/'animations/scripts'/(sex+' '+motion+'.fbx')),use_anim=True)
        imported=set(scene.objects)-old
        src=next(o for o in imported if o.type=='ARMATURE')
        source={canonical(b.name):b for b in src.pose.bones}
        src_action=src.animation_data.action
        start,end=[int(v) for v in src_action.frame_range]
        rig.animation_data_create(); action=bpy.data.actions.new(label); rig.animation_data.action=action
        ordered=sorted(rig.pose.bones,key=lambda b:len(b.parent_recursive))
        for frame in range(start,end+1):
            scene.frame_set(frame)
            for pb in ordered:
                sb=source.get(canonical(pb.name))
                if not sb: continue
                sr=(src.matrix_world @ sb.bone.matrix_local).to_quaternion()
                sp=(src.matrix_world @ sb.matrix).to_quaternion()
                tr=(rig.matrix_world @ pb.bone.matrix_local).to_quaternion()
                desired=rig.matrix_world.to_quaternion().inverted() @ sp @ sr.inverted() @ tr
                rest=pb.bone.matrix_local
                pos=(pb.parent.matrix @ pb.parent.bone.matrix_local.inverted() @ rest).translation if pb.parent else rest.translation.copy()
                if canonical(pb.name)=='hips':
                    delta=(src.matrix_world @ sb.matrix).translation-(src.matrix_world @ sb.bone.matrix_local).translation
                    pos=rest.translation+rig.matrix_world.to_3x3().inverted() @ delta
                    pos.x=rest.translation.x; pos.y=rest.translation.y
                pb.matrix=Matrix.LocRotScale(pos,desired,Vector((1,1,1)))
                pb.rotation_mode='QUATERNION'
                pb.keyframe_insert('rotation_quaternion',frame=frame-start,group=pb.name)
                pb.keyframe_insert('location',frame=frame-start,group=pb.name)
            bpy.context.view_layer.update()
        action.use_fake_user=True
        actions.append((action,filename,end-start))
        rig.animation_data.action=None
        for o in imported: bpy.data.objects.remove(o,do_unlink=True)
        if src_action.users==0: bpy.data.actions.remove(src_action)
        print('BAKED',label,end-start+1,'frames',flush=True)
    return actions

def install_tracks(rig, actions):
    for action,_,end in actions:
        track=rig.animation_data.nla_tracks.new();track.name=action.name
        strip=track.strips.new(action.name,0,action)
        strip.action_frame_start=0;strip.action_frame_end=end;track.mute=True
    rig.animation_data.action=None
    for pb in rig.pose.bones:pb.matrix_basis.identity()
    bpy.context.scene.frame_set(0)
