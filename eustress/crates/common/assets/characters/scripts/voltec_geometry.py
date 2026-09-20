"""Voltec's manufactured armor, mechanical chassis and portable PBR materials.

Geometry is authored in the Y Bot bind frame (metres, Blender Z up). Every
armor island belongs to a joint; the original deforming underbody is retained.
All surface maps are packed images, including tangent normals and roughness.
"""
import math
from pathlib import Path

import bmesh
import bpy
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def material(name, color, metal, rough, emission=0):
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*color, 1)
    m.use_nodes = True
    p = m.node_tree.nodes.get('Principled BSDF')
    p.inputs['Base Color'].default_value = (*color, 1)
    p.inputs['Metallic'].default_value = metal
    p.inputs['Roughness'].default_value = rough
    if emission:
        p.inputs['Emission Color'].default_value = (*color, 1)
        p.inputs['Emission Strength'].default_value = emission
    return m


def surface_maps(mat, kind, size=1024):
    """Deterministic multiscale wear; no renderer-only procedural nodes."""
    rng = np.random.default_rng(1731 if kind == 'ceramic' else 8402)
    yy, xx = np.mgrid[:size, :size]
    broad = np.zeros((size, size), dtype=np.float32)
    for frequency, amount in [(3, .4), (11, .25), (37, .12), (101, .045)]:
        phases = rng.uniform(0, 6.28, 4)
        broad += amount * (np.sin(xx / size * frequency * 6.28 + phases[0]) *
                           np.sin(yy / size * frequency * 4.3 + phases[1]) +
                           .4 * np.cos((xx + yy) / size * frequency * 5 + phases[2]))
    grain = rng.normal(0, 1, (size, size)).astype(np.float32)
    damage = np.zeros((size, size), dtype=np.float32)
    # Broken shallow scratches, with occasional dark exposed substrate.
    for _ in range(4200 if kind == 'ceramic' else 700):
        x, y = rng.integers(0, size, 2)
        length = int(rng.integers(3, 64))
        angle = rng.uniform(-2.8, 2.8)
        t = np.arange(length)
        sx = (x + t * math.cos(angle)).astype(int) % size
        sy = (y + t * math.sin(angle)).astype(int) % size
        strength = rng.uniform(.13, .8)
        damage[sy, sx] = strength
        if strength > .57:
            damage[(sy + 1) % size, sx] = strength * .55
    pits = (rng.random((size, size)) > .993).astype(np.float32)
    if kind == 'ceramic':
        grime = np.maximum(0, -broad - .16) ** 1.7
        value = np.clip(.77 + broad * .065 + grain * .009 - damage * .57 - pits * .12 - grime * .21, .12, .91)
        colors = np.stack([value * 1.015, value, value * .975], axis=-1)
        rough = np.clip(.49 + broad * .10 + damage * .32 + grain * .025, .34, .85)
        height = grain * .06 - damage * .95 - pits * .23
    else:
        # Crosswoven elastomer, with subdued metal grain beneath armor gaps.
        weave = np.sin(xx * .72) * np.sin(yy * .72)
        value = np.clip(.043 + broad * .009 + grain * .006 + weave * .009 - damage * .02, .013, .10)
        colors = np.stack([value * .90, value, value * 1.07], axis=-1)
        rough = np.clip(.52 + broad * .08 + weave * .065, .33, .78)
        height = weave * .2 + grain * .035 - damage * .30
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * .45
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * .45
    norm = np.stack([-dx, -dy, np.ones_like(dx)], axis=-1)
    norm /= np.linalg.norm(norm, axis=-1, keepdims=True)
    normals = norm * .5 + .5
    p = mat.node_tree.nodes.get('Principled BSDF')
    for label, data, colorspace in [('albedo', colors, 'sRGB'),
                                     ('roughness', np.repeat(rough[:, :, None], 3, axis=2), 'Non-Color'),
                                     ('normal', normals, 'Non-Color')]:
        image = bpy.data.images.new(f'{mat.name} / {label}', width=size, height=size)
        image.colorspace_settings.name = colorspace
        rgba = np.ones((size, size, 4), dtype=np.float32)
        rgba[:, :, :3] = data
        image.pixels.foreach_set(rgba.ravel())
        image.pack()
        tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
        tex.image = image
        if label == 'normal':
            node = mat.node_tree.nodes.new('ShaderNodeNormalMap')
            node.inputs['Strength'].default_value = .7
            mat.node_tree.links.new(tex.outputs['Color'], node.inputs['Color'])
            mat.node_tree.links.new(node.outputs['Normal'], p.inputs['Normal'])
        else:
            mat.node_tree.links.new(tex.outputs['Color'], p.inputs['Base Color' if label == 'albedo' else 'Roughness'])


def build(rig, bones):
    parts = []
    ceramic = material('01 / worn ivory ceramic', (.77, .76, .73), .24, .49)
    dark = material('02 / carbon composite and flexible seals', (.025, .030, .034), .32, .55)
    titanium = material('03 / satin machined titanium', (.19, .215, .23), .86, .34)
    black = material('04 / recessed graphite', (.009, .013, .018), .45, .42)
    glass = material('05 / recessed cobalt optical glass', (.004, .023, .10), .68, .21, .32)
    light = material('06 / blue optical light guide', (.007, .26, 1), .35, .22, 3.0)
    rubber = material('07 / traction elastomer', (.019, .022, .025), .02, .83)
    ink = material('08 / baked graphite markings', (.012, .017, .020), .05, .60)
    wear = material('09 / exposed coating substrate', (.115, .126, .128), .42, .73)
    surface_maps(ceramic, 'ceramic', 2048)
    surface_maps(dark, 'carbon')

    for obj in list(bpy.context.scene.objects):
        if obj.type != 'MESH':
            continue
        obj.data.materials.clear()
        obj.data.materials.append(dark)
        # Keep the donor's blended weights, but tuck its human musculature
        # inside the chassis instead of letting rounded flesh shapes dominate.
        inverse = obj.matrix_world.inverted()
        for vertex in obj.data.vertices:
            co = obj.matrix_world @ vertex.co
            if abs(co.x) < .22 and .82 < co.z < 1.48:
                co.x *= .88
                co.y = .025 + (co.y - .025) * .80
            elif .57 < co.z < .90:
                cx = .095 if co.x > 0 else -.095
                co.x = cx + (co.x - cx) * .85
                co.y *= .83
            elif co.z < .18:
                cx = .095 if co.x > 0 else -.095
                co.x = cx + (co.x - cx) * .84
                co.y = -.035 + (co.y + .035) * .82
            vertex.co = inverse @ co
        head = obj.vertex_groups.get(bones['head'].name)
        remove = {v.index for v in obj.data.vertices
                  if (head and any(g.group == head.index and g.weight > .5 for g in v.groups))
                  or (obj.matrix_world @ v.co).z < .180
                  or (.89 < (obj.matrix_world @ v.co).z < 1.590
                      and abs((obj.matrix_world @ v.co).x) < .23)}
        if remove:
            bm = bmesh.new()
            bm.from_mesh(obj.data)
            bm.verts.ensure_lookup_table()
            bmesh.ops.delete(bm, geom=[bm.verts[i] for i in remove], context='VERTS')
            bm.to_mesh(obj.data)
            bm.free()
        parts.append(obj)

    def active(obj):
        bpy.ops.object.select_all(action='DESELECT')
        obj.select_set(True)
        bpy.context.view_layer.objects.active = obj

    def skin(obj, bone, mat):
        obj.data.materials.clear()
        obj.data.materials.append(mat)
        active(obj)
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
        world = obj.matrix_world.copy()
        obj.parent = rig
        obj.matrix_world = world
        group = obj.vertex_groups.new(name=bones[bone].name)
        group.add(list(range(len(obj.data.vertices))), 1, 'REPLACE')
        mod = obj.modifiers.new('Joint skin / rigid armor', 'ARMATURE')
        mod.object = rig
        parts.append(obj)
        return obj

    def mesh(name, vertices, faces, bone, mat, uv=None, smooth=True):
        data = bpy.data.meshes.new(name)
        data.from_pydata(vertices, [], faces)
        data.update()
        obj = bpy.data.objects.new(name, data)
        bpy.context.collection.objects.link(obj)
        # Recalculate consistently, including mirrored left/right panels.
        bm = bmesh.new()
        bm.from_mesh(data)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        bm.to_mesh(data)
        bm.free()
        layer = data.uv_layers.new(name='Surface UV')
        for polygon in data.polygons:
            polygon.use_smooth = smooth
            for index in polygon.loop_indices:
                v = data.loops[index].vertex_index
                co = vertices[v]
                layer.data[index].uv = uv[v] if uv else (co[0] * 2.9 + .37, co[2] * 2.9 + .13)
        return skin(obj, bone, mat)

    def curve_points(points, rounds=2, closed=True):
        points = [Vector(p) for p in points]
        for _ in range(rounds):
            new = [] if closed else [points[0]]
            for i in range(len(points) if closed else len(points) - 1):
                a, b = points[i], points[(i + 1) % len(points)]
                new += [a * .75 + b * .25, a * .25 + b * .75]
            if not closed:
                new.append(points[-1])
            points = new
        return points

    def panel(name, outline, y, bone, mat=ceramic, dome=.020, thickness=.012, rounds=2, profile=None):
        """Concentric surfaces give the armor a convex face and rolled edge."""
        points = curve_points(outline, rounds)
        center = sum(points, Vector((0, 0))) / len(points)
        count = len(points)
        vertices = []
        # Outermost radius runs from the rear lip around the rolled edge,
        # across the front face. The dome remains separate from the bevel.
        rings = profile or [(1, thickness), (1.002, .005), (.988, 0), (.955, -.004),
                           (.82, -dome * .55), (.56, -dome * .86), (.27, -dome)]
        for radius, depth in rings:
            for p in points:
                pos = center + (p - center) * radius
                vertices.append((pos.x, y + depth, pos.y))
        vertices.append((center.x, y - dome, center.y))
        faces = []
        for j in range(len(rings) - 1):
            for i in range(count):
                a = j * count + i
                b = j * count + (i + 1) % count
                faces.append((a, b, b + count, a + count))
        for i in range(count):
            faces.append(((len(rings) - 1) * count + i, (len(rings) - 1) * count + (i + 1) % count, len(vertices) - 1))
        faces.append(tuple(reversed(range(count))))
        return mesh(name, vertices, faces, bone, mat)

    def tube(name, points, radius, bone, mat=titanium, sides=10, smooth_path=False):
        if smooth_path:
            points = curve_points(points, 2, closed=False)
        points = [Vector(p) for p in points]
        vertices, uv = [], []
        for i, p in enumerate(points):
            direction = points[min(i + 1, len(points) - 1)] - points[max(0, i - 1)]
            direction.normalize()
            axis = Vector((0, 1, 0)) if abs(direction.y) < .9 else Vector((1, 0, 0))
            u = direction.cross(axis).normalized()
            v = direction.cross(u).normalized()
            for k in range(sides):
                angle = k * 2 * math.pi / sides
                vertices.append(tuple(p + radius * (u * math.cos(angle) + v * math.sin(angle))))
                uv.append((k / sides, i * .13))
        faces = []
        for i in range(len(points) - 1):
            for j in range(sides):
                faces.append((i * sides + j, i * sides + (j + 1) % sides,
                              (i + 1) * sides + (j + 1) % sides, (i + 1) * sides + j))
        faces += [tuple(reversed(range(sides))), tuple((len(points) - 1) * sides + j for j in range(sides))]
        return mesh(name, vertices, faces, bone, mat, uv)

    def sleeve(name, rings, center, bone, mat=ceramic, axis='z', side=1,
               arc=(-math.pi, math.pi), exponent=2.5, thickness=.007):
        """A shaped, hollow shell: rings are (axis position, width, depth).

        Cross sections are rounded rectangles, not spherical primitives.
        Arc gaps reveal actual backing/frame geometry through panel seams.
        """
        # Cubic interpolation removes the stacked-cylinder silhouette while
        # retaining the authored taper and changes in the armor's thickness.
        original = [Vector(r) for r in rings]
        rings = []
        for j in range(len(original)-1):
            p1,p2=original[j:j+2]
            p0=original[j-1] if j else p1*2-p2
            p3=original[j+2] if j+2<len(original) else p2*2-p1
            for t in [0,.25,.5,.75]:
                p=.5*((2*p1)+(-p0+p2)*t+(2*p0-5*p1+4*p2-p3)*t*t+(-p0+3*p1-3*p2+p3)*t*t*t)
                rings.append(tuple(p))
        rings.append(tuple(original[-1]))
        count = 40
        verts, uv = [], []
        full = abs(arc[1] - arc[0] - math.tau) < .001
        for inset in [0, thickness]:
            for j, (distance, width, depth) in enumerate(rings):
                for i in range(count + 1):
                    angle = arc[0] + (arc[1] - arc[0]) * i / count
                    sin, cos = math.sin(angle), math.cos(angle)
                    cross = math.copysign(abs(sin) ** (2 / exponent), sin) * (width - inset)
                    forward = -math.copysign(abs(cos) ** (2 / exponent), cos) * (depth - inset)
                    co = (center[0] + cross, center[1] + forward, distance) if axis == 'z' else (side * distance, center[1] + forward, center[2] + cross)
                    verts.append(co)
                    uv.append((i / count, (distance - rings[0][0]) * 3.1))
        stride = count + 1
        offset = stride * len(rings)
        faces = []
        for inner in [0, offset]:
            for j in range(len(rings) - 1):
                for i in range(count):
                    a = inner + j * stride + i
                    faces.append((a, a + 1, a + 1 + stride, a + stride))
        for j in [0, len(rings) - 1]:
            for i in range(count):
                a = j * stride + i
                faces.append((a, a + 1, a + 1 + offset, a + offset))
        if not full:
            for i in [0, count]:
                for j in range(len(rings) - 1):
                    a = j * stride + i
                    faces.append((a, a + stride, a + stride + offset, a + offset))
        return mesh(name, verts, faces, bone, mat, uv)

    def ring(name, center, radius, bone, mat=dark, normal=(0, 1, 0), minor=.004):
        center, normal = Vector(center), Vector(normal).normalized()
        u = normal.cross(Vector((0, 0, 1)) if abs(normal.z) < .9 else Vector((1, 0, 0))).normalized()
        v = normal.cross(u).normalized()
        points = [center + radius * (u * math.cos(i * math.tau / 40) + v * math.sin(i * math.tau / 40)) for i in range(41)]
        return tube(name, points, minor, bone, mat, sides=8)

    def solid_loft(name, rings, center, bone, mat, exponent=3, arch=False):
        """Closed assembly with shared rim vertices and capped top/bottom."""
        vertices, faces = [], []
        count = 64
        for j, (z, width, depth) in enumerate(rings):
            for i in range(count):
                a = i * math.tau / count
                x = math.copysign(abs(math.sin(a)) ** (2/exponent), math.sin(a))*width
                y = -math.copysign(abs(math.cos(a)) ** (2/exponent), math.cos(a))*depth
                lift = .018*math.exp(-((y-.037)/.044)**2) if arch and j == 0 else 0
                vertices.append((center[0]+x,center[1]+y,z+lift))
        for j in range(len(rings)-1):
            for i in range(count):
                a=j*count+i; b=j*count+(i+1)%count
                faces.append((a,b,b+count,a+count))
        # Subdivide the arched cap radially so its lift follows the footbed
        # throughout the sole, instead of forming long triangular facets.
        bottom_start=0
        if arch:
            z,width,depth=rings[0]
            for scale in [.8,.6,.4,.2]:
                start=len(vertices)
                for i in range(count):
                    outer=vertices[i]
                    x=(outer[0]-center[0])*scale
                    y=(outer[1]-center[1])*scale
                    lift=.018*math.exp(-((y-.037)/.044)**2)
                    vertices.append((center[0]+x,center[1]+y,z+lift))
                for i in range(count):
                    faces.append((bottom_start+i,bottom_start+(i+1)%count,start+(i+1)%count,start+i))
                bottom_start=start
        for j in [0,len(rings)-1]:
            z=rings[j][0]
            lift=.018*math.exp(-(.037/.044)**2) if arch and j==0 else 0
            center_index=len(vertices)
            vertices.append((center[0],center[1],z+lift))
            start=bottom_start if j==0 else j*count
            for i in range(count):
                faces.append((center_index,start+i,start+(i+1)%count))
        obj=mesh(name,vertices,faces,bone,mat)
        bm=bmesh.new(); bm.from_mesh(obj.data)
        assert all(e.is_manifold for e in bm.edges), name
        bm.free()
        return obj

    def rear_panel(name, outline, y, bone, mat=ceramic, **kwargs):
        obj=panel(name,outline,-y,bone,mat,**kwargs)
        for v in obj.data.vertices:v.co.y=-v.co.y
        bm=bmesh.new();bm.from_mesh(obj.data)
        bmesh.ops.recalc_face_normals(bm,faces=bm.faces)
        bm.to_mesh(obj.data);bm.free()
        return obj

    def fastener(name, center, bone, radius=.012, normal=(0, -1, 0)):
        center, normal = Vector(center), Vector(normal).normalized()
        ring(name + ' / socket seat', center, radius, bone, black, normal, radius * .18)
        tube(name + ' / countersunk titanium', [center - normal * .004, center + normal * .001], radius * .66, bone, titanium, sides=12)
        tube(name + ' / hex recess', [center + normal * .0011, center + normal * .0015], radius * .31, bone, black, sides=6)

    surface_trees={}

    def on_face(obj, x, z, offset=.0007):
        # Work in the authored bind mesh. Object.ray_cast includes the current
        # armature pose, which can misplace details before the motion bake.
        if obj.name not in surface_trees:
            surface_trees[obj.name]=BVHTree.FromPolygons(
                [v.co for v in obj.data.vertices],
                [tuple(p.vertices) for p in obj.data.polygons])
        co, normal, _, _ = surface_trees[obj.name].ray_cast(Vector((x,-1,z)), Vector((0,1,0)))
        return (co.x,co.y-offset,co.z) if co is not None else None

    def engraved(name, obj, points, bone, radius=.0009):
        coords=[]
        for p in curve_points(points, 3, closed=False):
            co=on_face(obj,p.x,p.y,.0001)
            if co is None or (coords and (Vector(co)-Vector(coords[-1])).length>.02):
                if len(coords)>1:tube(name,coords,radius,bone,black,sides=8)
                coords=[]
            if co is not None:coords.append(co)
        if len(coords)>1:tube(name,coords,radius,bone,black,sides=8)

    font_path = Path('C:/Windows/Fonts/arialbd.ttf')
    font = bpy.data.fonts.load(str(font_path)) if font_path.exists() else None

    def lettering(body, center, width, bone, size=.03, mat=ink, surface=None):
        data = bpy.data.curves.new(body, 'FONT')
        data.body, data.align_x, data.size = body, 'CENTER', size
        data.space_character = 1.03
        data.extrude = .00012
        if font:
            data.font = font
        obj = bpy.data.objects.new(body, data)
        bpy.context.collection.objects.link(obj)
        obj.location = center
        obj.rotation_euler = (math.pi / 2, 0, 0)
        bpy.context.view_layer.update()
        obj.scale.x = width / max(obj.dimensions.x, .001)
        active(obj)
        bpy.ops.object.convert(target='MESH')
        obj=bpy.context.object
        if surface:
            bm=bmesh.new()
            bm.from_mesh(obj.data)
            bmesh.ops.triangulate(bm,faces=list(bm.faces))
            bmesh.ops.subdivide_edges(bm,edges=list(bm.edges),cuts=2,use_grid_fill=True)
            bm.to_mesh(obj.data)
            bm.free()
            bpy.context.view_layer.update()
            inverse=obj.matrix_world.inverted()
            for vertex in obj.data.vertices:
                world=obj.matrix_world@vertex.co
                projected=on_face(surface,world.x,world.z,.0015)
                if projected:vertex.co=inverse@Vector(projected)
        return skin(obj, bone, mat)

    # Helmet: continuous ivory face surrounds a genuinely recessed Y-shaped
    # aperture. Separate back shell keeps the cheek/jaw silhouette angular.
    outer = [(-.103, 1.786), (-.077, 1.841), (-.039, 1.86), (0, 1.864),
             (.039, 1.86), (.077, 1.841), (.103, 1.786), (.119, 1.749),
             (.116, 1.704), (.094, 1.663), (.071, 1.612), (.043, 1.582),
             (0, 1.574), (-.043, 1.582), (-.071, 1.612), (-.094, 1.663),
             (-.116, 1.704), (-.119, 1.749)]
    aperture = [(-.097, 1.758), (-.078, 1.751), (-.039, 1.735), (0, 1.733),
                (.039, 1.735), (.078, 1.751), (.097, 1.758), (.102, 1.738),
                (.083, 1.713), (.055, 1.699), (.031, 1.676), (.020, 1.614),
                (0, 1.603), (-.020, 1.614), (-.031, 1.676), (-.055, 1.699),
                (-.083, 1.713), (-.102, 1.738)]
    outer = curve_points(outer, 2)
    aperture = curve_points(aperture, 2)
    n = len(outer)
    verts = []
    def face_depth(x, z):
        return -.131 + .30 * x * x + .12 * (z - 1.72) ** 2
    for mix, recess in [(0, .008), (.035, -.002), (.12, -.005), (.45, .004), (.78, .023), (1, .058)]:
        for a, o in zip(aperture, outer):
            p = a.lerp(o, mix)
            verts.append((p.x, face_depth(p.x, p.y) + recess, p.y))
    for shrink, depth in [(1, -.011), (.94, .054), (.69, .105), (.2, .128)]:
        for p in outer:
            verts.append((p.x * shrink, depth, 1.721 + (p.y - 1.721) * shrink))
    faces = [(j * n + i, j * n + (i + 1) % n, (j + 1) * n + (i + 1) % n, (j + 1) * n + i) for j in range(9) for i in range(n)]
    faces.append(tuple(9 * n + i for i in range(n)))
    helmet=mesh('Helmet / sculpted ceramic face and cranial shell', verts, faces, 'head', ceramic)
    for s in [-1,1]:
        engraved('Helmet / crown inset seam',helmet,[(s*.068,1.841),(s*.063,1.802),(s*.047,1.781),(0,1.779)],'head')
        engraved('Helmet / temple panel split',helmet,[(s*.090,1.816),(s*.102,1.781),(s*.112,1.760)],'head')
        engraved('Helmet / jaw inset seam',helmet,[(s*.093,1.692),(s*.068,1.665),(s*.052,1.607),(s*.028,1.594)],'head')
    panel('Helmet / visor seal', [(p.x, p.y) for p in aperture], -.122, 'head', black, dome=0, rounds=0)
    panel('Helmet / optical recess', [(p.x * .94, 1.691 + (p.y - 1.691) * .94) for p in aperture], -.124, 'head', glass, dome=.0005, rounds=0)
    light_outline = [(p.x * .82, -.127, 1.691 + (p.y - 1.691) * .87) for p in aperture]
    tube('Helmet / continuous blue Y light', light_outline + [light_outline[0]], .0018, 'head', light, sides=8)
    for s in [-1, 1]:
        fastener('Helmet / ear pivot', (s*.115, .002, 1.708), 'head', .019, (s, 0, 0))
        for i in range(3):
            tube('Helmet / cheek ventilation', [(s*(.076+i*.007),-.104,1.680+i*.009),(s*(.066+i*.007),-.108,1.659+i*.009)], .0018, 'head', black)
    # Neck is a compact, ribbed gimbal rather than a smooth human neck.
    for j in range(7):
        ring('Cervical flex gaiter', (0,.025,1.508+j*.010), .056, 'neck', dark, (0,0,1), .0045)
    sleeve('Cervical frame', [(1.495,.052,.047),(1.57,.046,.043)], (0,.023,0), 'neck', titanium)

    chest = [(-.075,1.488),(-.142,1.510),(-.188,1.489),(-.216,1.449),
             (-.238,1.395),(-.237,1.356),(-.215,1.318),(-.176,1.290),
             (-.142,1.261),(-.111,1.278),(-.092,1.309),(-.055,1.320),
             (0,1.322),(.055,1.320),(.092,1.309),(.111,1.278),
             (.142,1.261),(.176,1.290),(.215,1.318),(.237,1.356),
             (.238,1.395),(.216,1.449),(.188,1.489),(.142,1.510),(.075,1.488)]
    # Broad planar branding face, a sloping pressed-metal transition and a
    # narrow rounded lip. This replaces the uniformly inflated first version.
    chest_profile=[(1,.064),(1.002,.052),(.990,.045),(.972,.042),
                   (.91,.033),(.80,.013),(.69,.001),(.63,0),(.28,0)]
    # A continuous return connects the face to the thorax instead of leaving
    # a freestanding plate. Both the return and face use the same spine joint.
    face_outline=[(x*.982,1.389+(z-1.389)*.98) for x,z in chest]
    perimeter=curve_points(face_outline,2)
    count=len(perimeter)
    mount_vertices=[]
    for scale,y in [(1,-.108),(.997,-.091),(.93,-.015),(.84,.066)]:
        for p in perimeter:
            mount_vertices.append((p.x*scale,y,1.389+(p.y-1.389)*scale))
    mount_faces=[(j*count+i,j*count+(i+1)%count,(j+1)*count+(i+1)%count,(j+1)*count+i)
                 for j in range(3) for i in range(count)]
    mount_faces.append(tuple(3*count+i for i in range(count)))
    mesh('Thorax / continuous fitted breastplate return',mount_vertices,mount_faces,'spine2',ceramic)
    panel('Thorax / structural graphite perimeter',chest,-.164,'spine2',black,dome=0,
          rounds=2,profile=chest_profile)
    breastplate=panel('Thorax / formed central breastplate',face_outline,
                      -.172,'spine2',ceramic,dome=0,rounds=2,profile=chest_profile)
    lettering('VOLTEC',(0,-.20,1.397),.321,'spine2',.073,surface=breastplate)
    lettering('SUPREME',(0,-.20,1.356),.231,'spine2',.035,surface=breastplate)
    lettering('VS-01',(.167,-.17,1.338),.029,'spine2',.006,surface=breastplate)
    for s in [-1,1]:
        panel('Thorax / separate lateral ceramic skirt',[(s*.177,1.454),(s*.228,1.442),
              (s*.25,1.391),(s*.246,1.327),(s*.199,1.278),(s*.177,1.307)],
              -.080,'spine2',ceramic,dome=.009,thickness=.037,rounds=1)
        engraved('Thorax / upper shoulder transition',breastplate,
                 [(s*.08,1.477),(s*.129,1.483),(s*.17,1.464),(s*.195,1.437)],'spine2',.00065)
        engraved('Thorax / lower stamped edge',breastplate,
                 [(s*.106,1.317),(s*.141,1.29),(s*.174,1.314),(s*.204,1.349)],'spine2',.00065)
    for s in [-1,1]:
        socket=on_face(breastplate,s*.141,1.287,.001)
        if socket:fastener('Thorax / lower locking socket',socket,'spine2',.011)
    # One continuous flexible boot replaces all overlapping torso cylinders.
    # Adjacent rings share vertices and blend between the spine joints, so no
    # independently rotating rims can intersect in the exposed chest opening.
    boot=sleeve('Abdomen / continuous flexible thoracic boot',
           [(.965,.117,.084),(1.014,.116,.085),(1.065,.116,.084),(1.125,.124,.084),
            (1.186,.129,.079),(1.236,.132,.083),(1.275,.135,.092),
            (1.315,.143,.107),(1.353,.154,.117),(1.390,.164,.121)],
           (0,.012,0),'spine1',black,exponent=3.2,thickness=.012)
    boot.vertex_groups.clear()
    groups={name:boot.vertex_groups.new(name=bones[name].name) for name in ['hips','spine','spine1','spine2']}
    def blend_weight(z,start,end):
        t=max(0,min(1,(z-start)/(end-start)))
        return t*t*(3-2*t)
    for vertex in boot.data.vertices:
        z=vertex.co.z
        lower=blend_weight(z,1.075,1.200)
        upper=blend_weight(z,1.235,1.345)
        waist=blend_weight(z,.985,1.080)
        weights={'hips':1-waist,'spine':waist*(1-lower),'spine1':waist*lower*(1-upper),'spine2':waist*lower*upper}
        for bone,weight in weights.items():
            if weight>0:groups[bone].add([vertex.index],weight,'REPLACE')
    panel('Abdomen / diaphragm armor',[(-.096,1.257),(-.106,1.229),(-.074,1.199),
          (.074,1.199),(.106,1.229),(.096,1.257),(.055,1.246),(-.055,1.246)],
          -.092,'spine1',ceramic,dome=.009,thickness=.017,rounds=1)
    for j,(z,width,bone) in enumerate([(1.178,.116,'spine1'),(1.121,.107,'spine'),(1.066,.099,'spine')]):
        outline=[(-width,z+.029),(-width*.96,z-.004),(-width*.66,z-.026),
                 (0,z-.032),(width*.66,z-.026),(width*.96,z-.004),(width,z+.029),
                 (0,z+.017)]
        panel('Abdomen / recessed segment gasket',outline,-.085,bone,black,dome=.007,thickness=.022,rounds=1)
        armor=panel('Abdomen / overlapping chevron armor',[(x*.96,z+(zz-z)*.92) for x,zz in outline],
                    -.093,bone,ceramic,dome=.006,thickness=.018,rounds=1)
        for s in [-1,1]:
            socket=on_face(armor,s*width*.68,z+.004,.001)
            if socket:fastener('Abdomen / captive service screw',socket,bone,.005)
    for s in [-1,1]:
        for z,bone in [(1.083,'spine'),(1.159,'spine1')]:
            # Short actuator modules terminate inside their own housing;
            # nothing bridges separate animated joints as a rigid loose rod.
            tube('Abdomen / recessed actuator housing',[(s*.108,-.033,z-.020),(s*.108,-.033,z+.021)],.012,bone,black)
            tube('Abdomen / inset piston',[(s*.108,-.043,z-.011),(s*.108,-.043,z+.011)],.004,bone,titanium)
            for end in [-1,1]:
                ring('Abdomen / actuator bearing',(s*.108,-.033,z+end*.019),.011,bone,dark,(0,0,1),.003)
        for j in range(3):
            tube('Abdomen / housed cable',[(s*(.113+j*.006),.017,1.108),
                 (s*(.12+j*.006),.018,1.157),(s*(.126+j*.006),.025,1.206)],
                 .0025,'spine1',dark,smooth_path=True)
    solid_loft('Pelvis / closed load bearing hip chassis',
               [(.853,.052,.059),(.892,.104,.080),(.950,.151,.093),(1.011,.148,.092),(1.043,.125,.079)],
               (0,.008,0),'hips',black,exponent=2.6)
    sleeve('Pelvis / continuous waist armor belt',[(.984,.147,.098),(1.014,.151,.098),(1.038,.132,.087)],
           (0,.008,0),'hips',titanium,thickness=.017,exponent=2.8)
    pelvis=[(-.090,1.025),(-.083,.945),(-.052,.866),(0,.845),(.052,.866),(.083,.945),(.090,1.025),(.055,1.010),(-.055,1.010)]
    panel('Pelvis / integrated shield mounting block',pelvis,-.094,'hips',black,dome=.010,thickness=.065)
    panel('Pelvis / formed groin shield',[(x*.93,.941+(z-.941)*.95) for x,z in pelvis],-.107,'hips',dome=.012,thickness=.038)
    for s in [-1,1]:
        panel('Pelvis / iliac wing',[(s*.070,1.023),(s*.164,1.073),(s*.196,1.042),(s*.191,1.001),(s*.099,.969)],-.095,'hips',dome=.018,thickness=.025)
        fastener('Pelvis / recessed actuator',(s*.146,-.115,1.025),'hips',.019)

    for side,s in [('left',1),('right',-1)]:
        arm, fore, hand = side+'arm', side+'forearm', side+'hand'
        thigh, shin, foot = side+'upleg', side+'leg', side+'foot'
        # A shoulder crown tapers into a long upper-arm shield. Gaps between
        # front/rear castings expose the internal satin actuator sleeve.
        sleeve(side+' / shoulder bearing',[(.195,.090,.083),(.265,.094,.088),(.316,.077,.076)],(0,.061,1.438),arm,dark,axis='x',side=s)
        shoulder=[(.101,.007,.007),(.116,.069,.066),(.146,.125,.112),(.191,.157,.140),(.246,.149,.138),(.302,.116,.119),(.325,.080,.090)]
        pauldron=sleeve(side+' / shoulder ceramic casting',shoulder,(0,.061,1.453),arm,axis='x',side=s,arc=(-1.66,1.70),exponent=2.25)
        engraved(side+' / shoulder panel split',pauldron,[(s*.138,1.47),(s*.157,1.502),(s*.204,1.535),(s*.271,1.528)],arm)
        sleeve(side+' / shoulder rear casting',shoulder,(0,.061,1.453),arm,axis='x',side=s,arc=(1.74,4.57),exponent=2.45)
        socket=on_face(pauldron,s*.233,1.460,.004)
        if socket:fastener(side+' / shoulder service socket',socket,arm,.024)
        upper=[(.328,.070,.073),(.34,.079,.080),(.37,.080,.082),(.413,.068,.075),(.427,.054,.061)]
        sleeve(side+' / upper arm titanium barrel',upper,(0,.061,1.436),arm,titanium,axis='x',side=s)
        sleeve(side+' / upper arm ceramic',[(a,b+.006,c+.006) for a,b,c in upper],(0,.061,1.436),arm,axis='x',side=s,arc=(-1.45,1.62))
        # Dense elastomer pleats are thin rings rather than oversized beads.
        for j in range(8):
            x=.432+j*.007
            ring(side+' / elbow gaiter',(s*x,.061,1.436),.052,fore,dark,(1,0,0),.0042)
        fastener(side+' / elbow hinge',(s*.46,-.001,1.436),fore,.029)
        lower=[(.49,.051,.056),(.505,.074,.073),(.53,.082,.085),(.57,.080,.088),(.626,.067,.077),(.677,.053,.062),(.706,.047,.051)]
        sleeve(side+' / forearm graphite frame',[(a,b*.95,c*.95) for a,b,c in lower],(0,.061,1.436),fore,dark,axis='x',side=s)
        sleeve(side+' / forearm front casting',lower,(0,.061,1.436),fore,axis='x',side=s,arc=(-1.42,1.40),exponent=3.1)
        sleeve(side+' / forearm rear casting',lower,(0,.061,1.436),fore,axis='x',side=s,arc=(1.47,4.80),exponent=2.6)
        for zoff in [-.025,.025]:
            tube(side+' / gauntlet longitudinal reveal',[(s*.521,-.027,1.436+zoff),(s*.574,-.031,1.436+zoff),(s*.658,-.008,1.436+zoff*.75)],.0013,fore,black,smooth_path=True)
        for x in [.52,.662]:
            fastener(side+' / forearm flush fastener',(s*x,-.026 if x<.6 else -.002,1.437),fore,.007)
        for j in range(5):
            ring(side+' / wrist seal',(s*(.706+j*.006),.061,1.436),.043,hand,dark,(1,0,0),.0033)
        # Hand armor is compact; all fifteen phalanges remain independently skinned.
        sleeve(side+' / palm dorsal shell',[(.741,.042,.027),(.762,.048,.03),(.801,.044,.030),(.825,.037,.027)],(0,.068,1.433),hand,dark,axis='x',side=s,exponent=3.4)
        for finger in ['thumb','index','middle','ring','pinky']:
            for joint in [1,2,3]:
                bone=side+'hand'+finger+str(joint)
                b=bones[bone]
                a=rig.matrix_world@b.head_local
                nxt=bones.get(side+'hand'+finger+str(joint+1))
                end=rig.matrix_world@nxt.head_local if nxt else a+Vector((s*.022,0,0))
                if (end-a).length > .065 or (end-a).length < .002:
                    # Donor terminal markers have invalid positions. Continue
                    # the preceding real phalanx, never use the marker frame.
                    previous=bones.get(side+'hand'+finger+str(max(1,joint-1)))
                    direction=(a-rig.matrix_world@previous.head_local).normalized()
                    if direction.length < .5:
                        direction=Vector((s,0,0))
                    end=a+direction*.022
                r=.009 if finger!='thumb' else .011
                tube(side+' / '+finger+' phalanx '+str(joint),[a.lerp(end,.13),a.lerp(end,.84)],r,bone,dark,sides=12)
                tube(side+' / '+finger+' dorsal knuckle '+str(joint),[a.lerp(end,.23)+Vector((0,-r*.63,0)),a.lerp(end,.72)+Vector((0,-r*.63,0))],r*.67,bone,titanium,sides=8)
                ring(side+' / '+finger+' joint seal '+str(joint),a,r*.94,bone,black,(end-a).normalized(),.0023)
        # Tapered femoral shells stop before the knee, exposing the actuator.
        femur=[(.608,.065,.066),(.625,.083,.087),(.686,.102,.108),(.785,.107,.116),(.857,.093,.106),(.886,.077,.084)]
        sleeve(side+' / femoral structural housing',[(a,b*.93,c*.93) for a,b,c in femur],(s*.099,.012,0),thigh,dark)
        sleeve(side+' / femoral ceramic front',[(a,b+.005,c+.007) for a,b,c in femur],(s*.099,.012,0),thigh,arc=(-1.42,1.48),exponent=2.7)
        sleeve(side+' / femoral rear shell',femur,(s*.099,.012,0),thigh,arc=(1.56,4.78))
        for j in range(6):
            ring(side+' / knee flex seal',(s*.094,.016,.53+j*.010),.062,shin,dark,(0,0,1),.0045)
        fastener(side+' / knee outer hinge',(s*.165,.017,.527),shin,.026,(s,0,0))
        panel(side+' / articulated patella',[(s*.037,.592),(s*.141,.596),(s*.163,.546),(s*.143,.469),(s*.098,.447),(s*.050,.473)],-.085,shin,dome=.025,thickness=.023)
        fastener(side+' / patella socket',(s*.097,-.11,.494),shin,.009)
        calf=[(.160,.071,.067),(.173,.079,.079),(.23,.082,.087),(.342,.090,.091),(.398,.080,.082),(.452,.068,.068)]
        sleeve(side+' / tibial carbon frame',[(a,b*.92,c*.92) for a,b,c in calf],(s*.095,.031,0),shin,dark)
        sleeve(side+' / tibial ceramic casting',[(a,b+.006,c+.013) for a,b,c in calf],(s*.095,.027,0),shin,arc=(-1.37,1.4),exponent=2.9)
        sleeve(side+' / calf rear ceramic',calf,(s*.095,.034,0),shin,arc=(1.58,4.75))
        for xx in [.048,.141]:
            tube(side+' / calf hydraulic cylinder',[(s*xx,.115,.222),(s*xx,.121,.331)],.010,shin,dark)
            tube(side+' / calf piston',[(s*xx,.116,.175),(s*xx,.122,.241)],.005,shin,titanium)
        fastener(side+' / ankle lateral hinge',(s*.168,.019,.171),foot,.024,(s,0,0))
        # A broad flat outsole and angular toe replace the original oval shoes.
        boot=[(.025,.095,.148),(.034,.099,.151),(.049,.100,.153),(.070,.092,.148),(.106,.082,.124),(.134,.065,.089)]
        solid_loft(side+' / sealed armored boot upper',boot,(s*.095,-.063,0),foot,black,exponent=3.5)
        solid_loft(side+' / closed arched outsole',[(.014,.098,.152),(.035,.102,.155),(.048,.099,.152)],
                   (s*.095,-.063,0),foot,rubber,exponent=3.6,arch=True)
        solid_loft(side+' / embedded arch support shank',[(.039,.055,.075),(.044,.066,.079)],
                   (s*.095,-.003,0),foot,titanium,exponent=3.0)
        # Broad chevron contact pads intersect the closed outsole. The arch
        # has smaller raised pads; heel and forefoot carry the ground plane.
        for j,y in enumerate([-.192,-.160,-.128,-.096,-.064,-.028,.048,.074]):
            for sx in [-1,1]:
                z=.009 if j not in [4,5] else .021
                width=.043 if j==7 else .061 if j in [0,6] else .078
                points=[(s*.095+sx*.012,y-.009),(s*.095+sx*width,y-.001),
                        (s*.095+sx*width,y+.014),(s*.095+sx*.012,y+.006)]
                verts=[(x,yy,zz) for zz in [z,.038] for x,yy in points]
                faces=[(0,3,2,1),(4,5,6,7)]+[(i,(i+1)%4,(i+1)%4+4,i+4) for i in range(4)]
                mesh(side+' / chevron traction pad',verts,faces,foot,rubber,smooth=False)
        solid_loft(side+' / ankle socket seal',[(.108,.059,.061),(.163,.059,.057),(.183,.056,.053)],
                   (s*.095,.014,0),foot,dark,exponent=2.5)
        sleeve(side+' / heel armor cup',[(.055,.088,.131),(.083,.084,.126),(.117,.067,.099)],
               (s*.095,-.056,0),foot,ceramic,arc=(1.65,4.63),thickness=.015,exponent=3.2)
        sleeve(side+' / ankle segmented cuff',[(.111,.073,.077),(.133,.078,.079),(.159,.075,.073),(.177,.068,.063)],(s*.095,.007,0),foot,arc=(-1.45,1.45),exponent=3)
        panel(side+' / toe impact cap',[(s*.019,.094),(s*.027,.061),(s*.165,.061),(s*.176,.094),(s*.148,.116),(s*.048,.116)],-.196,foot,dome=.006,thickness=.025)
        # Bent forefoot plate wraps up over the instep, with a real rolled lip.
        strap_verts=[]
        for dz in [0,-.012]:
            for y,z,width in [(-.181,.096,.083),(-.173,.111,.086),(-.139,.132,.078),(-.128,.129,.074)]:
                for i in range(13):
                    t=i/6-1
                    strap_verts.append((s*.095+t*width,y,z+.014*(1-t*t)+dz))
        strap_faces=[]
        for offset in [0,52]:
            for j in range(3):
                for i in range(12):
                    a=offset+j*13+i
                    strap_faces.append((a,a+1,a+14,a+13))
        for j in [0,3]:
            for i in range(12):
                a=j*13+i
                strap_faces.append((a,a+1,a+53,a+52))
        for i in [0,12]:
            for j in range(3):
                a=j*13+i
                strap_faces.append((a,a+13,a+65,a+52))
        mesh(side+' / formed instep plate',strap_verts,strap_faces,foot,ceramic)
        for j in range(3):
            tube(side+' / instep flexible rib',[(s*.034,-.110+j*.016,.106+j*.009),(s*.151,-.110+j*.016,.106+j*.009)],.006,foot,dark)

    # Complete the unseen rear with service panels, heat-exchanger fins and
    # separately jointed lumbar conduits, consistent with the front design.
    solid_loft('Thorax / enclosed shoulder bridge',
               [(1.350,.167,.106),(1.434,.204,.115),(1.484,.171,.089),(1.517,.087,.062)],
               (0,.020,0),'spine2',black,exponent=2.7)
    sleeve('Thorax / fitted dorsal shell',[(1.264,.154,.107),(1.31,.198,.128),
           (1.418,.205,.133),(1.476,.169,.108),(1.511,.082,.066)],
           (0,.025,0),'spine2',arc=(1.48,4.80),exponent=2.9,thickness=.018)
    for s in [-1,1]:
        rear_panel('Thorax / scapular armor',[(s*.038,1.466),(s*.095,1.492),
                   (s*.177,1.456),(s*.190,1.390),(s*.132,1.314),(s*.054,1.337)],
                   .150,'spine2',dome=.009,thickness=.045,rounds=1)
    rear_panel('Thorax / recessed dorsal service cassette',
               [(-.052,1.440),(-.051,1.339),(0,1.310),(.051,1.339),(.052,1.440)],
               .174,'spine2',black,dome=0,thickness=.050,rounds=1)
    for j in range(7):
        z=1.346+j*.012
        tube('Dorsal heat exchanger / seated fin',[(-.038,.175,z),(.038,.175,z)],.003,'spine2',titanium)
    for z,width,bone in [(1.261,.139,'spine1'),(1.197,.130,'spine1'),
                          (1.133,.121,'spine'),(1.069,.114,'spine')]:
        outline=[(-width,z+.030),(-width*.93,z-.012),(-.046,z-.032),
                 (.046,z-.032),(width*.93,z-.012),(width,z+.030),(0,z+.014)]
        rear_panel('Lumbar / overlapping vertebral armor',outline,
                   .109 if z<1.21 else .124,bone,ceramic,dome=.006,thickness=.030,rounds=1)
    rear_panel('Pelvis / sacral armor anchored to hip chassis',
               [(-.118,1.025),(-.133,.978),(-.091,.903),(0,.881),(.091,.903),(.133,.978),(.118,1.025)],
               .103,'hips',ceramic,dome=.008,thickness=.040,rounds=1)
    # Thin, surface-conforming paint chips make wear readable at avatar scale.
    # They remain ordinary skinned triangles, supported by both GLB renderers.
    rng=np.random.default_rng(662091)
    for target in list(parts):
        if not target.data.materials or target.data.materials[0]!=ceramic:
            continue
        coords=[v.co for v in target.data.vertices]
        xmin,xmax=min(v.x for v in coords),max(v.x for v in coords)
        zmin,zmax=min(v.z for v in coords),max(v.z for v in coords)
        count=int((xmax-xmin)*(zmax-zmin)*1250)
        verts,faces=[],[]
        for _ in range(count):
            x,z=rng.uniform(xmin,xmax),rng.uniform(zmin,zmax)
            length=rng.uniform(.002,.012)
            width=rng.uniform(.00015,.00065)
            angle=rng.uniform(-math.pi,math.pi)
            dx,dz=math.cos(angle),math.sin(angle)
            polygon=[]
            for u,v in [(-.5,0),(-.32,.8),(.18,.5),(.5,0),(.03,-.7)]:
                hit=on_face(target,x+dx*length*u-dz*width*v,z+dz*length*u+dx*width*v,.0003)
                if hit:polygon.append(hit)
            if len(polygon)==5:
                first=len(verts)
                verts.extend(polygon)
                faces.append(tuple(range(first,first+5)))
        if faces:
            group_name=target.vertex_groups[0].name
            bone=next(k for k,b in bones.items() if b.name==group_name)
            mesh(target.name+' / chipped coating',verts,faces,bone,wear,smooth=False)
    print('VOLTEC GEOMETRY',len(parts),'joint-bound islands',flush=True)
    return parts
