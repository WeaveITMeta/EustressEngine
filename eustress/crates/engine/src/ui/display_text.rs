//! How much of an outside string Studio's Slint UI draws at once.
//!
//! Slint's software renderer keeps physical coordinates as `i16`: a glyph
//! more than 32,767 physical px from its Text's origin used to abort Studio
//! (0xC0000409). A Text sized to its content grows one line per line of its
//! string, and a line with no break grows sideways, so a string from a file,
//! a Space, a model or a peer could crash Studio by its shape alone. The
//! renderer is patched to clip instead (`.patches/slint-2447c697/PATCH.md`);
//! the bound (`eustress_common::text_bound`, re-exported here) keeps every
//! such Text a size a person can read and a frame can lay out, at any display
//! scale up to 200%. [`shown_block`], [`shown_nowrap`] and [`shown_line`]
//! give it as Slint strings.
//!
//! Only on text that is shown, never on a field that is edited and saved
//! back (the script editor, a LineEdit, a TextEdit, a ComboBox's choices): a
//! bounded value there would save truncated. Those rely on the renderer patch.

pub use eustress_common::text_bound::*;

/// [`bound_block`], ready for a Slint property.
pub fn shown_block(s: &str) -> slint::SharedString {
    bound_block(s).into()
}

/// [`bound_block_nowrap`], ready for a Slint property.
pub fn shown_nowrap(s: &str) -> slint::SharedString {
    bound_block_nowrap(s).into()
}

/// [`bound_line`], ready for a Slint property.
pub fn shown_line(s: &str) -> slint::SharedString {
    bound_line(s).into()
}


/// The outside strings Studio shows, by where they reach Slint: every setter
/// call and every row field listed here passes its value through this module
/// (or a Workshop clip that does), or is a literal. A new outside string
/// shown in one of these files belongs in these lists. Fields that are edited
/// or passed back are left whole on purpose and are not listed.
#[cfg(test)]
mod sink_scan {
    const SLINT_UI: &str = include_str!("slint_ui.rs");
    const PUBLISH_TARGETS: &str = include_str!("publish_targets.rs");
    const UI_BUILDER: &str = include_str!("../ui_builder/mod.rs");
    const UPDATER: &str = include_str!("../updater.rs");

    /// Root-property setters: (file, setter).
    const SETTERS: &[(&str, &str)] = &[
        ("slint_ui", "set_login_error"),
        ("slint_ui", "set_publish_target"),
        ("slint_ui", "set_forge_status_message"),
        ("slint_ui", "set_forge_error_message"),
        ("slint_ui", "set_sync_domain_status"),
        ("slint_ui", "set_soul_test_status"),
        ("slint_ui", "set_soul_error_message"),
        ("slint_ui", "set_script_hover_text"),
        ("slint_ui", "set_parameter_dialog_target"),
        ("slint_ui", "set_parameter_dialog_error"),
        ("slint_ui", "set_api_key_status"),
        ("slint_ui", "set_insert_target_name"),
        ("slint_ui", "set_account_name"),
        ("slint_ui", "set_workshop_agent_activity"),
        ("slint_ui", "set_workshop_product_name"),
        ("slint_ui", "set_active_mode_name"),
        ("slint_ui", "set_unsaved_changes_names"),
        ("slint_ui", "set_scene_name"),
        ("publish_targets", "set_publish_targets_note"),
        ("updater", "set_update_version"),
        ("updater", "set_update_error"),
        ("ui_builder", "set_result_title"),
        ("ui_builder", "set_result_meta"),
        ("ui_builder", "set_selected_title"),
        ("ui_builder", "set_inserted_title"),
        ("ui_builder", "set_inserted_meta"),
        ("ui_builder", "set_usage"),
    ];

    /// Row fields: (file, Slint struct, field).
    const FIELDS: &[(&str, &str, &str)] = &[
        ("slint_ui", "GuiElementData", "text"),
        ("slint_ui", "PublishTreeNode", "name"),
        ("slint_ui", "SearchResultData", "name"),
        ("slint_ui", "SearchResultData", "path"),
        ("slint_ui", "InsertClassData", "display"),
        ("slint_ui", "InsertClassData", "section"),
        ("slint_ui", "RecentSpaceEntry", "label"),
        ("slint_ui", "WorkshopModelEntry", "name"),
        ("slint_ui", "WorkshopModelEntry", "tagline"),
        ("slint_ui", "ChatMessage", "content"),
        ("slint_ui", "ChatMessage", "mcp_endpoint"),
        ("slint_ui", "ChatMessage", "entity_class"),
        ("slint_ui", "ChatMessage", "tool_summary"),
        ("slint_ui", "ChatMessage", "tool_result"),
        ("slint_ui", "ConversationSummary", "title"),
        ("slint_ui", "ConversationSummary", "product_name"),
        ("slint_ui", "ConversationSummary", "preview"),
        ("slint_ui", "RibbonTabData", "name"),
        ("slint_ui", "ThemeEntry", "name"),
        ("slint_ui", "ThemeEntry", "description"),
        ("slint_ui", "ModeData", "name"),
        ("slint_ui", "SubmodeData", "name"),
        ("slint_ui", "CustomTabSectionData", "label"),
        ("slint_ui", "PluginSectionData", "label"),
        ("slint_ui", "PluginButtonData", "label"),
        ("slint_ui", "PluginButtonData", "tooltip"),
        ("slint_ui", "HistoryEntry", "description"),
        ("slint_ui", "DiagnosticSpan", "message"),
        ("slint_ui", "ProblemItem", "message"),
        ("slint_ui", "CenterTab", "name"),
        ("slint_ui", "AssetNode", "name"),
        ("slint_ui", "SoulScriptData", "name"),
        ("slint_ui", "SoulScriptData", "error_message"),
        ("slint_ui", "NotificationData", "title"),
        ("slint_ui", "NotificationData", "message"),
        ("slint_ui", "MentionItemData", "name"),
        ("slint_ui", "MentionItemData", "qualifier"),
        ("publish_targets", "PublishTargetRow", "title"),
        ("ui_builder", "UbRecent", "title"),
        ("ui_builder", "UbRecent", "meta"),
        ("ui_builder", "UbCheck", "detail"),
        ("ui_builder", "UbField", "hint"),
    ];

    /// Listed fields that a construction fills with Studio's own text: (file,
    /// struct, field, value).
    const OWN_TEXT: &[(&str, &str, &str, &str)] = &[
        // RIBBON_TAB_TABLE, the built-in tabs' names.
        ("slint_ui", "RibbonTabData", "name", "(*name).into()"),
    ];

    /// Bounds the value lands in that are not a call to this module.
    const REQUIRED: &[(&str, &str)] = &[
        ("slint_ui", "let step_label = super::display_text::shown_line(&bar_state.step_label);"),
        ("slint_ui", "let message = super::display_text::shown_line(&state.message);"),
        ("slint_ui", "let part_name = super::display_text::shown_line(&state.part_name);"),
        ("slint_ui", "let sketch_name = super::display_text::shown_line(&state.sketch_name);"),
        ("slint_ui", "let status_detail = super::display_text::shown_block(&state.status_detail);"),
        ("slint_ui", "let account_name = super::display_text::bound_line(&account_name);"),
        ("slint_ui", "r.description = super::display_text::shown_block(&r.description);"),
        ("slint_ui", "r.display_name = super::display_text::shown_line(&r.display_name);"),
        ("slint_ui", "super::display_text::bound_block(&clipped)"),
        ("slint_ui", "let text = bound_block(&flatten_markdown_for_display(&t));"),
        ("slint_ui", "let cell = |c: &str| slint::SharedString::from(bound_line(&flatten_markdown_for_display(c)));"),
    ];

    fn file(name: &str) -> &'static str {
        match name {
            "slint_ui" => SLINT_UI,
            "publish_targets" => PUBLISH_TARGETS,
            "ui_builder" => UI_BUILDER,
            "updater" => UPDATER,
            other => panic!("no file {other}"),
        }
    }

    /// Whether a value is bounded, or a literal that needs no bound.
    fn bounded(value: &str) -> bool {
        let v = value.trim();
        ["display_text::", "shown_text(", "shown_wrapped(", "clip_for_display(", "bound_block(", "bound_line("]
            .iter()
            .any(|m| v.contains(m))
            || v.starts_with('"')
            || v.starts_with("s(\"")
            || matches!(v, "slint::SharedString::default()" | "Default::default()" | "SharedString::default()")
    }

    /// The text from `open` (just after an opening bracket) to its closing
    /// bracket, split at the top-level commas; string and char literals are
    /// skipped whole.
    fn top_level_items(text: &str, open: usize) -> (Vec<&str>, usize) {
        let bytes = text.as_bytes();
        let (mut depth, mut i, mut start) = (0i32, open, open);
        let mut items = Vec::new();
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'\'' if i + 2 < bytes.len() && (bytes[i + 2] == b'\'' || bytes[i + 1] == b'\\') => {
                    i += if bytes[i + 1] == b'\\' { 3 } else { 2 };
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' if depth == 0 => {
                    items.push(&text[start..i]);
                    return (items, i);
                }
                b')' | b']' | b'}' => depth -= 1,
                b',' if depth == 0 => {
                    items.push(&text[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        (items, i)
    }

    #[test]
    fn every_listed_outside_string_is_bounded() {
        let mut unbounded = Vec::new();
        for (name, setter) in SETTERS {
            let text = file(name);
            let needle = format!(".{setter}(");
            let mut calls = 0;
            for (at, _) in text.match_indices(&needle) {
                calls += 1;
                let (args, _) = top_level_items(text, at + needle.len());
                let arg = args.first().copied().unwrap_or("");
                if !bounded(arg) {
                    unbounded.push(format!("{name}: .{setter}({})", arg.trim()));
                }
            }
            assert!(calls > 0, "{name} no longer calls .{setter}(: update this list");
        }
        for (name, ty, field) in FIELDS {
            let text = file(name);
            let needle = format!("{ty} {{");
            let mut literals = 0;
            for (at, _) in text.match_indices(&needle) {
                let before = text[..at].trim_end();
                let prefix_ok = text[..at].ends_with(|c: char| !c.is_alphanumeric() && c != '_' && c != ':');
                if !prefix_ok || before.ends_with("->") || before.ends_with("struct") || before.ends_with("impl") || before.ends_with("for") {
                    continue;
                }
                literals += 1;
                let (items, _) = top_level_items(text, at + needle.len());
                for item in items {
                    if let Some((key, value)) = item.split_once(':') {
                        let own = OWN_TEXT.iter().any(|(n, t, f, v)| n == name && t == ty && f == field && *v == value.trim());
                        if key.trim() == *field && !bounded(value) && !own {
                            unbounded.push(format!("{name}: {ty}.{field}: {}", value.trim()));
                        }
                    }
                }
            }
            assert!(literals > 0, "{name} no longer builds {ty}: update this list");
        }
        for (name, line) in REQUIRED {
            assert!(file(name).contains(line), "{name} no longer has `{line}`");
        }
        assert!(unbounded.is_empty(), "outside strings that skip display_text:\n{}", unbounded.join("\n"));
    }

    #[test]
    fn the_scan_finds_an_unbounded_value() {
        let src = "rows.push(Row { title: item.title.as_str().into(), id: 3 });\nui.set_x(name.into());";
        let at = src.find("Row {").unwrap() + "Row {".len();
        let (items, _) = top_level_items(src, at);
        let title = items.iter().find_map(|i| i.split_once(':').filter(|(k, _)| k.trim() == "title")).unwrap().1;
        assert!(!bounded(title), "a raw field passes");
        assert!(bounded(" super::display_text::shown_line(&item.title)"));
        assert!(bounded(" \"Plugins\".into()"));
        let call = src.find(".set_x(").unwrap() + ".set_x(".len();
        assert_eq!(top_level_items(src, call).0, vec!["name.into()"]);
        assert!(!bounded("name.into()"));
    }
}

/// The renderer patch is made for one Slint revision. While it is applied,
/// every Slint crate in the lockfile must come from that revision: a `cargo
/// update` that moves Slint fails here until the patch is checked against
/// the new revision (`.patches/slint-2447c697/PATCH.md`, `verify.mjs`).
#[cfg(test)]
mod slint_patch {
    const MANIFEST: &str = include_str!("../../../../Cargo.toml");
    const LOCK: &str = include_str!("../../../../Cargo.lock");
    const PATCHED_FOR: &str = "2447c697327e7f87c5044af229e0be6c18608fcc";

    #[test]
    fn slint_stays_on_the_revision_the_patch_was_made_for() {
        if !MANIFEST.contains(".patches/slint-2447c697/i-slint-renderer-software") {
            return; // the patch is not applied
        }
        let sources: Vec<&str> = LOCK
            .lines()
            .filter(|l| l.starts_with("source = \"git+https://github.com/slint-ui/slint"))
            .collect();
        assert!(!sources.is_empty(), "no Slint crates in Cargo.lock");
        for source in sources {
            assert!(
                source.ends_with(&format!("#{PATCHED_FOR}\"")),
                "Slint moved ({source}) while .patches/slint-2447c697 is applied: check the patch \
                 against the new revision (PATCH.md, verify.mjs) before building with it"
            );
        }
    }
}

/// The renderer half (`.patches/slint-2447c697/PATCH.md`): text past the
/// software renderer's `i16` pixel space draws instead of aborting Studio.
/// Every raw case here panics with the stock renderer.
#[cfg(test)]
mod render_tests {
    use super::*;
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
    use slint::platform::WindowAdapter;
    use slint::ComponentHandle;
    use std::cell::RefCell;
    use std::rc::Rc;

    slint::slint! {
        export component WrappedProbe inherits Window {
            in property <string> body;
            Text { x: 0px; y: 0px; width: 380px; height: self.preferred-height;
                   text: root.body; wrap: word-wrap; font-size: 14px; color: white; }
        }
        export component NowrapProbe inherits Window {
            in property <string> body;
            Text { x: 0px; y: 0px; width: self.preferred-width; height: self.preferred-height;
                   text: root.body; wrap: no-wrap; font-size: 14px; color: white; }
        }
        // The script editor's shape: a TextInput as tall as its text, in a
        // Flickable scrolled by `scroll`.
        export component EditorProbe inherits Window {
            in property <string> body;
            in property <length> scroll: 0px;
            public function select-everything() { input.select-all(); }
            Flickable {
                x: 0px; y: 0px; width: 380px; height: 300px;
                viewport-height: input.preferred-height;
                viewport-y: -root.scroll;
                input := TextInput { width: 380px; height: self.preferred-height; text: root.body;
                    single-line: false; wrap: word-wrap; font-size: 14px; color: white;
                    selection-background-color: #3366ff; }
            }
        }
    }

    thread_local! {
        // The window the next component is created in.
        static NEXT: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
    }

    struct Platform;
    impl slint::platform::Platform for Platform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            NEXT.with(|n| n.borrow().clone())
                .map(|w| w as Rc<dyn WindowAdapter>)
                .ok_or_else(|| slint::PlatformError::Other("no window for the probe".into()))
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum Shape {
        Wrapped,
        Nowrap,
        Editor { scroll: f32, select: bool },
    }

    const W: u32 = 800;
    const H: u32 = 600;

    /// Pixels that differ from the same window with an empty string.
    fn changed_pixels(shape: Shape, body: &str, buffer: RepaintBufferType) -> usize {
        let window = MinimalSoftwareWindow::new(buffer);
        NEXT.with(|n| *n.borrow_mut() = Some(window.clone()));
        window.dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: 1.5 });
        window.set_size(slint::PhysicalSize::new(W, H));
        let set: Box<dyn Fn(&str)> = match shape {
            Shape::Wrapped => {
                let c = WrappedProbe::new().unwrap();
                c.show().unwrap();
                Box::new(move |s: &str| c.set_body(s.into()))
            }
            Shape::Nowrap => {
                let c = NowrapProbe::new().unwrap();
                c.show().unwrap();
                Box::new(move |s: &str| c.set_body(s.into()))
            }
            Shape::Editor { scroll, select } => {
                let c = EditorProbe::new().unwrap();
                c.show().unwrap();
                Box::new(move |s: &str| {
                    c.set_body(s.into());
                    if !s.is_empty() {
                        c.set_scroll(scroll);
                        if select {
                            c.invoke_select_everything();
                        }
                    }
                })
            }
        };
        let mut base = vec![Rgb565Pixel(0); (W * H) as usize];
        set("");
        window.draw_if_needed(|r| {
            r.render(&mut base, W as usize);
        });
        let mut frame = base.clone();
        set(body);
        window.request_redraw();
        let drew = window.draw_if_needed(|r| {
            r.render(&mut frame, W as usize);
        });
        assert!(drew, "{shape:?}: no frame");
        base.iter().zip(&frame).filter(|(a, b)| a.0 != b.0).count()
    }

    #[test]
    fn text_past_the_i16_limit_draws() {
        slint::platform::set_platform(Box::new(Platform)).unwrap();
        let lines = "line of text\n".repeat(10_000);
        let long = "a".repeat(131_000);
        let cases = [
            ("10,000 lines", Shape::Wrapped, lines.clone()),
            ("131,000 characters", Shape::Wrapped, long.clone()),
            ("131,000 characters unwrapped", Shape::Nowrap, long.clone()),
            ("editor, top", Shape::Editor { scroll: 0.0, select: false }, lines.clone()),
            ("editor, 150,000 px down", Shape::Editor { scroll: 150_000.0, select: false }, lines.clone()),
            ("editor, all selected", Shape::Editor { scroll: 150_000.0, select: true }, lines.clone()),
            ("10,000 lines, bounded", Shape::Wrapped, bound_block(&lines)),
            ("131,000 characters, bounded", Shape::Wrapped, bound_block(&long)),
            ("131,000 characters unwrapped, bounded", Shape::Nowrap, bound_block_nowrap(&long)),
        ];
        for buffer in [RepaintBufferType::ReusedBuffer, RepaintBufferType::NewBuffer] {
            for (name, shape, body) in &cases {
                let changed = changed_pixels(*shape, body, buffer);
                assert!(changed > 0, "{name} ({buffer:?}): drew no text");
            }
        }
    }
}
