#!/usr/bin/env python3
"""Generate a proper corner wedge mesh with correct normals for Eustress Engine."""

import struct
import json
import math

def generate_corner_wedge_glb():
    """
    Generate a corner wedge (tetrahedron-like) mesh with proper flat-shaded normals.
    
    A corner wedge is like a cube with one corner cut off diagonally,
    leaving 4 faces: 3 right-angle faces and 1 diagonal slope face.
    
    It's used to fill corners where two wedges meet.
    
    Vertices (4 corners of a tetrahedron-like shape):
    - Bottom back left:  (-0.5, -0.5, -0.5)
    - Bottom back right: (0.5, -0.5, -0.5)
    - Bottom front left: (-0.5, -0.5, 0.5)
    - Top back left:     (-0.5, 0.5, -0.5)
    
    Faces:
    1. Bottom (Y = -0.5): triangle
    2. Back (Z = -0.5): triangle  
    3. Left (X = -0.5): triangle
    4. Slope (diagonal): triangle connecting the 3 outer corners
    """
    
    vertices = []
    normals = []
    indices = []
    
    def add_tri(v0, v1, v2, normal):
        """Add a triangle with the given normal."""
        base = len(vertices)
        vertices.extend([v0, v1, v2])
        normals.extend([normal, normal, normal])
        indices.extend([base, base+1, base+2])
    
    # Corner wedge vertices
    bottom_back_left = (-0.5, -0.5, -0.5)
    bottom_back_right = (0.5, -0.5, -0.5)
    bottom_front_left = (-0.5, -0.5, 0.5)
    top_back_left = (-0.5, 0.5, -0.5)
    
    # Face 1: Bottom face (Y = -0.5, normal pointing down)
    # Triangle: bottom_back_left, bottom_front_left, bottom_back_right
    add_tri(bottom_back_left, bottom_front_left, bottom_back_right, (0.0, -1.0, 0.0))
    
    # Face 2: Back face (Z = -0.5, normal pointing back)
    # Triangle: bottom_back_left, bottom_back_right, top_back_left
    add_tri(bottom_back_left, bottom_back_right, top_back_left, (0.0, 0.0, -1.0))
    
    # Face 3: Left face (X = -0.5, normal pointing left)
    # Triangle: bottom_back_left, top_back_left, bottom_front_left
    add_tri(bottom_back_left, top_back_left, bottom_front_left, (-1.0, 0.0, 0.0))
    
    # Face 4: Slope face (diagonal)
    # Triangle: bottom_back_right, bottom_front_left, top_back_left
    # Normal is perpendicular to this diagonal plane
    # Calculate normal from cross product of two edges
    # Edge 1: bottom_front_left - bottom_back_right = (-1, 0, 1)
    # Edge 2: top_back_left - bottom_back_right = (-1, 1, 0)
    # Cross: (0*0 - 1*1, 1*(-1) - (-1)*0, (-1)*1 - 0*(-1)) = (-1, -1, -1)
    # Normalized: (-1, -1, -1) / sqrt(3) = (-0.577, -0.577, -0.577)
    # But we want outward facing, so: (1, 1, 1) / sqrt(3)
    slope_len = math.sqrt(3)
    slope_normal = (1.0 / slope_len, 1.0 / slope_len, 1.0 / slope_len)
    add_tri(bottom_back_right, bottom_front_left, top_back_left, slope_normal)
    
    # Convert to binary buffers
    vertex_data = b''.join(struct.pack('<fff', *v) for v in vertices)
    normal_data = b''.join(struct.pack('<fff', *n) for n in normals)
    index_data = b''.join(struct.pack('<H', i) for i in indices)
    
    # Pad to 4-byte alignment
    def pad4(data):
        padding = (4 - len(data) % 4) % 4
        return data + b'\x00' * padding
    
    vertex_data = pad4(vertex_data)
    normal_data = pad4(normal_data)
    index_data = pad4(index_data)
    
    # Calculate bounds
    min_pos = [min(v[i] for v in vertices) for i in range(3)]
    max_pos = [max(v[i] for v in vertices) for i in range(3)]
    
    # Build glTF JSON
    buffer_data = vertex_data + normal_data + index_data
    
    gltf = {
        "asset": {"version": "2.0", "generator": "Eustress Corner Wedge Generator"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "CornerWedge"}],
        "meshes": [{
            "name": "CornerWedge",
            "primitives": [{
                "attributes": {
                    "POSITION": 0,
                    "NORMAL": 1
                },
                "indices": 2,
                "mode": 4  # TRIANGLES
            }]
        }],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,  # FLOAT
                "count": len(vertices),
                "type": "VEC3",
                "min": min_pos,
                "max": max_pos
            },
            {
                "bufferView": 1,
                "componentType": 5126,  # FLOAT
                "count": len(normals),
                "type": "VEC3"
            },
            {
                "bufferView": 2,
                "componentType": 5123,  # UNSIGNED_SHORT
                "count": len(indices),
                "type": "SCALAR"
            }
        ],
        "bufferViews": [
            {
                "buffer": 0,
                "byteOffset": 0,
                "byteLength": len(vertex_data),
                "target": 34962  # ARRAY_BUFFER
            },
            {
                "buffer": 0,
                "byteOffset": len(vertex_data),
                "byteLength": len(normal_data),
                "target": 34962  # ARRAY_BUFFER
            },
            {
                "buffer": 0,
                "byteOffset": len(vertex_data) + len(normal_data),
                "byteLength": len(index_data),
                "target": 34963  # ELEMENT_ARRAY_BUFFER
            }
        ],
        "buffers": [{
            "byteLength": len(buffer_data)
        }]
    }
    
    # Build GLB
    json_str = json.dumps(gltf, separators=(',', ':'))
    json_bytes = json_str.encode('utf-8')
    # Pad JSON to 4-byte alignment
    json_padding = (4 - len(json_bytes) % 4) % 4
    json_bytes += b' ' * json_padding
    
    # GLB header
    glb_header = struct.pack('<III', 
        0x46546C67,  # magic "glTF"
        2,           # version
        12 + 8 + len(json_bytes) + 8 + len(buffer_data)  # total length
    )
    
    # JSON chunk
    json_chunk = struct.pack('<II', len(json_bytes), 0x4E4F534A) + json_bytes  # "JSON"
    
    # Binary chunk
    bin_chunk = struct.pack('<II', len(buffer_data), 0x004E4942) + buffer_data  # "BIN\0"
    
    return glb_header + json_chunk + bin_chunk


if __name__ == "__main__":
    glb_data = generate_corner_wedge_glb()
    with open("corner_wedge.glb", "wb") as f:
        f.write(glb_data)
    print(f"Generated corner_wedge.glb ({len(glb_data)} bytes)")
