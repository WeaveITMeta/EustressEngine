// Icon System - Vector icons rendered directly in egui
// Replaces emoji-based icons with reliable vector graphics

use bevy_egui::egui::{self, Color32, Pos2, Stroke, Rect, Vec2, StrokeKind};
use crate::classes::ClassName;
use crate::ui::explorer::ServiceType;

// ============================================================================
// Icon Size Constants
// ============================================================================

pub const ICON_SIZE: f32 = 14.0;
pub const ICON_PADDING: f32 = 1.0;

// ============================================================================
// Class Icon Rendering
// ============================================================================

/// Draw a class icon at the given position
pub fn draw_class_icon(painter: &egui::Painter, pos: Pos2, class_name: ClassName, size: f32) {
    let color = class_color(class_name);
    let dark = darken_color(color, 0.3);
    
    match class_name {
        // Parts - 3D cube
        ClassName::Part | ClassName::BasePart | ClassName::PVInstance => {
            draw_cube_icon(painter, pos, size, color, dark);
        }
        // MeshPart - wireframe diamond
        ClassName::MeshPart => {
            draw_diamond_icon(painter, pos, size, color, dark);
        }
        // Model - stacked boxes
        ClassName::Model => {
            draw_model_icon(painter, pos, size, color, dark);
        }
        // Folder - folder shape
        ClassName::Folder => {
            draw_folder_icon(painter, pos, size, color, dark);
        }
        // Humanoid - stick figure
        ClassName::Humanoid => {
            draw_humanoid_icon(painter, pos, size, color);
        }
        // Camera
        ClassName::Camera => {
            draw_camera_icon(painter, pos, size, color, dark);
        }
        // Lights
        ClassName::PointLight => {
            draw_pointlight_icon(painter, pos, size, color);
        }
        ClassName::SpotLight => {
            draw_spotlight_icon(painter, pos, size, color, dark);
        }
        ClassName::SurfaceLight => {
            draw_surfacelight_icon(painter, pos, size, color);
        }
        ClassName::DirectionalLight => {
            draw_sun_icon(painter, pos, size, color);
        }
        // Constraints
        ClassName::Attachment => {
            draw_pin_icon(painter, pos, size, color, dark);
        }
        ClassName::WeldConstraint => {
            draw_link_icon(painter, pos, size, color);
        }
        ClassName::Motor6D => {
            draw_gear_icon(painter, pos, size, color, dark);
        }
        // Effects
        ClassName::ParticleEmitter => {
            draw_sparkle_icon(painter, pos, size, color);
        }
        ClassName::Beam => {
            draw_beam_icon(painter, pos, size, color);
        }
        // Soul Scripts
        ClassName::SoulScript => {
            draw_script_icon(painter, pos, size, color, dark, Some("S"));
        }
        // Audio
        ClassName::Sound => {
            draw_speaker_icon(painter, pos, size, color);
        }
        // Environment
        ClassName::Terrain => {
            draw_mountain_icon(painter, pos, size, color, dark);
        }
        ClassName::Sky => {
            draw_cloud_icon(painter, pos, size, color);
        }
        ClassName::Atmosphere => {
            draw_haze_icon(painter, pos, size, color);
        }
        // GUI
        ClassName::BillboardGui => {
            draw_billboard_icon(painter, pos, size, color, dark);
        }
        ClassName::SurfaceGui => {
            draw_billboard_icon(painter, pos, size, color, dark); // Similar to billboard
        }
        ClassName::ScreenGui => {
            draw_document_icon(painter, pos, size, color, dark); // Screen overlay
        }
        ClassName::TextLabel => {
            draw_text_icon(painter, pos, size, color, dark);
        }
        ClassName::Frame => {
            draw_document_icon(painter, pos, size, color, dark); // Container frame
        }
        ClassName::ImageLabel => {
            draw_image_icon(painter, pos, size, color, dark);
        }
        ClassName::TextButton => {
            draw_text_icon(painter, pos, size, color, dark); // Button with text
        }
        ClassName::ImageButton => {
            draw_image_icon(painter, pos, size, color, dark); // Button with image
        }
        // Visuals
        ClassName::Decal => {
            draw_image_icon(painter, pos, size, color, dark);
        }
        ClassName::SpecialMesh | ClassName::UnionOperation => {
            draw_mesh_icon(painter, pos, size, color);
        }
        // Animation
        ClassName::Animator | ClassName::KeyframeSequence => {
            draw_film_icon(painter, pos, size, color, dark);
        }
        // Services
        ClassName::Lighting => {
            draw_sun_icon(painter, pos, size, color);
        }
        ClassName::Workspace => {
            draw_globe_icon(painter, pos, size, color, dark);
        }
        ClassName::SpawnLocation => {
            draw_target_icon(painter, pos, size, color);
        }
        // Celestial
        ClassName::Clouds => {
            draw_cloud_icon(painter, pos, size, color);
        }
        ClassName::Sun => {
            draw_sun_icon(painter, pos, size, color);
        }
        ClassName::Moon => {
            draw_moon_icon(painter, pos, size, color);
        }
        // Seats
        ClassName::Seat => {
            draw_cube_icon(painter, pos, size, color, dark); // Simple seat icon
        }
        ClassName::VehicleSeat => {
            draw_gear_icon(painter, pos, size, color, dark); // Vehicle seat with gear
        }
        // Media Asset Classes
        ClassName::Document => {
            draw_document_icon(painter, pos, size, color, dark); // Document icon
        }
        ClassName::ImageAsset => {
            draw_image_icon(painter, pos, size, color, dark); // Image icon
        }
        ClassName::VideoAsset => {
            draw_play_icon(painter, pos, size, color, dark); // Video play icon
        }
        // ScrollingFrame
        ClassName::ScrollingFrame => {
            draw_scrolling_frame_icon(painter, pos, size, color, dark);
        }
        // VideoFrame
        ClassName::VideoFrame => {
            draw_video_frame_icon(painter, pos, size, color, dark);
        }
        // DocumentFrame
        ClassName::DocumentFrame => {
            draw_document_frame_icon(painter, pos, size, color, dark);
        }
        // WebFrame - use globe icon for web content
        ClassName::WebFrame => {
            draw_globe_icon(painter, pos, size, color, dark);
        }
        // TextBox - use edit icon
        ClassName::TextBox => {
            draw_document_icon(painter, pos, size, color, dark);
        }
        // ViewportFrame - use 3D icon
        ClassName::ViewportFrame => {
            draw_cube_icon(painter, pos, size, color, dark);
        }
        // Team - use flag icon
        ClassName::Team => {
            draw_flag_icon(painter, pos, size, color);
        }
        // Orbital classes
        ClassName::SolarSystem => {
            draw_globe_icon(painter, pos, size, color, dark); // Solar system as globe
        }
        ClassName::CelestialBody => {
            draw_moon_icon(painter, pos, size, color); // Planet/moon as sphere
        }
        ClassName::RegionChunk => {
            draw_mountain_icon(painter, pos, size, color, dark); // Region as terrain
        }
        // Default - document
        ClassName::Instance => {
            draw_document_icon(painter, pos, size, color, dark);
        }
    }
}

/// Draw a service icon at the given position
pub fn draw_service_icon(painter: &egui::Painter, pos: Pos2, service: ServiceType, size: f32) {
    let color = service.color();
    let dark = darken_color(color, 0.3);
    
    match service {
        ServiceType::Workspace => draw_globe_icon(painter, pos, size, color, dark),
        ServiceType::Players => draw_players_icon(painter, pos, size, color),
        ServiceType::Lighting => draw_sun_icon(painter, pos, size, color),
        ServiceType::SoulService => draw_script_icon(painter, pos, size, color, dark, Some("S")),
        ServiceType::ServerStorage => draw_server_icon(painter, pos, size, color, dark),
        ServiceType::StarterGui => draw_window_icon(painter, pos, size, color, dark),
        ServiceType::StarterPack => draw_backpack_icon(painter, pos, size, color, dark),
        ServiceType::StarterPlayer => draw_starterplayer_icon(painter, pos, size, color),
        ServiceType::SoundService => draw_music_icon(painter, pos, size, color),
        ServiceType::Teams => draw_flag_icon(painter, pos, size, color),
        ServiceType::Chat => draw_chat_icon(painter, pos, size, color),
        ServiceType::LocalizationService => draw_globe_icon(painter, pos, size, color, dark),
        ServiceType::TestService => draw_flask_icon(painter, pos, size, color),
    }
}

// ============================================================================
// Icon Drawing Functions
// ============================================================================

fn draw_cube_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Top face
    let top = [
        Pos2::new(cx, cy - s * 0.4),
        Pos2::new(cx + s * 0.4, cy - s * 0.15),
        Pos2::new(cx, cy + s * 0.1),
        Pos2::new(cx - s * 0.4, cy - s * 0.15),
    ];
    painter.add(egui::Shape::convex_polygon(top.to_vec(), lighten_color(color, 0.2), Stroke::new(0.5, dark)));
    
    // Left face
    let left = [
        Pos2::new(cx - s * 0.4, cy - s * 0.15),
        Pos2::new(cx, cy + s * 0.1),
        Pos2::new(cx, cy + s * 0.45),
        Pos2::new(cx - s * 0.4, cy + s * 0.2),
    ];
    painter.add(egui::Shape::convex_polygon(left.to_vec(), color, Stroke::new(0.5, dark)));
    
    // Right face
    let right = [
        Pos2::new(cx + s * 0.4, cy - s * 0.15),
        Pos2::new(cx + s * 0.4, cy + s * 0.2),
        Pos2::new(cx, cy + s * 0.45),
        Pos2::new(cx, cy + s * 0.1),
    ];
    painter.add(egui::Shape::convex_polygon(right.to_vec(), darken_color(color, 0.15), Stroke::new(0.5, dark)));
}

fn draw_diamond_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    let points = [
        Pos2::new(cx, cy - s * 0.45),
        Pos2::new(cx + s * 0.4, cy),
        Pos2::new(cx, cy + s * 0.45),
        Pos2::new(cx - s * 0.4, cy),
    ];
    painter.add(egui::Shape::convex_polygon(points.to_vec(), color, Stroke::new(1.0, dark)));
    
    // Cross lines
    painter.line_segment([Pos2::new(cx, cy - s * 0.45), Pos2::new(cx, cy + s * 0.45)], Stroke::new(0.5, dark));
    painter.line_segment([Pos2::new(cx - s * 0.4, cy), Pos2::new(cx + s * 0.4, cy)], Stroke::new(0.5, dark));
}

fn draw_model_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.3;
    
    // Back box
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(pos.x + s * 0.5, pos.y + s * 0.3), Vec2::new(s * 1.8, s * 1.8)),
        2.0, darken_color(color, 0.1)
    );
    // Front box
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(pos.x + s * 0.2, pos.y + s * 1.0), Vec2::new(s * 1.8, s * 1.8)),
        2.0, color
    );
    painter.rect_stroke(
        Rect::from_min_size(Pos2::new(pos.x + s * 0.2, pos.y + s * 1.0), Vec2::new(s * 1.8, s * 1.8)),
        egui::CornerRadius::same(2), Stroke::new(0.5, dark), StrokeKind::Outside
    );
}

fn draw_folder_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.15;
    
    // Tab
    let tab = [
        Pos2::new(x, y + s * 0.15),
        Pos2::new(x + s * 0.3, y + s * 0.15),
        Pos2::new(x + s * 0.4, y),
        Pos2::new(x + s * 0.7, y),
        Pos2::new(x + s * 0.7, y + s * 0.15),
    ];
    painter.add(egui::Shape::line(tab.to_vec(), Stroke::new(2.0, dark)));
    
    // Body
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(x, y + s * 0.15), Vec2::new(s, s * 0.7)),
        2.0, color
    );
    painter.rect_stroke(
        Rect::from_min_size(Pos2::new(x, y + s * 0.15), Vec2::new(s, s * 0.7)),
        egui::CornerRadius::same(2), Stroke::new(0.5, dark), StrokeKind::Outside
    );
}

fn draw_humanoid_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let s = size * 0.35;
    
    // Head
    painter.circle_filled(Pos2::new(cx, cy - s * 0.8), s * 0.4, color);
    // Body
    painter.line_segment([Pos2::new(cx, cy - s * 0.4), Pos2::new(cx, cy + s * 0.3)], Stroke::new(1.5, color));
    // Arms
    painter.line_segment([Pos2::new(cx - s * 0.6, cy - s * 0.1), Pos2::new(cx + s * 0.6, cy - s * 0.1)], Stroke::new(1.5, color));
    // Legs
    painter.line_segment([Pos2::new(cx, cy + s * 0.3), Pos2::new(cx - s * 0.4, cy + s * 1.0)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx, cy + s * 0.3), Pos2::new(cx + s * 0.4, cy + s * 1.0)], Stroke::new(1.5, color));
}

fn draw_camera_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.2;
    
    // Body
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y + s * 0.15), Vec2::new(s, s * 0.6)), 2.0, color);
    // Lens
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.45), s * 0.2, dark);
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.45), s * 0.12, color);
    // Flash
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.2, y), Vec2::new(s * 0.4, s * 0.15)), 1.0, color);
}

fn draw_pointlight_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.25;
    
    // Glow
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    // Rays
    for i in 0..8 {
        let angle = (i as f32) * std::f32::consts::PI / 4.0;
        let inner = r * 1.3;
        let outer = r * 1.8;
        painter.line_segment([
            Pos2::new(cx + angle.cos() * inner, cy + angle.sin() * inner),
            Pos2::new(cx + angle.cos() * outer, cy + angle.sin() * outer),
        ], Stroke::new(1.0, color));
    }
}

fn draw_spotlight_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let cx = pos.x + size / 2.0;
    let y = pos.y + size * 0.1;
    let s = size * 0.8;
    
    // Housing
    painter.rect_filled(Rect::from_min_size(Pos2::new(cx - s * 0.25, y), Vec2::new(s * 0.5, s * 0.25)), 2.0, dark);
    // Cone
    let cone = [
        Pos2::new(cx - s * 0.15, y + s * 0.25),
        Pos2::new(cx + s * 0.15, y + s * 0.25),
        Pos2::new(cx + s * 0.4, y + s * 0.9),
        Pos2::new(cx - s * 0.4, y + s * 0.9),
    ];
    painter.add(egui::Shape::convex_polygon(cone.to_vec(), Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 100), Stroke::NONE));
}

fn draw_surfacelight_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.2;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.5)), 2.0, color);
    // Rays down
    for i in 0..3 {
        let rx = x + s * 0.2 + (i as f32) * s * 0.3;
        painter.line_segment([Pos2::new(rx, y + s * 0.5), Pos2::new(rx, y + s * 0.8)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150)));
    }
}

fn draw_sun_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.22;
    
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    // Rays
    for i in 0..8 {
        let angle = (i as f32) * std::f32::consts::PI / 4.0;
        let inner = r * 1.4;
        let outer = r * 2.0;
        painter.line_segment([
            Pos2::new(cx + angle.cos() * inner, cy + angle.sin() * inner),
            Pos2::new(cx + angle.cos() * outer, cy + angle.sin() * outer),
        ], Stroke::new(1.2, color));
    }
}

fn draw_moon_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.35;
    
    // Main moon circle
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    
    // Crescent shadow (darker circle offset to create crescent)
    let dark = Color32::from_rgb(30, 30, 40);
    painter.circle_filled(Pos2::new(cx + r * 0.5, cy - r * 0.2), r * 0.85, dark);
}

fn draw_pin_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.25;
    
    painter.circle_filled(Pos2::new(cx, cy - r * 0.3), r, color);
    painter.circle_filled(Pos2::new(cx, cy - r * 0.3), r * 0.4, dark);
    // Point
    let point = [
        Pos2::new(cx - r * 0.5, cy + r * 0.3),
        Pos2::new(cx + r * 0.5, cy + r * 0.3),
        Pos2::new(cx, cy + r * 1.5),
    ];
    painter.add(egui::Shape::convex_polygon(point.to_vec(), color, Stroke::NONE));
}

fn draw_link_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let s = size * 0.3;
    
    // Two interlocking ovals
    painter.add(egui::Shape::ellipse_stroke(Pos2::new(cx - s * 0.4, cy), Vec2::new(s * 0.6, s * 1.0), Stroke::new(2.0, color)));
    painter.add(egui::Shape::ellipse_stroke(Pos2::new(cx + s * 0.4, cy), Vec2::new(s * 0.6, s * 1.0), Stroke::new(2.0, color)));
}

fn draw_gear_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.3;
    
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    painter.circle_filled(Pos2::new(cx, cy), r * 0.4, dark);
    // Teeth
    for i in 0..4 {
        let angle = (i as f32) * std::f32::consts::PI / 2.0;
        let tx = cx + angle.cos() * r * 1.3;
        let ty = cy + angle.sin() * r * 1.3;
        painter.rect_filled(Rect::from_center_size(Pos2::new(tx, ty), Vec2::new(size * 0.15, size * 0.15)), 1.0, color);
    }
}

fn draw_sparkle_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    painter.circle_filled(Pos2::new(cx, cy), size * 0.15, color);
    painter.circle_filled(Pos2::new(cx - size * 0.25, cy - size * 0.25), size * 0.08, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 180));
    painter.circle_filled(Pos2::new(cx + size * 0.3, cy - size * 0.2), size * 0.1, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150));
    painter.circle_filled(Pos2::new(cx - size * 0.2, cy + size * 0.3), size * 0.06, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 120));
    painter.circle_filled(Pos2::new(cx + size * 0.25, cy + size * 0.25), size * 0.07, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 140));
}

fn draw_beam_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.7;
    let s = size * 0.8;
    
    // Curved beam
    let points: Vec<Pos2> = (0..10).map(|i| {
        let t = i as f32 / 9.0;
        let px = x + t * s;
        let py = y - (t * (1.0 - t) * 4.0) * s * 0.6;
        Pos2::new(px, py)
    }).collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.5, color)));
    
    // End points
    painter.circle_filled(Pos2::new(x, y), size * 0.1, color);
    painter.circle_filled(Pos2::new(x + s, y), size * 0.1, color);
}

fn draw_script_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32, letter: Option<&str>) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.05;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 1.1)), 2.0, color);
    painter.rect_stroke(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 1.1)), egui::CornerRadius::same(2), Stroke::new(0.5, dark), StrokeKind::Outside);
    
    if let Some(l) = letter {
        painter.text(
            Pos2::new(x + s * 0.5, y + s * 0.65),
            egui::Align2::CENTER_CENTER,
            l,
            egui::FontId::proportional(size * 0.5),
            dark,
        );
    } else {
        // Lines
        for i in 0..3 {
            let ly = y + s * 0.25 + (i as f32) * s * 0.25;
            painter.line_segment([Pos2::new(x + s * 0.15, ly), Pos2::new(x + s * 0.85, ly)], Stroke::new(0.8, dark));
        }
    }
}

fn draw_speaker_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let x = pos.x + size * 0.1;
    let cy = pos.y + size / 2.0;
    let s = size * 0.3;
    
    // Speaker body
    let body = [
        Pos2::new(x, cy - s * 0.4),
        Pos2::new(x + s * 0.5, cy - s * 0.4),
        Pos2::new(x + s * 1.2, cy - s * 1.0),
        Pos2::new(x + s * 1.2, cy + s * 1.0),
        Pos2::new(x + s * 0.5, cy + s * 0.4),
        Pos2::new(x, cy + s * 0.4),
    ];
    painter.add(egui::Shape::convex_polygon(body.to_vec(), color, Stroke::NONE));
    
    // Sound waves
    for i in 1..3 {
        let r = s * 0.5 * (i as f32);
        painter.add(egui::Shape::circle_stroke(Pos2::new(x + s * 1.5 + r * 0.3, cy), r, Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 200 - i * 50))));
    }
}

fn draw_mountain_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.9;
    let x = pos.x + size * 0.05;
    let y = pos.y + size * 0.9;
    
    // Mountains
    let m1 = [Pos2::new(x, y), Pos2::new(x + s * 0.35, y - s * 0.7), Pos2::new(x + s * 0.6, y)];
    painter.add(egui::Shape::convex_polygon(m1.to_vec(), color, Stroke::new(0.5, dark)));
    
    let m2 = [Pos2::new(x + s * 0.4, y), Pos2::new(x + s * 0.7, y - s * 0.85), Pos2::new(x + s, y)];
    painter.add(egui::Shape::convex_polygon(m2.to_vec(), lighten_color(color, 0.1), Stroke::new(0.5, dark)));
    
    // Snow cap
    let snow = [Pos2::new(x + s * 0.6, y - s * 0.55), Pos2::new(x + s * 0.7, y - s * 0.85), Pos2::new(x + s * 0.8, y - s * 0.55)];
    painter.add(egui::Shape::convex_polygon(snow.to_vec(), Color32::WHITE, Stroke::NONE));
}

fn draw_cloud_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let s = size * 0.25;
    
    painter.circle_filled(Pos2::new(cx - s * 0.8, cy + s * 0.3), s * 0.8, color);
    painter.circle_filled(Pos2::new(cx + s * 0.6, cy + s * 0.2), s * 0.9, color);
    painter.circle_filled(Pos2::new(cx, cy - s * 0.2), s * 0.7, color);
    
    // Sun
    painter.circle_filled(Pos2::new(cx + s * 1.2, cy - s * 0.8), s * 0.5, Color32::from_rgb(255, 255, 100));
}

fn draw_haze_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let s = size * 0.35;
    
    for i in 0..4 {
        let y = pos.y + size * 0.2 + (i as f32) * size * 0.2;
        let alpha = 60 + i * 30;
        painter.add(egui::Shape::ellipse_filled(
            Pos2::new(cx, y),
            Vec2::new(s * (1.5 - i as f32 * 0.2), s * 0.3),
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha as u8)
        ));
    }
}

fn draw_billboard_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.6)), 2.0, color);
    painter.rect_stroke(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.6)), egui::CornerRadius::same(2), Stroke::new(0.5, dark), StrokeKind::Outside);
    // Pole
    painter.line_segment([Pos2::new(x + s * 0.5, y + s * 0.6), Pos2::new(x + s * 0.5, y + s * 1.0)], Stroke::new(1.5, dark));
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 1.0), size * 0.06, dark);
}

fn draw_text_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s)), 2.0, color);
    painter.text(
        Pos2::new(x + s * 0.5, y + s * 0.55),
        egui::Align2::CENTER_CENTER,
        "T",
        egui::FontId::proportional(size * 0.6),
        Color32::WHITE,
    );
}

fn draw_image_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s)), 2.0, color);
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.1, y + s * 0.1), Vec2::new(s * 0.8, s * 0.8)), 1.0, dark);
    
    // Mountain in frame
    let m = [Pos2::new(x + s * 0.15, y + s * 0.8), Pos2::new(x + s * 0.4, y + s * 0.4), Pos2::new(x + s * 0.85, y + s * 0.8)];
    painter.add(egui::Shape::convex_polygon(m.to_vec(), color, Stroke::NONE));
    // Sun
    painter.circle_filled(Pos2::new(x + s * 0.7, y + s * 0.3), s * 0.1, Color32::from_rgb(255, 255, 100));
}

fn draw_mesh_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let s = size * 0.4;
    
    // Triangle wireframe
    let tri = [
        Pos2::new(cx, cy - s),
        Pos2::new(cx + s * 0.9, cy + s * 0.7),
        Pos2::new(cx - s * 0.9, cy + s * 0.7),
    ];
    painter.add(egui::Shape::closed_line(tri.to_vec(), Stroke::new(1.5, color)));
    // Internal lines
    painter.line_segment([Pos2::new(cx, cy - s), Pos2::new(cx, cy + s * 0.7)], Stroke::new(0.5, color));
    painter.line_segment([Pos2::new(cx - s * 0.45, cy - s * 0.15), Pos2::new(cx + s * 0.45, cy + s * 0.7)], Stroke::new(0.5, color));
}

fn draw_film_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.15;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.7)), 2.0, color);
    // Clapboard top
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y - s * 0.2), Vec2::new(s, s * 0.2)), 1.0, dark);
    // Stripes
    for i in 0..3 {
        let sx = x + s * 0.2 + (i as f32) * s * 0.25;
        painter.line_segment([Pos2::new(sx, y - s * 0.2), Pos2::new(sx + s * 0.15, y)], Stroke::new(1.0, color));
    }
}

fn draw_document_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.7;
    let x = pos.x + size * 0.15;
    let y = pos.y + size * 0.05;
    
    // Document with folded corner
    let doc = [
        Pos2::new(x, y),
        Pos2::new(x + s * 0.7, y),
        Pos2::new(x + s, y + s * 0.3),
        Pos2::new(x + s, y + s * 1.2),
        Pos2::new(x, y + s * 1.2),
    ];
    painter.add(egui::Shape::convex_polygon(doc.to_vec(), color, Stroke::new(0.5, dark)));
    // Fold
    let fold = [
        Pos2::new(x + s * 0.7, y),
        Pos2::new(x + s * 0.7, y + s * 0.3),
        Pos2::new(x + s, y + s * 0.3),
    ];
    painter.add(egui::Shape::convex_polygon(fold.to_vec(), lighten_color(color, 0.2), Stroke::new(0.5, dark)));
}

fn draw_play_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let s = size * 0.35;
    
    // Circle background
    painter.circle_filled(Pos2::new(cx, cy), s * 1.2, dark);
    
    // Play triangle
    let triangle = [
        Pos2::new(cx - s * 0.4, cy - s * 0.6),
        Pos2::new(cx + s * 0.6, cy),
        Pos2::new(cx - s * 0.4, cy + s * 0.6),
    ];
    painter.add(egui::Shape::convex_polygon(triangle.to_vec(), color, Stroke::NONE));
}

fn draw_video_frame_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    // Frame background
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.7)), 2.0, color);
    
    // Play button in center
    let cx = x + s / 2.0;
    let cy = y + s * 0.35;
    let ps = s * 0.2;
    let triangle = [
        Pos2::new(cx - ps * 0.4, cy - ps * 0.6),
        Pos2::new(cx + ps * 0.6, cy),
        Pos2::new(cx - ps * 0.4, cy + ps * 0.6),
    ];
    painter.add(egui::Shape::convex_polygon(triangle.to_vec(), dark, Stroke::NONE));
    
    // Progress bar at bottom
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.1, y + s * 0.6), Vec2::new(s * 0.8, s * 0.05)), 1.0, darken_color(color, 0.3));
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.1, y + s * 0.6), Vec2::new(s * 0.3, s * 0.05)), 1.0, dark);
}

fn draw_document_frame_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.7;
    let x = pos.x + size * 0.15;
    let y = pos.y + size * 0.05;
    
    // Document background
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 1.2)), 2.0, color);
    painter.rect_stroke(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 1.2)), 2.0, Stroke::new(0.5, dark), egui::StrokeKind::Outside);
    
    // Text lines
    for i in 0..4 {
        let line_y = y + s * 0.15 + (i as f32) * s * 0.22;
        let line_w = if i == 3 { s * 0.5 } else { s * 0.7 };
        painter.line_segment(
            [Pos2::new(x + s * 0.15, line_y), Pos2::new(x + s * 0.15 + line_w, line_y)],
            Stroke::new(1.0, darken_color(color, 0.4))
        );
    }
    
    // Page number indicator at bottom
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.3, y + s * 1.05), Vec2::new(s * 0.4, s * 0.1)), 1.0, dark);
}

fn draw_scrolling_frame_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    // Frame filled background
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.9)), 2.0, color);
    // Frame outline
    painter.rect_stroke(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.9)), 2.0, Stroke::new(1.0, dark), egui::StrokeKind::Outside);
    
    // Scrollbar track (right side)
    let bar_x = x + s - s * 0.15;
    let bar_y = y + s * 0.1;
    let bar_h = s * 0.7;
    painter.rect_filled(Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(s * 0.1, bar_h)), 1.0, darken_color(color, 0.2));
    
    // Scrollbar thumb
    painter.rect_filled(Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(s * 0.1, bar_h * 0.4)), 1.0, dark);
    
    // Content lines (to show scrollable content)
    for i in 0..3 {
        let line_y = y + s * 0.15 + (i as f32) * s * 0.2;
        painter.line_segment(
            [Pos2::new(x + s * 0.1, line_y), Pos2::new(x + s * 0.7, line_y)],
            Stroke::new(1.0, darken_color(color, 0.3))
        );
    }
}

fn draw_globe_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.38;
    
    painter.circle_filled(Pos2::new(cx, cy), r, color);
    painter.circle_stroke(Pos2::new(cx, cy), r, Stroke::new(0.5, dark));
    // Latitude
    painter.add(egui::Shape::ellipse_stroke(Pos2::new(cx, cy), Vec2::new(r, r * 0.4), Stroke::new(0.5, dark)));
    // Longitude
    painter.add(egui::Shape::ellipse_stroke(Pos2::new(cx, cy), Vec2::new(r * 0.4, r), Stroke::new(0.5, dark)));
}

fn draw_target_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    painter.circle_stroke(Pos2::new(cx, cy), size * 0.38, Stroke::new(1.5, color));
    painter.circle_stroke(Pos2::new(cx, cy), size * 0.22, Stroke::new(1.0, color));
    painter.circle_filled(Pos2::new(cx, cy), size * 0.08, color);
    // Crosshairs
    painter.line_segment([Pos2::new(cx, cy - size * 0.45), Pos2::new(cx, cy - size * 0.25)], Stroke::new(1.0, color));
    painter.line_segment([Pos2::new(cx, cy + size * 0.25), Pos2::new(cx, cy + size * 0.45)], Stroke::new(1.0, color));
    painter.line_segment([Pos2::new(cx - size * 0.45, cy), Pos2::new(cx - size * 0.25, cy)], Stroke::new(1.0, color));
    painter.line_segment([Pos2::new(cx + size * 0.25, cy), Pos2::new(cx + size * 0.45, cy)], Stroke::new(1.0, color));
}

// Service-specific icons

fn draw_players_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let s = size * 0.25;
    // Two people
    painter.circle_filled(Pos2::new(pos.x + s * 1.2, pos.y + s * 1.0), s * 0.6, color);
    painter.add(egui::Shape::ellipse_filled(Pos2::new(pos.x + s * 1.2, pos.y + s * 2.5), Vec2::new(s * 0.9, s * 1.2), color));
    
    painter.circle_filled(Pos2::new(pos.x + s * 2.8, pos.y + s * 1.0), s * 0.6, color);
    painter.add(egui::Shape::ellipse_filled(Pos2::new(pos.x + s * 2.8, pos.y + s * 2.5), Vec2::new(s * 0.9, s * 1.2), color));
}

fn draw_fastforward_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let s = size * 0.35;
    let x = pos.x + size * 0.1;
    let cy = pos.y + size / 2.0;
    
    let t1 = [Pos2::new(x, cy - s), Pos2::new(x + s * 1.2, cy), Pos2::new(x, cy + s)];
    painter.add(egui::Shape::convex_polygon(t1.to_vec(), color, Stroke::NONE));
    
    let t2 = [Pos2::new(x + s * 1.2, cy - s), Pos2::new(x + s * 2.4, cy), Pos2::new(x + s * 1.2, cy + s)];
    painter.add(egui::Shape::convex_polygon(t2.to_vec(), color, Stroke::NONE));
}

fn draw_database_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.9)), 3.0, color);
    // Dividers
    painter.line_segment([Pos2::new(x, y + s * 0.3), Pos2::new(x + s, y + s * 0.3)], Stroke::new(0.5, dark));
    painter.line_segment([Pos2::new(x, y + s * 0.6), Pos2::new(x + s, y + s * 0.6)], Stroke::new(0.5, dark));
    // Dots
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.15), s * 0.08, Color32::WHITE);
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.45), s * 0.08, Color32::WHITE);
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.75), s * 0.08, Color32::WHITE);
}

fn draw_server_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.9)), 2.0, color);
    // Slots
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.1, y + s * 0.1), Vec2::new(s * 0.8, s * 0.2)), 1.0, dark);
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.1, y + s * 0.4), Vec2::new(s * 0.8, s * 0.2)), 1.0, dark);
    // LEDs
    painter.circle_filled(Pos2::new(x + s * 0.75, y + s * 0.2), s * 0.06, Color32::from_rgb(46, 204, 113));
    painter.circle_filled(Pos2::new(x + s * 0.75, y + s * 0.5), s * 0.06, Color32::from_rgb(46, 204, 113));
}

fn draw_serverscript_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.9)), 2.0, color);
    painter.text(
        Pos2::new(x + s * 0.5, y + s * 0.5),
        egui::Align2::CENTER_CENTER,
        "</>",
        egui::FontId::proportional(size * 0.35),
        Color32::WHITE,
    );
}

fn draw_window_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.9)), 2.0, color);
    // Title bar
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 0.2)), 2.0, dark);
    // Buttons
    painter.circle_filled(Pos2::new(x + s * 0.15, y + s * 0.1), s * 0.06, Color32::from_rgb(231, 76, 60));
    painter.circle_filled(Pos2::new(x + s * 0.3, y + s * 0.1), s * 0.06, Color32::from_rgb(241, 196, 15));
    painter.circle_filled(Pos2::new(x + s * 0.45, y + s * 0.1), s * 0.06, Color32::from_rgb(46, 204, 113));
}

fn draw_backpack_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32, dark: Color32) {
    let s = size * 0.7;
    let x = pos.x + size * 0.15;
    let y = pos.y + size * 0.2;
    
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s, s * 1.0)), 2.0, color);
    // Strap
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.3, y - s * 0.2), Vec2::new(s * 0.4, s * 0.25)), 1.0, dark);
    // Pocket
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.15, y + s * 0.4), Vec2::new(s * 0.7, s * 0.25)), 1.0, dark);
}

fn draw_starterplayer_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let s = size * 0.3;
    
    // Person
    painter.circle_filled(Pos2::new(cx - s * 0.3, cy - s * 0.3), s * 0.5, color);
    painter.add(egui::Shape::ellipse_filled(Pos2::new(cx - s * 0.3, cy + s * 0.8), Vec2::new(s * 0.7, s * 1.0), color));
    
    // Star
    let star_cx = cx + s * 0.8;
    let star_cy = cy - s * 0.5;
    let star_r = s * 0.5;
    let mut star_points = Vec::new();
    for i in 0..10 {
        let angle = (i as f32) * std::f32::consts::PI / 5.0 - std::f32::consts::PI / 2.0;
        let r = if i % 2 == 0 { star_r } else { star_r * 0.4 };
        star_points.push(Pos2::new(star_cx + angle.cos() * r, star_cy + angle.sin() * r));
    }
    painter.add(egui::Shape::convex_polygon(star_points, Color32::from_rgb(241, 196, 15), Stroke::NONE));
}

fn draw_music_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let x = pos.x + size * 0.25;
    let y = pos.y + size * 0.7;
    let s = size * 0.25;
    
    // Note head
    painter.circle_filled(Pos2::new(x, y), s, color);
    // Stem
    painter.rect_filled(Rect::from_min_size(Pos2::new(x + s * 0.7, y - s * 2.5), Vec2::new(s * 0.3, s * 2.5)), 0.0, color);
    // Flag
    let flag = [
        Pos2::new(x + s, y - s * 2.5),
        Pos2::new(x + s * 2.5, y - s * 1.5),
        Pos2::new(x + s * 2.5, y - s * 0.8),
        Pos2::new(x + s, y - s * 1.5),
    ];
    painter.add(egui::Shape::convex_polygon(flag.to_vec(), color, Stroke::NONE));
}

fn draw_flag_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let x = pos.x + size * 0.2;
    let y = pos.y + size * 0.1;
    let s = size * 0.7;
    
    // Pole
    painter.rect_filled(Rect::from_min_size(Pos2::new(x, y), Vec2::new(s * 0.1, s * 1.1)), 0.0, Color32::from_rgb(127, 140, 141));
    // Flag
    let flag = [
        Pos2::new(x + s * 0.1, y),
        Pos2::new(x + s, y),
        Pos2::new(x + s * 0.8, y + s * 0.3),
        Pos2::new(x + s, y + s * 0.6),
        Pos2::new(x + s * 0.1, y + s * 0.6),
    ];
    painter.add(egui::Shape::convex_polygon(flag.to_vec(), color, Stroke::NONE));
}

fn draw_chat_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let s = size * 0.8;
    let x = pos.x + size * 0.1;
    let y = pos.y + size * 0.1;
    
    // Bubble
    let bubble = [
        Pos2::new(x + s * 0.1, y),
        Pos2::new(x + s * 0.9, y),
        Pos2::new(x + s * 0.9, y + s * 0.6),
        Pos2::new(x + s * 0.4, y + s * 0.6),
        Pos2::new(x + s * 0.2, y + s * 0.9),
        Pos2::new(x + s * 0.2, y + s * 0.6),
        Pos2::new(x + s * 0.1, y + s * 0.6),
    ];
    painter.add(egui::Shape::convex_polygon(bubble.to_vec(), color, Stroke::NONE));
    
    // Dots
    painter.circle_filled(Pos2::new(x + s * 0.3, y + s * 0.3), s * 0.08, Color32::WHITE);
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.3), s * 0.08, Color32::WHITE);
    painter.circle_filled(Pos2::new(x + s * 0.7, y + s * 0.3), s * 0.08, Color32::WHITE);
}

fn draw_flask_icon(painter: &egui::Painter, pos: Pos2, size: f32, color: Color32) {
    let cx = pos.x + size / 2.0;
    let y = pos.y + size * 0.1;
    let s = size * 0.35;
    
    // Neck
    painter.rect_filled(Rect::from_min_size(Pos2::new(cx - s * 0.3, y), Vec2::new(s * 0.6, s * 0.6)), 0.0, color);
    // Body
    let body = [
        Pos2::new(cx - s * 0.3, y + s * 0.6),
        Pos2::new(cx + s * 0.3, y + s * 0.6),
        Pos2::new(cx + s * 1.0, y + s * 2.2),
        Pos2::new(cx - s * 1.0, y + s * 2.2),
    ];
    painter.add(egui::Shape::convex_polygon(body.to_vec(), color, Stroke::NONE));
    // Liquid
    let liquid = [
        Pos2::new(cx - s * 0.7, y + s * 1.5),
        Pos2::new(cx + s * 0.7, y + s * 1.5),
        Pos2::new(cx + s * 0.9, y + s * 2.1),
        Pos2::new(cx - s * 0.9, y + s * 2.1),
    ];
    painter.add(egui::Shape::convex_polygon(liquid.to_vec(), Color32::from_rgb(46, 204, 113), Stroke::NONE));
}

// ============================================================================
// Color Utilities
// ============================================================================

pub fn class_color(class_name: ClassName) -> Color32 {
    match class_name {
        ClassName::Part | ClassName::MeshPart | ClassName::BasePart | ClassName::PVInstance => {
            Color32::from_rgb(100, 150, 255)
        }
        ClassName::Model | ClassName::Folder => {
            Color32::from_rgb(255, 200, 100)
        }
        ClassName::Humanoid => {
            Color32::from_rgb(100, 255, 150)
        }
        ClassName::Camera => {
            Color32::from_rgb(100, 200, 255)
        }
        ClassName::PointLight | ClassName::SpotLight | ClassName::SurfaceLight | ClassName::DirectionalLight => {
            Color32::from_rgb(255, 255, 100)
        }
        ClassName::Attachment | ClassName::WeldConstraint | ClassName::Motor6D => {
            Color32::from_rgb(150, 150, 180)
        }
        ClassName::SpecialMesh | ClassName::UnionOperation => {
            Color32::from_rgb(200, 150, 255)
        }
        ClassName::Decal => {
            Color32::from_rgb(255, 150, 200)
        }
        ClassName::Animator | ClassName::KeyframeSequence => {
            Color32::from_rgb(255, 180, 150)
        }
        ClassName::ParticleEmitter | ClassName::Beam => {
            Color32::from_rgb(255, 100, 255)
        }
        ClassName::Sound => {
            Color32::from_rgb(150, 255, 100)
        }
        ClassName::Terrain | ClassName::Sky => {
            Color32::from_rgb(100, 200, 150)
        }
        ClassName::BillboardGui | ClassName::SurfaceGui | ClassName::ScreenGui |
        ClassName::TextLabel | ClassName::Frame | ClassName::ImageLabel |
        ClassName::TextButton | ClassName::ImageButton => {
            Color32::from_rgb(0, 200, 200)
        }
        ClassName::SoulScript => {
            Color32::from_rgb(255, 200, 50)  // Gold for Soul
        }
        ClassName::Lighting | ClassName::Atmosphere | ClassName::SpawnLocation | ClassName::Workspace => {
            Color32::from_rgb(255, 215, 0)
        }
        ClassName::Clouds => {
            Color32::from_rgb(200, 220, 255)
        }
        ClassName::Sun => {
            Color32::from_rgb(255, 230, 100)
        }
        ClassName::Moon => {
            Color32::from_rgb(200, 200, 220)
        }
        ClassName::Seat => {
            Color32::from_rgb(139, 90, 43) // Brown
        }
        ClassName::VehicleSeat => {
            Color32::from_rgb(70, 130, 180) // Steel blue
        }
        // Media Asset Classes
        ClassName::Document => {
            Color32::from_rgb(180, 80, 80) // Red-brown for documents
        }
        ClassName::ImageAsset => {
            Color32::from_rgb(100, 180, 100) // Green for images
        }
        ClassName::VideoAsset => {
            Color32::from_rgb(180, 100, 180) // Purple for videos
        }
        // ScrollingFrame
        ClassName::ScrollingFrame => {
            Color32::from_rgb(100, 149, 237) // Cornflower blue
        }
        // VideoFrame
        ClassName::VideoFrame => {
            Color32::from_rgb(220, 100, 180) // Pink-purple for video UI
        }
        // DocumentFrame
        ClassName::DocumentFrame => {
            Color32::from_rgb(200, 120, 80) // Orange-brown for document UI
        }
        // WebFrame
        ClassName::WebFrame => {
            Color32::from_rgb(70, 130, 180) // Steel blue for web content
        }
        // TextBox
        ClassName::TextBox => {
            Color32::from_rgb(100, 180, 100) // Green for input
        }
        // ViewportFrame
        ClassName::ViewportFrame => {
            Color32::from_rgb(150, 100, 200) // Purple for 3D viewport
        }
        // Team
        ClassName::Team => {
            Color32::from_rgb(255, 100, 100) // Red for teams
        }
        // Orbital classes
        ClassName::SolarSystem => {
            Color32::from_rgb(50, 50, 150) // Deep blue
        }
        ClassName::CelestialBody => {
            Color32::from_rgb(100, 80, 180) // Purple-blue
        }
        ClassName::RegionChunk => {
            Color32::from_rgb(80, 150, 100) // Earth green
        }
        ClassName::Instance => {
            Color32::GRAY
        }
    }
}

fn lighten_color(color: Color32, amount: f32) -> Color32 {
    Color32::from_rgb(
        (color.r() as f32 + (255.0 - color.r() as f32) * amount) as u8,
        (color.g() as f32 + (255.0 - color.g() as f32) * amount) as u8,
        (color.b() as f32 + (255.0 - color.b() as f32) * amount) as u8,
    )
}

fn darken_color(color: Color32, amount: f32) -> Color32 {
    Color32::from_rgb(
        (color.r() as f32 * (1.0 - amount)) as u8,
        (color.g() as f32 * (1.0 - amount)) as u8,
        (color.b() as f32 * (1.0 - amount)) as u8,
    )
}

// ============================================================================
// Terrain Brush Icons
// ============================================================================

/// Draw a terrain brush icon
pub fn draw_brush_icon(painter: &egui::Painter, pos: Pos2, brush_type: &str, size: f32) {
    match brush_type {
        "raise" | "add" => draw_brush_raise(painter, pos, size),
        "lower" | "subtract" | "remove" => draw_brush_lower(painter, pos, size),
        "smooth" => draw_brush_smooth(painter, pos, size),
        "flatten" => draw_brush_flatten(painter, pos, size),
        "paint" => draw_brush_paint(painter, pos, size),
        "erode" => draw_brush_erode(painter, pos, size),
        "grow" => draw_brush_grow(painter, pos, size),
        _ => draw_brush_raise(painter, pos, size),
    }
}

fn draw_brush_raise(painter: &egui::Painter, pos: Pos2, size: f32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.38;
    let color = Color32::from_rgb(46, 204, 113);
    
    painter.circle_stroke(Pos2::new(cx, cy), r, Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx, cy - r * 0.6), Pos2::new(cx, cy + r * 0.6)], Stroke::new(2.0, color));
    painter.line_segment([Pos2::new(cx - r * 0.6, cy), Pos2::new(cx + r * 0.6, cy)], Stroke::new(2.0, color));
}

fn draw_brush_lower(painter: &egui::Painter, pos: Pos2, size: f32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.38;
    let color = Color32::from_rgb(231, 76, 60);
    
    painter.circle_stroke(Pos2::new(cx, cy), r, Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx - r * 0.6, cy), Pos2::new(cx + r * 0.6, cy)], Stroke::new(2.0, color));
}

fn draw_brush_smooth(painter: &egui::Painter, pos: Pos2, size: f32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.38;
    let color = Color32::from_rgb(155, 89, 182);
    
    painter.circle_stroke(Pos2::new(cx, cy), r, Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150)));
    // Wave
    let points: Vec<Pos2> = (0..10).map(|i| {
        let t = (i as f32 / 9.0) * 2.0 - 1.0;
        let px = cx + t * r * 0.8;
        let py = cy + (t * std::f32::consts::PI).sin() * r * 0.3;
        Pos2::new(px, py)
    }).collect();
    painter.add(egui::Shape::line(points, Stroke::new(2.0, color)));
}

fn draw_brush_flatten(painter: &egui::Painter, pos: Pos2, size: f32) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.35;
    let color = Color32::from_rgb(243, 156, 18);
    
    painter.line_segment([Pos2::new(cx - r, cy), Pos2::new(cx + r, cy)], Stroke::new(3.0, color));
    painter.line_segment([Pos2::new(cx - r * 0.7, cy - r * 0.5), Pos2::new(cx - r * 0.7, cy + r * 0.5)], Stroke::new(1.0, color));
    painter.line_segment([Pos2::new(cx + r * 0.7, cy - r * 0.5), Pos2::new(cx + r * 0.7, cy + r * 0.5)], Stroke::new(1.0, color));
}

fn draw_brush_paint(painter: &egui::Painter, pos: Pos2, size: f32) {
    let x = pos.x + size * 0.2;
    let y = pos.y + size * 0.8;
    let s = size * 0.6;
    let color = Color32::from_rgb(52, 152, 219);
    
    // Brush handle
    let handle = [
        Pos2::new(x, y),
        Pos2::new(x + s * 0.3, y - s * 0.3),
        Pos2::new(x + s * 1.0, y - s * 1.0),
        Pos2::new(x + s * 0.7, y - s * 0.7),
    ];
    painter.add(egui::Shape::convex_polygon(handle.to_vec(), Color32::from_rgb(139, 90, 43), Stroke::NONE));
    // Brush tip
    painter.add(egui::Shape::ellipse_filled(
        Pos2::new(x + s * 1.1, y - s * 1.1),
        Vec2::new(s * 0.25, s * 0.15),
        color
    ));
}

fn draw_brush_erode(painter: &egui::Painter, pos: Pos2, size: f32) {
    let x = pos.x + size * 0.15;
    let y = pos.y + size * 0.8;
    let s = size * 0.7;
    let color = Color32::from_rgb(149, 165, 166);
    
    // Crumbling terrain line
    let points = [
        Pos2::new(x, y),
        Pos2::new(x + s * 0.2, y - s * 0.3),
        Pos2::new(x + s * 0.4, y - s * 0.15),
        Pos2::new(x + s * 0.6, y - s * 0.5),
        Pos2::new(x + s * 0.8, y - s * 0.35),
        Pos2::new(x + s, y - s * 0.6),
    ];
    painter.add(egui::Shape::line(points.to_vec(), Stroke::new(1.5, color)));
    // Falling particles
    painter.circle_filled(Pos2::new(x + s * 0.3, y + s * 0.1), size * 0.05, color);
    painter.circle_filled(Pos2::new(x + s * 0.5, y + s * 0.05), size * 0.04, color);
    painter.circle_filled(Pos2::new(x + s * 0.7, y + s * 0.08), size * 0.05, color);
}

fn draw_brush_grow(painter: &egui::Painter, pos: Pos2, size: f32) {
    let cx = pos.x + size / 2.0;
    let y = pos.y + size * 0.85;
    let s = size * 0.4;
    let color = Color32::from_rgb(39, 174, 96);
    
    // Stem
    painter.line_segment([Pos2::new(cx, y), Pos2::new(cx, y - s * 1.5)], Stroke::new(1.5, color));
    // Leaves
    let leaf1: Vec<Pos2> = vec![
        Pos2::new(cx, y - s * 1.5),
        Pos2::new(cx - s * 0.8, y - s * 2.0),
        Pos2::new(cx, y - s * 1.2),
    ];
    painter.add(egui::Shape::convex_polygon(leaf1, color, Stroke::NONE));
    let leaf2: Vec<Pos2> = vec![
        Pos2::new(cx, y - s * 1.0),
        Pos2::new(cx + s * 0.6, y - s * 1.4),
        Pos2::new(cx, y - s * 0.7),
    ];
    painter.add(egui::Shape::convex_polygon(leaf2, color, Stroke::NONE));
}

// ============================================================================
// Terrain Material Icons
// ============================================================================

/// Draw a terrain material icon
pub fn draw_material_icon(painter: &egui::Painter, pos: Pos2, material: &str, size: f32) {
    let rect = Rect::from_min_size(pos, Vec2::new(size, size));
    
    match material.to_lowercase().as_str() {
        "grass" => {
            let color = Color32::from_rgb(76, 175, 80);
            let dark = Color32::from_rgb(56, 142, 60);
            painter.rect_filled(rect, 2.0, color);
            // Grass blades
            for i in 0..3 {
                let x = pos.x + size * (0.2 + i as f32 * 0.3);
                painter.line_segment([
                    Pos2::new(x, pos.y + size),
                    Pos2::new(x, pos.y + size * 0.5)
                ], Stroke::new(1.5, dark));
            }
        }
        "rock" => {
            let color = Color32::from_rgb(127, 140, 141);
            let dark = Color32::from_rgb(93, 109, 126);
            painter.rect_filled(rect, 2.0, color);
            // Rock shapes
            let r1 = [
                Pos2::new(pos.x + size * 0.1, pos.y + size * 0.5),
                Pos2::new(pos.x + size * 0.3, pos.y + size * 0.2),
                Pos2::new(pos.x + size * 0.5, pos.y + size * 0.4),
                Pos2::new(pos.x + size * 0.4, pos.y + size * 0.6),
            ];
            painter.add(egui::Shape::convex_polygon(r1.to_vec(), dark, Stroke::NONE));
        }
        "dirt" => {
            let color = Color32::from_rgb(121, 85, 72);
            let dark = Color32::from_rgb(93, 64, 55);
            painter.rect_filled(rect, 2.0, color);
            // Dirt specks
            painter.circle_filled(Pos2::new(pos.x + size * 0.3, pos.y + size * 0.4), size * 0.08, dark);
            painter.circle_filled(Pos2::new(pos.x + size * 0.6, pos.y + size * 0.6), size * 0.06, dark);
            painter.circle_filled(Pos2::new(pos.x + size * 0.7, pos.y + size * 0.3), size * 0.07, dark);
        }
        "snow" => {
            let color = Color32::from_rgb(236, 240, 241);
            let dark = Color32::from_rgb(189, 195, 199);
            painter.rect_filled(rect, 2.0, color);
            // Snowflake hints
            painter.circle_filled(Pos2::new(pos.x + size * 0.25, pos.y + size * 0.3), size * 0.08, dark);
            painter.circle_filled(Pos2::new(pos.x + size * 0.6, pos.y + size * 0.5), size * 0.1, dark);
            painter.circle_filled(Pos2::new(pos.x + size * 0.4, pos.y + size * 0.7), size * 0.07, dark);
        }
        "sand" => {
            let color = Color32::from_rgb(244, 208, 63);
            let dark = Color32::from_rgb(212, 172, 13);
            painter.rect_filled(rect, 2.0, color);
            // Sand grains
            for i in 0..5 {
                let x = pos.x + size * (0.2 + (i as f32 * 0.15));
                let y = pos.y + size * (0.3 + ((i * 7) % 5) as f32 * 0.12);
                painter.circle_filled(Pos2::new(x, y), size * 0.04, dark);
            }
        }
        "mud" => {
            let color = Color32::from_rgb(109, 76, 65);
            let dark = Color32::from_rgb(78, 52, 46);
            painter.rect_filled(rect, 2.0, color);
            // Mud puddles
            painter.add(egui::Shape::ellipse_filled(
                Pos2::new(pos.x + size * 0.35, pos.y + size * 0.4),
                Vec2::new(size * 0.2, size * 0.1),
                dark
            ));
            painter.add(egui::Shape::ellipse_filled(
                Pos2::new(pos.x + size * 0.65, pos.y + size * 0.6),
                Vec2::new(size * 0.15, size * 0.08),
                dark
            ));
        }
        "concrete" => {
            let color = Color32::from_rgb(158, 158, 158);
            let dark = Color32::from_rgb(117, 117, 117);
            painter.rect_filled(rect, 2.0, color);
            // Grid lines
            painter.line_segment([Pos2::new(pos.x, pos.y + size * 0.33), Pos2::new(pos.x + size, pos.y + size * 0.33)], Stroke::new(0.5, dark));
            painter.line_segment([Pos2::new(pos.x, pos.y + size * 0.66), Pos2::new(pos.x + size, pos.y + size * 0.66)], Stroke::new(0.5, dark));
            painter.line_segment([Pos2::new(pos.x + size * 0.5, pos.y), Pos2::new(pos.x + size * 0.5, pos.y + size * 0.33)], Stroke::new(0.5, dark));
            painter.line_segment([Pos2::new(pos.x + size * 0.25, pos.y + size * 0.33), Pos2::new(pos.x + size * 0.25, pos.y + size * 0.66)], Stroke::new(0.5, dark));
            painter.line_segment([Pos2::new(pos.x + size * 0.75, pos.y + size * 0.33), Pos2::new(pos.x + size * 0.75, pos.y + size * 0.66)], Stroke::new(0.5, dark));
        }
        "asphalt" => {
            let color = Color32::from_rgb(51, 51, 56);
            let light = Color32::from_rgb(80, 80, 85);
            painter.rect_filled(rect, 2.0, color);
            // Aggregate specks
            painter.circle_filled(Pos2::new(pos.x + size * 0.2, pos.y + size * 0.3), size * 0.04, light);
            painter.circle_filled(Pos2::new(pos.x + size * 0.5, pos.y + size * 0.5), size * 0.03, light);
            painter.circle_filled(Pos2::new(pos.x + size * 0.7, pos.y + size * 0.4), size * 0.04, light);
            painter.circle_filled(Pos2::new(pos.x + size * 0.4, pos.y + size * 0.7), size * 0.03, light);
        }
        "water" => {
            let color = Color32::from_rgb(52, 152, 219);
            let light = Color32::from_rgb(93, 173, 226);
            painter.rect_filled(rect, 2.0, color);
            // Waves
            let wave1: Vec<Pos2> = (0..5).map(|i| {
                let t = i as f32 / 4.0;
                Pos2::new(pos.x + t * size, pos.y + size * 0.4 + (t * std::f32::consts::PI * 2.0).sin() * size * 0.1)
            }).collect();
            painter.add(egui::Shape::line(wave1, Stroke::new(1.0, light)));
        }
        "lava" => {
            let color = Color32::from_rgb(230, 126, 34);
            let bright = Color32::from_rgb(241, 196, 15);
            painter.rect_filled(rect, 2.0, color);
            // Glowing spots
            painter.circle_filled(Pos2::new(pos.x + size * 0.3, pos.y + size * 0.4), size * 0.12, bright);
            painter.circle_filled(Pos2::new(pos.x + size * 0.65, pos.y + size * 0.6), size * 0.1, bright);
        }
        "basalt" => {
            let color = Color32::from_rgb(44, 62, 80);
            let light = Color32::from_rgb(52, 73, 94);
            painter.rect_filled(rect, 2.0, color);
            // Columnar pattern
            for i in 0..3 {
                let x = pos.x + size * (0.2 + i as f32 * 0.3);
                painter.line_segment([Pos2::new(x, pos.y), Pos2::new(x, pos.y + size)], Stroke::new(1.0, light));
            }
        }
        _ => {
            // Default gray
            painter.rect_filled(rect, 2.0, Color32::GRAY);
        }
    }
}

// ============================================================================
// Terrain Brush Tool Icons (Material Design Style)
// ============================================================================

/// Draw a Raise brush icon (arrow pointing up from terrain)
pub fn draw_brush_raise_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(76, 175, 80) } else { Color32::from_rgb(150, 150, 150) };
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Mountain base
    let base_y = pos.y + size * 0.85;
    painter.line_segment(
        [Pos2::new(pos.x + size * 0.1, base_y), Pos2::new(pos.x + size * 0.9, base_y)],
        Stroke::new(1.5, color)
    );
    
    // Upward arrow
    let arrow_points = [
        Pos2::new(cx, pos.y + size * 0.15),
        Pos2::new(cx - size * 0.25, pos.y + size * 0.45),
        Pos2::new(cx - size * 0.1, pos.y + size * 0.45),
        Pos2::new(cx - size * 0.1, pos.y + size * 0.7),
        Pos2::new(cx + size * 0.1, pos.y + size * 0.7),
        Pos2::new(cx + size * 0.1, pos.y + size * 0.45),
        Pos2::new(cx + size * 0.25, pos.y + size * 0.45),
    ];
    painter.add(egui::Shape::convex_polygon(arrow_points.to_vec(), color, Stroke::NONE));
}

/// Draw a Lower brush icon (arrow pointing down into terrain)
pub fn draw_brush_lower_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(244, 67, 54) } else { Color32::from_rgb(150, 150, 150) };
    let cx = pos.x + size / 2.0;
    
    // Ground line at top
    let top_y = pos.y + size * 0.15;
    painter.line_segment(
        [Pos2::new(pos.x + size * 0.1, top_y), Pos2::new(pos.x + size * 0.9, top_y)],
        Stroke::new(1.5, color)
    );
    
    // Downward arrow
    let arrow_points = [
        Pos2::new(cx, pos.y + size * 0.85),
        Pos2::new(cx - size * 0.25, pos.y + size * 0.55),
        Pos2::new(cx - size * 0.1, pos.y + size * 0.55),
        Pos2::new(cx - size * 0.1, pos.y + size * 0.3),
        Pos2::new(cx + size * 0.1, pos.y + size * 0.3),
        Pos2::new(cx + size * 0.1, pos.y + size * 0.55),
        Pos2::new(cx + size * 0.25, pos.y + size * 0.55),
    ];
    painter.add(egui::Shape::convex_polygon(arrow_points.to_vec(), color, Stroke::NONE));
}

/// Draw a Smooth brush icon (wave/blur effect)
pub fn draw_brush_smooth_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(33, 150, 243) } else { Color32::from_rgb(150, 150, 150) };
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Draw smooth wave lines
    for i in 0..3 {
        let y_offset = (i as f32 - 1.0) * size * 0.25;
        let y = cy + y_offset;
        
        // Sine wave approximation with bezier
        let start = Pos2::new(pos.x + size * 0.1, y);
        let end = Pos2::new(pos.x + size * 0.9, y);
        let mid1 = Pos2::new(pos.x + size * 0.35, y - size * 0.1);
        let mid2 = Pos2::new(pos.x + size * 0.65, y + size * 0.1);
        
        painter.line_segment([start, mid1], Stroke::new(1.5, color));
        painter.line_segment([mid1, Pos2::new(cx, y)], Stroke::new(1.5, color));
        painter.line_segment([Pos2::new(cx, y), mid2], Stroke::new(1.5, color));
        painter.line_segment([mid2, end], Stroke::new(1.5, color));
    }
}

/// Draw a Flatten brush icon (horizontal line with arrows)
pub fn draw_brush_flatten_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(255, 193, 7) } else { Color32::from_rgb(150, 150, 150) };
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Horizontal flatten line
    painter.line_segment(
        [Pos2::new(pos.x + size * 0.1, cy), Pos2::new(pos.x + size * 0.9, cy)],
        Stroke::new(2.0, color)
    );
    
    // Down arrow on left
    painter.line_segment([Pos2::new(pos.x + size * 0.25, pos.y + size * 0.25), Pos2::new(pos.x + size * 0.25, cy - size * 0.05)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(pos.x + size * 0.15, cy - size * 0.15), Pos2::new(pos.x + size * 0.25, cy - size * 0.05)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(pos.x + size * 0.35, cy - size * 0.15), Pos2::new(pos.x + size * 0.25, cy - size * 0.05)], Stroke::new(1.5, color));
    
    // Up arrow on right
    painter.line_segment([Pos2::new(pos.x + size * 0.75, pos.y + size * 0.75), Pos2::new(pos.x + size * 0.75, cy + size * 0.05)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(pos.x + size * 0.65, cy + size * 0.15), Pos2::new(pos.x + size * 0.75, cy + size * 0.05)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(pos.x + size * 0.85, cy + size * 0.15), Pos2::new(pos.x + size * 0.75, cy + size * 0.05)], Stroke::new(1.5, color));
}

/// Draw a Paint brush icon (brush with color)
pub fn draw_brush_paint_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(156, 39, 176) } else { Color32::from_rgb(150, 150, 150) };
    
    // Brush handle
    let handle_points = [
        Pos2::new(pos.x + size * 0.7, pos.y + size * 0.1),
        Pos2::new(pos.x + size * 0.9, pos.y + size * 0.3),
        Pos2::new(pos.x + size * 0.5, pos.y + size * 0.7),
        Pos2::new(pos.x + size * 0.3, pos.y + size * 0.5),
    ];
    painter.add(egui::Shape::convex_polygon(handle_points.to_vec(), color, Stroke::NONE));
    
    // Brush tip
    let tip_points = [
        Pos2::new(pos.x + size * 0.3, pos.y + size * 0.5),
        Pos2::new(pos.x + size * 0.5, pos.y + size * 0.7),
        Pos2::new(pos.x + size * 0.2, pos.y + size * 0.9),
        Pos2::new(pos.x + size * 0.1, pos.y + size * 0.7),
    ];
    let tip_color = if selected { Color32::from_rgb(233, 30, 99) } else { Color32::from_rgb(120, 120, 120) };
    painter.add(egui::Shape::convex_polygon(tip_points.to_vec(), tip_color, Stroke::NONE));
}

/// Draw a Voxel Add icon (plus in cube)
pub fn draw_brush_voxel_add_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(0, 150, 136) } else { Color32::from_rgb(150, 150, 150) };
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Cube outline
    let cube_size = size * 0.7;
    let half = cube_size / 2.0;
    painter.rect_stroke(
        Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(cube_size, cube_size)),
        2.0, Stroke::new(1.5, color), egui::StrokeKind::Inside
    );
    
    // Plus sign
    painter.line_segment([Pos2::new(cx - half * 0.5, cy), Pos2::new(cx + half * 0.5, cy)], Stroke::new(2.0, color));
    painter.line_segment([Pos2::new(cx, cy - half * 0.5), Pos2::new(cx, cy + half * 0.5)], Stroke::new(2.0, color));
}

/// Draw a Voxel Remove icon (minus in cube)
pub fn draw_brush_voxel_remove_icon(painter: &egui::Painter, pos: Pos2, size: f32, selected: bool) {
    let color = if selected { Color32::from_rgb(255, 87, 34) } else { Color32::from_rgb(150, 150, 150) };
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Cube outline
    let cube_size = size * 0.7;
    let half = cube_size / 2.0;
    painter.rect_stroke(
        Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(cube_size, cube_size)),
        2.0, Stroke::new(1.5, color), egui::StrokeKind::Inside
    );
    
    // Minus sign
    painter.line_segment([Pos2::new(cx - half * 0.5, cy), Pos2::new(cx + half * 0.5, cy)], Stroke::new(2.0, color));
}

// ============================================================================
// File Menu Icons (Google Material Design Style)
// ============================================================================

/// Draw a New Scene icon (document with plus)
pub fn draw_new_scene_icon(painter: &egui::Painter, pos: Pos2, size: f32) {
    let color = Color32::from_rgb(100, 180, 100); // Green for new
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Document outline
    let doc_w = size * 0.5;
    let doc_h = size * 0.65;
    let corner = size * 0.12;
    
    // Document body with folded corner
    let points = [
        Pos2::new(cx - doc_w / 2.0, cy - doc_h / 2.0),
        Pos2::new(cx + doc_w / 2.0 - corner, cy - doc_h / 2.0),
        Pos2::new(cx + doc_w / 2.0, cy - doc_h / 2.0 + corner),
        Pos2::new(cx + doc_w / 2.0, cy + doc_h / 2.0),
        Pos2::new(cx - doc_w / 2.0, cy + doc_h / 2.0),
    ];
    painter.add(egui::Shape::convex_polygon(points.to_vec(), Color32::from_rgb(60, 60, 70), Stroke::new(1.0, color)));
    
    // Plus sign
    let plus_size = size * 0.2;
    painter.line_segment([Pos2::new(cx - plus_size, cy), Pos2::new(cx + plus_size, cy)], Stroke::new(1.5, color));
    painter.line_segment([Pos2::new(cx, cy - plus_size), Pos2::new(cx, cy + plus_size)], Stroke::new(1.5, color));
}

/// Draw an Open Scene icon (folder with arrow)
pub fn draw_open_scene_icon(painter: &egui::Painter, pos: Pos2, size: f32) {
    let color = Color32::from_rgb(66, 165, 245); // Blue for open
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Folder shape
    let folder_w = size * 0.6;
    let folder_h = size * 0.45;
    let tab_w = size * 0.2;
    let tab_h = size * 0.1;
    
    // Folder body
    let folder_top = cy - folder_h / 2.0;
    let folder_left = cx - folder_w / 2.0;
    
    // Tab on folder
    let tab_points = [
        Pos2::new(folder_left, folder_top),
        Pos2::new(folder_left + tab_w, folder_top),
        Pos2::new(folder_left + tab_w + tab_h, folder_top - tab_h),
        Pos2::new(folder_left, folder_top - tab_h),
    ];
    painter.add(egui::Shape::convex_polygon(tab_points.to_vec(), color, Stroke::NONE));
    
    // Folder body
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(folder_left, folder_top), Vec2::new(folder_w, folder_h)),
        2.0, color
    );
    
    // Up arrow inside
    let arrow_size = size * 0.15;
    let arrow_y = cy + size * 0.05;
    painter.line_segment([Pos2::new(cx, arrow_y - arrow_size), Pos2::new(cx, arrow_y + arrow_size * 0.5)], Stroke::new(1.5, Color32::WHITE));
    painter.line_segment([Pos2::new(cx - arrow_size * 0.6, arrow_y - arrow_size * 0.3), Pos2::new(cx, arrow_y - arrow_size)], Stroke::new(1.5, Color32::WHITE));
    painter.line_segment([Pos2::new(cx + arrow_size * 0.6, arrow_y - arrow_size * 0.3), Pos2::new(cx, arrow_y - arrow_size)], Stroke::new(1.5, Color32::WHITE));
}

/// Draw a Save Scene icon (floppy disk)
pub fn draw_save_scene_icon(painter: &egui::Painter, pos: Pos2, size: f32) {
    let color = Color32::from_rgb(76, 175, 80); // Green for save
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Floppy disk body
    let disk_size = size * 0.6;
    let corner = size * 0.1;
    
    // Main body with cut corner
    let points = [
        Pos2::new(cx - disk_size / 2.0, cy - disk_size / 2.0),
        Pos2::new(cx + disk_size / 2.0 - corner, cy - disk_size / 2.0),
        Pos2::new(cx + disk_size / 2.0, cy - disk_size / 2.0 + corner),
        Pos2::new(cx + disk_size / 2.0, cy + disk_size / 2.0),
        Pos2::new(cx - disk_size / 2.0, cy + disk_size / 2.0),
    ];
    painter.add(egui::Shape::convex_polygon(points.to_vec(), color, Stroke::NONE));
    
    // Label area (white rectangle at top)
    let label_w = disk_size * 0.6;
    let label_h = disk_size * 0.25;
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(cx, cy - disk_size * 0.2), Vec2::new(label_w, label_h)),
        1.0, Color32::WHITE
    );
    
    // Metal slider (dark rectangle at bottom)
    let slider_w = disk_size * 0.35;
    let slider_h = disk_size * 0.3;
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(cx, cy + disk_size * 0.2), Vec2::new(slider_w, slider_h)),
        1.0, Color32::from_rgb(40, 40, 50)
    );
}

/// Draw a Save Scene As icon (floppy disk with pencil)
pub fn draw_save_scene_as_icon(painter: &egui::Painter, pos: Pos2, size: f32) {
    let color = Color32::from_rgb(255, 193, 7); // Amber for save as
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    
    // Smaller floppy disk (offset to make room for pencil)
    let disk_size = size * 0.5;
    let disk_cx = cx - size * 0.08;
    let disk_cy = cy + size * 0.05;
    
    // Main body
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(disk_cx, disk_cy), Vec2::new(disk_size, disk_size)),
        2.0, color
    );
    
    // Label area
    let label_w = disk_size * 0.6;
    let label_h = disk_size * 0.2;
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(disk_cx, disk_cy - disk_size * 0.2), Vec2::new(label_w, label_h)),
        1.0, Color32::WHITE
    );
    
    // Pencil overlay (top right)
    let pencil_len = size * 0.35;
    let pencil_start = Pos2::new(cx + size * 0.15, cy - size * 0.25);
    let pencil_end = Pos2::new(cx + size * 0.35, cy - size * 0.45);
    painter.line_segment([pencil_start, pencil_end], Stroke::new(2.5, Color32::from_rgb(100, 100, 100)));
    // Pencil tip
    painter.line_segment([pencil_start, Pos2::new(pencil_start.x - size * 0.05, pencil_start.y + size * 0.05)], Stroke::new(2.0, Color32::from_rgb(255, 200, 100)));
}

// ============================================================================
// Brush Shape Icons
// ============================================================================

/// Draw a brush shape icon
pub fn draw_brush_shape_icon(painter: &egui::Painter, pos: Pos2, shape: &str, size: f32, selected: bool) {
    let cx = pos.x + size / 2.0;
    let cy = pos.y + size / 2.0;
    let r = size * 0.35;
    let color = if selected { Color32::from_rgb(52, 152, 219) } else { Color32::from_rgb(150, 150, 150) };
    
    match shape.to_lowercase().as_str() {
        "circle" => {
            painter.circle_filled(Pos2::new(cx, cy), r, color);
        }
        "square" => {
            painter.rect_filled(
                Rect::from_center_size(Pos2::new(cx, cy), Vec2::new(r * 2.0, r * 2.0)),
                0.0, color
            );
        }
        "diamond" => {
            let points = [
                Pos2::new(cx, cy - r),
                Pos2::new(cx + r, cy),
                Pos2::new(cx, cy + r),
                Pos2::new(cx - r, cy),
            ];
            painter.add(egui::Shape::convex_polygon(points.to_vec(), color, Stroke::NONE));
        }
        _ => {
            painter.circle_filled(Pos2::new(cx, cy), r, color);
        }
    }
}
