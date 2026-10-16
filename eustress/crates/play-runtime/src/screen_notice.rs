//! # A notice on the screen
//!
//! What the Player says to the person in front of it when something stops it,
//! in plain words: it could not join a host, the host turned it away, the
//! world would not download. Before this, those reasons went to the log file
//! only, and the window stayed empty.
//!
//! Any system sets [`ScreenNotice`]; this draws it as a card in the middle of
//! a dimmed screen, painted by the billboard rasterizer (the HUD's own text),
//! on an unlit quad seen by an overlay camera of its own, above the HUD.
//! Clearing it takes the card and its camera away.

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

/// The render layer the notice and its camera share.
pub const NOTICE_LAYER: usize = 28;
/// The notice camera's order: above the HUD's (100), below Studio's Slint
/// editor overlay (300).
pub const NOTICE_CAMERA_ORDER: isize = 200;
/// The largest notice texture side, physical pixels.
const MAX_NOTICE_PX: u32 = 4096;

/// What to tell the player: a headline, what happened, and what to do about
/// it (if anything helps).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Notice {
    pub title: String,
    pub detail: String,
    pub hint: Option<String>,
}

/// The notice on screen, if any.
#[derive(Resource, Debug, Default, Clone)]
pub struct ScreenNotice(pub Option<Notice>);

impl ScreenNotice {
    pub fn show(&mut self, notice: Notice) {
        self.0 = Some(notice);
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

/// The notice's camera.
#[derive(Component, Debug)]
pub struct NoticeCamera;

/// The notice's quad.
#[derive(Component, Debug)]
pub struct NoticeQuad;

/// Draws [`ScreenNotice`]. Needs the billboard drawing plugin (its text state
/// and image cache), which `PlayRuntimePlugin` adds beside it.
pub struct ScreenNoticePlugin;

impl Plugin for ScreenNoticePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenNotice>()
            .add_systems(PostUpdate, show_screen_notice.before(TransformSystems::Propagate));
    }
}

/// The card's look, logical pixels.
const CARD_WIDTH: f32 = 620.0;
const MARGIN: f32 = 16.0;
const PAD: f32 = 28.0;
const GAP: f32 = 12.0;
const TITLE_H: f32 = 36.0;
const DETAIL_H: f32 = 96.0;
const HINT_H: f32 = 48.0;
/// The longest detail shown; the log keeps the whole reason.
const MAX_DETAIL_CHARS: usize = 280;

/// A box of the card, `rect` in window logical pixels.
fn elem(rect: Rect, bg: [f32; 4], text: &str, size: f32, weight: i32, color: [f32; 4]) -> GuiElementDisplay {
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
        text_align: "Left".into(),
        text_y_align: "Top".into(),
        text_stroke_color: [0.0; 4],
        text_scaled: false,
        image_path: String::new(),
        class_type: if text.is_empty() { "Frame" } else { "TextLabel" }.into(),
        mouse_filter: "ignore".into(),
    }
}

/// `detail`, cut to [`MAX_DETAIL_CHARS`] on a character boundary.
fn shortened(detail: &str) -> String {
    let detail = detail.trim();
    if detail.chars().count() <= MAX_DETAIL_CHARS {
        return detail.to_string();
    }
    let cut: String = detail.chars().take(MAX_DETAIL_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The notice laid out on a window of `size` logical pixels, in paint order:
/// the dimmed screen, the card, then its title, detail and hint. The card is
/// centred and keeps a margin on a narrow window; its text is clipped to it.
fn layout(notice: &Notice, size: Vec2) -> Vec<FlatElem> {
    let width = CARD_WIDTH.min(size.x - 2.0 * MARGIN).max(120.0);
    let hint_h = if notice.hint.is_some() { GAP + HINT_H } else { 0.0 };
    let height = PAD + TITLE_H + GAP + DETAIL_H + hint_h + PAD;
    let min = Vec2::new(((size.x - width) * 0.5).max(0.0), ((size.y - height) * 0.5).max(0.0));
    let card = Rect::from_corners(min, min + Vec2::new(width, height));
    let inner_w = width - 2.0 * PAD;
    let row = |top: f32, h: f32| Rect::from_corners(card.min + Vec2::new(PAD, top), card.min + Vec2::new(PAD + inner_w, top + h));
    let clip = Some([card.min.x, card.min.y, card.width(), card.height()]);
    let flat = |e: GuiElementDisplay, clip: Option<[f32; 4]>| FlatElem { abs_x: e.x, abs_y: e.y, elem: e, clip_rect: clip };

    let mut card_box = elem(card, [0.11, 0.12, 0.14, 0.96], "", 0.0, 400, [0.0; 4]);
    card_box.corner_radius = 12.0;
    card_box.border_size = 1.0;
    card_box.border_color = [1.0, 1.0, 1.0, 0.12];

    let mut out = vec![
        flat(elem(Rect::from_corners(Vec2::ZERO, size), [0.0, 0.0, 0.0, 0.6], "", 0.0, 400, [0.0; 4]), None),
        flat(card_box, None),
        flat(elem(row(PAD, TITLE_H), [0.0; 4], &notice.title, 24.0, 700, [1.0, 1.0, 1.0, 1.0]), clip),
        flat(
            elem(row(PAD + TITLE_H + GAP, DETAIL_H), [0.0; 4], &shortened(&notice.detail), 17.0, 400, [0.86, 0.88, 0.92, 1.0]),
            clip,
        ),
    ];
    if let Some(hint) = &notice.hint {
        let top = PAD + TITLE_H + GAP + DETAIL_H + GAP;
        out.push(flat(elem(row(top, HINT_H), [0.0; 4], hint, 15.0, 400, [0.62, 0.80, 1.0, 1.0]), clip));
    }
    out
}

/// The notice's camera, quad and texture, and what it last painted.
#[derive(Default)]
struct Shown {
    camera: Option<Entity>,
    quad: Option<Entity>,
    image: Option<Handle<Image>>,
    material: Option<Handle<StandardMaterial>>,
    mesh: Option<Handle<Mesh>>,
    painted: Option<(Notice, UVec2, u32)>,
}

/// Shows [`ScreenNotice`] while it is set: paints the card whenever the
/// notice or the window changes, and takes everything away once it is
/// cleared.
#[allow(clippy::too_many_arguments)]
fn show_screen_notice(
    mut commands: Commands,
    notice: Res<ScreenNotice>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut text_state: NonSendMut<BillboardTextState>,
    mut gui_images: ResMut<GuiImages>,
    mut assets: (ResMut<Assets<Image>>, ResMut<Assets<StandardMaterial>>, ResMut<Assets<Mesh>>),
    mut cameras: Query<&mut Projection, With<NoticeCamera>>,
    mut quads: Query<&mut Transform, With<NoticeQuad>>,
    mut shown: Local<Shown>,
) {
    let (images, materials, meshes) = (&mut assets.0, &mut assets.1, &mut assets.2);
    let Some(current) = notice.0.clone() else {
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
        ((size.x * scale).round() as u32).clamp(1, MAX_NOTICE_PX),
        ((size.y * scale).round() as u32).clamp(1, MAX_NOTICE_PX),
    );
    let projection = Projection::from(OrthographicProjection {
        near: -1000.0,
        far: 1000.0,
        scaling_mode: ScalingMode::Fixed { width: size.x, height: size.y },
        ..OrthographicProjection::default_3d()
    });

    // The camera: set up as the HUD's, one layer and one order up.
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
                        order: NOTICE_CAMERA_ORDER,
                        clear_color: ClearColorConfig::Custom(Color::NONE),
                        output_mode: CameraOutputMode::Write {
                            blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                            clear_color: ClearColorConfig::Custom(Color::NONE),
                        },
                        ..default()
                    },
                    Tonemapping::None,
                    Msaa::Off,
                    RenderLayers::layer(NOTICE_LAYER),
                    eustress_common::plugins::lighting_plugin::NoAtmosphere,
                    NoticeCamera,
                    Name::new("Notice Camera"),
                ))
                .id();
            shown.camera = Some(camera);
        }
    }

    // The texture: repainted when the notice, the window or its density changed.
    let key = (current.clone(), px, scale.to_bits());
    if shown.painted.as_ref() != Some(&key) {
        let Some(mut pixmap) = Pixmap::new(px.x, px.y) else { return };
        for f in layout(&current, size) {
            render_element(
                &mut pixmap,
                &f.elem,
                f.abs_x,
                f.abs_y,
                f.clip_rect,
                scale,
                &mut text_state,
                &mut gui_images,
                std::path::Path::new(""),
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
                    RenderLayers::layer(NOTICE_LAYER),
                    NoticeQuad,
                    Name::new("Notice"),
                ))
                .id();
            shown.quad = Some(quad);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(hint: Option<&str>) -> Notice {
        Notice { title: "Could not join".into(), detail: "The host did not answer.".into(), hint: hint.map(str::to_string) }
    }

    fn rect(f: &FlatElem) -> Rect {
        Rect::from_corners(Vec2::new(f.abs_x, f.abs_y), Vec2::new(f.abs_x + f.elem.width, f.abs_y + f.elem.height))
    }

    /// The screen dims whole, and the card sits in its middle with its text
    /// inside it, clipped to it.
    #[test]
    fn the_card_is_centred_over_a_dimmed_screen() {
        let size = Vec2::new(1920.0, 1080.0);
        let flat = layout(&notice(Some("Open the link again.")), size);
        assert_eq!(flat.len(), 5, "backdrop, card, title, detail, hint");
        assert_eq!(rect(&flat[0]), Rect::from_corners(Vec2::ZERO, size));
        assert!(flat[0].elem.bg_color[3] > 0.0 && flat[0].elem.text.is_empty());
        let card = rect(&flat[1]);
        assert!((card.center() - size * 0.5).length() < 0.5, "centred: {card:?}");
        for f in &flat[2..] {
            let r = rect(f);
            assert!(card.contains(r.min) && card.contains(r.max), "{:?} inside the card", f.elem.text);
            assert_eq!(f.clip_rect, Some([card.min.x, card.min.y, card.width(), card.height()]));
        }
        assert_eq!(flat[2].elem.text, "Could not join");
        assert_eq!(flat[4].elem.text, "Open the link again.");
    }

    /// No hint, no hint row, and a shorter card.
    #[test]
    fn a_notice_without_a_hint_has_no_hint_row() {
        let size = Vec2::new(1280.0, 720.0);
        let with = layout(&notice(Some("x")), size);
        let without = layout(&notice(None), size);
        assert_eq!(without.len(), 4);
        assert!(rect(&without[1]).height() < rect(&with[1]).height());
    }

    /// A phone-narrow window keeps the card on screen with its margin.
    #[test]
    fn a_narrow_window_keeps_the_card_on_screen() {
        let size = Vec2::new(360.0, 640.0);
        let card = rect(&layout(&notice(None), size)[1]);
        assert!((card.min.x - MARGIN).abs() < 0.01 && (card.max.x - (size.x - MARGIN)).abs() < 0.01, "{card:?}");
    }

    /// A long reason is cut on a character boundary, with an ellipsis.
    #[test]
    fn a_long_detail_is_shortened() {
        let long = "é".repeat(MAX_DETAIL_CHARS + 50);
        let short = shortened(&long);
        assert_eq!(short.chars().count(), MAX_DETAIL_CHARS);
        assert!(short.ends_with('\u{2026}'));
        assert_eq!(shortened("  as is  "), "as is");
    }
}
