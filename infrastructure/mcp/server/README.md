# `@eustress/mcp-server`

Model Context Protocol server for [Eustress Engine](https://eustress.dev).
Exposes your Universe — Spaces, Rune scripts, entities, assets, git
state, and Workshop conversation archives — to any MCP-compatible AI
client (Windsurf, Claude Desktop, Cursor, Zed).

Once installed, an IDE's AI assistant can navigate your project the way
Eustress Studio does: by Space, Script, and Entity identity rather than
raw filesystem paths.

## Install

### Via Windsurf marketplace (recommended)

Search for **"Eustress Engine"** in Windsurf's MCP marketplace. One
click installs, wires the config, and lets you point it at your
Universe folder.

### Manual (any MCP client)

```jsonc
// ~/.codeium/windsurf/mcp_config.json       (macOS/Linux)
// %APPDATA%\Codeium\windsurf\mcp_config.json (Windows)
{
  "mcpServers": {
    "eustress-engine": {
      "command": "npx",
      "args": ["-y", "@eustress/mcp-server"],
      "env": {
        "EUSTRESS_UNIVERSE": "/absolute/path/to/your/Universe"
      }
    }
  }
}
```

Restart Windsurf. The Eustress tools appear in the AI chat's tool
drawer.

## Resources (v2)

The server exposes Universe content as **addressable, pinnable,
live-updating** MCP resources. When you or the AI pin one of these URIs,
the external IDE's chat keeps the content fresh automatically — save a
file in Eustress Studio and the pinned resource refreshes in Windsurf
without a re-query.

| URI shape | Content |
|---|---|
| `eustress://space/{space}` | Markdown overview of a Space — services, script counts, top-level listing. |
| `eustress://script/{space}/{+path}` | A script's source + summary bundled into one markdown document. |
| `eustress://entity/{space}/{+path}` | A folder-based entity's `_instance.toml` + class metadata. |
| `eustress://file/{space}/{+path}` | Any text file under a Space (rejects binary extensions). |
| `eustress://conversation/{session_id}` | A persisted Workshop chat session. |
| `eustress://brief/{product}` | An `ideation_brief.toml` by product name. |

The URIs are 1:1 with Workshop's `@mention` canonical paths
(`@script:Space1/SoulService/foo` ↔ `eustress://script/Space1/SoulService/foo`).
Anything you can `@` in Studio, you can pin in Windsurf.

`resources/templates/list` advertises these schemes so the AI can build
URIs for things it discovers out-of-band, and `resources/subscribe` lets
clients opt in to live updates on specific URIs.

## Tools (v1)

| Tool | Purpose |
|---|---|
| `eustress_list_universes` | Discover Universes on disk (search roots + Universe enclosing cwd). |
| `eustress_set_default_universe` | Change the server's default Universe mid-session — no restart. |
| `eustress_list_spaces` | Every Space under `{universe}/Spaces/`. |
| `eustress_list_scripts` | Folder-based Rune/Luau scripts, with class + source path. |
| `eustress_read_script` | Source + summary for one script (canonical `<folder>.rune` + `<folder>.md`, with legacy fallback). |
| `eustress_find_entity` | Case-insensitive name search across entity TOMLs. |
| `eustress_list_assets` | Meshes, textures, GUIs, audio — filtered by kind. |
| `eustress_search_universe` | Text search across `.rune`, `.toml`, `.md`. |
| `eustress_git_status` | `git status --porcelain=v1` parsed into entries. |
| `eustress_git_log` | Recent commits (default 20). |
| `eustress_git_diff` | Uncommitted diff for one path or the whole Universe. |
| `eustress_create_script` | Scaffolds a new script folder using Studio's canonical layout. |
| `eustress_get_conversation` | Load a persisted Workshop conversation. |

## Dynamic Universe selection

The server is **never locked to a single Universe**. Resolution, first
match wins:

1. Explicit `universe: "<abs-path>"` in the tool call arguments.
2. Walk-up from a `path` arg — if the client points at
   `/foo/Universe1/Spaces/Space1/script.rune`, the server auto-detects
   `/foo/Universe1/` as the Universe.
3. `state.currentUniverse` — either the launch default
   (`--universe` / `EUSTRESS_UNIVERSE`) or whatever was last set via
   `eustress_set_default_universe`.
4. Walk-up from the process's CWD as a last-ditch guess.

Use `eustress_list_universes` to enumerate Universes the server can
see (searches `EUSTRESS_UNIVERSES_PATH` plus `~/Eustress`,
`~/Documents/Eustress`, and the current working directory). Use
`eustress_set_default_universe` to pin one for the remainder of the
session.

Launching with no Universe configured is valid — tools error cleanly
with "No Universe configured" until one is set or passed per call.

## Requirements

- Node.js 18+
- A Universe directory containing `Spaces/` (Eustress's standard
  project layout). The server warns if the target doesn't look like a
  Universe but still runs; per-call `universe` overrides let clients
  target the right place.
- `git` on PATH if you use the git tools.

## Running standalone

```bash
# npx — downloads on demand, good for quick tries.
npx -y @eustress/mcp-server --universe /path/to/Universe

# Global install — faster startups, same behavior.
npm i -g @eustress/mcp-server
eustress-mcp-server --universe /path/to/Universe
```

The server writes human logs to stderr and MCP JSON-RPC to stdout.
Don't pipe both to the same file in production — the client reads
stdout as structured data.

## Privacy

The server runs locally under your user. Nothing leaves your machine;
the AI client (Windsurf / Cursor / etc.) talks to it over stdio. File
reads are bounded (256 KB per file) to keep responses manageable; if a
file is truncated the response flags it so the client can page.

## Development

See [`../README.md`](../README.md) in the repo for build/test/publish
instructions.

## License

MIT
