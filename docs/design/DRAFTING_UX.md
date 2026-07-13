# Drafting UX — The Finished Experience, Worked Backwards

**Status:** ACTIVE design doctrine for the Drafting/CAD surface (2026-07-10).
Companion to `docs/architecture/CAD_PLATFORM_PLAN.md` (which owns the kernel
phases). This document owns *how drafting feels*. UI changes to the Drafting
tab, Model tab, or any CAD tool must conform to it or change it first.

---

## 1. The finished session (narrative spec)

This is the target. Every design decision below is derived by working
backwards from this five-minute session:

1. **Insert.** Model tab → Parts. The user clicks *Plate*. A parametric plate
   lands at the camera-forward point, selected, gizmos live. It behaves like
   any Part: move it, rotate it, resize it — with the scale gizmo or the
   Properties Size field, in whatever unit the status bar says. There is no
   visible "CAD taxonomy": no special property categories, no second way to
   express size. **A CadPart is a Part.**

2. **Enter Sketch.** The user double-clicks the plate (or presses the Sketch
   ribbon button with it selected). The Studio enters the **Sketch
   environment** at the *contextual-tab* tier: a **Sketch tab** appears
   alongside the normal tabs and activates — it does not hide them, and
   invoking a modeling operation mid-sketch auto-finishes the sketch first
   (Fusion's escalation model: contextual *tab* for sketching, full
   contextual *environment* reserved for heavier modes). The camera eases to
   look squarely down the sketch plane. A single, always-visible **Finish
   Sketch** button sits at the right end of the Sketch tab. Everything about
   this state says: *you are inside a sketch; here is the one door out.*

3. **Draw and constrain.** Sketch-tab groups: **Create** (Line, Rectangle,
   Circle, Arc, Point), **Constrain** (Horizontal, Vertical, Perpendicular,
   Coincident, Equal, Fix), **Dimension**, **Inspect**. The user clicks Line
   and clicks twice in the viewport — entities are created by drawing, not by
   editing TOML. Constraints are **verb-then-target**: click Perpendicular,
   the tool arms, then click the two entities in the viewport; the tool
   stays armed for repeated application until Esc. Every applied constraint
   leaves a small persistent **glyph** beside the geometry — the sketch
   itself shows what constrains it, without opening any inspector.

   Sketch geometry carries a fixed, memorize-once **state palette**
   (dark-theme mapping of Fusion's convention):

   | State                | Rendering                          |
   |----------------------|------------------------------------|
   | Free / unconstrained | `cat-parts` blue                   |
   | Construction         | dashed, amber                      |
   | Fixed (Fix applied)  | green                              |
   | Fully constrained    | `text-primary` white (max contrast)|

   A **solve-status chip** near the ribbon doubles the signal for
   accessibility: `● Fully constrained` (green) / `◐ 4 DOF free` (amber) /
   `✗ Over-constrained` (red) — always in the same place. The solver runs on
   every change; the user never presses a "Solve" button in the common path.
   The taught verification is interactive, not visual: *if you can still
   drag it, it isn't fully constrained.*

4. **Feature.** Finish Sketch → the plate regenerates. Features (Extrude
   depth changes, Holes, Shell) are edited by **direct manipulation** where
   possible — drag an arrow, type into the floating numeric input that
   already serves the move/scale tools — with the same commit (Enter/click)
   and cancel (Esc) grammar as every other tool.

5. **History.** The bottom **Timeline panel** (the existing panel, not a new
   one) shows the part's feature history as chips: `[Sketch1] [Extrude1]
   [Hole1]`. Right-click a chip → Edit / Suppress / Delete. Drag to reorder.
   This is where feature-tree interaction lives — never in the Properties
   panel. Properties stays a Part's properties.

6. **Assemble.** Model tab → Constraints. Fixed / Hinge / Slide / Ball sit
   beside Attach / Weld / Motor / Beam, in the same amber family, because to
   the user they are the same *kind of thing*: relationships between parts.
   Clicking one arms a pick tool that speaks through the standard tool
   options bar: "click first part", "click second part", commit on second
   pick, Esc cancels. Identical lifecycle to Gap Fill or Part Swap.

   Semantics note (Fusion's Joint vs As-Built Joint split): our mates are
   **as-built** — they record the relationship between parts *where they
   already stand* and never repostion anything, so pick order carries no
   hidden meaning. A positioning joint command (pick origin on A, pick
   origin on B, A moves to B, motion preview before commit) is a distinct
   future command, not a mode flag on these.

7. **Export.** GLB from the Parts group or File menu. Toast confirms with
   the triangle count and path.

## 2. Design laws

These are binding on the Model tab, Drafting tab, and every CAD tool.

### Law 1 — One panel chrome
Every floating panel and dialog is built from one shared `PanelChrome`
component: icon-optional title row (13px/600), optional status dot, and a
**close ×, 24px hit target, always top-right**. One background token, one
radius token, one border token, one padding scale. No panel invents its own
header. Esc closes the topmost floating panel — unless a tool drag is in
progress, in which case Esc cancels the drag first (strict ordering, one
owner per keypress).

### Law 2 — One tool lifecycle
Every tool that needs more than a single click speaks through the **tool
options bar / ARMED chip**, and only through it: armed state, step hint,
options. Commit = Enter or the tool's natural final click. Cancel = Esc or
right-click. No tool ships its own hint card, legend, or bespoke floating
instructions. (Ribbon help cards are banned; discoverability lives in
tooltips and the options-bar step hints.)

Additionally, for commands with numeric inputs:
- **Field ↔ manipulator concurrency.** Every numeric field a command
  exposes has a live viewport manipulator, and both write the same value in
  real time — dragging the handle updates the field, typing updates the
  handle. Neither waits for the other.
- **Preview until OK.** Nothing touches persistent state (feature tree,
  disk, undo stack) until the explicit commit; Esc reverts completely. No
  partial-apply states.
- **Selection precedes configuration; options are staged.** A command's
  panel reveals only the fields relevant to the current type/mode choice —
  never a flat twenty-field sheet.

### Law 3 — Color is semantic, and there are few of them
Tints encode *category*, centrally defined in `theme.slint` — never picked
per-button:

| Token            | Meaning                                      | Value     |
|------------------|----------------------------------------------|-----------|
| `cat-parts`      | Creates geometry (primitives AND parametric) | `#64b5f6` |
| `cat-structure`  | Containers/organization                      | `#81c784` |
| `cat-constraint` | Relationships between things                 | `#ffb74d` |
| `cat-modify`     | Patterns/arrays/bulk edits                   | `#ba68c8` |
| `cat-data`       | Import/export/IO                             | `#4dd0e1` |
| `accent-cyan`    | Selection, armed state, focus                | `#00bcd4` |

Within-group uniformity beats sub-category distinctions: a ToolGroup renders
ONE hue. Parametric parts are parts.

Red is reserved for destructive actions and errors. Solve states use
green/amber/red exclusively for constraint health. Any new button picks an
existing token; adding a color means amending this table.

### Law 4 — Ribbon grammar
A tab is a *context*, not a category dump. A group holds at most ~6 buttons
and is named with a noun. The Drafting tab is the **selection-operations
context** (Smart Edit, Align, Pattern, Boolean — things you do *to selected
parts*); the Model tab is the **creation context** (Parts, Structure,
Constraints, Effects); the Sketch tab is contextual and appears only inside
the sketch environment. Tab content is horizontally centered. Nothing
appears in two tabs.

### Law 5 — Units are ambient
Every length the user sees or types is in the status-bar display unit.
Meters exist only in code and on disk. A label that hardcodes a unit is a
bug.

### Law 6 — Feedback is uniform
Success/warn/error land as toasts with consistent phrasing (verb-first,
includes the object: "Exported CadPlate.glb — 128 tris"). The output console
is a log, not a primary channel. Solver/kernel status is a chip, not prose.

## 3. What this kills

- The Properties panel's CAD/FeatureTree pseudo-categories (history lives in
  the Timeline; size lives in Size).
- Per-tool hint/legend cards in the ribbon.
- Ad-hoc tint choices and duplicate-label buttons ("Weld" twice).
- Separate "CAD variables" as a user-visible concept for envelope
  dimensions. Feature-specific inputs (hole diameter, wall thickness) are
  edited where the feature is edited: its Timeline chip → Edit.

## 4. Increments

- **UX-0 (now):** Unified chrome + semantics. `PanelChrome` extracted and
  adopted by the sketch panel and numeric input; tint tokens centralized;
  ribbon regrouped (done); help cards removed; Esc ordering enforced;
  Size-driven resize for all templates (done).
- **UX-1:** Sketch environment v1 — contextual Sketch tab, viewport
  entity picking (click sketch lines in the 3D view, not list rows),
  continuous solve with the status chip, Finish Sketch flow. Timeline panel
  CAD lane with Edit/Suppress/Delete/reorder chips. DisplayUnit conversion
  plumbed through ModalTool numeric options (labels are truthful "m" until
  then). CSG booleans record undo and adopt the labeled Undo toast that
  modal-tool commits already get. Studio-wide chrome debts from the UI
  audit (shared DialogTitleBar for the seven copy-pasted dialogs, one
  DropdownPopup shell for the twelve ribbon popups, Esc-to-close on
  centered modals) land here as the panel-system pass widens beyond the
  drafting scope.
- **UX-2:** Draw-in-viewport (Line/Rect/Circle placement on the sketch
  plane), dimension placement, direct-manipulation Extrude arrow.
- **UX-3:** Joint origin snapping for mates (Fusion-style pick points with
  isolatable motion preview before commit), keyword command search (S-key
  box — the parallel fast path that scales with command count). A radial
  marking menu is explicitly deferred: per-context slot curation and gesture
  recognition only pay off at Autodesk's feature count and user base.
