// SPDX-License-Identifier: MIT
//
// Tool registry for the Eustress MCP server. Each tool is a pair of
// (JSON-schema description, async handler). The schema is what the MCP
// client sees in `tools/list`; the handler runs on `tools/call`.
//
// All tools take `universe?: string` as their first input so clients can
// target a specific Universe even without the server launch config.
// When absent, the server's default Universe (from env/arg) is used.

import * as path from 'node:path';
import * as fs from 'node:fs/promises';
import * as fsSync from 'node:fs';
import { execFile as _execFile } from 'node:child_process';
import { promisify } from 'node:util';
import {
  listSpaces, listScripts, findEntity, searchUniverse,
  readCapped, resolveInUniverse, extractToml,
  findUniverseRoot, discoverUniverses,
  MAX_FILE_BYTES,
} from './universe.js';

const execFile = promisify(_execFile);

// ─── MCP response shape ───────────────────────────────────────────────
export type ToolContent =
  | { type: 'text'; text: string };

export type ToolResult = {
  content: ToolContent[];
  isError?: boolean;
};

// Helper to emit JSON as a text block; MCP clients parse it on their side.
function asJson(value: unknown): ToolResult {
  return { content: [{ type: 'text', text: JSON.stringify(value, null, 2) }] };
}
function asText(text: string): ToolResult {
  return { content: [{ type: 'text', text }] };
}
function asError(msg: string): ToolResult {
  return { content: [{ type: 'text', text: `Error: ${msg}` }], isError: true };
}

// ─── Server state (mutable; held by index.ts) ────────────────────────
//
// The Universe is dynamic: it can be set at launch, changed mid-session
// by `eustress_set_default_universe`, or inferred per-call from a
// `path` argument. Tools read (and sometimes mutate) this shared state.
export type ServerState = {
  currentUniverse: string | null;
  searchRoots: string[];
};

// ─── Tool descriptor ──────────────────────────────────────────────────
export type ToolDescriptor = {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
  handler: (args: Record<string, unknown>, state: ServerState) => Promise<ToolResult>;
};

// ─── Universe resolution (the dynamic part) ───────────────────────────
//
// Precedence, first match wins:
//   1. explicit `universe` in args (client says where)
//   2. walk up from `path` in args (client points at a script/entity,
//      we find the enclosing Universe automatically)
//   3. `state.currentUniverse` (launch default or last set via tool)
//   4. walk up from process.cwd() (last-ditch auto-detect)
//
// Returns null if nothing resolves — callers error with a clear message
// rather than silently operating on the wrong folder.
function resolveUniverse(
  args: Record<string, unknown>,
  state: ServerState,
): string | null {
  if (typeof args.universe === 'string' && args.universe.trim()) {
    return path.resolve(args.universe);
  }
  if (typeof args.path === 'string' && args.path) {
    const startAbs = path.isAbsolute(args.path)
      ? args.path
      : path.resolve(state.currentUniverse ?? process.cwd(), args.path);
    const found = findUniverseRoot(startAbs);
    if (found) return found;
  }
  if (state.currentUniverse) return state.currentUniverse;
  return findUniverseRoot(process.cwd());
}

// Wrap resolveUniverse with an error-producing variant for tool handlers.
function requireUniverse(
  args: Record<string, unknown>,
  state: ServerState,
): string | ToolResult {
  const u = resolveUniverse(args, state);
  if (!u) {
    return asError(
      'No Universe configured. Pass `universe` explicitly, or call ' +
      '`eustress_list_universes` + `eustress_set_default_universe` first.',
    );
  }
  return u;
}

// Standard arg — every tool shares it.
const universeArg = {
  universe: {
    type: 'string' as const,
    description: 'Absolute path to the Universe root. Optional — auto-resolves from `path` arg, or falls back to the server\'s current default.',
  },
};

// ─── eustress_list_universes ─────────────────────────────────────────
const list_universes: ToolDescriptor = {
  name: 'eustress_list_universes',
  description: 'Discover Universes on disk. Scans the server\'s search roots (EUSTRESS_UNIVERSES_PATH, or sensible defaults like ~/Eustress) and any enclosing Universe of the current working directory.',
  inputSchema: {
    type: 'object',
    properties: {
      extra_roots: {
        type: 'array',
        items: { type: 'string' },
        description: 'Additional directories to scan (in addition to the server\'s configured search roots).',
      },
    },
  },
  handler: async (args, state) => {
    const extra = Array.isArray(args.extra_roots)
      ? args.extra_roots.filter((x): x is string => typeof x === 'string')
      : [];
    const roots = [...state.searchRoots, ...extra];
    const discovered = await discoverUniverses(roots);
    // Also include the Universe enclosing cwd if we find one — common
    // case: user runs the server from inside a project they just cloned.
    const cwdUniverse = findUniverseRoot(process.cwd());
    if (cwdUniverse && !discovered.includes(cwdUniverse)) {
      discovered.push(cwdUniverse);
    }
    discovered.sort();
    return asJson({
      current: state.currentUniverse,
      search_roots: roots,
      universes: discovered,
    });
  },
};

// ─── eustress_set_default_universe ───────────────────────────────────
const set_default_universe: ToolDescriptor = {
  name: 'eustress_set_default_universe',
  description: 'Change the server\'s default Universe mid-session. Subsequent tool calls that don\'t pass `universe` use this one.',
  inputSchema: {
    type: 'object',
    required: ['universe'],
    properties: {
      universe: {
        type: 'string',
        description: 'Absolute path to a Universe root (a folder containing `Spaces/`).',
      },
    },
  },
  handler: async (args, state) => {
    const requested = typeof args.universe === 'string' ? args.universe : '';
    if (!requested) return asError('`universe` is required');
    const abs = path.resolve(requested);
    if (!fsSync.existsSync(path.join(abs, 'Spaces'))) {
      return asError(`'${abs}' is not a Universe (no Spaces/ directory found).`);
    }
    const previous = state.currentUniverse;
    state.currentUniverse = abs;
    return asJson({
      previous,
      current: state.currentUniverse,
      changed: previous !== abs,
    });
  },
};

// ─── eustress_list_spaces ────────────────────────────────────────────
const list_spaces: ToolDescriptor = {
  name: 'eustress_list_spaces',
  description: 'List every Space in the Universe (subdirectories under {universe}/Spaces/).',
  inputSchema: {
    type: 'object',
    properties: { ...universeArg },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const spaces = await listSpaces(u);
    return asJson({ universe: u, spaces });
  },
};

// ─── eustress_list_scripts ───────────────────────────────────────────
const list_scripts: ToolDescriptor = {
  name: 'eustress_list_scripts',
  description: 'List every folder-based Rune/Luau script in a Space (or all Spaces). Returns class, source path, summary path.',
  inputSchema: {
    type: 'object',
    properties: {
      ...universeArg,
      space: {
        type: 'string',
        description: 'Optional Space name to scope the listing. Omit to walk every Space.',
      },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const space = typeof args.space === 'string' ? args.space : undefined;
    const scripts = await listScripts(u, space);
    return asJson({ universe: u, count: scripts.length, scripts });
  },
};

// ─── eustress_read_script ────────────────────────────────────────────
const read_script: ToolDescriptor = {
  name: 'eustress_read_script',
  description: 'Read a script\'s source + summary together. `path` can point at the script folder or the `.rune` file directly.',
  inputSchema: {
    type: 'object',
    required: ['path'],
    properties: {
      ...universeArg,
      path: {
        type: 'string',
        description: 'Absolute path to the script folder or its source file.',
      },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const p = typeof args.path === 'string' ? args.path : '';
    if (!p) return asError('`path` is required');
    let resolved: string;
    try { resolved = resolveInUniverse(u, p); }
    catch (e) { return asError(String(e)); }

    const stat = await fs.stat(resolved).catch(() => null);
    if (!stat) return asError(`no such path: ${resolved}`);

    let folder: string;
    if (stat.isDirectory()) {
      folder = resolved;
    } else {
      folder = path.dirname(resolved);
    }

    // Find script within the folder via the same preference order as
    // Studio (canonical `<folder>.rune` → legacy `Source.rune`).
    const name = path.basename(folder);
    const candidates = [
      path.join(folder, `${name}.rune`),
      path.join(folder, 'Source.rune'),
    ];
    let sourcePath: string | null = null;
    for (const c of candidates) {
      if (await fs.stat(c).then(() => true, () => false)) {
        sourcePath = c; break;
      }
    }
    if (!sourcePath) {
      // Fallback: any .rune in the folder.
      const entries = await fs.readdir(folder).catch(() => []);
      const match = entries.find(n => n.endsWith('.rune') || n.endsWith('.luau'));
      if (match) sourcePath = path.join(folder, match);
    }

    const summaryPath = [
      path.join(folder, `${name}.md`),
      path.join(folder, 'Summary.md'),
    ].find(s => fs.stat(s).then(() => true, () => false)) ?? null;

    const source = sourcePath
      ? await readCapped(sourcePath).then(r => r)
      : null;
    const summary = summaryPath
      ? await readCapped(summaryPath).then(r => r)
      : null;

    return asJson({
      folder,
      name,
      source_path: sourcePath,
      summary_path: summaryPath,
      source: source ? { text: source.text, truncated: source.truncated } : null,
      summary: summary ? { text: summary.text, truncated: summary.truncated } : null,
      byte_cap: MAX_FILE_BYTES,
    });
  },
};

// ─── eustress_find_entity ────────────────────────────────────────────
const find_entity: ToolDescriptor = {
  name: 'eustress_find_entity',
  description: 'Find entities by name (case-insensitive substring) across a Space or all Spaces.',
  inputSchema: {
    type: 'object',
    required: ['query'],
    properties: {
      ...universeArg,
      query:  { type: 'string', description: 'Name substring to match.' },
      space:  { type: 'string', description: 'Optional Space name to scope the search.' },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const query = typeof args.query === 'string' ? args.query : '';
    if (!query) return asError('`query` is required');
    const space = typeof args.space === 'string' ? args.space : undefined;
    const matches = await findEntity(u, query, space);
    return asJson({ universe: u, query, count: matches.length, matches });
  },
};

// ─── eustress_list_assets ────────────────────────────────────────────
const list_assets: ToolDescriptor = {
  name: 'eustress_list_assets',
  description: 'List assets (meshes, textures, GUIs) under a Space. Filters by extension family.',
  inputSchema: {
    type: 'object',
    properties: {
      ...universeArg,
      space: { type: 'string', description: 'Optional Space name (omit for all).' },
      kind:  {
        type: 'string',
        enum: ['meshes', 'textures', 'gui', 'audio', 'all'],
        description: 'Asset family filter. Default: "all".',
      },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const universe = u;
    const space = typeof args.space === 'string' ? args.space : undefined;
    const kind = typeof args.kind === 'string' ? args.kind : 'all';

    const kindExts: Record<string, string[]> = {
      meshes:   ['.glb', '.gltf', '.fbx', '.obj', '.stl'],
      textures: ['.png', '.jpg', '.jpeg', '.webp', '.tga', '.ktx2'],
      gui:      ['.slint'],
      audio:    ['.wav', '.mp3', '.ogg', '.flac'],
    };
    const exts = kind === 'all'
      ? ([] as string[]).concat(...Object.values(kindExts))
      : kindExts[kind] ?? [];

    const spaces = space
      ? (await listSpaces(universe)).filter(s => s.name === space)
      : await listSpaces(universe);

    const out: Array<{ path: string; space: string; kind: string; size: number }> = [];
    for (const s of spaces) {
      await walk(s.path, async (p) => {
        const ext = path.extname(p).toLowerCase();
        if (!exts.includes(ext)) return;
        const k = (Object.entries(kindExts).find(([_, es]) => es.includes(ext))?.[0]) ?? 'other';
        const st = await fs.stat(p).catch(() => null);
        if (!st) return;
        out.push({ path: p, space: s.name, kind: k, size: st.size });
      });
      if (out.length >= 500) break;
    }
    return asJson({ universe, kind, count: out.length, assets: out.slice(0, 500) });
  },
};

// ─── eustress_search_universe ────────────────────────────────────────
const search_universe: ToolDescriptor = {
  name: 'eustress_search_universe',
  description: 'Case-insensitive text search across `.rune`, `.toml`, and `.md` files under the Universe.',
  inputSchema: {
    type: 'object',
    required: ['query'],
    properties: {
      ...universeArg,
      query: { type: 'string', description: 'Text to search for.' },
      space: { type: 'string', description: 'Optional Space to scope the search.' },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const query = typeof args.query === 'string' ? args.query : '';
    if (!query) return asError('`query` is required');
    const space = typeof args.space === 'string' ? args.space : undefined;
    const matches = await searchUniverse(u, query, space);
    return asJson({ universe: u, query, count: matches.length, matches });
  },
};

// ─── Git tools (thin wrappers around git CLI) ────────────────────────
async function gitCmd(universe: string, args: string[]): Promise<string> {
  const { stdout } = await execFile('git', args, {
    cwd: universe,
    maxBuffer: 4 * 1024 * 1024,
    windowsHide: true,
  });
  return stdout;
}

const git_status: ToolDescriptor = {
  name: 'eustress_git_status',
  description: 'Run `git status --porcelain` in the Universe root. Returns parsed entries.',
  inputSchema: { type: 'object', properties: { ...universeArg } },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    try {
      const out = await gitCmd(u, ['status', '--porcelain=v1']);
      const entries = out.split('\n').filter(Boolean).map(line => ({
        status: line.slice(0, 2).trim(),
        path:   line.slice(3),
      }));
      return asJson({ universe: u, count: entries.length, entries });
    } catch (e) {
      return asError(`git status failed: ${String(e)}`);
    }
  },
};

const git_log: ToolDescriptor = {
  name: 'eustress_git_log',
  description: 'Recent commits in the Universe repo. Default: last 20.',
  inputSchema: {
    type: 'object',
    properties: {
      ...universeArg,
      limit: { type: 'integer', description: 'Max commits to return. 1-200.' },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const limitRaw = typeof args.limit === 'number' ? args.limit : 20;
    const limit = Math.max(1, Math.min(200, Math.floor(limitRaw)));
    try {
      const out = await gitCmd(u, [
        'log', `-n${limit}`,
        '--pretty=format:%H%x09%an%x09%ar%x09%s',
      ]);
      const commits = out.split('\n').filter(Boolean).map(line => {
        const [hash, author, date, ...rest] = line.split('\t');
        return { hash, author, date, subject: rest.join('\t') };
      });
      return asJson({ universe: u, count: commits.length, commits });
    } catch (e) {
      return asError(`git log failed: ${String(e)}`);
    }
  },
};

const git_diff: ToolDescriptor = {
  name: 'eustress_git_diff',
  description: 'Show uncommitted diff for a path (or the whole Universe if omitted).',
  inputSchema: {
    type: 'object',
    properties: {
      ...universeArg,
      path:   { type: 'string', description: 'Optional path scope (relative to Universe or absolute).' },
      staged: { type: 'boolean', description: 'If true, show staged diff (--cached). Default false.' },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const relPath = typeof args.path === 'string' && args.path
      ? path.relative(u, resolveInUniverse(u, args.path))
      : null;
    const staged = args.staged === true;
    try {
      const diffArgs = ['diff'];
      if (staged) diffArgs.push('--cached');
      if (relPath) diffArgs.push('--', relPath);
      const out = await gitCmd(u, diffArgs);
      return asText(out.length ? out : '(no changes)');
    } catch (e) {
      return asError(`git diff failed: ${String(e)}`);
    }
  },
};

// ─── eustress_create_script ──────────────────────────────────────────
const create_script: ToolDescriptor = {
  name: 'eustress_create_script',
  description: 'Create a new Rune script folder using Studio\'s canonical layout: folder/folder.rune + folder.md + _instance.toml.',
  inputSchema: {
    type: 'object',
    required: ['space', 'service', 'name'],
    properties: {
      ...universeArg,
      space:     { type: 'string', description: 'Target Space name.' },
      service:   { type: 'string', description: 'Service folder (e.g. "SoulService", "StarterPlayerScripts").' },
      name:      { type: 'string', description: 'Script folder name. Used for the source + summary file names too.' },
      body:      { type: 'string', description: 'Initial Rune source. Optional; a minimal stub is written if omitted.' },
      summary:   { type: 'string', description: 'Initial summary markdown. Optional.' },
      class:     {
        type: 'string',
        enum: ['Script', 'SoulScript', 'LocalScript', 'ModuleScript'],
        description: 'Script class_name. Default: "Script".',
      },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const universe = u;
    const space   = String(args.space   ?? '');
    const service = String(args.service ?? '');
    const name    = String(args.name    ?? '');
    const klass   = String(args.class ?? 'Script');
    if (!space || !service || !name) {
      return asError('space, service, and name are all required');
    }
    if (!/^[A-Za-z_][A-Za-z0-9_-]*$/.test(name)) {
      return asError(`invalid script name: ${name}`);
    }

    const folder = path.join(universe, 'Spaces', space, service, name);
    try {
      await fs.mkdir(folder, { recursive: false });
    } catch {
      return asError(`folder already exists or cannot be created: ${folder}`);
    }

    const body = typeof args.body === 'string' ? args.body
      : 'pub fn main() {\n    println("Hello from Rune!");\n}\n';
    const summary = typeof args.summary === 'string' ? args.summary
      : `# ${name}\n\nNew script.\n`;

    const instanceToml =
      `[metadata]\n` +
      `class_name = "${klass}"\n` +
      `archivable = true\n\n` +
      `[script]\n` +
      `source = "${name}.rune"\n`;

    await fs.writeFile(path.join(folder, '_instance.toml'), instanceToml);
    await fs.writeFile(path.join(folder, `${name}.rune`), body);
    await fs.writeFile(path.join(folder, `${name}.md`),   summary);

    return asJson({
      created: true,
      folder,
      files: [
        path.join(folder, '_instance.toml'),
        path.join(folder, `${name}.rune`),
        path.join(folder, `${name}.md`),
      ],
    });
  },
};

// ─── eustress_get_conversation ───────────────────────────────────────
const get_conversation: ToolDescriptor = {
  name: 'eustress_get_conversation',
  description: 'Load a Workshop conversation persisted under `{universe}/.eustress/knowledge/`.',
  inputSchema: {
    type: 'object',
    required: ['session_id'],
    properties: {
      ...universeArg,
      session_id: { type: 'string', description: 'Workshop session id.' },
    },
  },
  handler: async (args, state) => {
    const u = requireUniverse(args, state);
    if (typeof u !== 'string') return u;
    const sid = typeof args.session_id === 'string' ? args.session_id : '';
    if (!sid) return asError('`session_id` is required');
    const sessionFile = path.join(u, '.eustress', 'knowledge', 'sessions', `${sid}.json`);
    try {
      const contents = await fs.readFile(sessionFile, 'utf8');
      return asJson(JSON.parse(contents));
    } catch (e) {
      return asError(`no such session: ${sid}`);
    }
  },
};

// ─── Generic fs walker ───────────────────────────────────────────────
async function walk(dir: string, cb: (path: string) => Promise<void>): Promise<void> {
  let entries: import('node:fs').Dirent[] = [];
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch { return; }
  for (const e of entries) {
    const abs = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (e.name.startsWith('.')) continue;
      await walk(abs, cb);
    } else if (e.isFile()) {
      await cb(abs);
    }
  }
}

// Silence unused-import warning — `extractToml` is re-exported for tools
// that may need it (kept out of current registry but in the public API).
void extractToml;

// ─── Export registry ─────────────────────────────────────────────────
//
// Ordering matters for the `tools/list` response the client shows in
// its tool drawer: discovery + configuration tools first, then the
// content tools, then git, then write tools.
export const TOOLS: ToolDescriptor[] = [
  list_universes,
  set_default_universe,
  list_spaces,
  list_scripts,
  read_script,
  find_entity,
  list_assets,
  search_universe,
  git_status,
  git_log,
  git_diff,
  create_script,
  get_conversation,
];
