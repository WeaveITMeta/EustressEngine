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
        motion_report[label]=len(moved)
    return dict(file=path.name,joints=len(joints),meshes=len(d['meshes']),primitives=sum(len(m['primitives']) for m in d['meshes']),vertices=vertices,triangles=triangles,moving_bones=motion_report)

reports=[validate_model(ROOT/'voltec_supreme.glb')]
for body in ['x_bot','y_bot','voltec_supreme']:
    reports.append(validate_model(WEB/(body+'.glb')))
for motion,label in [('idle','Idle'),('walking','Walk'),('running','Run'),('jump','Jump')]:
    d,b=read_glb(ROOT/'animations'/('robot_'+motion+'.glb'))
    assert len(d['animations'])==1
    assert len(d['animations'][0]['channels'])>=120
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
