//! # The Player's pause menu, on the screen
//!
//! The Esc menu (Resume, Settings, Quit) opened and drew nothing: its drawing
//! was a stub. This module paints the menu card the way
//! [`crate::screen_notice`] paints a notice: the billboard rasterizer's boxes
//! and text, on an overlay camera above the HUD and the notice. What the rows
//! are, and what choosing one does, belong to the Player (`client` pause menu);
//! this module lays the card out, paints it, and says which row a point is on.

use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, ClearColorConfig, ScalingMode};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{BlendState, Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::Msaa;
use bevy::window::PrimaryWindow;
use tiny_skia::Pixmap;

use eustress_common::gui::billboard_renderer::GuiElementDisplay;

use crate::billboards::{render_element, BillboardTextState, FlatElem};
use crate::gui_images::GuiImages;

/// The render layer the menu and its camera share.
pub const MENU_LAYER: usize = 27;
/// The menu camera's order: above the HUD (100) and the notice (200), below
/// Studio's Slint editor overlay (300).
pub const MENU_CAMERA_ORDER: isize = 210;
/// The largest menu texture side, physical pixels.
const MAX_MENU_PX: u32 = 4096;

const CARD_WIDTH: f32 = 420.0;
const MARGIN: f32 = 16.0;
const PAD: f32 = 24.0;
const TITLE_H: f32 = 40.0;
const ROW_H: f32 = 44.0;
const ROW_GAP: f32 = 8.0;
const GAP: f32 = 12.0;
const FOOTER_H: f32 = 26.0;

/// One menu card: a title, its rows top to bottom, the highlighted row, and a
/// line of instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuCard {
    pub title: String,
    pub rows: Vec<String>,
    /// The highlighted row's index.
    pub selected: usize,
    pub footer: String,
}

/// The pause menu: Resume, Settings, Quit, in that order.
pub fn pause_card(selected: usize) -> MenuCard {
    MenuCard {
        title: "Paused".into(),
        rows: vec!["Resume".into(), "Settings".into(), "Quit".into()],
        selected,
        footer: "Up and Down to choose, Enter to select, Esc to resume".into(),
    }
}

/// The settings screen: the mouse sensitivity (changed with Left and Right),
/// then Back.
pub fn settings_card(mouse_sensitivity: f32, selected: usize) -> MenuCard {
    MenuCard {
        title: "Settings".into(),
        rows: vec![format!("Mouse sensitivity: {mouse_sensitivity:.2}"), "Back".into()],
        selected,
        footer: "Left and Right to change. Enter on Back, or Esc, returns.".into(),
    }
}

/// The row `delta` steps from `selected`, wrapping at either end. A card with
/// no rows has no row to choose, so the answer is 0.
pub fn step(selected: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    (selected as i64 + delta as i64).rem_euclid(len as i64) as usize
}

/// The card's box, its title and footer, and each row's box, in window
/// logical px.
struct Boxes {
    card: Rect,
    title: Rect,
    rows: Vec<Rect>,
    footer: Rect,
}

fn boxes(card: &MenuCard, size: Vec2) -> Boxes {
    let rows_n = card.rows.len() as f32;
    let width = CARD_WIDTH.min(size.x - 2.0 * MARGIN).max(160.0);
    let rows_h = rows_n * ROW_H + (rows_n - 1.0).max(0.0) * ROW_GAP;
    let height = PAD + TITLE_H + GAP + rows_h + GAP + FOOTER_H + PAD;
    let min = Vec2::new(((size.x - width) * 0.5).max(0.0), ((size.y - height) * 0.5).max(0.0));
    let card_rect = Rect::from_corners(min, min + Vec2::new(width, height));
    let inner = width - 2.0 * PAD;
    let at = |top: f32, h: f32| {
        Rect::from_corners(
            card_rect.min + Vec2::new(PAD, top),
            card_rect.min + Vec2::new(PAD + inner, top + h),
        )
    };
    let rows = (0..card.rows.len())
        .map(|i| at(PAD + TITLE_H + GAP + i as f32 * (ROW_H + ROW_GAP), ROW_H))
        .collect();
    let footer_top = PAD + TITLE_H + GAP + rows_h + GAP;
    Boxes { card: card_rect, title: at(PAD, TITLE_H), rows, footer: at(footer_top, FOOTER_H) }
}

/// Whether `point` lies in `rect` (its edges included).
fn within(rect: &Rect, point: Vec2) -> bool {
    point.x >= rect.min.x && point.x <= rect.max.x && point.y >= rect.min.y && point.y <= rect.max.y
}

/// The row at `point` (window logical px), if the card shows a row there.
pub fn row_at(card: &MenuCard, size: Vec2, point: Vec2) -> Option<usize> {
    boxes(card, size).rows.iter().position(|row| within(row, point))
}

/// One element of the card, as the billboard rasterizer draws it: a box or a
/// line of text, at a window-logical position.
fn element(
    rect: Rect,
    bg: [f32; 4],
    text: &str,
    size: f32,
    weight: i32,
    color: [f32; 4],
    align: &str,
) -> GuiElementDisplay {
    GuiElementDisplay {
        x: rect.min.x,
        y: rect.min.y,
        width: rect.width(),
        height: rect.height(),
        position_udim2: [0.0; 4],
        size_udim2: [0.0; 4],
        anchor_point: [0.0; 2],
        z_order: 0,
        visible: true,
        clip_children: false,
        scroll_x: 0.0,
        scroll_y: 0.0,
        bg_color: bg,
        border_size: 0.0,
        border_color: [0.0; 4],
        corner_radius: 0.0,
        text: text.to_string(),
        text_color: color,
        font: String::new(),
        font_size: size,
        font_weight: weight,
        text_align: align.into(),
        text_y_align: "Center".into(),
        text_stroke_color: [0.0; 4],
        text_scaled: false,
        image_path: String::new(),
        class_type: if text.is_empty() { "Frame" } else { "TextLabel" }.into(),
        mouse_filter: "ignore".into(),
    }
}

/// Every element of the card in paint order: the dimmed screen, the card, its
/// title, each row's box and label (the selected row highlighted), and the
/// footer.
fn elements(card: &MenuCard, size: Vec2) -> Vec<FlatElem> {
    let b = boxes(card, size);
    let flat = |e: GuiElementDisplay, clip: Option<[f32; 4]>| FlatElem { abs_x: e.x, abs_y: e.y, elem: e, clip_rect: clip };
    let clip_to = |r: Rect| Some([r.min.x, r.min.y, r.width(), r.height()]);
    let white = [1.0, 1.0, 1.0, 1.0];

    let mut card_box = element(b.card, [0.11, 0.12, 0.14, 0.96], "", 0.0, 400, [0.0; 4], "Center");
    card_box.corner_radius = 12.0;
    card_box.border_size = 1.0;
    card_box.border_color = [1.0, 1.0, 1.0, 0.12];

    let mut out = vec![
        flat(element(Rect::from_corners(Vec2::ZERO, size), [0.0, 0.0, 0.0, 0.6], "", 0.0, 400, [0.0; 4], "Center"), None),
        flat(card_box, None),
        flat(element(b.title, [0.0; 4], &card.title, 26.0, 700, white, "Center"), clip_to(b.card)),
    ];
    for (i, (row, label)) in b.rows.iter().zip(&card.rows).enumerate() {
        let selected = i == card.selected;
        let bg = if selected { [0.22, 0.42, 0.78, 0.95] } else { [0.18, 0.19, 0.22, 0.95] };
        let mut row_box = element(*row, bg, "", 0.0, 400, [0.0; 4], "Center");
        row_box.corner_radius = 8.0;
        out.push(flat(row_box, clip_to(b.card)));
        let weight = if selected { 700 } else { 400 };
        out.push(flat(element(*row, [0.0; 4], label, 20.0, weight, white, "Center"), clip_to(*row)));
    }
    out.push(flat(
        element(b.footer, [0.0; 4], &card.footer, 14.0, 400, [0.7, 0.75, 0.82, 1.0], "Center"),
        clip_to(b.card),
    ));
    out
}

/// What the menu shows: `Some(card)` while the menu is open.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct ScreenMenu(pub Option<MenuCard>);

/// The menu's camera.
#[derive(Component, Debug)]
pub struct MenuCamera;

/// The menu's quad.
#[derive(Component, Debug)]
pub struct MenuQuad;

/// Draws [`ScreenMenu`]. Needs the billboard drawing plugin (its text state and
/// image cache), which `PlayRuntimePlugin` adds beside it.
pub struct ScreenMenuPlugin;

impl Plugin for ScreenMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenMenu>()
            .add_systems(PostUpdate, show_screen_menu.run_if(crate::draw_assets_ready).before(TransformSystems::Propagate));
    }
}

/// The menu's camera, quad and texture, and what it last painted.
#[derive(Default)]
struct Shown {
    camera: Option<Entity>,
    quad: Option<Entity>,
    image: Option<Handle<Image>>,
    material: Option<Handle<StandardMaterial>>,
    mesh: Option<Handle<Mesh>>,
    painted: Option<(MenuCard, UVec2, u32)>,
}

/// Shows [`ScreenMenu`] while it is set: paints the card whenever the card or
/// the window changes, and takes everything away once it is cleared.
#[allow(clippy::too_many_arguments)]
fn show_screen_menu(
    mut commands: Commands,
    menu: Res<ScreenMenu>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut text_state: NonSendMut<BillboardTextState>,
    mut gui_images: ResMut<GuiImages>,
    mut assets: (ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Mesh>>),
    mut cameras: Query<&mut Projection, With<MenuCamera>>,
    mut quads: Query<&mut Transform, With<MenuQuad>>,
    mut shown: Local<Shown>,
) {
    let (images, materials, meshes) = (&mut assets.0, &mut assets.1, &mut assets.2);
    let Some(card) = menu.0.clone() else {
        for entity in [shown.camera.take(), shown.quad.take()].into_iter().flatten() {
            commands.entity(entity).try_despawn();
        }
        if let Some(image) = shown.image.take() {
            images.remove(&image);
        }
        if let Some(material) = shown.material.take() {
            materials.remove(&material);
        }
        shown.painted = None;
        return;
    };
    let Ok(window) = windows.single() else { return };
    let size = Vec2::new(window.width(), window.height()).max(Vec2::ONE);
    let scale = window.scale_factor().max(0.01);
    let px = UVec2::new(
        ((size.x * scale).round() as u32).clamp(1, MAX_MENU_PX),
        ((size.y * scale).round() as u32).clamp(1, MAX_MENU_PX),
    );
    let projection = Projection::from(OrthographicProjection {
        near: -1000.0,
        far: 1000.0,
        scaling_mode: ScalingMode::Fixed { width: size.x, height: size.y },
        ..OrthographicProjection::default_3d()
    });

    match shown.camera.and_then(|c| cameras.get_mut(c).ok()) {
        Some(mut current) => {
            let stale = !matches!(
                &*current,
                Projection::Orthographic(o) if matches!(o.scaling_mode,
                    ScalingMode::Fixed { width, height } if (width - size.x).abs() < 0.01 && (height - size.y).abs() < 0.01)
            );
            if stale {
                *current = projection;
            }
        }
        None => {
            let camera = commands
                .spawn((
                    Camera3d::default(),
                    projection,
                    Camera {
                        order: MENU_CAMERA_ORDER,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        output_mode: CameraOutputMode::Write {
                            blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                            clear_color: ClearColorConfig::Custom(Color::NONE),
                        },
                        ..default()
                    },
                    Tonemapping::None,
                    Msaa::Off,
                    RenderLayers::layer(MENU_LAYER),
                    eustress_common::plugins::lighting_plugin::NoAtmosphere,
                    MenuCamera,
                    Name::new("Menu Camera"),
                ))
                .id();
            shown.camera = Some(camera);
        }
    }

    // The texture: repainted when the card, the window or its density changed.
    let key = (card.clone(), px, scale.to_bits());
    if shown.painted.as_ref() != Some(&key) {
        let Some(mut pixmap) = Pixmap::new(px.x, px.y) else { return };
        for f in elements(&card, size) {
            render_element(
                &mut pixmap,
                &f.elem,
                f.abs_x,
                f.abs_y,
                f.clip_rect,
                scale,
                &mut text_state,
                &mut gui_images,
                Path::new(""),
            );
        }
        let mut image = Image::new(
            Extent3d { width: px.x, height: px.y, depth_or_array_layers: 1 },
            TextureDimension::D2,
            pixmap.take(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        let handle = images.add(image);
        if let Some(old) = shown.image.replace(handle.clone()) {
            images.remove(&old);
        }
        let existing = shown.material.clone().filter(|m| materials.contains(m));
        if let Some(material) = existing {
            if let Some(mut material) = materials.get_mut(&material) {
                material.base_color_texture = Some(handle);
            }
        } else {
            shown.material = Some(materials.add(StandardMaterial {
                base_color_texture: Some(handle),
                unlit: true,
                alpha_mode: AlphaMode::Premultiplied,
                fog_enabled: false,
                cull_mode: None,
                ..default()
            }));
        }
        shown.painted = Some(key);
    }

    // The quad: the whole window, centred on the camera.
    let want = Transform::from_scale(Vec3::new(size.x, size.y, 1.0));
    match shown.quad.and_then(|q| quads.get_mut(q).ok()) {
        Some(mut tf) => {
            if *tf != want {
                *tf = want;
            }
        }
        None => {
            let Some(material) = shown.material.clone() else { return };
            let mesh = shown.mesh.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
            let quad = commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    want,
                    RenderLayers::layer(MENU_LAYER),
                    MenuQuad,
                    Name::new("Pause Menu"),
                ))
                .id();
            shown.quad = Some(quad);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: (f32, f32) = (1920.0, 1080.0);

    fn size() -> Vec2 {
        Vec2::new(WINDOW.0, WINDOW.1)
    }

    /// The pause menu lists Resume, Settings and Quit, in that order, with the
    /// first row highlighted. Before this module, the menu drew nothing.
    #[test]
    fn the_pause_menu_offers_resume_settings_and_quit() {
        let card = pause_card(0);
        assert_eq!(card.title, "Paused");
        assert_eq!(card.rows, vec!["Resume", "Settings", "Quit"]);
        assert_eq!(card.selected, 0);
        assert!(card.footer.contains("Esc to resume"), "{}", card.footer);
    }

    #[test]
    fn choosing_wraps_at_either_end() {
        assert_eq!(step(0, 3, -1), 2, "up from the first row wraps to the last");
        assert_eq!(step(2, 3, 1), 0, "down from the last row wraps to the first");
        assert_eq!(step(1, 3, 1), 2);
        assert_eq!(step(0, 0, 1), 0, "a card with no rows has nothing to choose");
    }

    #[test]
    fn a_point_on_a_row_is_that_row() {
        let card = pause_card(0);
        let b = boxes(&card, size());
        for (i, row) in b.rows.iter().enumerate() {
            assert_eq!(row_at(&card, size(), row.center()), Some(i), "row {i}");
        }
        assert_eq!(row_at(&card, size(), Vec2::new(5.0, 5.0)), None, "outside the card");
        assert_eq!(row_at(&card, size(), b.title.center()), None, "on the title, not a row");
    }

    #[test]
    fn the_rows_sit_inside_the_card() {
        let card = pause_card(0);
        let b = boxes(&card, size());
        for row in &b.rows {
            assert!(within(&b.card, row.min) && within(&b.card, row.max), "{row:?} outside {:?}", b.card);
        }
        assert!(within(&b.card, b.footer.min) && within(&b.card, b.footer.max));
    }

    #[test]
    fn the_menu_fits_a_narrow_window() {
        let narrow = Vec2::new(360.0, 640.0);
        let b = boxes(&pause_card(0), narrow);
        assert!(b.card.min.x >= 0.0 && b.card.max.x <= narrow.x, "{:?}", b.card);
        assert!(b.card.min.y >= 0.0 && b.card.max.y <= narrow.y, "{:?}", b.card);
    }

    /// The highlighted row is drawn in the highlight colour, the others in the
    /// plain one, and the label of each row is drawn over its own box.
    #[test]
    fn the_selected_row_is_highlighted() {
        let flat = elements(&pause_card(1), size());
        let row_boxes: Vec<[f32; 4]> = flat
            .iter()
            .filter(|f| f.elem.text.is_empty() && f.elem.corner_radius == 8.0)
            .map(|f| f.elem.bg_color)
            .collect();
        assert_eq!(row_boxes.len(), 3);
        assert!(row_boxes[1][2] > row_boxes[0][2], "the selected row is the blue one: {row_boxes:?}");
        let labels: Vec<&str> = flat.iter().filter(|f| !f.elem.text.is_empty()).map(|f| f.elem.text.as_str()).collect();
        assert!(labels.contains(&"Resume") && labels.contains(&"Settings") && labels.contains(&"Quit"), "{labels:?}");
    }

    #[test]
    fn the_settings_card_shows_the_sensitivity_and_back() {
        let card = settings_card(1.5, 0);
        assert_eq!(card.title, "Settings");
        assert_eq!(card.rows, vec!["Mouse sensitivity: 1.50", "Back"]);
    }
}
