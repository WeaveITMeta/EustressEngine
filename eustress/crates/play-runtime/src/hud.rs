//! # The HUD: a player's ScreenGuis on the screen
//!
//! What draws: the ScreenGuis under `Players.LocalPlayer.PlayerGui` that are
//! enabled, with no hidden ancestor. StarterGui's originals never draw in Play,
//! and neither does another player's PlayerGui, which a host holds for each
//! joined player (`docs/architecture/SHARED_PLAY_RUNTIME.md`, rules 5 and 6).
//!
//! How it draws: each element directly under a drawn ScreenGui is a panel. A
//! panel's subtree is laid out against the 3D viewport's logical size (a
//! UDim2 is scale times the parent's extent plus offset, then `AnchorPoint`;
//! an unset UDim2 falls back to the legacy pixel fields, as Studio's overlay
//! laid it out), then painted by the billboard rasterizer into the panel's own
//! texture at the viewport's pixel density. A panel repaints only when its
//! content hash changes, so an ammo counter repaints its own panel and nothing
//! else. The panels are unlit quads seen by a dedicated orthographic overlay
//! camera with no tonemapping, which composites over the play camera's output
//! after post-processing, so a HUD's colours are the colours its script set.
//!
//! Panels stack by their ScreenGui's `DisplayOrder`, then their `ZIndex`, then
//! tree order; inside a panel, elements stack by `ZIndex`, then tree order.
//! [`HudLayout`] keeps every laid-out rect in draw order for the hit test
//! ([`crate::hud_input`]), so a click lands on what was drawn.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig, ScalingMode, Viewport};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{BlendState, Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::Msaa;
use bevy::window::PrimaryWindow;
use tiny_skia::Pixmap;

use eustress_common::datamodel::{DataModel, DmValue};
use eustress_common::gui::billboard_renderer::GuiElementDisplay;
use eustress_common::play_session::{PlayDataModel, ViewportBounds};

use crate::apply::PlayAssetRoot;
use crate::billboards::{label_hash, render_element, BillboardTextState, FlatElem};
use crate::gui_images::GuiImages;

/// The render layer the HUD panels and their camera share.
pub const HUD_LAYER: usize = 29;
/// The overlay camera's order: above every play camera, below Studio's Slint
/// editor overlay (order 300).
pub const HUD_CAMERA_ORDER: isize = 100;
/// The largest panel texture side, physical pixels.
const MAX_PANEL_PX: u32 = 4096;

/// The HUD's overlay camera.
#[derive(Component, Debug)]
pub struct HudCamera;

/// A panel's quad.
#[derive(Component, Debug)]
pub struct HudPanelQuad;

/// One laid-out element, in viewport-local logical pixels.
#[derive(Debug, Clone)]
pub struct HudItem {
    pub entity: Entity,
    pub rect: Rect,
    /// The rect its clipping ancestors leave visible, if any clips it.
    pub clip: Option<Rect>,
    pub class_type: String,
    pub mouse_filter: String,
}

/// What the HUD drew this frame, for the hit test: every visible element in
/// draw order (the last one is on top), and the viewport it was laid out in.
#[derive(Resource, Debug, Default, Clone)]
pub struct HudLayout {
    /// The viewport's size, logical pixels.
    pub viewport: Vec2,
    /// The viewport's top-left in the window, logical pixels.
    pub origin: Vec2,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    pub items: Vec<HudItem>,
}

/// A panel's texture and quad, and the content it last painted.
struct Panel {
    quad: Entity,
    image: Handle<Image>,
    material: Handle<StandardMaterial>,
    hash: u64,
    px: UVec2,
}

#[derive(Default)]
struct HudPanels {
    by_root: HashMap<Entity, Panel>,
    quad_mesh: Option<Handle<Mesh>>,
}

/// Draws the local player's ScreenGuis. Needs the billboard drawing plugin
/// (its text state and image cache), which both apps add.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudLayout>().add_systems(
            PostUpdate,
            (sync_hud_camera, paint_hud.run_if(resource_exists::<PlayDataModel>))
                .chain()
                .before(TransformSystems::Propagate),
        );
    }
}

/// The 3D viewport: its top-left in the window and its size, logical pixels,
/// and the window's scale factor. Studio's viewport is the rect its layout
/// reports; a shell without one uses the whole window.
fn viewport(window: &Window, bounds: Option<&ViewportBounds>) -> (Vec2, Vec2, f32) {
    let scale = window.scale_factor().max(0.01);
    match bounds {
        Some(b) if b.width > 1.0 && b.height > 1.0 => (
            Vec2::new(b.x, b.y) / scale,
            Vec2::new(b.width, b.height) / scale,
            scale,
        ),
        _ => (Vec2::ZERO, Vec2::new(window.width(), window.height()), scale),
    }
}

/// One overlay camera while a session plays, covering the 3D viewport.
fn sync_hud_camera(
    mut commands: Commands,
    tree: Option<Res<PlayDataModel>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    bounds: Option<Res<ViewportBounds>>,
    mut cameras: Query<(Entity, &mut Camera, &mut Projection), With<HudCamera>>,
) {
    if tree.is_none() {
        for (entity, ..) in cameras.iter() {
            commands.entity(entity).try_despawn();
        }
        return;
    }
    let Ok(window) = windows.single() else { return };
    let (_, size, _) = viewport(window, bounds.as_deref());
    let physical = bounds
        .as_deref()
        .filter(|b| b.width > 1.0 && b.height > 1.0)
        .map(|b| Viewport {
            physical_position: UVec2::new(b.x.max(0.0) as u32, b.y.max(0.0) as u32),
            physical_size: UVec2::new(b.width as u32, b.height as u32),
            ..default()
        });
    let projection = || {
        Projection::from(OrthographicProjection {
            near: -1000.0,
            far: 1000.0,
            scaling_mode: ScalingMode::Fixed { width: size.x.max(1.0), height: size.y.max(1.0) },
            ..OrthographicProjection::default_3d()
        })
    };
    let Some((_, mut camera, mut current)) = cameras.iter_mut().next() else {
        commands.spawn((
            Camera3d::default(),
            projection(),
            Camera {
                order: HUD_CAMERA_ORDER,
                viewport: physical,
                // Its own target clears to transparent each frame; the window
                // keeps the play camera's image (an output clear applies only
                // to the first camera writing a target) and the HUD blends
                // over it, premultiplied like the panels' pixels.
                clear_color: ClearColorConfig::Custom(Color::NONE),
                output_mode: CameraOutputMode::Write {
                    blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                },
                ..default()
            },
            // The HUD's colours are the script's colours: no tonemapping, and
            // nothing to anti-alias (the panels are already rasterized).
            Tonemapping::None,
            Msaa::Off,
            RenderLayers::layer(HUD_LAYER),
            // No sky or atmosphere on an overlay.
            eustress_common::plugins::lighting_plugin::NoAtmosphere,
            HudCamera,
            Name::new("HUD Camera"),
        ));
        return;
    };
    // `Viewport` has no `PartialEq`; compare the rect.
    let rect = |v: &Option<Viewport>| v.as_ref().map(|v| (v.physical_position, v.physical_size));
    if rect(&camera.viewport) != rect(&physical) {
        camera.viewport = physical;
    }
    let want = ScalingMode::Fixed { width: size.x.max(1.0), height: size.y.max(1.0) };
    let stale = match &*current {
        Projection::Orthographic(o) => !matches!(
            (o.scaling_mode, want),
            (ScalingMode::Fixed { width: a, height: b }, ScalingMode::Fixed { width: c, height: d })
                if (a - c).abs() < 0.01 && (b - d).abs() < 0.01
        ),
        _ => true,
    };
    if stale {
        *current = projection();
    }
}

/// The ScreenGuis this machine draws, with their `DisplayOrder`, in tree
/// order: under `Players.LocalPlayer.PlayerGui`, nothing else.
fn drawn_screen_guis(g: &DataModel) -> Vec<(Entity, i32)> {
    let Some(player) = g.local_player.filter(|p| g.exists(*p)) else { return Vec::new() };
    let Some(player_gui) = g.find_first_child(player, "PlayerGui", false) else { return Vec::new() };
    g.descendants(player_gui)
        .into_iter()
        .filter(|&d| g.class_of(d) == Some("ScreenGui"))
        .filter_map(|d| {
            let entity = Entity::from_bits(g.entity_of(d)?);
            let order = match g.get_prop(d, "DisplayOrder") {
                Some(DmValue::Number(n)) => n as i32,
                _ => 0,
            };
            Some((entity, order))
        })
        .collect()
}

/// An element's rect under a parent rect, as Studio's overlay resolved it:
/// `scale * parent extent + offset` per axis, the legacy pixel fields when a
/// UDim2 is entirely unset, then `AnchorPoint` by the element's own size.
fn resolve_rect(d: &GuiElementDisplay, parent: Rect) -> Rect {
    let unset = |s: f32, o: f32| s == 0.0 && o == 0.0;
    let (pw, ph) = (parent.width(), parent.height());
    let w = if unset(d.size_udim2[0], d.size_udim2[1]) {
        d.width.max(1.0)
    } else {
        (d.size_udim2[0] * pw + d.size_udim2[1]).max(1.0)
    };
    let h = if unset(d.size_udim2[2], d.size_udim2[3]) {
        d.height.max(1.0)
    } else {
        (d.size_udim2[2] * ph + d.size_udim2[3]).max(1.0)
    };
    let x = if unset(d.position_udim2[0], d.position_udim2[1]) {
        d.x
    } else {
        d.position_udim2[0] * pw + d.position_udim2[1]
    };
    let y = if unset(d.position_udim2[2], d.position_udim2[3]) {
        d.y
    } else {
        d.position_udim2[2] * ph + d.position_udim2[3]
    };
    let min = parent.min + Vec2::new(x - d.anchor_point[0] * w, y - d.anchor_point[1] * h);
    Rect::from_corners(min, min + Vec2::new(w, h))
}

/// Lay out a subtree depth first into `out`. A hidden element hides its
/// descendants; a clipping element clips them to the part of its rect its own
/// clipping ancestors leave.
fn flatten(
    entity: Entity,
    parent: Rect,
    clip: Option<Rect>,
    gui: &Query<(&GuiElementDisplay, Option<&Children>)>,
    out: &mut Vec<(Entity, FlatElem)>,
) {
    let Ok((d, children)) = gui.get(entity) else { return };
    if !d.visible {
        return;
    }
    let rect = resolve_rect(d, parent);
    let mut elem = d.clone();
    elem.x = rect.min.x - parent.min.x;
    elem.y = rect.min.y - parent.min.y;
    elem.width = rect.width();
    elem.height = rect.height();
    out.push((
        entity,
        FlatElem {
            elem,
            abs_x: rect.min.x,
            abs_y: rect.min.y,
            clip_rect: clip.map(|c| [c.min.x, c.min.y, c.width(), c.height()]),
        },
    ));
    let inner = if d.clip_children { Some(clip.map_or(rect, |c| c.intersect(rect))) } else { clip };
    if let Some(children) = children {
        for child in children.iter() {
            flatten(child, rect, inner, gui, out);
        }
    }
}

/// Lays out the drawn ScreenGuis, repaints the panels whose content changed,
/// places every panel's quad, and records the layout for the hit test.
#[allow(clippy::too_many_arguments)]
fn paint_hud(
    mut commands: Commands,
    tree: Res<PlayDataModel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    bounds: Option<Res<ViewportBounds>>,
    gui: Query<(&GuiElementDisplay, Option<&Children>)>,
    mut text_state: NonSendMut<BillboardTextState>,
    mut gui_images: ResMut<GuiImages>,
    root: Option<Res<PlayAssetRoot>>,
    mut assets: (ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Mesh>>),
    mut quads: Query<&mut Transform, With<HudPanelQuad>>,
    mut panels: Local<HudPanels>,
    mut layout: ResMut<HudLayout>,
) {
    let (images, materials, meshes) = (&mut assets.0, &mut assets.1, &mut assets.2);
    let Ok(window) = windows.single() else { return };
    let (origin, size, scale) = viewport(window, bounds.as_deref());
    let screen = Rect::from_corners(Vec2::ZERO, size);
    let root_path = root.as_deref().map(|r| r.0.clone()).unwrap_or_default();

    let guis = drawn_screen_guis(&tree.dm.lock());

    // Panels in stacking order: DisplayOrder, then the root's ZIndex, then
    // tree order.
    let mut roots: Vec<(i32, i32, usize, Entity)> = Vec::new();
    for (gui_entity, order) in &guis {
        let Ok((screen_gui, children)) = gui.get(*gui_entity) else { continue };
        if !screen_gui.visible {
            continue; // `Enabled = false`
        }
        for child in children.into_iter().flat_map(|c| c.iter()) {
            if let Ok((d, _)) = gui.get(child) {
                roots.push((*order, d.z_order, roots.len(), child));
            }
        }
    }
    roots.sort_by_key(|&(order, z, seq, _)| (order, z, seq));

    let quad_mesh = panels.quad_mesh.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
    let mut items = Vec::new();
    let mut live = HashSet::new();
    for (rank, &(_, _, _, root_entity)) in roots.iter().enumerate() {
        let mut flat = Vec::new();
        flatten(root_entity, screen, None, &gui, &mut flat);
        // Stable: equal ZIndex keeps tree order, so a parent paints under its
        // children.
        flat.sort_by_key(|(_, f)| f.elem.z_order);
        let Some(bounds_rect) = panel_bounds(&flat, screen, scale) else {
            // Nothing visible on screen: keep the panel's texture, hide its quad.
            if let Some(panel) = panels.by_root.get(&root_entity) {
                if let Ok(mut tf) = quads.get_mut(panel.quad) {
                    tf.scale = Vec3::ZERO;
                }
                live.insert(root_entity);
            }
            continue;
        };
        for (entity, f) in &flat {
            let rect = Rect::from_corners(
                Vec2::new(f.abs_x, f.abs_y),
                Vec2::new(f.abs_x + f.elem.width, f.abs_y + f.elem.height),
            );
            let clip = f.clip_rect.map(|c| Rect::new(c[0], c[1], c[0] + c[2], c[1] + c[3]));
            items.push(HudItem {
                entity: *entity,
                rect,
                clip,
                class_type: f.elem.class_type.clone(),
                mouse_filter: f.elem.mouse_filter.clone(),
            });
        }

        let px = UVec2::new(
            ((bounds_rect.width() * scale).round() as u32).clamp(1, MAX_PANEL_PX),
            ((bounds_rect.height() * scale).round() as u32).clamp(1, MAX_PANEL_PX),
        );
        let local: Vec<FlatElem> = flat
            .iter()
            .map(|(_, f)| FlatElem {
                elem: f.elem.clone(),
                abs_x: f.abs_x - bounds_rect.min.x,
                abs_y: f.abs_y - bounds_rect.min.y,
                clip_rect: f.clip_rect.map(|c| [c[0] - bounds_rect.min.x, c[1] - bounds_rect.min.y, c[2], c[3]]),
            })
            .collect();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            label_hash(&local).hash(&mut h);
            px.hash(&mut h);
            scale.to_bits().hash(&mut h);
            h.finish()
        };

        if !panels.by_root.contains_key(&root_entity) {
            let image = images.add(blank_image(px));
            let material = materials.add(StandardMaterial {
                base_color_texture: Some(image.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Premultiplied,
                fog_enabled: false,
                cull_mode: None,
                ..default()
            });
            let quad = commands
                .spawn((
                    Mesh3d(quad_mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_scale(Vec3::ZERO),
                    RenderLayers::layer(HUD_LAYER),
                    HudPanelQuad,
                    Name::new("HUD Panel"),
                ))
                .id();
            panels.by_root.insert(root_entity, Panel { quad, image, material, hash: 0, px });
        }
        let Some(panel) = panels.by_root.get_mut(&root_entity) else { continue };
        live.insert(root_entity);

        if panel.hash != hash {
            if let Some(mut pixmap) = Pixmap::new(px.x, px.y) {
                for f in &local {
                    render_element(
                        &mut pixmap,
                        &f.elem,
                        f.abs_x,
                        f.abs_y,
                        f.clip_rect,
                        scale,
                        &mut text_state,
                        &mut gui_images,
                        &root_path,
                    );
                }
                if panel.px == px {
                    if let Some(mut image) = images.get_mut(&panel.image) {
                        image.data = Some(pixmap.take());
                    }
                } else {
                    let mut image = blank_image(px);
                    image.data = Some(pixmap.take());
                    panel.image = images.add(image);
                    panel.px = px;
                    if let Some(mut material) = materials.get_mut(&panel.material) {
                        material.base_color_texture = Some(panel.image.clone());
                    }
                }
                panel.hash = hash;
            }
        }

        // The ortho camera looks down -Z with the viewport centred on the
        // origin, +Y up; later panels sit nearer, so they draw on top.
        let centre = bounds_rect.center();
        let at = Vec3::new(centre.x - size.x * 0.5, size.y * 0.5 - centre.y, rank as f32 * 0.01);
        let want = Transform::from_translation(at).with_scale(Vec3::new(bounds_rect.width(), bounds_rect.height(), 1.0));
        if let Ok(mut tf) = quads.get_mut(panel.quad) {
            if *tf != want {
                *tf = want;
            }
        }
    }

    // Panels whose root no longer draws.
    panels.by_root.retain(|root_entity, panel| {
        if live.contains(root_entity) {
            return true;
        }
        commands.entity(panel.quad).try_despawn();
        images.remove(&panel.image);
        materials.remove(&panel.material);
        false
    });

    *layout = HudLayout { viewport: size, origin, scale, items };
}

/// The part of the screen a panel's elements cover, snapped outward to whole
/// physical pixels so the texture maps one texel to one pixel; `None` when
/// nothing of it is on screen.
fn panel_bounds(flat: &[(Entity, FlatElem)], screen: Rect, scale: f32) -> Option<Rect> {
    let mut covered: Option<Rect> = None;
    for (_, f) in flat {
        let mut r = Rect::from_corners(
            Vec2::new(f.abs_x, f.abs_y),
            Vec2::new(f.abs_x + f.elem.width, f.abs_y + f.elem.height),
        );
        if let Some(c) = f.clip_rect {
            r = r.intersect(Rect::new(c[0], c[1], c[0] + c[2], c[1] + c[3]));
        }
        // A text stroke paints a pixel past the rect.
        r = r.inflate(1.0);
        covered = Some(covered.map_or(r, |c| c.union(r)));
    }
    let r = covered?.intersect(screen);
    if r.is_empty() {
        return None;
    }
    let snap = |v: f32, up: bool| if up { (v * scale).ceil() / scale } else { (v * scale).floor() / scale };
    Some(Rect::from_corners(Vec2::new(snap(r.min.x, false), snap(r.min.y, false)), Vec2::new(snap(r.max.x, true), snap(r.max.y, true))))
}

/// A transparent panel texture, sampled texel for texel.
fn blank_image(px: UVec2) -> Image {
    let mut image = Image::new(
        Extent3d { width: px.x, height: px.y, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; (px.x * px.y * 4) as usize],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(pos: [f32; 4], size: [f32; 4], anchor: [f32; 2]) -> GuiElementDisplay {
        GuiElementDisplay {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            position_udim2: pos,
            size_udim2: size,
            anchor_point: anchor,
            z_order: 0,
            visible: true,
            clip_children: false,
            scroll_x: 0.0,
            scroll_y: 0.0,
            bg_color: [0.0; 4],
            border_size: 0.0,
            border_color: [0.0; 4],
            corner_radius: 0.0,
            text: String::new(),
            text_color: [0.0; 4],
            font: String::new(),
            font_size: 14.0,
            font_weight: 400,
            text_align: String::new(),
            text_y_align: String::new(),
            text_stroke_color: [0.0; 4],
            text_scaled: false,
            image_path: String::new(),
            class_type: "Frame".into(),
            mouse_filter: "stop".into(),
        }
    }

    #[test]
    fn a_udim2_resolves_against_its_parent_and_its_anchor() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 500.0);
        // Centred, half the width, 40 px tall.
        let r = resolve_rect(&display([0.5, 0.0, 0.5, 0.0], [0.5, 0.0, 0.0, 40.0], [0.5, 0.5]), screen);
        assert_eq!(r, Rect::new(250.0, 230.0, 750.0, 270.0));
        // Offset only, top-left anchored, inside a parent that is not at the origin.
        let parent = Rect::new(100.0, 50.0, 300.0, 150.0);
        let r = resolve_rect(&display([0.0, 10.0, 1.0, -20.0], [0.0, 30.0, 0.0, 20.0], [0.0, 0.0]), parent);
        assert_eq!(r, Rect::new(110.0, 130.0, 140.0, 150.0));
    }

    #[test]
    fn an_unset_udim2_uses_the_legacy_pixel_fields() {
        let mut d = display([0.0; 4], [0.0; 4], [0.0, 0.0]);
        d.x = 12.0;
        d.y = 8.0;
        d.width = 64.0;
        d.height = 32.0;
        let r = resolve_rect(&d, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(r, Rect::new(12.0, 8.0, 76.0, 40.0));
    }

    #[test]
    fn a_panel_is_snapped_to_whole_pixels_and_clipped_to_the_screen() {
        let screen = Rect::new(0.0, 0.0, 100.0, 100.0);
        let elem = |x: f32, w: f32| FlatElem {
            elem: GuiElementDisplay { width: w, height: 10.0, ..display([0.0; 4], [0.0; 4], [0.0, 0.0]) },
            abs_x: x,
            abs_y: 5.25,
            clip_rect: None,
        };
        let flat = vec![(Entity::PLACEHOLDER, elem(10.3, 20.0)), (Entity::PLACEHOLDER, elem(90.0, 50.0))];
        let r = panel_bounds(&flat, screen, 2.0).unwrap();
        assert_eq!(r.min, Vec2::new(9.0, 4.0), "one pixel of stroke room, floored to half pixels");
        assert_eq!(r.max.x, 100.0, "clipped to the screen");
        let off = vec![(Entity::PLACEHOLDER, elem(500.0, 10.0))];
        assert!(panel_bounds(&off, screen, 1.0).is_none());
    }
}
