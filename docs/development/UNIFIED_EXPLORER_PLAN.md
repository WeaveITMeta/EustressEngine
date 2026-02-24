# Unified Explorer + Tabbed Viewer + VS Code Integration

> **Status:** Planning  
> **Author:** Eustress Engine Team  
> **Date:** 2026-02-22  
> **Priority:** P0 — Flagship Feature  

---

## Table of Contents

1. [Vision](#1-vision)
2. [Why This Is Revolutionary](#2-why-this-is-revolutionary)
3. [Architecture Overview](#3-architecture-overview)
4. [Existing Infrastructure](#4-existing-infrastructure)
5. [Data Models](#5-data-models)
6. [Frame Types & File Routing](#6-frame-types--file-routing)
7. [Icon System](#7-icon-system)
8. [Slint UI Components](#8-slint-ui-components)
9. [Rust Systems & Modules](#9-rust-systems--modules)
10. [Monaco Editor Integration](#10-monaco-editor-integration)
11. [VS Code Keybindings](#11-vs-code-keybindings)
12. [Phased Implementation Plan](#12-phased-implementation-plan)
13. [Open Source References](#13-open-source-references)
14. [Risk Assessment](#14-risk-assessment)
15. [Testing Strategy](#15-testing-strategy)

---

## 1. Vision

Transform Eustress Studio into the **first game engine with a fully integrated VS Code-class development environment**. Users never leave the editor — they browse files, edit code with syntax highlighting and intellisense, preview images, watch videos, read documentation, and build 3D worlds all in one window.

### Core Principle: One Explorer, All Content

The Explorer panel presents a **single unified tree** combining:

- **ECS entities** (Workspace, Lighting, Players, Services) — existing hierarchy
- **Filesystem nodes** (project directory, assets, scripts, configs) — integrated inline

No mode switching. No tabs. One tree showing both game objects and files. Double-clicking any item opens it in the appropriate center tab frame.

**Example unified tree:**
```
📦 Workspace
  📷 Camera
  🧊 Baseplate
  🎲 Welcome Cube
💡 Lighting
  ☀️ Sun
👥 Players
📁 src/           ← Filesystem starts here
  📄 main.rs
  📄 lib.rs
📁 assets/
  📁 models/
    🎨 character.gltf
  📁 textures/
    🖼️ grass.png
📁 docs/
  📝 README.md
```

### Core Principle: Smart Tab Routing

Every file type maps to an optimal viewer frame:

| Content | Frame | Renderer |
|---------|-------|----------|
| `.eep`, `.gltf`, `.obj`, `.fbx` | **Scene Frame** | Bevy 3D viewport |
| `.rs`, `.lua`, `.ts`, `.json`, `.toml`, `.yaml`, `.soul` | **Code Frame** | Monaco Editor via wry |
| `.html`, `.htm`, URLs | **Web Frame** | Wry WebView direct |
| `.mp4`, `.webm`, `.mov`, `.avi` | **Video Frame** | HTML5 `<video>` in wry |
| `.png`, `.jpg`, `.svg`, `.bmp`, `.webp` | **Image Frame** | Slint `Image` or wry |
| `.md`, `.rst`, `.txt` | **Document Frame** | Markdown→HTML in wry |
| `.pdf` | **Document Frame** | PDF.js in wry |
| `.wav`, `.ogg`, `.mp3`, `.flac` | **Audio Frame** | HTML5 `<audio>` in wry |
| `.csv`, `.tsv` | **Table Frame** | HTML table in wry |
| Unknown binary | **Hex Frame** | Hex viewer in wry |
| Unknown text | **Text Frame** | Monaco plain text |

---

## 2. Why This Is Revolutionary

### Competitive Landscape

| Engine | File Explorer | Code Editor | Web Browser | Media Viewer | Integrated? |
|--------|--------------|-------------|-------------|-------------|-------------|
| **Unreal** | Asset browser only | None (external VS/Rider) | None | Texture preview | ❌ |
| **Unity** | Asset browser only | None (external VS/Rider) | None | Texture/audio preview | ❌ |
| **Roblox Studio** | Instance tree only | Built-in Lua editor | None | None | Partial |
| **Godot** | File + scene tree | Built-in GDScript editor | None | Texture preview | Partial |
| **Eustress** | Instance tree + **filesystem** | **Monaco Editor** | **Wry browser** | **All media** | **✓ Full** |

No engine combines all five. Eustress would be the first to offer a complete development environment that rivals VS Code itself, embedded inside a 3D engine.

### Productivity Impact

- **Zero context switching** — No Alt-Tab between editor and IDE
- **Instant preview** — Edit a shader, see it live in the viewport next tab
- **Unified search** — Ctrl+Shift+F searches entities AND files
- **Drag-and-drop** — Drag an image from file explorer onto a 3D object
- **One project** — `.eep` scene files, Rust code, assets, docs all visible

---

## 3. Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      Eustress Studio                         │
│  ┌─────────┐  ┌────────────────────────────────┐  ┌───────┐ │
│  │EXPLORER │  │        CENTER TABS             │  │PROPS  │ │
│  │         │  │  ┌──────┬──────┬──────┬──────┐ │  │       │ │
│  │📦 Work  │  │  │Scene │Code  │Web   │Image │ │  │       │ │
│  │💡 Light │  │  │.eep  │.rs   │.html │.png  │ │  │       │ │
│  │👥 Play  │  │  │      │      │      │      │ │  │       │ │
│  │📁 src/  │  │  │Bevy  │Monaco│Wry   │Slint │ │  │       │ │
│  │ 📄 main │  │  │3D    │Editor│View  │Image │ │  │       │ │
│  │ 📄 lib  │  │  │      │      │      │      │ │  │       │ │
│  │📁 assets│  │  └──────┴──────┴──────┴──────┘ │  │       │ │
│  │� docs  │  │                                  │  │       │ │
│  └─────────┘  │      OUTPUT / TERMINAL           │  └───────┘ │
│               └────────────────────────────────┘            │
└─────────────────────────────────────────────────────────────┘
```

### System Flow

```
User double-clicks file in Explorer
  → Rust: FileOpenEvent { path, detected_type }
  → Rust: resolve_frame_type(extension) → FrameType
  → Rust: create or focus CenterTab { tab_type: FrameType }
  → Slint: tab bar updates, content area switches to correct frame
  → Frame-specific init:
      Code  → Monaco loads file content via wry
      Image → Slint Image loads from path
      Web   → Wry navigates to URL/file
      Scene → Bevy loads .eep/.gltf
```

---

## 4. Existing Infrastructure

### What We Already Have

| Component | Location | Status |
|-----------|----------|--------|
| Explorer Panel (ECS entities) | `explorer.slint` + `slint_ui.rs:sync_explorer_to_slint` | ✅ Working |
| EntityNode struct | `explorer.slint:10-20` | ✅ Working |
| TreeItem component | `theme.slint` | ✅ Working |
| CenterTab model | `script_editor.slint:11-22` | ✅ Working |
| Tab bar with drag-drop | `main.slint:210-232` | ✅ Working |
| Content area routing | `main.slint` (scene/script/web conditionals) | ✅ Working |
| WebBrowser component | `web_browser.slint` | ✅ Working |
| ScriptEditor component | `script_editor.slint` | ✅ Basic (plain text) |
| Wry WebView plugin | `webview.rs` (behind `webview` feature) | ✅ Working |
| SVG icon pipeline | `assets/icons/` (49 engine + 52 UI icons) | ✅ Working |
| `load_class_icon()` | `slint_ui.rs` | ✅ Working |
| ExplorerState resource | `slint_ui.rs:692-698` | ✅ Working |
| ExplorerExpanded resource | `slint_ui.rs:687-690` | ✅ Working |

### What Needs Extension

| Component | Current | Target |
|-----------|---------|--------|
| Explorer panel | ECS entities only | ECS + filesystem **unified single tree** |
| CenterTab.tab_type | `"scene"`, `"script"`, `"web"` | + `"code"`, `"image"`, `"video"`, `"audio"`, `"document"`, `"table"`, `"hex"` |
| ScriptEditor | Plain TextEdit | Monaco Editor via wry WebView |
| Icon system | Class-based (`load_class_icon`) | Class + file-extension-based |
| File operations | None | Create, rename, delete, move |
| Search | Entity name search only | + file content search (ripgrep) |

---

## 5. Data Models

### 5.1 Slint: FileNode (new)

```slint
// Unified tree node for both ECS entities and filesystem items
export struct TreeNode {
    // Common fields
    id: int,                // Entity ID (for ECS) or hash (for files)
    name: string,           // Display name
    icon: image,            // Icon (class icon or file-type icon)
    depth: int,             // Tree indentation level
    expandable: bool,       // Has children
    expanded: bool,         // Currently expanded
    selected: bool,         // Currently selected
    visible: bool,          // Matches search filter
    
    // Type discriminator
    node-type: string,      // "entity" or "file"
    
    // Entity-specific (when node-type == "entity")
    class-name: string,     // ECS class name
    
    // File-specific (when node-type == "file")
    path: string,           // Absolute path
    is-directory: bool,     // Folder vs file
    extension: string,      // File extension
    size: string,           // Human-readable size ("4.2 KB")
    modified: bool,         // Has unsaved changes (dirty dot)
}
```

### 5.2 Slint: CenterTab (extended)

```slint
export struct CenterTab {
    entity-id: int,         // Entity ID for ECS tabs (-1 for file tabs)
    name: string,           // Display name / filename
    tab-type: string,       // "scene", "code", "web", "image", "video", "audio", "document", "table", "hex"
    dirty: bool,            // Unsaved changes
    content: string,        // Text content (code/document)
    url: string,            // URL for web tabs OR file:// path
    loading: bool,          // Loading state
    favicon: image,         // Tab icon (file-type icon or favicon)
    can-go-back: bool,      // Browser nav
    can-go-forward: bool,   // Browser nav
    file-path: string,      // Absolute file path (for file-backed tabs)
    language: string,       // Language ID for syntax highlighting ("rust", "lua", "json")
    read-only: bool,        // Whether editing is allowed
    line-count: int,        // Total lines (for code tabs)
    cursor-line: int,       // Current cursor line
    cursor-col: int,        // Current cursor column
}
```

### 5.3 Rust: UnifiedExplorerState (replaces ExplorerState + new FileExplorerState)

```rust
/// Unified explorer state combining ECS entities and filesystem
#[derive(Resource)]
pub struct UnifiedExplorerState {
    /// Currently selected item (entity or file)
    pub selected: SelectedItem,
    /// Set of expanded entity IDs
    pub expanded_entities: HashSet<Entity>,
    /// Set of expanded directory paths
    pub expanded_dirs: HashSet<PathBuf>,
    /// Search query for filtering
    pub search_query: String,
    /// Project root directory
    pub project_root: PathBuf,
    /// Cached filesystem tree
    pub file_cache: FileTreeCache,
    /// Whether cache needs refresh
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectedItem {
    Entity(Entity),
    File(PathBuf),
    None,
}

/// Cached directory tree for efficient Slint sync
pub struct FileTreeCache {
    pub nodes: Vec<FileNodeData>,
    pub last_scan: Instant,
}

pub struct FileNodeData {
    pub path: PathBuf,
    pub name: String,
    pub is_directory: bool,
    pub extension: String,
    pub size: u64,
    pub modified: SystemTime,
}
```

### 5.4 Rust: FrameType enum (new)

```rust
/// Determines which center tab frame renders a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Scene,      // 3D viewport (Bevy renderer)
    Code,       // Monaco Editor (wry WebView)
    Web,        // Web browser (wry WebView direct)
    Image,      // Image viewer (Slint Image)
    Video,      // Video player (HTML5 <video> in wry)
    Audio,      // Audio player (HTML5 <audio> in wry)
    Document,   // Markdown/PDF viewer (wry)
    Table,      // CSV/TSV viewer (HTML table in wry)
    Hex,        // Hex viewer (wry)
    Text,       // Plain text fallback (Monaco plain)
}

impl FrameType {
    /// Resolve file extension to frame type
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            // Scene files
            "eep" | "gltf" | "glb" | "obj" | "fbx" | "stl" | "dae" => Self::Scene,
            // Code files
            "rs" | "lua" | "soul" | "ts" | "js" | "tsx" | "jsx"
            | "json" | "jsonc" | "toml" | "yaml" | "yml" | "ron"
            | "xml" | "html" | "css" | "scss" | "less"
            | "py" | "rb" | "go" | "c" | "cpp" | "h" | "hpp"
            | "java" | "kt" | "swift" | "zig" | "odin"
            | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd"
            | "sql" | "graphql" | "proto" | "wgsl" | "glsl" | "hlsl"
            | "ini" | "cfg" | "conf" | "env" | "gitignore" | "dockerignore"
            | "makefile" | "cmake" => Self::Code,
            // Images
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg"
            | "ico" | "tga" | "tiff" | "tif" | "exr" | "hdr" | "avif" => Self::Image,
            // Video
            "mp4" | "webm" | "mov" | "avi" | "mkv" | "flv" | "wmv" | "m4v" => Self::Video,
            // Audio
            "wav" | "ogg" | "mp3" | "flac" | "aac" | "wma" | "m4a" | "opus" => Self::Audio,
            // Documents
            "md" | "markdown" | "rst" | "txt" | "rtf" => Self::Document,
            "pdf" => Self::Document,
            // Tables
            "csv" | "tsv" | "psv" => Self::Table,
            // Archives and binaries → hex view
            "zip" | "tar" | "gz" | "7z" | "rar" | "exe" | "dll" | "so"
            | "bin" | "dat" | "wasm" => Self::Hex,
            // Fallback: try to detect text vs binary
            _ => Self::Text,
        }
    }

    /// Get the tab-type string for Slint
    pub fn to_tab_type(&self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Code => "code",
            Self::Web => "web",
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Document => "document",
            Self::Table => "table",
            Self::Hex => "hex",
            Self::Text => "code", // Text uses Monaco in plain mode
        }
    }

    /// Get the Monaco language ID for syntax highlighting
    pub fn to_language_id(ext: &str) -> &'static str {
        match ext.to_lowercase().as_str() {
            "rs" | "ron" => "rust",
            "lua" | "soul" => "lua",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" | "mjs" | "cjs" => "javascript",
            "json" | "jsonc" => "json",
            "toml" => "toml",
            "yaml" | "yml" => "yaml",
            "xml" | "xsl" | "xsd" => "xml",
            "html" | "htm" => "html",
            "css" => "css",
            "scss" => "scss",
            "less" => "less",
            "py" => "python",
            "rb" => "ruby",
            "go" => "go",
            "c" | "h" => "c",
            "cpp" | "hpp" | "cc" | "cxx" => "cpp",
            "java" => "java",
            "kt" | "kts" => "kotlin",
            "swift" => "swift",
            "zig" => "zig",
            "sh" | "bash" | "zsh" => "shell",
            "ps1" => "powershell",
            "sql" => "sql",
            "md" | "markdown" => "markdown",
            "wgsl" => "wgsl",
            "glsl" | "vert" | "frag" => "glsl",
            "hlsl" => "hlsl",
            "graphql" | "gql" => "graphql",
            "proto" => "protobuf",
            "dockerfile" => "dockerfile",
            _ => "plaintext",
        }
    }
}
```

---

## 6. Frame Types & File Routing

### 6.1 Scene Frame (existing)

The Bevy 3D viewport. Already works for `.eep` scenes. Extend to support:
- `.gltf` / `.glb` — Load via `bevy_gltf`
- `.obj` — Load via custom loader
- `.stl` — Load via custom loader

### 6.2 Code Frame (Monaco via Wry)

The crown jewel. Embed Monaco Editor in a wry WebView tab.

**How it works:**
1. Bundle a minimal HTML page with Monaco Editor loaded from CDN or local assets
2. When a code tab opens, create a wry WebView pointed at `file:///assets/monaco/index.html`
3. Pass file content and language ID via JavaScript injection
4. Monaco provides: syntax highlighting, intellisense, minimap, bracket matching, multi-cursor, find/replace
5. On save (Ctrl+S), Monaco sends content back to Rust via `window.ipc.postMessage()`
6. Rust writes to disk

**Monaco HTML template** (bundled in assets):
```html
<!DOCTYPE html>
<html>
<head>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/monaco-editor@0.52/min/vs/editor/editor.main.css">
</head>
<body style="margin:0; background:#1e1e1e; overflow:hidden;">
  <div id="editor" style="width:100%; height:100vh;"></div>
  <script src="https://cdn.jsdelivr.net/npm/monaco-editor@0.52/min/vs/loader.js"></script>
  <script>
    require.config({ paths: { vs: 'https://cdn.jsdelivr.net/npm/monaco-editor@0.52/min/vs' }});
    require(['vs/editor/editor.main'], function() {
      const editor = monaco.editor.create(document.getElementById('editor'), {
        value: '',
        language: 'plaintext',
        theme: 'vs-dark',
        automaticLayout: true,
        minimap: { enabled: true },
        fontSize: 14,
        fontFamily: "'Cascadia Code', 'Fira Code', 'JetBrains Mono', monospace",
        fontLigatures: true,
        scrollBeyondLastLine: false,
        renderWhitespace: 'selection',
        bracketPairColorization: { enabled: true },
      });

      // IPC: Receive file content from Rust
      window.setContent = (content, language) => {
        editor.setValue(content);
        monaco.editor.setModelLanguage(editor.getModel(), language);
      };

      // IPC: Send content back to Rust on save
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
        window.ipc.postMessage(JSON.stringify({
          type: 'save',
          content: editor.getValue()
        }));
      });

      // IPC: Report cursor position
      editor.onDidChangeCursorPosition((e) => {
        window.ipc.postMessage(JSON.stringify({
          type: 'cursor',
          line: e.position.lineNumber,
          column: e.position.column
        }));
      });

      // IPC: Report dirty state
      let savedContent = '';
      window.markSaved = () => { savedContent = editor.getValue(); };
      editor.onDidChangeModelContent(() => {
        const dirty = editor.getValue() !== savedContent;
        window.ipc.postMessage(JSON.stringify({ type: 'dirty', dirty }));
      });
    });
  </script>
</body>
</html>
```

### 6.3 Web Frame (existing)

Already implemented via `web_browser.slint` + `webview.rs`. No changes needed.

### 6.4 Image Frame (new Slint component)

```slint
export component ImageViewer inherits Rectangle {
    in property <image> source;
    in property <string> filename: "";
    in property <string> dimensions: "";  // "1920x1080"
    in property <string> file-size: "";   // "2.4 MB"

    background: #1e1e1e;

    VerticalLayout {
        // Info bar
        Rectangle {
            height: 28px;
            background: Theme.header-background;
            HorizontalLayout {
                padding: 4px 8px;
                Text { text: root.filename; color: Theme.text-primary; font-size: 12px; }
                Text { text: root.dimensions; color: Theme.text-secondary; font-size: 11px; }
                Text { text: root.file-size; color: Theme.text-secondary; font-size: 11px; }
            }
        }
        // Zoomable image area
        Rectangle {
            vertical-stretch: 1;
            // Checkerboard background for transparency
            Image {
                width: 100%;
                height: 100%;
                source: root.source;
                image-fit: contain;
            }
        }
    }
}
```

### 6.5 Video Frame (wry HTML5)

Render an HTML page with `<video>` tag. Wry handles the actual playback.

### 6.6 Audio Frame (wry HTML5)

Render an HTML page with `<audio>` tag plus waveform visualization.

### 6.7 Document Frame (wry Markdown)

Render Markdown to HTML using `pulldown-cmark` in Rust, then display in wry. For PDF, use PDF.js.

### 6.8 Table Frame (wry HTML)

Parse CSV/TSV in Rust, render as an HTML table in wry with sorting and filtering.

---

## 7. Icon System

### 7.1 Source: VS Code Material Icon Theme

**Repository:** [material-extensions/vscode-material-icon-theme](https://github.com/material-extensions/vscode-material-icon-theme)  
**License:** MIT  
**Icon location:** `icons/` directory — 800+ SVG files  
**Mapping source:** `src/core/icons/fileIcons.ts` — extension→icon name mapping  

### 7.2 Icon Directory Structure

```
assets/icons/
├── engine/          # Existing ECS class icons (49 SVGs)
│   ├── workspace.svg
│   ├── lighting.svg
│   ├── part.svg
│   └── ...
├── ui/              # Existing UI action icons (52 SVGs)
│   ├── cursor.svg
│   ├── move.svg
│   └── ...
├── filetypes/       # NEW — File type icons from material-icon-theme
│   ├── file.svg         # Default file icon
│   ├── folder.svg       # Default folder icon
│   ├── folder-open.svg  # Expanded folder icon
│   ├── rust.svg
│   ├── lua.svg
│   ├── javascript.svg
│   ├── typescript.svg
│   ├── json.svg
│   ├── toml.svg
│   ├── yaml.svg
│   ├── markdown.svg
│   ├── html.svg
│   ├── css.svg
│   ├── python.svg
│   ├── image.svg
│   ├── video.svg
│   ├── audio.svg
│   ├── pdf.svg
│   ├── database.svg
│   ├── console.svg
│   ├── git.svg
│   ├── docker.svg
│   ├── readme.svg
│   ├── license.svg
│   ├── settings.svg
│   ├── xml.svg
│   ├── svg.svg
│   ├── font.svg
│   ├── zip.svg
│   ├── key.svg
│   ├── certificate.svg
│   ├── table.svg
│   ├── word.svg
│   ├── go.svg
│   ├── c.svg
│   ├── cpp.svg
│   ├── java.svg
│   ├── kotlin.svg
│   ├── swift.svg
│   ├── zig.svg
│   ├── assembly.svg
│   ├── cmake.svg
│   ├── shader.svg       # WGSL/GLSL/HLSL
│   ├── proto.svg
│   └── ... (import ~80 most common)
└── folders/         # NEW — Folder-specific icons
    ├── folder-src.svg
    ├── folder-src-open.svg
    ├── folder-assets.svg
    ├── folder-assets-open.svg
    ├── folder-docs.svg
    ├── folder-docs-open.svg
    ├── folder-test.svg
    ├── folder-test-open.svg
    ├── folder-config.svg
    ├── folder-config-open.svg
    └── ... (import ~30 most common)
```

### 7.3 Priority Icon Import List (Phase 0)

**Tier 1 — Must Have (30 icons):**
`file`, `folder`, `folder-open`, `rust`, `lua`, `javascript`, `typescript`, `json`, `toml`, `yaml`, `markdown`, `html`, `css`, `python`, `image`, `video`, `audio`, `pdf`, `database`, `console`, `git`, `docker`, `settings`, `xml`, `svg`, `font`, `zip`, `key`, `table`, `readme`

**Tier 2 — Should Have (25 icons):**
`go`, `c`, `cpp`, `java`, `kotlin`, `swift`, `zig`, `assembly`, `cmake`, `shader` (wgsl), `proto`, `ruby`, `sass`, `less`, `react`, `vue`, `word`, `certificate`, `license`, `lock`, `hex`, `dll`, `lib`, `test-js`, `test-rs`

**Tier 3 — Nice to Have (25 icons):**
`haskell`, `ocaml`, `odin`, `dart`, `r`, `perl`, `lua`, `groovy`, `powershell`, `graphql`, `handlebars`, `pug`, `astro`, `nuxt`, `vscode`, `npm`, `yarn`, `pnpm`, `cargo`, `gradle`, `makefile`, `nginx`, `terraform`, `kubernetes`, `aws`

**Folder Icons (15 pairs = 30 icons):**
`folder-src`, `folder-assets`, `folder-docs`, `folder-test`, `folder-config`, `folder-dist`, `folder-build`, `folder-scripts`, `folder-lib`, `folder-node_modules`, `folder-target`, `folder-git`, `folder-github`, `folder-vscode`, `folder-images`

### 7.4 Rust Icon Resolver

```rust
/// Resolves a file path to its icon image for the Slint explorer
pub fn load_file_icon(path: &Path) -> slint::Image {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Special filenames first (highest priority)
    let icon_name = match filename.as_str() {
        "cargo.toml" | "cargo.lock" => "rust",
        "readme.md" | "readme.txt" | "readme" => "readme",
        "license" | "license.md" | "license.txt" => "license",
        "dockerfile" | "docker-compose.yml" => "docker",
        ".gitignore" | ".gitattributes" | ".gitmodules" => "git",
        "makefile" | "cmakelists.txt" => "cmake",
        _ => {
            // Extension-based lookup
            match ext.as_str() {
                "rs" | "ron" => "rust",
                "lua" | "soul" => "lua",
                "js" | "mjs" | "cjs" => "javascript",
                "ts" | "mts" | "cts" => "typescript",
                "jsx" => "react",
                "tsx" => "react_ts",
                "json" | "jsonc" => "json",
                "toml" => "toml",
                "yaml" | "yml" => "yaml",
                "md" | "markdown" => "markdown",
                "html" | "htm" => "html",
                "css" => "css",
                "scss" | "sass" => "sass",
                "less" => "less",
                "py" => "python",
                "rb" => "ruby",
                "go" => "go",
                "c" | "h" => "c",
                "cpp" | "hpp" | "cc" | "cxx" => "cpp",
                "java" => "java",
                "kt" | "kts" => "kotlin",
                "swift" => "swift",
                "zig" | "zon" => "zig",
                "xml" | "xsl" | "xsd" => "xml",
                "svg" => "svg",
                "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp"
                | "ico" | "tga" | "tiff" | "exr" | "hdr" | "avif" => "image",
                "mp4" | "webm" | "mov" | "avi" | "mkv" => "video",
                "wav" | "ogg" | "mp3" | "flac" | "aac" => "audio",
                "pdf" => "pdf",
                "sql" | "sqlite" | "db" => "database",
                "sh" | "bash" | "zsh" | "fish" | "bat" | "cmd" => "console",
                "ps1" => "powershell",
                "wgsl" | "glsl" | "hlsl" | "vert" | "frag" => "shader",
                "proto" => "proto",
                "csv" | "tsv" => "table",
                "zip" | "tar" | "gz" | "7z" | "rar" => "zip",
                "woff" | "woff2" | "ttf" | "otf" => "font",
                "pem" | "key" | "pub" => "key",
                "cer" | "crt" => "certificate",
                "doc" | "docx" | "rtf" | "odt" => "word",
                "exe" | "msi" => "exe",
                "dll" | "so" => "dll",
                "ini" | "cfg" | "conf" | "env" => "settings",
                "lock" => "lock",
                "graphql" | "gql" => "graphql",
                _ => "file", // Default
            }
        }
    };

    load_svg_icon(&format!("assets/icons/filetypes/{}.svg", icon_name))
}

/// Resolves a directory name to its folder icon
pub fn load_folder_icon(name: &str, expanded: bool) -> slint::Image {
    let suffix = if expanded { "-open" } else { "" };

    let folder_name = match name.to_lowercase().as_str() {
        "src" | "source" => "folder-src",
        "assets" | "asset" | "resources" | "res" => "folder-assets",
        "docs" | "doc" | "documentation" => "folder-docs",
        "test" | "tests" | "spec" | "specs" | "__tests__" => "folder-test",
        "config" | "configs" | "configuration" | ".config" => "folder-config",
        "dist" | "build" | "out" | "output" => "folder-dist",
        "scripts" | "script" => "folder-scripts",
        "lib" | "libs" | "library" => "folder-lib",
        "node_modules" => "folder-node_modules",
        "target" => "folder-target",
        ".git" => "folder-git",
        ".github" => "folder-github",
        ".vscode" => "folder-vscode",
        "images" | "img" | "icons" | "textures" => "folder-images",
        _ => "folder",
    };

    let icon_path = format!("assets/icons/folders/{}{}.svg", folder_name, suffix);
    // Fallback to default folder if specific icon doesn't exist
    load_svg_icon_with_fallback(&icon_path, &format!("assets/icons/filetypes/folder{}.svg", suffix))
}
```

---

## 8. Slint UI Components

### 8.1 Explorer Panel — Dual Mode

The explorer gets a tab bar at the top to switch between Game and Files views:

```slint
export component ExplorerPanel inherits Rectangle {
    // Mode: 0 = Game (ECS entities), 1 = Files (filesystem)
    in-out property <int> explorer-mode: 0;

    // Game tree data (existing)
    in property <[EntityNode]> entities: [];
    in property <[EntityNode]> workspace-entities: [];
    in property <[EntityNode]> lighting-entities: [];

    // File tree data (new)
    in property <[FileNode]> file-nodes: [];

    // Callbacks (existing)
    callback on-select-entity(int);
    callback on-expand-entity(int);
    callback on-collapse-entity(int);

    // Callbacks (new — file operations)
    callback on-select-file(string);        // path
    callback on-expand-folder(string);      // path
    callback on-collapse-folder(string);    // path
    callback on-open-file(string);          // path (double-click)
    callback on-rename-file(string, string); // old-path, new-name
    callback on-delete-file(string);        // path
    callback on-create-file(string, string); // parent-path, name
    callback on-create-folder(string, string);
    callback on-file-context-menu(string, length, length); // path, x, y

    VerticalLayout {
        // Mode tab bar
        Rectangle {
            height: 28px;
            HorizontalLayout {
                tab-game := Rectangle { /* "Game" tab */ }
                tab-files := Rectangle { /* "Files" tab */ }
            }
        }

        // Search bar
        Rectangle { height: 30px; /* ... */ }

        // Content — switches based on mode
        if root.explorer-mode == 0: ScrollView { /* existing entity tree */ }
        if root.explorer-mode == 1: ScrollView { /* new file tree */ }
    }
}
```

### 8.2 FileTreeItem (new component in theme.slint)

```slint
export component FileTreeItem inherits Rectangle {
    in property <int> depth: 0;
    in property <image> icon;
    in property <string> label: "";
    in property <bool> is-directory: false;
    in property <bool> expandable: false;
    in property <bool> expanded: false;
    in property <bool> selected: false;
    in property <bool> modified: false;
    in property <string> size-text: "";

    callback clicked();
    callback double-clicked();
    callback toggle-expanded();
    callback context-menu(length, length);

    // Similar to TreeItem but with:
    // - Double-click to open files
    // - Modified indicator (dot)
    // - File size display
    // - Folder expand/collapse arrow
}
```

### 8.3 Center Content Area — Extended Routing

```slint
// In main.slint — extend the content area conditional
if root.active-tab-type == "scene": Rectangle { /* 3D viewport */ }
if root.active-tab-type == "code": Rectangle { /* Monaco WebView placeholder */ }
if root.active-tab-type == "web": WebBrowser { /* existing */ }
if root.active-tab-type == "image": ImageViewer { /* new */ }
if root.active-tab-type == "video": Rectangle { /* video WebView placeholder */ }
if root.active-tab-type == "audio": Rectangle { /* audio WebView placeholder */ }
if root.active-tab-type == "document": Rectangle { /* markdown/PDF WebView placeholder */ }
if root.active-tab-type == "table": Rectangle { /* CSV WebView placeholder */ }
```

---

## 9. Rust Systems & Modules

### 9.1 Updated Module: `slint_ui.rs` — Unified Explorer Sync

**Replace `sync_explorer_to_slint` with `sync_unified_explorer_to_slint`:**

```rust
/// Syncs both ECS entities and filesystem to a single unified tree in Slint
pub fn sync_unified_explorer_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    perf: Option<Res<UIPerformance>>,
    explorer_state: Res<UnifiedExplorerState>,
    instances: Query<(Entity, &Instance)>,
    children_query: Query<&Children>,
    child_of_query: Query<&ChildOf>,
) {
    if let Some(ref perf) = perf {
        if perf.should_throttle(30) { return; }
    }
    let Some(slint_context) = slint_context else { return };
    let ui = &slint_context.window;
    
    let mut nodes = Vec::new();
    
    // 1. Build ECS entity nodes (Workspace, Lighting, Players, etc.)
    build_entity_nodes(&mut nodes, &instances, &children_query, &explorer_state);
    
    // 2. Build filesystem nodes starting at project root
    build_file_nodes(&mut nodes, &explorer_state.project_root, &explorer_state);
    
    // 3. Filter by search query if present
    if !explorer_state.search_query.is_empty() {
        filter_nodes(&mut nodes, &explorer_state.search_query);
    }
    
    // 4. Push to Slint
    let slint_nodes: Vec<TreeNode> = nodes.into_iter().map(|n| n.to_slint()).collect();
    ui.set_explorer_nodes(slint_nodes.into());
}

fn build_entity_nodes(
    nodes: &mut Vec<TreeNodeData>,
    instances: &Query<(Entity, &Instance)>,
    children_query: &Query<&Children>,
    state: &UnifiedExplorerState,
) {
    // Existing logic from sync_explorer_to_slint
    // Creates TreeNodeData with node_type: "entity"
}

fn build_file_nodes(
    nodes: &mut Vec<TreeNodeData>,
    root: &Path,
    state: &UnifiedExplorerState,
) {
    // Scan filesystem and create TreeNodeData with node_type: "file"
    // Use state.file_cache for efficiency
    // Respect state.expanded_dirs for tree depth
}

struct TreeNodeData {
    id: i32,
    name: String,
    icon_path: String,
    depth: i32,
    expandable: bool,
    expanded: bool,
    selected: bool,
    visible: bool,
    node_type: String, // "entity" or "file"
    class_name: String,
    path: String,
    is_directory: bool,
    extension: String,
    size: String,
    modified: bool,
}

impl TreeNodeData {
    fn to_slint(&self) -> TreeNode {
        // Convert to Slint TreeNode struct
    }
}
```

### 9.2 New Module: `file_explorer.rs`

```rust
// Table of Contents:
// 1. FileExplorerState — Resource tracking filesystem tree state
// 2. FileExplorerPlugin — Plugin registration
// 3. scan_directory — Recursive directory scanner (rayon parallel)
// 4. sync_file_tree_to_slint — Push FileNode list to Slint
// 5. handle_file_open — Open file in appropriate center tab
// 6. handle_file_operations — Create/rename/delete/move
// 7. watch_filesystem — Detect external changes (notify crate)

pub struct FileExplorerPlugin;

impl Plugin for FileExplorerPlugin {
    fn build(&self, app: &mut App) {
        app
            .init_resource::<FileExplorerState>()
            .add_message::<FileOpenEvent>()
            .add_message::<FileOperationEvent>()
            .add_systems(Update, (
                sync_file_tree_to_slint,  // Throttled: every 60 frames
                handle_file_open,
                handle_file_operations,
            ));
    }
}
```

### 9.2 New Module: `file_icons.rs`

```rust
// Table of Contents:
// 1. load_file_icon — Extension-based icon resolver
// 2. load_folder_icon — Directory name-based icon resolver
// 3. ICON_CACHE — Lazy static HashMap<String, slint::Image> for dedup
```

### 9.3 New Module: `monaco_bridge.rs`

```rust
// Table of Contents:
// 1. MonacoBridgePlugin — Plugin for Monaco ↔ Bevy communication
// 2. MonacoState — Resource tracking active Monaco instances
// 3. handle_monaco_ipc — Process IPC messages from Monaco (save, cursor, dirty)
// 4. send_content_to_monaco — Push file content to Monaco via JS injection
// 5. handle_code_save — Write file to disk on Ctrl+S
```

### 9.4 Extended: `slint_ui.rs`

Add new callbacks and sync systems:

```rust
// New SlintAction variants:
SlintAction::SelectFile(String),        // File selected in explorer
SlintAction::OpenFile(String),          // File double-clicked
SlintAction::ExpandFolder(String),      // Folder expanded
SlintAction::CollapseFolder(String),    // Folder collapsed
SlintAction::RenameFile(String, String),
SlintAction::DeleteFile(String),
SlintAction::CreateFile(String, String),
SlintAction::CreateFolder(String, String),
SlintAction::SwitchExplorerMode(i32),   // 0=Game, 1=Files

// New sync system:
fn sync_file_tree_to_slint(
    slint_context: Option<NonSend<SlintUiState>>,
    file_state: Res<FileExplorerState>,
    perf: Option<Res<UIPerformance>>,
) {
    // Throttle: every 60 frames
    // Build FileNode list from FileExplorerState.cache
    // Push to Slint via ui.set_file_nodes(...)
}
```

### 9.5 Extended: `webview.rs`

Add Monaco WebView management alongside browser WebViews:

```rust
// MonacoWebView — A wry WebView instance running Monaco Editor
// Separate from browser WebViews — these have IPC for save/cursor/dirty
// Managed per code tab (one Monaco instance per open file)
```

---

## 10. Monaco Editor Integration

### 10.1 Architecture

```
┌──────────────┐     IPC (JSON)      ┌──────────────┐
│  Bevy/Rust   │ ◄──────────────────► │  Wry WebView │
│              │                      │  (Monaco)    │
│ monaco_bridge│  save, cursor, dirty │              │
│   .rs        │ ────────────────────►│ index.html   │
│              │  setContent(text,    │ + monaco.js  │
│              │   language)          │              │
└──────────────┘                      └──────────────┘
```

### 10.2 IPC Protocol

**Rust → Monaco (JavaScript injection):**
```javascript
window.setContent("fn main() { ... }", "rust");
window.markSaved();
window.setReadOnly(true);
window.setTheme("vs-dark");
```

**Monaco → Rust (IPC postMessage):**
```json
{ "type": "save", "content": "fn main() { ... }" }
{ "type": "cursor", "line": 42, "column": 15 }
{ "type": "dirty", "dirty": true }
{ "type": "ready" }
```

### 10.3 Offline Support

Bundle Monaco Editor locally in `assets/monaco/` for offline use:
- `assets/monaco/vs/` — Monaco core files (~5MB)
- `assets/monaco/index.html` — Editor template
- Load via `file:///` URL in wry

### 10.4 Language Support Priority

| Language | Monaco Built-in | Custom Grammar Needed |
|----------|----------------|----------------------|
| Rust | ✅ Yes | No |
| Lua | ❌ No | No |
| Soul | ✅ Yes | Yes (Engligh/Rune-based) |
| TypeScript/JavaScript | ✅ Yes | No |
| JSON/TOML/YAML | ✅ Yes | No |
| HTML/CSS/SCSS | ✅ Yes | No |
| Python/Go/C/C++ | ✅ Yes | No |
| WGSL | ❌ No | Yes (shader lang) |
| GLSL/HLSL | ❌ No | Yes (shader lang) |
| RON | ❌ No | Yes (Rust-like) |

---

## 11. VS Code Keybindings

### 11.1 File Navigation

| Shortcut | Action | Implementation |
|----------|--------|---------------|
| `Ctrl+P` | Quick Open (file picker) | Command bar with file search |
| `Ctrl+Shift+E` | Focus Explorer | Switch to explorer panel |
| `Ctrl+Shift+F` | Search in Files | Ripgrep integration |
| `Ctrl+Tab` | Switch between open tabs | Tab cycling |
| `Ctrl+W` | Close active tab | Already implemented |
| `Ctrl+T` | Open new web tab | Already implemented |
| `Ctrl+\` | Split editor | Future |
| `Ctrl+B` | Toggle sidebar | Toggle explorer panel |
| `` Ctrl+` `` | Toggle terminal | Toggle output panel |

### 11.2 Code Editing (handled by Monaco)

| Shortcut | Action |
|----------|--------|
| `Ctrl+S` | Save file |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+F` | Find |
| `Ctrl+H` | Find and Replace |
| `Ctrl+G` | Go to Line |
| `Ctrl+D` | Select next occurrence |
| `Ctrl+Shift+K` | Delete line |
| `Alt+Up/Down` | Move line up/down |
| `Ctrl+/` | Toggle comment |
| `Ctrl+Shift+P` | Command palette (Monaco) |

---

## 12. Phased Implementation Plan

### Phase 0: Foundation — Documentation & Icons (1-2 days)

**Goal:** Establish the icon pipeline and architecture documentation.

- [x] Create this architecture document
- [ ] Clone material-icon-theme SVGs (Tier 1: 30 file icons + 15 folder icon pairs)
- [ ] Create `assets/icons/filetypes/` directory
- [ ] Create `assets/icons/folders/` directory
- [ ] Create `file_icons.rs` — extension→icon mapping module
- [ ] Verify all SVGs render correctly in Slint

**Deliverable:** Icon infrastructure ready, all Tier 1 icons imported.

### Phase 1: Unified Explorer Single Tree (5-7 days)

**Goal:** Merge ECS entities and filesystem into one unified tree in the Explorer panel.

**Tasks:**
1. **Rename `EntityNode` to `TreeNode`** in `explorer.slint` with unified fields
2. **Update `ExplorerPanel`** to use single `tree-nodes` property (remove separate workspace/lighting arrays)
3. **Create `UnifiedExplorerState`** resource replacing `ExplorerState`:
   - Merge `expanded_entities` + `expanded_dirs`
   - Add `SelectedItem` enum (Entity | File | None)
   - Add `project_root` PathBuf
   - Add `file_cache` for filesystem scanning
4. **Rewrite `sync_explorer_to_slint`** → `sync_unified_explorer_to_slint`:
   - Build ECS entity nodes first (Workspace, Lighting, Players, etc.)
   - Append filesystem nodes starting at project root
   - Single flat Vec<TreeNode> with proper depth values
   - Load icons via `load_class_icon()` for entities, `load_file_icon()` for files
5. **Create `file_icons.rs` module**:
   - `load_file_icon(extension)` — Map extension to SVG path
   - `load_folder_icon(name, expanded)` — Map folder name to icon
   - Use the 75 SVG icons created in Phase 0
6. **Update callbacks** in `explorer.slint`:
   - `on-select-node(id, node-type)` — handles both entities and files
   - `on-expand-node(id, node-type)` — expands entities or directories
   - `on-open-node(id, node-type)` → triggers tab creation
7. **Add file watcher** with `notify` crate for live filesystem updates
8. **Test:** Single tree shows Workspace entities followed by src/, assets/, docs/ foldersders, see correct icons

**Deliverable:** File tree visible in Explorer, double-click opens files.

### Phase 2: Monaco Code Editor (5-7 days)

**Goal:** Replace plain TextEdit with Monaco Editor for code files.

- [ ] Bundle Monaco Editor in `assets/monaco/`
- [ ] Create `monaco_bridge.rs` — IPC bridge
- [ ] Create Monaco HTML template with dark theme
- [ ] Extend `webview.rs` to manage Monaco WebView instances
- [ ] Implement `setContent()` / `markSaved()` IPC
- [ ] Implement save-on-Ctrl+S → write to disk
- [ ] Implement cursor position sync → status bar
- [ ] Implement dirty state sync → tab dot indicator
- [ ] Add language detection from file extension
- [ ] Test: open `.rs` file, see syntax highlighting, save, see dirty indicator

**Deliverable:** Full code editing with syntax highlighting for 20+ languages.

### Phase 3: Media Frames — Image, Video, Audio, Document (3-4 days)

**Goal:** View all media types in center tabs.

- [ ] Create `ImageViewer` Slint component
- [ ] Create video player HTML template (wry)
- [ ] Create audio player HTML template (wry)
- [ ] Create Markdown renderer (pulldown-cmark → HTML → wry)
- [ ] Create PDF viewer (PDF.js in wry)
- [ ] Create CSV/TSV table viewer (HTML table in wry)
- [ ] Extend `CenterTab` model with new tab types
- [ ] Extend content area routing in `main.slint`
- [ ] Test: open image, video, audio, markdown, PDF, CSV files

**Deliverable:** All common file types viewable in tabs.

### Phase 4: File Operations (3-4 days)

**Goal:** Full CRUD for files and folders from the Explorer.

- [ ] Implement context menu for file tree (right-click)
- [ ] Create File → New File / New Folder
- [ ] Implement inline rename (F2)
- [ ] Implement delete with confirmation dialog
- [ ] Implement move via drag-and-drop
- [ ] Implement copy/paste for files
- [ ] Add filesystem watcher (notify crate) for external changes
- [ ] Test: create, rename, delete, move files from Explorer

**Deliverable:** Full file management without leaving the editor.

### Phase 5: VS Code Keybindings & Search (3-5 days)

**Goal:** VS Code-level keyboard productivity.

- [ ] Implement Ctrl+P quick file open (fuzzy search)
- [ ] Implement Ctrl+Shift+F search in files (ripgrep)
- [ ] Implement Ctrl+B toggle sidebar
- [ ] Implement Ctrl+Tab tab cycling
- [ ] Implement Ctrl+Shift+E focus explorer
- [ ] Add breadcrumb bar above editor (file path)
- [ ] Add status bar (line:col, language, encoding, file size)
- [ ] Test: all keybindings work, search finds results

**Deliverable:** VS Code-level navigation and search.

### Phase 6: Polish & Integration (5-7 days)

**Goal:** Production quality, edge cases, performance.

- [ ] Tab persistence across sessions (save/restore open tabs)
- [ ] Recent files list
- [ ] File change detection (external editor modified file)
- [ ] Minimap in Monaco
- [ ] Split view (two editors side by side)
- [ ] Drag file from Explorer to 3D viewport (asset import)
- [ ] Drag file from OS file manager into Explorer
- [ ] Performance profiling — ensure <16ms frame time
- [ ] Import Tier 2 + Tier 3 icons
- [ ] Accessibility audit (keyboard navigation, screen reader)
- [ ] Documentation: user guide for the unified explorer

**Deliverable:** Ship-ready feature.

---

## 13. Open Source References

### Primary Sources

| Resource | URL | What We Use |
|----------|-----|-------------|
| **VS Code** | github.com/microsoft/vscode | Architecture reference, keybinding design |
| **Monaco Editor** | github.com/microsoft/monaco-editor | Embeddable code editor (MIT) |
| **Material Icon Theme** | github.com/material-extensions/vscode-material-icon-theme | 800+ SVG file icons (MIT) |
| **Wry** | github.com/nicedoc/wry | WebView for Monaco/media frames |
| **pulldown-cmark** | crates.io/crates/pulldown-cmark | Markdown→HTML for document frame |
| **PDF.js** | mozilla.github.io/pdf.js | PDF rendering in wry |
| **notify** | crates.io/crates/notify | Filesystem watcher |
| **ignore** | crates.io/crates/ignore | .gitignore-aware file walking |

### Architecture References

| Project | What to Study |
|---------|--------------|
| **Zed Editor** (github.com/zed-industries/zed) | Rust-native editor with GPU rendering, file tree, tabs |
| **Lapce** (github.com/lapce/lapce) | Rust editor with Wry integration, plugin system |
| **Godot** (github.com/godotengine/godot) | Built-in script editor inside game engine |

---

## 14. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Monaco CDN unavailable offline | Medium | High | Bundle Monaco locally in assets/ |
| Wry WebView performance on Linux | Low | Medium | Test on all platforms early |
| Large directory trees slow to scan | Medium | Medium | Rayon parallel scan + lazy loading |
| Monaco ↔ Rust IPC latency | Low | Low | Batch cursor updates, debounce dirty |
| SVG icon rendering in Slint | Low | Low | Already proven with 100+ icons |
| File watcher flooding events | Medium | Low | Debounce with 500ms cooldown |
| Memory usage with many open tabs | Medium | Medium | Limit to 20 open tabs, LRU eviction |
| Monaco keyboard shortcuts conflict with Bevy | High | High | Only forward keys to Monaco when code tab is focused |

---

## 15. Testing Strategy

### Unit Tests

- `FrameType::from_extension()` — all extension mappings
- `load_file_icon()` — all icon resolutions
- `load_folder_icon()` — all folder name mappings
- `FileExplorerState` — expand/collapse/select

### Integration Tests

- Open each file type → correct frame renders
- Save file via Monaco → content written to disk
- Create/rename/delete file → explorer updates
- External file change → explorer refreshes

### Manual Testing Checklist

- [ ] Browse project directory with 1000+ files — no lag
- [ ] Open 10 code tabs simultaneously — no memory spike
- [ ] Edit Rust file with syntax highlighting — correct colors
- [ ] Save file — no data loss
- [ ] Open image — correct display with zoom
- [ ] Open video — playback works
- [ ] Open PDF — pages render
- [ ] Ctrl+P quick open — finds files fast
- [ ] Ctrl+Shift+F search — results correct
- [ ] All VS Code keybindings — work as expected

---

## Appendix A: CenterTab Type String Registry

| `tab-type` | Frame | Renderer | Wry? |
|-----------|-------|----------|------|
| `"scene"` | Scene Frame | Bevy 3D viewport | No |
| `"code"` | Code Frame | Monaco Editor | Yes |
| `"web"` | Web Frame | Direct browser | Yes |
| `"image"` | Image Frame | Slint Image | No |
| `"video"` | Video Frame | HTML5 `<video>` | Yes |
| `"audio"` | Audio Frame | HTML5 `<audio>` | Yes |
| `"document"` | Document Frame | Markdown/PDF | Yes |
| `"table"` | Table Frame | HTML table | Yes |
| `"hex"` | Hex Frame | Hex viewer | Yes |

## Appendix B: New Cargo Dependencies

```toml
# Filesystem watching for external changes
notify = "7"
# .gitignore-aware file walking
ignore = "0.4"
# Markdown → HTML rendering
pulldown-cmark = "0.12"
# File size formatting
humansize = "2"
```

## Appendix C: Estimated Line Counts

| Module | New Lines | Modified Lines |
|--------|-----------|---------------|
| `file_explorer.rs` | ~400 | — |
| `file_icons.rs` | ~250 | — |
| `monaco_bridge.rs` | ~300 | — |
| `explorer.slint` | ~150 | ~50 |
| `theme.slint` (FileTreeItem) | ~80 | — |
| `main.slint` (routing) | ~60 | ~30 |
| `image_viewer.slint` | ~80 | — |
| `slint_ui.rs` (callbacks + sync) | ~200 | ~100 |
| `webview.rs` (Monaco mgmt) | ~150 | ~50 |
| `keybindings.rs` (new shortcuts) | ~30 | ~20 |
| `mod.rs` (new types) | ~50 | ~20 |
| `assets/monaco/index.html` | ~100 | — |
| SVG icons | ~110 files | — |
| **Total** | **~1,850** | **~270** |

---

*This document is the single source of truth for the Unified Explorer feature. Update it as implementation progresses.*
