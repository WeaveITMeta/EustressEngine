"""
generate_cube.py -- Blender 4.4 Python script
Generates the Voltec Cube V1 mesh package and exports to cube.glb

Run from command line:
  "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe"
      --background --python generate_cube.py --
      --output "e:/Workspace/EustressEngine/eustress/crates/workshop/cube/V1/meshes/cube.glb"

All dimensions in millimetres. Scale to metres is applied in the .glb.toml instance file.
"""

import bpy  # type: ignore[import]  — only available inside Blender runtime
import math
import sys
import os
import argparse

# ── Argument parsing ──────────────────────────────────────────────────────────

def parse_args():
    argv = sys.argv
    if "--" in argv:
        argv = argv[argv.index("--") + 1:]
    else:
        argv = []
    parser = argparse.ArgumentParser(description="Generate Voltec Cube V1 mesh")
    parser.add_argument(
        "--output",
        default=os.path.join(os.path.dirname(__file__), "../cube.glb"),
        help="Output .glb file path",
    )
    return parser.parse_args(argv)


# ── Scene setup ───────────────────────────────────────────────────────────────

def clear_scene():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete()
    for block in list(bpy.data.meshes):
        bpy.data.meshes.remove(block)
    for block in list(bpy.data.materials):
        bpy.data.materials.remove(block)


# ── Material helpers ──────────────────────────────────────────────────────────

def make_material(name, base_color, metallic=0.0, roughness=0.5, alpha=1.0):
    mat = bpy.data.materials.new(name=name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (*base_color, alpha)
    bsdf.inputs["Metallic"].default_value = metallic
    bsdf.inputs["Roughness"].default_value = roughness
    if alpha < 1.0:
        mat.blend_method = "BLEND"
    return mat


def assign_material(obj, mat):
    obj.data.materials.clear()
    obj.data.materials.append(mat)


# ── Geometry helpers ──────────────────────────────────────────────────────────

def add_box(name, size_xyz, location=(0, 0, 0), bevel_width=0.3, bevel_segments=3):
    """Create a bevelled rectangular box centred at origin, then translated."""
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0, 0, 0))
    obj = bpy.context.active_object
    obj.name = name
    obj.scale = (size_xyz[0], size_xyz[1], size_xyz[2])
    bpy.ops.object.transform_apply(scale=True)

    bpy.ops.object.modifier_add(type="BEVEL")
    mod = obj.modifiers["Bevel"]
    mod.width = bevel_width
    mod.segments = bevel_segments
    mod.limit_method = "ANGLE"
    bpy.ops.object.modifier_apply(modifier="Bevel")

    obj.location = location
    return obj


def add_cylinder(name, radius, depth, location=(0, 0, 0), vertices=32):
    bpy.ops.mesh.primitive_cylinder_add(
        radius=radius, depth=depth, vertices=vertices, location=location
    )
    obj = bpy.context.active_object
    obj.name = name
    return obj


def add_sphere(name, radius, location=(0, 0, 0)):
    bpy.ops.mesh.primitive_uv_sphere_add(
        radius=radius, location=location, segments=24, ring_count=16
    )
    obj = bpy.context.active_object
    obj.name = name
    bpy.ops.object.shade_smooth()
    return obj


def add_torus(name, major_radius, minor_radius, location=(0, 0, 0)):
    bpy.ops.mesh.primitive_torus_add(
        major_radius=major_radius,
        minor_radius=minor_radius,
        major_segments=48,
        minor_segments=12,
        location=location,
    )
    obj = bpy.context.active_object
    obj.name = name
    return obj


def add_ic_package(name, width, depth, height, location=(0, 0, 0)):
    """Simplified QFN IC package with chamfered top edge."""
    obj = add_box(name, (width, depth, height), location=location, bevel_width=0.1, bevel_segments=2)
    return obj


def add_coil(name, radius, wire_radius, turns, pitch, location=(0, 0, 0)):
    """Generate a helix coil mesh using a screw modifier on a circle."""
    bpy.ops.curve.primitive_bezier_circle_add(radius=wire_radius, location=(0, 0, 0))
    profile = bpy.context.active_object
    profile.name = name + "_profile"

    bpy.ops.mesh.primitive_circle_add(radius=radius, vertices=64, location=(0, 0, 0))
    path_obj = bpy.context.active_object
    path_obj.name = name + "_path"

    # Use screw modifier to generate helix
    bpy.ops.mesh.primitive_cylinder_add(radius=0.0, depth=0.0, location=(0, 0, 0), vertices=4)
    coil = bpy.context.active_object
    coil.name = name

    screw_mod = coil.modifiers.new(name="Screw", type="SCREW")
    screw_mod.angle = math.radians(360 * turns)
    screw_mod.screw_offset = pitch
    screw_mod.steps = 64 * turns
    screw_mod.render_steps = 64 * turns

    bpy.ops.object.modifier_apply(modifier="Screw")

    bpy.data.objects.remove(path_obj)
    bpy.data.objects.remove(profile)

    coil.location = location
    return coil


# ── Main mesh generation ──────────────────────────────────────────────────────

def build_cube_v1():
    """Build all mesh objects for The Cube V1."""

    # ── Materials ─────────────────────────────────────────────────────────────
    mat_al      = make_material("Al6061_Anodised",  (0.72, 0.72, 0.72), metallic=0.95, roughness=0.20)
    mat_fr4     = make_material("FR4_ENIG",         (0.05, 0.28, 0.05), metallic=0.0,  roughness=0.60)
    mat_ic      = make_material("IC_Black_Mold",    (0.04, 0.04, 0.04), metallic=0.0,  roughness=0.80)
    mat_pzt     = make_material("PZT_Grey",         (0.65, 0.63, 0.60), metallic=0.0,  roughness=0.50)
    mat_ndfeb   = make_material("NdFeB_Chrome",     (0.85, 0.85, 0.88), metallic=1.0,  roughness=0.05)
    mat_copper  = make_material("Copper_Coil",      (0.72, 0.45, 0.20), metallic=0.95, roughness=0.25)
    mat_silicone = make_material("Silicone_Black",  (0.05, 0.05, 0.05), metallic=0.0,  roughness=0.90)
    mat_cap     = make_material("Capacitor_Silver", (0.80, 0.80, 0.82), metallic=0.70, roughness=0.30)

    # ── Housing bottom (1mm thick, 18×18mm, 4mm tall) ─────────────────────────
    # Housing is two halves: bottom tray (4mm) + top lid (1mm)
    housing_bot = add_box("Housing_Bottom", (9.0, 9.0, 2.0), location=(0, 0, 0))
    assign_material(housing_bot, mat_al)

    # ── Housing top lid (1mm) ─────────────────────────────────────────────────
    housing_top = add_box("Housing_Top", (9.0, 9.0, 0.5), location=(0, 0, 5.0))
    assign_material(housing_top, mat_al)

    # ── O-ring gasket ─────────────────────────────────────────────────────────
    # Torus: major radius 7.5mm, minor radius 0.5mm, at Z=4.7mm
    oring = add_torus("Oring_Gasket", major_radius=7.5, minor_radius=0.4, location=(0, 0, 4.7))
    assign_material(oring, mat_silicone)

    # ── PCB (17.6×17.6×1.2mm at Z=1mm) ───────────────────────────────────────
    pcb = add_box("PCB", (8.8, 8.8, 0.6), location=(0, 0, 1.0), bevel_width=0.05, bevel_segments=1)
    assign_material(pcb, mat_fr4)

    # ── nRF52840 SoC (7×7×0.9mm QFN at Z=2.2mm) ──────────────────────────────
    nrf = add_ic_package("nRF52840", 3.5, 3.5, 0.45, location=(3.0, 2.0, 2.2))
    assign_material(nrf, mat_ic)

    # ── AEM10941 PMIC (4×4×0.9mm QFN) ────────────────────────────────────────
    aem = add_ic_package("AEM10941", 2.0, 2.0, 0.45, location=(-3.0, 2.0, 2.2))
    assign_material(aem, mat_ic)

    # ── Supercap 100uF (1206 SMD equivalent, 3.2×1.6×1.1mm) ─────────────────
    cap = add_box("Supercap", (1.6, 0.8, 0.55), location=(0, -3.5, 2.2), bevel_width=0.05, bevel_segments=1)
    assign_material(cap, mat_cap)

    # ── PZT disc array (3× discs stacked, 12mm diameter, 0.5mm each) ─────────
    # Represent as a single flat cylinder stack
    pzt = add_cylinder("PZT_Array", radius=6.0, depth=1.5, location=(0, 0, 2.5))
    assign_material(pzt, mat_pzt)
    bpy.ops.object.shade_smooth()

    # ── EM cavity bore (14mm diameter through PCB layer) ─────────────────────
    em_cavity = add_cylinder("EM_Cavity", radius=7.0, depth=2.5, location=(0, 0, 3.5))
    assign_material(em_cavity, mat_fr4)
    bpy.ops.object.shade_smooth()

    # ── NdFeB N52 sphere (8mm diameter) ──────────────────────────────────────
    ndfeb = add_sphere("NdFeB_Sphere", radius=4.0, location=(0, 0, 3.5))
    assign_material(ndfeb, mat_ndfeb)

    # ── Coil winding (simplified torus representing wound copper) ─────────────
    coil = add_torus("Coil_Winding", major_radius=6.5, minor_radius=0.8, location=(0, 0, 3.5))
    assign_material(coil, mat_copper)

    print("[Cube V1] All mesh objects created.")
    inner_mesh_objects = [
        housing_bot, housing_top, oring, pcb,
        nrf, aem, cap, pzt,
        em_cavity, ndfeb, coil,
    ]
    return inner_mesh_objects


# ── Export ────────────────────────────────────────────────────────────────────

def export_glb(mesh_list, output_path):
    # Deselect all, then select only our mesh objects
    bpy.ops.object.select_all(action="DESELECT")
    for obj in mesh_list:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = mesh_list[0]

    os.makedirs(os.path.dirname(output_path), exist_ok=True)

    bpy.ops.export_scene.gltf(
        filepath=output_path,
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_materials="EXPORT",
        export_normals=True,
        export_tangents=True,
        export_texcoords=True,
        export_draco_mesh_compression_enable=True,
        export_draco_mesh_compression_level=6,
        export_yup=True,
    )
    print(f"[Cube V1] Exported to: {output_path}")


# ── Entry point ───────────────────────────────────────────────────────────────

if __name__ == "__main__":
    args = parse_args()
    clear_scene()
    objects = build_cube_v1()
    export_glb(objects, os.path.abspath(args.output))
    print("[Cube V1] Done.")
