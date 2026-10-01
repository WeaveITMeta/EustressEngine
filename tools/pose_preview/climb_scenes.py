"""Render every authored climb pose on the Y Bot, against a wall or a ledge.

    python tools/pose_preview/climb_scenes.py [out_dir] [--only NAME ...]

Reads the pose tables straight out of `eustress/crates/common/src/avatar/ik.rs`,
so a preview always shows the poses the engine ships. Places the body and the
geometry the way `climb.rs` places them, runs the same two-bone IK for hands and
feet, renders a back view and a side view of each scene with Blender, and joins
them into `sheet.png` in the output folder.

Blender comes from the `BLENDER` environment variable, else the default Windows
install of Blender 4.4. `EUSTRESS_IK_RS` points at another copy of `ik.rs`, to
preview poses before they are in the tree. The sheet needs Pillow; without it
the separate renders are still written.
"""
import json
import math
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))
IK_RS = os.environ.get("EUSTRESS_IK_RS") or os.path.join(
    REPO, "eustress", "crates", "common", "src", "avatar", "ik.rs"
)
BLENDER = os.environ.get("BLENDER", r"C:\Program Files\Blender Foundation\Blender 4.4\blender.exe")

# ── Poses, parsed from ik.rs ────────────────────────────────────────────────

POSE_RE = re.compile(r"const (\w+): &\[PoseDir\] = &\[(.*?)\n\];", re.S)
ENTRY_RE = re.compile(r"\(HumanoidBone::(\w+),\s*HumanoidBone::(\w+),\s*\[([^\]]+)\]\)")


def load_poses():
    src = open(IK_RS, encoding="utf-8").read()
    poses = {}
    for name, body in POSE_RE.findall(src):
        entries = []
        for bone, child, dirs in ENTRY_RE.findall(body):
            entries.append((bone, child, [float(v) for v in dirs.split(",")]))
        poses[name] = entries
    return poses


# ── The geometry climb.rs builds, for the Y Bot (1.785 m) ──────────────────
#
# Mirrored from climb.rs and ik.rs; the comment on each names its source.

H = 1.785                    # the Y Bot's standing height
HALF = H * 0.5               # capsule half extent (feet on the capsule bottom)
R = 0.27                     # capsule radius
ARM = H * 0.30               # ARM_SPAN_FRAC
SPAN_LEG = 0.826             # hip to ankle on the Y Bot
HW = 0.188                   # shoulder half width
STANDOFF = R * 1.08          # HANG_STANDOFF
FREE_STANDOFF = R * 0.45     # FREE_HANG_STANDOFF
HANG_DROP = H * 0.30 * 0.70 + H * 0.33   # hang_drop()
WRIST_BELOW = H * 0.055      # WRIST_BELOW_LIP_FRAC
BRACED_LEG_REACH = 0.66      # ik.rs
HIP = {"Left": (0.091, 0.001, 0.0385), "Right": (-0.091, 0.001, 0.0385)}

# Blender frame: forward (into the wall) is -Y, the character's right is -X,
# up is +Z. The wall normal n points back at the climber (+Y); the lip
# tangent is the character's right (-X).
N = (0.0, 1.0, 0.0)
T = (-1.0, 0.0, 0.0)


def add(a, b, s=1.0):
    return [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]


def wall(lip, depth=1.0, height=4.0, width=4.0):
    return {"centre": [lip[0], lip[1] - depth / 2, lip[2] - height / 2], "size": [width, depth, height]}


def arms_ik(points, reaching=0, press=False, below=WRIST_BELOW):
    out = []
    for idx, (side, up, lo, end) in enumerate([(-1, "LeftArm", "LeftForeArm", "LeftHand"),
                                                (1, "RightArm", "RightForeArm", "RightHand")]):
        if press:
            pole = add(add((0, 0, 0), N, ARM * 0.9), T, side * ARM * 0.45)
            pull = 0.0
        else:
            pole = add(add((0, 0, 0), (0, 0, -ARM * 0.85)), T, side * ARM * 0.55)
            pull = ARM * 0.16 if idx != reaching else 0.0
        out.append({"up": up, "lo": lo, "end": end, "target": add(points[idx], (0, 0, -below)),
                    "pole_rel": pole, "w": 1.0, "pull": pull, "push_out": True})
    return out


def braced_feet(root, face_y, w, sides=("Left", "Right")):
    out = []
    for side_name in sides:
        s = 1.0 if side_name == "Left" else -1.0
        hip = add(root, HIP[side_name])
        lat = 0.06
        across = math.hypot(hip[1] - (face_y + 0.05), lat)
        reach = BRACED_LEG_REACH * SPAN_LEG
        drop = math.sqrt(max(reach * reach - across * across, 0.0))
        target = [hip[0] + s * lat, face_y + 0.05, hip[2] - drop]
        out.append({"up": f"{side_name}UpLeg", "lo": f"{side_name}Leg", "end": f"{side_name}Foot",
                    "target": target, "pole_rel": [s * 0.42, -0.28, -0.28], "w": w, "push_out": False})
    return out


def scenes(P):
    root = [0.0, 0.0, 1.30]
    lip = [root[0], root[1] - STANDOFF, root[2] + HANG_DROP]
    holds = [add(lip, T, -HW), add(lip, T, HW)]
    bar = {"centre": [lip[0], lip[1] - 0.06, lip[2] - 0.06], "size": [3.0, 0.12, 0.12]}
    free_root = [0.0, lip[1] + FREE_STANDOFF, root[2]]

    # A swing of 0.3 rad carries the feet toward the wall (-Y).
    a = 0.3
    rel = [free_root[i] - [0.0, lip[1], lip[2]][i] for i in range(3)]
    swung = [0.0, lip[1] + rel[1] * math.cos(a) + rel[2] * math.sin(a),
             lip[2] - rel[1] * math.sin(a) + rel[2] * math.cos(a)]

    pull_root = [0.0, lip[1] + STANDOFF, lip[2] + H * (0.06 - 0.33)]
    knee_root = [0.0, lip[1] + R * 0.75, lip[2] - H * 0.01]
    press = [add(add(lip, T, -HW * 1.15), N, -0.14), add(add(lip, T, HW * 1.15), N, -0.14)]
    ground = {"centre": [0.0, 0.0, -0.5], "size": [6.0, 6.0, 1.0]}
    block = {"centre": [0.0, -0.8, 0.45], "size": [3.0, 0.8, 0.9]}

    def knee(lead):
        side = "Right" if lead == "Right" else "Left"
        shin = next(d for b, _, d in P["MANTLE_KNEE_POSE"] if b == "RightLeg")
        if lead == "Left":
            shin = [-shin[0], shin[1], shin[2]]
        return [{"thigh": f"{side}UpLeg", "shin": f"{side}Leg", "foot": f"{side}Foot",
                 "plane_z": lip[2] + 0.06, "forward": [0, -1, 0], "shin_dir": shin}]

    return [
        {"name": "hang_braced", "root": root, "layers": [{"pose": P["HANG_POSE"]}],
         "ik": arms_ik(holds) + braced_feet(root, lip[1], 0.75), "props": [wall(lip)]},
        {"name": "hang_free", "root": free_root,
         "layers": [{"pose": P["HANG_POSE"]}, {"pose": P["FREE_HANG_POSE"]}],
         "ik": arms_ik(holds), "props": [bar]},
        {"name": "hang_free_swinging", "root": swung, "tilt_deg": -math.degrees(a),
         "layers": [{"pose": P["HANG_POSE"]}, {"pose": P["FREE_HANG_POSE"]}],
         "ik": arms_ik(holds), "props": [bar]},
        {"name": "leap_gather", "root": root,
         "layers": [{"pose": P["HANG_POSE"]}, {"pose": P["LEAP_TUCK_POSE"], "w": 0.9}],
         "ik": arms_ik(holds), "props": [wall(lip)]},
        {"name": "mantle_pull", "root": pull_root, "layers": [{"pose": P["MANTLE_POSE"]}],
         "ik": arms_ik(holds) + braced_feet(pull_root, lip[1], 0.45), "props": [wall(lip)]},
        {"name": "mantle_knee_right", "root": knee_root,
         "layers": [{"pose": P["MANTLE_POSE"]}, {"pose": P["MANTLE_KNEE_POSE"]}],
         "ik": arms_ik([add(p, (0, 0, 0.035)) for p in press], press=True, below=0.0)
               + braced_feet(knee_root, lip[1], 0.45, sides=("Left",)),
         "knee_on_top": knee("Right"), "props": [wall(lip)]},
        {"name": "mantle_knee_left", "root": knee_root,
         "layers": [{"pose": P["MANTLE_POSE"]}, {"pose": P["MANTLE_KNEE_POSE"], "mirror": True}],
         "ik": arms_ik([add(p, (0, 0, 0.035)) for p in press], press=True, below=0.0)
               + braced_feet(knee_root, lip[1], 0.45, sides=("Right",)),
         "knee_on_top": knee("Left"), "props": [wall(lip)]},
        {"name": "mantle_stand", "root": [0.0, lip[1] - 0.15, lip[2] + 0.62],
         "layers": [{"pose": P["MANTLE_POSE"]}, {"pose": P["MANTLE_KNEE_POSE"]},
                    {"pose": P["MANTLE_STAND_POSE"]}],
         "props": [wall(lip)]},
        {"name": "vault_launch", "root": [0.0, 0.1, HALF + 0.35],
         "layers": [{"pose": P["VAULT_LAUNCH_POSE"]}], "props": [ground, block]},
        {"name": "vault_land", "root": [0.0, -0.6, HALF + 0.92],
         "layers": [{"pose": P["VAULT_LAUNCH_POSE"]}, {"pose": P["VAULT_LAND_POSE"]}],
         "props": [ground, block]},
    ]


def sheet(out_dir, names):
    try:
        from PIL import Image, ImageDraw
    except ImportError:
        print("Pillow is not installed; the renders are in", out_dir)
        return
    W, Hh = 520, 560
    img = Image.new("RGB", (W * 2, Hh * len(names)), "white")
    draw = ImageDraw.Draw(img)
    for r, n in enumerate(names):
        for c, v in enumerate(["back", "side"]):
            p = os.path.join(out_dir, f"{n}__{v}.png")
            if os.path.exists(p):
                img.paste(Image.open(p).convert("RGB"), (c * W, r * Hh))
            draw.rectangle([c * W, r * Hh, c * W + 300, r * Hh + 18], fill="black")
            draw.text((c * W + 4, r * Hh + 3), f"{n} / {v}", fill="yellow")
    path = os.path.join(out_dir, "sheet.png")
    img.save(path)
    print("sheet:", path)


def main():
    args = sys.argv[1:]
    only = []
    if "--only" in args:
        i = args.index("--only")
        only = args[i + 1:]
        args = args[:i]
    out_dir = os.path.abspath(args[0] if args else os.path.join(HERE, "out"))
    os.makedirs(out_dir, exist_ok=True)

    poses = load_poses()
    chosen = [s for s in scenes(poses) if not only or any(s["name"].startswith(o) for o in only)]
    spec = os.path.join(out_dir, "scenes.json")
    with open(spec, "w") as f:
        json.dump(chosen, f)
    subprocess.run(
        [BLENDER, "-b", "--factory-startup", "-P", os.path.join(HERE, "render.py"), "--", spec, out_dir],
        check=True,
    )
    sheet(out_dir, [s["name"] for s in chosen])


if __name__ == "__main__":
    main()
