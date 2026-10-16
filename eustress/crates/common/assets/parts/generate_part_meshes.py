#!/usr/bin/env python3
"""The wedge and corner-wedge part meshes, from one vertex table.

Roblox's shapes, in a unit box centred on the origin (a part of Size 1, 1, 1),
with the part's front at -Z (its LookVector), right at +X and top at +Y:

- WedgePart: the bottom and the back (+Z) are full squares; the slope runs from
  the top back edge down to the bottom front edge, facing up and toward the
  front. Six corners: the bottom four, and (-0.5, 0.5, 0.5), (0.5, 0.5, 0.5).
- CornerWedgePart: a square base and one apex, over the right front corner at
  (0.5, 0.5, -0.5): a third of the box.

Each face is flat-shaded (its own vertices and normal), wound counter-clockwise
seen from outside (glTF front faces), with planar UVs in metres along the face.
`common/src/part_meshes.rs` holds the same table for the colliders, and its test
reads these files back against it, so change both together.

usage: python generate_part_meshes.py   (writes wedge.glb and corner_wedge.glb beside this file)
"""

import json
import math
import struct
from pathlib import Path

# Corners, then faces as loops of corner indices, counter-clockwise seen from
# outside, exactly as common/src/part_meshes.rs lists them (the writer would wind
# a stray loop outward anyway). Keep the two in step.
WEDGE = {
    "corners": [
        (-0.5, -0.5, -0.5),  # 0 bottom front left
        (0.5, -0.5, -0.5),   # 1 bottom front right
        (0.5, -0.5, 0.5),    # 2 bottom back right
        (-0.5, -0.5, 0.5),   # 3 bottom back left
        (-0.5, 0.5, 0.5),    # 4 top back left
        (0.5, 0.5, 0.5),     # 5 top back right
    ],
    "faces": [
        (0, 1, 2, 3),  # bottom
        (3, 2, 5, 4),  # back
        (1, 0, 4, 5),  # slope
        (0, 3, 4),     # left
        (1, 5, 2),     # right
    ],
}
CORNER_WEDGE = {
    "corners": [
        (-0.5, -0.5, -0.5),  # 0 bottom front left
        (0.5, -0.5, -0.5),   # 1 bottom front right
        (0.5, -0.5, 0.5),    # 2 bottom back right
        (-0.5, -0.5, 0.5),   # 3 bottom back left
        (0.5, 0.5, -0.5),    # 4 apex, over the front right corner
    ],
    "faces": [
        (0, 1, 2, 3),  # bottom
        (0, 4, 1),     # front (vertical)
        (1, 4, 2),     # right (vertical)
        (2, 4, 3),     # back slope
        (3, 4, 0),     # left slope
    ],
}


def sub(a, b):
    return tuple(a[i] - b[i] for i in range(3))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def dot(a, b):
    return sum(a[i] * b[i] for i in range(3))


def unit(a):
    n = math.sqrt(dot(a, a))
    return tuple(x / n for x in a)


def build(shape):
    corners = shape["corners"]
    centroid = tuple(sum(c[i] for c in corners) / len(corners) for i in range(3))
    positions, normals, uvs, indices = [], [], [], []
    for face in shape["faces"]:
        loop = [corners[i] for i in face]
        normal = unit(cross(sub(loop[1], loop[0]), sub(loop[2], loop[0])))
        center = tuple(sum(p[i] for p in loop) / len(loop) for i in range(3))
        if dot(normal, sub(center, centroid)) < 0:
            loop.reverse()
            normal = tuple(-x for x in normal)
        # Planar UVs: metres along the face's first edge and across it.
        u_axis = unit(sub(loop[1], loop[0]))
        v_axis = cross(normal, u_axis)
        base = len(positions)
        for p in loop:
            d = sub(p, loop[0])
            positions.append(p)
            normals.append(normal)
            uvs.append((dot(d, u_axis), dot(d, v_axis)))
        for k in range(1, len(loop) - 1):
            indices += [base, base + k, base + k + 1]
    return positions, normals, uvs, indices


def glb(positions, normals, uvs, indices, generator):
    def pack(fmt, rows):
        return b"".join(struct.pack(fmt, *r) for r in rows)

    def pad(data, fill):
        return data + fill * (-len(data) % 4)

    blobs = [
        pad(pack("<3f", positions), b"\0"),
        pad(pack("<3f", normals), b"\0"),
        pad(pack("<2f", uvs), b"\0"),
        pad(b"".join(struct.pack("<H", i) for i in indices), b"\0"),
    ]
    views, offset = [], 0
    for blob, target in zip(blobs, [34962, 34962, 34962, 34963]):
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(blob), "target": target})
        offset += len(blob)
    lo = [min(p[i] for p in positions) for i in range(3)]
    hi = [max(p[i] for p in positions) for i in range(3)]
    doc = {
        "asset": {"version": "2.0", "generator": generator},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2}, "indices": 3}]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": len(positions), "type": "VEC3", "min": lo, "max": hi},
            {"bufferView": 1, "componentType": 5126, "count": len(normals), "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": len(uvs), "type": "VEC2"},
            {"bufferView": 3, "componentType": 5123, "count": len(indices), "type": "SCALAR"},
        ],
        "bufferViews": views,
        "buffers": [{"byteLength": offset}],
    }
    js = pad(json.dumps(doc, separators=(",", ":")).encode("utf-8"), b" ")
    bin_ = b"".join(blobs)
    total = 12 + 8 + len(js) + 8 + len(bin_)
    return (
        struct.pack("<III", 0x46546C67, 2, total)
        + struct.pack("<II", len(js), 0x4E4F534A) + js
        + struct.pack("<II", len(bin_), 0x004E4942) + bin_
    )


def main():
    here = Path(__file__).resolve().parent
    for name, shape in [("wedge.glb", WEDGE), ("corner_wedge.glb", CORNER_WEDGE)]:
        data = glb(*build(shape), generator="Eustress generate_part_meshes.py")
        (here / name).write_bytes(data)
        print(f"wrote {name}: {len(data)} bytes")


if __name__ == "__main__":
    main()
