"""Voltec Supreme, forged: hard-surface armour built around the Y Bot skeleton.

Blender 4.4. Called from build_voltec.py around the Mixamo bake:

    widen_skeleton(rig)          # before baking
    space_arms(rig, actions)     # after baking, fitting the motion to the armour
    limit_knees(rig, actions)
    turn_palms(rig, actions)     # and giving it the concept's heavy posture
    curl_fingers(rig, actions)
    settle_idle(rig, actions)
    limit_elbows(rig, actions)
    parts = build(rig, idle)

Rules the whole module follows:

* Joints come from bone HEADS only. The glTF importer fabricates bone tails
  (the hips bone arrives ten metres long), so no tail is ever read.
* Every piece is authored in the IDLE pose, in the frame of the bone that
  carries it, then mapped back to bind space through that bone's inverse
  deformation. Idle is the stance the reference concept shows and the one a
  player sees, so each shape is judged where it is seen. Building each limb
  piece in its own bone's frame keeps left and right exact mirror images even
  though the idle clip itself leans slightly.
* Armour is shaped from superellipse cross-sections with exponents near 2.3:
  rounded ceramic shells, not boxes. Much above 2.6 every limb turns into a
  rounded box. What makes armour read as pillows is isolated plates bulging in
  empty space, not roundness itself; every plate here sits on the chassis.
* The black chassis is continuous from boot to collar, so a gap between two
  plates always shows chassis and never empty space.
* Plates whose edges are not simple rings (breastplate, collar, codpiece,
  knees) are lofted generously, then cut to an outline taken from the
  concept's front view. Panel lines are shallow grooves that follow the
  surface, cut against an inset copy of it.

Axes: forward is -Y, left is +X, up is +Z.
"""
import math
import os
import re
import zlib

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Quaternion, Vector

UP = Vector((0.0, 0.0, 1.0))
FWD = Vector((0.0, -1.0, 0.0))
LEFT = Vector((1.0, 0.0, 0.0))

# Metres of surface per texture repeat. The wear maps tile, so this sets how
# large a chip reads on the body.
TILE = 0.30

# How far each limb chain moves outward from the spine, in metres. The armour
# is far bulkier than the donor body; without the clearance the arms and
# thighs are buried in the torso and hips.
ARM_SPREAD = 0.14
LEG_SPREAD = 0.1

# The idle frame the armour is authored against.
IDLE_FRAME = 20

# Degrees each upper arm is held further from the body in every clip.
ARM_SPACING_DEG = 12.0

# The furthest any clip may fold a knee or an elbow, in degrees from
# straight.
KNEE_LIMIT_DEG = 115.0
ELBOW_LIMIT_DEG = 95.0

# The height of the hips in the source idle. The torso armour is laid out
# against it, then lowered with the hips by the idle's crouch.
HIPS_DESIGN_Z = 0.989

# Degrees each finger joint curls toward the palm in every clip, knuckle
# first; the thumb curls less.
FINGER_CURL_DEG = (22.0, 38.0, 28.0)
THUMB_CURL_DEG = 0.0

# Degrees each forearm turns its palm from facing the thigh toward facing
# back, in every clip.
PALM_TURN_DEG = 55.0

# The idle's heavy stance: extra elbow bend, and the hip bend that sets the
# knee bend (twice this) and lowers the hips to keep the feet planted.
IDLE_ELBOW_DEG = 16.0
IDLE_CROUCH_DEG = 10.0
# The idle hangs its arms a little wider still, clear of the thighs.
IDLE_ARM_SPREAD_DEG = 3.5
# The walk keeps a little of the idle's elbow bend.
WALK_ELBOW_DEG = 12.0


def key(name):
    return re.sub(r'[^a-z0-9]', '', re.sub(r'_\d+$', '', name.split(':')[-1]).lower())


def smoothstep(x):
    x = min(max(x, 0.0), 1.0)
    return x * x * (3.0 - 2.0 * x)


# ── skeleton ─────────────────────────────────────────────────────────────────

def widen_skeleton(rig):
    """Move each arm and leg chain outward so bulky armour clears the torso.

    Translation only. Bone orientations are untouched, so the rest-relative
    motion bake still reproduces every source clip exactly.
    """
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode='EDIT')
    inverse = rig.matrix_world.inverted().to_3x3()
    for bone in rig.data.edit_bones:
        k = key(bone.name)
        side = 1 if k.startswith('left') else -1 if k.startswith('right') else 0
        if not side:
            continue
        if 'arm' in k or 'hand' in k:
            spread = ARM_SPREAD
        elif any(word in k for word in ('leg', 'foot', 'toe')):
            spread = LEG_SPREAD
        else:
            continue
        bone.use_connect = False
        delta = inverse @ Vector((side * spread, 0.0, 0.0))
        bone.head += delta
        bone.tail += delta
    bpy.ops.object.mode_set(mode='OBJECT')


def _bone(rig, k):
    return next(b for b in rig.pose.bones if key(b.name) == k)


def _rekey(action, pb, change):
    """Pass every keyed rotation of one bone through `change`.

    Each result is stored on the same hemisphere as the key it replaces: q
    and -q are the same rotation, but the four components are separate
    curves, and flipping one key makes the exporter's samples swing the
    bone through a full turn between neighbours.
    """
    path = f'pose.bones["{pb.name}"].rotation_quaternion'
    curves = [action.fcurves.find(path, index=i) for i in range(4)]
    if not all(curves):
        return
    for k in range(len(curves[0].keyframe_points)):
        q = Quaternion([curves[i].keyframe_points[k].co[1] for i in range(4)])
        changed = change(q)
        if changed.dot(q) < 0.0:
            changed.negate()
        for i in range(4):
            point = curves[i].keyframe_points[k]
            point.co[1] = changed[i]
            point.handle_left[1] = changed[i]
            point.handle_right[1] = changed[i]
    for curve in curves:
        curve.update()


def _turn(rig, pb, axis, degrees):
    """A rotation about a world axis as the bone's rest frame sees it.

    Pre-multiplied onto a keyed rotation, it turns the animated bone about
    that axis as fixed to its parent, whatever the clip is doing.
    """
    rest = (rig.matrix_world @ pb.bone.matrix_local).to_3x3().normalized()
    return Quaternion((rest.inverted() @ axis).normalized(), math.radians(degrees))


def space_arms(rig, actions, degrees=ARM_SPACING_DEG):
    """Hold each upper arm a few degrees further from the body in every clip.

    The Mixamo motions were captured on a slim body. Swung across this chest,
    the running forearm passed straight through the breastplate at the top of
    every stride. Rotating each upper arm outward in its parent's frame fixes
    that for every clip at once, and matches the concept, whose arms hang
    clear of the torso.

    Must run before the armour is built, because the armour is authored
    against the idle pose this changes.
    """
    for side, sgn in (('left', 1.0), ('right', -1.0)):
        pb = _bone(rig, side + 'arm')
        turn = _turn(rig, pb, FWD, degrees * sgn)
        for action, _, _ in actions:
            _rekey(action, pb, lambda q: turn @ q)


def limit_knees(rig, actions, degrees=KNEE_LIMIT_DEG):
    """Fold no knee further than `degrees` in any clip.

    The Mixamo run kicks each heel up to within 30 degrees of the thigh.
    Folded that far, the calf and ankle armour sink into the back of the
    thigh, and no rigid plate can avoid it; stopping the fold short of that
    also suits a machine this heavy. Each keyed rotation is shortened along
    its own axis, so the knee still bends the way the clip bends it.
    """
    limit = math.radians(degrees)

    def hold(q):
        q = q.copy()
        if q.w < 0.0:
            q.negate()
        return q if q.angle <= limit else Quaternion().slerp(q, limit / q.angle)
    for side in ('left', 'right'):
        pb = _bone(rig, side + 'leg')
        for action, _, _ in actions:
            _rekey(action, pb, hold)


def limit_elbows(rig, actions, degrees=ELBOW_LIMIT_DEG):
    """Fold no elbow further than `degrees` in any clip.

    The Mixamo run folds the elbows to 120 degrees. Past about 95 the front
    of a forearm this thick swings up into the upper arm, whatever shape the
    plates take. Only the fold about the elbow's hinge is shortened; the
    forearm's twist is kept.
    """
    world = rig.matrix_world
    limit = math.radians(degrees)
    for side in ('left', 'right'):
        fore, hand = _bone(rig, side + 'forearm'), _bone(rig, side + 'hand')
        reach = (world @ hand.bone.head_local - world @ fore.bone.head_local).normalized()
        rest = (world @ fore.bone.matrix_local).to_3x3().normalized()
        hinge = (rest.inverted() @ reach.cross(FWD)).normalized()

        def hold(q):
            q = q.copy()
            if q.w < 0.0:
                q.negate()
            along = Vector((q.x, q.y, q.z)).dot(hinge)
            if 2.0 * math.atan2(along, q.w) <= limit:
                return q
            fold = Quaternion((q.w, *(hinge * along))).normalized()
            return (q @ fold.inverted()) @ Quaternion(hinge, limit)
        for action, _, _ in actions:
            _rekey(action, fore, hold)


def turn_palms(rig, actions, degrees=PALM_TURN_DEG):
    """Turn each palm from facing the thigh toward facing back, in every clip.

    The concept holds its hands with the backs forward and the palms back.
    The Mixamo clips hold the palms against the thighs, so curled claws dig
    straight into the thigh plates. The forearm twists about its own length;
    applied to every clip, the forearm plates keep one relation to the arm.
    """
    for side, sgn in (('left', 1.0), ('right', -1.0)):
        pb = _bone(rig, side + 'forearm')
        # A bone's own length is its local Y. Looking down the hanging
        # forearm, the left palm turns from the body toward the back about
        # +Y, the right about -Y.
        turn = Quaternion((0.0, 1.0, 0.0), math.radians(degrees) * sgn)
        for action, _, _ in actions:
            _rekey(action, pb, lambda q: q @ turn)


def curl_fingers(rig, actions, degrees=FINGER_CURL_DEG, thumb=THUMB_CURL_DEG):
    """Curl every finger toward the palm in every clip.

    The Mixamo hands hang open and flat; the concept's are heavy half-closed
    claws. In the rest pose the palms face down, so each joint turns about
    the axis that carries its finger toward -Z.
    """
    world = rig.matrix_world
    down = Vector((0.0, 0.0, -1.0))
    for side in ('left', 'right'):
        for finger in ('thumb', 'index', 'middle', 'ring', 'pinky'):
            try:
                bones = [_bone(rig, f'{side}hand{finger}{i}') for i in (1, 2, 3)]
            except StopIteration:
                continue
            heads = [world @ b.bone.head_local for b in bones]
            # The end markers carry garbage positions, so the last joint
            # borrows the direction of the one before it.
            directions = [heads[1] - heads[0], heads[2] - heads[1], heads[2] - heads[1]]
            for i, pb in enumerate(bones):
                axis = directions[i].normalized().cross(down)
                if axis.length < 1e-4:
                    continue
                turn = _turn(rig, pb, axis, thumb if finger == 'thumb' else degrees[i])
                for action, _, _ in actions:
                    _rekey(action, pb, lambda q: turn @ q)


def settle_idle(rig, actions, elbow=IDLE_ELBOW_DEG, crouch=IDLE_CROUCH_DEG, walk_elbow=WALK_ELBOW_DEG,
                spread=IDLE_ARM_SPREAD_DEG):
    """Give the idle the concept's heavy stance.

    The arms hang a little wider, clear of the thighs that the crouch
    brings forward, the elbows bend, and the legs crouch a little: each thigh
    swings forward,
    each shin back by twice as much and each foot back to level, and the
    hips drop by exactly the height that takes off the legs, so the feet stay
    where the clip planted them. The idle stands with more weight on one
    leg, so the left folds by `crouch` and the right by whatever lifts its
    ankle the same height, found by measurement. The walk keeps some of the
    elbow bend, so the two blend without the arms snapping straight.
    """
    world = rig.matrix_world
    scene = bpy.context.scene
    by_name = {a.name: a for a, _, _ in actions}
    idle, walk = by_name['Idle'], by_name.get('Walk')
    side_axis = LEFT
    held = rig.animation_data.action
    rig.animation_data.action = idle

    def ankle_at(side):
        scene.frame_set(IDLE_FRAME)
        bpy.context.view_layer.update()
        return world @ _bone(rig, side + 'foot').head

    def ankle(side):
        return ankle_at(side).z

    stance = {side: ankle_at(side) for side in ('left', 'right')}

    def fold(side, degrees):
        # Each leg bone bends about its own lateral axis (post-multiplied),
        # so the leg folds in its own plane. The idle turns the toes out;
        # swinging the thigh about the pelvis's axis instead while the knee
        # bends about its own slides the foot sideways by centimetres.
        for k, factor in (('upleg', -1.0), ('leg', 2.0), ('foot', -1.0)):
            pb = _bone(rig, side + k)
            turn = _turn(rig, pb, side_axis, factor * degrees)
            _rekey(idle, pb, lambda q: q @ turn)

    planted = {side: ankle(side) for side in ('left', 'right')}
    fold('left', crouch)
    drop = ankle('left') - planted['left']
    low, high, applied = 0.0, 2.5 * crouch, 0.0
    for _ in range(24):
        mid = (low + high) / 2.0
        fold('right', mid - applied)
        applied = mid
        if ankle('right') - planted['right'] < drop:
            low = mid
        else:
            high = mid
    # The legs splay outward, so folding each in its own plane draws the foot
    # in toward the other. Swinging the thigh back out about the pelvis's
    # forward axis, and rolling the foot back level, returns it to its mark.
    for side, sgn in (('left', 1.0), ('right', -1.0)):
        inward = (stance[side].x - ankle_at(side).x) * sgn
        thigh, foot = _bone(rig, side + 'upleg'), _bone(rig, side + 'foot')
        length = (world @ thigh.head - world @ foot.head).length
        degrees = math.degrees(math.atan2(inward, length))
        swing = _turn(rig, thigh, FWD, degrees * sgn)
        roll = _turn(rig, foot, FWD, -degrees * sgn)
        _rekey(idle, thigh, lambda q: swing @ q)
        _rekey(idle, foot, lambda q: roll @ q)
    for side, sgn in (('left', 1.0), ('right', -1.0)):
        upper = _bone(rig, side + 'arm')
        wider = _turn(rig, upper, FWD, spread * sgn)
        _rekey(idle, upper, lambda q: wider @ q)
        fore = _bone(rig, side + 'forearm')
        hand = _bone(rig, side + 'hand')
        # At rest the arm points along +/-X with the palm down; the elbow
        # carries the hand forward.
        reach = (world @ hand.bone.head_local - world @ fore.bone.head_local).normalized()
        hinge = reach.cross(FWD).normalized()
        bend = _turn(rig, fore, hinge, elbow)
        _rekey(idle, fore, lambda q: bend @ q)
        if walk is not None:
            walk_bend = _turn(rig, fore, hinge, walk_elbow)
            _rekey(walk, fore, lambda q: walk_bend @ q)
    # Drop the hips by what the fold took off the legs' height.
    hips = _bone(rig, 'hips')
    rest = hips.bone.matrix_local.to_3x3()
    shift = rest.inverted() @ (world.to_3x3().inverted() @ Vector((0.0, 0.0, -drop)))
    path = f'pose.bones["{hips.name}"].location'
    for i in range(3):
        curve = idle.fcurves.find(path, index=i)
        if curve is None:
            continue
        for point in curve.keyframe_points:
            point.co[1] += shift[i]
            point.handle_left[1] += shift[i]
            point.handle_right[1] += shift[i]
        curve.update()
    rig.animation_data.action = held


class Pose:
    """Joint positions and skinning matrices of the rig as currently posed."""

    def __init__(self, rig):
        world = rig.matrix_world
        world_inverse = world.inverted()
        self.name, self.head, self.rest, self.deform = {}, {}, {}, {}
        for pb in rig.pose.bones:
            k = key(pb.name)
            self.name[k] = pb.name
            self.head[k] = world @ pb.head
            self.rest[k] = world @ pb.bone.head_local
            self.deform[k] = world @ pb.matrix @ pb.bone.matrix_local.inverted() @ world_inverse

    def valid(self, k):
        # End markers arrive with their REST head at the armature origin. Posed,
        # that becomes a plausible-looking but meaningless position far from the
        # finger, so validity is judged at rest; judging it posed would stretch
        # a fingertip across the room.
        return k in self.rest and self.rest[k].length > 0.05

    def frame(self, k, child, side=1):
        """Origin at the joint, T down the bone, F forward, S outward."""
        o = self.head[k]
        t = (self.head[child] - o).normalized()
        f = FWD - FWD.dot(t) * t
        if f.length < 1e-4:
            f = UP - UP.dot(t) * t
        f.normalize()
        s = f.cross(t).normalized() * side
        return o, t, f, s


# ── materials ────────────────────────────────────────────────────────────────

def material(name, color, metal, rough, emission=0.0):
    m = bpy.data.materials.new(name)
    m.diffuse_color = (*color, 1)
    m.use_nodes = True
    p = m.node_tree.nodes['Principled BSDF']
    p.inputs['Base Color'].default_value = (*color, 1)
    p.inputs['Metallic'].default_value = metal
    p.inputs['Roughness'].default_value = rough
    if emission:
        p.inputs['Emission Color'].default_value = (*color, 1)
        p.inputs['Emission Strength'].default_value = emission
    return m


def _stamp(canvas, x, y, radius, value):
    """Darken a soft disc into a tiling canvas, wrapping at the edges."""
    size = canvas.shape[0]
    r = int(math.ceil(radius + 1.0))
    ys = np.arange(y - r, y + r + 1) % size
    xs = np.arange(x - r, x + r + 1) % size
    dy, dx = np.meshgrid(np.arange(-r, r + 1), np.arange(-r, r + 1), indexing='ij')
    falloff = np.clip(radius + 0.5 - np.sqrt(dx * dx + dy * dy), 0.0, 1.0)
    region = np.ix_(ys, xs)
    canvas[region] = np.minimum(canvas[region], 1.0 - falloff * (1.0 - value))


def _periodic_noise(rng, size, grid):
    """Smooth value noise that tiles exactly across the canvas."""
    lattice = rng.random((grid, grid)).astype(np.float32)
    axis = np.arange(size, dtype=np.float32) * grid / size
    cells = axis.astype(np.int32)
    frac = axis - cells
    frac = frac * frac * (3.0 - 2.0 * frac)
    nxt = (cells + 1) % grid
    rows = lattice[:, cells] * (1.0 - frac)[None, :] + lattice[:, nxt] * frac[None, :]
    return rows[cells, :] * (1.0 - frac)[:, None] + rows[nxt, :] * frac[:, None]


def _stroke(canvas, rng, x, y, heading, length, width, value, wander):
    """Darken a stroke one pixel per step, its heading drifting by `wander`."""
    for _ in range(int(length)):
        heading += rng.normal(0, wander)
        x += math.cos(heading)
        y += math.sin(heading)
        _stamp(canvas, int(x), int(y), width, value)


def surface_maps(mat, kind, size=1024):
    """Pack tiling albedo, roughness and normal maps onto a material.

    Generated deterministically with numpy, so they export to glTF exactly as
    rendered here; renderer-only procedural nodes would not survive export.

    The ceramic follows the reference: a white coating, pitted with dark
    grunge where it has worn, with scratches and hairline cracks. The
    graphite is a matte black marble with pale veins.
    """
    rng = np.random.default_rng(2207 if kind == 'ceramic' else 6113)
    wear = _periodic_noise(rng, size, 5) * 0.6 + _periodic_noise(rng, size, 17) * 0.4
    if kind == 'ceramic':
        # Broad patches where the coating has taken wear; between them it is
        # nearly clean.
        patch = np.clip((_periodic_noise(rng, size, 4) * 0.6 + _periodic_noise(rng, size, 9) * 0.4 - 0.4) * 2.6,
                        0.0, 1.0)
        paint = np.ones((size, size), dtype=np.float32)
        # Grunge: each fleck is a short run of overlapping discs, so the marks
        # are irregular and a little elongated, and they cluster in the
        # patches. Evenly spread round dots read as a dalmatian, not wear.
        for _ in range(14000):
            x, y = rng.uniform(0, size, 2)
            if rng.random() > 0.06 + 0.94 * patch[int(y) % size, int(x) % size] ** 1.6:
                continue
            heading = rng.uniform(0, 2 * math.pi)
            big = rng.random() < 0.06
            value = float(rng.uniform(0.08, 0.45))
            for _ in range(int(rng.integers(3, 9 if big else 5))):
                radius = rng.uniform(1.4, 3.2) if big else rng.uniform(0.6, 1.6)
                _stamp(paint, int(x), int(y), float(radius), value)
                heading += rng.normal(0, 0.8)
                step = rng.uniform(0.8, 2.2)
                x += math.cos(heading) * step
                y += math.sin(heading) * step
        # Grime: soft dark mottling inside the patches.
        mottle = _periodic_noise(rng, size, 19) * 0.55 + _periodic_noise(rng, size, 41) * 0.45
        paint -= patch * np.clip((mottle - 0.45) * 2.0, 0.0, 1.0) * 0.2
        # Scratches: short, nearly straight strokes.
        for _ in range(200):
            x, y = rng.uniform(0, size, 2)
            if rng.random() > 0.1 + 0.9 * patch[int(y) % size, int(x) % size]:
                continue
            _stroke(paint, rng, x, y, rng.uniform(0, 2 * math.pi), rng.uniform(20, 100),
                    float(rng.choice([0.6, 0.9, 1.3], p=[0.5, 0.35, 0.15])), float(rng.uniform(0.4, 0.7)), 0.03)
        # Hairline cracks: long wandering lines.
        for _ in range(45):
            x, y = rng.uniform(0, size, 2)
            _stroke(paint, rng, x, y, rng.uniform(0, 2 * math.pi), rng.uniform(90, 300),
                    float(rng.uniform(0.6, 1.0)), float(rng.uniform(0.3, 0.55)), 0.2)
        paint = np.clip(paint, 0.0, 1.0)
        chipped = 1.0 - paint
        broad = _periodic_noise(rng, size, 3) - 0.5
        value = np.clip((0.905 + broad * 0.03 - wear * 0.02) * paint, 0.02, 0.95)
        colors = np.stack([value, value * 0.995, value * 0.975], axis=-1)
        rough = np.clip(0.36 + wear * 0.06 + chipped * 0.34, 0.28, 0.85)
        height = -chipped * 0.6 + rng.normal(0, 1, (size, size)).astype(np.float32) * 0.02
    else:
        # Black marble: a dark base with pale veins swirling through it at two
        # scales, as on the concept's boots and joints. What reads as moulded
        # plastic is an even surface, not gloss: here the roughness and the
        # relief vary with the veins and the mottling, so highlights break up.
        # Fully matte would not help; a dielectric's sheen spread by a matte
        # finish turns black to grey under broad light.
        yy, xx = np.mgrid[:size, :size].astype(np.float32) / size
        warp = (_periodic_noise(rng, size, 6) - 0.5) * 3.0 + (_periodic_noise(rng, size, 19) - 0.5) * 0.8
        band = np.abs(np.sin(2 * math.pi * (2 * xx + yy) + warp * 2.2))
        vein = np.exp(-(band / 0.06) ** 2) * (0.3 + 0.7 * _periodic_noise(rng, size, 9))
        warp2 = (_periodic_noise(rng, size, 11) - 0.5) * 2.6 + (_periodic_noise(rng, size, 31) - 0.5) * 0.7
        band2 = np.abs(np.sin(2 * math.pi * (3 * yy - xx) + warp2 * 2.6))
        vein2 = np.exp(-(band2 / 0.045) ** 2) * np.clip(_periodic_noise(rng, size, 7) * 1.4 - 0.3, 0.0, 1.0)
        veins = np.maximum(vein, vein2 * 0.8)
        mottle = _periodic_noise(rng, size, 23) - 0.5
        grain = rng.normal(0, 1, (size, size)).astype(np.float32)
        value = np.clip(0.01 + mottle * 0.008 + veins * 0.36 + grain * 0.003, 0.004, 0.45)
        colors = np.stack([value * 0.96, value, value * 1.05], axis=-1)
        rough = np.clip(0.44 + mottle * 0.5 + wear * 0.12 - veins * 0.12, 0.26, 0.72)
        height = veins * 0.45 + grain * 0.06 + mottle * 0.5
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * 0.5
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * 0.5
    normal = np.stack([-dx, -dy, np.ones_like(dx)], axis=-1)
    normal /= np.linalg.norm(normal, axis=-1, keepdims=True)
    normal = normal * 0.5 + 0.5
    p = mat.node_tree.nodes['Principled BSDF']
    for label, data, colorspace in (
            ('albedo', colors, 'sRGB'),
            ('roughness', np.repeat(rough[:, :, None], 3, axis=2), 'Non-Color'),
            ('normal', normal, 'Non-Color')):
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
            node.inputs['Strength'].default_value = 0.6
            mat.node_tree.links.new(tex.outputs['Color'], node.inputs['Color'])
            mat.node_tree.links.new(node.outputs['Normal'], p.inputs['Normal'])
        else:
            target = 'Base Color' if label == 'albedo' else 'Roughness'
            mat.node_tree.links.new(tex.outputs['Color'], p.inputs[target])


def palette():
    """The Voltec design language: white precision, black structure, xenon live."""
    ceramic = material('01 / ivory ceramic', (0.86, 0.86, 0.84), 0.0, 0.38)
    graphite = material('02 / graphite elastomer', (0.012, 0.013, 0.014), 0.0, 0.45)
    # Emission stays moderate so the hue survives tone mapping; much brighter
    # and the blue washes out toward sky blue. The visor well is matte enough
    # not to mirror the key light across the whole visor; the lens along the
    # Y is glossy, so the highlights follow the Y as in the concept.
    xenon = material('03 / xenon status', (0.04, 0.36, 1.0), 0.0, 0.25, emission=2.6)
    xenon_deep = material('06 / xenon visor well', (0.0, 0.035, 0.52), 0.0, 0.32, emission=0.55)
    xenon_lens = material('07 / xenon visor lens', (0.01, 0.1, 0.85), 0.0, 0.1, emission=0.7)
    steel = material('04 / dark machined steel', (0.12, 0.125, 0.13), 0.85, 0.32)
    ink = material('05 / carbon lettering', (0.012, 0.013, 0.015), 0.0, 0.85)
    surface_maps(ceramic, 'ceramic')
    surface_maps(graphite, 'graphite')
    return dict(ceramic=ceramic, graphite=graphite, xenon=xenon, xenon_deep=xenon_deep, xenon_lens=xenon_lens,
                steel=steel, ink=ink)


# ── geometry ─────────────────────────────────────────────────────────────────

def superellipse(theta, n):
    c, s = math.cos(theta), math.sin(theta)
    return (math.copysign(abs(c) ** (2.0 / n), c), math.copysign(abs(s) ** (2.0 / n), s))


def R(t, s, f=None, n=3.0, s_in=None, f_back=None, ds=0.0, df=0.0):
    """One cross-section: half-sizes outward/inward (s) and front/back (f)."""
    f = s if f is None else f
    return dict(t=t, out=s, inn=s if s_in is None else s_in,
                front=f, back=f if f_back is None else f_back, n=n, ds=ds, df=df)


def _filleted(rings, radius, caps, poles, steps=4):
    """Round the ends of a solid loft with a quarter-circle fillet."""
    out = list(rings)
    for end in (0, 1):
        if not caps[end] or poles[end]:
            continue
        edge = out[0] if end == 0 else out[-1]
        inner = out[1] if end == 0 else out[-2]
        direction = -1.0 if (inner['t'] - edge['t']) > 0 else 1.0
        extra = []
        for k in range(1, steps + 1):
            phi = k / steps * math.pi / 2.0
            shrink = radius * (1.0 - math.cos(phi))
            r = dict(edge)
            r['t'] = edge['t'] + direction * radius * math.sin(phi)
            for side in ('out', 'inn', 'front', 'back'):
                r[side] = max(edge[side] - shrink, 0.003)
            extra.append(r)
        out = (list(reversed(extra)) + out) if end == 0 else (out + extra)
    return out


def rolled(rings, amount=0.014, length=0.012):
    """Curl a plate's two edges in toward the body, like a pressed rim.

    A plate that simply stops ends in a flat ring, and a stack of those reads
    as boxes on a pole. Rolling the edges inward is what makes a shell read as
    formed armour.
    """
    first, last = dict(rings[0]), dict(rings[-1])
    rising = 1.0 if rings[1]['t'] > rings[0]['t'] else -1.0
    first['t'] = rings[0]['t'] - rising * length
    last['t'] = rings[-1]['t'] + rising * length
    for r in (first, last):
        for side in ('out', 'inn', 'front', 'back'):
            r[side] = max(r[side] - amount, 0.004)
    return [first] + list(rings) + [last]


def _link(obj):
    bpy.context.scene.collection.objects.link(obj)
    return obj


def loft(name, frame, rings, *, segments=48, arc=None, thickness=None,
         caps=(True, True), fillet=0.0, poles=(False, False), material=None,
         ridge=None, bevel=0.0, sharp=42.0, slant=None):
    """Sweep superellipse sections along a straight axis.

    `arc` limits the sweep to part of the circumference (radians, measured
    from outward toward forward, so pi/2 is dead ahead). `thickness` makes a
    shell with real depth and closed rims instead of a solid. `slant(t, x, y)`
    moves each point along the axis by a distance that may depend on where
    round the section it lies (x outward, y forward, each -1 to 1), which
    shapes an end edge without cutting through the rims.
    """
    o, T, F, S = frame
    solid = thickness is None
    closed = arc is None
    rings = [dict(r) for r in rings]
    if solid and fillet > 0.0:
        rings = _filleted(rings, fillet, caps, poles)
    count = segments if closed else segments + 1
    thetas = [(2.0 * math.pi * i / segments) if closed
              else arc[0] + (arc[1] - arc[0]) * i / segments for i in range(count)]

    def centre(r):
        return o + T * r['t'] + S * r['ds'] + F * r['df']

    def point(r, theta, inset):
        x, y = superellipse(theta, r['n'])
        sx = max((r['out'] if x >= 0.0 else r['inn']) - inset, 0.002)
        fy = max((r['front'] if y >= 0.0 else r['back']) - inset, 0.002)
        radial = S * (x * sx) + F * (y * fy)
        if ridge is not None and inset == 0.0:
            angle, width, height = ridge
            d = math.atan2(math.sin(theta - angle), math.cos(theta - angle))
            if abs(d) < width:
                radial = radial + radial.normalized() * height * (1.0 - (d / width) ** 2) ** 2
        if slant is not None:
            radial = radial + T * slant(r['t'], x, y)
        return centre(r) + radial

    along = [0.0]
    for a, b in zip(rings, rings[1:]):
        along.append(along[-1] + (centre(b) - centre(a)).length)
    perimeter = []
    for r in rings:
        pts = [point(r, th, 0.0) for th in thetas]
        loop = pts + [pts[0]] if closed else pts
        perimeter.append(sum((b - a).length for a, b in zip(loop, loop[1:])))
    span = max(sum(perimeter) / len(perimeter), 1e-4) / TILE

    bm = bmesh.new()
    uv = bm.loops.layers.uv.new('Surface UV')

    def u(i):
        return i / segments * span

    def rows_for(inset):
        rows = []
        for index, r in enumerate(rings):
            pole = (index == 0 and poles[0]) or (index == len(rings) - 1 and poles[1])
            if pole:
                c = centre(r)
                if inset:
                    toward = centre(rings[1] if index == 0 else rings[-2]) - c
                    c = c + toward.normalized() * inset
                rows.append(bm.verts.new(c))
            else:
                rows.append([bm.verts.new(point(r, th, inset)) for th in thetas])
        return rows

    def face(verts, uvs):
        try:
            f = bm.faces.new(verts)
        except ValueError:
            return None
        for loop, c in zip(f.loops, uvs):
            loop[uv].uv = c
        return f

    def surface(rows):
        for index in range(len(rows) - 1):
            a, b = rows[index], rows[index + 1]
            va, vb = along[index] / TILE, along[index + 1] / TILE
            if isinstance(a, list) and isinstance(b, list):
                for i in range(len(a) if closed else len(a) - 1):
                    j = (i + 1) % len(a)
                    face((a[i], a[j], b[j], b[i]), ((u(i), va), (u(i + 1), va), (u(i + 1), vb), (u(i), vb)))
            else:
                ring, pole = (b, a) if not isinstance(a, list) else (a, b)
                vr, vp = (vb, va) if not isinstance(a, list) else (va, vb)
                for i in range(len(ring) if closed else len(ring) - 1):
                    j = (i + 1) % len(ring)
                    face((ring[i], ring[j], pole), ((u(i), vr), (u(i + 1), vr), (u(i + 0.5), vp)))

    outer = rows_for(0.0)
    surface(outer)
    if solid:
        for end, index in ((0, 0), (1, len(rings) - 1)):
            if not caps[end] or poles[end]:
                continue
            ring = outer[index]
            hub = bm.verts.new(centre(rings[index]))
            v = along[index] / TILE
            for i in range(len(ring) if closed else len(ring) - 1):
                j = (i + 1) % len(ring)
                face((ring[i], ring[j], hub), ((u(i), v), (u(i + 1), v), (u(i + 0.5), v + 0.08)))
    else:
        inner = rows_for(thickness)
        surface(inner)
        depth = thickness / TILE
        for index in (0, len(rings) - 1):
            a, b = outer[index], inner[index]
            if not isinstance(a, list):
                continue
            v = along[index] / TILE
            for i in range(len(a) if closed else len(a) - 1):
                j = (i + 1) % len(a)
                face((a[i], a[j], b[j], b[i]),
                     ((u(i), v), (u(i + 1), v), (u(i + 1), v + depth), (u(i), v + depth)))
        if not closed:
            for i in (0, len(thetas) - 1):
                for index in range(len(rings) - 1):
                    a0, a1 = outer[index], outer[index + 1]
                    b0, b1 = inner[index], inner[index + 1]
                    if not (isinstance(a0, list) and isinstance(a1, list)):
                        continue
                    va, vb = along[index] / TILE, along[index + 1] / TILE
                    face((a0[i], a1[i], b1[i], b0[i]), ((va, 0), (vb, 0), (vb, depth), (va, depth)))

    bmesh.ops.remove_doubles(bm, verts=bm.verts[:], dist=1e-6)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    if bevel > 0.0:
        edges = [e for e in bm.edges if e.is_manifold and e.calc_face_angle(0.0) > math.radians(55.0)]
        if edges:
            bmesh.ops.bevel(bm, geom=edges, offset=bevel, offset_type='OFFSET', segments=2,
                            profile=0.5, affect='EDGES', clamp_overlap=True)
    return _finish(name, bm, material, sharp)


def _finish(name, bm, mat, sharp=42.0):
    # Each piece starts its texture at its own place in the tile, so the
    # same marks do not repeat at the start of every plate.
    layer = bm.loops.layers.uv.get('Surface UV')
    if layer is not None:
        seed = zlib.crc32(name.encode('utf-8'))
        du, dv = (seed & 0xFFFF) / 65536.0, (seed >> 16) / 65536.0
        for f in bm.faces:
            for loop in f.loops:
                u, v = loop[layer].uv
                loop[layer].uv = (u + du, v + dv)
    for f in bm.faces:
        f.smooth = True
    for e in bm.edges:
        if e.is_manifold and e.calc_face_angle(0.0) > math.radians(sharp):
            e.smooth = False
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = _link(bpy.data.objects.new(name, mesh))
    if mat is not None:
        mesh.materials.append(mat)
    return obj


def apply_modifiers(obj):
    bpy.context.view_layer.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    evaluated = obj.evaluated_get(depsgraph)
    mesh = bpy.data.meshes.new_from_object(evaluated, preserve_all_data_layers=True, depsgraph=depsgraph)
    old = obj.data
    obj.modifiers.clear()
    obj.data = mesh
    bpy.data.meshes.remove(old)


def reshade(obj, sharp=42.0):
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    for f in bm.faces:
        f.smooth = True
    for e in bm.edges:
        e.smooth = not (e.is_manifold and e.calc_face_angle(0.0) > math.radians(sharp))
    bm.to_mesh(obj.data)
    bm.free()


def _open_edges(mesh):
    bm = bmesh.new()
    bm.from_mesh(mesh)
    count = sum(1 for e in bm.edges if not e.is_manifold)
    bm.free()
    return count


def carve(obj, cutter, operation='DIFFERENCE'):
    """Boolean with the exact solver. Cut faces take the cutter's material.

    Pieces are carved before their rims are bevelled (see `soften`): the
    bevel leaves slivers overlapping by fractions of a millimetre, and on
    those the exact solver returns an empty mesh for an ordinary cut. A cut
    that still empties the piece or tears it open is undone and redone with
    self-intersection handling, which resolves the near-degenerate contacts
    where two grooves cross.
    """
    original = obj.data.copy()
    open_before = _open_edges(original)
    for tolerant in (False, True):
        mod = obj.modifiers.new('carve', 'BOOLEAN')
        mod.operation = operation
        mod.solver = 'EXACT'
        mod.use_self = tolerant
        mod.object = cutter
        mod.material_mode = 'TRANSFER'
        apply_modifiers(obj)
        if obj.data.vertices and _open_edges(obj.data) <= open_before:
            break
        if not tolerant:
            torn = obj.data
            obj.data = original.copy()
            bpy.data.meshes.remove(torn)
    bpy.data.meshes.remove(original)
    bpy.data.objects.remove(cutter, do_unlink=True)
    reshade(obj)


def prism(name, outline, near, far, axis_depth=FWD, origin=Vector(), right=LEFT, up=UP, mat=None):
    """Extrude a 2D outline (right, up) through a depth range along `axis_depth`."""
    bm = bmesh.new()
    uv = bm.loops.layers.uv.new('Surface UV')
    front = [bm.verts.new(origin + right * x + up * z + axis_depth * near) for x, z in outline]
    back = [bm.verts.new(origin + right * x + up * z + axis_depth * far) for x, z in outline]
    faces = [bm.faces.new(front), bm.faces.new(list(reversed(back)))]
    for i in range(len(outline)):
        j = (i + 1) % len(outline)
        faces.append(bm.faces.new((front[i], front[j], back[j], back[i])))
    for f in faces:
        for loop in f.loops:
            p = loop.vert.co - origin
            loop[uv].uv = (p.dot(right) / TILE, p.dot(up) / TILE)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    return _finish(name, bm, mat)


def offset_outline(points, d):
    """Offset a closed 2D outline by d, outward when d is positive, mitred."""
    n = len(points)
    area = sum(points[i][0] * points[(i + 1) % n][1] - points[(i + 1) % n][0] * points[i][1] for i in range(n))
    flip = 1.0 if area > 0.0 else -1.0

    def normal(a, b):
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dx, dy) or 1.0
        return flip * dy / length, -flip * dx / length

    out = []
    for i in range(n):
        n1 = normal(points[i - 1], points[i])
        n2 = normal(points[i], points[(i + 1) % n])
        mx, my = n1[0] + n2[0], n1[1] + n2[1]
        length = math.hypot(mx, my) or 1.0
        mx, my = mx / length, my / length
        reach = d / max(mx * n1[0] + my * n1[1], 0.35)
        out.append((points[i][0] + mx * reach, points[i][1] + my * reach))
    return out


def grown(rings, d):
    """The same sections moved outward by d all round."""
    return [dict(r, out=r['out'] + d, inn=r['inn'] + d, front=r['front'] + d, back=r['back'] + d) for r in rings]


def ring_at(rings, t):
    """The section at t, interpolated between the rings either side of it."""
    for a, b in zip(rings, rings[1:]):
        if min(a['t'], b['t']) <= t <= max(a['t'], b['t']) and a['t'] != b['t']:
            u = (t - a['t']) / (b['t'] - a['t'])
            return {k: a[k] + (b[k] - a[k]) * u for k in a}
    nearest = min(rings, key=lambda r: abs(r['t'] - t))
    return dict(nearest, t=t)


def groove(name, frame, rings, material, *, along=None, around=None, width=0.003, depth=0.0025, lift=0.012,
           slant=None):
    """A cutter for one panel line: a thin shell lying in the plate's surface.

    `along=(theta, t0, t1)` runs the line down the plate at one angle;
    `around=(t, theta0, theta1)` runs it round the plate at one height, or
    all the way round when both angles are None. The shell is built from the
    plate's own sections, so it follows the surface exactly, reaching
    `depth` below it and `lift` above, enough to cut through a raised ridge
    as well. An arc should reach a little past the plate's edges: a cutter
    ending exactly on a rim leaves coplanar faces the solver can tear on.
    """
    if along is not None:
        theta, t0, t1 = along
        ts = sorted({t0, t1, *(r['t'] for r in rings if min(t0, t1) < r['t'] < max(t0, t1))})
        sections = [ring_at(rings, t) for t in ts]
        radius = sum(max(r['out'], r['front']) for r in sections) / len(sections)
        half = width / (2.0 * radius)
        arc, segments = (theta - half, theta + half), 2
    else:
        t, theta0, theta1 = around
        sections = [ring_at(rings, t - width / 2.0), ring_at(rings, t + width / 2.0)]
        arc = None if theta0 is None else (theta0, theta1)
        segments = 64
    return loft(name, frame, grown(sections, lift), segments=segments, arc=arc, thickness=lift + depth,
                material=material, slant=slant)


def soften(obj, offset):
    """Round a carved piece's sharp edges, after every cut is made.

    Edges touching a panel line are left sharp, or the rounding would fill
    the groove in.
    """
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    plate = 0
    edges = [e for e in bm.edges if e.is_manifold and e.calc_face_angle(0.0) > math.radians(55.0)
             and all(f.material_index == plate for f in e.link_faces)]
    if edges:
        bmesh.ops.bevel(bm, geom=edges, offset=offset, offset_type='OFFSET', segments=2, profile=0.5,
                        affect='EDGES', clamp_overlap=True)
    bm.to_mesh(obj.data)
    bm.free()
    reshade(obj)


def raycast(obj, origin, direction):
    bpy.context.view_layer.update()
    hit, location, normal, _ = obj.ray_cast(origin, direction.normalized())
    return (location, normal.normalized()) if hit else (None, None)


def port(name, target, origin, direction, radius, mats, depth=0.012):
    """A machined round port set into a surface: a steel bezel round a dark core."""
    location, normal = raycast(target, origin, direction)
    if location is None:
        return []
    t = normal
    helper = UP if abs(t.dot(UP)) < 0.9 else FWD
    s = helper.cross(t).normalized()
    f = t.cross(s).normalized()
    frame = (location - t * 0.004, t, f, s)
    bezel = loft(name + ' bezel', frame, [R(0.0, radius, radius, 2.0), R(depth, radius, radius, 2.0)],
                 segments=28, thickness=radius * 0.32, material=mats['steel'], bevel=radius * 0.08)
    core = loft(name + ' core', frame, [R(0.0, radius * 0.72, radius * 0.72, 2.0),
                                        R(depth * 0.55, radius * 0.72, radius * 0.72, 2.0)],
                segments=28, fillet=radius * 0.1, material=mats['graphite'])
    return [bezel, core]


def lettering(text, cap_height, centre, target, mat, weight=0.0, stretch=1.0, spacing=1.0):
    """Raised glyphs laid onto a curved plate, facing forward.

    The face is Inter, which ships with Blender under the SIL Open Font
    License, so the build is portable and the glyph geometry is licence-clean.
    Weight comes from a rounded bevel, which grows each stroke without
    re-triangulating the outline. Widening the outline itself (`offset`)
    would cross the M's inner vertex over into a bowtie. The concept's
    lettering is an extended face, so the glyphs are stretched sideways.
    """
    fonts = bpy.utils.system_resource('DATAFILES', path='fonts')
    curve = bpy.data.curves.new(text, 'FONT')
    curve.font = bpy.data.fonts.load(os.path.join(fonts, 'Inter.woff2'), check_existing=True)
    curve.body = text
    curve.align_x = 'CENTER'
    curve.align_y = 'CENTER'
    curve.size = cap_height / 0.727
    curve.space_character = spacing
    curve.extrude = 0.0006
    curve.bevel_depth = weight
    curve.bevel_resolution = 2
    curve.resolution_u = 5
    carrier = _link(bpy.data.objects.new(text + ' (text)', curve))
    carrier.location = centre
    carrier.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    carrier.scale = (stretch, 1.0, 1.0)
    bpy.context.view_layer.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    mesh = bpy.data.meshes.new_from_object(carrier.evaluated_get(depsgraph))
    mesh.transform(carrier.matrix_world)
    bpy.data.objects.remove(carrier, do_unlink=True)
    # Conform to the plate while keeping the relief: each vertex keeps its
    # height above the letter's back face. Snapping every vertex to the
    # surface would flatten the raised strokes onto one plane.
    back = max(v.co.y for v in mesh.vertices)
    for v in mesh.vertices:
        hit, _ = raycast(target, Vector((v.co.x, v.co.y - 0.5, v.co.z)), -FWD)
        if hit is not None:
            v.co.y = hit.y - 0.0004 - (back - v.co.y)
    mesh.update()
    obj = _link(bpy.data.objects.new(text, mesh))
    mesh.uv_layers.new(name='Surface UV')
    obj.data.materials.append(mat)
    return obj


# ── weighting ────────────────────────────────────────────────────────────────

def rigid(k):
    return lambda p: {k: 1.0}


def chain(joints, width=0.045):
    """Blend a vertex across a chain of bones by its height along the chain.

    `joints` is [(bone, head)] from root to tip, roughly vertical. Inside a
    bone's span the vertex belongs wholly to that bone; within `width` of a
    joint it blends smoothly into the next.
    """
    heights = [h.z for _, h in joints]
    names = [k for k, _ in joints]

    def weights(p):
        s = sum(smoothstep((p.z - h + width) / (2.0 * width)) for h in heights[1:])
        i = min(int(s), len(names) - 1)
        frac = s - i
        if i + 1 >= len(names) or frac < 1e-4:
            return {names[i]: 1.0}
        return {names[i]: 1.0 - frac, names[i + 1]: frac}
    return weights


def blend(parent, child, origin, axis, at, width):
    """Blend from parent to child across a joint, measured along an axis."""
    def weights(p):
        x = smoothstep(((p - origin).dot(axis) - (at - width)) / (2.0 * width))
        if x < 1e-4:
            return {parent: 1.0}
        if x > 1.0 - 1e-4:
            return {child: 1.0}
        return {parent: 1.0 - x, child: x}
    return weights


def bulge(t0, t1, r_end, r_mid, n=2.2, steps=6):
    """Sections of a smooth joint swelling from r_end to r_mid and back.

    The concept's joints are heavy black masses as broad as the plates either
    side of them; a thin corrugated boot there pinches every limb to a neck.
    """
    return [R(t0 + (t1 - t0) * i / steps, r, r, n)
            for i in range(steps + 1)
            for r in (r_end + (r_mid - r_end) * math.sin(math.pi * i / steps),)]


# ── the figure ───────────────────────────────────────────────────────────────

def helmet_rings(inset=0.0):
    """Helmet sections about the head joint, moved inward by `inset`.

    Chin at the bottom, crown at the top. The helmet is as wide as a large
    motorcycle helmet, 0.28 m, because the concept's helmet is about three
    fifths the width of its breastplate. The crown is an ellipsoidal cap
    closing on a single pole, since filleting a tiny top ring would pinch it
    into a spike. The cap insets along its own normal, pole included, so a
    recess cut against it keeps its depth over the top.
    """
    d = inset
    rings = [R(t + (d if i == 0 else 0.0), s - d, f - d, n, f_back=b - d) for i, (t, s, f, b, n) in enumerate((
        (-0.118, 0.066, 0.084, 0.078, 2.2),
        (-0.106, 0.09, 0.11, 0.1, 2.3),
        (-0.082, 0.108, 0.13, 0.118, 2.4),
        (-0.046, 0.123, 0.142, 0.132, 2.45),
        (-0.005, 0.134, 0.148, 0.143, 2.45),
        (0.04, 0.14, 0.15, 0.15, 2.4)))]
    base, height = 0.073, 0.14
    for phi in np.linspace(0.12, 1.47, 9):
        k = math.cos(phi)
        rings.append(R(base + (height - d) * math.sin(phi), (0.14 - d) * k, (0.15 - d) * k,
                       2.4 - 0.27 * phi, f_back=(0.152 - d) * k))
    rings.append(R(base + height - d, 0.0, 0.0, 2.0))
    return rings


# The visor outline about the head joint, (left, up) in metres, measured off
# the concept: a band across the eyes with rounded ends, whose lower edge
# sweeps down into a stem that narrows to the chin. The cheeks either side of
# the stem stay white, which is what makes it read as a Y and not a window,
# and the band's top edge falls away toward the wing tips.
VISOR = [
    (-0.119, 0.083), (-0.1, 0.092), (-0.06, 0.103), (0.0, 0.108), (0.06, 0.103), (0.1, 0.092), (0.119, 0.083),
    (0.124, 0.07), (0.122, 0.058), (0.116, 0.046), (0.106, 0.037), (0.094, 0.029),
    (0.08, 0.018), (0.066, 0.006), (0.052, -0.007), (0.042, -0.018),
    (0.034, -0.034), (0.028, -0.052), (0.022, -0.068), (0.014, -0.08),
    (0.006, -0.087), (0.0, -0.089), (-0.006, -0.087),
    (-0.014, -0.08), (-0.022, -0.068), (-0.028, -0.052), (-0.034, -0.034),
    (-0.042, -0.018), (-0.052, -0.007), (-0.066, 0.006), (-0.08, 0.018),
    (-0.094, 0.029), (-0.106, 0.037), (-0.116, 0.046), (-0.122, 0.058), (-0.124, 0.07),
]

# The lens inside the visor: a Y stroke along the middle of each wing and
# down the stem, where the concept's glass catches the light. Filling the
# whole visor instead reads as a pale cup.
VISOR_LENS = [
    (-0.104, 0.071), (-0.06, 0.064), (-0.024, 0.045), (0.0, 0.04), (0.024, 0.045), (0.06, 0.064), (0.104, 0.071),
    (0.102, 0.052), (0.062, 0.041), (0.026, 0.015), (0.016, -0.018), (0.011, -0.05), (0.0, -0.07),
    (-0.011, -0.05), (-0.016, -0.018), (-0.026, 0.015), (-0.062, 0.041), (-0.102, 0.052),
]


class Forge:
    # The chassis cross-section by height: (z, half-width, front, back, n).
    # Every torso plate rides this, offset outward, so plates fit by
    # construction instead of floating where they happen to be placed.
    # The reference's 0.44 m chest width already INCLUDES the breastplate, so
    # the chassis is narrow and the plates stand proud of it to supply the
    # bulk. A wide chassis under flush plates reads as a padded suit.
    TORSO = [
        (0.865, 0.115, 0.098, 0.106, 2.6),
        (0.925, 0.160, 0.122, 0.131, 3.0),
        (1.000, 0.176, 0.132, 0.142, 3.0),
        (1.060, 0.168, 0.13, 0.136, 3.0),
        (1.120, 0.158, 0.128, 0.13, 3.0),
        (1.200, 0.163, 0.138, 0.136, 3.0),
        (1.290, 0.172, 0.143, 0.141, 2.7),
        (1.380, 0.178, 0.146, 0.143, 2.7),
        (1.455, 0.170, 0.136, 0.137, 2.7),
        (1.515, 0.135, 0.116, 0.112, 2.8),
        (1.560, 0.085, 0.078, 0.082, 2.4),
    ]

    centre_frame = (Vector((0.0, 0.0, 0.0)), UP, FWD, LEFT)

    def __init__(self, pose, mats):
        self.pose, self.m = pose, mats
        self.pieces = []
        # How far the idle's crouch has lowered the hips below their design
        # height; every torso piece is built at design height, then lowered.
        self.lift = pose.head['hips'].z - HIPS_DESIGN_Z

    def add(self, obj, weights):
        if obj is not None:
            self.pieces.append((obj, weights))
        return obj

    def spine_joints(self):
        return [(k, self.pose.head[k]) for k in ('hips', 'spine', 'spine1', 'spine2', 'neck', 'head')]

    def spine_offset(self, z):
        """Where the spine passes at design height z, so torso sections follow its lean."""
        z += self.lift
        pts = [h for _, h in self.spine_joints()]
        if z <= pts[0].z:
            return pts[0].x, pts[0].y
        for a, b in zip(pts, pts[1:]):
            if z <= b.z:
                u = (z - a.z) / max(b.z - a.z, 1e-6)
                return a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u
        return pts[-1].x, pts[-1].y

    def torso_ring(self, z, s, f, b, n=3.0):
        x, y = self.spine_offset(z)
        return R(z, s, f, n, f_back=b, ds=x, df=-y)

    def torso_at(self, z):
        rows = self.TORSO
        if z <= rows[0][0]:
            return rows[0][1:]
        for a, b in zip(rows, rows[1:]):
            if z <= b[0]:
                u = (z - a[0]) / (b[0] - a[0])
                return tuple(a[i] + (b[i] - a[i]) * u for i in range(1, 5))
        return rows[-1][1:]

    def shell_rings(self, z0, z1, standoff, thickness, steps=12, flare=None):
        """Outer sections of a plate whose inner face stands `standoff` off the chassis."""
        out = []
        for i in range(steps + 1):
            z = z0 + (z1 - z0) * i / steps
            s, f, b, n = self.torso_at(z)
            extra = standoff + thickness + (flare(i / steps) if flare else 0.0)
            out.append(self.torso_ring(z, s + extra, f + extra, b + extra, n))
        return out

    # ── chassis ──────────────────────────────────────────────────────────
    def torso_chassis(self):
        self.add(loft('Chassis torso', self.centre_frame,
                      [self.torso_ring(z, s, f, b, n) for z, s, f, b, n in self.TORSO],
                      segments=64, fillet=0.03, material=self.m['graphite']), chain(self.spine_joints()))

    def neck_chassis(self):
        m, P = self.m, self.pose
        neck, head = P.head['neck'], P.head['head']
        axis = (head - neck).normalized()
        f = (FWD - FWD.dot(axis) * axis).normalized()
        self.add(loft('Chassis neck', (neck, axis, f, f.cross(axis).normalized()),
                      bulge(0.02, (head - neck).length + 0.05, 0.066, 0.074),
                      segments=40, fillet=0.012, material=m['graphite']),
                 blend('neck', 'head', neck, axis, (head - neck).length, 0.05))

    def limb_chassis(self, side, sgn):
        m, P = self.m, self.pose
        arm, fore, hand = side + 'arm', side + 'forearm', side + 'hand'
        frame = P.frame(arm, fore, sgn)
        o, t = frame[0], frame[1]
        L = (P.head[fore] - o).length
        # The joints are as massive as the plates either side of them, so a
        # limb's outline runs on through each joint instead of pinching.
        self.add(loft(f'Chassis shoulder {side}', frame,
                      [R(-0.085, 0.07, 0.07, 2.0), R(-0.045, 0.09, 0.09, 2.0), R(0.0, 0.094, 0.094, 2.0),
                       R(0.045, 0.086, 0.086, 2.0), R(0.08, 0.07, 0.07, 2.0)],
                      segments=32, fillet=0.03, material=m['graphite']), rigid(arm))
        self.add(loft(f'Chassis upper arm {side}', frame,
                      [R(0.03, 0.07, 0.07, 2.4), R(L * 0.6, 0.068, 0.068, 2.4), R(L - 0.02, 0.066, 0.066, 2.4)],
                      segments=32, fillet=0.02, material=m['graphite']), rigid(arm))
        self.add(loft(f'Chassis elbow {side}', frame, bulge(L - 0.06, L + 0.06, 0.066, 0.084),
                      segments=32, fillet=0.01, material=m['graphite']), blend(arm, fore, o, t, L, 0.035))
        frame2 = P.frame(fore, hand, sgn)
        o2, t2 = frame2[0], frame2[1]
        L2 = (P.head[hand] - o2).length
        self.add(loft(f'Chassis forearm {side}', frame2,
                      [R(0.02, 0.068, 0.068, 2.4), R(L2 * 0.5, 0.064, 0.064, 2.4), R(L2 - 0.02, 0.058, 0.058, 2.4)],
                      segments=32, fillet=0.02, material=m['graphite']), rigid(fore))
        self.add(loft(f'Chassis wrist {side}', frame2, bulge(L2 - 0.04, L2 + 0.035, 0.056, 0.066),
                      segments=32, fillet=0.008, material=m['graphite']), blend(fore, hand, o2, t2, L2, 0.025))
        up, leg, foot = side + 'upleg', side + 'leg', side + 'foot'
        frame = P.frame(up, leg, sgn)
        o, t = frame[0], frame[1]
        L = (P.head[leg] - o).length
        # The hip is a ball centred on the joint, so a rising thigh turns it
        # in place instead of sweeping it through the belt, and the thigh's
        # black upper end tapers into it.
        ball = 0.112
        self.add(loft(f'Chassis hip {side}', frame,
                      [R(-ball, 0.0, 0.0, 2.0)] + [R(ball * math.sin(a), ball * math.cos(a), ball * math.cos(a), 2.0)
                                                   for a in np.linspace(-1.4, 1.4, 11)] + [R(ball, 0.0, 0.0, 2.0)],
                      segments=40, poles=(True, True), material=m['graphite']), rigid(up))
        self.add(loft(f'Chassis thigh {side}', frame,
                      [R(0.05, 0.088, 0.092, 2.4), R(0.12, 0.106, 0.112, 2.5), R(L * 0.5, 0.102, 0.106, 2.5),
                       R(L - 0.03, 0.094, 0.098, 2.5)],
                      segments=40, fillet=0.02, material=m['graphite']), rigid(up))
        self.add(loft(f'Chassis knee {side}', frame, bulge(L - 0.07, L + 0.07, 0.094, 0.112),
                      segments=40, fillet=0.01, material=m['graphite']), blend(up, leg, o, t, L, 0.045))
        frame2 = P.frame(leg, foot, sgn)
        L2 = (P.head[foot] - frame2[0]).length
        self.add(loft(f'Chassis shin {side}', frame2,
                      [R(0.03, 0.095, 0.097, 2.5), R(L2 * 0.5, 0.088, 0.09, 2.5), R(L2 - 0.02, 0.078, 0.081, 2.5)],
                      segments=40, fillet=0.02, material=m['graphite']), rigid(leg))

    # ── helmet ───────────────────────────────────────────────────────────
    def helmet(self):
        m, P = self.m, self.pose
        o = P.head['head']
        frame = (o, UP, FWD, LEFT)
        shell = loft('Helmet', frame, helmet_rings(), segments=80, fillet=0.025, poles=(False, True),
                     material=m['ceramic'])

        def inside(name, depth, mat):
            return loft(name, frame, helmet_rings(depth), segments=80, fillet=0.02, poles=(False, True), material=mat)

        # Every visor cut is a prism from the front minus the helmet inset by
        # the cut's depth, so each recess follows the curvature at a constant
        # depth; a flat-backed cutter only bites where the dome bulges
        # forward. The cuts take their cutters' materials: a black rim round a
        # deep blue well.
        border = prism('Visor surround', offset_outline(VISOR, 0.0045), 0.3, 0.02, origin=Vector(o),
                       mat=m['graphite'])
        carve(border, inside('Visor surround floor', 0.004, m['graphite']))
        carve(shell, border)
        cutter = prism('Visor cut', VISOR, 0.3, 0.02, origin=Vector(o), mat=m['xenon_deep'])
        carve(cutter, inside('Visor floor', 0.012, m['xenon_deep']))
        carve(shell, cutter)
        # A lens stands in the well along the middle of the Y, so the glass
        # is bright at its centre and deep at its edges, as in the concept.
        lens = prism('Visor lens', VISOR_LENS, 0.3, 0.02, origin=Vector(o), mat=m['xenon_lens'])
        carve(lens, inside('Visor lens face', 0.005, m['xenon_lens']), 'INTERSECT')
        self.add(lens, rigid('head'))
        # Crest: two panel lines from the brow up over the crown. They run
        # along meridians, so they close toward the top as the concept's do.
        for sgn in (1, -1):
            carve(shell, groove(f'Crest line {sgn}', frame, helmet_rings(), m['graphite'],
                                along=(math.pi / 2.0 - sgn * 0.25, 0.116, 0.205), lift=0.004))
        self.add(shell, rigid('head'))
        for sgn in (1, -1):
            for piece in port(f'Helmet port {sgn}', shell, o + LEFT * sgn * 0.3 + UP * 0.04, LEFT * -sgn, 0.018, m):
                self.add(piece, rigid('head'))
        # Status array: five xenon cells in a row under one wing of the visor,
        # parallel to the wing's lower edge.
        for i in range(5):
            loc, nrm = raycast(shell, o + Vector((0.058 + i * 0.009, -0.3, -0.03 + i * 0.0075)), -FWD)
            if loc is None:
                continue
            cell = (loc - nrm * 0.002, nrm, UP, UP.cross(nrm).normalized())
            self.add(loft(f'Status cell {i}', cell, [R(0.0, 0.0035, 0.0035, 2.0), R(0.004, 0.0035, 0.0035, 2.0)],
                          segments=12, fillet=0.0012, material=m['xenon']), rigid('head'))

    # ── torso armour ─────────────────────────────────────────────────────
    def torso_armour(self):
        m = self.m
        cf = self.centre_frame
        front = math.pi / 2.0
        # Breastplate: a deep barrel over the chest, lofted generously and cut
        # to the concept's front outline: a broad top edge under the collar
        # that dips at the throat, sides drawing in toward the waist, and a
        # lower edge rising at the centre between two rounded lobes.
        chest_rings = []
        for i in range(13):
            z = 1.19 + (1.44 - 1.19) * i / 12
            s_, f_, b_, n_ = self.torso_at(min(z, 1.42))
            swell = 0.02 * math.sin(math.pi * i / 12)
            chest_rings.append(self.torso_ring(z, min(s_ + 0.052, 0.232), f_ + 0.068 + swell, b_ + 0.05, 2.3))
        chest = loft('Breastplate', cf, chest_rings, segments=72, arc=(front - 1.4, front + 1.4), thickness=0.036,
                     material=m['ceramic'])
        outline = [(-0.19, 1.4), (-0.15, 1.424), (-0.11, 1.418), (-0.07, 1.405), (-0.035, 1.397), (0.0, 1.395),
                   (0.035, 1.397), (0.07, 1.405), (0.11, 1.418), (0.15, 1.424), (0.19, 1.4),
                   (0.222, 1.37), (0.228, 1.34), (0.224, 1.31), (0.212, 1.27), (0.192, 1.24), (0.166, 1.218),
                   (0.125, 1.206), (0.07, 1.204), (0.035, 1.21), (0.0, 1.222), (-0.035, 1.21), (-0.07, 1.204),
                   (-0.125, 1.206), (-0.166, 1.218), (-0.192, 1.24), (-0.212, 1.27), (-0.224, 1.31),
                   (-0.228, 1.34), (-0.222, 1.37)]
        carve(chest, prism('Breastplate outline', outline, 0.6, -0.3, mat=m['ceramic']), 'INTERSECT')
        soften(chest, 0.01)
        self.add(chest, rigid('spine2'))
        centre_y = self.spine_offset(1.34)[1]
        self.add(lettering('VOLTEC', 0.056, Vector((0.0, centre_y - 0.23, 1.337)), chest, m['ink'],
                           weight=0.0033, stretch=1.45), rigid('spine2'))
        self.add(lettering('SUPREME', 0.032, Vector((0.0, centre_y - 0.23, 1.276)), chest, m['ink'],
                           weight=0.0021, stretch=1.35, spacing=1.1), rigid('spine2'))
        # Collar: a high gorget whose wings stand either side of the helmet up
        # to its cheeks, open at the throat so the visor and chin show, and
        # sloping down toward the shoulders. The helmet sits down in it, the
        # concept's hunched, neckless look.
        collar = loft('Collar', cf, rolled([self.torso_ring(z, s, f, b, 2.3) for z, s, f, b in (
            (1.4, 0.198, 0.19, 0.178), (1.45, 0.196, 0.18, 0.17), (1.5, 0.195, 0.172, 0.165),
            (1.54, 0.198, 0.168, 0.162), (1.565, 0.195, 0.166, 0.16))], 0.008, 0.006),
            segments=72, arc=(front - 1.62, front + 1.62), thickness=0.026, material=m['ceramic'])
        throat = [(-0.168, 1.72), (-0.166, 1.58), (-0.155, 1.53), (-0.13, 1.48), (-0.085, 1.44), (0.0, 1.425),
                  (0.085, 1.44), (0.13, 1.48), (0.155, 1.53), (0.166, 1.58), (0.168, 1.72)]
        carve(collar, prism('Collar throat', throat, 0.4, -0.12, mat=m['ceramic']))
        # Each wing's top falls away from the helmet toward the shoulder.
        for sgn in (1, -1):
            slope = [(sgn * x, z) for x, z in ((0.166, 1.7), (0.166, 1.58), (0.232, 1.45), (0.4, 1.45), (0.4, 1.7))]
            carve(collar, prism(f'Collar slope {sgn}', slope, 0.4, -0.4, mat=m['ceramic']))
        soften(collar, 0.009)
        self.add(collar, rigid('spine2'))
        for sgn in (1, -1):
            for piece in port(f'Collar port {sgn}', collar, Vector((sgn * 0.16, -0.5, 1.465)), -FWD, 0.022, m):
                self.add(piece, rigid('spine2'))
        # Beneath the breastplate: a sternum band whose top edge dips in the
        # middle and a plate on each flank.
        band = loft('Sternum band', cf, rolled(self.shell_rings(1.13, 1.185, 0.01, 0.028, steps=4), 0.01, 0.008),
                    segments=56, arc=(front - 0.72, front + 0.72), thickness=0.028, material=m['ceramic'])
        smile = [(-0.078, 1.26), (-0.064, 1.19), (-0.04, 1.172), (0.0, 1.166), (0.04, 1.172), (0.064, 1.19),
                 (0.078, 1.26)]
        carve(band, prism('Sternum notch', smile, 0.6, -0.25, mat=m['ceramic']))
        soften(band, 0.008)
        self.add(band, rigid('spine1'))
        for sgn, arc in ((1, (0.2, 0.75)), (-1, (math.pi - 0.75, math.pi - 0.2))):
            self.add(loft(f'Flank plate {sgn}', cf,
                          rolled(self.shell_rings(1.085, 1.185, 0.008, 0.024, steps=5), 0.01, 0.008),
                          segments=24, arc=arc, thickness=0.024, material=m['ceramic'], bevel=0.007), rigid('spine1'))
        # The abdomen is armoured in black: two rows of plates either side of
        # a central seam, standing off the chassis like the concept's.
        for row, (z0, z1) in enumerate(((1.018, 1.064), (1.07, 1.118))):
            for sgn, arc in ((1, (front - 0.56, front - 0.05)), (-1, (front + 0.05, front + 0.56))):
                self.add(loft(f'Abdominal plate {row} {sgn}', cf,
                              rolled(self.shell_rings(z0, z1, 0.004, 0.02, steps=3), 0.008, 0.006),
                              segments=20, arc=arc, thickness=0.02, material=m['graphite'], bevel=0.006),
                         rigid('spine'))
        self.add(loft('Backplate', cf, rolled(self.shell_rings(1.205, 1.48, 0.012, 0.034, steps=11), 0.012, 0.01),
                      segments=64, arc=(front + math.pi - 1.0, front + math.pi + 1.0), thickness=0.034,
                      material=m['ceramic'], bevel=0.009), rigid('spine2'))
        # Ion thruster pack (canon: two Hall-effect thrusters and a xenon tank).
        x, y = self.spine_offset(1.36)
        # Seated into the backplate, so it reads as mounted rather than hung
        # behind the back.
        pack_frame = (Vector((x, y + 0.195, 0.0)), UP, FWD, LEFT)
        self.add(loft('Thruster pack', pack_frame, [
            R(1.215, 0.1, 0.06, 4.0, f_back=0.05), R(1.25, 0.122, 0.066, 4.0, f_back=0.058),
            R(1.40, 0.126, 0.066, 4.0, f_back=0.06), R(1.475, 0.112, 0.06, 4.0, f_back=0.052)],
            segments=48, fillet=0.022, material=m['ceramic']), rigid('spine2'))
        for sgn in (1, -1):
            nozzle = (Vector((x + sgn * 0.06, y + 0.205, 1.215)), -UP, FWD, LEFT)
            self.add(loft(f'Thruster {sgn}', nozzle, [R(0.0, 0.034, 0.034, 2.0), R(0.045, 0.04, 0.04, 2.0),
                                                     R(0.06, 0.042, 0.042, 2.0)],
                          segments=28, thickness=0.006, material=m['steel'], bevel=0.002), rigid('spine2'))
            self.add(loft(f'Thruster throat {sgn}', nozzle, [R(0.004, 0.026, 0.026, 2.0), R(0.012, 0.026, 0.026, 2.0)],
                          segments=24, fillet=0.004, material=m['xenon']), rigid('spine2'))
        self.add(loft('Pack status strip', (Vector((x, y + 0.25, 1.41)), -FWD, UP, LEFT),
                      [R(0.0, 0.07, 0.006, 4.0), R(0.004, 0.07, 0.006, 4.0)],
                      segments=24, fillet=0.002, material=m['xenon']), rigid('spine2'))
        belt = loft('Hip belt', cf, rolled(self.shell_rings(0.975, 1.015, 0.006, 0.024, steps=3), 0.008, 0.006),
                    segments=72, thickness=0.024, material=m['ceramic'], bevel=0.007)
        self.add(belt, rigid('hips'))
        # The hip bolts sit on the belt either side of the codpiece, facing
        # forward as in the concept. Nothing stands off the front of the
        # hips: a raised thigh sweeps its plate's top edge through there.
        for sgn in (1, -1):
            for piece in port(f'Hip bolt {sgn}', belt, Vector((sgn * 0.122, -0.5, 0.995)), -FWD, 0.016, m):
                self.add(piece, rigid('hips'))
        # Codpiece: a long shield from the belt to between the thighs, narrow
        # enough below the belt that the thighs swing past its sides.
        cod = loft('Codpiece', cf, rolled([
            self.torso_ring(0.995, 0.17, 0.17, 0.15, 2.6), self.torso_ring(0.94, 0.163, 0.167, 0.15, 2.6),
            self.torso_ring(0.88, 0.146, 0.16, 0.14, 2.5), self.torso_ring(0.83, 0.124, 0.15, 0.13, 2.4),
            self.torso_ring(0.79, 0.102, 0.138, 0.12, 2.3), self.torso_ring(0.765, 0.086, 0.127, 0.11, 2.2)],
            0.01, 0.008),
            segments=48, arc=(front - 0.62, front + 0.62), thickness=0.03,
            material=m['ceramic'], ridge=(front, 0.3, 0.008))
        shield = [(-0.1, 1.03), (0.1, 1.03), (0.1, 0.955), (0.08, 0.905), (0.062, 0.86), (0.05, 0.82),
                  (0.036, 0.785), (0.02, 0.763), (0.0, 0.755), (-0.02, 0.763), (-0.036, 0.785), (-0.05, 0.82),
                  (-0.062, 0.86), (-0.08, 0.905), (-0.1, 0.955)]
        carve(cod, prism('Codpiece outline', shield, 0.6, -0.3, mat=m['ceramic']), 'INTERSECT')
        soften(cod, 0.009)
        self.add(cod, rigid('hips'))
        self.add(loft('Sacral plate', cf, [
            self.torso_ring(1.0, 0.2, 0.15, 0.158, 3.0), self.torso_ring(0.93, 0.18, 0.14, 0.15, 3.0),
            self.torso_ring(0.875, 0.15, 0.13, 0.135, 2.8)],
            segments=40, arc=(front + math.pi - 0.7, front + math.pi + 0.7), thickness=0.03,
            material=m['ceramic'], bevel=0.008), rigid('hips'))

    # ── arms ─────────────────────────────────────────────────────────────
    def arm(self, side, sgn):
        m, P = self.m, self.pose
        arm, fore, hand = side + 'arm', side + 'forearm', side + 'hand'
        frame = P.frame(arm, fore, sgn)
        o, t, f, s = frame
        L = (P.head[fore] - o).length
        # Pauldron: a bulbous dome over the shoulder from level with the
        # helmet's cheek down over the top of the arm, its rim curling in
        # over the upper arm plate. The concept's pauldrons are under half
        # the breastplate's width. Centred on the joint, so the arm swings
        # it about its own middle.
        side_r, front_r, height, equator, out = 0.112, 0.12, 0.115, -0.03, 0.028
        dome = [R(equator - height, 0.0, 0.0, 2.0, ds=out)]
        for phi in np.linspace(1.42, 0.0, 10):
            k = math.cos(phi)
            dome.append(R(equator - height * math.sin(phi), side_r * k, front_r * k, 2.05,
                          s_in=(side_r - 0.038) * k, ds=out))
        dome += [R(0.02, side_r + 0.003, front_r + 0.003, 2.1, s_in=side_r - 0.03, ds=out + 0.001),
                 R(0.07, side_r + 0.004, front_r + 0.004, 2.15, s_in=side_r - 0.028, ds=out + 0.002),
                 R(0.084, side_r - 0.008, front_r - 0.008, 2.15, s_in=side_r - 0.04, ds=out + 0.002)]
        pauldron = loft(f'Pauldron {side}', frame, dome, segments=72, thickness=0.026, poles=(True, False),
                        material=m['ceramic'])
        # A panel line round it above the rim separates dome from skirt.
        carve(pauldron, groove(f'Pauldron line {side}', frame, dome, m['graphite'], around=(0.02, None, None),
                               lift=0.006))
        soften(pauldron, 0.009)
        self.add(pauldron, rigid(arm))
        aim = (s * math.cos(0.8) + f * math.sin(0.8)).normalized()
        for piece in port(f'Pauldron port {side}', pauldron, o - t * 0.03 + aim * 0.45, -aim, 0.025, m):
            self.add(piece, rigid(arm))
        # The upper arm plate runs from under the pauldron's rim right down to
        # the elbow, as broad as the pauldron above it, so the arm's outline
        # carries on unbroken. Open on the inner face.
        upper_rings = rolled([R(0.1, 0.096, 0.092, 2.3), R(0.17, 0.101, 0.097, 2.3), R(L - 0.06, 0.099, 0.095, 2.3),
                              R(L - 0.01, 0.096, 0.092, 2.3)], 0.012, 0.01)
        # Its lower edge rises at the front, the same crease above the elbow
        # as the gauntlet has below it, so a folding forearm, twisting as the
        # clips twist it, meets black. Shaped by slanting the sections rather
        # than cut, so the rim stays whole.
        rise = lambda t_, x, y: -0.13 * max(y, 0.0) * min(max((t_ - 0.14) / (L - 0.14), 0.0), 1.0)
        upper = loft(f'Upper arm guard {side}', frame, upper_rings, segments=56, arc=(-2.4, 2.0), thickness=0.024,
                     material=m['ceramic'], ridge=(0.0, 0.5, 0.006), slant=rise)
        carve(upper, groove(f'Upper arm line {side}', frame, upper_rings, m['graphite'],
                            along=(0.6, 0.14, L - 0.1), slant=rise))
        soften(upper, 0.008)
        self.add(upper, rigid(arm))
        frame2 = P.frame(fore, hand, sgn)
        o2, t2, f2, s2 = frame2
        L2 = (P.head[hand] - o2).length
        # The forearm is one massive gauntlet flaring over the elbow and again
        # into a cuff at the wrist. Its top edge is high at the sides, on the
        # elbow's hinge, where it overlaps the upper arm plate, and dips at
        # the front of the elbow, by slanted sections rather than a cut:
        # folded to the elbow limit, anything nearer the crease there would
        # swing up into the upper arm.
        # The top ends in a lip that flares outward, clear of the upper arm
        # plate it overlaps; curled inward like the other rims it would cut
        # into it.
        fore_rings = [R(-0.05, 0.136, 0.13, 2.3), R(-0.036, 0.132, 0.126, 2.3)] + rolled(
            [R(0.0, 0.132, 0.126, 2.3), R(0.06, 0.127, 0.121, 2.3), R(0.12, 0.119, 0.113, 2.3),
             R(0.18, 0.109, 0.104, 2.3), R(L2 - 0.055, 0.1, 0.096, 2.3), R(L2 - 0.022, 0.105, 0.1, 2.3),
             R(L2 - 0.004, 0.108, 0.103, 2.3)], 0.012, 0.01)[1:]
        dip = lambda t_, x, y: 0.18 * max(y, 0.0) * min(max((0.26 - t_) / 0.31, 0.0), 1.0)
        forearm = loft(f'Forearm guard {side}', frame2, fore_rings, segments=64, arc=(-2.4, 2.0), thickness=0.026,
                       material=m['ceramic'], ridge=(0.0, 0.45, 0.008), slant=dip)
        # Panel lines: one round the plate below the crease and one down the
        # front of the outer face, faceting it like the concept's plates.
        # The ring is cut first; cut across an existing line, the exact
        # solver tears the right arm's plate open where the two cross.
        carve(forearm, groove(f'Forearm ring {side}', frame2, fore_rings, m['graphite'],
                              around=(0.15, -2.46, 2.06), slant=dip))
        carve(forearm, groove(f'Forearm line {side}', frame2, fore_rings, m['graphite'],
                              along=(0.55, 0.05, L2 - 0.07), slant=dip))
        soften(forearm, 0.009)
        self.add(forearm, rigid(fore))
        self.gauntlet(side, sgn)

    def gauntlet(self, side, sgn):
        m, P = self.m, self.pose
        hand = side + 'hand'
        wrist = P.head[hand]
        knuckles = P.head[side + 'handmiddle1']
        t = (knuckles - wrist).normalized()
        across = P.head[side + 'handindex1'] - P.head[side + 'handpinky1']
        across = (across - across.dot(t) * t).normalized()
        back = t.cross(across).normalized()
        if back.dot(LEFT * sgn) < 0:
            back = -back
        L = (knuckles - wrist).length
        frame = (wrist, t, back, across)
        # Heavy gauntlets: the concept's hands are as massive as its forearms,
        # half-closed claws. At donor proportions they read as bare human
        # hands on armoured arms.
        self.add(loft(f'Gauntlet {side}', frame, [
            R(-0.01, 0.07, 0.047, 3.2, f_back=0.044), R(0.03, 0.085, 0.052, 3.4, f_back=0.045),
            R(L - 0.01, 0.089, 0.048, 3.4, f_back=0.041), R(L + 0.016, 0.082, 0.038, 3.0, f_back=0.033)],
            segments=40, fillet=0.013, material=m['graphite']), rigid(hand))
        self.add(loft(f'Knuckle guard {side}', (wrist + back * 0.017, t, back, across), [
            R(0.035, 0.072, 0.037, 3.2), R(L - 0.015, 0.08, 0.04, 3.2), R(L + 0.012, 0.072, 0.033, 2.9)],
            segments=40, arc=(math.pi / 2 - 1.1, math.pi / 2 + 1.1), thickness=0.013,
            material=m['graphite'], bevel=0.004), rigid(hand))
        for finger, radius in (('thumb', 0.0218), ('index', 0.0198), ('middle', 0.0203),
                               ('ring', 0.0194), ('pinky', 0.0176)):
            keys = [f'{side}hand{finger}{i}' for i in range(1, 5)]
            # Stop at the first joint that is not a real phalanx away. The
            # finger end markers carry positions tens of centimetres off, and
            # building a segment to one drew a rod out past the head.
            heads = []
            for k in keys:
                if not P.valid(k) or (heads and (P.head[k] - heads[-1]).length > 0.06):
                    break
                heads.append(P.head[k])
            if len(heads) < 3:
                continue
            if len(heads) == 3:
                heads.append(heads[2] + (heads[2] - heads[1]) * 0.9)
            for i in range(3):
                a, b = heads[i], heads[i + 1]
                seg = (b - a).length
                ft = (b - a).normalized()
                fb = back - back.dot(ft) * ft
                fb = fb.normalized() if fb.length > 1e-4 else back
                fs = ft.cross(fb).normalized()
                r = radius * (1.0 - 0.07 * i)
                # The last joint tapers to a blunt claw.
                tip = 0.55 if i == 2 else 0.96
                self.add(loft(f'{finger} {i + 1} {side}', (a, ft, fb, fs), [
                    R(-0.004, r, r * 1.05, 2.8), R(seg + (0.004 if i < 2 else 0.004), r * tip, r * tip * 1.04, 2.8)],
                    segments=20, fillet=r * 0.4, material=m['graphite']), rigid(keys[i]))

    # ── legs ─────────────────────────────────────────────────────────────
    def leg(self, side, sgn):
        m, P = self.m, self.pose
        up, leg, foot = side + 'upleg', side + 'leg', side + 'foot'
        frame = P.frame(up, leg, sgn)
        o, t, f, s = frame
        L = (P.head[leg] - o).length
        front = math.pi / 2.0
        # One plate over the front and outer side of the thigh, bulging
        # through its middle; the inner side is black, which leaves the
        # codpiece room between the thighs. Its top edge slants: high on the
        # outer side, where nothing on the pelvis is in the way, and well
        # below the hip joint on the inner side, so as the thigh rises the
        # edge passes in front of the belt instead of into it. A V is cut up
        # into its lower edge and a panel line runs on from the V's point:
        # the faceting all of the concept's plates share.
        thigh_rings = rolled([
            R(0.065, 0.136, 0.153, 2.35, s_in=0.124, f_back=0.136),
            R(0.12, 0.142, 0.163, 2.35, s_in=0.128, f_back=0.14),
            R(0.19, 0.15, 0.169, 2.35, s_in=0.13, f_back=0.142),
            R(0.26, 0.146, 0.158, 2.35, s_in=0.12, f_back=0.13),
            R(L - 0.1, 0.136, 0.143, 2.3, s_in=0.108, f_back=0.114)], 0.018, 0.015)
        thigh = loft(f'Thigh guard {side}', frame, thigh_rings, segments=72, arc=(front - 1.3, front + 0.6),
                     thickness=0.033, material=m['ceramic'])
        carve(thigh, prism(f'Thigh slant {side}', [(-0.3, -0.15), (0.3, -0.045), (0.3, 0.3), (-0.3, 0.3)],
                           0.4, -0.4, axis_depth=f, origin=o, right=s, up=-t, mat=m['ceramic']))
        carve(thigh, prism(f'Thigh notch {side}', [(-0.03, -(L - 0.07)), (0.03, -(L - 0.15)), (0.09, -(L - 0.07))],
                           0.4, 0.0, axis_depth=f, origin=o, right=s, up=-t, mat=m['ceramic']))
        carve(thigh, groove(f'Thigh line {side}', frame, thigh_rings, m['graphite'],
                            along=(front - 0.15, 0.13, L - 0.15)))
        soften(thigh, 0.01)
        self.add(thigh, rigid(up))
        # A cap over the outer side of the black hip ball, with a bolt at its
        # centre. It sits on the axis the thigh swings about, so it turns in
        # place as the leg rises, clear of the belt and the swinging arms.
        radius = 0.115
        cap = loft(f'Hip cap {side}', (o, s, f, t), [R(radius, 0.0, 0.0, 2.0)] + [
            R(radius * math.cos(a), radius * math.sin(a), radius * math.sin(a), 2.0) for a in np.linspace(0.2, 1.1, 8)],
            segments=48, thickness=0.014, poles=(True, False), material=m['ceramic'], bevel=0.005)
        self.add(cap, rigid(up))
        for piece in port(f'Hip cap bolt {side}', cap, o + s * 0.4, -s, 0.022, m):
            self.add(piece, rigid(up))
        frame2 = P.frame(leg, foot, sgn)
        o2, t2, f2, s2 = frame2
        L2 = (P.head[foot] - o2).length
        # A plate high on the back of the thigh, toward its outer side, so the
        # leg reads as armoured from behind, where the camera spends most of
        # its time. The inner thigh stays black, and the plate stays well above
        # the knee, where the calf would meet it as the leg folds.
        self.add(loft(f'Thigh back plate {side}', frame, rolled([
            R(0.11, 0.136, 0.136, 2.4, f_back=0.136), R(0.15, 0.137, 0.137, 2.4, f_back=0.136),
            R(0.19, 0.134, 0.134, 2.4, f_back=0.13)], 0.012, 0.01),
            segments=40, arc=(-math.pi / 2 - 0.55, -math.pi / 2 + 0.8), thickness=0.025,
            material=m['ceramic'], bevel=0.008), rigid(up))
        # A big shield-shaped knee cap on a heavy black joint, its point
        # dropping into a V cut down into the shin plate's top edge.
        knee = loft(f'Knee guard {side}', frame2, [
            R(-0.072, 0.078, 0.11, 2.2, df=0.02), R(-0.04, 0.098, 0.14, 2.2, df=0.02),
            R(0.0, 0.106, 0.15, 2.2, df=0.02), R(0.045, 0.096, 0.138, 2.2, df=0.02),
            R(0.09, 0.07, 0.104, 2.05, df=0.02)],
            segments=48, arc=(front - 1.15, front + 1.15), thickness=0.032,
            material=m['ceramic'], ridge=(front, 0.4, 0.011))
        shield = [(-0.13, 0.12), (0.13, 0.12), (0.13, 0.02), (0.1, -0.03), (0.06, -0.07), (0.0, -0.096),
                  (-0.06, -0.07), (-0.1, -0.03), (-0.13, 0.02)]
        carve(knee, prism(f'Knee outline {side}', shield, 0.4, -0.1, axis_depth=f2, origin=o2, right=s2, up=-t2,
                          mat=m['ceramic']), 'INTERSECT')
        soften(knee, 0.01)
        self.add(knee, rigid(leg))
        aim = (s2 * math.cos(0.62) + f2 * math.sin(0.62)).normalized()
        for piece in port(f'Knee bolt {side}', knee, o2 + aim * 0.4, -aim, 0.023, m):
            self.add(piece, rigid(leg))
        # A shin plate flaring wider than the knee over the front and sides of
        # the calf, a calf plate behind, then black, then a cuff over the
        # ankle with a bolt on its outer face.
        shin = loft(f'Shin plate {side}', frame2, rolled([
            R(0.105, 0.14, 0.142, 2.3, s_in=0.12, f_back=0.116),
            R(0.155, 0.134, 0.136, 2.3, s_in=0.115, f_back=0.11),
            R(0.215, 0.123, 0.127, 2.3, s_in=0.106, f_back=0.1),
            R(0.265, 0.115, 0.12, 2.3, s_in=0.1, f_back=0.094)], 0.013, 0.01),
            segments=64, arc=(front - 1.4, front + 1.15), thickness=0.029,
            material=m['ceramic'], ridge=(front, 0.36, 0.009))
        carve(shin, prism(f'Shin notch {side}', [(-0.06, -0.07), (0.0, -0.145), (0.06, -0.07)],
                          0.4, 0.0, axis_depth=f2, origin=o2, right=s2, up=-t2, mat=m['ceramic']))
        soften(shin, 0.009)
        self.add(shin, rigid(leg))
        self.add(loft(f'Calf plate {side}', frame2, rolled([
            R(0.2, 0.124, 0.122, 2.35, f_back=0.118), R(0.24, 0.12, 0.118, 2.35, f_back=0.113),
            R(0.27, 0.115, 0.114, 2.35, f_back=0.107)], 0.012, 0.01),
            segments=40, arc=(-math.pi / 2 - 0.85, -math.pi / 2 + 0.85), thickness=0.025,
            material=m['ceramic'], bevel=0.008), rigid(leg))
        cuff = loft(f'Ankle guard {side}', frame2, rolled([
            R(L2 - 0.115, 0.108, 0.116, 2.3, f_back=0.1), R(L2 - 0.08, 0.118, 0.127, 2.3, f_back=0.11),
            R(L2 - 0.044, 0.13, 0.138, 2.3, f_back=0.12)], 0.01, 0.008),
            segments=56, thickness=0.027, material=m['ceramic'], bevel=0.008)
        self.add(cuff, rigid(leg))
        aim = (s2 * math.cos(0.45) + f2 * math.sin(0.45)).normalized()
        for piece in port(f'Ankle bolt {side}', cuff, o2 + t2 * (L2 - 0.078) + aim * 0.4, -aim, 0.018, m):
            self.add(piece, rigid(leg))
        self.boot(side, sgn)

    def boot(self, side, sgn):
        m, P = self.m, self.pose
        foot, toe = side + 'foot', side + 'toebase'
        ankle, ball = P.head[foot], P.head[toe]
        ahead = Vector((ball.x - ankle.x, ball.y - ankle.y, 0.0))
        reach = ahead.length
        ahead.normalize()
        across = ahead.cross(UP).normalized() * -sgn
        # A ground frame: T runs heel to toe along the floor, F is straight up,
        # so a section's "front" half-size is half its height.
        frame = (Vector((ankle.x, ankle.y, 0.0)), ahead, UP, across)

        def section(x, half_w, top, n=3.6, bottom=0.0):
            h = (top - bottom) / 2.0
            return R(x, half_w, h, n, df=bottom + h)
        # The concept's boots are massive rounded clogs on a thick black
        # sole. The foot is one loft heel to ball, the toe a second on the toe
        # bone so it can roll.
        self.add(loft(f'Boot {side}', frame, [
            section(-0.13, 0.09, 0.16, 2.8), section(-0.108, 0.115, 0.215, 2.8), section(-0.05, 0.126, 0.23, 2.8),
            section(0.0, 0.128, 0.23, 2.8), section(0.06, 0.13, 0.205, 2.8), section(reach - 0.01, 0.13, 0.18, 2.8)],
            segments=56, fillet=0.026, material=m['graphite']), rigid(foot))
        self.add(loft(f'Boot toe {side}', frame, [
            section(reach - 0.025, 0.129, 0.178, 2.7), section(reach + 0.05, 0.127, 0.16, 2.6),
            section(reach + 0.11, 0.113, 0.13, 2.5), section(reach + 0.142, 0.086, 0.095, 2.3)],
            segments=56, fillet=0.026, material=m['graphite']), rigid(toe))
        # Shaft: its own vertical loft, so it can narrow into the ankle guard.
        # Low and pulled forward so it stays inside the guard; any taller and
        # it shows behind the ankle as a black block.
        shaft = (Vector((ankle.x, ankle.y, 0.0)) + ahead * 0.006, UP, ahead, across)
        self.add(loft(f'Boot shaft {side}', shaft, [
            R(0.1, 0.114, 0.122, 2.5, f_back=0.107), R(0.165, 0.107, 0.114, 2.5, f_back=0.098),
            R(0.215, 0.1, 0.106, 2.5, f_back=0.09)],
            segments=48, fillet=0.012, material=m['graphite']), rigid(foot))
        self.add(loft(f'Instep plate {side}', frame, [
            R(0.02, 0.09, 0.034, 2.8, df=0.208), R(0.075, 0.093, 0.034, 2.8, df=0.184),
            R(reach - 0.015, 0.09, 0.032, 2.8, df=0.162)],
            segments=40, arc=(math.pi / 2 - 1.2, math.pi / 2 + 1.2), thickness=0.018,
            material=m['ceramic'], bevel=0.005), rigid(foot))
        # A thick black sole, broader than the upper, as in the concept.
        self.add(loft(f'Sole {side}', frame, [
            section(-0.14, 0.094, 0.046, 3.0), section(-0.116, 0.125, 0.046, 3.0),
            section(reach + 0.125, 0.125, 0.046, 3.0), section(reach + 0.152, 0.094, 0.046, 3.0)],
            segments=56, fillet=0.012, material=m['graphite']), rigid(foot))

    def build(self):
        start = len(self.pieces)
        self.torso_chassis()
        self.torso_armour()
        # The torso is laid out at the design height of the hips; the idle's
        # crouch lowers them, and every torso piece with them.
        lower = Matrix.Translation((0.0, 0.0, self.lift))
        for obj, _ in self.pieces[start:]:
            obj.data.transform(lower)
        self.neck_chassis()
        self.helmet()
        for side, sgn in (('left', 1), ('right', -1)):
            self.limb_chassis(side, sgn)
            self.arm(side, sgn)
            self.leg(side, sgn)


# ── bind-space mapping ───────────────────────────────────────────────────────

def to_bind(obj, weights, pose):
    """Carry an idle-pose mesh back to bind space and write its skin weights."""
    apply_modifiers(obj)
    mesh = obj.data
    zero = Matrix(((0, 0, 0, 0),) * 4)
    groups = {}
    for v in mesh.vertices:
        w = weights(v.co.copy())
        total = sum(w.values())
        blended = zero.copy()
        for k, x in w.items():
            blended = blended + pose.deform[k] * (x / total)
        v.co = blended.inverted_safe() @ v.co
        for k, x in w.items():
            groups.setdefault(k, {}).setdefault(round(x / total, 6), []).append(v.index)
    mesh.update()
    for k, by_weight in groups.items():
        vg = obj.vertex_groups.get(pose.name[k]) or obj.vertex_groups.new(name=pose.name[k])
        for x, indices in by_weight.items():
            vg.add(indices, x, 'REPLACE')


def build(rig, idle_action, frame=IDLE_FRAME):
    """Forge every piece around the posed rig; return bind-space skinned parts."""
    scene = bpy.context.scene
    rig.animation_data.action = idle_action
    scene.frame_set(frame)
    bpy.context.view_layer.update()
    pose = Pose(rig)
    forge = Forge(pose, palette())
    forge.build()
    for obj, weights in forge.pieces:
        to_bind(obj, weights, pose)
    rig.animation_data.action = None
    for pb in rig.pose.bones:
        pb.matrix_basis.identity()
    scene.frame_set(0)
    parts = []
    for obj, _ in forge.pieces:
        obj.parent = rig
        obj.matrix_parent_inverse = rig.matrix_world.inverted()
        mod = obj.modifiers.new('Armature', 'ARMATURE')
        mod.object = rig
        parts.append(obj)
    return parts
