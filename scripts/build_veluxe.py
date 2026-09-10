"""Reference-guided jewelry Gaussian authoring. Units: meters. No training claim."""
from pathlib import Path
import numpy as np
from PIL import Image
import json, math, uuid, base64, shutil

OUT=Path(__file__).resolve().parents[1]/'output'/'Veluxe'
OUT.mkdir(parents=True,exist_ok=True)
AS=OUT/'assets'/'splats'; AS.mkdir(parents=True,exist_ok=True)
ref=Path('C:/Users/miksu/AppData/Local/Temp/codex-clipboard-8925a7cb-6165-462d-9682-435fb3d8f53b.png')
im=np.asarray(Image.open(ref).convert('RGB'))/255.
profile=np.array([[0,0],[.015,.0023],[.04,.0039],[.08,.0054],[.13,.00665],[.20,.0075],[.26,.00755],[.34,.0069],[.45,.0055],[.57,.0040],[.7,.00265],[.82,.0018],[.92,.0013],[.97,.0009],[.99,.00055],[1,0]])
# Smooth cubic Hermite interpolation, avoiding a faceted silhouette.
xp,rp=profile.T
sl=np.gradient(rp,xp)
def radius(t):
    k=np.clip(np.searchsorted(xp,t)-1,0,len(xp)-2); d=xp[k+1]-xp[k]; u=(t-xp[k])/d
    return (2*u**3-3*u**2+1)*rp[k]+(u**3-2*u**2+u)*d*sl[k]+(-2*u**3+3*u**2)*rp[k+1]+(u**3-u**2)*d*sl[k+1]
points=[]; colors=[]; normals=[]; scales=[]
for side,cx in enumerate([-.0101,.0101]):
    for t in np.linspace(.0005,.9995,420):
        r=max(1e-6,radius(t)); dr=(radius(min(.99999,t+.0001))-radius(max(.00001,t-.0001)))/.0002/.045
        count=max(12,int(2*np.pi*r/.00014))
        th=np.arange(count)*2*np.pi/count
        x=cx+r*np.cos(th); y=np.full(count,.045*t); z=.62*r*np.sin(th)
        n=np.stack([np.cos(th),np.full(count,-dr),np.sin(th)/.62],axis=1); n/=np.linalg.norm(n,axis=1)[:,None]
        # Photograph supplies the front radiance; hidden surfaces use a gold studio estimate.
        px=np.clip(np.rint((418 if side==0 else 840)+(x-cx)/.045*932).astype(int),0,im.shape[1]-1)
        py=int(np.clip(round(1052-t*932),0,im.shape[0]-1))
        front=im[py,px]
        stripe=.22+.60*np.exp(-((np.cos(th)+.42)/.30)**2)+.2*np.exp(-((np.cos(th)-.7)/.14)**2)
        rear=np.clip(stripe[:,None]*np.array([1,.78,.34])+np.array([.16,.095,.015]),0,1)
        w=np.clip(np.sin(th)*4,0,1)[:,None]; col=front*w+rear*(1-w)
        points.extend(np.stack([x,y,z],axis=1)); normals.extend(n); colors.extend(col); scales.extend([[.000092,.000092,.000027]]*count)
    # Inferred back post and circular clutch: deliberately separate from observed front.
    for z0 in np.linspace(-.001,-.007,48):
        for a in np.linspace(0,2*np.pi,20,endpoint=False):
            points.append([cx+.0004*np.cos(a),.0404+.0004*np.sin(a),z0]); normals.append([np.cos(a),np.sin(a),0]); colors.append([.80,.58,.17]); scales.append([.00010]*3)
    for a in np.linspace(0,2*np.pi,150,endpoint=False):
        for b in np.linspace(0,2*np.pi,16,endpoint=False):
            rr=.0018+.00032*np.cos(b)
            points.append([cx+rr*np.cos(a),.0404+rr*np.sin(a),-.0057+.00032*np.sin(b)]); normals.append([np.cos(a)*np.cos(b),np.sin(a)*np.cos(b),np.sin(b)]); colors.append([.88,.66,.24]); scales.append([.000085]*3)
p=np.array(points,dtype=np.float32); c=np.array(colors,dtype=np.float32); n=np.array(normals,dtype=np.float32); s=np.array(scales,dtype=np.float32)
# Quaternion rotates thin Gaussian's local Z into its surface normal, scalar-first PLY convention.
q=np.column_stack([1+n[:,2],-n[:,1],n[:,0],np.zeros(len(n))]); bad=np.linalg.norm(q,axis=1)<1e-5; q[bad]=[0,1,0,0]; q/=np.linalg.norm(q,axis=1)[:,None]
fields=['x','y','z','nx','ny','nz']+[f'f_dc_{i}' for i in range(3)]+[f'f_rest_{i}' for i in range(45)]+['opacity']+[f'scale_{i}' for i in range(3)]+[f'rot_{i}' for i in range(4)]
data=np.column_stack([p,n,(c-.5)/.28209479177387814,np.zeros((len(p),45)),np.full(len(p),3.8),np.log(s),q]).astype('<f4')
header='ply\nformat binary_little_endian 1.0\ncomment Veluxe Hints; synthetic reference-guided 3DGS; meters\nelement vertex '+str(len(p))+'\n'+''.join('property float '+f+'\n' for f in fields)+'end_header\n'
ply=AS/'aureole-golden-drops.ply'; ply.write_bytes(header.encode()+data.tobytes())
space=OUT/'Spaces'/'Hints'; entity=space/'Workspace'/'Aureole Golden Drops'; entity.mkdir(parents=True,exist_ok=True)
(entity/'_instance.toml').write_text(f'''[metadata]
class_name = "GaussianSplats"
name = "Aureole Golden Drops"
uuid = "{uuid.uuid4().hex}"
archivable = true
unit = "m"

[transform]
position = [0.0, 0.0, 0.0]
rotation = [0.0, 0.0, 0.0, 1.0]
scale = [1.0, 1.0, 1.0]

[gaussian_splats]
path = "assets/splats/aureole-golden-drops.ply"
cull_floaters = false
ppisp = false
''',encoding='utf-8')
manifest={'title':'Aureole | Golden Drops','universe':'Veluxe','space':'Hints','representation':'Synthetic reference-guided anisotropic 3D Gaussian splats, degree-0 radiance padded to SH degree 3','gaussians':len(p),'units':'meters','height_mm':45,'maximum_body_width_mm':round(2*max(radius(t) for t in np.linspace(0,1,1000))*1000,2),'body_depth_mm':9.4,'pair_center_spacing_mm':20.2,'bounds_m':[p.min(0).tolist(),p.max(0).tolist()],'source':'User-supplied single photograph','assumptions':['45 mm height is a design assumption, not measured from the photograph.','Hidden backs, posts, and clutches are inferred.','Gold appearance is baked reference radiance; this is not a measured alloy or physically relightable metal.','Procedural 3DGS authoring, not a multi-view trained reconstruction.','Presentation model, not production CAD or manufacturing certification.']}
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2))
(space/'README.md').write_text('# VELUXE / HINTS\n\n## Aureole — Golden Drops\n\nA study in liquid gold. Two long drops, one uninterrupted gesture.\n\nNative GaussianSplats model: `Workspace/Aureole Golden Drops`. Asset: `assets/splats/aureole-golden-drops.ply` at Universe root. Coordinates are meters, Y-up; the front is +Z. 45 mm assumed height, 20.2 mm center spacing. Open `presentation.html` at Universe root for the interactive splat inspection.\n\n'+ '\n'.join('- '+a for a in manifest['assumptions'])+'\n',encoding='utf-8')
shutil.copyfile(ref,OUT/'reference.png')
# Portable, dependency-free WebGL splat inspector, embedding the actual PLY positions/colors.
packed=np.column_stack([p,c]).astype('<f4')
payload=base64.b64encode(packed.tobytes()).decode()
template=Path(__file__).with_name('veluxe_presentation.html').read_text(encoding='utf-8')
(OUT/'presentation.html').write_text(template.replace('__SPLAT_DATA__',payload).replace('__COUNT__',f'{len(p):,}'),encoding='utf-8')
assert np.isfinite(data).all() and abs((p[:,1].max()-p[:,1].min())-.045)<.0001
assert ply.stat().st_size==len(header.encode())+len(p)*len(fields)*4
print(json.dumps(manifest,indent=2)); print('Validated PLY:',ply,ply.stat().st_size)
