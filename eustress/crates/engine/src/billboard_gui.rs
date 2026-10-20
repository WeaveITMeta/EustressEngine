//! Billboard authoring in Studio: hiding the bevy_ui nodes the edit-mode
//! spawners give billboard GUI, mirroring class edits into the drawn
//! elements, saving them back to TOML, and editing a label in the viewport.
//! Drawing is `eustress_play_runtime::billboards`, shared with the Player.

use bevy::prelude::*;
use eustress_common::classes::BillboardGui;
use eustress_common::gui::billboard_renderer::{BillboardGuiMarker, GuiElementDisplay};

pub use eustress_play_runtime::billboards::*;

/// Stand `bevy_ui` down for every GUI element owned by a BillboardGui.
///
/// GUI leaves (TextLabel / Frame / TextButton / …) are spawned with a
/// `bevy::ui::Node` unconditionally, because the SAME class also serves
/// screen-space ScreenGui content. Under a billboard that is one renderer too
/// many: this module already rasterises the subtree into the shared atlas and
/// maps it onto the 3D quad, while `bevy_ui` independently lays the very same
/// `Node` out in SCREEN space (`position_type: Absolute`) and draws it pinned
/// to the viewport — the "2D overlay that follows me everywhere" on top of the
/// correct 3D label.
///
/// `Display::None` (rather than `Visibility::Hidden`) is deliberate: it takes
/// the node out of `bevy_ui`'s layout entirely, so it costs nothing and cannot
/// contribute to a parent's computed size. The atlas renderer reads
/// `GuiElementDisplay`, never `Node`, so the 3D billboard is unaffected.
///
/// Runs on ADDED nodes and on re-parent (`Changed<ChildOf>`), so an element
/// dragged out of a billboard and into a ScreenGui becomes visible again.
fn hide_billboard_owned_ui_nodes(
    mut nodes: Query<
        (Entity, &mut bevy::ui::Node),
        (
            With<eustress_common::gui::billboard_renderer::GuiElementDisplay>,
            Or<(Added<bevy::ui::Node>, Changed<ChildOf>)>,
        ),
    >,
    billboard_q: Query<(), With<BillboardGuiMarker>>,
    parent_q: Query<&ChildOf>,
) {
    for (entity, mut node) in &mut nodes {
        // Walk to the nearest billboard ancestor, if any.
        let mut cur = entity;
        let mut under_billboard = false;
        loop {
            if billboard_q.get(cur).is_ok() {
                under_billboard = true;
                break;
            }
            match parent_q.get(cur) {
                Ok(p) => cur = p.parent(),
                Err(_) => break,
            }
        }

        let want = if under_billboard {
            bevy::ui::Display::None
        } else {
            bevy::ui::Display::Flex
        };
        if node.display != want {
            node.display = want;
        }
    }
}

fn sync_textlabel_to_display(
    mut q: Query<(&eustress_common::classes::TextLabel, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::TextLabel>>,
) {
    use eustress_common::classes::{Font, TextXAlignment, TextYAlignment};
    for (tl, mut gui) in &mut q {
        gui.text = tl.text.clone();
        gui.text_color = [tl.text_color3[0], tl.text_color3[1], tl.text_color3[2],
                          (1.0 - tl.text_transparency).clamp(0.0, 1.0)];
        gui.font_size = tl.font_size.max(1.0);
        gui.font_weight = match tl.font {
            Font::GothamBold => 700,
            Font::GothamLight => 300,
            _ => 400,
        };
        gui.text_align = match tl.text_x_alignment {
            TextXAlignment::Left => "Left".to_string(),
            TextXAlignment::Center => "Center".to_string(),
            TextXAlignment::Right => "Right".to_string(),
        };
        gui.text_y_align = match tl.text_y_alignment {
            TextYAlignment::Top => "Top".to_string(),
            TextYAlignment::Center => "Center".to_string(),
            TextYAlignment::Bottom => "Bottom".to_string(),
        };
        // Text stroke (halo) — alpha derived from transparency. Zero
        // alpha = renderer skips the stroke pass entirely.
        gui.text_stroke_color = [
            tl.text_stroke_color3[0],
            tl.text_stroke_color3[1],
            tl.text_stroke_color3[2],
            (1.0 - tl.text_stroke_transparency).clamp(0.0, 1.0),
        ];
        gui.text_scaled = tl.text_scaled;
        gui.bg_color = [tl.background_color3[0], tl.background_color3[1], tl.background_color3[2],
                        (1.0 - tl.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [tl.border_color3[0], tl.border_color3[1], tl.border_color3[2], 1.0];
        gui.border_size = tl.border_size_pixel as f32;
        gui.visible = tl.visible;
        gui.z_order = tl.z_index;
        gui.anchor_point = tl.anchor_point;
        // Store BOTH the source UDim2 AND a best-effort Offset-only
        // resolved rect. `collect_subtree` re-resolves Scale at render
        // time using the parent billboard's canvas size — that's where
        // `Size = (1, 0, 1, 0)` becomes "fill the parent". The Offset
        // here keeps non-parented previews looking sane.
        gui.position_udim2 = [
            tl.position.x.scale, tl.position.x.offset,
            tl.position.y.scale, tl.position.y.offset,
        ];
        gui.size_udim2 = [
            tl.size.x.scale, tl.size.x.offset,
            tl.size.y.scale, tl.size.y.offset,
        ];
        gui.x = tl.position.x.offset;
        gui.y = tl.position.y.offset;
        gui.width = tl.size.x.offset.max(1.0);
        gui.height = tl.size.y.offset.max(1.0);
    }
}

fn sync_frame_to_display(
    mut q: Query<(&eustress_common::classes::Frame, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::Frame>>,
) {
    for (f, mut gui) in &mut q {
        gui.bg_color = [f.background_color3[0], f.background_color3[1], f.background_color3[2],
                        (1.0 - f.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [f.border_color3[0], f.border_color3[1], f.border_color3[2], 1.0];
        gui.border_size = f.border_size_pixel as f32;
        gui.visible = f.visible;
        gui.z_order = f.z_index;
        gui.clip_children = f.clips_descendants;
        gui.anchor_point = f.anchor_point;
        gui.position_udim2 = [
            f.position.x.scale, f.position.x.offset,
            f.position.y.scale, f.position.y.offset,
        ];
        gui.size_udim2 = [
            f.size.x.scale, f.size.x.offset,
            f.size.y.scale, f.size.y.offset,
        ];
        gui.x = f.position.x.offset;
        gui.y = f.position.y.offset;
        gui.width = f.size.x.offset.max(1.0);
        gui.height = f.size.y.offset.max(1.0);
    }
}

fn sync_textbutton_to_display(
    mut q: Query<(&eustress_common::classes::TextButton, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::TextButton>>,
) {
    use eustress_common::classes::TextXAlignment;
    for (b, mut gui) in &mut q {
        gui.text = b.text.clone();
        gui.text_color = [b.text_color3[0], b.text_color3[1], b.text_color3[2],
                          (1.0 - b.text_transparency).clamp(0.0, 1.0)];
        gui.font_size = b.font_size.max(1.0);
        // TextButton has no `font` family field — default to regular
        // weight (400). Users get weighted variants through TextLabel
        // siblings or via font_family overrides at the TOML layer.
        gui.font_weight = 400;
        gui.text_align = match b.text_x_alignment {
            TextXAlignment::Left => "Left".to_string(),
            TextXAlignment::Center => "Center".to_string(),
            TextXAlignment::Right => "Right".to_string(),
        };
        gui.text_y_align = "Center".to_string();
        gui.bg_color = [b.background_color3[0], b.background_color3[1], b.background_color3[2],
                        (1.0 - b.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [b.border_color3[0], b.border_color3[1], b.border_color3[2], 1.0];
        gui.border_size = b.border_size_pixel as f32;
        gui.visible = b.visible;
        gui.z_order = b.z_index;
        gui.anchor_point = b.anchor_point;
        gui.position_udim2 = [
            b.position.x.scale, b.position.x.offset,
            b.position.y.scale, b.position.y.offset,
        ];
        gui.size_udim2 = [
            b.size.x.scale, b.size.x.offset,
            b.size.y.scale, b.size.y.offset,
        ];
        gui.x = b.position.x.offset;
        gui.y = b.position.y.offset;
        gui.width = b.size.x.offset.max(1.0);
        gui.height = b.size.y.offset.max(1.0);
    }
}

// ── TOML persistence (one save-on-change system per UI class) ─────────────
//
// Each UI class component is the authoritative state — `Changed<T>` fires
// whenever the Properties panel, a script, MCP, or hot-reload mutates it.
// These systems write the corresponding GuiTomlFile back to disk so the
// next session sees the change. Skips:
//
// - `Added<T>` — initial spawn fires Changed for every freshly inserted
//   component. Without this skip every loaded scene would queue thousands
//   of write-amplification round-trips on the first frame.
// - `Without<BeingDragged>` — defers writes during gizmo manipulation.
//   The gizmo's own release branch writes the final transform; we don't
//   need a duplicate write here. (UI elements aren't gizmo-dragged today,
//   but the marker is harmless to filter for.)
// - `recently_written` — if this process just touched the TOML, skip
//   another write to prevent the file-watcher reload loop.

/// Helper: pour a `BillboardGui` class component into an existing
/// `GuiTomlFile`. Preserves the existing `text` / `asset` / `transform`
/// / `properties` / `tags` sections (those aren't BillboardGui state).
fn apply_billboard_gui_to_toml(
    class: &BillboardGui,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    use eustress_common::classes::ZIndexBehavior;
    // Stage-4 disk normalisation: read the file's authored unit (from
    // `[metadata].unit`) and convert engine-native length-typed fields
    // back to that unit before writing. `extents_offset*` is a part-
    // size multiplier (ratio, not length) and so passes through
    // unconverted, matching the load-side rule.
    let authored = toml.metadata.unit.as_deref()
        .and_then(eustress_common::units::Unit::from_symbol)
        .unwrap_or(eustress_common::units::ENGINE_NATIVE_UNIT);
    let to_authored_vec3 = |v: [f32; 3]| eustress_common::units::engine_to_authored_vec3_f32(v, authored);
    let to_authored_f32  = |v: f32|       eustress_common::units::engine_to_authored_f32(v, authored);

    toml.gui.size = class.size;
    toml.gui.size_offset = Some(class.size_offset);
    toml.gui.active = Some(class.active);
    toml.gui.enabled = Some(class.enabled);
    toml.gui.always_on_top = Some(class.always_on_top);
    toml.gui.clips_descendants = Some(class.clips_descendants);
    toml.gui.reset_on_spawn = Some(class.reset_on_spawn);
    toml.gui.stiffness_by_distance = Some(class.stiffness_by_distance);
    toml.gui.max_distance = Some(to_authored_f32(class.max_distance));
    toml.gui.distance_lower_limit = Some(to_authored_f32(class.distance_lower_limit));
    toml.gui.distance_upper_limit = Some(to_authored_f32(class.distance_upper_limit));
    toml.gui.distance_step = Some(to_authored_f32(class.distance_step));
    toml.gui.brightness = Some(class.brightness);
    toml.gui.light_influence = Some(class.light_influence);
    toml.gui.extents_offset = Some(class.extents_offset);
    toml.gui.extents_offset_world_space = Some(class.extents_offset_world_space);
    toml.gui.units_offset = Some(to_authored_vec3(class.units_offset));
    toml.gui.units_offset_world_space = Some(to_authored_vec3(class.units_offset_world_space));
    // ZIndex on BillboardGui is a depth bias (per-billboard, integer),
    // not the GuiObject sort-order ZIndex used by Frame/TextLabel. Map
    // it into the same TOML `z_index` slot — there's no ambiguity per
    // file since each `_instance.toml` belongs to exactly one class.
    toml.gui.z_index = class.z_index;
    toml.gui.z_index_behavior = Some(match class.z_index_behavior {
        ZIndexBehavior::Global => "Global".to_string(),
        ZIndexBehavior::Sibling => "Sibling".to_string(),
    });
}

fn save_billboard_gui_changes(
    q: Query<
        (Entity, &BillboardGui, &crate::space::instance_loader::InstanceFile),
        (
            Changed<BillboardGui>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<BillboardGui>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    // Synchronous TOML I/O. We tried background-thread writes; they
    // introduce a copy-paste race — copy_dir_recursive reads disk, so
    // if the user edits a property then immediately Ctrl+C the source
    // TOML is still mid-flight, and the duplicate inherits the
    // pre-edit content. Inline writes keep the on-disk state in lock-
    // step with the ECS at the cost of a few ms per discrete user
    // commit, which is below the perception threshold.
    //
    // The `was_recently_written` SKIP-on-save check stays removed —
    // it dropped rapid edits because Bevy's `Changed<T>` resets the
    // moment this system iterates. We only `mark_written` AFTER the
    // write to break the save → file-watcher → save loop.
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t,
            Err(e) => {
                debug!("🪧 save_billboard_gui: skip {} ({})", inst_file.toml_path.display(), e);
                continue;
            }
        };
        apply_billboard_gui_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_billboard_gui: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

/// Mirror Roblox-parity TextLabel state into both the `gui` and `text`
/// sections of `GuiTomlFile`. `text` holds the text-specific subset
/// (string, color, font, alignment); `gui` holds the layout subset
/// (size/position UDim2, anchor, visibility, z_index).
fn apply_text_label_to_toml(
    class: &eustress_common::classes::TextLabel,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    use eustress_common::classes::{Font, TextXAlignment, TextYAlignment};
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;

    let font_name = match class.font {
        Font::GothamBold => "GothamBold",
        Font::GothamLight => "GothamLight",
        Font::RobotoMono => "RobotoMono",
        Font::Bangers => "Bangers",
        Font::Fantasy => "Fantasy",
        Font::Merriweather => "Merriweather",
        Font::Nunito => "Nunito",
        Font::Ubuntu => "Ubuntu",
        _ => "SourceSans",
    };
    let x_align = match class.text_x_alignment {
        TextXAlignment::Left => "Left",
        TextXAlignment::Center => "Center",
        TextXAlignment::Right => "Right",
    };
    let y_align = match class.text_y_alignment {
        TextYAlignment::Top => "Top",
        TextYAlignment::Center => "Center",
        TextYAlignment::Bottom => "Bottom",
    };
    toml.text = Some(crate::space::gui_loader::GuiTomlText {
        text: class.text.clone(),
        text_color: [
            class.text_color3[0],
            class.text_color3[1],
            class.text_color3[2],
            (1.0 - class.text_transparency).clamp(0.0, 1.0),
        ],
        font_size: class.font_size,
        font_family: String::new(),
        font: font_name.to_string(),
        text_x_alignment: x_align.to_string(),
        text_y_alignment: y_align.to_string(),
        text_scaled: class.text_scaled,
        // Compile scaffold: the struct's serde-default values (opaque text, no
        // stroke) so the tree builds. Real stroke round-trip is the co-agent's
        // in-flight text-stroke feature — left for them to wire from `class`.
        text_transparency: 0.0,
        text_stroke_color: [0.0, 0.0, 0.0, 1.0],
        text_stroke_transparency: 1.0,
    });
}

fn save_text_label_changes(
    q: Query<
        (Entity, &eustress_common::classes::TextLabel, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::TextLabel>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::TextLabel>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    // Synchronous I/O — see save_billboard_gui_changes for the rationale.
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t,
            Err(e) => {
                debug!("🪧 save_text_label: skip {} ({})", inst_file.toml_path.display(), e);
                continue;
            }
        };
        apply_text_label_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_text_label: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        info!("💾 save_text_label: text={:?} font_size={} z_index={} → {}",
            class.text, class.font_size, class.z_index, inst_file.toml_path.display());
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn apply_frame_to_toml(
    class: &eustress_common::classes::Frame,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;
    toml.gui.clips_descendants = Some(class.clips_descendants);
}

fn save_frame_changes(
    q: Query<
        (Entity, &eustress_common::classes::Frame, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::Frame>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::Frame>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t, Err(_) => continue,
        };
        apply_frame_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_frame: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn apply_text_button_to_toml(
    class: &eustress_common::classes::TextButton,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    use eustress_common::classes::TextXAlignment;
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;
    let x_align = match class.text_x_alignment {
        TextXAlignment::Left => "Left",
        TextXAlignment::Center => "Center",
        TextXAlignment::Right => "Right",
    };
    toml.text = Some(crate::space::gui_loader::GuiTomlText {
        text: class.text.clone(),
        text_color: [
            class.text_color3[0],
            class.text_color3[1],
            class.text_color3[2],
            (1.0 - class.text_transparency).clamp(0.0, 1.0),
        ],
        font_size: class.font_size,
        font_family: String::new(),
        font: String::new(),
        text_x_alignment: x_align.to_string(),
        text_y_alignment: "Center".to_string(),
        text_scaled: false,
        // Compile scaffold (serde-default stroke) — co-agent finalizes wiring.
        text_transparency: 0.0,
        text_stroke_color: [0.0, 0.0, 0.0, 1.0],
        text_stroke_transparency: 1.0,
    });
}

fn save_text_button_changes(
    q: Query<
        (Entity, &eustress_common::classes::TextButton, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::TextButton>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::TextButton>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t, Err(_) => continue,
        };
        apply_text_button_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_text_button: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn apply_text_box_to_toml(
    class: &eustress_common::classes::TextBox,
    toml: &mut crate::space::gui_loader::GuiTomlFile,
) {
    toml.gui.position = class.position;
    toml.gui.size = class.size;
    toml.gui.anchor_point = class.anchor_point;
    toml.gui.background_color = [
        class.background_color3[0],
        class.background_color3[1],
        class.background_color3[2],
        (1.0 - class.background_transparency).clamp(0.0, 1.0),
    ];
    toml.gui.border_size = class.border_size_pixel as f32;
    toml.gui.border_color = [
        class.border_color3[0],
        class.border_color3[1],
        class.border_color3[2],
        1.0,
    ];
    toml.gui.visible = class.visible;
    toml.gui.z_index = class.z_index;
    // TextBox uses placeholder_text when empty — round-trip the
    // user-facing text either way.
    toml.text = Some(crate::space::gui_loader::GuiTomlText {
        text: if class.text.is_empty() { class.placeholder_text.clone() } else { class.text.clone() },
        text_color: [
            class.text_color3[0],
            class.text_color3[1],
            class.text_color3[2],
            (1.0 - class.text_transparency).clamp(0.0, 1.0),
        ],
        font_size: class.font_size,
        font_family: String::new(),
        font: String::new(),
        text_x_alignment: "Left".to_string(),
        text_y_alignment: "Center".to_string(),
        text_scaled: false,
        // Compile scaffold (serde-default stroke) — co-agent finalizes wiring.
        text_transparency: 0.0,
        text_stroke_color: [0.0, 0.0, 0.0, 1.0],
        text_stroke_transparency: 1.0,
    });
}

fn save_text_box_changes(
    q: Query<
        (Entity, &eustress_common::classes::TextBox, &crate::space::instance_loader::InstanceFile),
        (
            Changed<eustress_common::classes::TextBox>,
            Without<crate::space::instance_loader::BeingDragged>,
        ),
    >,
    added: Query<Entity, Added<eustress_common::classes::TextBox>>,
    mut recently_written: ResMut<crate::space::file_watcher::RecentlyWrittenFiles>,
) {
    let just_added: std::collections::HashSet<Entity> = added.iter().collect();
    for (entity, class, inst_file) in &q {
        if just_added.contains(&entity) { continue; }
        let mut toml = match crate::space::gui_loader::load_gui_definition(&inst_file.toml_path) {
            Ok(t) => t, Err(_) => continue,
        };
        apply_text_box_to_toml(class, &mut toml);
        if let Err(e) = crate::space::gui_loader::write_gui_toml(&inst_file.toml_path, &toml) {
            warn!("🪧 save_text_box: write {} failed: {}", inst_file.toml_path.display(), e);
            continue;
        }
        recently_written.mark_written(inst_file.toml_path.clone());
    }
}

fn sync_textbox_to_display(
    mut q: Query<(&eustress_common::classes::TextBox, &mut GuiElementDisplay),
                 Changed<eustress_common::classes::TextBox>>,
) {
    for (tb, mut gui) in &mut q {
        // Show placeholder when text is empty (Roblox behaviour).
        // Use the placeholder colour when the placeholder is showing so
        // empty TextBoxes read as a hint, not a real value.
        let showing_placeholder = tb.text.is_empty();
        gui.text = if showing_placeholder { tb.placeholder_text.clone() } else { tb.text.clone() };
        let text_rgb = if showing_placeholder {
            tb.placeholder_color3
        } else {
            tb.text_color3
        };
        gui.text_color = [text_rgb[0], text_rgb[1], text_rgb[2],
                          (1.0 - tb.text_transparency).clamp(0.0, 1.0)];
        gui.font_size = tb.font_size.max(1.0);
        // TextBox has no `font` or `text_x_alignment` field — use sane
        // defaults (regular weight, left-aligned text).
        gui.font_weight = 400;
        gui.text_align = "Left".to_string();
        gui.text_y_align = "Center".to_string();
        gui.bg_color = [tb.background_color3[0], tb.background_color3[1], tb.background_color3[2],
                        (1.0 - tb.background_transparency).clamp(0.0, 1.0)];
        gui.border_color = [tb.border_color3[0], tb.border_color3[1], tb.border_color3[2], 1.0];
        gui.border_size = tb.border_size_pixel as f32;
        gui.visible = tb.visible;
        gui.z_order = tb.z_index;
        gui.anchor_point = tb.anchor_point;
        gui.position_udim2 = [
            tb.position.x.scale, tb.position.x.offset,
            tb.position.y.scale, tb.position.y.offset,
        ];
        gui.size_udim2 = [
            tb.size.x.scale, tb.size.x.offset,
            tb.size.y.scale, tb.size.y.offset,
        ];
        gui.x = tb.position.x.offset;
        gui.y = tb.position.y.offset;
        gui.width = tb.size.x.offset.max(1.0);
        gui.height = tb.size.y.offset.max(1.0);
    }
}

pub struct BillboardGuiPlugin;

impl Plugin for BillboardGuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(BillboardDrawPlugin)
            .init_resource::<BillboardEditState>()
            // The load throttle the shared drawing reads, from Studio's loader.
            .add_systems(First, sync_gui_load_throttle)
            // bevy_ui must not draw billboard-owned GUI in screen space.
            .add_systems(Update, hide_billboard_owned_ui_nodes)
            // Register DoubleClickedPart alongside the systems that read it.
            // History: the engine crate used to be DUAL-COMPILED (lib + bin
            // each declaring these modules), so the bin's writer and this
            // lib plugin's readers held DIFFERENT TypeIds and never connected
            // — double-click billboard editing was silently dead, and on
            // bevy 0.19 the readers were fetch-time-skipped outright. The
            // 2026-07-02 thin-bin untangling made the lib the single
            // compilation, so writer, readers, and this registration now
            // share one type. Kept here (not only in main.rs) so the plugin
            // is self-sufficient for future headless/alternate bins.
            .add_message::<crate::part_selection::DoubleClickedPart>()
            // Mirror UI-class property edits into the renderer's
            // GuiElementDisplay cache so changes show up live: after the
            // billboards are prepared, before they are rasterized.
            .add_systems(
                Update,
                (
                    sync_textlabel_to_display,
                    sync_frame_to_display,
                    sync_textbutton_to_display,
                    sync_textbox_to_display,
                )
                    .after(BillboardDrawSet::Prepare)
                    .before(BillboardDrawSet::Render),
            )
            // ── In-viewport TextLabel editing ─────────────────────────────
            // Double-click on a part with a BillboardGui descendant enters
            // edit mode on the first TextLabel found. While editing,
            // keyboard input mutates `TextLabel.text` directly so the
            // billboard atlas re-renders live; Enter commits, Escape
            // reverts.
            .add_systems(
                Update,
                (
                    enter_billboard_edit_on_double_click,
                    process_billboard_edit_keyboard
                        .after(enter_billboard_edit_on_double_click),
                ),
            );

        // ── UI-class TOML write-back ──────────────────────────────────
        // GuiTomlFile-to-disk persistence for Property-panel / script /
        // MCP edits. Gated behind the `toml` feature for the same reason
        // `write_instance_changes_system` is (2026-05-15 ECS+DB pivot):
        // in the default build persistence is the WorldDb, not
        // `_instance.toml`, so these systems must NOT run — they also
        // require `RecentlyWrittenFiles`, which is part of the legacy
        // TOML write path. Re-enabled with `--features toml`.
        #[cfg(feature = "toml")]
        {
            app.add_systems(
                Update,
                (
                    save_billboard_gui_changes,
                    save_text_label_changes,
                    save_frame_changes,
                    save_text_button_changes,
                    save_text_box_changes,
                ),
            );
        }
    }
}

/// The shared billboard drawing's load throttle, from Studio's loader: once a
/// frame, so every system that asks in a frame gets the same answer.
fn sync_gui_load_throttle(frames: Res<bevy::diagnostic::FrameCount>, mut throttle: ResMut<GuiLoadThrottle>) {
    let now = GuiLoadThrottle {
        bulk_loading: crate::space::file_loader::bulk_load_active(),
        sync_tick: crate::space::file_loader::ui_sync_tick_for_frame(frames.0 as u64),
    };
    if throttle.bulk_loading != now.bulk_loading || throttle.sync_tick != now.sync_tick {
        *throttle = now;
    }
}

// ============================================================================
// BillboardEditState — in-viewport text editing
// ============================================================================

/// Tracks which TextLabel (if any) the user is currently editing
/// in-viewport. Populated by [`enter_billboard_edit_on_double_click`]
/// when the user double-clicks a part with a BillboardGui descendant;
/// consumed by [`process_billboard_edit_keyboard`] which routes typed
/// characters into `TextLabel.text` on the editing entity.
///
/// `original` is the text that was on the label when edit mode entered,
/// captured so Escape can revert. `replace_on_first_type` mirrors the
/// "select-all + type-to-replace" behaviour every text input on every
/// OS implements — the first printable character clears the existing
/// text, subsequent ones append.
#[derive(Resource, Default, Debug)]
pub struct BillboardEditState {
    pub editing: Option<Entity>,
    pub original: String,
    pub replace_on_first_type: bool,
    /// The same mouse-down that fired the second-click of a double-click
    /// is still `just_pressed` for the rest of this frame. Without this
    /// guard `process_billboard_edit_keyboard`'s click-to-exit branch
    /// would fire on the exact click that entered edit mode and
    /// instantly cancel it. Set true on entry, cleared the next frame.
    pub skip_next_click: bool,
}

/// Walk the ChildOf descendants of `root` and return the first entity
/// that has a [`TextLabel`] component. DFS, sibling order = whatever
/// Bevy hands us — for the typical Part → BillboardGui → TextLabel
/// chain there's only one candidate anyway.
fn find_first_textlabel_descendant(
    root: Entity,
    children_q: &Query<&Children>,
    label_q: &Query<(), With<eustress_common::classes::TextLabel>>,
) -> Option<Entity> {
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(e) = stack.pop() {
        visited += 1;
        if label_q.get(e).is_ok() {
            info!("✏️ found TextLabel descendant {:?} (visited {} entities)", e, visited);
            return Some(e);
        }
        match children_q.get(e) {
            Ok(children) => {
                info!("✏️ entity {:?} has {} children", e, children.len());
                for child in children.iter() {
                    stack.push(child);
                }
            }
            Err(_) => {
                info!("✏️ entity {:?} has NO Children component", e);
            }
        }
    }
    info!("✏️ no TextLabel descendant of {:?} (visited {} entities)", root, visited);
    None
}

/// Entry point: react to `DoubleClickedPart` messages by finding a
/// TextLabel descendant of the clicked entity and entering edit mode
/// on it. If nothing editable is found we just ignore the message —
/// double-clicking a plain part with no label is a no-op.
fn enter_billboard_edit_on_double_click(
    mut events: MessageReader<crate::part_selection::DoubleClickedPart>,
    children_q: Query<&Children>,
    label_q: Query<(), With<eustress_common::classes::TextLabel>>,
    text_q: Query<&eustress_common::classes::TextLabel>,
    // Reverse-direction lookup: every TextLabel entity + its ChildOf
    // chain. Used to walk UP from the clicked entity instead of DOWN.
    // We need both directions because the clicked Part doesn't always
    // own a `Children` component (some spawn paths set `ChildOf` on
    // the child without Bevy ever attaching the reciprocal `Children`
    // to the parent — depends on whether the parent was spawned
    // before the child's ChildOf insert was flushed). The Children
    // descent is the fast path; the ChildOf ascent is the fallback.
    all_textlabels: Query<Entity, With<eustress_common::classes::TextLabel>>,
    child_of_q: Query<&ChildOf>,
    mut edit_state: ResMut<BillboardEditState>,
) {
    for ev in events.read() {
        info!("✏️ DoubleClickedPart received for entity {:?}", ev.entity);

        // Try descent via Children first (fast path).
        let mut label_entity = find_first_textlabel_descendant(ev.entity, &children_q, &label_q);

        // Fallback: scan every TextLabel and walk its ChildOf chain
        // upward. If any ancestor is `ev.entity`, that's our match.
        // O(N_text_labels × tree_depth) per double-click — N is tiny
        // in practice and double-clicks are user-initiated.
        if label_entity.is_none() {
            info!("✏️ descent failed; trying ChildOf ascent over {} TextLabel(s)",
                all_textlabels.iter().count());
            for tl in all_textlabels.iter() {
                let mut cur = tl;
                // Cap the walk at 32 hops — way deeper than any real
                // hierarchy, but stops a malformed cycle from looping.
                for _ in 0..32 {
                    if cur == ev.entity {
                        label_entity = Some(tl);
                        info!("✏️ found via ChildOf ascent: TextLabel {:?} → ... → {:?}", tl, ev.entity);
                        break;
                    }
                    match child_of_q.get(cur) {
                        Ok(parent) => cur = parent.parent(),
                        Err(_) => break,
                    }
                }
                if label_entity.is_some() { break; }
            }
        }

        if let Some(label_entity) = label_entity {
            let original = text_q.get(label_entity)
                .map(|t| t.text.clone())
                .unwrap_or_default();
            edit_state.editing = Some(label_entity);
            edit_state.original = original;
            edit_state.replace_on_first_type = true;
            edit_state.skip_next_click = true;
            info!(
                "✏️ Entered billboard text edit mode on {:?} (original={:?})",
                label_entity, edit_state.original,
            );
        } else {
            info!("✏️ no TextLabel anywhere under {:?} — double-click is a no-op", ev.entity);
        }
    }
}

/// Read keyboard input + commit/abort triggers while in edit mode.
/// Mutates `TextLabel.text` directly so the existing
/// `sync_textlabel_to_display` + atlas re-render machinery shows the
/// changes live in the viewport. `save_text_label_changes` then
/// persists to disk through the normal `Changed<TextLabel>` path.
fn process_billboard_edit_keyboard(
    mut edit_state: ResMut<BillboardEditState>,
    mut text_q: Query<&mut eustress_common::classes::TextLabel>,
    keys: Res<ButtonInput<KeyCode>>,
    mut key_events: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut dbl_events: MessageReader<crate::part_selection::DoubleClickedPart>,
    ui_focus: Option<Res<crate::ui::SlintUIFocus>>,
) {
    // Drain any double-click messages produced THIS frame; they were
    // consumed by `enter_billboard_edit_on_double_click` already but
    // un-read messages persist and we don't want them double-counted
    // against the "click while editing → exit" guard below.
    let _ = dbl_events.read().count();

    let Some(label_entity) = edit_state.editing else {
        // Even if nothing is editing, drain keyboard events so this
        // system doesn't accumulate a queue while inactive.
        let _ = key_events.read().count();
        return;
    };

    // Block edit-mode input when the user has clicked into a Slint
    // panel (Properties, Workshop, …). Without this, typing into the
    // Properties panel while a billboard happens to be in edit mode
    // would double-write the keystroke.
    if let Some(focus) = ui_focus.as_ref() {
        if focus.has_focus {
            let _ = key_events.read().count();
            return;
        }
    }

    // Escape — revert original text, exit edit mode.
    if keys.just_pressed(KeyCode::Escape) {
        if let Ok(mut label) = text_q.get_mut(label_entity) {
            label.text = std::mem::take(&mut edit_state.original);
        }
        edit_state.editing = None;
        edit_state.replace_on_first_type = false;
        info!("✏️ Billboard edit cancelled (Escape)");
        let _ = key_events.read().count();
        return;
    }

    // Enter — commit current text, exit edit mode. The text already
    // sits in TextLabel.text from the live updates below, so we just
    // drop edit state; `save_text_label_changes` writes the TOML.
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        edit_state.editing = None;
        edit_state.replace_on_first_type = false;
        edit_state.original.clear();
        info!("✏️ Billboard edit committed (Enter)");
        let _ = key_events.read().count();
        return;
    }

    // Click outside the label commits and exits. The double-click
    // that triggered edit-mode entry is still `just_pressed` for the
    // rest of THIS frame, so we swallow it once via `skip_next_click`
    // and exit only on the NEXT distinct mouse-down.
    if mouse.just_pressed(MouseButton::Left) {
        if edit_state.skip_next_click {
            edit_state.skip_next_click = false;
        } else {
            edit_state.editing = None;
            edit_state.replace_on_first_type = false;
            edit_state.original.clear();
            info!("✏️ Billboard edit committed (click)");
            let _ = key_events.read().count();
            return;
        }
    } else if !mouse.pressed(MouseButton::Left) {
        // Mouse button released — clear the guard so the next press
        // (which will be `just_pressed` again) commits properly.
        edit_state.skip_next_click = false;
    }

    // Live character input. `KeyboardInput` carries the platform-
    // resolved character via `text: Option<SmolStr>` — that gives us
    // proper layout handling (shift, AltGr, dead keys) for free, far
    // better than mapping `KeyCode` to chars ourselves.
    let mut label_mut = match text_q.get_mut(label_entity) {
        Ok(t) => t,
        Err(_) => return,
    };
    use bevy::input::ButtonState;
    for ev in key_events.read() {
        if ev.state != ButtonState::Pressed { continue; }
        // Backspace — delete one grapheme cluster off the end. Skip
        // the "replace" arming flag; a Backspace with nothing typed
        // yet should clear the original text (matching select-all UX).
        if ev.key_code == KeyCode::Backspace {
            if edit_state.replace_on_first_type {
                label_mut.text.clear();
                edit_state.replace_on_first_type = false;
            } else {
                label_mut.text.pop();
            }
            continue;
        }
        // Typed characters arrive in `event.text`. Filter out the
        // control characters that would otherwise sneak in (Enter,
        // Tab, etc. carry `text` payloads on some platforms).
        if let Some(text) = ev.text.as_ref() {
            for ch in text.chars() {
                if ch.is_control() { continue; }
                if edit_state.replace_on_first_type {
                    label_mut.text.clear();
                    edit_state.replace_on_first_type = false;
                }
                label_mut.text.push(ch);
            }
        }
    }
}
