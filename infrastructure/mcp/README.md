# Eustress MCP — Developer README

Home of the [Model Context Protocol](https://modelcontextprotocol.io)
server that exposes the Eustress Universe to external AI clients.

For the design rationale (why TypeScript, why stdio, tool surface,
Phase 2 engine bridge) read [`ARCHITECTURE.md`](./ARCHITECTURE.md).
For end-user install steps read [`server/README.md`](./server/README.md)
— that one ends up on the npm listing.

## Repo layout

```
infrastructure/mcp/
├── ARCHITECTURE.md         design doc
├── README.md               this file — developer quick-start
├── server/                 TypeScript server source
│   ├── package.json
│   ├── tsconfig.json
│   ├── .gitignore
│   ├── src/
│   │   ├── index.ts        stdio transport + handler registration
│   │   ├── universe.ts     fs helpers (spaces, scripts, entities, search)
│   │   └── tools.ts        tool schemas + handlers
│   └── README.md           (shown on the npm listing)
└── config/
    └── windsurf.json       copy-paste `mcp_config.json` snippet
```

## Dev loop

```bash
cd infrastructure/mcp/server
npm install        # see "Why npm for install?" in ../../extensions/lsp/README.md
bun run watch      # rebuilds on save via tsc --watch

# In another shell, run it against a test Universe:
EUSTRESS_UNIVERSE=/path/to/Universe1 bun run start

# Or point Windsurf at `dist/index.js` while iterating:
#   "command": "node", "args": ["/abs/path/to/dist/index.js"]
```

The server is small enough that end-to-end iteration is just "edit
tools.ts, save, the watcher recompiles, restart Windsurf's MCP
connection" — no special tooling needed.

## Testing manually

Easiest path: the [MCP Inspector](https://github.com/modelcontextprotocol/inspector):

```bash
npx @modelcontextprotocol/inspector \
  node dist/index.js --universe /path/to/Universe
```

Inspector gives you a browser UI to list tools, fire calls with
schema-validated arguments, and see the responses. Ship-quality smoke
testing before pushing a new version.

## Publishing

### npm

```bash
cd server
npm version patch            # or minor / major
npm publish --access public  # first publish needs the `@eustress` scope configured
```

The `prepublishOnly` hook runs `bun run build` so we never publish a
stale `dist/`.

### Windsurf marketplace

Windsurf pulls from the public npm registry. Once a new version is
published and tagged `latest`, users who already installed via the
marketplace get the update on their next MCP reconnect. No separate
marketplace submission per version.

For the initial listing, submit the server at
[windsurf.com/mcp/submit](https://windsurf.com/mcp/submit) (exact URL
depends on current Windsurf docs) with:

- Package: `@eustress/mcp-server`
- Category: Game Engines / Developer Tools
- Keywords: `rune`, `eustress`, `game-engine`
- Icon: our diamond glyph (reuse
  `infrastructure/extensions/lsp/vscode/icons/rune.svg`)

### Other clients

No action needed — any MCP client that reads `mcpServers` in a JSON
config file works with the template at [`config/windsurf.json`](./config/windsurf.json).
Cursor, Zed, Claude Desktop, and Neovim's `mcp.nvim` all use the same
shape.

## Versioning

The server's `package.json` version and the engine crate's version don't
have to match (the MCP surface is decoupled from Studio internals).
We do align on **major** bumps so "Eustress 1.x" always pairs with
"@eustress/mcp-server 1.x" marketing-wise.

## Phase 2 — Engine bridge (planned)

When Eustress Studio ships with `--mcp-port 24786`, the server will:

1. Probe `http://localhost:24786/health` on startup.
2. If reachable, append bridged tools (`eustress_query_ecs`,
   `eustress_get_sim_value`, `eustress_execute_rune`) to the capability
   list.
3. Absent the engine, those tools stay hidden so clients never see
   broken endpoints.

See [`ARCHITECTURE.md`](./ARCHITECTURE.md) for the contract.
