//! # Adornment Renderer
//!
//! Attaches `Mesh3d` + `MeshMaterial3d` + `NotShadowCaster` to entities that
//! carry Roblox-style `*HandleAdornment` marker components. Keeps the
//! actual tool code (Move, Rotate, Scale) free of mesh-asset details —
//! tools only spawn the marker components, and this renderer resolves
//! them to visible geometry.
//!
//! ## Why mesh-based, not Bevy gizmos
//!
//! Bevy's immediate-mode `Gizmos<G>` pipeline has fought our camera stack
//! (main at order 0 → Slint overlay at order 300) repeatedly. Every
//! `depth_bias` / `render_layers` / `Hdr` tweak in the git history either
//! doesn't render or breaks something else. Mesh-based adornments use
//! the same pipeline as the SelectionBox wireframe — which is
//! demonstrably visible through the Slint overlay — so we don't depend
//! on the gizmo render graph at all.
//!
//! ## What this plugin provides
//!
//! - `AdornmentMeshes`: cached primitive meshes (unit cone, cylinder, cube,
//!   sphere). Shared across all handle instances.
//! - `AdornmentMaterials`: cached unlit emissive materials keyed by axis
//!   color (X red, Y green, Z blue, plus hover/drag variants).
//! - Attach-systems that watch `Added<ConeHandleAdornment>` etc. and bundle
//!   `Mesh3d + MeshMaterial3d + NotShadowCaster` onto the entity.
//! - Update-systems that rebuild when `Changed<*HandleAdornment>` fires so
//!   live edits (e.g. shaft length via a property panel) reflect visually.
//!
//! ## Color assignment
//!
//! The adornment's color comes from a sibling `AdornmentAxisColor` component
//! (written by whichever tool spawned the adornment). If absent, the
//! material defaults to white. This keeps the renderer agnostic about
//! tool semantics — Move, Rotate, and Scale each tag their adornments
//! with whatever axis makes sense.

use bevy::prelude::*;
use bevy::light::NotShadowCaster;
use eustress_common::adornments::{
    BoxHandleAdornment, ConeHandleAdornment, CylinderHandleAdornment,
    SphereHandleAdornment,
};

// ============================================================================
// Color tag — tools write this alongside a HandleAdornment to pick color
// ============================================================================

/// Which axis/face color a handle should render with. The tool that spawns
/// the handle attaches this; the renderer reads it to pick a material
/// from `AdornmentMaterials`. Keeping color selection data-driven (rather
/// than embedded in each HandleAdornment type) means the renderer doesn't
/// need to know about Move vs Rotate vs Scale semantics.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdornmentAxisColor {
    X,
    Y,
    Z,
    XYPlane,
    XZPlane,
    YZPlane,
    Center,
    Hover,
    Drag,
}

// ============================================================================
// Cached primitive meshes
// ============================================================================

#[derive(Resource)]
pub struct AdornmentMeshes {
    /// Unit cone, height = 1, base radius = 0.5, tip at +Y.
    pub cone: Handle<Mesh>,
    /// Unit cylinder, height = 1, radius = 0.5, axis along Y.
    pub cylinder: Handle<Mesh>,
    /// Unit cube, 1×1×1 centered at origin.
    pub cube: Handle<Mesh>,
    /// Unit sphere, radius = 0.5.
    pub sphere: Handle<Mesh>,
    /// Unit torus — major_radius = 0.5, minor_radius = 0.05 (thin ring).
    /// Used when `CylinderHandleAdornment.inner_radius > 0` to render
    /// rotation rings. The shaft's `height` field re-purposes as the
    /// world major radius; `radius` becomes the ring's minor radius.
    pub torus_thin: Handle<Mesh>,
}

// ============================================================================
// Cached materials — unlit emissive for always-on-top appearance
// ============================================================================

#[derive(Resource)]
pub struct AdornmentMaterials {
    pub x: Handle<StandardMaterial>,
    pub y: Handle<StandardMaterial>,
    pub z: Handle<StandardMaterial>,
    pub xy: Handle<StandardMaterial>,
    pub xz: Handle<StandardMaterial>,
    pub yz: Handle<StandardMaterial>,
    pub center: Handle<StandardMaterial>,
    pub hover: Handle<StandardMaterial>,
    pub drag: Handle<StandardMaterial>,
    /// Shared ghost-preview material used by every Smart Build Tool
    /// (Gap Fill, Resize Align, Edge Align, Part Swap, Model Reflect,
    /// Part to Terrain). Translucent green-emissive, always-on-top,
    /// alpha pulses via `pulse_ghost_preview_alpha`. See
    /// [TOOLSET_UX.md §3.6](../../../docs/development/TOOLSET_UX.md).
    pub ghost_preview: Handle<StandardMaterial>,
    /// Silhouette outline pass for ghost preview — thin cyan halo so
    /// 0.05m geometry is still legible against any background.
    pub ghost_preview_outline: Handle<StandardMaterial>,
    /// Brief success flash after a commit succeeds — bright green,
    /// fades out in 150ms.
    pub commit_flash: Handle<StandardMaterial>,
}

impl AdornmentMaterials {
    pub fn pick(&self, color: AdornmentAxisColor) -> Handle<StandardMaterial> {
        match color {
            AdornmentAxisColor::X => self.x.clone(),
            AdornmentAxisColor::Y => self.y.clone(),
            AdornmentAxisColor::Z => self.z.clone(),
            AdornmentAxisColor::XYPlane => self.xy.clone(),
            AdornmentAxisColor::XZPlane => self.xz.clone(),
            AdornmentAxisColor::YZPlane => self.yz.clone(),
            AdornmentAxisColor::Center => self.center.clone(),
            AdornmentAxisColor::Hover => self.hover.clone(),
            AdornmentAxisColor::Drag => self.drag.clone(),
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

pub struct AdornmentRendererPlugin;

impl Plugin for AdornmentRendererPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_adornment_assets)
            .add_systems(
                PostUpdate,
                (
                    attach_cone_mesh,
                    attach_cylinder_mesh,
                    attach_box_mesh,
                    attach_sphere_mesh,
                    update_cone_on_change,
                    update_cylinder_on_change,
                    update_box_on_change,
                    update_sphere_on_change,
                    update_material_on_color_change,
                ),
            )
            .add_systems(Update, pulse_ghost_preview_alpha);
    }
}

// ============================================================================
// Startup: build meshes + materials once
// ============================================================================

fn create_adornment_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Primitive meshes — all UNIT-sized so per-adornment Transform::scale
    // can stretch them to whatever `height`/`radius`/`size` is requested.
    let cone = meshes.add(Mesh::from(Cone { radius: 0.5, height: 1.0 }));
    let cylinder = meshes.add(Mesh::from(Cylinder { radius: 0.5, half_height: 0.5 }));
    let cube = meshes.add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)));
    let sphere = meshes.add(Mesh::from(Sphere { radius: 0.5 }));
    // Thin torus in XZ plane. Major radius 0.5 (so unit-circle diameter
    // at scale=1), minor radius 0.02 so the ring is ~4% thick — readable
    // but doesn't crowd the handle. Rotate handles scale this to their
    // desired world radius.
    let torus_thin = meshes.add(Mesh::from(Torus { minor_radius: 0.02, major_radius: 0.5 }));

    commands.insert_resource(AdornmentMeshes { cone, cylinder, cube, sphere, torus_thin });

    // Unlit emissive materials. `emissive * 3.0` gives a bright readable
    // handle even against atmosphere-lit scenes; `unlit = true` bypasses
    // shading so the color stays saturated on every face regardless of
    // light direction.
    let x       = axis_material(&mut materials, Color::srgb(1.00, 0.15, 0.15));
    let y       = axis_material(&mut materials, Color::srgb(0.15, 1.00, 0.15));
    let z       = axis_material(&mut materials, Color::srgb(0.15, 0.35, 1.00));
    let xy      = axis_material(&mut materials, Color::srgb(1.00, 1.00, 0.20));
    let xz      = axis_material(&mut materials, Color::srgb(1.00, 0.20, 1.00));
    let yz      = axis_material(&mut materials, Color::srgb(0.20, 1.00, 1.00));
    let center  = axis_material(&mut materials, Color::srgb(1.00, 1.00, 1.00));
    let hover   = axis_material(&mut materials, Color::srgb(1.00, 1.00, 0.60));
    let drag    = axis_material(&mut materials, Color::srgb(1.00, 0.90, 0.30));

    // Ghost preview: translucent green, pulses alpha, always-on-top.
    // accent-green #3cba54 at 40% base alpha (pulsed by pulse_ghost_preview_alpha).
    let ghost_preview = materials.add(StandardMaterial {
        base_color: Color::srgba(0.235, 0.729, 0.329, 0.40),
        emissive: LinearRgba::from(Color::srgb(0.235, 0.729, 0.329)) * 2.5,
        unlit: true,
        depth_bias: -1000.0,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    // Outline pass: thin cyan silhouette for legibility on any background.
    // accent-cyan #00bcd4.
    let ghost_preview_outline = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.737, 0.831, 0.85),
        emissive: LinearRgba::from(Color::srgb(0.0, 0.737, 0.831)) * 3.0,
        unlit: true,
        depth_bias: -2000.0,  // above the fill so it never gets occluded
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });
    // Commit flash: 150ms bright-green burst. accent-green-bright #00e676.
    let commit_flash = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.902, 0.463, 0.70),
        emissive: LinearRgba::from(Color::srgb(0.0, 0.902, 0.463)) * 4.0,
        unlit: true,
        depth_bias: -1000.0,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..default()
    });

    commands.insert_resource(AdornmentMaterials {
        x, y, z, xy, xz, yz, center, hover, drag,
        ghost_preview, ghost_preview_outline, commit_flash,
    });
}

/// Pulse the ghost-preview material's alpha so it reads as "live preview"
/// rather than static geometry. Frequency 1.5 Hz, alpha oscillates 0.30 ↔ 0.50.
fn pulse_ghost_preview_alpha(
    time: Res<Time>,
    mats_res: Option<Res<AdornmentMaterials>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mats_res) = mats_res else { return };
    let t = time.elapsed_secs();
    let alpha = 0.40 + 0.10 * (t * std::f32::consts::TAU * 1.5).sin();
    if let Some(m) = materials.get_mut(&mats_res.ghost_preview) {
        m.base_color.set_alpha(alpha);
    }
}

fn axis_material(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        emissive: LinearRgba::from(color) * 2.5,
        unlit: true,
        // Always-on-top so tool handles stay visible when the camera gets
        // close to or inside the adornee. `depth_bias` is in Bevy's
        // clip-space depth units — a large negative value pushes the
        // fragment toward the near plane so opaque geometry in front of
        // the handle loses the depth test. `AlphaMode::Blend` routes
        // through the transparent-3d render pass, which doesn't write
        // depth (so handles never occlude each other's siblings or the
        // real scene) and respects the bias.
        depth_bias: -1000.0,
        alpha_mode: AlphaMode::Blend,
        // Don't drop back-faces — with heavy bias the viewing angle can
        // put us "inside" the cone, and we want the user to still see
        // the handle.
        cull_mode: None,
        ..default()
    })
}

// ============================================================================
// Attach systems — run once per entity on `Added<*HandleAdornment>`
// ============================================================================

fn attach_cone_mesh(
    mut commands: Commands,
    assets: Res<AdornmentMeshes>,
    mats: Res<AdornmentMaterials>,
    query: Query<
        (Entity, &ConeHandleAdornment, Option<&AdornmentAxisColor>),
        Added<ConeHandleAdornment>,
    >,
) {
    for (e, cone, color) in &query {
        let mat = mats.pick(color.copied().unwrap_or(AdornmentAxisColor::Center));
        commands.entity(e).insert((
            Mesh3d(assets.cone.clone()),
            MeshMaterial3d(mat),
            NotShadowCaster,
        ));
        apply_cone_transform(&mut commands, e, cone);
    }
}

fn attach_cylinder_mesh(
    mut commands: Commands,
    assets: Res<AdornmentMeshes>,
    mats: Res<AdornmentMaterials>,
    query: Query<
        (Entity, &CylinderHandleAdornment, Option<&AdornmentAxisColor>),
        Added<CylinderHandleAdornment>,
    >,
) {
    for (e, cyl, color) in &query {
        let mat = mats.pick(color.copied().unwrap_or(AdornmentAxisColor::Center));

        // CylinderHandleAdornment is overloaded per the Roblox schema:
        //   - inner_radius == 0  →  solid cylinder (shafts, pillars)
        //   - inner_radius  > 0  →  thin ring / torus (rotation arcs)
        // Angle < 360 would be a pie / arc slice; not yet implemented.
        if cyl.inner_radius > 0.0 {
            commands.entity(e).insert((
                Mesh3d(assets.torus_thin.clone()),
                MeshMaterial3d(mat),
                NotShadowCaster,
            ));
            apply_torus_transform(&mut commands, e, cyl);
        } else {
            commands.entity(e).insert((
                Mesh3d(assets.cylinder.clone()),
                MeshMaterial3d(mat),
                NotShadowCaster,
            ));
            apply_cylinder_transform(&mut commands, e, cyl);
        }
    }
}

fn attach_box_mesh(
    mut commands: Commands,
    assets: Res<AdornmentMeshes>,
    mats: Res<AdornmentMaterials>,
    query: Query<
        (Entity, &BoxHandleAdornment, Option<&AdornmentAxisColor>),
        Added<BoxHandleAdornment>,
    >,
) {
    for (e, bx, color) in &query {
        let mat = mats.pick(color.copied().unwrap_or(AdornmentAxisColor::Center));
        commands.entity(e).insert((
            Mesh3d(assets.cube.clone()),
            MeshMaterial3d(mat),
            NotShadowCaster,
        ));
        apply_box_transform(&mut commands, e, bx);
    }
}

fn attach_sphere_mesh(
    mut commands: Commands,
    assets: Res<AdornmentMeshes>,
    mats: Res<AdornmentMaterials>,
    query: Query<
        (Entity, &SphereHandleAdornment, Option<&AdornmentAxisColor>),
        Added<SphereHandleAdornment>,
    >,
) {
    for (e, sph, color) in &query {
        let mat = mats.pick(color.copied().unwrap_or(AdornmentAxisColor::Center));
        commands.entity(e).insert((
            Mesh3d(assets.sphere.clone()),
            MeshMaterial3d(mat),
            NotShadowCaster,
        ));
        apply_sphere_transform(&mut commands, e, sph);
    }
}

// ============================================================================
// Update systems — rerun when the marker component's props change
// ============================================================================

fn update_cone_on_change(
    mut commands: Commands,
    query: Query<(Entity, &ConeHandleAdornment), Changed<ConeHandleAdornment>>,
) {
    for (e, cone) in &query {
        apply_cone_transform(&mut commands, e, cone);
    }
}

fn update_cylinder_on_change(
    mut commands: Commands,
    query: Query<(Entity, &CylinderHandleAdornment), Changed<CylinderHandleAdornment>>,
) {
    for (e, cyl) in &query {
        if cyl.inner_radius > 0.0 {
            apply_torus_transform(&mut commands, e, cyl);
        } else {
            apply_cylinder_transform(&mut commands, e, cyl);
        }
    }
}

fn update_box_on_change(
    mut commands: Commands,
    query: Query<(Entity, &BoxHandleAdornment), Changed<BoxHandleAdornment>>,
) {
    for (e, bx) in &query {
        apply_box_transform(&mut commands, e, bx);
    }
}

fn update_sphere_on_change(
    mut commands: Commands,
    query: Query<(Entity, &SphereHandleAdornment), Changed<SphereHandleAdornment>>,
) {
    for (e, sph) in &query {
        apply_sphere_transform(&mut commands, e, sph);
    }
}

fn update_material_on_color_change(
    mut commands: Commands,
    mats: Res<AdornmentMaterials>,
    query: Query<(Entity, &AdornmentAxisColor), Changed<AdornmentAxisColor>>,
) {
    for (e, color) in &query {
        commands.entity(e).insert(MeshMaterial3d(mats.pick(*color)));
    }
}

// ============================================================================
// Transform helpers — scale the unit-sized primitive mesh to the size the
// adornment marker requests. The marker's props are in world-space units
// (studs); the primitive meshes are unit cubes/cones/cylinders, so
// scale = requested-size directly.
//
// NOTE: local Transform.translation / rotation on the entity are NOT
// overwritten here — the tool (Move handles, Arc handles, etc.) sets those
// to position each handle along its axis. We only set scale.
// ============================================================================

fn apply_cone_transform(
    commands: &mut Commands,
    entity: Entity,
    cone: &ConeHandleAdornment,
) {
    // Cone primitive: height = 1 along +Y, base radius = 0.5.
    // Requested: height in studs along local +Y, radius in studs.
    // Scale XZ by `radius * 2` (since primitive radius = 0.5), Y by `height`.
    let scale = Vec3::new(cone.radius * 2.0, cone.height, cone.radius * 2.0);
    apply_scale_preserving_pos_rot(commands, entity, scale);
}

fn apply_cylinder_transform(
    commands: &mut Commands,
    entity: Entity,
    cyl: &CylinderHandleAdornment,
) {
    // Cylinder primitive: height = 1 along Y, radius = 0.5.
    let scale = Vec3::new(cyl.radius * 2.0, cyl.height, cyl.radius * 2.0);
    apply_scale_preserving_pos_rot(commands, entity, scale);
}

fn apply_box_transform(
    commands: &mut Commands,
    entity: Entity,
    bx: &BoxHandleAdornment,
) {
    // Cube primitive: 1×1×1.
    apply_scale_preserving_pos_rot(commands, entity, bx.size);
}

fn apply_sphere_transform(
    commands: &mut Commands,
    entity: Entity,
    sph: &SphereHandleAdornment,
) {
    // Sphere primitive: radius = 0.5.
    let s = sph.radius * 2.0;
    apply_scale_preserving_pos_rot(commands, entity, Vec3::splat(s));
}

/// Torus variant of a `CylinderHandleAdornment` (inner_radius > 0).
/// Reinterprets the shaft's props:
///   - `height`  → torus major diameter (ring spacing). Scale X/Z accordingly.
///   - `radius`  → torus minor radius (tube thickness). Our unit torus has
///                  minor_radius = 0.02 so we scale all axes by the same
///                  factor to enlarge the whole ring uniformly.
/// The torus lies in the XZ plane in local space; the tool is responsible
/// for rotating the entity so the ring faces the correct axis.
fn apply_torus_transform(
    commands: &mut Commands,
    entity: Entity,
    cyl: &CylinderHandleAdornment,
) {
    // Unit torus: major_radius = 0.5. Scale so the final major radius
    // equals `height / 2` in world units (since `height` is reused as
    // the ring's diameter-ish metric).
    let major_scale = cyl.height.max(0.001);
    apply_scale_preserving_pos_rot(commands, entity, Vec3::splat(major_scale));
}

/// Preserve translation + rotation set by whoever spawned the entity;
/// only overwrite scale. Routed through `commands.queue` because we can't
/// take a second `&mut Transform` query param alongside the change-filter
/// query in the attach/update systems without risk of borrow conflicts.
fn apply_scale_preserving_pos_rot(
    commands: &mut Commands,
    entity: Entity,
    scale: Vec3,
) {
    commands.queue(move |world: &mut World| {
        if let Some(mut t) = world.get_mut::<Transform>(entity) {
            t.scale = scale;
        }
        // If the entity has no Transform (shouldn't happen — the tool
        // always spawns one), skip silently. Adding a bare Transform here
        // would lose whatever translation/rotation the tool set.
    });
}
