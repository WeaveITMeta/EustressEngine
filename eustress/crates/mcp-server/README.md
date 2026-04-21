# Eustress MCP Server

Native-Rust [Model Context Protocol](https://modelcontextprotocol.io) server
for the Eustress Engine. Ships inside the engine installer so external AI
clients (Windsurf, Claude Desktop, Cursor, Zed) can read your Universe —
Spaces, Rune scripts, entities, assets, git state, Workshop history — through
a curated MCP surface.

For the full design rationale, URI scheme table, and tool contract
invariants, read [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Install

The binary is bundled with Eustress Engine. After installing the engine, the
executable lives next to `eustress-engine`:

```
<install-dir>/eustress-mcp.exe        # Windows
<install-dir>/eustress-mcp            # macOS / Linux
```

No separate download, no Node.js, no bun.

## Build from source

```bash
cd eustress
cargo build --release --bin eustress-mcp
# → eustress/target/release/eustress-mcp  (≈ 2.7 MB stripped)
```

## Configure your IDE

Pick the config file that matches your client:

| Client            | Config file (macOS/Linux)                  | Config file (Windows)                                       |
|-------------------|--------------------------------------------|-------------------------------------------------------------|
| Windsurf          | `~/.codeium/windsurf/mcp_config.json`      | `%APPDATA%\Codeium\windsurf\mcp_config.json`                |
| Cursor            | `~/.cursor/mcp.json`                       | `%USERPROFILE%\.cursor\mcp.json`                            |
| Claude Desktop    | `~/Library/Application Support/Claude/claude_desktop_config.json` | `%APPDATA%\Claude\claude_desktop_config.json` |

Paste this into the client's config, adjusting paths:

```json
{
  "mcpServers": {
    "eustress-engine": {
      "command": "C:/Program Files/Eustress Engine/eustress-mcp.exe",
      "args": [],
      "env": {
        "EUSTRESS_UNIVERSES_PATH": "C:\\Users\\you\\Documents\\Eustress"
      }
    }
  }
}
```

`EUSTRESS_UNIVERSES_PATH` is optional — omit it and the server falls back to
`~/Eustress`, `~/Documents/Eustress`, and a cwd walk. Multiple roots can be
separated with `;` on Windows or `:` elsewhere.

Restart the IDE after editing the config. MCP servers are loaded at launch.

Eustress Engine has a **Help → Setup MCP** menu that links to the online
docs with ready-to-paste snippets for each IDE — convenient for non-repo
users who don't have this README handy.

## Verify

Ask the assistant: *"list my Eustress Spaces."*

It should call `eustress_list_spaces` and return whatever's under
`{Universe}/Spaces/`. If it instead says "no resources" or returns only a
`eustress://help/setup` pseudo-resource, the server couldn't find a
Universe — set `EUSTRESS_UNIVERSE` or `EUSTRESS_UNIVERSES_PATH` and
restart.

## Tool surface

13 tools, prefixed `eustress_` to avoid collisions with other MCP servers.
Full table + contracts in [`ARCHITECTURE.md`](./ARCHITECTURE.md#tool-surface).

| Tool                            | Purpose                                           |
|---------------------------------|---------------------------------------------------|
| `eustress_list_universes`       | Discover Universes on disk.                       |
| `eustress_set_default_universe` | Change the default Universe mid-session.          |
| `eustress_list_spaces`          | Every Space under `{universe}/Spaces/`.           |
| `eustress_list_scripts`         | Every Rune/Luau script folder in a Space.         |
| `eustress_read_script`          | Source + summary of one script.                   |
| `eustress_find_entity`          | Find entities by name.                            |
| `eustress_list_assets`          | Enumerate assets (meshes, textures, GUIs, audio). |
| `eustress_search_universe`      | Text search across `.rune`, `.toml`, `.md`.       |
| `eustress_git_status`           | `git status --porcelain`.                         |
| `eustress_git_log`              | Recent commits.                                   |
| `eustress_git_diff`             | Uncommitted diff.                                 |
| `eustress_create_script`        | Create a new Rune script folder.                  |
| `eustress_get_conversation`     | Load a Workshop conversation archive.             |

## Resource surface

6 resource kinds, all subscribable. The file watcher emits
`notifications/resources/updated` as files change, so pinned resources
stay fresh without re-invocation.

| URI template                            | Content                                        |
|-----------------------------------------|------------------------------------------------|
| `eustress://space/{space}`              | Markdown overview of a Space.                  |
| `eustress://script/{space}/{+path}`     | Source + summary bundle.                       |
| `eustress://entity/{space}/{+path}`     | `_instance.toml` + class + name.               |
| `eustress://file/{space}/{+path}`       | Any text file under a Space.                   |
| `eustress://conversation/{session_id}`  | Persisted Workshop session JSON.               |
| `eustress://brief/{product}`            | `ideation_brief.toml` by product name.         |

URIs are 1:1 with Workshop's `@mention` canonical paths, so anything you
can @-mention inside Eustress Engine you can pin as a resource from your IDE.

## Troubleshooting

**Server starts but sees no Spaces.**
Check `EUSTRESS_UNIVERSES_PATH`. Run the `eustress_list_universes` tool
to see what the server discovered, then call `eustress_set_default_universe`
to pin one.

**Saves in Eustress Engine don't propagate to pinned resources.**
The subscription model only kicks in after the client calls
`resources/subscribe`. Most IDEs do this automatically when you pin a
resource in chat; some require an explicit "pin & watch" action.

**Stderr is full of `tracing` output.**
That's intended — all logs go to stderr to keep stdout clean for the
JSON-RPC channel. Set `RUST_LOG=warn` (or `error`) to quiet it.

**Tool call fails with "path escapes Universe root".**
The path-safety gatekeeper is refusing something that resolves outside
the Universe. Use paths relative to the Universe root (or absolute paths
inside it) — `../../etc/passwd`-style escapes are rejected.

## License

MIT OR Apache-2.0 — same as the rest of the Eustress workspace.
