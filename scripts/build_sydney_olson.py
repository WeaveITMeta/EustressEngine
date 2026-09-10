"""Parametric prototype jewelry components, STL in mm / Eustress GLB in meters.
Reference-guided design interpretation, not a dimensional scan or casting approval.
"""
from pathlib import Path
import sys,json,math,csv,uuid,zipfile,shutil
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'output'/'veluxe_tooling'))
import numpy as np
import trimesh as tm
from scipy.interpolate import PchipInterpolator

OUT=ROOT/'output'/'VeluxeManufacturing'; OUT.mkdir(parents=True,exist_ok=True)
PARAMS={
 'units':'mm','pendant_leaf_height':28.,'pendant_leaf_width':13.,
 'earring_leaf_height':22.,'earring_leaf_width':9.5,
 'engagement_inner_diameter':17.3,'wedding_inner_diameter':20.,
 'bracelet_link_count':12,'bracelet_centerline_circumference':190.,
 'necklace_chain_length':450.,'necklace_link_count':90,
 'inlay_clearance':.12,'stone_seat_clearance':.08,
 'aureole_height':45.,'aureole_min_wall':.8,'casting_shrink_compensation':0.0}
paramfile=OUT/'parameters.json'
if paramfile.exists(): PARAMS.update(json.loads(paramfile.read_text()))
paramfile.write_text(json.dumps(PARAMS,indent=2))
records=[]; assemblies={}; cache={}
COLORS={'metal':[180,183,189,255],'gold':[214,160,49,255],'gem':[205,225,243,255]}

def move(mesh,p):
 m=mesh.copy(); m.apply_translation(p); return m
def rot(mesh,angle,axis=(0,0,1)):
 m=mesh.copy(); m.apply_transform(tm.transformations.rotation_matrix(angle,axis)); return m
def union(items):
 return tm.boolean.union(items,engine='manifold') if len(items)>1 else items[0]
def subtract(m,items):
 return tm.boolean.difference([m]+items,engine='manifold') if items else m
def box(size,p=(0,0,0)): return move(tm.creation.box(size),p)
def sphere(r,p=(0,0,0)): return move(tm.creation.icosphere(subdivisions=2,radius=r),p)
def cylinder(r,length,p=(0,0,0)): return move(tm.creation.cylinder(radius=r,height=length,sections=40),p)

def tube(path,r=.55,sides=16):
 path=np.array(path); N=len(path); verts=[]; faces=[]
 for i,p in enumerate(path):
  tang=path[(i+1)%N]-path[(i-1)%N]; tang/=np.linalg.norm(tang)
  b=np.array([0.,0.,1.]); b-=tang*np.dot(b,tang)
  if np.linalg.norm(b)<.1: b=np.array([0.,1.,0.]); b-=tang*np.dot(b,tang)
  b/=np.linalg.norm(b); a=np.cross(tang,b)
  for th in np.arange(sides)*2*np.pi/sides: verts.append(p+r*(a*np.cos(th)+b*np.sin(th)))
 for i in range(N):
  for j in range(sides):
   a=i*sides+j;b=i*sides+(j+1)%sides;c=((i+1)%N)*sides+(j+1)%sides;d=((i+1)%N)*sides+j
   faces.extend([[a,b,c],[a,c,d]])
 m=tm.Trimesh(verts,faces,process=True); m.fix_normals(); return m
def oval(rx,ry,r=.5,z=0):
 a=np.arange(96)*2*np.pi/96;return tube(np.c_[rx*np.cos(a),ry*np.sin(a),np.full(96,z)],r)
def leafpath(w,h,z=0):
 a=np.arange(144)*2*np.pi/144
 return np.c_[w/2*np.sin(a)*np.abs(np.sin(a))**.20,h/2*np.cos(a),np.full(len(a),z)]
def prism(poly,z0,z1):
 # These authored contours are convex; fan triangulation with consistent winding.
 poly=np.array(poly);n=len(poly);v=np.vstack([np.c_[poly,np.full(n,z0)],np.c_[poly,np.full(n,z1)]])
 faces=[]
 for i in range(1,n-1):faces.extend([[0,i+1,i],[n,n+i,n+i+1]])
 for i in range(n):j=(i+1)%n;faces.extend([[i,j,n+j],[i,n+j,n+i]])
 m=tm.Trimesh(v,faces,process=True);m.fix_normals();return m
def gemstone(w,h,depth,kind='round'):
 count=16 if kind=='round' else 24
 a=np.arange(count)*2*np.pi/count
 x=w/2*np.cos(a); y=h/2*np.sin(a)
 if kind=='marquise': x*=np.abs(np.cos(a))**.28
 rings=[(.02,-depth*.60),(.98,0),(1,.10*depth),(.52,.40*depth)]
 v=[];f=[]
 for scale,z in rings:v.extend(np.c_[x*scale,y*scale,np.full(count,z)])
 for k in range(3):
  for i in range(count):j=(i+1)%count;f.extend([[k*count+i,k*count+j,(k+1)*count+j],[k*count+i,(k+1)*count+j,(k+1)*count+i]])
 for i in range(1,count-1):f.extend([[0,i+1,i],[3*count,3*count+i,3*count+i+1]])
 m=tm.Trimesh(v,f,process=True);m.fix_normals();return m

def export_component(group,name,mesh,material='metal',notes='',alternative=False):
 mesh=mesh.copy();mesh.remove_unreferenced_vertices();mesh.fix_normals()
 if not mesh.is_watertight or not mesh.is_winding_consistent or mesh.volume<=0:raise RuntimeError(f'Invalid solid {group}/{name}')
 components=len(mesh.split(only_watertight=False))
 if components!=1:raise RuntimeError(f'{group}/{name}: {components} disconnected solids')
 folder=OUT/'STL'/group;folder.mkdir(parents=True,exist_ok=True)
 filename=name+'.stl'; mesh.export(folder/filename)
 # Re-import the delivered STL, verifying actual triangle soup after serialization.
 check=tm.load_mesh(folder/filename,process=True)
 if not check.is_watertight or not check.is_winding_consistent or check.volume<=0:raise RuntimeError('STL round-trip failed: '+name)
 rec={'assembly':group,'component':name,'material':material,'file':f'STL/{group}/{filename}','units':'mm','triangles':len(check.faces),'watertight':True,'connected_solids':components,'volume_mm3':round(float(check.volume),4),'bounds_mm':np.round(check.bounds,4).tolist(),'alternative':alternative,'notes':notes}
 records.append(rec)
 if not alternative:assemblies.setdefault(group,[]).append((name,mesh,material))
 return mesh

def leaf_module(w,h):
 frame=tube(leafpath(w,h),.62,20)
 cross=union([box([1.75,h-1.1,1.15],(0,0,.05)),box([w-1.1,1.8,1.15],(0,h*.10,.05))])
 # Lower gold regions rest on integral backing shelves, surrounded by the frame/cross.
 bases=[];inlays=[]
 for sign in [-1,1]:
  ys=np.linspace(-h*.42,-h*.02,22)
  xs=w/2*(np.maximum(0,1-(2*ys/h)**2))**.60
  poly=np.vstack([[sign*.64,ys[0]],np.c_[sign*xs,ys],[sign*.64,ys[-1]]])
  base=prism(poly,-.40,.15);bases.append(base)
  centroid=np.array([sign*w*.235,-h*.20]);inner=centroid+(poly-centroid)*[.72,.78]
  inlays.append(prism(inner,.15+PARAMS['inlay_clearance'],.55))
 body=union([frame,cross]+bases)
 gems=[];cutters=[];prongs=[]
 # Pavé-like center cross; all stones are independent faceted solids.
 locations=[(0,float(y)) for y in np.arange(-h*.36,h*.39,1.85)]
 locations += [(float(x),h*.10) for x in np.arange(-w*.32,w*.33,1.9) if abs(x)>1.1]
 for x,y in locations:
  stone=move(gemstone(1.32,1.32,.95),[x,y,.88]);gems.append(stone)
  cutters.append(cylinder(.66+PARAMS['stone_seat_clearance'],.67,[x,y,.43]))
  cutters.append(move(gemstone(1.48,1.48,1.07),[x,y,.88]))
  for dx in [-.76,.76]:prongs.append(cylinder(.20,.95,[x+dx,y,.66]))
 body=subtract(union([body]+prongs),cutters)
 return body,inlays,gems

def hanging_leaf(group,w,h,pendant=False):
 body,inlays,gems=leaf_module(w,h)
 top=move(oval(1.25,1.55,.42),[0,h/2+.9,0]);bottom=move(oval(1.1,1.4,.40),[0,-h/2-.8,0])
 # Eyelets meet the leaf tip; union into one castable solid.
 body=union([body,top,bottom])
 export_component(group,'01_leaf_frame_cross_and_prongs',body,notes='Integral frame, backing shelves, eyelets and prongs. Stone seats assumed; jeweler must fit actual stones.')
 for i,m in enumerate(inlays):export_component(group,f'02_gold_inlay_{i+1:02}',m,'gold','Solid decorative inlay carrier; actual gold leaf is applied after finishing. 0.12 mm nominal stand-off.')
 for i,m in enumerate(gems):export_component(group,f'03_cross_diamond_{i+1:02}',m,'gem','Faceted gemstone proxy; do not cast as metal.')
 center=[0,-h/2-6.8,.15]
 drop=move(gemstone(3.6,7.0,2.5,'marquise'),center)
 export_component(group,'04_drop_marquise_gemstone',drop,'gem')
 basket=move(oval(1.85,3.55,.30,z=-.25),center)
 prongs=[cylinder(.3,1.55,[center[0]+x,center[1]+y,.4]) for x,y in [(0,3.35),(0,-3.35),(1.7,0),(-1.7,0)]]
 basket=union([basket]+prongs+[move(oval(.9,1.2,.30),[0,center[1]+4.4,-.1])])
 basket=subtract(basket,[move(gemstone(3.72,7.12,2.62,'marquise'),center)])
 export_component(group,'05_drop_setting',basket)
 export_component(group,'06_lower_jump_ring',move(rot(oval(1.0,1.35,.35),math.pi/2,(0,1,0)),[0,-h/2-2.65,0]),notes='Closed master; slit, assemble and solder/laser-weld after casting.')
 if pendant:
  # Tapered oval bail provides a true through-hole for the chain.
  bail=rot(oval(1.65,3.0,.70),math.pi/2,(0,1,0));bail=move(bail,[0,h/2+4.2,0])
  export_component(group,'07_chain_bail',bail)
 else:
  cy=h/2+6.5
  export_component(group,'07_stud_diamond',move(gemstone(3.2,3.2,2.0),[0,cy,.8]),'gem')
  stud=oval(1.7,1.7,.35,z=-.1)
  stud=union([stud]+[cylinder(.3,1.7,[1.62*math.cos(a),1.62*math.sin(a),.55]) for a in np.arange(6)*math.pi/3]+[cylinder(.45,.8,[0,0,-.3]),box([3.1,.6,.6],[0,0,-.3]),move(oval(.9,1.1,.3),[0,-2.5,-.1])])
  stud=subtract(stud,[move(gemstone(3.34,3.34,2.14),[0,0,.8])])
  export_component(group,'08_stud_setting',move(stud,[0,cy,0]))
  export_component(group,'09_upper_jump_ring',move(rot(oval(.95,1.2,.32),math.pi/2,(0,1,0)),[0,h/2+3.1,0]))
  export_component(group,'10_post',cylinder(.4,9,[0,cy,-5.0]),notes='0.8 mm diameter separate post; solder joint and alloy choice require workshop review.')
  export_component(group,'11_back',move(oval(1.6,1.6,.45),[0,cy,-8.4]),notes='Illustrative back; use a qualified commercial clutch for wearable production.')

def band(inner,width,wall):
 # Comfort-fit band: rounded rectangular section swept around ring axis Z.
 center=inner/2+wall/2
 section=[]
 for cx,cz,start in [(wall/2-.35,width/2-.35,0),(-wall/2+.35,width/2-.35,90),(-wall/2+.35,-width/2+.35,180),(wall/2-.35,-width/2+.35,270)]:
  for deg in np.linspace(start,start+90,12,endpoint=False): section.append([center+cx+.35*np.cos(np.deg2rad(deg)),cz+.35*np.sin(np.deg2rad(deg))])
 section.append(section[0]);m=tm.creation.revolve(np.array(section),sections=192);m.fix_normals();return m

def engagement():
 g='04_Engagement_Ring';inner=PARAMS['engagement_inner_diameter'];R=inner/2
 body=band(inner,2.8,1.65)
 # Basket lies in XZ above the band; supports connect into shoulders.
 basket=move(rot(oval(2.7,5.2,.5),math.pi/2,(1,0,0)),[0,R+1.5,0])
 supports=[box([1.1,3.5,1.1],[x,R+.1,z]) for x in [-2.4,2.4] for z in [-.8,.8]]
 prongs=[move(rot(cylinder(.40,3.8),math.pi/2,(1,0,0)),[x,R+2.4,z]) for x in [-2.0,2.0] for z in [-3.8,3.8]]
 # Connect prongs to basket with short radial struts.
 struts=[box([1.1,1.0,2.0],[x,R+1.45,z]) for x in [-2.,2.] for z in [-3.8,3.8]]
 body=union([body,basket]+supports+prongs+struts)
 stone=move(rot(gemstone(4.5,10,3.3,'marquise'),-math.pi/2,(1,0,0)),[0,R+2.5,0])
 export_component(g,'02_center_marquise_diamond',stone,'gem')
 inlays=[];cuts=[]
 for sign in [-1,1]:
  # Gold accents and round side diamonds follow the upper ring shoulders.
  poly=np.array([[-2.3,0],[0,-.85],[2.3,0],[0,.85]])
  inlay=move(prism(poly,-.2,.2),[sign*5.6,R-.9,1.3]);inlays.append(subtract(inlay,[cylinder(R,30)]))
  seat=move(prism(poly*1.07,-.3,.4),[sign*5.6,R-.9,1.3]);cuts.append(seat)
  # Add contiguous shoulders behind the inserts before cutting the seats.
  shoulder=move(prism(poly*1.17,-1.2,1.6),[sign*5.6,R-.9,0]);body=union([body,shoulder])
  for j,dx in enumerate([-.7,.7]):
   x=sign*4.5+dx;y=R-.05
   gem=move(gemstone(1.15,1.15,.8),[x,y,1.7]);export_component(g,f'04_side_diamond_{sign+2}_{j+1}',gem,'gem')
   # Individual bead seats integrated with shoulder metal.
   collar=move(oval(.67,.67,.18,z=1.48),[x,y,0]);body=union([body,collar])
 cuts.append(move(rot(gemstone(4.66,10.16,3.46,'marquise'),-math.pi/2,(1,0,0)),[0,R+2.5,0]))
 cuts.append(cylinder(R,30))
 body=subtract(body,cuts)
 export_component(g,'01_band_basket_and_prongs',body,notes=f'{inner} mm assumed internal diameter; actual center stone seat must be fitted by setter.')
 for i,m in enumerate(inlays):export_component(g,f'03_gold_inlay_{i+1}',m,'gold')

def bracelet():
 g='03_Bracelet';count=PARAMS['bracelet_link_count'];radius=PARAMS['bracelet_centerline_circumference']/(2*np.pi)
 base,ins,gems=leaf_module(7,12)
 # Marquise modules run tangentially around wrist, decorated face outward in +Z.
 base=union([base,move(oval(.85,1.1,.35),[0,6.7,0]),move(oval(.85,1.1,.35),[0,-6.7,0])])
 for i in range(count):
  a=i*2*np.pi/count
  def place(m):return move(rot(m,a),[radius*np.cos(a),radius*np.sin(a),0])
  export_component(g,f'link_{i+1:02}_frame',place(base))
  for j,m in enumerate(ins):export_component(g,f'link_{i+1:02}_gold_{j+1}',place(m),'gold')
  for j,m in enumerate(gems):export_component(g,f'link_{i+1:02}_diamond_{j+1:02}',place(m),'gem')
  mid=a+np.pi/count
  join=move(rot(rot(oval(.85,1.30,.35),math.pi/2,(0,1,0)),mid),[radius*np.cos(mid),radius*np.sin(mid),0])
  export_component(g,f'connector_{i+1:02}',join,notes='Closed master; split and solder after casting. Link articulation requires workshop fit check.')
 # Separate clasp prototype is supplied off assembly, so no false claim of a functional latch.
 clasp=subtract(box([6,4,2.5]),[box([4.4,4.2,1.1],[0,.3,.2])])
 export_component(g,'clasp_housing_master',move(clasp,[radius+10,0,0]),notes='Off-assembly box-clasp design blank. Requires engineered spring tongue/retention before use.')
 export_component(g,'clasp_tongue_master',box([3.6,5,.6],[radius+18,0,0]),notes='Off-assembly tongue blank; no spring or retention performance implied.')

def chain():
 g='02_Necklace';n=PARAMS['necklace_link_count'];length=PARAMS['necklace_chain_length'];R=length/(2*np.pi)
 # Link pitch ~5 mm, outer length 7.0 mm. Alternating planes permit interlinking.
 proto=oval(1.65,3.1,.4)
 for i in range(n):
  a=-math.pi/2+i*2*np.pi/n
  m=proto.copy()
  if i%2:m=rot(m,math.pi/2,(0,1,0))
  m=rot(m,a);m=move(m,[R*np.cos(a),R+20+R*np.sin(a),0])
  export_component(g,f'chain_link_{i+1:03}',m,notes='Individual closed link master. Print/cast separately; open, interlink and solder. Chain length is nominal centerline length.')
 clasp=move(oval(2.3,4.2,.6),[0,2*R+25,0])
 export_component(g,'chain_clasp_blank',clasp,notes='Decorative clasp blank only. Use a commercial functional clasp or engineer a latch.')

def aureole():
 g='06_Aureole_Golden_Earrings'
 prof=np.array([[0,0],[.015,2.3],[.04,3.9],[.08,5.4],[.13,6.65],[.20,7.5],[.26,7.55],[.34,6.9],[.45,5.5],[.57,4.0],[.7,2.65],[.82,1.8],[.92,1.3],[.97,.9],[.99,.55],[1,0]])
 f=PchipInterpolator(prof[:,0],prof[:,1]);H=PARAMS['aureole_height'];wall=PARAMS['aureole_min_wall']
 def surface(inner=False):
  t=np.linspace(0,1,360);r=f(t);y=t*H
  if inner:
   # Conservative allowance in the compressed depth axis and sloped neck.
   allowance=wall*1.55
   keep=(r>allowance/.62+.10)&(y>allowance)&(y<H-allowance);t=t[keep];r=r[keep]-allowance;y=y[keep]
   # Closed cavity tips; body shell retains at least 0.8 mm radial allowance.
   r=np.r_[0,r,0];y=np.r_[y[0]-.1,y,y[-1]+.1]
  section=np.c_[r,y]
  m=tm.creation.revolve(section,sections=144)
  # revolve uses Z as height. Map to reference Y-up, flatten body depth.
  v=m.vertices.copy();m.vertices=np.c_[v[:,0],v[:,2],v[:,1]*.62]
  m.fix_normals();return m
 solid=surface(); cavity=surface(True)
 # Additional flattening preserves thickness on the back and front walls.
 cavity.vertices[:,2]*=.85
 hollow=subtract(solid,[cavity,cylinder(1.1,5,[0,10,-5.2]),cylinder(1.1,4,[0,31,-3.6])])
 for side,x in [('Left',-10.1),('Right',10.1)]:
  export_component(g,f'{side}_01_hollow_body',move(hollow,[x,0,0]),'gold','Hollow prototype; rear 2.2 mm drain/access ports. Inspect minimum wall and foundry process before casting.')
  export_component(g,f'{side}_02_post',cylinder(.4,9,[x,40.4,-5.8]),'gold','Separate solder-on 0.8 mm post.')
  export_component(g,f'{side}_03_back',move(oval(1.7,1.7,.45),[x,40.4,-8.5]),'gold','Illustrative clutch master; qualified commercial back recommended.')
  export_component(g,f'{side}_SOLID_BODY_ALTERNATIVE',move(solid,[x,0,0]),'gold','Solid alternative; substantially heavier. Do not print alongside hollow body as one part.',True)

def finish():
 # Native Space assemblies, every component independently selectable, authored in meters.
 space=OUT/'Spaces'/'Sydney Olson';ws=space/'Workspace';ws.mkdir(parents=True,exist_ok=True)
 scene_all=tm.Scene();overview=[]
 offsets={'01_Earring_Left':[-65,40,0],'01_Earring_Right':[-40,40,0],'02_Necklace':[30,20,0],'03_Bracelet':[-35,-80,0],'04_Engagement_Ring':[35,-65,0],'05_Wedding_Band':[65,-65,0],'06_Aureole_Golden_Earrings':[95,-25,0]}
 for group,parts in assemblies.items():
  folder=ws/group;folder.mkdir(parents=True,exist_ok=True)
  (folder/'_instance.toml').write_text(f'[metadata]\nclass_name = "Model"\nname = "{group}"\nuuid = "{uuid.uuid5(uuid.NAMESPACE_URL,"veluxe/"+group).hex}"\n[transform]\nposition = [0.0, 0.0, 0.0]\nscale = [1.0, 1.0, 1.0]\n')
  assembly=tm.Scene()
  for name,m,mat in parts:
   mm=m.copy();mm.visual=tm.visual.TextureVisuals(material=tm.visual.material.PBRMaterial(baseColorFactor=COLORS[mat],metallicFactor=0 if mat=='gem' else 1,roughnessFactor=.13 if mat!='gold' else .25))
   assembly.add_geometry(mm,node_name=name,geom_name=name)
   native=mm.copy();native.apply_scale(.001)
   d=folder/name;d.mkdir(parents=True,exist_ok=True);native.export(d/'mesh.glb')
   off=np.array(offsets[group])*.001
   (d/'_instance.toml').write_text(f'[metadata]\nclass_name = "Part"\nname = "{name}"\nuuid = "{uuid.uuid5(uuid.NAMESPACE_URL,"veluxe/"+group+"/"+name).hex}"\n[asset]\nmesh = "mesh.glb"\nscene = "Scene0"\n[transform]\nposition = {off.tolist()}\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [1.0, 1.0, 1.0]\n[properties]\nanchored = true\ncan_collide = false\nmaterial = "Metal"\n')
   mm.apply_translation(offsets[group]);scene_all.add_geometry(mm,node_name=group+'/'+name,geom_name=group+'/'+name)
   overview.append({'assembly':group,'name':name,'material':mat,'bounds':mm.bounds.tolist(),'vertices':np.round(mm.vertices,4).tolist(),'faces':mm.faces.tolist()})
  # GLB spec is meters; STL remains millimeters.
  assembly.apply_scale(.001);p=OUT/'Assemblies';p.mkdir(exist_ok=True);assembly.export(p/(group+'.glb'))
 scene_all.apply_scale(.001);scene_all.export(OUT/'Sydney_Olson_Collection.glb')
 (OUT/'components.json').write_text(json.dumps(records,indent=2))
 with (OUT/'BOM.csv').open('w',newline='') as f:
  writer=csv.DictWriter(f,fieldnames=['assembly','component','material','file','units','triangles','watertight','connected_solids','volume_mm3','alternative','notes']);writer.writeheader();writer.writerows({k:v for k,v in r.items() if k!='bounds_mm'} for r in records)
 (OUT/'preview-data.json').write_text(json.dumps(overview,separators=(',',':')))
 doc='''# VELUXE — Sydney Olson Collection / fabrication prototypes

Reference-guided interpretations of the supplied collection image. These are newly modeled solids, not recovered manufacturing CAD. All dimensions are assumptions until approved against actual finger/wrist sizes and purchased stones.

## Files and units
- STL files are in **millimeters** (STL has no embedded unit metadata). Import at 1:1 mm.
- Every delivered STL contains one connected, watertight, consistently wound solid, verified after re-import.
- Each gemstone, gold inlay, metal body, setting, jump ring, post, clasp blank and chain link is independent.
- Coordinates preserve each assembly; do not auto-center individual files when checking fit. Center and orient each component independently for printing.
- GLB assemblies are in **meters**, with independent named components and materials. Eustress Space parts are also meters.
- BOM.csv and components.json list every part and its volume. parameters.json records editable source dimensions. Run scripts/build_sydney_olson.py to regenerate.

## Proposed sizes
Pendant leaf 28 × 13 mm; earring leaf 22 × 9.5 mm; engagement ring internal diameter 17.3 mm; wedding band internal diameter 20 mm. Bracelet nominal centerline circumference 190 mm; chain nominal centerline length 450 mm. Aureole drop body height 45 mm.

## Process review required
These meshes are suitable for prototype evaluation, **not released for production casting**. Watertightness does not validate stone retention, clearance, spring behavior, strength, comfort, weight, spruing, shrinkage or investment burnout. No shrink allowance, supports or casting sprues have been added. Seats assume generic faceted stone shapes; a jeweler must cut/final-fit them to actual stones. The 0.12 mm inlay allowance and 0.08 mm stone-seat allowance are starting assumptions, not foundry specifications.

The image labels describe aesthetic intent only. Silver-tone metal does not specify a castable alloy. Rhodium is a finish; titanium requires a compatible specialist process. Confirm the actual alloy and process with the workshop. Gold inlays here are solid carrier geometries, not literal micrometer-thick leaf; leaf/adhesive/sealant are finishing operations. Gem STLs are shape proxies to be kept out of the metal casting job.

Closed links are separate masters: slit/open, interlink, then solder or laser-weld as appropriate. Bracelet box-clasp blanks and necklace clasp blank do not include a qualified locking mechanism; use commercial hardware or engineer and test one. Earring backs are illustrative, not spring-qualified clutches. Review linkage clearances and assembly before casting. Aureole hollow bodies include two rear access ports; verify wall thickness, cavity cleanout and weight. Solid alternatives are in the same folder and marked ALTERNATIVE; never merge them with hollow bodies.

## Verification
See validation.json for per-STL topology checks. Visual similarity is interpretive; exact unseen geometry cannot be inferred from one image. No promise of gem grade, alloy purity, wear safety or production readiness is made.
'''
 (OUT/'README.md').write_text(doc,encoding='utf-8');(space/'README.md').write_text(doc,encoding='utf-8')
 shutil.copyfile(Path(__file__).with_name('sydney_olson_presentation.html'),OUT/'presentation.html')
 shutil.copyfile('C:/Users/miksu/AppData/Local/Temp/codex-clipboard-4cd1b858-722c-4f5f-b207-6f0b7a6e210e.png',OUT/'reference.png')
 (OUT/'validation.json').write_text(json.dumps({'stl_count':len(records),'all_watertight':True,'all_single_connected_solid':True,'all_consistent_winding':True,'all_positive_volume':True,'units':'mm','production_release':False,'components':records},indent=2))
 for group in assemblies:
  with zipfile.ZipFile(OUT/(group+'.zip'),'w',zipfile.ZIP_DEFLATED) as z:
   for f in (OUT/'STL'/group).glob('*.stl'):z.write(f,f.relative_to(OUT))
   for n in ['README.md','parameters.json','BOM.csv']:z.write(OUT/n,n)
 with zipfile.ZipFile(OUT/'Veluxe_STL_Collection.zip','w',zipfile.ZIP_DEFLATED) as z:
  for f in (OUT/'STL').rglob('*.stl'):z.write(f,f.relative_to(OUT))
  for n in ['README.md','parameters.json','BOM.csv','validation.json']:z.write(OUT/n,n)
 print(json.dumps({'components':len(records),'assemblies':{k:len(v) for k,v in assemblies.items()},'all_valid':True,'output':str(OUT)},indent=2),flush=True)

if __name__=='__main__':
 for side in ['Left','Right']:
  print('Building earring',side,flush=True);hanging_leaf('01_Earring_'+side,PARAMS['earring_leaf_width'],PARAMS['earring_leaf_height'])
 print('Building pendant',flush=True);hanging_leaf('02_Necklace',PARAMS['pendant_leaf_width'],PARAMS['pendant_leaf_height'],True)
 print('Building chain',flush=True);chain()
 print('Building bracelet',flush=True);bracelet()
 print('Building engagement ring',flush=True);engagement()
 export_component('05_Wedding_Band','01_comfort_fit_band',band(PARAMS['wedding_inner_diameter'],5.,1.8),notes='20 mm assumed bore; 5 mm width; comfort-fit profile.')
 print('Building Aureole',flush=True);aureole()
 finish()
