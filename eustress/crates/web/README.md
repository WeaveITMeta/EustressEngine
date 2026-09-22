# Eustress Web

Leptos-based frontend for Eustress Engine, built with Trunk.

## Prerequisites

```bash
# Install Trunk
cargo install trunk

# Add WASM target
rustup target add wasm32-unknown-unknown

# Optional: Install wasm-opt for smaller builds
# Download from https://github.com/WebAssembly/binaryen/releases
```

## Development

```bash
# Navigate to web crate
cd crates/web

# Start dev server (hot-reload)
trunk serve --open

# Build for production
trunk build --release
```

The dev server runs at `http://localhost:3000`.

## Project Structure

```
web/
├── Cargo.toml          # Dependencies (Leptos, gloo, etc.)
├── Trunk.toml          # Trunk build configuration
├── index.html          # HTML entry point
├── style/
│   └── main.css        # Global styles
└── src/
    ├── main.rs         # WASM entry point
    ├── lib.rs          # Library exports
    ├── app.rs          # Root App component + Router
    ├── state.rs        # Global state (auth, theme)
    ├── api/            # HTTP client + API functions
    │   ├── mod.rs
    │   ├── auth.rs     # Login/register/logout
    │   └── projects.rs # CRUD for projects
    ├── components/     # Reusable UI components
    │   ├── mod.rs
    │   ├── layout.rs   # Header, Sidebar, Footer
    │   ├── common.rs   # Button, Card, Modal, etc.
    │   └── forms.rs    # TextInput, Checkbox, Select
    ├── pages/          # Route pages
    │   ├── mod.rs
    │   ├── home.rs     # Landing page
    │   ├── login.rs    # Auth page
    │   ├── dashboard.rs
    │   ├── projects.rs # Project listing
    │   ├── editor.rs   # 3D editor (canvas host)
    │   └── not_found.rs
    └── utils.rs        # DOM/format/validation helpers
```

## Architecture

```
┌─────────────────┐     HTTP      ┌─────────────────┐     SQLx     ┌──────────┐
│  Leptos WASM    │ ◄───────────► │  Backend API    │ ◄──────────► │ Database │
│  (This crate)   │               │  (crates/backend)│              │          │
└─────────────────┘               └─────────────────┘              └──────────┘
```

- **Frontend (this crate)**: Leptos CSR app compiled to WASM
- **Backend (future)**: Axum + SQLx server for auth/projects API
- **Database**: PostgreSQL (recommended) or SQLite

## Environment Variables

Set via `trunk serve` or in `Trunk.toml`:

| Variable | Default | Description |
|----------|---------|-------------|
| `API_URL` | `http://localhost:7000` | Backend API base URL |

## Features

- [x] Reactive UI with Leptos
- [x] Client-side routing
- [x] Auth state management
- [x] Dark/light theme toggle
- [x] API client with auth headers
- [x] Form components
- [x] Modal dialogs
- [x] Loading states
- [ ] Backend integration (needs `crates/backend`)
- [ ] Eustress WASM engine embedding

## Building for Production

```bash
trunk build --release
cargo run --no-default-features --features ssr --bin prerender
wrangler pages deploy dist --project-name eustress --branch main
```

Output is in `dist/`, served by Cloudflare Pages at eustress.dev.

The second step matters for anything that reads the site without running
WebAssembly: AI agents, link unfurlers, and search engines before their
renderer gets to a page. The Trunk shell has an empty `#app`, so on its own a
crawler sees the `<head>` and nothing else. `prerender` renders every route in
`sitemap.xml` on the native target and writes `dist/<route>.html` with the
page markup inside `#app`; the WASM build empties that div and mounts on top
(`mount_app` in `lib.rs`). Adding a public route means adding it to
`sitemap.xml`, which is the one list both the crawlers and the prerender read.

It is a native build of the same crate with the `ssr` feature instead of
`csr`, so a component that touches the browser while it is being constructed
(rather than inside an effect or an event handler) panics during the render.
The bin reports the route and skips it; pass `--strict` to fail the build
instead.

`llms.txt`, `robots.txt` and `sitemap.xml` at the crate root are copied to
the root of `dist/` by `index.html`, along with Markdown copies of the Website
Service docs under `dist/docs/`.

## Next Steps

1. **Create `crates/backend`** with Axum + SQLx for the API
2. **Embed Eustress engine** in the editor canvas via WASM
3. **Add WebSocket** for real-time collaboration
