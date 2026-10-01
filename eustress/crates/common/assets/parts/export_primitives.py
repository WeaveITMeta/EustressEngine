"""
Eustress Engine — Primitive Shape Exporter for Blender

Generates unit-scale .glb files for all 6 primitive part types.
Each mesh is centered at origin with unit dimensions (1m × 1m × 1m bounding box).
The engine scales these at runtime via BasePart.size.

Usage:
  blender --background --python export_primitives.py

Output:
  block.glb        — 1×1×1 cube
  ball.glb         — Sphere with radius 0.5 (diameter 1)
  cylinder.glb     — Cylinder with radius 0.5, height 1
  wedge.glb        — Right-angle wedge (ramp), 1×1×1 bounding box
  corner_wedge.glb — Corner wedge (pyramid corner), 1×1×1 bounding box
  cone.glb         — Cone with radius 0.5, height 1
"""

import bpy
import bmesh
import os
import math

# Output directory = same directory as this script
OUTPUT_DIR = os.path.dirname(os.path.abspath(__file__))


def clear_scene():
    """Remove all objects from the scene."""
    bpy.ops.object.select_all(action='SELECT')
    bpy.ops.object.delete(use_global=False)
    # Clear orphan data
    for block in bpy.data.meshes:
        if block.users == 0:
            bpy.data.meshes.remove(block)


def export_glb(name):
    """Export the active object as a .glb file."""
    filepath = os.path.join(OUTPUT_DIR, f"{name}.glb")
    bpy.ops.export_scene.gltf(
        filepath=filepath,
        export_format='GLB',
        use_selection=True,
        export_apply=True,
        export_yup=True,          # glTF uses Y-up
        export_texcoords=True,
        export_normals=True,
        export_materials='NONE',   # No materials — engine applies its own
        export_cameras=False,
        export_lights=False,
    )
    print(f"  Exported: {filepath}")


def make_block():
    """1×1×1 cube centered at origin."""
    clear_scene()
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, 0))
    obj = bpy.context.active_object
    obj.name = "Block"
    obj.select_set(True)
    export_glb("block")


def make_ball():
    """Sphere with radius 0.5 (diameter 1), centered at origin."""
    clear_scene()
    bpy.ops.mesh.primitive_uv_sphere_add(
        radius=0.5,
        segments=32,
        ring_count=16,
        location=(0, 0, 0)
    )
    obj = bpy.context.active_object
    obj.name = "Ball"
    # Smooth shading
    bpy.ops.object.shade_smooth()
    obj.select_set(True)
    export_glb("ball")


def make_cylinder():
    """Cylinder with radius 0.5, height 1, centered at origin."""
    clear_scene()
    bpy.ops.mesh.primitive_cylinder_add(
        radius=0.5,
        depth=1.0,
        vertices=32,
        location=(0, 0, 0)
    )
    obj = bpy.context.active_object
    obj.name = "Cylinder"
    bpy.ops.object.shade_smooth()
    obj.select_set(True)
    export_glb("cylinder")


def make_wedge():
    """Right-angle wedge (ramp shape), 1×1×1 bounding box.
    
    Vertices:
      Bottom face: (-0.5, -0.5, -0.5), (0.5, -0.5, -0.5), (0.5, -0.5, 0.5), (-0.5, -0.5, 0.5)
      Top edge:    (-0.5,  0.5, -0.5), (0.5,  0.5, -0.5)
    
    The wedge slopes from the back top edge down to the front bottom edge.
    """
    clear_scene()
    
    mesh = bpy.data.meshes.new("Wedge")
    obj = bpy.data.objects.new("Wedge", mesh)
    bpy.context.collection.objects.link(obj)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    
    bm = bmesh.new()
    
    # Bottom-front-left, bottom-front-right, bottom-back-right, bottom-back-left
    v0 = bm.verts.new((-0.5, -0.5,  0.5))  # front-left bottom
    v1 = bm.verts.new(( 0.5, -0.5,  0.5))  # front-right bottom
    v2 = bm.verts.new(( 0.5, -0.5, -0.5))  # back-right bottom
    v3 = bm.verts.new((-0.5, -0.5, -0.5))  # back-left bottom
    # Top-back-left, top-back-right (the top edge of the ramp)
    v4 = bm.verts.new((-0.5,  0.5, -0.5))  # back-left top
    v5 = bm.verts.new(( 0.5,  0.5, -0.5))  # back-right top
    
    # Faces
    bm.faces.new([v0, v1, v2, v3])  # Bottom
    bm.faces.new([v3, v2, v5, v4])  # Back (vertical)
    bm.faces.new([v0, v1, v5, v4])  # Slope (ramp surface)
    bm.faces.new([v0, v3, v4])      # Left triangle
    bm.faces.new([v1, v2, v5])      # Right triangle
    
    bm.to_mesh(mesh)
    bm.free()
    
    # Recalculate normals (Blender 4.x handles this automatically)
    mesh.update()
    
    export_glb("wedge")


def make_corner_wedge():
    """Corner wedge (pyramid corner), 1×1×1 bounding box.
    
    Like a wedge but tapers to a single top vertex instead of a top edge.
    
    Vertices:
      Bottom face: (-0.5, -0.5, -0.5), (0.5, -0.5, -0.5), (0.5, -0.5, 0.5), (-0.5, -0.5, 0.5)
      Top vertex:  (-0.5,  0.5, -0.5)
    """
    clear_scene()
    
    mesh = bpy.data.meshes.new("CornerWedge")
    obj = bpy.data.objects.new("CornerWedge", mesh)
    bpy.context.collection.objects.link(obj)
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    
    bm = bmesh.new()
    
    v0 = bm.verts.new((-0.5, -0.5,  0.5))  # front-left bottom
    v1 = bm.verts.new(( 0.5, -0.5,  0.5))  # front-right bottom
    v2 = bm.verts.new(( 0.5, -0.5, -0.5))  # back-right bottom
    v3 = bm.verts.new((-0.5, -0.5, -0.5))  # back-left bottom
    v4 = bm.verts.new((-0.5,  0.5, -0.5))  # top vertex (back-left corner)
    
    # Faces
    bm.faces.new([v0, v1, v2, v3])  # Bottom
    bm.faces.new([v3, v2, v4])      # Back triangle
    bm.faces.new([v0, v3, v4])      # Left triangle
    bm.faces.new([v0, v1, v4])      # Front slope
    bm.faces.new([v1, v2, v4])      # Right slope
    
    bm.to_mesh(mesh)
    bm.free()
    
    mesh.update()
    
    export_glb("corner_wedge")


def make_cone():
    """Cone with radius 0.5, height 1, centered at origin."""
    clear_scene()
    bpy.ops.mesh.primitive_cone_add(
        radius1=0.5,
        radius2=0.0,
        depth=1.0,
        vertices=32,
        location=(0, 0, 0)
    )
    obj = bpy.context.active_object
    obj.name = "Cone"
    bpy.ops.object.shade_smooth()
    obj.select_set(True)
    export_glb("cone")


# ============================================================================
# Main
# ============================================================================

if __name__ == "__main__":
    print("=" * 60)
    print("Eustress Engine — Primitive Shape Exporter")
    print(f"Output directory: {OUTPUT_DIR}")
    print("=" * 60)
    
    print("\n1/6 Block (1×1×1 cube)")
    make_block()
    
    print("\n2/6 Ball (sphere, diameter 1)")
    make_ball()
    
    print("\n3/6 Cylinder (radius 0.5, height 1)")
    make_cylinder()
    
    print("\n4/6 Wedge (ramp, 1×1×1)")
    make_wedge()
    
    print("\n5/6 CornerWedge (pyramid corner, 1×1×1)")
    make_corner_wedge()
    
    print("\n6/6 Cone (radius 0.5, height 1)")
    make_cone()
    
    print("\n" + "=" * 60)
    print("Done! All 6 primitives exported as .glb")
    print("=" * 60)
