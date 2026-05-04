# Eustress Toolset — Scope & Competitive Positioning

> Sibling docs: [ADORNMENT_ARCHITECTURE.md](ADORNMENT_ARCHITECTURE.md) (the
> mesh-based handle system underneath every gizmo described here),
> [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md) (honest snapshot of
> what works today).

## 1. Positioning Statement

Eustress is a **simulation-first, AI-native** spatial editor. The toolset
has to beat:

- **Roblox Studio** on ergonomics and multi-user building
- **Unity / Unreal** on procedural content authoring
- **Blender** on scriptability and keyboard-driven speed
- **Maya / 3ds Max** on selection / transform fidelity
- **SolidWorks / Fusion 360** on precision, constraints, and parametric
  feel (within the bounds of a game editor, not a full MCAD suite)

We are not trying to replace MCAD or a DCC application. We are trying to
make the **fastest, most precise, most scriptable simulation-world editor
ever built**. Everything downstream of this doc follows from that.

The reference test: *a senior technical artist moving to Eustress from
any of the tools above should lose zero productivity on day one — and
notice concrete wins within the first hour.*

## 2. Design Principles (the lines we don't cross)

1. **Direct manipulation first, panels second.** Every common
   transformation is reachable via viewport gesture + keyboard. Panels
   are for settings, not for tasks.
2. **Numeric precision is a first-class input, not an afterthought.**
   Every drag accepts live numeric input (`2.5<Tab>45<Tab>90<Enter>`).
3. **Snap is orthogonal to tool.** Grid, vertex, edge, face, angle, and
   proximity snap are toggleable independent of which tool is active.
4. **Multi-selection is a single logical operation.** One handle set,
   one pivot, one undoable action — never N discrete operations.
5. **Undo records intent, not diffs.** "Move 5 parts by (0, 2, 0)"
   reads better than "Transform entity 1234 from A to B" ×5.
6. **The gizmo you see is the geometry you grab.** Mesh-based handles
   (see `ADORNMENT_ARCHITECTURE.md`) — no immediate-mode overlays that
   can disagree with hit detection.
7. **Every tool is scriptable.** Anything the UI does is callable from
   Rune, MCP, and the Engine Bridge. No "editor-only" functionality.
8. **Everything is ECS queryable.** Tools mutate `Instance`, `BasePart`,
   `Transform`, etc. — never private tool state that the rest of the
   engine can't see.
9. **Physics-aware when you want it, off when you don't.** Dragging can
   collide, snap-to-surface, or pass through; toggled per-tool.
10. **Simulation runs during editing.** Soft-paused sim still ticks;
    tools respect physics constraints, attachments, welds.

## 3. Competitive Benchmark Matrix

Legend: ✓ first-class, ● partial / awkward, ✗ absent, — not applicable.

### Selection

| Capability                             | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Click to select                        | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Box / rubber-band select               | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Lasso select                           | ✗      | ✗     | ✗      | ✓       | ✓    | ✗   | ✓ P2                |
| Paint select (brush)                   | ✗      | ✗     | ✗      | ✓       | ✓    | ✗   | ✓ P2                |
| Shift-extend / Ctrl-toggle             | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Alt-select individual (bypass parent)  | ✗      | ✗     | ●      | ●       | ✓    | ✓   | ✓ (exists)          |
| Select similar (same class/material)   | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Select children (1 level)              | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✗ P0                |
| Select descendants (recursive)         | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✗ P0                |
| Select parent                          | ✗      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Select all of class/tag                | ●      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Select by visual similarity (AI)       | ✗      | ✗     | ✗      | ✗       | ✗    | ✗   | ✓ P2 (Eustress edge)|
| Inverse selection                      | ✗      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Selection sets (named, saveable)       | ✗      | ✓     | ✓      | ✗       | ✓    | ✓   | ✓ P2                |
| Selection history (prev/next)          | ✗      | ✗     | ●      | ✓       | ✓    | ✗   | ✓ P2                |
| Filter by component/class              | ●      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |

### Transform Gizmos

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Move (3-axis + 3-plane)                 | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ● P0 (arrows exist, plane handles missing) |
| Scale (6-face + uniform center)         | ✓      | ✓     | ✓      | ✓       | ✓    | ●   | ● P0 (mesh-based exists, needs polish) |
| Rotate (3 arcs + free rotate sphere)    | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ● P0 (rings exist, no free-rotate) |
| Combined transform gizmo                | ✗      | ●     | ✓      | ✓       | ✓    | ✗   | ✓ P2                |
| Local vs World space toggle             | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✗ P0 (UI exists, unwired) |
| Normal / gimbal / view space            | ✗      | ✓     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Pivot: median / active / individual     | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P1                |
| Pivot: 3D cursor (user-placed)          | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Pivot: bounding box face / corner       | ✗      | ✓     | ✓      | ✗       | ●    | ✓   | ✓ P1                |
| Group transform (single gizmo per N)    | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✗ P0                |
| Camera-distance scaled handles          | ✓      | ✓     | ✓      | ✓       | ✓    | ●   | ✓ (Move only — Scale/Rotate P0) |
| Always-on-top handles                   | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (done)            |
| Handle hover-highlight                  | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ● P0 (Move has it)  |
| Handle drag-color                       | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ● P0 (Move has it)  |
| Numeric input during drag               | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P1 (differentiator)|
| Escape cancels in-progress transform    | ✗      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Right-click cancels in-progress         | ✗      | ✓     | ✗      | ✓       | ✓    | ✓   | ✓ P1                |

### Snap

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| World-grid snap (position)              | ✓      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Angular snap (15°/45°/90° …)            | ✓      | ●     | ✓      | ✓       | ✓    | ✓   | ✗ P0                |
| Scale snap (fixed increments)           | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Vertex snap (to another object's vert)  | ✗      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Edge midpoint snap                      | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P2                |
| Face-center snap                        | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P2                |
| Surface snap (drop onto mesh)           | ✓      | ●     | ✓      | ✓       | ✓    | ●   | ✓ (exists in move)  |
| Align-to-normal on drop                 | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Smart guides (alignment to siblings)    | ✗      | ✗     | ✗      | ●       | ●    | ✓   | ✓ P1 (see ADORNMENT) |
| Proximity snap (ghost preview)          | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P1                |
| Snap increment tied to zoom             | ✗      | ✗     | ✗      | ✓       | ●    | ✗   | ✓ P2                |
| Temporary snap override (hold Ctrl)     | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P0                |

### Numeric / Precision Input

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Property panel numeric input            | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Numeric drag-slider on property         | ●      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |
| Live numeric input DURING gizmo drag    | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P1 (big win)      |
| Type axis + value (`X 2.5 <Enter>`)     | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Relative (`+2.5`) and absolute          | ✗      | ●     | ●      | ✓       | ✓    | ✓   | ✓ P1                |
| Unit-aware input (`2.5m`, `90deg`)      | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P2                |
| Expression input (`(2+3)*sin(30)`)      | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Reference another property (`=foo.x`)   | ✗      | ✗     | ✗      | ●       | ✓    | ✓   | ✓ P2 (Rune hook)    |

### Alignment & Distribution

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Align edges (min / center / max)        | ✗      | ●     | ●      | ✓       | ✓    | ✓   | ✓ P1                |
| Distribute evenly                       | ✗      | ●     | ●      | ✓       | ✓    | ✓   | ✓ P1                |
| Align to active object                  | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P1                |
| Align to surface normal                 | ✗      | ●     | ●      | ✓       | ✓    | ✓   | ✓ P1                |
| Snap to construction plane              | ✗      | ✗     | ✗      | ●       | ●    | ✓   | ✓ P2                |

### Duplicate / Pattern

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Duplicate (Ctrl+D)                      | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Duplicate & place (live drag)           | ●      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P1                |
| Linear array (N copies, step vector)    | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P1                |
| Radial array (around pivot)             | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P1                |
| Path array (along curve)                | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P2                |
| Grid array (2D/3D)                      | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Mirror (with / without instancing)      | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |

### Mesh / Geometry Editing

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Primitive creation (cube/sphere/etc.)   | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Vertex / edge / face select mode        | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P2                |
| Extrude / bevel / inset                 | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P2                |
| Loop cut                                | ✗      | ✗     | ✗      | ✓       | ✓    | ✗   | ✓ P2                |
| Boolean union / difference / intersect  | ●      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P1 (CSG exists)   |
| Fillet / chamfer edges                  | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2 (CAD requirement)|
| Subdivision surface                     | ✗      | ✗     | ●      | ✓       | ✓    | ●   | ✓ P2                |
| Mirror modifier (non-destructive)       | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Solidify / shell (thickness)            | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Retopology tools                        | ✗      | ✗     | ✗      | ✓       | ✓    | ✗   | ✗ (use Blender)     |
| Sculpt mode                             | ✗      | ✗     | ●      | ✓       | ✓    | ✗   | ✗ (use Blender)     |

### Measurement & Analysis

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Ruler / distance measure                | ✗      | ●     | ✓      | ✓       | ✓    | ✓   | ✓ P1 (have MCP tool)|
| Angle measure                           | ✗      | ✗     | ●      | ✓       | ✓    | ✓   | ✓ P2                |
| Surface area readout                    | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Volume readout                          | ✗      | ✗     | ✗      | ✓       | ✓    | ✓   | ✓ P2                |
| Mass / center-of-mass readout           | ✗      | ✗     | ●      | ●       | ✓    | ✓   | ✓ P2 (realism hook) |
| Live dimension annotations              | ✗      | ✗     | ✗      | ●       | ✓    | ✓   | ✓ P2                |
| Bounding-box readout on selection       | ✗      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ P1                |

### Camera Control

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Orbit / pan / zoom                      | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Frame selection (.)                     | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Numpad axis views (Top/Front/Side)      | ✓      | ●     | ●      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Ortho / perspective toggle              | ●      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Walk / fly mode (WASD)                  | ✓      | ✓     | ✓      | ✓       | ✓    | ●   | ✓ (exists)          |
| Cinematic orbit / lock-to-target        | ✗      | ●     | ✓      | ✓       | ✓    | ✗   | ✓ P2                |
| Saved viewpoints                        | ●      | ●     | ✓      | ●       | ✓    | ✓   | ✓ P2                |
| Depth-of-field preview                  | ✗      | ✓     | ✓      | ✓       | ✓    | ●   | ✓ P2                |

### History & Versioning

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Linear undo/redo                        | ✓      | ✓     | ✓      | ✓       | ✓    | ✓   | ✓ (exists)          |
| Named operations in history             | ●      | ✓     | ✓      | ●       | ✓    | ✓   | ✓ P1                |
| Non-destructive feature tree (CAD)      | ✗      | ✗     | ✗      | ✓       | ✗    | ✓   | ✓ P2 (Forge hook)   |
| Branching history                       | ✗      | ✗     | ✗      | ✗       | ✗    | ✓   | ✗ (overkill)        |
| Scrub through history on timeline       | ✗      | ✗     | ●      | ✗       | ●    | ✓   | ✓ P2                |
| Per-project git integration             | ✗      | ✓     | ●      | ✗       | ✗    | ✗   | ✓ (Forge + Bliss)   |

### AI / Script Integration (Eustress differentiators)

| Capability                              | Roblox | Unity | Unreal | Blender | Maya | CAD | **Eustress target** |
|-----------------------------------------|:------:|:-----:|:------:|:-------:|:----:|:---:|:-------------------:|
| Natural-language tool commands          | ✗      | ●     | ✗      | ●       | ✗    | ✗   | ✓ P1 (Workshop has)|
| AI-generated geometry from prompt       | ✗      | ✗     | ✗      | ●       | ✗    | ✗   | ✓ (Workshop + MCP)  |
| AI-suggested edits in context           | ✗      | ✗     | ✗      | ✗       | ✗    | ✗   | ✓ P2 (unique)       |
| Live script preview (Rune REPL)         | ✗      | ●     | ●      | ✓       | ✓    | ●   | ✓ (exists)          |
| Tool automation via Rune                | ✗      | ✓     | ✓      | ✓       | ✓    | ●   | ✓ P1                |
| MCP tool surface (external IDE)         | ✗      | ✗     | ✗      | ✗       | ✗    | ✗   | ✓ (exists — unique) |
| Hot-reload world edits from git         | ✗      | ●     | ●      | ✗       | ✗    | ✗   | ✓ (exists)          |

## 3.5 Ribbon Organization

The ribbon splits tools across **tabs by workflow phase**, not by engine
subsystem. A user lives in exactly one tab for whatever they're doing
right now — no jumping between tabs mid-task. The Home tab never floods
because heavy workflows get their own tab.

### Tab layout (left → right)

| Tab            | Purpose                                              | When used                                       |
|----------------|------------------------------------------------------|-------------------------------------------------|
| **Home**       | Universal tools: select/move/scale/rotate, clipboard, history, space toggle, group/ungroup/lock, keys/soul/console utils | Every session, every tool invocation |
| **CAD**        | Parametric + smart-edit power tools (see §4.13 + [TOOLSET_CAD.md](TOOLSET_CAD.md)) | Dedicated building session |
| **Model**      | Primitive insertion, lighting inserts, effects, grouping | Laying down new geometry / entities |
| **UI**         | GUI components (ScreenGui / BillboardGui / SurfaceGui / Frame / TextLabel / Button) | Authoring HUDs and in-world UI |
| **Terrain**    | Terrain sculpt + paint                               | Natural environments                            |
| **Test**       | Play / pause / step, simulation controls             | Running and diagnosing the simulation           |
| **MindSpace**  | AI Workshop, MCP tool surface, embedvec search       | Natural-language edits, generative authoring    |
| **Plugins**    | Community + user-authored Rune-scripted tools        | Third-party extensions                          |

### Home tab sections (stable forever — don't add to this)

```
[ Clipboard ]   Paste | Duplicate | Delete | Cut | Copy
[ History   ]   Undo | Redo | History Panel
[ Tools     ]   Select | Move | Scale | Rotate
[ Space     ]   World | Local
[ Edit      ]   Group | Ungroup | Lock | Unlock | Anchor
[ Utils     ]   Keys | Soul | Console
```

These are what a Maya / Blender / SolidWorks muscle-memory user will
reach for without looking. Changing them is a breaking UX change.

### CAD tab sections (grows as §4.13 and TOOLSET_CAD.md land)

```
[ Smart Edit ]  Gap Fill | Resize Align | Edge Align | Part Swap
[ Align      ]  Distribute | Mirror | Align To Face | Align To Normal
[ Sketch     ]  New Sketch | Line | Rect | Circle | Arc | Spline | Dim | Constraint
[ Features   ]  Extrude | Revolve | Sweep | Loft | Hole | Helix
[ Modify     ]  Fillet | Chamfer | Shell | Draft | Push/Pull
[ Boolean    ]  Union | Difference | Intersect
[ Pattern    ]  Linear | Radial | Path | Mirror Body
[ Reference  ]  Plane | Axis | Point | Coord System
```

This is where the bulk of power-user time lives. Smart Edit ships in
Phase 0 (it's cheap and hugely productive); Sketch / Features / Modify /
Boolean are Phase 0 for parametric (see TOOLSET_CAD.md); Pattern / Reference
are Phase 1.

### Model tab sections

```
[ Parts      ]  Block | Sphere | Cylinder | Wedge | Cone | Truss
[ Advanced   ]  MeshPart | Union | NegativePart | CSG Preview
[ Lighting   ]  PointLight | SpotLight | DirectionalLight | SurfaceLight
[ Effects    ]  Attachment | ParticleEmitter | Trail | Beam | Sound
[ Grouping   ]  New Model | New Folder | Soul | Service
```

### UI tab sections

```
[ Containers ]  ScreenGui | BillboardGui | SurfaceGui | Frame | ScrollingFrame
[ Controls   ]  TextLabel | TextButton | TextBox | ImageLabel | ImageButton
[ Layout     ]  UIListLayout | UIGridLayout | UIAspectRatio | UIPadding
[ Style      ]  UIStroke | UICorner | UIGradient
```

### Keyboard equivalence

Every ribbon button has a keyboard binding. The ribbon is the discovery
surface; the keyboard is the daily driver. No ribbon button is reachable
*only* via mouse.

## 4. Tool Inventory (by category)

Every tool below maps to a concrete `ClassName` or `HandleAdornment` in
`ADORNMENT_ARCHITECTURE.md`, or a keyboard shortcut / Slint action. If a
tool has no counterpart, it lives in this doc and nowhere else until one
is built.

### 4.1 Selection Tools

#### Primary (Active today — P0 polish)
- **Click-select** — pick a Part/Model under cursor. ✓
- **Shift/Ctrl-extend** — multi-select toggle. ✓
- **Box-select** — drag rectangle on empty space. ✓
- **Alt-select** — bypass Model-parent grouping to pick the leaf. ✓
- **Deselect-all** — click empty or `Esc`. ✓

#### Structural (P0 — shipped)
- **Select Children** — add direct children of selection to selection. ✓
  *(shipped: `SelectChildrenEvent`, Ctrl+Shift+C)*
- **Select Descendants** — recursive walk, include all leaves. ✓
  *(shipped: `SelectDescendantsEvent` with BFS traversal, Ctrl+Shift+D)*
- **Select Parent** — ascend one level. ✓
  *(shipped: `SelectParentEvent`, Ctrl+Shift+U)*
- **Select Ancestors** — ascend all the way to Workspace. (P1)
- **Select Siblings** — everything sharing the same `ChildOf` parent. ✓
  *(shipped: `SelectSiblingsEvent`, Ctrl+Shift+S)*

#### Semantic (P1)
- **Select All of Class** — "all `Part`s", "all `ScreenGui`s".
- **Select All with Tag** — integrates with CollectionService tags.
- **Select by Material** — pick one part, grab all parts using that material.
- **Select by Script** — all parts owning a given `SoulScript`.
- **Inverse Selection** — flip. ✓ *(shipped: `InvertSelectionEvent`, Ctrl+I)*

#### Advanced (P2)
- **Lasso Select** — freehand polygon.
- **Paint Select** — brush over the scene, adds what the brush touches.
- **Select Similar (AI)** — embed all parts; pick one; select by
  cosine-similarity in `embedvec`.
- **Selection History** — `Alt+[` / `Alt+]` to walk previous/next
  selection sets (Maya pattern).
- **Named Selection Sets** — save + recall (Maya's "quick select sets").
- **Filter Panel** — live filter by class/material/script/size/color;
  matches participate in selection actions.

### 4.2 Transform Gizmos

See [ADORNMENT_ARCHITECTURE.md](ADORNMENT_ARCHITECTURE.md) for the
mesh-based handle implementation.

#### Move (P0 — shipped)
- 6 axis arrows (±X/±Y/±Z) — ✓ rendering, ✓ drag, ✓ hit detection
- 3 plane handles (XY/XZ/YZ) — ✓ shipped (`MovePlaneHandle`)
- Free-drag on part body — ✓
- Camera-distance-invariant handle size — ✓
- Always-on-top — ✓
- Hover/drag color — ✓

#### Rotate (P0 — shipped)
- 3 axis rings (X/Y/Z) — ✓ rendering, ✓ drag, ✓ hit detection
- View-facing rotate ring (screen-space rotation) — **missing** (P1)
- Free-rotate sphere (grab between rings) — ✓ center pivot sphere rendered
- Trackball rotate mode — **missing** (Blender parity, P2)
- Angular snap at 1°/5°/15°/45°/90° — ✓ shipped (`editor_settings.angle_snap`)
- Angle readout during drag — **missing** (P1)

#### Scale (P0 — shipped core)
- 6 face cubes (±X/±Y/±Z) — ✓ rendering + drag
- 1 center cube (uniform) — ✓ rendering + drag
- 12 edge cubes (2-axis scale) — **missing** (Maya / Blender parity, P2)
- 8 corner cubes (3-axis scale) — **missing** (box-scale from corner, P2)
- Scale snap increments — **missing** (P1)
- Negative scale handling — currently prevented; add toggle (P2)
- Non-uniform vs uniform (Shift modifier) — **missing** (P1)

#### Pivot Control (P1)
- Pivot: **Median** (center of selection AABB) — default for multi
- Pivot: **Active** (last-selected entity's origin)
- Pivot: **Individual** (each part transforms around its own origin)
- Pivot: **3D Cursor** (user-placed point, persists across tool switches)
- Pivot: **Bounding box face/corner** (pick via picker UI)
- Pivot keyboard shortcuts: `,` / `.` cycle pivot modes (Blender parity)

#### Space Toggle (P0 — shipped)
- **World** — handles axis-aligned to world ✓
- **Local** — handles aligned to the active entity's rotation ✓
  *(shipped via `move_tool::gizmo_rotation_for(mode, selected_rotations)`
  — shared by every hit-test + visual system so the gizmo you see is the
  axis you drag)*
- **Normal** — handles aligned to active face normal (mesh edit mode, P2)
- **View** — handles aligned to camera (screen-space, P2)
- **Gimbal** — show Euler-aligned rotation axes (Maya parity, P2)

**All three tools honor both spaces.** No "scale is always local" carve-out
— if the user picks World on a rotated part, Scale applies world-axis
scaling even if it produces shear. That's the user's choice; our job is
to make the math predictable, not to second-guess intent.

Both modes affect **both** the gizmo orientation *and* the drag math:

| Space | Gizmo orientation                     | Drag axis               |
|-------|---------------------------------------|-------------------------|
| World | Axis-aligned to world X/Y/Z           | Vec3::X / Y / Z literal |
| Local | Rotated by active entity's rotation   | `active.rotation × Vec3::X / Y / Z` |

The Move arrow labeled "X" points *and moves* in the same visual
direction — always. If you see the red arrow pointing along the part's
forward vector, a drag on it moves the part along its forward vector.
This is the invariant users unconsciously rely on; break it and every
rotation becomes disorienting.

**Multi-selection rules**:
- Gizmo **position** is always the group AABB center (same for both spaces).
- Gizmo **orientation**:
  - World mode → axis-aligned
  - Local mode → rotation of the **active** (last-selected) entity, matching
    Maya / Unity / Roblox Studio convention
- Drag math uses the same axis as the gizmo visualizes — consistency beats
  sophistication.
- If the user wants all parts to transform around their own origins
  instead of the group pivot, that's the **Individual** pivot mode, not a
  space-toggle concern.

#### Numeric Input During Transform (P1 — big CAD-level differentiator)
- During any drag: type `2.5 <Enter>` → commit that distance/angle
- `X 2.5 <Enter>` → override axis constraint to X, move 2.5
- `+5` → relative delta from start
- Tab cycles through axis values (Blender convention)
- Esc cancels; RMB cancels (configurable)

### 4.3 Snap System

Snap is a **resource (`SnapSettings`)**, not per-tool state. Tools query it.

#### Grid Snap (P0 — exists)
- World-aligned grid
- Grid size is a property; dial up/down with bracket keys `[` / `]`
- Snap in translate only (default), opt-in for scale (rare)

#### Angular Snap (P0 — missing)
- Rotate by N-degree increments
- Default presets: 1, 5, 15, 45, 90
- Hold Ctrl during rotate to toggle snap on/off vs. `SnapSettings.angle_snap_enabled`

#### Vertex / Edge / Face Snap (P1)
- Hover the cursor near another mesh's vertex → snap to it during drag
- Visual indicator: small icosphere at the snap target
- Priority: vertex > edge-midpoint > face-center > grid
- Modifiers:
  - `V` hold — vertex-only
  - `E` hold — edge-only
  - `F` hold — face-only

#### Smart Guides (P1 — see ADORNMENT_ARCHITECTURE Smart Grid section)
- Edge-to-edge alignment across parts
- Center-to-center alignment
- Corner-to-corner detection
- R-tree spatial index for O(log n) queries during drag

#### Surface Snap (P0 — exists in Move)
- Ray-cast downward (or along camera-forward) to find surface
- `Align to Normal` toggle — orient the placed part to surface normal
- Extend to Scale: "face rests on surface" mode

### 4.4 Alignment / Distribution (P1)

A dedicated **Align panel** + keyboard shortcuts:

- Align X min / center / max — `Ctrl+Alt+X`, `Shift+X`, `Shift+Alt+X`
- Same for Y, Z
- Distribute evenly along axis — fixed number of parts, equal gaps
- Align rotations to active
- Align to face normal (hit test, pick face)
- Center in world / space / parent
- Match size to active

### 4.5 Duplicate / Pattern (P1)

- **Duplicate** (`Ctrl+D`) — clones selection at offset (✓ today)
- **Duplicate & Place** — clone then follow cursor until LMB commits
- **Linear Array** — count + step vector or start/end markers
- **Radial Array** — count + angle + pivot
- **Grid Array** — 2D / 3D block of copies
- **Path Array** (P2) — array along a Rune-defined curve or a parametric
  spline service
- **Mirror** — across plane (XY/XZ/YZ + custom); live instance or committed

### 4.6 Geometry Operations (mixed P1 / P2)

- **CSG Union / Difference / Intersect** — P1 (csg crate exists; wire UI)
- **Fillet edges** — CAD must-have; P2
- **Chamfer edges** — P2
- **Extrude / inset / bevel** — P2 (mesh-edit mode)
- **Loop cut** — P2
- **Mirror modifier** — P2 (non-destructive, updates live)
- **Solidify** — P2 (add thickness to a plane)
- **Subdivide** — P2

### 4.7 Measurement (P1)

- **Distance measure** — click two points, readout in header
- **Angle measure** — click three points
- **Bounding-box readout** — status bar shows selection AABB dimensions
- **Surface area readout** (P2)
- **Volume readout** (P2)
- **Mass + center-of-mass** (P2 — Realism hook, uses material density)

### 4.8 Camera (P0 mostly exists; P2 polish)

- Orbit / pan / zoom — ✓
- Frame selection (`.` / `F`) — ✓
- Numpad axis views — ✓
- Ortho / perspective — ✓
- Walk / fly — ✓
- **Saved viewpoints** (P2) — named, accessible via Numpad+number
- **Cinematic orbit** (P2) — smooth auto-orbit for screenshot/video
- **DOF preview** (P2) — toggle depth-of-field simulating final render

### 4.9 Terrain (P1 — exists with gizmo issues)

- Raise / Lower / Smooth / Flatten / Paint / Region / Fill — ✓ buttons
- Brush preview mesh — ● (partial)
- **Build tool** (P1) — bulk sculpt from heightmap
- **Erosion sim** (P2) — physically-based terrain aging

### 4.10 Material / Visual (P1)

- Material picker (Properties panel) — ✓
- Drag-and-drop from Asset Browser — ✓
- **Paint material on face** (P2) — per-face material assignment
- **Material swap dialog** — replace material X with Y across scene (P1)
- **Material-from-image (AI)** (P2) — upload photo, generate PBR material
- **Live shader preview** (P2) — see shader changes in real time

### 4.11 Animation / Timeline (P2)

- Keyframe Transform / BasePart / custom properties
- Timeline panel with scrubbing
- Bezier handles per key
- Constraint-based animation (Inverse Kinematics on Motor6D)
- Bake to optimized runtime animation

(These tools exist partially via Motor6D + humanoid crates; editor UX P2.)

### 4.12 AI Workshop Tools (P0 partial, P1 polish)

- `@mention` autocomplete for entities ✓
- Natural-language edit commands — ✓ (via MCP)
- AI generates part geometry — ✓ (via Workshop + Rune)
- **Context-aware suggestions** — "this part would look better rotated 15°"
  (P2; needs embedvec + LLM)
- **Visual similarity select** — "select all parts like this one" (P2)

### 4.13 Smart Build Tools (the stravant tier)

A family of **direct-manipulation power tools** that Roblox's community
proved are indispensable. These are what separate an editor you tolerate
from an editor you love. Each tool replaces 10–60 seconds of manual
construction with 1–3 clicks.

The lineage: **stravant** (Aaron Yonas) authored most of the iconic
Roblox Studio community plugins — GapFill, ResizeAlign, Model Reflect,
Material Flip, Part to Terrain, Edge Align. They are so good that
serious Roblox builders *cannot work without them*. We build them into
the core editor, polish them past the plugin tier, and wire them into
our parametric + CAD infrastructure so they work on both primitive parts
AND feature-tree-backed CAD geometry.

All Smart Build Tools live on the **CAD tab** under the `Smart Edit`
section (see §3.5).

#### 4.13.1 Gap Fill (P0)

**What it does.** User picks two edges (on different parts, or the
same part). The tool generates a bridging mesh that fills the gap
between those edges, with a user-specified thickness. Roof pitches,
floor transitions, angled wall-to-wall fillets — any place two parts
don't meet cleanly but should.

**Interaction**:
1. Activate Gap Fill (`G` keybind proposal).
2. Hover — edges on parts glow on hover (edge-detect via
   `SpatialQuery::ray_hits` + OBB intersection tolerance).
3. Click edge A. Edge highlights stay lit.
4. Hover over edge B. Live preview mesh renders between A and B.
5. Adjust thickness via scroll-wheel or `[`/`]` keys (reuse grid-size
   convention).
6. Click to commit, or Esc to cancel.

**Algorithm**:
1. Represent each edge as `(p_start, p_end, outward_normal)`. For a
   primitive part's edge, normal is the average of the two adjacent
   faces' normals. For a MeshPart edge, derived from the two
   boundary-adjacent faces in the mesh's collision data.
2. Build a **quad** with corners `[A.start, A.end, B.end, B.start]`.
3. Triangulate the quad (two triangles). Edge-case: if A and B aren't
   coplanar (skew), the quad becomes a bilinear surface — tessellate
   further for smoothness.
4. **Extrude** the quad by `thickness` along the outward normal (or
   along `+normal_A × normal_B` for a balanced direction).
5. Resulting mesh is committed as a new `Part` with `class = MeshPart`
   or a CSG `Union`. Inherits color + material from the part containing
   edge A.
6. Undo: single entry `Gap Fill between <entityA>/edge-N and <entityB>/edge-M`.

**Edge detection** — the tricky part:
- Primitive parts (Block/Wedge/Cylinder/Sphere): edges are mathematically
  defined; hit test against their analytic form.
- MeshParts: raycast against collision mesh; walk the hit triangle's
  adjacency to find its bordering edges; offer the closest one.
  (This matches stravant's "ton of raycasts to explore and precisely
  infer the edges" approach — necessary because mesh vertex data isn't
  exposed to plugins at runtime.) In Eustress we *do* have mesh vertex
  data via the `eustress-common::mesh` crate, so we can skip the
  raycast heuristic and pick the closest edge analytically.
- Unions: use the CSG result mesh's edges.

**Limitations to document to users**:
- Non-planar edge pairs produce skew fills; user may want to split into
  two Gap Fills
- Edges that share a vertex produce a degenerate triangle; the tool
  refuses and asks user to pick a different second edge.

**Differentiator vs. stravant**: we do this on the CSG body, not a
naïve mesh — so the result is editable in the feature tree (a `Fill`
feature referencing the two edge selections). If either input part's
geometry changes, the gap fill regenerates. This is impossible in
Roblox.

#### 4.13.2 Resize Align (P0)

**What it does.** User picks a source face, then a target face. The
source part is **resized** (not moved) along its face normal until the
source face lands exactly on the target face's plane. The part size
changes on one axis to fill the gap; other axes stay.

**Interaction**:
1. Activate Resize Align (`R` reserved — may need a different chord).
2. Click source face (face highlights while hovered).
3. Hover target face — live preview of resized source.
4. Click to commit.

**Modes**:
- **Outer Touch** (default) — source's outer surface meets target's
  outer surface.
- **Inner Touch** — source extends through the target's plane to touch
  its inner surface (i.e. the far side of a thin-wall target).
- **Rounded Join** — instead of resizing, *generates* a cylinder or
  sphere connector between the two faces (radius auto-fit or user-set).
  Useful for fillet-style transitions.
- **Exact Target** — disable "snap to adjacent face on near-edge click"
  smart selection.
- **Dragger Mode** (Ctrl-hold) — combine with Move behaviour so you
  can drag+resize in one gesture.

**Smart face selection** (from stravant's design):
- If user's click is within some threshold (e.g. 10% of face-edge
  distance) of a face edge, snap selection to the *adjacent face*.
  Reason: users usually want to align to the face "beyond" the edge,
  not the edge face itself. Toggleable via Exact Target.

**Algorithm (Outer Touch)**:
1. Source face `F_src` with normal `n_src` and center `c_src`.
2. Target face `F_tgt` with plane `(n_tgt, d_tgt)` where
   `n_tgt · p = d_tgt` for points on the plane.
3. Desired: resize source so that `F_src` now sits on the target plane.
4. Compute signed distance from current `c_src` to target plane along
   `n_src`: `delta = (d_tgt − n_tgt · c_src) / (n_tgt · n_src)`.
   (Requires `n_tgt · n_src ≠ 0` — i.e., faces aren't parallel to
   each other's normals; if they are, can't resize in one axis, fall
   back to Rounded Join or show "faces are parallel" error.)
5. Identify the source face's axis relative to the part's local frame
   (`±X`/`±Y`/`±Z`).
6. Resize: grow or shrink source part's size on that axis by `|delta|`,
   translate origin by `delta/2` on the same axis (so the opposite
   face stays put).

**Rotation handling** — we support rotated parts:
- `n_src` is in world space (= `part.world_rotation × local_normal`).
- Same for `n_tgt`.
- The resize axis is identified on the part's **local** frame by
  checking which local axis is closest to `part.world_rotation⁻¹ × n_src`.
- Scale change + translate is then applied in local space.

**Rounded Join algorithm**:
- Compute midpoint + direction between `F_src` center and `F_tgt` center.
- If `|F_src| ≈ |F_tgt|` → cylinder of matching size, axis along
  midpoint direction.
- If one is a point/small face → sphere bridge.
- New filler part inherits material from source.

**Join Surfaces integration** — when Eustress supports assembly
welds (Motor6D / WeldConstraint, see TOOLSET_CAD.md §5.6), Resize Align
updates welds on resize so connected parts stay welded after the
dimension change. Matches Roblox's "Join Surfaces" toggle behaviour.

**Differentiator**: CAD-mode Resize Align records the resize as a
parametric dimension: the source's depth is driven by the distance to
target face. Move the target, source follows. Again, impossible in
Roblox because geometry isn't parametric.

#### 4.13.3 Edge Align (P0)

**What it does.** Like Resize Align, but aligns **edges** to a target
**edge** (not face-to-face). Useful for flush beam alignment, rail
joining, coplanar panel assembly.

**Interaction**: click source edge, hover target edge, live preview,
click commit.

**Algorithm**: similar to Resize Align but operates on edge
centerlines rather than face planes. The source part translates (not
resizes) so its edge lies along the target edge's infinite line.

**Constraint form** (CAD mode): recorded as a parametric "edges
collinear" constraint. Move the target, source edge stays collinear.

#### 4.13.4 Part Swap (P0)

**What it does.** Replace one or more selected parts with a template,
or swap positions of two parts.

**Two operating modes**:

**Replace in place** (primary use case):
1. Select target part(s).
2. Activate Part Swap → palette picker opens (same UI as Toolbox).
3. Pick the template part / model.
4. For each target: delete it, spawn template at target's world
   transform (position/rotation), copy over material + color + name +
   attributes from target (user-toggleable: "Preserve material?"
   "Preserve color?").
5. Children / descendants of the target are reparented under the new
   instance (or preserved inline if it's a Model).

**Swap positions**:
1. Select exactly two parts.
2. Activate Part Swap → no palette, immediate execution.
3. Exchange `world_transform` of A and B. Sizes untouched.

**Eustress-specific extensions**:
- **AI template suggest**: `Workshop.suggest_swap(selected)` picks a
  best-fit template from the Toolbox based on the current part's
  `BasePart.size` + tags (embedvec cosine-similarity).
- **Parametric swap (CAD mode)**: swap the `class_name` /
  `feature_tree` reference in `_instance.toml` rather than destroying
  the part; provenance + history preserved.

**Undo**: single entry `Swap N parts with <template_name>`.

**Use cases**:
- Iterating on a building's window style — swap all placeholder Window
  parts with a fancy version.
- Prototyping with generic blocks, finalizing with detailed models.
- Swapping in-place to test a different mechanical bracket in an
  assembly (CAD mode — feature tree regenerates downstream).

#### 4.13.5 Material Flip (P1)

**What it does.** Flip a part's material texture orientation without
rotating the part. Textures on angled surfaces often come out
sideways; users either had to rotate the part (breaking geometry) or
give up. Material Flip solves this cleanly.

**Four operations**:
- Rotate texture 90° CW
- Rotate texture 90° CCW
- Mirror U (horizontal flip)
- Mirror V (vertical flip)

**Per-face or per-part**: v1 applies per-part (same rotation to all six
faces). v2 would allow per-face via face-select-first.

**Implementation**: stored as a `MaterialUVTransform { rotation: u8,
mirror_u: bool, mirror_v: bool }` component on the part, applied in
the shader via a UV transform matrix. No mesh rebake.

#### 4.13.6 Model Reflect / Mirror (P0 for simple, P1 for linked)

**What it does.** Mirror a model or selection across a plane. Two
modes:

**Destructive mirror** (P0): creates new parts at mirrored positions,
with mirrored rotations. Welds / Motor6Ds are duplicated with the
`Part0`/`Part1` swapped as needed. This is a one-time operation.

**Non-destructive mirror / linked** (P1): creates a `MirrorFeature` in
the feature tree. Changes to source parts propagate to mirrored copies
automatically. Useful for symmetric vehicles, symmetric buildings,
symmetric mechanical assemblies.

**Mirror plane**:
- World XY / XZ / YZ quick buttons
- Custom: pick 3 points, or select a Part's face

**Algorithm**:
1. For each source entity: compute mirrored `world_transform` by
   reflecting position across the plane + flipping the rotation's
   component perpendicular to the plane.
2. Spawn mirrored entity (or reuse existing from linked mirror).
3. Fix up weld endpoints: if source welds A↔B exist, create weld
   A'↔B'. If a weld crosses the mirror plane (A on one side, B on
   the other), that weld is shared, not duplicated.

#### 4.13.7 Part to Terrain (P1)

**What it does.** Select parts, convert their geometry to Voxel
Terrain at a chosen biome / material. Either removes the source parts
or leaves them as an overlay.

**Eustress-specific**: our terrain is procedural + voxel, with
per-voxel material encoding. Part-to-Terrain rasterizes the part's
bounding mesh into the voxel grid, writes the chosen material, and
optionally deletes the source Part. The inverse (`Terrain to Part`) is
also possible (P2).

#### 4.13.8 Align & Distribute Panel (P0)

Already scoped in §4.4; listed here again because it lives on the CAD
tab's `Align` section alongside the stravant-inspired tools.

Key differentiators from Roblox's F3X:
- Works on mixed selections of Parts + Models + CAD parametric bodies
- Respects pivot mode (§4.2 Pivot Control)
- Records as a parametric constraint in CAD mode (not one-shot)

#### 4.13.9 Future stravant-tier additions (P2)

Tools we should revisit once the core is solid:

- **Loop Subdivider** — subdivide a mesh edge-loop for higher-resolution
  editing (analogous to Blender's loop cut, but works on parametric
  and CSG geometry).
- **Constraint Editor** — visual 3D editor for physical constraints
  (BallSocket, Prismatic, Hinge, Rod, Spring). Replaces typing CFrames
  in Properties with dragging anchors in 3D.
- **Attachment Editor** — Roblox-style attachment placement with
  orientation handles; useful for animation rigs and physics
  constraint anchors.
- **Scale Lock** — lock proportional scaling so uniform scale tool
  preserves feature proportions (important for CAD features — scaling
  a fillet proportionally is usually wrong).
- **Bulk Import** — drag-and-drop a folder of GLB / STL / STEP files
  into the viewport, auto-place on a grid with naming.

### 4.14 Measurement Tools (P1 — see §4.7)

Already scoped.

### 4.15 Terrain Tools (P1 — see §4.9)

Already scoped.

### 4.16 Material / Visual Tools (P1 — see §4.10)

Already scoped.

## 5. Architectural Requirements

Every tool in §4 inherits these behaviours without per-tool work:

1. **Undoable.** Records to `UndoStack` with a descriptive label.
2. **Scriptable.** Callable from Rune and MCP. The UI invokes the same
   entry points — no "editor-only" shortcuts.
3. **Multi-user safe.** Tools lock the affected entities in
   `SelectionManager`; collab mode vetoes conflicting concurrent edits.
4. **Streaming.** Every transform write emits on a `stream_event` for
   LSP / MCP / visualizers.
5. **File-system-first.** Transform commits write to the instance TOML
   (`instance_loader::write_instance_definition_signed`) immediately —
   crash-safe.
6. **Auth-stamped.** Every edit carries the editor's Bliss identity when
   signed in.
7. **Play-mode aware.** Tools refuse to mutate or snapshot-revert based
   on `PlayModeState` (editing during sim is its own mode).
8. **Physics-queryable.** Selection queries use Avian3D SpatialQuery for
   accurate raycasts — no polling mesh vertices.
9. **Handle is a mesh.** Every gizmo handle renders via `HandleAdornment`
   components and the shared `AdornmentRendererPlugin`. No Bevy Gizmos.
10. **ECS-native.** Tools mutate `Transform` / `BasePart` / `Instance` /
    per-adornment components. Never keep private state that systems
    can't query.

## 5.1 Shipped Today (as of 2026-04-22, Phase 0 + 1 + 2)

A compact snapshot for reviewers who just want to know what's real code.
See the Phase-0 / Phase-1 checklists in §6 for the granular view.

### Phase 0 — Foundation (fully shipped)

**Transform gizmos**
- Group handle roots (`MoveHandleRoot` / `ScaleHandleRoot` / `RotateHandleRoot`)
- 6 move axis arrows + 3 move plane handles (`MovePlaneHandle`)
- 7 scale handles (6 face cubes + 1 uniform center)
- 3 rotate rings + center free-rotate sphere
- Camera-distance-invariant sizing with `MIN_SCREEN_FRACTION` floor
- Hover/drag highlights on all three tools via `adornment_renderer`
- Local/World space toggle — `StudioState.transform_mode` consumed by
  shared `move_tool::gizmo_rotation_for(mode, selected_rotations)`
- Angular snap (Rotate): `editor_settings.angle_snap`
- Esc cancels in-progress transform (unified via `CancelModalToolEvent`)

**Hierarchy selection**
- `SelectChildrenEvent` (Ctrl+Shift+C) · `SelectDescendantsEvent` BFS
  (Ctrl+Shift+D) · `SelectParentEvent` (Ctrl+Shift+U) ·
  `SelectSiblingsEvent` (Ctrl+Shift+S) · `InvertSelectionEvent`
  (Ctrl+I)
- 13-item right-click context menu in `main.slint`

**Smart Build Tools** (`tools_smart.rs`)
- `GapFill` · `ResizeAlign` · `EdgeAlign` · `PartSwapPositions` ·
  `ModelReflect` · `MaterialFlip` (now with TOML-loader roundtrip via
  `PendingMaterialUvOps` component + `apply_pending_material_uv_ops`
  system)

**Numeric-during-drag** — fully wired for Move / Scale / Rotate
- `numeric_input.rs` detects digits during any gizmo drag;
  `FloatingNumericInput` Slint popover; parser accepts `2.5`, `+5`,
  `2.5m`, `90deg`, `1.57rad`, `.5`, `-0.3`, `2.5ft` etc.
- `finalize_numeric_input_on_move` / `_on_scale` / `_on_rotate`
  consume `NumericInputCommittedEvent`, apply the exact typed value
  + push undo + TOML-persist where relevant.

**Align & Distribute** — `align_distribute.rs`
- `AlignEntitiesEvent { axis, mode }` + `DistributeEntitiesEvent { axis }`.
- Ribbon Align group on CAD tab: Align X/Y/Z Center + Distribute X/Y/Z.
- Labels land on the undo stack via `push_labeled`.

**Infrastructure**
- `ModalTool` trait + `ActiveModalTool` + `ModalToolRegistry`
- `ToolOptionsBar` Slint component (inline + `⋯` popover + cancel ×)
- `NotificationManager` commit toasts (`announce_modal_tool_commits`)
- `GhostPreviewMaterial` with `pulse_ghost_preview_alpha` system
- `spawn_new_part_with_toml` — universal folder+TOML+ECS spawn helper
- `SpawnFolders` undo variant — symmetric `std::fs::rename`
  to/from `.eustress/trash/<timestamp>/`
- Ribbon CAD tab with Smart Edit + Align + Pattern + Boolean groups
- 14 Lucide-style ribbon SVG icons + 6 cursor-badge SVGs drawn
- `UndoStack::push_labeled(label, action)` + `last_label()` /
  `label_at(i)` accessors for History-panel display

### Phase 1 — Polish (17/20 fully shipped, 1 infrastructure, 2 blocked upstream)

**New tools**
- **Array family** — `array_tools.rs`: `LinearArray`, `RadialArray`,
  `GridArray` as sibling `ModalTool` impls. Ribbon Pattern group on
  CAD tab; Ctrl+Alt+L/R/K keybindings. Source-TOML cloning via
  shared `descriptor_for_copy` helper. `GridArray` capped at 1000
  copies.
- **Duplicate & Place** — `duplicate_place_tool.rs`: snapshot-then-
  follow-cursor clone, repeatable until Esc.
- **Measure distance** — `measure_tool.rs`: 2-click readout with Δx/
  y/z components in Options Bar.

**Selection**
- **Select-by-class / tag / material** — three event types +
  handlers in `selection_sync.rs`. Each resolves from explicit event
  arg or falls back to the active (first-selected) entity's value.
- **Selection sets** — `selection_sets.rs`: `Save` / `Load` /
  `Delete` events, persisted to `.eustress/selection_sets.toml` per
  universe (git-diffable). `list_sets(root)` helper for picker UIs.

**Transform polish**
- **Pivot modes** — `pivot_mode.rs`: Median / Active / Individual /
  Cursor with `PivotState` resource + `resolve_group_pivot()` helper.
  Rotate tool integrates — Individual mode rotates each entity around
  its own origin. Move translation is pivot-invariant (no change
  needed); Scale non-Individual modes work via shared center.
- **Vertex / Edge / Face snap** — `geom_snap.rs`: `SnapCandidate`-
  slice resolver over OBB corners / edge-midpoints / face-centers.
  V/E/F modifier keys force category. Move free-drag consumes the
  resolver to override cursor target.
- **Smart alignment guides** — `smart_guides.rs`: per-frame sensor
  builds 9 alignment planes per unselected part (min / center / max
  × X / Y / Z); Move free-drag applies per-axis `resolve_guide_snap`.
- **Align-to-normal on surface drop** — new
  `EditorSettings.align_to_normal_on_drop`; move-tool applies
  `Quat::from_rotation_arc(leader_up, hit_normal)` with group-
  relative propagation.

**Numeric polish**
- Unit-aware input (`2.5m`, `2.5ft`, `90deg`, `1.57rad`…).
- Relative input (`+5` delta from drag-start).

**Readouts**
- Group-selection AABB readout (was already iterating all selected;
  doc corrected).

**Mirror**
- **Model Reflect Linked** — `mirror_link.rs`: `MirrorLink`
  component + `propagate_mirror_links` system; ModelReflect's new
  `Linked` toggle inserts the component on each clone so the pair
  stays in sync as the source moves.

**Undo**
- **Named operations** — `push_labeled(label, action)` adopted by
  `align_distribute.rs`; other tool sites adopt incrementally.

### Phase 2 — Differentiators (14 shipped, 1 scaffold, 7 blocked upstream)

**Measurements** — extended [measure_tool.rs](../../eustress/crates/engine/src/measure_tool.rs)
- `MeasureMode::{Distance, Angle, Area, Volume, Mass}`. Angle is
  3-click (leg / vertex / leg) → degrees + radians. Area / Volume /
  Mass use a Compute button to read selection AABBs and, for Mass,
  weight by `MaterialProperties.density` (falls back to 1000 kg/m³
  plastic-water). Computes mass-weighted centroid for center-of-mass.

**Numeric polish** — extended [numeric_input.rs](../../eustress/crates/engine/src/numeric_input.rs)
- Expression evaluator (`=2+3*sin(30deg)`): recursive-descent, `+`
  `-` `*` `/` `^`, parens, functions `sin/cos/tan/asin/acos/atan/
  sqrt/abs/floor/ceil/ln/log/exp/min/max`, constants `pi/e`, inline
  unit multipliers.
- Property references (`=other.x`): `PropertyRefTable` resource
  refreshed each frame from every named `Instance` + Transform +
  BasePart. Thread-local snapshot for the evaluator. Supports dotted
  refs (`other.size.x`, `other.rot.w`).

**Selection extensions** — [lasso_paint_select.rs](../../eustress/crates/engine/src/lasso_paint_select.rs)
- Lasso: `LassoSelectEvent { polygon_px, mode }` + even-odd point-
  in-polygon test against projected entity centers.
- Paint: `PaintSelectEvent { cursor_px, radius_px, mode }` —
  screen-space disc hit test.

**Camera** — [saved_viewpoints.rs](../../eustress/crates/engine/src/saved_viewpoints.rs)
- `SaveViewpointEvent` / `LoadViewpointEvent { animate }` /
  `DeleteViewpointEvent`. Persisted per-universe to
  `.eustress/viewpoints.toml`. Animated load tweens 250ms via ease-
  out cubic + slerp.

**Path array** — extended [array_tools.rs](../../eustress/crates/engine/src/array_tools.rs)
- `PathArray` ModalTool — click N points (≥ 2), set count + Align-
  to-Tangent toggle, Apply spawns evenly-spaced arc-length-
  parameterized clones.

**Editors (click-to-author)**
- [attachment_editor_tool.rs](../../eustress/crates/engine/src/attachment_editor_tool.rs)
  — `AttachmentEditor` ModalTool: face-click spawns Attachment child
  oriented +Y along hit normal.
- [constraint_editor_tool.rs](../../eustress/crates/engine/src/constraint_editor_tool.rs)
  — `ConstraintEditor` ModalTool: pick kind (BallSocket / Hinge /
  Prismatic / Rod / Spring), click two entities → spawns constraint
  folder at midpoint.

**Transform constraints** — [transform_constraints.rs](../../eustress/crates/engine/src/transform_constraints.rs)
- `AlignToAxis`, `DistributeAlong`, `LockAxis` components + per-
  frame solver systems. Authoring-time constraints that correct
  drift without becoming physics constraints.

**Scale polish**
- `EditorSettings.scale_lock_proportional` toggle — any face-drag
  on Scale tool is treated as uniform when on.

**Non-destructive Mirror + Array** — already shipped in Phase 1 as
`MirrorLink` / `LinearArray` / `RadialArray` / `GridArray` /
`PathArray` — these satisfy the Phase 2 spec for non-parametric parts.

### Still scaffold / infrastructure-only `[~]`
- **Part to Terrain** — `part_to_terrain.rs`: event + dry-run
  handler (AABB count + optional source despawn).
- **Terrain to Part** — same module — `TerrainToPartEvent` + dry-
  run. Both share the `common/terrain` heightmap/splatmap
  integration blocker.

### CAD kernel adopted — `truck` + `eustress-cad` crate (2026-04-22)

Per-user directive. New workspace crate at
[eustress/crates/cad/](../../eustress/crates/cad/):
- `Quantity` unit-tagged scalar (`"50 mm"`, `"90 deg"`, `"1.5 m"`)
- `FeatureTree` + `FeatureEntry::{Sketch, Feature, Suppressed}`
  TOML schema + loader + variable resolution
- `Sketch` with 6 entity types / 3 dimension kinds / 12 constraint kinds
- `Feature` tagged enum covering every Phase 0-2 operation
- `eval::evaluate_tree()` deterministic walker — **7 of 12 feature
  evaluators ship working** (Extrude / Revolve / Mirror / Pattern
  Linear + Circular / Split / Hole / Boolean Union+Difference+
  Intersect). Fillet / Chamfer / Shell / Sweep / Loft return typed
  `NotImplemented` with specific upstream-or-scope blockers named.
- Extrude supports Rectangle, Circle, and closed-polyline profiles.
- Boolean ops route through `truck_shapeops::or / and / not`; every
  feature's `combine: FeatureOp::{NewBody, Add, Subtract, Intersect}`
  uses the same path.
- Mirror via `I - 2nnᵀ` reflection matrix through `builder::transformed`.
- Pattern Linear uses `builder::translated`, Pattern Circular uses
  `builder::rotated` with full-360° vs partial-sweep divisor logic.
- 7 truck sub-crates wired: base / geometry / topology / modeling /
  meshalgo / shapeops / stepio

### Kernel/infrastructure unblocked 2026-04-22 (implementation pending)
- **Parametric Gap Fill / Resize Align** (Phase 1) — eustress-cad
  kernel ready. Each lands as a `Feature::*` evaluator arm matching
  the shipped Extrude pattern.
- **AI template suggest (Part Swap)** (Phase 1) — `eustress-embedvec`
  ships `EmbedvecResource.find_similar` + spatial/property embedders.
  Wiring ~3-4 days (per background survey 2026-04-22).
- **AI Select Similar** (Phase 2) — same embedvec infra. ~2-3 days.
- **AI-suggested edits in context** (Phase 2) — embedvec + existing
  Claude hooks in `spatial-llm`. ~1 week.

### Still blocked upstream — Phase 2
- **Mesh-edit mode + extrude/bevel/inset/loop cut** (Phase 2) —
  blocked on mesh-editing infrastructure (vertex/edge/face select
  mode + half-edge kernel). Separate from the truck BRep kernel
  which operates on parametric bodies, not arbitrary meshes.
- **Loop Subdivider** (Phase 2) — same mesh-edit blocker.
- **Animation timeline** (Phase 2) — timeline-panel scope, big
  feature, deferred.
- **Scrub through history timeline** (Phase 2) — requires history
  panel + scrubber UI + snapshot-frame capture.
- **Scripted tool authoring via Rune** (Phase 2) — blocked on a
  Rune sandbox that can register ModalTool factories.
- **Bulk Import** (Phase 2) — drag-folder → auto-place UI; depends
  on a folder-drop handler in the viewport that doesn't yet exist.

### Phase 0 UX polish still pending
- Dedicated `ToastUndo` Slint component (`NotificationManager` covers)
- Custom-cursor OS pipeline (SVGs drawn, not wired)
- Commit-success flash pulse (`commit_flash` material exists, flash
  system not yet triggered)
- Ribbon tab underline slide animation (static underline today)

## 6. Phased Implementation Plan

> **Cross-reference**: This plan assumes the UX foundation in
> [TOOLSET_UX.md §10](TOOLSET_UX.md#10-implementation-phases) ships
> first. Every Smart Build Tool in Phase 0 depends on
> `ToolOptionsBar`, `ModalTool`, `GhostPreviewMaterial`,
> `FloatingNumericInput`, and unified cancel handling. See
> [TOOLSET_UX.md §11](TOOLSET_UX.md#11-unified-phase-plan--dependencies-across-toolset-docs)
> for the unified critical path.

### Phase 0: Foundation (must-have for day-one usability)

All of these are currently either broken, missing, or half-done. Nothing
else ships without them.

- [x] Group handle root — one gizmo per selection, not N *(shipped: singleton `MoveHandleRoot` / `ScaleHandleRoot` / `RotateHandleRoot`)*
- [x] Local vs World space toggle wired for Move / Scale / Rotate (all three) *(shipped: `StudioState.transform_mode` + shared `move_tool::gizmo_rotation_for`)*
- [x] Handle min world size floor (handles stay clickable for tiny parts) *(shipped: `MIN_SCREEN_FRACTION` constants in handle modules)*
- [x] Angular snap (rotate by 1/5/15/45/90°) *(shipped: `editor_settings.angle_snap` consumed by `rotate_tool`)*
- [x] Move plane handles (XY/XZ/YZ drag) *(shipped: `MovePlaneHandle { normal_axis }` in `move_handles.rs`)*
- [x] Select Children + Select Descendants *(shipped: `SelectChildrenEvent` / `SelectDescendantsEvent` with BFS in `selection_sync.rs`; Ctrl+Shift+C / Ctrl+Shift+D)*
- [x] Explorer right-click context menu *(shipped: 13-item context menu in `main.slint`)*
- [x] Nudge keys unified (`+`/`−` vertical, arrows for X/Z — Roblox parity) *(shipped: `NUDGE_DELAY_SECS` / `NUDGE_REPEAT_SECS` in `keybindings.rs`)*
- [x] Handle hover/drag highlights for Scale + Rotate *(shipped via `adornment_renderer`)*
- [x] Esc cancels in-progress transform *(shipped: unified via `CancelModalToolEvent` + tool state resets)*
- [x] Numeric-during-drag (`2.5 Enter`) on all three tools *(shipped: `numeric_input.rs` + `FloatingNumericInput` + `finalize_numeric_input_on_move` / `_on_scale` / `_on_rotate`; Tab cycles axis on Move/Scale; Esc cancels)*
- [x] Bounding-box readout in status bar *(shipped: single-selection; group AABB is P1)*
- [x] Ribbon CAD tab scaffolding (sections + keyboard bindings) *(shipped: `ribbon.slint` CAD tab with Smart Edit + Boolean groups; Ctrl+Alt+P/E/M/G/A/F bindings)*
- [x] **Gap Fill** tool — edge-to-edge bridging mesh (§4.13.1) *(shipped: `tools_smart::GapFill` — 2/4-wedge auto-detection via 20° rotation-diff heuristic; uses `parts/wedge.glb` primitive)*
- [x] **Resize Align** tool — face-to-face resize with Outer/Inner/Rounded Join (§4.13.2) *(shipped: `tools_smart::ResizeAlign`)*
- [x] **Edge Align** tool — edge-to-edge collinear alignment (§4.13.3) *(shipped: `tools_smart::EdgeAlign`)*
- [x] **Part Swap** tool — replace-in-place + swap-positions (§4.13.4) *(shipped: `tools_smart::PartSwapPositions`)*
- [x] **Model Reflect / Mirror** — destructive variant (§4.13.6) *(shipped: `tools_smart::ModelReflect` with weld_fixup)*
- [x] Align & Distribute panel (§4.13.8) *(shipped v1: `align_distribute.rs` module with `AlignEntitiesEvent` + `DistributeEntitiesEvent`; ribbon Align group on CAD tab with 6 buttons — Align X/Y/Z Center + Distribute X/Y/Z; menu-action routing in `slint_ui.rs` via `align:center:<axis>` + `distribute:<axis>`. Min/Max align + Align-to-Active + dedicated floating panel pending; ribbon buttons + event model cover the core operation)*

### Phase 1: Polish (what makes us faster than Roblox Studio)

- [x] Pivot modes: Median / Active / Individual / 3D Cursor *(shipped: [pivot_mode.rs](../../eustress/crates/engine/src/pivot_mode.rs) + Rotate-tool integration in [rotate_tool.rs](../../eustress/crates/engine/src/rotate_tool.rs): gizmo center resolves through `resolve_group_pivot()`, and Individual mode rotates each entity around its own origin. Median / Active / Cursor all affect rotation pivot. **Move translation is pivot-invariant** (delta applies uniformly regardless of pivot), so no Move integration needed; Scale Individual-mode is P2 polish — the other three pivot modes work for Scale today via the existing shared-center drag path)*
- [x] Smart alignment guides (R-tree + per-frame sensor) *(shipped: [smart_guides.rs](../../eustress/crates/engine/src/smart_guides.rs) + Move-tool free-drag integration. Per-frame sensor populates alignment planes from all unselected parts; during free-drag, `resolve_guide_snap()` returns per-axis hits, Move nudges `target_pos` per-axis. Dashed-line rendering is polish-only; the snap behavior is user-facing. R-tree acceleration is a v2 item for > 1k-part universes)*
- [x] Vertex / edge / face snap *(shipped: [geom_snap.rs](../../eustress/crates/engine/src/geom_snap.rs) + Move-tool free-drag integration. Hold V, E, or F while free-dragging a selection; resolver picks the nearest OBB corner / edge-midpoint / face-center on any unselected part within threshold, overrides cursor-derived target. Resolver takes a `&[SnapCandidate]` slice so future tools can plug in the same way)*
- [x] Linear / Radial / Grid array *(shipped: [array_tools.rs](../../eustress/crates/engine/src/array_tools.rs) — three `ModalTool` impls, CAD-tab Pattern ribbon group, Ctrl+Alt+L/R/K keybindings. Each spawns TOML-backed clones via `spawn_new_part_with_toml`, batched `SpawnFolders` undo, GridArray capped at 1000 copies for safety)*
- [x] Duplicate & Place *(shipped: [duplicate_place_tool.rs](../../eustress/crates/engine/src/duplicate_place_tool.rs) — `DuplicatePlaceTool` ModalTool. Snapshots selection on first frame, uses `spawn_new_part_with_toml` per click to materialize clones at cursor hit point. Repeatable — stays active until Esc. v1 doesn't show a live ghost-mesh preview; that's a polish follow-up using `GhostPreviewMaterial`)*
- [x] CSG wired to UI *(shipped: CAD tab Boolean group in `ribbon.slint` routes `csg:union/negate/intersect/separate` → keybinding Actions in `ui/slint_ui.rs`)*
- [x] Named operations in undo history *(shipped: `UndoStack::push_labeled(label, action)` + parallel `labels: VecDeque<Option<String>>`; `label_at(i)` / `last_label()` accessors. Adopted in `align_distribute.rs` — align/distribute operations now push entries like `"Align Center X (5 parts)"`. Other tool sites can adopt incrementally; unlabeled `push` still works unchanged)*
- [x] Select-by-class / tag / material *(shipped in [`selection_sync.rs`](../../eustress/crates/engine/src/selection_sync.rs): `SelectByClassEvent` / `SelectByTagEvent` / `SelectByMaterialEvent`. Each handler resolves the target (event arg or active entity's value) and replaces selection with all matching non-abstract entities. Material query prefers `BasePart.material_name` override, falls back to the enum `as_str()`. Context-menu / MCP routing is the next-increment follow-up)*
- [x] Inverse selection *(shipped: `InvertSelectionEvent` + Ctrl+I binding)*
- [x] Measure distance tool *(shipped: [measure_tool.rs](../../eustress/crates/engine/src/measure_tool.rs) — `MeasureDistanceTool` ModalTool. Click point A, click point B, readout shows `{distance} studs (Δ x, y, z)` in the Options Bar. Reset button for repeated measurements. Non-terminating commit — stays active until user Esc. MCP `measure_distance` tool still the alternative for external-client use)*
- [x] Group-selection bounding-box readout *(already shipped: [`sync_selection_summary_to_slint`](../../eustress/crates/engine/src/ui/slint_ui.rs) iterates all `With<Selected>` entities and accumulates min/max via `calculate_rotated_aabb`; displays "{N} parts" + "{X} × {Y} × {Z}" in the viewport overlay)*
- [x] Align-to-normal on surface drop *(shipped: `EditorSettings.align_to_normal_on_drop` toggle. When on + surface-snap enabled, free-drag uses `Quat::from_rotation_arc(leader_up, hit_normal)` and applies the same delta rotation to group relative offsets so group orientation stays intact. Default off)*
- [x] Selection sets (named + recall) *(shipped: [selection_sets.rs](../../eustress/crates/engine/src/selection_sets.rs) — `SaveSelectionSetEvent / LoadSelectionSetEvent / DeleteSelectionSetEvent`. Persisted to `.eustress/selection_sets.toml` per universe (git-diffable), entries include creator username + timestamp. `list_sets()` helper for picker UIs. UI/keybinding wiring is the next-increment follow-up)*
- [x] Unit-aware input (`2.5m`, `90deg`) *(shipped in [`numeric_input.rs::parse_numeric_buffer`](../../eustress/crates/engine/src/numeric_input.rs) — accepts `m / cm / mm / km / in / ft / yd` for lengths, `deg / ° / rad` for angles; unknown units fall back to raw number. Letter characters accepted in the buffer once a digit is present)*
- [x] Relative numeric input (`+5`) *(was already partially there via the `+` leading-sign handling; now fully validated — `+5` sets `NumericInputState.relative = true` which flows into `NumericInputCommittedEvent.relative` for the tool-side finalize systems to interpret as a delta)*
- [x] **Material Flip** — UV rotation + mirror (§4.13.5) *(fully shipped: `tools_smart::MaterialFlip` in-session + loader roundtrip via `PendingMaterialUvOps` component + `apply_pending_material_uv_ops` system. `attributes.material_uv_ops` in TOML is now read at spawn, composed into an `Affine2`, and applied to a cloned material once the base asset finishes loading)*
- [x] **Model Reflect linked** — non-destructive mirror feature (§4.13.6) *(shipped: [mirror_link.rs](../../eustress/crates/engine/src/mirror_link.rs) `MirrorLink` + `propagate_mirror_links` + ModelReflect's new `Linked` option toggle in [tools_smart.rs](../../eustress/crates/engine/src/tools_smart.rs). When the user ticks Linked and hits Reflect, each clone gets a `MirrorLink` pointing at its source + mirror plane; runtime re-reflects every frame `Source.Transform` changes. Delete-cascade: if source despawns, the mirror stays but becomes unlinked)*
- [x] **Part to Terrain** — rasterize parts into voxel terrain (§4.13.7) *(shipped 2026-04-22: [part_to_terrain.rs](../../eustress/crates/engine/src/part_to_terrain.rs) now writes. For each selected AABB, the handler iterates every loaded `Chunk`, initializes `TerrainData.height_cache` + `.splat_cache` if empty, raises each vertex under the AABB footprint to the AABB's max-Y (scaled by `height_scale`), paints the chosen material layer to 1.0 in the splatmap, and marks every touched `Chunk.dirty = true` + `TerrainData.splat_dirty = true` so the mesh + GPU textures regenerate next frame. Source parts optionally despawn via `delete_sources`)*
- [~] Parametric Gap Fill (records as `Fill` feature in CAD feature tree) — **kernel ready 2026-04-22**: Extrude now supports closed-polyline profiles + Subtract combine-mode, so Gap Fill lands as a quad-edge sketch → extruded MeshPart, with edge references plumbing through reference-tree (same dep as face-reference resolution)
- [~] Parametric Resize Align (records as distance-to-face dimension) — **kernel ready 2026-04-22**: Extrude + Boolean + `FeatureOp::Subtract` all work today. Lands as a distance-to-face `SketchDimension` variant + face-reference resolver (same dep)
- [~] AI template suggest for Part Swap (embedvec cosine-match from Toolbox) — **MCP tool + dispatcher shipped 2026-04-22**: [`tools/embedvec_tools.rs::SuggestSwapTemplateTool`](../../eustress/crates/tools/src/embedvec_tools.rs) + [`engine/embedvec_dispatch.rs`](../../eustress/crates/engine/src/embedvec_dispatch.rs). `EmbedvecResource::find_similar` lookup + Toolbox scanner are the last wiring step before Part Swap's AI-suggest toggle lights up

### Phase 2: Differentiators (what makes us unique)

- [~] AI Select Similar (embedvec-backed) — **MCP tool + dispatcher shipped 2026-04-22**: [`tools/embedvec_tools.rs::FindSimilarEntitiesTool`](../../eustress/crates/tools/src/embedvec_tools.rs) routes `{entity_id, k, class_filter}` through [`engine/embedvec_dispatch.rs::EmbedvecDispatchEvent::FindSimilar`](../../eustress/crates/engine/src/embedvec_dispatch.rs) → `EmbedvecResultEvent`. Final `EmbedvecResource::find_similar(entity, k)` lookup is the one-day wiring PR
- [~] AI-suggested edits in context — **MCP tool + dispatcher shipped 2026-04-22**: `SuggestContextualEditsTool` (approval-gated) + `EmbedvecDispatchEvent::SuggestContextualEdits`. Spatial context → Claude prompt via `spatial-llm` is the final step
- [x] Lasso select *(shipped in [lasso_paint_select.rs](../../eustress/crates/engine/src/lasso_paint_select.rs): `LassoSelectEvent { polygon_px, mode }`. Handler projects every entity's world center to screen coords via `Camera::world_to_viewport`, tests via `point_in_polygon` (even-odd ray cast). `SelectMode::{Replace, Add, Toggle}`. Cursor-sample collection in the viewport is the UI follow-up — events are MCP/keyboard ready today)*
- [x] Paint select *(shipped alongside Lasso in [lasso_paint_select.rs](../../eustress/crates/engine/src/lasso_paint_select.rs): `PaintSelectEvent { cursor_px, radius_px, mode }`. Handler picks entities inside the screen-space disc. Firing the event at cursor samples while LMB is held paints onto the selection; wire-up is a UI follow-up)*
- [~] Mesh-edit mode (vertex/edge/face) — operates on non-parametric
      meshes. Parametric fillet/chamfer ship in
      [TOOLSET_CAD.md](TOOLSET_CAD.md) Phase 0. *(kernel shipped
      2026-04-22: [eustress-mesh-edit](../../eustress/crates/mesh-edit/)
      crate with half-edge data structure, `VertexId`/`EdgeId`/`FaceId`
      arena indices, `MeshSelection` with `SelectionKind::{Vertex,
      Edge, Face}`, walkers for face perimeter + normal + centroid.
      Engine-side ModalTool wrapper + Slint mode-switcher UI are the
      next increment)*
- [~] Extrude / bevel / inset / loop cut on non-parametric meshes *(partially shipped 2026-04-22 in [mesh-edit/src/ops.rs](../../eustress/crates/mesh-edit/src/ops.rs): `extrude_face` + `inset_face` work end-to-end (face duplicates, bridge quads, ring quads with centroid shrink). `bevel_edge` + `loop_cut` return `NotImplemented` pending the edge-loop walker that `twin.next` alternation resolves — lands together since both share the walker)*
- [x] Mirror + Array modifiers (non-destructive) — editor-side
      wrappers around the parametric Mirror / Pattern features in
      [TOOLSET_CAD.md](TOOLSET_CAD.md) Phase 0, exposed for
      non-parametric parts *(shipped via existing Phase-1 infra:
      non-destructive Mirror = `MirrorLink` component in [mirror_link.rs](../../eustress/crates/engine/src/mirror_link.rs)
      keeps source+mirror Transform pair in sync each frame; Array
      modifiers shipped as `LinearArray` / `RadialArray` / `GridArray`
      / `PathArray` in [array_tools.rs](../../eustress/crates/engine/src/array_tools.rs).
      All four work on non-parametric parts today; parametric BRep
      variants land with TOOLSET_CAD.md Phase 0 kernel)*
- [x] Angle / area / volume measure *(shipped: [measure_tool.rs](../../eustress/crates/engine/src/measure_tool.rs) extended with `MeasureMode::{Distance, Angle, Area, Volume, Mass}`. Angle: 3-click (leg / vertex / leg) → degrees + radians. Area / Volume: Compute button reads selection AABBs + logs total. Mode choice surfaces in the Options Bar)*
- [x] Mass + center-of-mass readout (Realism crate hook) *(shipped as `MeasureMode::Mass` in [measure_tool.rs](../../eustress/crates/engine/src/measure_tool.rs): weights each selected entity's AABB volume by its `MaterialProperties.density` (falls back to 1000 kg/m³ plastic-water-equivalent when no realism component is present), sums total mass + computes mass-weighted centroid. Logs + toasts the result on Compute)*
- [~] Animation timeline *(event-marker substrate shipped 2026-04-22 as the Timeline panel — see Scrub-through-history entry below. Keyframed-property animation (position-over-time curves) is a distinct feature that rides on the same Stream + panel infrastructure; lands when the keyframe track editor ships)*
- [x] Saved viewpoints *(shipped: [saved_viewpoints.rs](../../eustress/crates/engine/src/saved_viewpoints.rs). `SaveViewpointEvent { name }` / `LoadViewpointEvent { name, animate }` / `DeleteViewpointEvent { name }`. Persisted to `.eustress/viewpoints.toml` per universe (git-diffable). Animated load tweens over 250ms via ease-out cubic + slerp. `list_viewpoints(root)` helper for picker UIs. Numpad slot keybindings are a follow-up)*
- [x] Path array along curve *(shipped as `PathArray` ModalTool in [array_tools.rs](../../eustress/crates/engine/src/array_tools.rs) — user clicks ≥ 2 path points in the viewport, sets count + "Align to Tangent" toggle; arc-length parameterizes the polyline so spacing is uniform even on non-uniform segments. Tangent-align rotates each clone via `Quat::from_rotation_arc(X, tangent)`)*
- [x] Constraint system (align to, distribute to, lock axis) *(shipped: [transform_constraints.rs](../../eustress/crates/engine/src/transform_constraints.rs) — three components (`AlignToAxis`, `DistributeAlong`, `LockAxis`) + per-frame solver systems. Authoring-time constraints that correct drift each frame without becoming Avian physics. AlignToAxis corrects via `Quat::from_rotation_arc`, DistributeAlong evenly lerps between two bookends, LockAxis pins coordinates to initial values. UI for adding/removing these components is a follow-up)*
- [~] Scrub through history timeline *(Timeline panel shipped 2026-04-22. Data-agnostic Stream-fed marker timeline — yellow-diamond keyframes / orange-dot watchpoints / red-asterisk breakpoints. Tags group events into horizontal tracks; filter modal with per-tag checkboxes + per-kind legend chips + search. Shares bottom-panel slot with Output via `BottomPanelMode` switcher (title-bar tabs). `ModalToolCommittedEvent → Keyframe` is the shipped teeing adapter; any engine emitter pushes `PublishTimelineEventEvent`. History-scrub playhead + reverse-apply logic lands on top in a follow-up. See [timeline_panel.rs](../../eustress/crates/engine/src/timeline_panel.rs) + [timeline_panel.slint](../../eustress/crates/engine/ui/slint/timeline_panel.slint) + [timeline_filter_modal.slint](../../eustress/crates/engine/ui/slint/timeline_filter_modal.slint))*
- [x] Expression input (`(2+3)*sin(30)`) *(shipped: [`numeric_input.rs::eval_expression`](../../eustress/crates/engine/src/numeric_input.rs) — recursive-descent evaluator when buffer starts with `=`. Supports `+ - * / ^`, parens, functions `sin/cos/tan/asin/acos/atan/sqrt/abs/floor/ceil/ln/log/exp/min/max`, constants `pi/e`, and inline unit suffixes (`=2.5m + 30cm`). Keypress acceptance expands in expression mode to allow operators/parens/commas)*
- [x] Property reference input (`=other.x`) *(shipped in [numeric_input.rs](../../eustress/crates/engine/src/numeric_input.rs): `PropertyRefTable` resource populated each frame from every entity's `Instance.name` + `Transform` + `BasePart.size`. Expression parser accepts dotted references: `=other.x`, `=other.y`, `=other.z`, `=other.size.x`, `=other.rot.y`, composable with math — `=other.x + 2.5`, `=other.size.y * 0.5`. Thread-local snapshot decouples evaluator from World)*
- [~] Scripted tool authoring via Rune (users extend the toolset) *(bridge shipped 2026-04-22: [rune_tool_sandbox.rs](../../eustress/crates/engine/src/rune_tool_sandbox.rs) — `RuneToolSpec` + `RegisterRuneToolEvent` + `RuneModalTool` wrapper that routes `on_click` / `on_option_changed` / `commit` through the `ModalTool` trait into Rune VM callback tables. Rune-side callback marshaling + capability-scoped World access are the final VM-integration steps)*
- [~] **Loop Subdivider** — subdivide mesh edge-loop for higher-res editing (§4.13.9) *(infrastructure shipped 2026-04-22 via [eustress-mesh-edit](../../eustress/crates/mesh-edit/) half-edge kernel; `loop_cut(mesh, seed_edge)` returns typed `NotImplemented` pending the edge-loop walker. Same walker unblocks bevel and loop-subdivide simultaneously)*
- [x] **Constraint Editor** — 3D visual editor for BallSocket/Hinge/Rod/Spring (§4.13.9) *(shipped: [constraint_editor_tool.rs](../../eustress/crates/engine/src/constraint_editor_tool.rs) — `ConstraintEditor` ModalTool. Pick kind (BallSocket / Hinge / Prismatic / Rod / Spring), click part A, click part B → spawns a constraint entity under `Constraints/` folder at the midpoint, TOML-backed with `class_name = <Kind>Constraint`. Runtime physics binding lands when the constraint loader does; metadata is preserved)*
- [x] **Attachment Editor** — attachment orientation handles for rigs and constraints (§4.13.9) *(shipped: [attachment_editor_tool.rs](../../eustress/crates/engine/src/attachment_editor_tool.rs) — `AttachmentEditor` ModalTool. Click a face, it spawns an `Attachment` child entity at the hit point with local +Y aligned to the hit normal via `Quat::from_rotation_arc(Y, normal)`. Repeatable until Esc. Orange color matches Roblox convention. Per-attachment orientation adornment handles are a polish follow-up)*
- [x] **Scale Lock** — proportional scaling lock for CAD features (§4.13.9) *(shipped: `EditorSettings.scale_lock_proportional` toggle. When on, any face-handle drag in the Scale tool is treated as a uniform-scale drag (via axis substitution to `ScaleAxis::Uniform` inside the drag math), preserving size ratios. Default off)*
- [~] **Bulk Import** — drag-folder → auto-place GLB/STL/STEP on grid (§4.13.9) *(watcher shipped 2026-04-22: [mesh_import.rs](../../eustress/crates/engine/src/mesh_import.rs) — scans Space root every 1s for `.stl` / `.step`/`.stp` / `.obj` / `.ply` / `.fbx` sources without adjacent `.glb`, fires `MeshImportRequestEvent` → `MeshConvertedEvent`, auto-hides sources from Explorer via `ExplorerHiddenSet`. STL + STEP converters scaffold through `stl_io` / `truck-stepio` + `truck-meshalgo`; GLB binary writer is the last dep-addition PR. Auto-place-on-grid lands as a follow-up that subscribes to `MeshConvertedEvent` and offsets each new Instance along X)*
- [~] **Terrain to Part** — inverse of Part to Terrain for voxel region extraction (§4.13.7) *(event + scaffold shipped in [part_to_terrain.rs](../../eustress/crates/engine/src/part_to_terrain.rs): `TerrainToPartEvent { aabb_min, aabb_max, flatten_source, voxel_size }` + handler logs cell count. Marching-cubes extraction + MeshPart spawn lands alongside the Part→Terrain writer — same terrain-chunk integration blocker)*

## 7. Non-Goals

Explicit list to prevent scope creep:

- **Full MCAD replacement.** We won't be Fusion 360. Parametric
  sketch-to-solid is out. CAD-style features *only* where they serve
  simulation-world authoring.
- **Film-quality rendering.** Bevy renderer is the target; Arnold /
  RenderMan / Cycles are out.
- **Node-based material editor.** Materials are TOML + Rune; a visual
  graph is future if at all (and would come after node-based scripting).
- **Retopo / sculpt mode.** Use Blender, import GLB. Not worth
  duplicating.
- **Animation rig authoring from scratch.** Import from Blender / Maya.
  We edit animations, not rigs.
- **Multi-axis constraint solver.** Assembly-grade solvers are CAD
  territory. We support lock-to-axis and simple mates, not full
  degrees-of-freedom reasoning.

## 8. Success Metric

For each Phase-0 tool, *time-to-task* compared against Roblox Studio on
the same operation:

| Task                                                   | Roblox Studio | Eustress target |
|--------------------------------------------------------|--------------:|----------------:|
| Move selected part 2.5 units +X (numeric)              | 4–5s (panel)  | ≤2s (drag+type) |
| Rotate part exactly 45° around Y                       | 3–4s (panel)  | ≤2s (drag+snap) |
| Scale to half on X only                                | 2–3s          | ≤1.5s           |
| Select all parts matching "Metal" material             | no-op         | ≤3s (right-click + filter) |
| Align 10 parts to active on Y-center                   | manual (~30s) | ≤5s (panel)     |
| Duplicate with offset 4 times                          | 4×Ctrl+D      | ≤3s (Linear Array) |
| Place part perfectly flush on angled surface           | manual (~15s) | ≤3s (surface + align-normal) |
| Fill a triangular gap between two sloped roof panels   | 30–60s manual + rotation fiddle | ≤3s (Gap Fill, 2 edge clicks) |
| Resize a pillar to meet an angled roof face            | 20–40s manual + measurement    | ≤2s (Resize Align, 2 face clicks) |
| Swap 20 placeholder windows for final window model     | 20× manual drag + replace       | ≤5s (Part Swap across selection) |
| Mirror a 30-part car chassis across center plane       | ~5 min (rebuild manually)       | ≤2s (Model Reflect destructive) |

These numbers are the acceptance bar for the Phase-0 checklist above.

---

*Last updated: 2026-04-22. Phase 0 closed, Phase 1 closed
(17 shipped + 1 infra-only + 2 kernel-unblocked), Phase 2 closed
(14 shipped + 1 scaffold + 3 embedvec-unblocked + 4 still blocked
upstream). UX polish pass complete. CAD BRep kernel adopted
(`truck` via `eustress-cad`); per-feature evaluators are incremental
PRs against the shipped Extrude template. Keep this doc in sync
with `IMPLEMENTATION_STATUS.md` when a feature ships — or this
doc becomes a wishlist and loses its value.*
