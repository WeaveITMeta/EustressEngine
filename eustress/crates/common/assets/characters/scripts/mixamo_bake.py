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
    rig_world_q=rig.matrix_world.to_quaternion()
    rig_world_inv3=rig.matrix_world.to_3x3().inverted()
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
        # Last stored quaternion per bone, so consecutive keys stay on one
        # hemisphere. q and -q are the same rotation, but the four components
        # are stored as independent F-curves and interpolate through the
        # origin between them; the exporter samples that swing as a whip.
        prev_q={}
        for frame in range(start,end+1):
            scene.frame_set(frame)
            # Armature-space pose matrices decided THIS frame, parents first.
            #
            # Each child is placed against the parent pose recorded here, never
            # against the depsgraph's view of the parent. Assigning `pb.matrix`
            # reads the parent's LAST EVALUATED pose, and nothing re-evaluates
            # between bones in this loop, so a child's basis absorbed the
            # parent's frame-to-frame delta as error. At frame 1 that delta is
            # the whole rest-to-pose jump, and hands and forearms whipped
            # through 150 to 179 degrees before settling.
            pose={}
            for pb in ordered:
                rest=pb.bone.matrix_local
                if pb.parent:
                    # Where this bone sits at rest, relative to the parent's new pose.
                    rest_from_parent=pose[pb.parent.name] @ pb.parent.bone.matrix_local.inverted() @ rest
                else:
                    rest_from_parent=rest.copy()
                sb=source.get(canonical(pb.name))
                if not sb:
                    # No source motion: hold rest against the (new) parent pose so
                    # its own children are placed correctly.
                    pose[pb.name]=rest_from_parent
                    continue
                sr=(src.matrix_world @ sb.bone.matrix_local).to_quaternion()
                sp=(src.matrix_world @ sb.matrix).to_quaternion()
                tr=(rig.matrix_world @ rest).to_quaternion()
                desired=rig_world_q.inverted() @ sp @ sr.inverted() @ tr
                pos=rest_from_parent.translation.copy()
                if canonical(pb.name)=='hips':
                    delta=(src.matrix_world @ sb.matrix).translation-(src.matrix_world @ sb.bone.matrix_local).translation
                    pos=rest.translation+rig_world_inv3 @ delta
                    pos.x=rest.translation.x; pos.y=rest.translation.y
                desired_mat=Matrix.LocRotScale(pos,desired,Vector((1,1,1)))
                pose[pb.name]=desired_mat
                # pose = rest_from_parent @ basis, so the basis follows directly.
                basis=rest_from_parent.inverted() @ desired_mat
                q=basis.to_quaternion()
                if pb.name in prev_q and q.dot(prev_q[pb.name])<0:
                    q.negate()
                prev_q[pb.name]=q.copy()
                pb.rotation_mode='QUATERNION'
                pb.rotation_quaternion=q
                pb.location=basis.to_translation()
                pb.keyframe_insert('rotation_quaternion',frame=frame-start,group=pb.name)
                pb.keyframe_insert('location',frame=frame-start,group=pb.name)
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
