"""Reference-contour armor authored in the review pose, then unposed to skin.

Image coordinates describe the supplied 1024 x 1536 concept. Each manufactured
plate has a closed curved front, beveled perimeter and a deep structural return.
The single front photograph cannot determine the unseen rear surfaces.
"""
import math
import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def refit(rig, bones, parts):
    ceramic=bpy.data.materials['01 / worn ivory ceramic']
    graphite=bpy.data.materials['04 / recessed graphite']
    metal=bpy.data.materials['03 / satin machined titanium']
    ink=bpy.data.materials['08 / baked graphite markings']
    remove_tokens=['Thorax / formed central','Thorax / structural graphite perimeter',
        'Thorax / continuous fitted','Thorax / separate lateral','Thorax / upper shoulder',
        'Thorax / lower stamped','Thorax / lower locking','shoulder ceramic casting',
        'shoulder rear casting','shoulder panel split','shoulder etched','shoulder service',
        'upper arm ceramic','upper arm plate break','forearm front casting',
        'forearm angular','forearm outer','gauntlet longitudinal','forearm flush',
        'femoral ceramic front','femoral diagonal','femoral lateral','femoral recessed',
        'tibial ceramic casting','tibial angular','tibial lateral','tibial access',
        'articulated patella','patella socket','Abdomen / overlapping chevron',
        'Abdomen / recessed graphite core','Abdomen / captive service',
        'Abdomen / diaphragm armor']
    for obj in list(parts):
        if any(t in obj.name for t in remove_tokens) or obj.name in ['VOLTEC','SUPREME','VS-01']:
            parts.remove(obj);bpy.data.objects.remove(obj,do_unlink=True)
    bpy.context.view_layer.update()
    # Force evaluation of the selected reference action after the bake has
    # finished assigning pose channels by hand. Read evaluated pose matrices.
    bpy.context.view_layer.update()
    evaluated_rig=rig.evaluated_get(bpy.context.evaluated_depsgraph_get())
    transforms={key:rig.matrix_world @ evaluated_rig.pose.bones[b.name].matrix @ b.matrix_local.inverted() @ rig.matrix_world.inverted()
                for key,b in bones.items()}
    backing={}
    depsgraph=bpy.context.evaluated_depsgraph_get()
    for obj in parts:
        if len(obj.vertex_groups)!=1:continue
        group=obj.vertex_groups[0].name
        evaluated=obj.evaluated_get(depsgraph)
        geo=evaluated.to_mesh()
        backing.setdefault(group,[]).extend(evaluated.matrix_world @ v.co for v in geo.vertices)
        evaluated.to_mesh_clear()

    surfaces={}
    def make(name,verts,faces,bone,mat,subdivide=False):
        inv=transforms[bone].inverted()
        data=bpy.data.meshes.new(name);data.from_pydata(verts,[],faces);data.update()
        bm=bmesh.new();bm.from_mesh(data);bmesh.ops.recalc_face_normals(bm,faces=bm.faces);bm.to_mesh(data);bm.free()
        obj=bpy.data.objects.new(name,data);bpy.context.collection.objects.link(obj)
        data.materials.append(mat)
        uv=data.uv_layers.new(name='Surface UV')
        for face in data.polygons:
            face.use_smooth=True
            for li in face.loop_indices:
                p=verts[data.loops[li].vertex_index]
                uv.data[li].uv=(p[0]*2.5,p[2]*2.5)
        if subdivide:
            bpy.ops.object.select_all(action='DESELECT');obj.select_set(True);bpy.context.view_layer.objects.active=obj
            smooth=obj.modifiers.new('Curved casting surface','SUBSURF');smooth.levels=1
            bpy.ops.object.modifier_apply(modifier=smooth.name)
            data=obj.data
        posed_vertices=[v.co.copy() for v in data.vertices]
        posed_faces=[tuple(p.vertices) for p in data.polygons]
        for vertex in data.vertices:vertex.co=inv @ vertex.co
        data.update()
        group=obj.vertex_groups.new(name=bones[bone].name);group.add(list(range(len(data.vertices))),1,'REPLACE')
        mod=obj.modifiers.new('Reference plate skin','ARMATURE');mod.object=rig
        parts.append(obj)
        surfaces.setdefault(bone,[]).append(BVHTree.FromPolygons(posed_vertices,posed_faces))
        return obj

    def outline(points):
        pts=[Vector(p) for p in points]
        for _ in range(2):
            pts=[q for a,b in zip(pts,pts[1:]+pts[:1]) for q in [a*.82+b*.18,a*.18+b*.82]]
        return pts

    def plate(name,points,bone,front,back,dome=.02,mat=ceramic,xmap=None):
        pts=outline(points)
        pts=[Vector(((p.x-512)*.00130 if xmap is None else xmap(p.x),(1430-p.y)*.00136+.025)) for p in pts]
        c=sum(pts,Vector((0,0)))/len(pts);n=len(pts)
        if bone not in ['spine','spine1','spine2']:
            xmin,xmax=min(p.x for p in pts),max(p.x for p in pts)
            zmin,zmax=min(p.y for p in pts),max(p.y for p in pts)
            support=[v.y for v in backing.get(bones[bone].name,[]) if xmin<=v.x<=xmax and zmin<=v.z<=zmax]
            if support:front=min(front,min(support)-.008)
        verts=[]
        # Round the casting through its full depth. The silhouette lies at
        # the side of the housing, not on the face of an extruded stencil.
        depth=max(.025,back-front)
        curvature=min(.052,depth*.25)
        profile=[(.50,back),(.83,back-.010),(1,front+depth*.52),
                 (.985,front+depth*.28),(.94,front+.014),(.78,front-.010),
                 (.48,front-curvature),(.20,front-curvature-.002)]
        for scale,y in profile:
            for p in pts:
                q=c+(p-c)*scale;verts.append((q.x,y,q.y))
        verts.append((c.x,front-curvature-.002,c.y))
        last=len(profile)-1
        faces=[tuple(reversed(range(n)))]+[(j*n+i,j*n+(i+1)%n,(j+1)*n+(i+1)%n,(j+1)*n+i) for j in range(last) for i in range(n)]
        faces += [(last*n+i,last*n+(i+1)%n,len(verts)-1) for i in range(n)]
        return make(name,verts,faces,bone,mat,subdivide=True)

    def socket(name,x,y,z,bone,radius=.014):
        support_trees=list(surfaces.get(bone,[]))
        def surface_y(px,pz):
            points=[tree.ray_cast(Vector((px,-1,pz)),Vector((0,1,0)))[0] for tree in support_trees]
            points=[p for p in points if p is not None]
            return min(p.y for p in points) if points else y
        hits=[tree.ray_cast(Vector((x,-1,z)),Vector((0,1,0)))[0] for tree in support_trees]
        hits=[hit for hit in hits if hit is not None]
        if hits:y=min(hit.y for hit in hits)-.001
        verts=[];faces=[];n=32
        for radius_scale,depth in [(1,.002),(.91,-.002),(.60,-.004),(.48,-.004)]:
            for i in range(n):
                a=math.tau*i/n
                px=x+radius*radius_scale*math.cos(a);pz=z+radius*radius_scale*math.sin(a)
                verts.append((px,surface_y(px,pz)+depth-.003,pz))
        for j in range(3):
            for i in range(n):faces.append((j*n+i,j*n+(i+1)%n,(j+1)*n+(i+1)%n,(j+1)*n+i))
        make(name,verts,faces,bone,metal)
        disc=[]
        for i in range(n):
            px=x+radius*.48*math.cos(math.tau*i/n);pz=z+radius*.48*math.sin(math.tau*i/n)
            disc.append((px,surface_y(px,pz)-.003,pz))
        make(name+' / recessed black pivot',disc,[tuple(range(n))],bone,graphite)

    plate('Reference / broad sternum casting',[(379,391),(433,401),(485,394),(541,394),(590,401),(642,389),
        (670,422),(681,476),(663,522),(626,545),(591,566),(563,547),(548,539),(484,538),
        (464,552),(437,570),(405,549),(375,520),(348,480),(352,438)],'spine2',-.174,.070,.014)
    for body,z,width in [('VOLTEC',1.347,.30),('SUPREME',1.300,.23)]:
        data=bpy.data.curves.new(body,'FONT');data.body=body;data.align_x='CENTER';data.size=.065 if body=='VOLTEC' else .038
        data.font=bpy.data.fonts.load('C:/Windows/Fonts/arialbd.ttf');data.extrude=.00012
        obj=bpy.data.objects.new(body,data);bpy.context.collection.objects.link(obj)
        obj.location=(0,-.192,z);obj.rotation_euler=(math.pi/2,0,0)
        bpy.context.view_layer.update();obj.scale.x=width/obj.dimensions.x
        bpy.ops.object.select_all(action='DESELECT');obj.select_set(True);bpy.context.view_layer.objects.active=obj
        bpy.ops.object.convert(target='MESH');obj=bpy.context.object
        # Subdivide and fit branding to the curved chest instead of placing
        # an independent flat label in front of the armor.
        bm=bmesh.new();bm.from_mesh(obj.data)
        bmesh.ops.triangulate(bm,faces=list(bm.faces))
        bmesh.ops.subdivide_edges(bm,edges=list(bm.edges),cuts=4,use_grid_fill=True)
        bm.to_mesh(obj.data);bm.free()
        verts=[obj.matrix_world @ v.co for v in obj.data.vertices];faces=[tuple(p.vertices) for p in obj.data.polygons]
        for vertex in verts:
            hit=surfaces['spine2'][0].ray_cast(Vector((vertex.x,-1,vertex.z)),Vector((0,1,0)))[0]
            if hit is not None:vertex.y=hit.y-.0015
        bpy.data.objects.remove(obj,do_unlink=True);make(body,verts,faces,'spine2',ink)
    plate('Reference / compact diaphragm',[(439,577),(478,585),(537,585),(577,577),(584,604),(556,629),(469,626),(438,610)],'spine1',-.112,-.030,.009)
    for y in [662,706]:
        plate('Reference / recessed abdominal mechanism',[(467,y-15),(542,y-15),(555,y),(535,y+20),(477,y+20),(457,y)],
              'spine' if y>680 else 'spine1',-.097,-.025,.005,graphite)
    # Left-side source contours are mirrored; leg plates are centered over
    # the existing Mixamo leg pivots rather than moving the animation rig.
    for side,s in [('left',1),('right',-1)]:
        arm=side+'arm';fore=side+'forearm';thigh=side+'upleg';shin=side+'leg'
        armmap=lambda x,s=s:s*(512-x)*.00130
        bicepmap=lambda x,s=s:s*((512-x)*.00130-.025)
        foremap=lambda x,s=s:s*((512-x)*.00130-.065)
        legmap=lambda x,s=s:s*(.101+(385-x)*.00130)
        kneemap=lambda x,s=s:s*(.096+(354-x)*.00130)
        plate('Reference / '+side+' sloped shoulder',[(349,329),(310,325),(266,342),(227,378),(209,419),(209,480),
              (225,505),(264,481),(312,463),(338,431),(350,384)],arm,-.133,.085,.024,xmap=armmap)
        x=armmap(307);z=(1430-414)*.00136+.025
        socket('Reference / '+side+' shoulder socket',x,-.158,z,arm,.020)
        plate('Reference / '+side+' angled bicep',[(265,490),(311,478),(342,506),(348,544),(327,575),(303,566),
              (286,580),(249,567),(226,540),(234,513)],arm,-.100,.085,.012,xmap=bicepmap)
        plate('Reference / '+side+' tapered gauntlet',[(207,601),(230,614),(259,642),(280,620),(299,646),
              (313,722),(304,772),(283,795),(239,787),(198,814),(184,791),(184,733),(191,662)],
              fore,-.114,.075,.014,xmap=foremap)
        plate('Reference / '+side+' swept thigh',[(365,781),(405,807),(442,852),(463,906),(445,951),
              (425,982),(407,966),(389,945),(339,950),(313,934),(320,868),(336,818)],
              thigh,-.097,.073,.014,xmap=legmap)
        plate('Reference / '+side+' knee shield',[(321,978),(368,984),(390,1010),(386,1065),(365,1107),
              (339,1111),(311,1084),(298,1041),(307,1003)],shin,-.094,-.001,.014,xmap=kneemap)
        plate('Reference / '+side+' swept shin',[(414,1055),(438,1101),(438,1142),(414,1199),(401,1243),
              (377,1257),(333,1237),(299,1230),(290,1194),(290,1134),(309,1088),(331,1132),(359,1162),(380,1112)],
              shin,-.093,.076,.012,xmap=kneemap)
        socket('Reference / '+side+' shin port',kneemap(408),-.109,(1430-1101)*.00136+.025,shin,.018)
        plate('Reference / '+side+' lower shin wrap',[(309,1210),(342,1217),(379,1236),(410,1214),
              (421,1250),(404,1284),(376,1300),(339,1285),(302,1282),(290,1253)],
              shin,-.098,.063,.010,xmap=kneemap)
    return parts
