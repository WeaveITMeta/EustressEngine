"""Render climb poses on the Y Bot, the way the engine lays them down.

Run by `climb_scenes.py`, or by hand:

    blender -b --factory-startup -P render.py -- <scenes.json> <out_dir> [<rig.glb>]

The engine authors each pose as the direction every bone should POINT in the
body frame (+X right, +Y up, +Z back; -Z is forward, into the wall), applies
the entries in list order, then solves two-bone IK for the hands and feet.
This script does the same on the same rig so a pose can be judged by eye
before it reaches the engine.

Blender frame for the imported Y Bot: the character faces -Y, its left is +X,
up is +Z. So body (x, y, z) maps to Blender (-x, z, y).
"""
import bpy, json, math, os, re, sys
from mathutils import Vector, Quaternion, Matrix

args = sys.argv[sys.argv.index("--") + 1:]
scenes_path, out_dir = args[0], args[1]
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
GLB = args[2] if len(args) > 2 else os.path.join(
    REPO, "eustress", "crates", "common", "assets", "characters", "y_bot.glb"
)
scenes = json.load(open(scenes_path))

MIN_EXT, MAX_EXT = 0.58, 0.97


def body(v):
    return Vector((-v[0], v[2], v[1]))


def canon(n):
    n = n.split(':')[-1].split('|')[-1]
    n = re.sub(r'_\d+$', '', n)
    return re.sub(r'[^A-Za-z0-9]', '', n).lower()


def setup():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=GLB)
    for o in list(bpy.data.objects):
        if o.name == "Icosphere":
            bpy.data.objects.remove(o, do_unlink=True)
    arm = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
    # The import carries the Mixamo clip as an action, which re-poses the rig
    # on every evaluation and would overwrite anything set here.
    arm.animation_data_clear()
    names = {canon(pb.name): pb.name for pb in arm.pose.bones}
    return arm, names


arm, NAMES = setup()
BONE = lambda c: arm.pose.bones[NAMES[c.lower()]]


def update():
    bpy.context.view_layer.update()


def head(c):
    return arm.matrix_world @ BONE(c).head


def rotate_bone_world(c, delta):
    """Rotate bone `c` by the world rotation `delta` about its own head."""
    pb = BONE(c)
    h = arm.matrix_world @ pb.head
    M = arm.matrix_world @ pb.matrix
    T = Matrix.Translation(h)
    newM = T @ delta.to_matrix().to_4x4() @ T.inverted() @ M
    pb.matrix = arm.matrix_world.inverted() @ newM
    update()


def aim(bone, child, want, w=1.0):
    update()
    cur = (head(child) - head(bone))
    if cur.length < 1e-8:
        return
    d = cur.normalized().rotation_difference(want.normalized())
    d = Quaternion().slerp(d, max(0.0, min(1.0, w)))
    rotate_bone_world(bone, d)


def reset():
    for pb in arm.pose.bones:
        pb.rotation_mode = 'QUATERNION'
        pb.rotation_quaternion = Quaternion()
        pb.location = Vector()
        pb.scale = Vector((1, 1, 1))
    update()


def apply_pose(entries, w=1.0, mirror=False, frame=Quaternion()):
    for bone, child, d in entries:
        if mirror:
            bone, child, d = mirrored(bone), mirrored(child), [-d[0], d[1], d[2]]
        aim(bone, child, frame @ body(d), w)


def mirrored(n):
    if n.startswith("Left"):
        return "Right" + n[4:]
    if n.startswith("Right"):
        return "Left" + n[5:]
    return n


def solve_two_bone(root, target, ul, ll, pole):
    total = ul + ll
    to_t = target - root
    dist = to_t.length
    if dist < 1e-5:
        return Vector((0, 0, -1)), Vector((0, 0, -1))
    d = to_t / dist
    if dist >= total * 0.999:
        return d, d
    dist = max(dist, abs(ul - ll) * 1.001)
    cos_r = max(-1.0, min(1.0, (ul * ul + dist * dist - ll * ll) / (2 * ul * dist)))
    ang = math.acos(cos_r)
    axis = d.cross(pole - root)
    if axis.length < 1e-6:
        axis = d.cross(Vector((0, 1, 0)))
    axis.normalize()
    up = Quaternion(axis, ang) @ d
    joint = root + up * ul
    lo = (target - joint).normalized()
    return up, lo


def two_bone(up_b, lo_b, end_b, target, pole, w, push_out):
    if w < 1e-3:
        return None
    update()
    a, b, c = head(up_b), head(lo_b), head(end_b)
    ul, ll = (b - a).length, (c - b).length
    span = ul + ll
    to_t = target - a
    dist = to_t.length
    if dist > span * MAX_EXT:
        target = a + to_t.normalized() * span * MAX_EXT
    elif dist < span * MIN_EXT:
        if not push_out:
            return None
        target = a + to_t.normalized() * span * MIN_EXT
    want_up, want_lo = solve_two_bone(a, target, ul, ll, pole)
    cur_up = (b - a).normalized()
    d_up = Quaternion().slerp(cur_up.rotation_difference(want_up), w)
    rotate_bone_world(up_b, d_up)
    b2, c2 = head(lo_b), head(end_b)
    cur_lo = (c2 - b2).normalized()
    d_lo = Quaternion().slerp(cur_lo.rotation_difference(want_lo), w)
    rotate_bone_world(lo_b, d_lo)
    return (head(end_b) - target).length


def box(name, centre, size, color):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=centre)
    o = bpy.context.active_object
    o.name = name
    o.scale = size
    mat = bpy.data.materials.new(name + "_m")
    mat.diffuse_color = color
    o.data.materials.append(mat)
    o.color = color
    return o


def clear_props():
    for o in list(bpy.data.objects):
        if o.name.startswith("prop_") or o.type in ('CAMERA', 'LIGHT'):
            bpy.data.objects.remove(o, do_unlink=True)


def render(scene_name, views):
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_WORKBENCH'
    sc.display.shading.light = 'STUDIO'
    sc.display.shading.color_type = 'OBJECT'
    sc.display.shading.show_shadows = True
    sc.display.shading.show_cavity = True
    sc.render.resolution_x = 520
    sc.render.resolution_y = 560
    sc.render.film_transparent = False
    world = bpy.data.worlds.new("w") if not sc.world else sc.world
    sc.world = world
    for o in arm.children:
        o.color = (0.78, 0.78, 0.8, 1.0)
    outs = []
    for vname, loc, look in views:
        cam_data = bpy.data.cameras.new("cam")
        cam_data.lens = 40
        cam = bpy.data.objects.new("cam", cam_data)
        bpy.context.scene.collection.objects.link(cam)
        cam.location = Vector(loc)
        direction = Vector(look) - cam.location
        cam.rotation_euler = direction.to_track_quat('-Z', 'Y').to_euler()
        sc.camera = cam
        path = f"{out_dir}/{scene_name}__{vname}.png"
        sc.render.filepath = path
        bpy.ops.render.render(write_still=True)
        outs.append(path)
        bpy.data.objects.remove(cam, do_unlink=True)
    return outs


H = 1.785           # standing height of the rig
HALF = H * 0.5      # capsule half extent (feet on the capsule bottom)
R = 0.27            # capsule radius


def place_root(root):
    """Put the capsule centre at `root` (Blender coordinates)."""
    arm.location = Vector((root[0], root[1], root[2] - HALF))
    update()


for sc_def in scenes:
    reset()
    clear_props()
    name = sc_def["name"]
    root = Vector(sc_def.get("root", [0, 0, HALF]))
    place_root(root)
    tilt = sc_def.get("tilt_deg", 0.0)
    frame = Quaternion(Vector((1, 0, 0)), math.radians(tilt))  # pitch about the body's lateral axis
    for layer in sc_def["layers"]:
        apply_pose(layer["pose"], layer.get("w", 1.0), layer.get("mirror", False), frame)
    # IK
    for ik in sc_def.get("ik", []):
        update()
        t = Vector(ik["target"])
        base = head(ik["up"])
        if ik.get("pull", 0.0) > 0.0:
            t = t + (base - t).normalized() * ik["pull"]
        p = base + Vector(ik["pole_rel"])
        err = two_bone(ik["up"], ik["lo"], ik["end"], t, p, ik.get("w", 1.0), ik.get("push_out", True))
        print("IK", name, ik["end"], "err", err)
    # knee aims (thigh toward a world point)
    for ka in sc_def.get("aims", []):
        update()
        want = Vector(ka["to"]) - head(ka["bone"])
        aim(ka["bone"], ka["child"], want, ka.get("w", 1.0))
    # Knee onto a top surface: the thigh swings forward until the knee sits
    # on the plane, then the shin is re-aimed to its authored direction.
    for kn in sc_def.get("knee_on_top", []):
        update()
        hip = head(kn["thigh"])
        knee = head(kn["shin"])
        L = (knee - hip).length
        fwd = Vector(kn["forward"]).normalized()
        side = Vector(kn.get("side", [0, 0, 0]))
        dz = kn["plane_z"] - hip.z
        if abs(dz) < L:
            h = math.sqrt(L * L - dz * dz)
            k_star = Vector((hip.x, hip.y, kn["plane_z"])) + fwd * h + side
        else:
            k_star = hip + (fwd * 0.6 + Vector((0, 0, 0.3 if dz > 0 else -0.3))).normalized() * L
        aim(kn["thigh"], kn["shin"], k_star - hip, kn.get("w", 1.0))
        shin_dir = frame @ body(kn["shin_dir"])
        aim(kn["shin"], kn["foot"], shin_dir, kn.get("w", 1.0))
        print("KNEE", name, "hip", tuple(round(v, 3) for v in hip), "knee->", tuple(round(v, 3) for v in head(kn["shin"])), "plane", round(kn["plane_z"], 3))
    for i, p in enumerate(sc_def.get("props", [])):
        box(f"prop_{i}", p["centre"], p["size"], p.get("color", (0.25, 0.42, 0.7, 1.0)))
    for i, m in enumerate(sc_def.get("markers", [])):
        bpy.ops.mesh.primitive_uv_sphere_add(radius=0.03, location=m)
        o = bpy.context.active_object
        o.name = f"prop_m{i}"
        o.color = (1.0, 0.8, 0.1, 1.0)
    look = root + Vector((0, 0, 0.1))
    views = [
        ("back", (look.x + 1.3, look.y + 3.4, look.z + 1.2), tuple(look)),
        ("side", (look.x + 3.6, look.y + 0.2, look.z + 0.4), tuple(look)),
    ]
    render(name, views)
print("DONE")
