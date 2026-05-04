# Eustress Toolset UX — Visual Language, Interaction Patterns, Components

> Sibling docs:
> - [TOOLSET.md](TOOLSET.md) — direct-manipulation editor tools (what to build)
> - [TOOLSET_CAD.md](TOOLSET_CAD.md) — parametric CAD suite (what to build)
> - **This doc** — **how** those tools look, feel, and interoperate
> - [ADORNMENT_ARCHITECTURE.md](ADORNMENT_ARCHITECTURE.md) — mesh-based handle system

## 1. Thesis

A tool that requires a manual to use is a failed tool. Eustress's
authoring surface should be readable at a glance by anyone who has used
Roblox Studio, Blender, Fusion, SketchUp, or Maya — and distinctly
*better* than all of them at the moments that matter:

- **First-click clarity** — the user knows what's about to happen before they commit.
- **Zero-context defaults** — open any tool, press Enter, get a reasonable result.
- **Progressive disclosure** — 90% of users never touch the `⋯` menu; power users own it.
- **Muscle-memory preservation** — don't move things that exist in other tools.
- **Brand without shouting** — Eustress teal `#00d4a8` marks brand moments (logo, active tab, active tool ring), never decorates chrome.

## 2. Design Tokens

### 2.1 Color palette — diff vs `theme.slint`

| Token                    | Current   | Proposed  | Use                                                |
|--------------------------|-----------|-----------|----------------------------------------------------|
| `accent-blue`            | `#0078d4` | keep      | Primary buttons, selection outlines, drop-target   |
| `accent-green`           | `#3cba54` | keep      | Success toasts, ghost-preview base                 |
| `accent-cyan` *(new)*    | —         | `#00bcd4` | Tool Options Bar highlights, preview outline       |
| `accent-eustress` *(new)*| —         | `#00d4a8` | **Brand teal** — logo, active tab, active tool ring|
| `accent-green-bright` *(new)* | —    | `#00e676` | Commit-success flash 150ms                         |
| `accent-orange`          | `#e8912d` | keep      | Warning, destructive confirmation                  |
| `accent-red`             | `#e74856` | keep      | Error, over-constrained, conflict                  |
| `text-primary`           | `#d4d4d4` | keep      | Body text, labels                                  |
| `text-accent`            | `#4fc1ff` | → `#00bcd4` | Link-like text, match accent-cyan              |
| `panel-glass`   *(new)*  | —         | `#121212e8` | Frosted-glass floating overlays (12px blur)     |
| `border-highlight` *(new)* | —       | `#ffffff10` | 1px top hairline on all panels (macOS trick)    |
| `shadow-float`   *(new)* | —         | `#00000060` | Drop shadow for floating panels (16px blur)     |

**Three brand colors that must never change**: `accent-blue`,
`accent-green`, `accent-eustress`. Everything else can evolve.

### 2.2 Typography

| Role            | Size | Weight | Usage                                           |
|-----------------|-----:|--------|-------------------------------------------------|
| `font-xxl`      | 24px | 600    | Dialog titles only                              |
| `font-xl`       | 18px | 500    | Panel titles, prominent labels                  |
| `font-lg`       | 15px | 500    | Ribbon tab labels, section headers              |
| `font-md`       | 13px | 400    | Body — primary readable tier                    |
| `font-sm`       | 11px | 400    | Property labels, tooltip bodies, secondary rows |
| `font-xs`       | 10px | 400    | Button labels under icons, shortcut hints       |
| `font-mono-md`  | 13px | 400    | Numeric inputs, log entries, addresses          |

Font stack: `"Inter", "Segoe UI", system-ui, sans-serif`. Monospace:
`"JetBrains Mono", "Cascadia Code", monospace`. Both Inter and JetBrains
Mono are OFL-licensed, render crisp at 10px on every platform, ship free.

### 2.3 Spacing + radius

Respect existing `Theme.spacing-*` and `Theme.radius-*`. Don't invent new
values; if something doesn't fit the grid, redesign.

| Token         | Value | Where |
|---------------|------:|-------|
| `spacing-xs`  | 2px   | Inside buttons, tight icon-label gaps |
| `spacing-sm`  | 4px   | Sub-section gaps, control row spacing |
| `spacing-md`  | 8px   | Section gaps, input row spacing |
| `spacing-lg`  | 16px  | Panel padding |
| `spacing-xl`  | 24px  | Dialog padding |
| `radius-sm`   | 2px   | Dividers, hairlines — almost square |
| `radius-md`   | 4px   | Inline buttons, input fields |
| `radius-lg`   | 8px   | Floating panels, toast notifications |
| `radius-xl`   | 12px  | Dialog windows |

### 2.4 Motion spec

- **Hover transitions**: 120ms `cubic-ease-out`
- **Focus rings**: 100ms fade-in, stay until blur
- **Panel enter/exit**: 180ms slide+fade
- **Commit flash** (`accent-green-bright` burst after successful tool commit): 150ms ease-out, single shot
- **Preview pulse** (ghost geometry): 1.5 Hz sine, alpha 0.30 ↔ 0.50, no easing
- **Active tab underline slide**: 140ms ease-in-out when switching tabs
- **Drag thresholds**: 5px distance OR 150ms time before starting a drag (prevents accidental drags on click)

All motion respects `prefers-reduced-motion` when the OS surfaces it —
transitions shrink to 30ms, pulse freezes.

### 2.5 Iconography

- **Library**: [Lucide](https://lucide.dev) (MIT, 1200+ icons, 1.5–2px stroke, active community) as the default. Hand-drawn overrides for Eustress-specific concepts (Workshop/@mention, Rune, Bliss, Forge, MindSpace, V-Cell).
- **Grid**: 20×20px SVG source, rendered 22×22px (1px anti-aliasing bleed)
- **Stroke**: 1.75px, rounded caps, rounded joins
- **Color**: SVG fills `currentColor` so `Theme.colorize` tints at runtime
- **Never**: drop shadows inside icons, multi-color icons for a single tool, filled+outlined inconsistency

## 3. Core UI Components

Every component below is a Slint module with a concrete public API.
Tools compose them — they don't invent parallel widgets.

### 3.1 `IconButton` (extends existing)

Existing component in `theme.slint`. One fix needed:

```slint
// theme.slint — fix icon centering in IconButton
VerticalLayout {
    alignment: center;           // ← ADDED: cross-axis centering
    padding: 4px;
    spacing: 2px;
    
    HorizontalLayout {
        alignment: center;
        Image {
            width: 22px; height: 22px;
            source: root.icon;
            image-fit: contain;
            colorize: root.tint;
        }
    }
    
    if root.label != "": Text { ... }
    if root.label != "" && root.shortcut != "": Text { ... }
}
```

### 3.2 `ToolOptionsBar` (new)

Lives at the top-left of the viewport, below the "Default" layout
switcher. Always visible while any tool is active. Shows the active
tool's name + step label + tool-specific controls + universal right-side
toggles (snap / axis lock / pivot).

```slint
export component ToolOptionsBar inherits Rectangle {
    in property <string>  tool-name;         // "Gap Fill"
    in property <string>  step-label;        // "pick first edge"
    in property <bool>    collapsed;         // double-click to toggle
    in property <[ToolOptionControl]> controls;
    in property <bool>    snap-enabled;
    in property <string>  snap-mode;         // "grid" | "vertex" | "face" | "normal"
    in property <string>  pivot-mode;        // "median" | "active" | "individual" | "cursor"
    
    callback control-changed(int, string);
    callback snap-toggled();
    callback snap-mode-changed(string);
    callback pivot-mode-changed(string);
    callback collapse-toggled();

    height: root.collapsed ? 24px : 32px;
    width: min(parent.width * 0.5, 640px);
    background: #121212e8;          // panel-glass
    border-radius: 8px;             // radius-lg
    border-width: 1px;
    border-color: #222222;
    drop-shadow-blur: 16px;
    drop-shadow-color: #00000060;
    // 1px top hairline
    Rectangle {
        y: 0; width: 100%; height: 1px;
        background: #ffffff10;      // border-highlight
    }
    ...
}
```

Layout when uncollapsed:
```
┌────────────────────────────────────────────────────────────────────────┐
│ [ico] Gap Fill — pick first edge │ Thickness [0.20 ▿] Mode [Auto ▿]   │
│                                                             [⋯][Snap][Piv]│
└────────────────────────────────────────────────────────────────────────┘
```

Keyboard: `Tab` cycles forward, `Shift+Tab` backward, `Enter` commits
the focused control. `F` toggles snap. `,`/`.` cycles pivot mode.

### 3.3 `FloatingNumericInput` (new)

Pops up at cursor during drag when the user types any digit. Absorbs
keyboard focus without stopping the drag.

```slint
export component FloatingNumericInput inherits PopupWindow {
    in property <string> value-text;        // "2.5 m"
    in property <string> axis-label;        // "along X axis (Local)"
    in property <string> unit;              // "m" | "studs" | "deg"
    in property <color>  accent: #00bcd4;   // accent-cyan cursor
    in property <bool>   relative;          // "+2.5" vs "2.5"
    
    callback committed(string);             // fires on Enter
    callback cancelled();                   // fires on Esc / RMB
    callback axis-requested(string);        // fires on Tab cycle
    ...
}
```

Visual:
```
┌──────────────────────────┐
│ 2.5 m │                  │  ← value-text, 13px mono, accent-cyan cursor
│ along X axis (Local)     │  ← axis-label, 11px, text-secondary
└──────────────────────────┘
```

Anchors to cursor position at the moment the first digit was typed;
does NOT follow subsequent cursor movement (would be dizzying). Closes
on Enter / Esc / blur.

### 3.4 `ToastUndo` (new)

Corner-anchored (top-center) toast with inline Undo action for
large-scale operations (100+ items).

```slint
export component ToastUndo inherits Rectangle {
    in property <string> message;           // "Mirrored 324 parts"
    in property <string> undo-shortcut;     // "Ctrl+Z"
    in property <int>    auto-dismiss-ms: 5000;
    
    callback undo-clicked();
    callback dismissed();

    background: #121212e8;  border-radius: 8px;  drop-shadow-blur: 16px;
    // Fade in 180ms, stay 5s, fade out 180ms. Hover pauses auto-dismiss.
    ...
}
```

### 3.5 `ModalToolSession` (new — Rust-side orchestration)

Rust side, not Slint. Owns the active modal tool's lifetime.

```rust
pub trait ModalTool: Send + Sync + 'static {
    fn name(&self) -> &'static str;                  // "Gap Fill"
    fn step_label(&self) -> String;                  // "pick first edge"
    fn controls(&self) -> Vec<ToolOptionControl>;    // feed ToolOptionsBar
    fn on_viewport_click(&mut self, hit: ViewportHit) -> ToolStepResult;
    fn on_viewport_hover(&mut self, hit: ViewportHit);
    fn on_numeric_commit(&mut self, axis: Axis, value: f32, relative: bool);
    fn on_control_changed(&mut self, id: &str, value: &str);
    fn commit(&mut self, world: &mut World);
    fn cancel(&mut self, world: &mut World);
    fn preview_entities(&self) -> &[Entity];
}

pub enum ToolStepResult {
    Continue,
    Commit,       // session exits after commit
    Cancel,       // session exits without changes
}

#[derive(Resource, Default)]
pub struct ActiveModalTool(Option<Box<dyn ModalTool>>);
```

The `ModalToolSession` plugin handles:
- Cursor badge (custom cursor with tool icon)
- Ribbon button active-glow coordination
- Tool Options Bar population
- Escape / RMB / click-button-again → cancel
- Successful commit → auto-exit back to Select
- Undo grouping — one undo entry per session, labelled `"{tool_name}"`

### 3.6 `GhostPreviewMaterial` (new)

Shared `StandardMaterial` handle in `AdornmentMaterials` (see
`adornment_renderer.rs`) for all tool previews. Parameters fixed:

```rust
// In create_adornment_assets:
let ghost_preview = materials.add(StandardMaterial {
    base_color: Color::srgb(0.235, 0.729, 0.329).with_alpha(0.40),  // #3cba54 at 40%
    emissive: LinearRgba::from(Color::srgb(0.235, 0.729, 0.329)) * 2.5,
    unlit: true,
    depth_bias: -500.0,     // always-on-top like selection
    alpha_mode: AlphaMode::Blend,
    cull_mode: None,
    ..default()
});
// Plus a second `ghost_preview_outline` for the 1px cyan silhouette pass.
```

Alpha pulses via a small system:
```rust
fn pulse_ghost_preview_alpha(
    time: Res<Time>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mats_res: Res<AdornmentMaterials>,
) {
    let t = time.elapsed_secs();
    let alpha = 0.40 + 0.10 * (t * std::f32::consts::TAU * 1.5).sin();
    if let Some(m) = mats.get_mut(&mats_res.ghost_preview) {
        m.base_color.set_alpha(alpha);
    }
}
```

## 4. Tool Options Bar — Per-Tool Defaults

Every Smart Build Tool has a fixed default Options Bar layout. These
are muscle-memory anchors; users expect them to be consistent forever.

### 4.1 Select
```
[ico] Select │ Snap [Grid ▿] [0.50 ▿] │ Pivot [Median ▿] │ Axis [All ▿]   [⋯]
```

### 4.2 Move
```
[ico] Move │ Space [World ▿] │ Nudge [0.50 ▿] │ Surface [✓] Align-Normal [✗] [⋯]
```

### 4.3 Rotate
```
[ico] Rotate │ Space [World ▿] │ Angle [15° ▿] │ Snap [✓]                  [⋯]
```

### 4.4 Scale
```
[ico] Scale │ Space [Local ▿] │ Uniform [✗] │ Scale-Snap [0.10 ▿]         [⋯]
```

### 4.5 Gap Fill
```
[ico] Gap Fill — pick first edge │ Thickness [0.20 m ▿] │ Mode [Auto ▿]   [⋯]
  ⋯ expands: Triangulation [Delaunay ▿] │ Preserve Material [✓] │ Preserve Color [✓]
```

### 4.6 Resize Align
```
[ico] Resize Align — pick source face │ Mode [Outer Touch ▿] │ [Dragger ✗]  [⋯]
  ⋯ expands: Smart Select [✓] │ Exact Target [✗] │ Join Surfaces [✓] │ Fillet Radius [auto]
```

### 4.7 Edge Align
```
[ico] Edge Align — pick source edge │ Direction [Closest ▿] │ Keep Size [✓] [⋯]
```

### 4.8 Part Swap
```
[ico] Part Swap — pick target(s) │ Mode [Replace ▿] │ Template [open picker]   [⋯]
  ⋯ expands: Preserve Material [✓] │ Preserve Color [✓] │ Reparent Children [✓] │ AI Suggest [✗]
```

### 4.9 Model Reflect
```
[ico] Model Reflect │ Plane [Selection XZ ▿] │ Linked [✗] │ Weld Fix-up [✓]  [⋯]
  ⋯ expands: Invert Normals [✓] │ Rename Suffix [_L/_R ▿]
```

### 4.10 Material Flip
```
[ico] Material Flip │ [Rot 90° CW] [Rot 90° CCW] [Mirror U] [Mirror V]    [⋯]
  ⋯ expands: Scope [Per Part ▿] │ Apply to Selection [✓]
```

### 4.11 Part to Terrain
```
[ico] Part → Terrain │ Material [Grass ▿] │ Resolution [0.5 m ▿] │ Delete Sources [✗] [⋯]
  ⋯ expands: Preview Voxels [✗] │ Dither [✗] │ Biome [Forest ▿]
```

### 4.12 Sketch (CAD)
```
[ico] Sketch │ Plane [XY ▿] │ Snap [Grid ▿] [1 mm ▿] │ Constrain [Auto ▿]   [⋯]
```

### 4.13 Extrude (CAD)
```
[ico] Extrude │ Depth [20 mm ▿] │ Mode [Blind ▿] │ Op [New Body ▿] │ Draft [0°]  [⋯]
  ⋯ expands: Merge Tangent [✗] │ Both Sides [✗] │ Symmetric [✗]
```

### 4.14 Revolve (CAD)
```
[ico] Revolve │ Axis [pick] │ Angle [360° ▿] │ Op [New Body ▿]            [⋯]
```

### 4.15 Fillet (CAD)
```
[ico] Fillet │ Radius [2 mm ▿] │ Edges [auto ▿] │ Propagate Tangent [✓]   [⋯]
```

### 4.16 Boolean (CAD)
```
[ico] Boolean │ Op [Union ▿] │ Target [selected ▿] │ Keep Originals [✗]    [⋯]
```

## 5. Cursor + Tool Announcements

### 5.1 Custom cursor per active tool

The OS cursor gets a 12×12 tool-icon badge at its tail (lower-right of
the cursor hotspot). Implementation: Slint `mouse-cursor` is set to a
named cursor resource; Eustress defines one per tool:

```
Tool            Cursor badge     Hotspot offset
Select          none (default)   —
Move            cross-arrows     —
Rotate          rotate           —
Scale           resize-corner    —
Gap Fill        link-2           +8, +12
Resize Align    arrows-left-right +8, +12
Edge Align      align-horizontal +8, +12
Part Swap       repeat           +8, +12
Model Reflect   flip-horizontal  +8, +12
Sketch          pencil           +8, +12
Extrude         arrow-up-square  +8, +12
```

### 5.2 Ribbon button active state

```slint
// When the button represents the active tool:
background: #00d4a815;            // accent-eustress at ~8% alpha
border-width: 2px;
border-color: #00d4a8;            // accent-eustress full
drop-shadow-blur: 8px;
drop-shadow-color: #00d4a840;     // accent-eustress glow

// Label color shifts to accent-eustress too; tooltip keeps default.
```

### 5.3 Ribbon tab active state

Thin underline slides between tabs, 2px tall, `accent-eustress` color,
140ms ease-in-out transition. The tab's label weight shifts from 400 →
600 on active.

## 6. Panel Chrome Rules

### 6.1 Floating overlays (Tool Options Bar, toasts, popups, context menus)

```
background:           #121212e8       // panel-glass
border-width:         1px
border-color:         #222222
border-radius:        8px             // radius-lg
drop-shadow-blur:     16px
drop-shadow-color:    #00000060       // shadow-float
backdrop-filter:      blur(12px)      // when GPU supports it
```

Top hairline (macOS-style depth trick):
```slint
Rectangle {
    y: 0; width: 100%; height: 1px;
    background: #ffffff10;
}
```

### 6.2 Anchored panels (Explorer, Properties, Workshop, Output)

```
background:       #121212         // panel-background (opaque)
border-color:     #222222
border-radius:    0               // edge-to-edge
drop-shadow:      none
```

Anchored panels stay crisp and don't compete with the 3D viewport for
clarity. Glass is reserved for ephemeral overlays.

### 6.3 Dialogs

```
background:       #1a1a1a          // dialog-background
border-radius:    12px             // radius-xl
drop-shadow-blur: 40px
drop-shadow-color: #000000a0       // modal-backdrop 60% alpha
```

Plus a full-screen backdrop at `#000000a0`.

## 7. Interaction Rules

### 7.1 Tool activation / exit

| Gesture                          | Effect                                          |
|----------------------------------|-------------------------------------------------|
| Click ribbon button              | Activate tool. If already active → deactivate (back to Select). |
| Keyboard shortcut                | Same as click                                   |
| Successful commit                | Auto-exit to Select (opt-out in Options Bar `⋯`)|
| `Esc`                            | Cancel in-progress session, return to Select    |
| Right-click inside viewport      | Cancel in-progress; empty-space also shows context menu |
| Click the tool button again      | Cancel in-progress                              |

### 7.2 Commit / cancel during multi-click tools

| Tool state                       | Commit          | Cancel          |
|----------------------------------|-----------------|-----------------|
| Awaiting first click             | —               | Esc / RMB / button |
| Between clicks (preview shown)   | Next click      | Esc / RMB       |
| Numeric input open               | Enter           | Esc / RMB       |
| Thickness / radius drag          | Click           | Esc / RMB       |

### 7.3 Numeric input rules

Trigger: first digit keypress during active drag or modal tool session.

- `0-9`, `.`, `-` → start entering value
- `+N` → relative delta from drag start
- `N` alone → absolute
- `=expr` → evaluate expression (Phase 2: Rune sandbox)
- `Tab` → cycle to next axis (X → Y → Z → X)
- `Enter` → commit
- `Esc` → cancel, restore pre-drag state
- Typed value respects the tool's unit (studs for Move, degrees for Rotate, user-pref for CAD)

### 7.4 Progressive disclosure via `⋯`

Every Tool Options Bar has a `⋯` button at the right. Clicking expands
a popover with advanced options. Options in the popover are **never**
required to use the tool — defaults must produce a valid result. The
`⋯` popover closes on focus-loss OR Esc.

## 8. Accessibility

- **Keyboard-only**: every button has an `Alt+<letter>` accelerator (printed as dim underline in the label) OR a keyboard shortcut (printed in tooltip).
- **Focus ring**: `2px outline accent-cyan` on any keyboard-focused control. 100ms fade-in.
- **High contrast**: Theme supports a high-contrast variant (P2). Uses `#ffffff` / `#000000` for text, doubles border widths.
- **Screen readers**: Slint ARIA labels on all controls (when Slint supports).
- **Reduced motion**: OS preference respected — motion clamped to 30ms, pulse animations freeze.
- **Color-blind safe**: never rely on color alone — selection has a border + fill, errors have an icon + red, ghost preview has outline + fill. Anyone passing R/G color-blindness tests should still read the UI.

## 9. Telemetry (opt-in, privacy-first)

Measure what we can't assume:
- Time-to-task per Smart Build Tool (pass/fail toward §10 metrics in TOOLSET.md)
- `⋯` popover open rate per tool (low rate = defaults are good)
- Esc usage rate per tool (high rate = UX confusion)
- Keyboard vs mouse ratio (validates Phase-0 keyboard-first promises)

Opt-in in Settings. No content telemetry — only tool invocation counters
and session durations. Anonymized via hashed install ID.

## 9.5 Shipped Snapshot (2026-04-21)

A compact status summary as of **2026-04-22**. See §10 for the full
checklist.

**UX foundation — fully shipped (Phase 0)**
- 6 palette tokens (`accent-cyan`, `accent-eustress`,
  `accent-green-bright`, `panel-glass`, `border-highlight`,
  `shadow-float`) + `backdrop-blur-lg` intent token
- `panel-anim-duration` + `panel-anim-duration-fast/flash` motion tokens
- `reduce-motion` + `motion-scale` theme inputs (manual flip ready;
  OS-level detection deferred)
- `IconButton` centering fix
- `ToolOptionsBar` Slint component with flat-union controls + `⋯`
  advanced popover + cancel × + **double-click collapse** (180ms
  height animation)
- `FloatingNumericInput` Slint component + `numeric_input.rs` —
  end-to-end across Move / Scale / Rotate. Parser handles units
  (`m / cm / mm / km / in / ft / yd / deg / ° / rad`), relative
  (`+N`), and expressions (`=2+3*sin(30deg)` + `=other.x`)
- `ModalTool` trait + `ActiveModalTool` + `ModalToolRegistry` +
  `ToolOptionsBarState` reflection
- `GhostPreviewMaterial` with 1.5 Hz α 0.30↔0.50 pulse
- Ribbon active-state brand-teal glow + **140ms underline slide
  animation** between tabs + top panel hairline
- Unified Esc / RMB / button-again cancel via `CancelModalToolEvent`
- `Alt+<letter>` accelerators + auto-exit-to-Select after commit
- **`ToastUndo` Slint component** — top-center glass toast with
  inline Undo action, 5s auto-dismiss, 180ms fade-out, reads
  `UndoStack::last_label()` for contextual copy
- **Commit-success flash** — `CommitFlashState` resource, 150ms
  `accent-green-bright` border pulse on every `ModalToolCommittedEvent`
- `NotificationManager` toasts on every Smart Build Tool commit
- **Lucide icon directory** at `assets/icons/lucide/` with README
  manifest + import procedure
- **Telemetry opt-in scaffolding** — `ToolUsageCounters` +
  subscribers gated on `TelemetrySettings.enabled` (default false);
  explicit `flush_counters_to_disk()` — no auto-phone-home

**Partial — infrastructure ready, final wiring pending**
- Custom cursors (6 badge SVGs drawn, blocked on Slint exposing
  OS image-cursor binding)
- Backdrop blur (intent token declared, GPU filter pipe pending in
  the Slint renderer)
- Prefers-reduced-motion OS gate (Theme token + multiplier exist;
  OS detection + Settings toggle deferred per user directive)

**Deferred / skipped this increment (per user directive)**
- Hand-drawn Eustress-specific icons (Workshop / MCP / Rune / Forge
  / Bliss / V-Cell)
- ViewCube redesign
- User-customizable accent color
- Custom user-saved Options Bar layouts per tool
- High-contrast theme variant

**Blocked upstream**
- Screen-reader ARIA (Slint accessibility API)
- AI-suggested Options Bar defaults (embedvec)

## 10. Implementation Phases

### Phase 0 — UX Foundation (blocks every Smart Build Tool in TOOLSET.md)

Nothing in TOOLSET.md Phase 0 ships cleanly without these.

- [x] Palette refinement in `theme.slint` (5 new tokens: accent-cyan, accent-eustress, accent-green-bright, panel-glass, border-highlight, shadow-float) *(shipped)*
- [x] `IconButton` centering fix *(shipped: outer `VerticalLayout { alignment: center }`)*
- [x] Icon library adoption (Lucide bulk import to `assets/icons/lucide/`) *(shipped: directory created at [eustress/crates/engine/assets/icons/lucide/](../../eustress/crates/engine/assets/icons/lucide/) + [README.md](../../eustress/crates/engine/assets/icons/lucide/README.md) documenting the style contract, currently-used manifest, and bulk-import procedure. 14 hand-drawn ribbon SVGs + 6 cursor-badge SVGs already follow the style; new icons drop into this directory per the procedure)*
- [x] `ToolOptionsBar` component (Slint) *(shipped with flat-union `ToolOptionControlData` { kind: number/bool/choice/label } + inline controls + `⋯` advanced popover + cancel ×)*
- [x] `FloatingNumericInput` component (Slint + Rust controller) *(shipped: `floating_numeric_input.slint` + `numeric_input.rs` + Move / Scale / Rotate finalize systems)*
- [x] `ToastUndo` component (Slint + Rust controller) *(shipped: [toast_undo.slint](../../eustress/crates/engine/ui/slint/toast_undo.slint) + [toast_undo.rs](../../eustress/crates/engine/src/toast_undo.rs). Top-center glass toast with inline Undo button + dismiss ×. Surfaces automatically on any labeled commit via `surface_toast_on_labeled_commit` subscribing to `ModalToolCommittedEvent` + reading `UndoStack::last_label()`. 5-second auto-dismiss with 180ms fade-out; hover pauses. Undo button fires the same `UndoEvent` Ctrl+Z does)*
- [x] `ModalTool` trait + `ActiveModalTool` resource (Rust) *(shipped: trait + `ActiveModalTool` + `ModalToolRegistry` + `ToolOptionsBarState` + `run_active_modal_tool` system in `modal_tool.rs`)*
- [x] `GhostPreviewMaterial` in `AdornmentMaterials` *(shipped: `ghost_preview` + `ghost_preview_outline` + `pulse_ghost_preview_alpha` @ 1.5 Hz, α 0.30 ↔ 0.50)*
- [x] Ribbon active-state glow (Theme + main.slint) *(shipped: brand-teal accent on active tool / tab)*
- [x] Ribbon tab underline slide animation *(shipped: [ribbon.slint](../../eustress/crates/engine/ui/slint/ribbon.slint) — 2px `accent-eustress` bar at the bottom of each tab, width + x animate over 140ms `ease-in-out` when `ribbon-tab` changes. Font-weight transition also 140ms for smooth active-state feel)*
- [~] Custom cursor per tool (Slint `mouse-cursor`) — **workaround shipped 2026-04-22**: in-viewport cursor-follower via [cursor_badge.rs](../../eustress/crates/engine/src/cursor_badge.rs) + [cursor_badge.slint](../../eustress/crates/engine/ui/slint/cursor_badge.slint). Renders the active tool's 16×16 badge SVG at the OS-cursor position (offset +10/+12 px) so it sits adjacent to the system cursor without covering it. Maps 6 tool ids → their shipped badge assets; auto-hides when the cursor leaves viewport bounds or no ModalTool is active. Not pixel-identical to an OS custom cursor — swap to the real thing when Slint exposes `winit` image-cursor binding upstream
- [x] Top hairline on all floating panels *(shipped via `border-highlight #ffffff10`)*
- [x] Commit-success flash (150ms `accent-green-bright` border pulse on successful commit) *(shipped: [commit_flash.rs](../../eustress/crates/engine/src/commit_flash.rs) — `CommitFlashState.progress` resource, set to 1.0 on every `ModalToolCommittedEvent`, linearly decays over 150ms. Slint renders a 2px `accent-green-bright` border pulse anchored to the ToolOptionsBar with `opacity = progress`)*
- [x] `Esc` / RMB / button-click-again → unified cancel (routed through `ActiveModalTool`) *(shipped via `CancelModalToolEvent`)*
- [x] `Alt+<letter>` accelerator rendering (underline in label) *(shipped)*
- [x] Auto-exit-to-Select after commit (opt-out per tool) *(shipped: `ModalTool::auto_exit_on_commit`, forces `StudioState.current_tool = Tool::Select`)*

### Phase 1 — UX Polish

- [x] Advanced `⋯` popover for every tool *(shipped: `ToolOptionsBar` in [tool_options_bar.slint](../../eustress/crates/engine/ui/slint/tool_options_bar.slint) — `advanced-expanded` property, controls with `advanced: true` render only inside the expander; `⋯` button toggles it. Active across every ModalTool's `options()` output)*
- [~] Backdrop blur on floating overlays (where GPU supports) *(intent token `backdrop-blur-lg: 12px` declared in [theme.slint](../../eustress/crates/engine/ui/slint/theme.slint); components approximate via `panel-glass` 91% opacity today. GPU backdrop-filter wiring lands when the Slint renderer exposes it — the design token is in place so every floating overlay adopts centrally once it does)*
- [x] Panel enter/exit animations (180ms slide+fade) *(shipped: `Theme.panel-anim-duration: 180ms` + `motion-scale` multiplier; applied to `ToolOptionsBar` height + opacity transitions. Other overlays inherit the same token when they add animate blocks)*
- [x] Progressive disclosure of advanced numeric features (`+5`, `=foo.x`) *(shipped via Phase 1 + Phase 2 expression-input work in [numeric_input.rs](../../eustress/crates/engine/src/numeric_input.rs) — `+N` relative, unit suffixes, `=`-prefixed expression mode with math functions + `=other.x` property refs. Advanced features surface only when the user reaches for them, which is the progressive-disclosure intent)*
- [x] Telemetry opt-in scaffolding *(shipped: [telemetry.rs](../../eustress/crates/engine/src/telemetry.rs) extended with `ToolUsageCounters` resource (per-tool activation / commit / cancel counts + `⋯` expansion counter), subscriber systems gated on `TelemetrySettings.enabled` (default `false`). `flush_counters_to_disk(space_root, settings, counters)` writes a JSON snapshot to `.eustress/telemetry.json` — explicit call only, never automatic. No content data, only counters)*
- [~] `prefers-reduced-motion` OS integration *(partial: `Theme.reduce-motion` Slint input + `motion-scale` multiplier declared in [theme.slint](../../eustress/crates/engine/ui/slint/theme.slint); components multiply animation durations by `motion-scale` so flipping the Theme property collapses motion globally. OS-level detection + settings-panel toggle skipped per user directive — the hook is there for any future Settings UI to flip)*
- [x] Toast notification system wired to all Smart Build Tool commits *(shipped: [`announce_modal_tool_commits`](../../eustress/crates/engine/src/tools_smart.rs) subscribes to `ModalToolCommittedEvent` and emits per-tool toasts via `NotificationManager` — covers all 6 Smart Build Tools + 3 Array tools)*
- [x] Double-click ToolOptionsBar → collapse to 24px strip *(shipped in [tool_options_bar.slint](../../eustress/crates/engine/ui/slint/tool_options_bar.slint): `collapsed` in-out property, background `TouchArea` with `double-clicked` handler, 180ms ease-in-out height animation. Collapses the bar to a name + step-label strip so users who want the viewport unobstructed can reclaim the pixels)*

### Phase 2 — UX Differentiators

- [x] Rune expression input in FloatingNumericInput (`=size.x * 2`) *(shipped: [`numeric_input.rs::eval_expression`](../../eustress/crates/engine/src/numeric_input.rs) handles the expression syntax natively. Not technically Rune — we ship a hand-rolled recursive-descent evaluator so there's no Rune sandbox dep — but the user-facing capability (`=pi`, `=sqrt(2)`, `=other.x*2`) is all there)*
- [~] High-contrast theme variant *(skipped this increment per user directive — deferred. Token swap surface is clear: a parallel `Theme.high-contrast: bool` input + alternate color outputs keyed off it would land in `theme.slint` when prioritised)*
- [~] Screen-reader ARIA labels *(scaffold shipped 2026-04-22: [accessibility.rs](../../eustress/crates/engine/src/accessibility.rs) — `AccessibilityManifest` Rust resource with `AccessibleRole` enum (18 role variants) + `declare()` + `declare_full(description, shortcut)` APIs. `seed_core_labels` populates ~30 core UI entries at startup (viewport tools, ribbon CAD tab, bottom-panel tabs, ToolOptionsBar, ToastUndo, Timeline). `apply_to_slint_window(manifest)` is the single integration site — currently logs + no-ops; when Slint ships `accessible-role` + `accessible-label` binding upstream, that one function lifts the manifest into the live a11y tree without touching feature code)*
- [~] Custom user-saved Options Bar layouts per tool *(skipped this increment per user directive — deferred indefinitely. Infrastructure lead: would need a `ToolOptionsLayoutOverride` resource + per-tool-id persistence in `.eustress/tool_layouts.toml` + a drag-to-reorder pass on `ToolOptionsBar`. Not blocking Phase 2 feature work)*
- [~] AI-suggested Options Bar defaults based on selection (`Gap Fill` auto-sets thickness from similar recent fills) — **blocked on embedvec integration** (same blocker as AI Select Similar in TOOLSET.md Phase 2). Hook lands when embedvec ships
- [~] Hand-drawn Eustress-specific icon overrides (Workshop, MCP, Rune, Forge, Bliss, V-Cell) *(skipped this increment per user directive — deferred. Directory + manifest ready at `assets/icons/lucide/`; custom icons drop in alongside once illustration work happens)*
- [~] ViewCube redesign with Eustress brand teal accents *(deferred — the existing axis gizmo at bottom-right is the functional equivalent today; a full ViewCube redesign requires visual iteration that doesn't ship well without live render-loop validation. Brand-teal accent integration lands with whatever Settings or onboarding UI ships first)*
- [~] User-customizable accent color (within a curated palette) *(skipped this increment per user directive — deferred. Would land as `Theme.accent-override: color` input + Settings picker constrained to palette entries; current `accent-eustress` / `accent-blue` / `accent-cyan` stay as the brand defaults)*

## 11. Unified Phase Plan — Dependencies Across TOOLSET Docs

One master schedule across `TOOLSET.md`, `TOOLSET_CAD.md`, and this
doc. Resolves the case where a Phase-0 item in one doc depends on a
Phase-0 item in another.

### Critical-path Phase 0 (nothing downstream works until these land)

1. **UX foundation block** (this doc §10 Phase 0)
   - Theme palette refinement, `IconButton` fix, `ToolOptionsBar`,
     `FloatingNumericInput`, `ModalTool` trait, `GhostPreviewMaterial`,
     cursor badges, ribbon active state.
2. **Group handle root + Local/World wiring + min-size floor**
   (TOOLSET.md §4.2 — gizmo P0)
3. **Select Children / Select Descendants + Explorer context menu**
   (TOOLSET.md Phase 0)
4. **Smart Build Tools — Gap Fill, Resize Align, Edge Align,
    Part Swap, Model Reflect destructive, Align & Distribute panel**
   (TOOLSET.md §4.13 — requires #1 + #2)
5. **CAD Foundation — Quantity type, feature tree loader, sketch
    solver, Extrude/Revolve/Fillet/Chamfer, Reference geometry,
    parametric variables, hot reload** (TOOLSET_CAD.md Phase 0 — requires #1)

Items 2-5 can proceed in parallel once #1 is done. Within each track
items are listed in dependency order.

### Phase 1 — layered on top

After every Phase-0 item ships:

- **UX polish**: `⋯` popover, backdrop blur, telemetry, toast system
- **Editor polish**: pivot modes, smart guides, vertex snap, arrays, mirror, named undo, selection sets, unit-aware input
- **Smart tool extensions**: Material Flip, Model Reflect linked, Part to Terrain, parametric Gap Fill / Resize Align, AI template suggest
- **CAD production**: Sweep/Loft/Helix, Shell, Hole Wizard, Push/Pull, assembly mates, Joint → Motor6D, AI-inferred constraints, STEP export, BOM, Forge parametric handoff, co-edit CRDT

### Phase 2 — differentiators

- **UX differentiators**: expression input, high-contrast, custom layouts, AI-suggested defaults, Eustress-specific icons, ViewCube redesign
- **Editor differentiators**: AI Select Similar, lasso/paint select, mesh-edit mode, extrude/bevel/inset/loop cut, fillet/chamfer, mass readout, animation timeline, scripted tool authoring via Rune, Loop Subdivider, Constraint Editor, Attachment Editor, Scale Lock, Bulk Import
- **CAD differentiators**: variable-radius fillet, draft, rib, thread, surface suite, 3D sketch, sheet metal, weldments, technical drawings, PDF/DXF/DWG export, advanced joints (cylindrical/universal/gear), tolerance stacks, branching + parametric-aware git diff, IGES export, CAM toolpath

### Dependency graph (tl;dr)

```
UX Phase 0 (theme + ToolOptionsBar + ModalTool + GhostPreview + ribbon state)
       ├── Editor Phase 0 (gizmo group root, space toggle, snap, plane handles)
       │        ├── Editor Phase 1 (pivot modes, smart guides, vertex snap, …)
       │        └── Editor Phase 2 (mesh edit, animation, AI Select Similar, …)
       │
       ├── Smart Build Phase 0 (Gap Fill, Resize Align, Edge Align, Part Swap, Mirror)
       │        ├── Smart Build Phase 1 (Material Flip, linked Mirror, Part→Terrain)
       │        └── Parametric Smart tools (requires CAD Phase 0)
       │
       └── CAD Phase 0 (Quantity, feature tree, sketch solver, extrude/revolve/fillet)
                ├── CAD Phase 1 (sweep/loft/shell, assembly mates, AI constraints, STEP)
                └── CAD Phase 2 (surfaces, sheet metal, drawings, CAM, branching)
```

Every leaf-and-branch is independently deliverable; the arrows only
block downstream work when an upstream item's interface hasn't landed.

## 12. Before-and-After — Concrete Wins per Phase 0 Item

| Item                         | Today                                       | After Phase 0 ships                        |
|------------------------------|---------------------------------------------|--------------------------------------------|
| Activate Move tool           | Click button, arrows appear                 | Click button, arrows + cursor badge + Options Bar + tab glow |
| Home tab icons               | Drift off-center                            | Crisp centered grid, consistent 22px       |
| Type 2.5 during drag         | No effect                                   | Floating input appears, commits exactly 2.5 |
| Gap in roof corner           | 30–60s of manual cube placement             | Gap Fill in ≤3s (2 clicks + Enter)         |
| Pillar to angled ceiling     | 20–40s of eyeball + measure                 | Resize Align in ≤2s (2 clicks)             |
| Swap 20 placeholder windows  | 20× manual drag + replace                   | Part Swap across selection in ≤5s          |
| Mirror a vehicle chassis     | ~5 minutes rebuild                          | Model Reflect in ≤2s (1 plane pick)        |
| Undo a 324-part mirror       | 324× Ctrl+Z with no context                 | One Ctrl+Z, labelled "Model Reflect 324 parts" |
| Destructive op on 500 parts  | Hidden until it's done, no recovery signal  | Toast "Mirrored 500 parts [Undo Ctrl+Z]"   |
| Tool state hiding in keyboard-only mode | Invisible — must look at cursor    | Focus ring + accelerator underline visible |

Each row is a measurable improvement. Each one is blocked on the Phase-0
UX foundation. Ship the foundation; every downstream feature carries
its own celebration.

---

*Last updated: 2026-04-22 — Phase 0 + 1 + 2 UX polish pass complete.
This doc is the single source of truth for
visual and interaction patterns; update it when a component ships or
a token value changes. Every other doc in this series (TOOLSET.md,
TOOLSET_CAD.md) assumes the patterns here.*
