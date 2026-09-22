"""Structural and motion checks against the actual distributable GLBs.
Run using Python 3; no dependencies required. Fails on frozen clips/unbound mesh.
"""
import json
import math
import re
import struct
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
WEB=ROOT.parents[2]/'web/assets/characters'
TYPES={'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4,'MAT4':16}
COMPONENTS={5120:'b',5121:'B',5122:'h',5123:'H',5125:'I',5126:'f'}

def read_glb(path):
    data=path.read_bytes()
    magic,version,length=struct.unpack_from('<4sII',data)
    assert magic==b'glTF' and version==2 and length==len(data),path
    n,kind=struct.unpack_from('<II',data,12);assert kind==0x4E4F534A
    doc=json.loads(data[20:20+n]);offset=20+n
    size,kind=struct.unpack_from('<II',data,offset);assert kind==0x004E4942
    return doc,data[offset+8:offset+8+size]

def accessor(doc,blob,index):
    a=doc['accessors'][index];v=doc['bufferViews'][a['bufferView']]
    fmt='<'+COMPONENTS[a['componentType']]*TYPES[a['type']]
    stride=v.get('byteStride',struct.calcsize(fmt));offset=v.get('byteOffset',0)+a.get('byteOffset',0)
    return [struct.unpack_from(fmt,blob,offset+i*stride) for i in range(a['count'])]

def canonical(name):
    return re.sub(r'[^a-z0-9]','',re.sub(r'_\d+$','',name.split(':')[-1]).lower())

# Largest rotation any bone may make between consecutive 60 fps keys, in degrees.
# The clean direct-converted Mixamo clips peak at about 30 (a toe mid-stride), so
# this leaves 2x headroom while still failing the defect it guards: the bake once
# placed children against a stale parent pose, which swung hands and forearms
# through 150 to 179 degrees over frames 1 to 4 of every clip and read on screen
# as a twitch once per loop.
MAX_ROTATION_STEP_DEG=60.0

def assert_continuous(doc,blob,animation,label,file):
    """No rotation channel turns more than MAX_ROTATION_STEP_DEG between
    consecutive keys. Returns the worst step seen.

    Measured sign-agnostically: the exporter re-decomposes each sampled pose
    matrix and picks its own quaternion sign, so q on one key and -q on the
    next is routine and means zero motion. Every consumer (three.js in the web
    preview, glam in the engine) slerps the short way, so a sign flip is
    invisible; only the angle between the two rotations reaches the screen."""
    worst=(0.0,None,0)
    for ch in animation['channels']:
        if ch['target']['path']!='rotation':continue
        q=accessor(doc,blob,animation['samplers'][ch['sampler']]['output'])
        bone=canonical(doc['nodes'][ch['target']['node']]['name'])
        for i in range(1,len(q)):
            dot=sum(x*y for x,y in zip(q[i],q[i-1]))
            step=math.degrees(2*math.acos(min(1.0,abs(dot))))
            if step>worst[0]:worst=(step,bone,i)
    assert worst[0]<=MAX_ROTATION_STEP_DEG,(file,label,f'{worst[0]:.1f} deg between frames {worst[2]-1} and {worst[2]} on {worst[1]}')
    return worst[0]

def validate_model(path):
    d,b=read_glb(path)
    if path.stem=='voltec_supreme':
        ceramic=next(m for m in d['materials'] if 'ivory ceramic' in m['name'])
        assert 'normalTexture' in ceramic, 'Ceramic surface detail missing from export'
        assert {'baseColorTexture','metallicRoughnessTexture'}<=ceramic['pbrMetallicRoughness'].keys()
        assert all('bufferView' in image and 'uri' not in image for image in d['images']), 'Unpacked texture'
    assert len(d['skins'])==1
    joints=d['skins'][0]['joints'];assert len(joints)>=65
    names={canonical(d['nodes'][i]['name']) for i in joints}
    required={'hips','head','spine','spine1','spine2'}|{side+bone for side in ['left','right'] for bone in ['arm','forearm','hand','upleg','leg','foot','handindex1','handmiddle2','handthumb3']}
    assert required<=names,required-names
    vertices=triangles=0
    for mesh in d['meshes']:
        for p in mesh['primitives']:
            a=p['attributes'];assert {'POSITION','NORMAL','JOINTS_0','WEIGHTS_0'}<=a.keys()
            if path.stem=='voltec_supreme' and 'normalTexture' in d['materials'][p['material']]:
                assert 'TEXCOORD_0' in a, 'Normal-mapped surface has no UVs'
                mat=d['materials'][p['material']]
                if 'ivory ceramic' in mat['name']:
                    channel=mat['pbrMetallicRoughness']['baseColorTexture'].get('texCoord',0)
                    uv=accessor(d,b,a['TEXCOORD_'+str(channel)])
                    indices=[v[0] for v in accessor(d,b,p['indices'])]
                    covered=0
                    for i in range(0,len(indices),3):
                        u,v,w=(uv[k] for k in indices[i:i+3])
                        area=abs((v[0]-u[0])*(w[1]-u[1])-(w[0]-u[0])*(v[1]-u[1]))
                        covered+=area>1e-12
                    # Narrow shell rims can have zero projected UV area;
                    # the broad armor faces must cover the texture image.
                    assert covered/(len(indices)/3)>.85, 'Ceramic texture coordinates collapsed during mesh join'
            positions=accessor(d,b,a['POSITION']);weights=accessor(d,b,a['WEIGHTS_0']);ids=accessor(d,b,a['JOINTS_0'])
            assert len(positions)==len(weights)==len(ids)
            assert all(all(math.isfinite(v) for v in row) for row in positions)
            assert all(abs(sum(w)-1)<1e-4 and min(w)>=0 for w in weights)
            assert all(all(0<=i<len(joints) for i in row) for row in ids)
            vertices+=len(positions);triangles+=d['accessors'][p['indices']]['count']//3
    animations={a['name']:a for a in d['animations']}
    assert set(animations)=={'Idle','Walk','Run','Jump'},set(animations)
    motion_report={}
    for label,a in animations.items():
        moved=set()
        for ch in a['channels']:
            s=a['samplers'][ch['sampler']];times=accessor(d,b,s['input']);values=accessor(d,b,s['output'])
            assert all(times[i][0]<times[i+1][0] for i in range(len(times)-1))
            assert all(all(math.isfinite(v) for v in row) for row in values)
            if ch['target']['path']=='rotation' and max(sum(abs(x-y) for x,y in zip(values[0],v)) for v in values)>1e-3:
                moved.add(canonical(d['nodes'][ch['target']['node']]['name']))
        assert {'leftarm','rightarm','leftupleg','rightupleg'}<=moved,(label,moved)
        assert len(moved)>=20,(label,len(moved))
        worst_step=assert_continuous(d,b,a,label,path.name)
        motion_report[label]=dict(moving_bones=len(moved),worst_step_deg=round(worst_step,1))
    return dict(file=path.name,joints=len(joints),meshes=len(d['meshes']),primitives=sum(len(m['primitives']) for m in d['meshes']),vertices=vertices,triangles=triangles,moving_bones=motion_report)

reports=[validate_model(ROOT/'voltec_supreme.glb')]
for body in ['x_bot','y_bot','voltec_supreme']:
    reports.append(validate_model(WEB/(body+'.glb')))
for motion,label in [('idle','Idle'),('walking','Walk'),('running','Run'),('jump','Jump')]:
    d,b=read_glb(ROOT/'animations'/('robot_'+motion+'.glb'))
    assert len(d['animations'])==1
    assert len(d['animations'][0]['channels'])>=120
    assert_continuous(d,b,d['animations'][0],label,'robot_'+motion+'.glb')
    # ACTIVE_ACTIONS calls the standalone clip "Animation"; runtime uses #Animation0.
    # Its sampled curves must exactly match the named clip in the model.
    model,model_blob=read_glb(ROOT/'voltec_supreme.glb')
    def curves(doc,blob,animation):
        return {(canonical(doc['nodes'][ch['target']['node']]['name']),ch['target']['path']):
            (accessor(doc,blob,animation['samplers'][ch['sampler']]['input']),accessor(doc,blob,animation['samplers'][ch['sampler']]['output']))
            for ch in animation['channels']}
    embedded=next(a for a in model['animations'] if a['name']==label)
    assert curves(d,b,d['animations'][0])==curves(model,model_blob,embedded),motion
catalog=json.loads((WEB/'rigs.json').read_text())
assert {r['id']:r['identity'] for r in catalog if r['id'] in {'y_bot','x_bot','voltec_supreme'}}=={'y_bot':'M','x_bot':'F','voltec_supreme':'R'}
assert (WEB/'voltec_supreme.glb').read_bytes()==(ROOT/'voltec_supreme.glb').read_bytes()
report=ROOT/'voltec_review/validation.json';report.write_text(json.dumps(reports,indent=2)+'\n')
print(json.dumps(reports,indent=2))
