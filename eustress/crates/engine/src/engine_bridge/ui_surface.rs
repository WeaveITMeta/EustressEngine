//! Agent-facing UI surface: the mode / discipline / tab / tool taxonomy, tool
//! invocation, mode switching, synthetic pointer input, and publishing.
//!
//! # Why this exists
//!
//! The ribbon carries ~1,400 distinct tool ids across twelve modes and their
//! disciplines, but only a small fraction are wired; the rest render fully and
//! deliberately do nothing (see `tool_metadata::ToolMeta::wired`). An agent
//! driving this surface therefore needs two things a human gets for free from
//! looking at the screen: a way to ENUMERATE what exists, and an honest answer
//! about whether a given button actually does something.
//!
//! # The two rules this module exists to keep
//!
//! 1. **Never report a no-op as success.** `ui.invoke_tool` refuses an unwired
//!    id up front rather than dispatching it. The UI already handles a human
//!    clicking a dream button by saying "on the roadmap"; an agent gets the
//!    same answer as a failed call, because an agent that believes it applied
//!    a fillet will build its next ten steps on a fiction.
//!
//! 2. **Never let an agent vote.** A human clicking an unwired button records
//!    a vote in `usage_telemetry` that ranks the build backlog. Agent traffic
//!    must not enter that signal, so the refusal in rule 1 happens BEFORE
//!    anything is queued — the dispatch path that records the vote is never
//!    reached. Wired tools still record ordinary usage, which is accurate:
//!    the tool really was used.
//!
//! # Layering
//!
//! Tool invocation and mode switching go through `SlintActionQueue`, the same
//! queue every real click funnels into, so an agent runs the identical code
//! path as a human rather than a parallel one that can drift. Sequencing lives
//! in the MCP server, NOT here: each step is its own bridge round-trip so the
//! frame loop keeps running between them.

use bevy::prelude::*;
use serde_json::{json, Value};

use super::{BridgeError, BridgeRequest, BridgeResponse};
use crate::studio_modes::{ModeManifest, ModeRegistry};
use crate::tool_metadata::tool_meta;
use crate::ui::slint_ui::{SlintAction, SlintActionQueue};

/// Default page size for `ui.tools`. The full surface is ~1,400 ids; handing
/// that back in one response buries the answer the caller actually wanted.
const TOOLS_PAGE_DEFAULT: usize = 100;
const TOOLS_PAGE_MAX: usize = 1000;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn param_str<'a>(req: &'a BridgeRequest, key: &str) -> Option<&'a str> {
    req.params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn param_bool(req: &BridgeRequest, key: &str) -> Option<bool> {
    req.params.get(key).and_then(|v| v.as_bool())
}

fn param_usize(req: &BridgeRequest, key: &str) -> Option<usize> {
    req.params.get(key).and_then(|v| v.as_u64()).map(|n| n as usize)
}

fn param_f32(req: &BridgeRequest, key: &str) -> Option<f32> {
    req.params.get(key).and_then(|v| v.as_f64()).map(|n| n as f32)
}

/// Resolve the mode/submode a request targets, defaulting to whatever is
/// active. Returns an error naming the valid ids when the caller asks for a
/// mode that does not exist — a typo should say so, not silently fall back to
/// the active mode and return a confidently wrong tool list.
fn resolve_mode<'a>(
    registry: &'a ModeRegistry,
    req: &BridgeRequest,
) -> Result<(&'a ModeManifest, String), String> {
    let mode_id = param_str(req, "mode").unwrap_or(&registry.active_id);
    let manifest = registry.find(mode_id).ok_or_else(|| {
        let known: Vec<&str> = registry.modes.iter().map(|m| m.id.as_str()).collect();
        format!("unknown mode '{mode_id}' — known modes: {}", known.join(", "))
    })?;

    let submode = match param_str(req, "submode") {
        Some(s) => {
            if !manifest.submodes.iter().any(|sm| sm.id == s) {
                let known: Vec<&str> = manifest.submodes.iter().map(|sm| sm.id.as_str()).collect();
                return Err(format!(
                    "mode '{}' has no submode '{s}' — known: {}",
                    manifest.id,
                    if known.is_empty() { "(none)".to_string() } else { known.join(", ") }
                ));
            }
            s.to_string()
        }
        // Only inherit the active submode when the caller did not name a mode
        // either; a submode from a different mode would select nothing.
        None if manifest.id == registry.active_id => registry.active_submode_id.clone(),
        None => String::new(),
    };
    Ok((manifest, submode))
}

/// One tool's public shape. `wired` is the field that matters: everything else
/// is presentation.
fn tool_json(id: &str) -> Value {
    match tool_meta(id) {
        Some(m) => json!({
            "id": id,
            "label": m.label,
            "tooltip": m.tooltip,
            "icon": m.icon,
            "wired": m.wired,
        }),
        // A manifest id with no metadata entry fails the build's completeness
        // test, so this is unreachable in a shipped binary. Reported rather
        // than skipped so a broken manifest is visible instead of silent.
        None => json!({
            "id": id,
            "label": id,
            "tooltip": "",
            "icon": "",
            "wired": false,
            "missing_metadata": true,
        }),
    }
}

// ---------------------------------------------------------------------------
// ui.modes — the taxonomy
// ---------------------------------------------------------------------------

/// Enumerate modes, their disciplines (submodes), and their tabs, with per-mode
/// tool counts split by wired vs total. The counts are the point: they tell a
/// caller how much of a mode is real before it starts planning against it.
pub fn ui_modes(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let Some(registry) = world.get_resource::<ModeRegistry>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("ModeRegistry not available (no UI is running)"),
        );
    };

    let modes: Vec<Value> = registry
        .modes
        .iter()
        .map(|m| {
            let submodes: Vec<Value> = m
                .submodes
                .iter()
                .map(|sm| {
                    let tabs = m.effective_custom_tabs(&sm.id);
                    let (wired, total) = count_tools(tabs);
                    json!({
                        "id": sm.id,
                        "name": sm.name,
                        "required_role": sm.required_role,
                        "tabs": tabs.iter().map(|t| &t.id).collect::<Vec<_>>(),
                        "tools_total": total,
                        "tools_wired": wired,
                    })
                })
                .collect();
            let (wired, total) = count_tools(&m.custom_tabs);
            json!({
                "id": m.id,
                "name": m.name,
                "icon": m.icon,
                "color": m.menu_color.clone().or_else(|| m.accent.clone()),
                "required_role": m.required_role,
                "builtin_tabs": m.tabs,
                "custom_tabs": m.custom_tabs.iter().map(|t| json!({
                    "id": t.id, "name": t.name,
                    "sections": t.sections.len(),
                })).collect::<Vec<_>>(),
                "submodes": submodes,
                "tools_total": total,
                "tools_wired": wired,
            })
        })
        .collect();

    BridgeResponse::ok(
        req.id.clone(),
        json!({
            "active_mode": registry.active_id,
            "active_submode": registry.active_submode_id,
            "mode_count": modes.len(),
            "modes": modes,
            "note": "tools_wired vs tools_total: unwired ids render in the UI but do nothing. \
                     ui.invoke_tool refuses them rather than reporting a false success.",
        }),
    )
}

fn count_tools(tabs: &[crate::studio_modes::CustomTab]) -> (usize, usize) {
    let mut wired = 0usize;
    let mut total = 0usize;
    for tab in tabs {
        for section in &tab.sections {
            for id in &section.tools {
                total += 1;
                if tool_meta(id).map(|m| m.wired).unwrap_or(false) {
                    wired += 1;
                }
            }
        }
    }
    (wired, total)
}

// ---------------------------------------------------------------------------
// ui.tools — the tool listing
// ---------------------------------------------------------------------------

/// List the tools of a mode/submode, optionally narrowed to one tab, to a text
/// query, or to only the ones that actually work.
pub fn ui_tools(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let Some(registry) = world.get_resource::<ModeRegistry>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("ModeRegistry not available (no UI is running)"),
        );
    };
    let (manifest, submode) = match resolve_mode(registry, req) {
        Ok(v) => v,
        Err(e) => return BridgeResponse::error(req.id.clone(), BridgeError::invalid_params(e)),
    };

    let tab_filter = param_str(req, "tab").map(str::to_owned);
    let query = param_str(req, "query").map(|q| q.to_lowercase());
    let wired_only = param_bool(req, "wired_only").unwrap_or(false);
    let offset = param_usize(req, "offset").unwrap_or(0);
    let limit = param_usize(req, "limit")
        .unwrap_or(TOOLS_PAGE_DEFAULT)
        .clamp(1, TOOLS_PAGE_MAX);

    // Flatten first so paging is over tools, not tabs — a caller asking for
    // 100 tools should get 100 tools regardless of how they are grouped.
    let mut flat: Vec<Value> = Vec::new();
    for tab in manifest.effective_custom_tabs(&submode) {
        if tab_filter.as_deref().is_some_and(|f| f != tab.id) {
            continue;
        }
        for section in &tab.sections {
            for id in &section.tools {
                let meta = tool_meta(id);
                let wired = meta.as_ref().map(|m| m.wired).unwrap_or(false);
                if wired_only && !wired {
                    continue;
                }
                if let Some(q) = &query {
                    let label = meta.as_ref().map(|m| m.label).unwrap_or("");
                    if !id.to_lowercase().contains(q) && !label.to_lowercase().contains(q) {
                        continue;
                    }
                }
                let mut entry = tool_json(id);
                entry["tab"] = json!(tab.id);
                entry["tab_name"] = json!(tab.name);
                entry["section"] = json!(section.name);
                flat.push(entry);
            }
        }
    }

    let matched = flat.len();
    let wired_count = flat.iter().filter(|t| t["wired"] == json!(true)).count();
    let page: Vec<Value> = flat.into_iter().skip(offset).take(limit).collect();
    let next_offset = offset + page.len();

    let mut out = json!({
        "mode": manifest.id,
        "submode": submode,
        "matched": matched,
        "matched_wired": wired_count,
        "returned": page.len(),
        "offset": offset,
        "next_offset": if next_offset < matched { json!(next_offset) } else { Value::Null },
        "tools": page,
    });

    // A container mode (Engineering, Public Sector) declares NO tools of its
    // own — every one lives under a discipline. Asking for such a mode without
    // naming one is otherwise answered with a bare `matched: 0`, which reads
    // as "this mode is empty" when it actually has hundreds. Say which
    // disciplines to ask for instead of letting the caller draw the wrong
    // conclusion from a true-but-useless zero.
    if matched == 0 && submode.is_empty() && !manifest.submodes.is_empty() {
        let disciplines: Vec<Value> = manifest
            .submodes
            .iter()
            .map(|sm| {
                let (wired, total) = count_tools(manifest.effective_custom_tabs(&sm.id));
                json!({ "id": sm.id, "name": sm.name, "tools_total": total, "tools_wired": wired })
            })
            .collect();
        let total: u64 = disciplines
            .iter()
            .filter_map(|d| d["tools_total"].as_u64())
            .sum();
        out["hint"] = json!(format!(
            "'{}' declares no tools directly — its {total} tool(s) live under its disciplines. \
             Re-query with `submode` set to one of them (listed in `disciplines`).",
            manifest.id
        ));
        out["disciplines"] = json!(disciplines);
    }

    BridgeResponse::ok(req.id.clone(), out)
}

// ---------------------------------------------------------------------------
// ui.invoke_tool — run one ribbon tool
// ---------------------------------------------------------------------------

/// Invoke a ribbon tool by id, through the same queue a real click uses.
///
/// Refuses an unwired id instead of dispatching it. See the module docs for
/// why that refusal is load-bearing rather than merely tidy.
pub fn ui_invoke_tool(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let Some(id) = param_str(req, "tool_id").map(str::to_owned) else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("missing `tool_id` (get ids from ui.tools)"),
        );
    };

    // The id MUST be a manifest tool that is wired. Both halves matter.
    //
    // `wired` keeps a declared-but-unbuilt button from reporting success.
    //
    // Requiring metadata AT ALL is the security half: `MenuAction` also
    // dispatches the BUILT-IN ribbon strings, which carry no `tool_meta` entry
    // and include plainly destructive ones — `"delete"` writes
    // `keybindings::Action::Delete`. Letting an unknown id through "because
    // built-ins legitimately have no metadata" would hand this tool the entire
    // built-in menu surface, turning a bounded tool-runner into an arbitrary
    // action dispatcher. Built-in actions have their own tool (`invoke_action`)
    // which is gated accordingly; this one stays bounded to the manifest
    // surface it advertises, so what it can reach is exactly what `ui.tools`
    // lists.
    let Some(meta) = tool_meta(&id) else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(format!(
                "'{id}' is not a ribbon tool declared by any mode manifest. This tool only runs \
                 ids that ui.tools lists. For a built-in editor action (Delete, Copy, Group, \
                 SaveScene, ...) use invoke_action, which is gated separately because it can \
                 reach destructive commands."
            )),
        );
    };
    if !meta.wired {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(format!(
                "'{}' ({id}) is declared in the ribbon but not implemented — it renders as a \
                 button and does nothing. Not dispatched, so nothing changed. Use ui.tools with \
                 wired_only=true to list the tools that do work.",
                meta.label
            )),
        );
    }

    let Some(queue) = world.get_resource::<SlintActionQueue>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("SlintActionQueue not available (no UI is running)"),
        );
    };
    queue.push(SlintAction::MenuAction(id.clone()));

    let label = meta.label.to_string();
    BridgeResponse::ok(
        req.id.clone(),
        json!({
            "tool_id": id,
            "label": label,
            "queued": true,
            // The queue drains next frame, so the effect is not observable in
            // this response. Say so rather than implying the work is done.
            "note": "Queued on the UI action queue and applied on the next frame. Confirm the \
                     effect with inspect_scene / get_editor_state rather than assuming it landed.",
        }),
    )
}

// ---------------------------------------------------------------------------
// ui.set_mode — switch mode / discipline
// ---------------------------------------------------------------------------

/// Switch the active mode, and optionally the discipline within it.
///
/// Goes through the same queue actions the Modes dropdown uses, so the theme
/// overlay and persisted editor settings follow exactly as they do for a human.
pub fn ui_set_mode(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let Some(mode_id) = param_str(req, "mode").map(str::to_owned) else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("missing `mode` (get ids from ui.modes)"),
        );
    };

    let Some(registry) = world.get_resource::<ModeRegistry>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("ModeRegistry not available (no UI is running)"),
        );
    };
    let Some(manifest) = registry.find(&mode_id) else {
        let known: Vec<&str> = registry.modes.iter().map(|m| m.id.as_str()).collect();
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(format!(
                "unknown mode '{mode_id}' — known: {}",
                known.join(", ")
            )),
        );
    };

    let submode = param_str(req, "submode").map(str::to_owned);
    if let Some(ref s) = submode {
        if !manifest.submodes.iter().any(|sm| &sm.id == s) {
            let known: Vec<&str> = manifest.submodes.iter().map(|sm| sm.id.as_str()).collect();
            return BridgeResponse::error(
                req.id.clone(),
                BridgeError::invalid_params(format!(
                    "mode '{mode_id}' has no submode '{s}' — known: {}",
                    if known.is_empty() { "(none)".to_string() } else { known.join(", ") }
                )),
            );
        }
    }
    // A mode that carries submodes is not directly selectable in the UI (its
    // dropdown row only expands the submenu), so selecting it without naming
    // one would leave the ribbon on whatever was there before.
    if submode.is_none() && !manifest.submodes.is_empty() {
        let known: Vec<&str> = manifest.submodes.iter().map(|sm| sm.id.as_str()).collect();
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(format!(
                "mode '{mode_id}' is a container: pick one of its disciplines via `submode` — {}",
                known.join(", ")
            )),
        );
    }

    let Some(queue) = world.get_resource::<SlintActionQueue>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("SlintActionQueue not available (no UI is running)"),
        );
    };
    match &submode {
        Some(s) => queue.push(SlintAction::SelectSubmode(mode_id.clone(), s.clone())),
        None => queue.push(SlintAction::SelectMode(mode_id.clone())),
    }

    BridgeResponse::ok(
        req.id.clone(),
        json!({
            "mode": mode_id,
            "submode": submode,
            "queued": true,
            "note": "Applied on the next frame; call ui.modes to confirm the active ids.",
        }),
    )
}

// ---------------------------------------------------------------------------
// ui.click — synthetic pointer input
// ---------------------------------------------------------------------------

/// Dispatch a synthetic pointer event at a logical coordinate.
///
/// This is the escape hatch for surfaces with no tool id: panel widgets, list
/// rows, dialog buttons. It is deliberately the LAST resort — `ui.invoke_tool`
/// names what it is doing and survives a layout change, whereas a coordinate
/// silently means something different the moment the UI moves. Prefer the
/// named path wherever one exists.
pub fn ui_click(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let (Some(x), Some(y)) = (param_f32(req, "x"), param_f32(req, "y")) else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("`x` and `y` are required (logical UI coordinates)"),
        );
    };
    if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("`x`/`y` must be finite, non-negative logical coordinates"),
        );
    }

    let button = param_str(req, "button").unwrap_or("left").to_lowercase();
    if !matches!(button.as_str(), "left" | "right" | "middle") {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(format!(
                "unknown button '{button}' (expected left|right|middle)"
            )),
        );
    }
    let kind = param_str(req, "action").unwrap_or("click").to_lowercase();
    if !matches!(kind.as_str(), "click" | "press" | "release" | "move" | "double") {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(format!(
                "unknown action '{kind}' (expected click|press|release|move|double)"
            )),
        );
    }

    let Some(queue) = world.get_resource::<SlintActionQueue>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("SlintActionQueue not available (no UI is running)"),
        );
    };
    queue.push(SlintAction::SyntheticPointer {
        x,
        y,
        button: button.clone(),
        action: kind.clone(),
    });

    BridgeResponse::ok(
        req.id.clone(),
        json!({
            "x": x, "y": y, "button": button, "action": kind,
            "queued": true,
            "note": "Dispatched into the same Slint window events real mouse input uses. \
                     Verify with capture_viewport — a coordinate cannot confirm its own effect.",
        }),
    )
}

// ---------------------------------------------------------------------------
// publish.status / publish.submit
// ---------------------------------------------------------------------------

/// Report whether this session could publish right now, without publishing.
///
/// Publishing is outward-facing and creates a listing other people can see, so
/// the readiness check is a separate call from the act. An agent can establish
/// that every precondition holds and still leave the decision to a human.
pub fn publish_status(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    let space_root = world
        .get_resource::<crate::space::SpaceRoot>()
        .map(|r| r.0.clone());
    let signed_in = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.token.clone())
        .is_some();
    let user = world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.user.as_ref().map(|u| format!("{u:?}")));
    let in_flight = world
        .get_resource::<crate::ui::file_event_handler::PublishProgressHandle>()
        .and_then(|h| h.0.lock().ok().map(|p| (p.stage.clone(), p.percent, p.complete)));

    let mut blockers: Vec<String> = Vec::new();
    if space_root.is_none() {
        blockers.push("no Space is open — publish operates on the open Space/Universe".into());
    }
    if !signed_in {
        blockers.push("not signed in — publish requires an auth token".into());
    }
    if let Some((_, _, complete)) = &in_flight {
        if !complete {
            blockers.push("a publish is already in flight".into());
        }
    }

    BridgeResponse::ok(
        req.id.clone(),
        json!({
            "ready": blockers.is_empty(),
            "blockers": blockers,
            "signed_in": signed_in,
            "user": user,
            "space_root": space_root.map(|p| p.to_string_lossy().to_string()),
            "in_flight": in_flight.map(|(stage, percent, complete)| json!({
                "stage": stage, "percent": percent, "complete": complete,
            })),
        }),
    )
}

/// Start a publish. Queues the same `FileAction::Publish` the Publish dialog
/// queues, so packaging, the website manifest bake, the moderation dossier and
/// the upload all run exactly as they do for a human.
pub fn publish_submit(world: &mut World, req: &BridgeRequest) -> BridgeResponse {
    use crate::ui::file_dialogs::PublishRequest;
    use crate::ui::file_event_handler::{FileAction, PendingFileActions};

    let name = param_str(req, "experience_name").unwrap_or("").to_string();
    if name.is_empty() {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params(
                "`experience_name` is required — a publish creates a listing and an unnamed one \
                 is not recoverable without editing it afterwards",
            ),
        );
    }

    // Re-check the preconditions here rather than trusting an earlier
    // publish.status: the two calls are separate round-trips and the session
    // can sign out between them.
    if world.get_resource::<crate::space::SpaceRoot>().is_none() {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("no Space is open"),
        );
    }
    if world
        .get_resource::<crate::auth::AuthState>()
        .and_then(|a| a.token.clone())
        .is_none()
    {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::invalid_params("not signed in — publish requires an auth token"),
        );
    }

    let request = PublishRequest {
        experience_name: name.clone(),
        description: param_str(req, "description").unwrap_or("").to_string(),
        genre: param_str(req, "genre").unwrap_or("All").to_string(),
        is_public: param_bool(req, "is_public").unwrap_or(true),
        open_source: param_bool(req, "open_source").unwrap_or(false),
        studio_editable: param_bool(req, "studio_editable").unwrap_or(false),
        space_only: param_bool(req, "space_only").unwrap_or(false),
    };
    let is_public = request.is_public;

    let Some(mut pending) = world.get_resource_mut::<PendingFileActions>() else {
        return BridgeResponse::error(
            req.id.clone(),
            BridgeError::internal("PendingFileActions not available"),
        );
    };
    pending.actions.push(FileAction::Publish(request));

    BridgeResponse::ok(
        req.id.clone(),
        json!({
            "queued": true,
            "experience_name": name,
            "is_public": is_public,
            "note": "Publish started: packaging, website manifest bake, moderation dossier and \
                     upload run over the following frames. Poll publish.status for progress; a \
                     public listing is gated on moderation approval.",
        }),
    )
}
