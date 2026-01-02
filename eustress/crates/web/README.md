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
```

Output is in `dist/`. Deploy to any static host (Netlify, Vercel, S3, etc.).

## Next Steps

1. **Create `crates/backend`** with Axum + SQLx for the API
2. **Embed Eustress engine** in the editor canvas via WASM
3. **Add WebSocket** for real-time collaboration
