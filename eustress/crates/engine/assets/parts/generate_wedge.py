#!/usr/bin/env python3
"""Generate a proper wedge mesh with correct normals for Eustress Engine."""

import struct
import json
import base64

def generate_wedge_glb():
    """
    Generate a wedge (triangular prism) mesh with proper flat-shaded normals.
    
    The wedge is a right triangular prism:
    - Base is a right triangle in the XZ plane
    - Height extends along the Y axis
    - Vertices are duplicated per-face for flat shading (correct normals)
    
    Dimensions: 1x1x1 unit cube bounds (centered at origin)
    """
    
    # Wedge vertices (8 corners, but we need duplicates for flat shading)
    # The wedge has 5 faces: 2 triangular ends, 3 rectangular sides
    #
    # Top view (looking down -Y):
    #   
    #   (-0.5, y, -0.5) -------- (0.5, y, -0.5)
    #          \                      |
    #           \                     |
    #            \                    |
    #             \                   |
    #              \                  |
    #               (0.5, y, 0.5) ----+
    #
    # The sloped face goes from top-left to bottom-right
    
    # Define vertices per face for flat shading
    # Each face has its own set of vertices with the face normal
    
    vertices = []
    normals = []
    indices = []
    
    # Helper to add a quad (2 triangles)
    def add_quad(v0, v1, v2, v3, normal):
        """Add a quad as 2 triangles with the given normal."""
        base = len(vertices)
        vertices.extend([v0, v1, v2, v3])
        normals.extend([normal, normal, normal, normal])
        # Triangle 1: v0, v1, v2
        # Triangle 2: v0, v2, v3
        indices.extend([base, base+1, base+2, base, base+2, base+3])
    
    def add_tri(v0, v1, v2, normal):
        """Add a triangle with the given normal."""
        base = len(vertices)
        vertices.extend([v0, v1, v2])
        normals.extend([normal, normal, normal])
        indices.extend([base, base+1, base+2])
    
    # Wedge corner positions
    # Bottom triangle (y = -0.5)
    bl_back_left = (-0.5, -0.5, -0.5)   # Back left
    bl_back_right = (0.5, -0.5, -0.5)   # Back right  
    bl_front_right = (0.5, -0.5, 0.5)   # Front right
    
    # Top triangle (y = 0.5)
    tl_back_left = (-0.5, 0.5, -0.5)    # Back left
    tl_back_right = (0.5, 0.5, -0.5)    # Back right
    tl_front_right = (0.5, 0.5, 0.5)    # Front right
    
    # Face 1: Bottom face (Y = -0.5, normal pointing down)
    add_tri(bl_back_left, bl_front_right, bl_back_right, (0.0, -1.0, 0.0))
    
    # Face 2: Top face (Y = 0.5, normal pointing up)
    add_tri(tl_back_left, tl_back_right, tl_front_right, (0.0, 1.0, 0.0))
    
    # Face 3: Back face (Z = -0.5, normal pointing back)
    add_quad(bl_back_left, tl_back_left, tl_back_right, bl_back_right, (0.0, 0.0, -1.0))
    
    # Face 4: Right face (X = 0.5, normal pointing right)
    add_quad(bl_back_right, tl_back_right, tl_front_right, bl_front_right, (1.0, 0.0, 0.0))
    
    # Face 5: Sloped face (diagonal from back-left to front-right)
    # Normal is perpendicular to the slope: normalize(-1, 0, 1) = (-0.707, 0, 0.707)
    import math
    slope_normal = (-1.0 / math.sqrt(2), 0.0, 1.0 / math.sqrt(2))
    add_quad(bl_front_right, tl_front_right, tl_back_left, bl_back_left, slope_normal)
    
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
        "asset": {"version": "2.0", "generator": "Eustress Wedge Generator"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "Wedge"}],
        "meshes": [{
            "name": "Wedge",
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
    glb_data = generate_wedge_glb()
    with open("wedge.glb", "wb") as f:
        f.write(glb_data)
    print(f"Generated wedge.glb ({len(glb_data)} bytes)")
