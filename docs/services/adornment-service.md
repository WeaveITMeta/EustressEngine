# AdornmentService

**Category:** Rendering  
**Class Name:** `AdornmentService`  
**Learn URL:** `/learn/services/adornment-service`

## Overview

AdornmentService provides visual decoration objects that overlay or surround
parts. These include SelectionBox (wireframe outline), Highlight (glow/outline
effect), BillboardGui (world-space UI), SurfaceGui (texture-space UI), and
Beam (particle trail between attachments).

## Properties

AdornmentService has no configurable properties on the service itself.
Adornments are created as child objects of Parts or other instances.

## Adornment Types

### SelectionBox
A wireframe outline drawn around a part's bounding box. Commonly used for
selection indicators in editors and building games.

| Property | Type | Description |
|----------|------|-------------|
| `Adornee` | Instance | The part to outline. |
| `Color3` | Color3 | Color of the wireframe lines. |
| `LineThickness` | float | Thickness of the outline in pixels. |
| `SurfaceColor3` | Color3 | Fill color for the box faces. |
| `SurfaceTransparency` | float | Transparency of the face fill (0–1). |

### Highlight
A glow and/or outline effect applied to an entire model or part. Used for
hover effects, enemy indicators, and interactive object cues.

| Property | Type | Description |
|----------|------|-------------|
| `Adornee` | Instance | The part or model to highlight. |
| `FillColor` | Color3 | Color of the inner glow fill. |
| `FillTransparency` | float | Transparency of the fill (0–1). |
| `OutlineColor` | Color3 | Color of the silhouette outline. |
| `OutlineTransparency` | float | Transparency of the outline (0–1). |
| `DepthMode` | enum | Whether highlight renders through walls. |

### BillboardGui
A GUI plane that always faces the camera, positioned in world space. Used for
nametags, health bars, and floating labels.

| Property | Type | Description |
|----------|------|-------------|
| `Adornee` | Instance | The part to attach to. |
| `Size` | UDim2 | Size of the billboard in offset/scale. |
| `StudsOffset` | Vector3 | Offset from the adornee in studs. |
| `MaxDistance` | float | Distance beyond which the billboard hides. |
| `AlwaysOnTop` | bool | Render through walls. |

### SurfaceGui
A GUI plane mapped to a specific face of a part. Used for in-world screens,
signs, and interactive displays.

| Property | Type | Description |
|----------|------|-------------|
| `Adornee` | Instance | The part to attach to. |
| `Face` | enum | Which face of the part (Top, Bottom, Front, etc.). |
| `SizingMode` | enum | How the GUI scales to the face. |
| `PixelsPerStud` | float | Resolution of the surface texture. |

### Beam
A textured trail rendered between two Attachments. Used for laser effects,
rope visuals, trails, and connections between objects.

| Property | Type | Description |
|----------|------|-------------|
| `Attachment0` | Attachment | Start point. |
| `Attachment1` | Attachment | End point. |
| `Color` | ColorSequence | Color gradient along the beam. |
| `Width0` / `Width1` | float | Width at start and end. |
| `Texture` | string | Texture asset for the beam surface. |
| `TextureSpeed` | float | Scroll speed of the texture. |

## Common Patterns

### Hover Highlight
```rune
let highlight = Instance.new("Highlight");
highlight.FillColor = Color3.new(0, 0.5, 1);
highlight.FillTransparency = 0.5;
highlight.OutlineColor = Color3.new(0, 0.7, 1);
highlight.Parent = hovered_part;
```

### Nametag
```rune
let billboard = Instance.new("BillboardGui");
billboard.Adornee = character.Head;
billboard.Size = UDim2.new(0, 100, 0, 30);
billboard.StudsOffset = Vector3.new(0, 2, 0);

let label = Instance.new("TextLabel");
label.Text = player.Name;
label.Parent = billboard;
billboard.Parent = character;
```

## Related Services

- [Workspace](workspace.md) — Adornments attach to parts in Workspace
- [MaterialService](material-service.md) — Material rendering beneath adornments
- [Lighting](lighting.md) — Lighting affects adornment visibility
