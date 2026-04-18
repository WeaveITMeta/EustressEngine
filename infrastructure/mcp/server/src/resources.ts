// SPDX-License-Identifier: MIT
//
// Resource resolvers — one per URI kind. Each resolver reads what it
// needs from the Universe filesystem and returns a `{uri, mimeType,
// text}` block fit for `resources/read`.
//
// Resolvers are intentionally separate from tools. Tools are actions
// the AI invokes; resources are memory the AI pins and refers back to.
// The split means a tool like `eustress_read_script` and a resource at
// `eustress://script/...` can diverge in formatting (tools prefer
// JSON, resources prefer human-readable markdown) without either
// duplicating logic for the other.

import * as fs from 'node:fs/promises';
import * as fsSync from 'node:fs';
import * as path from 'node:path';
import {
  listSpaces, listScripts, readCapped, extractToml,
  MAX_FILE_BYTES,
} from './universe.js';
import { buildUri, parseUri, type EustressUri } from './uri.js';

export type ResourceBlock = {
  uri: string;
  mimeType?: string;
  text?: string;
  blob?: string;  // base64; not used in v1 but part of MCP contract
};

export type ListedResource = {
  uri: string;
  name: string;
  description?: string;
  mimeType?: string;
};

// ═══════════════════════════════════════════════════════════════════════
// `resources/list` — enumerate the common, useful-right-now resources
// so clients that don't know the URI scheme can still browse. Tier-1
// listing: scripts (up to 100) + spaces + recent conversations +
// discovered briefs. Templates handle the long tail.
// ═══════════════════════════════════════════════════════════════════════

const MAX_LIST = 200;

export async function listResources(universe: string): Promise<ListedResource[]> {
  const out: ListedResource[] = [];

  // Spaces as folder-like entries.
  for (const s of await listSpaces(universe)) {
    out.push({
      uri: buildUri({ kind: 'space', space: s.name }),
      name: `Space: ${s.name}`,
      description: `Overview of ${s.name} — services, top-level scripts, counts.`,
      mimeType: 'text/markdown',
    });
    if (out.length >= MAX_LIST) return out;
  }

  // Scripts — bundled source + summary is the highest-value pin the AI
  // can make, so we surface them early.
  const scripts = await listScripts(universe);
  for (const s of scripts) {
    const relPath = path.relative(
      path.join(universe, 'Spaces', s.space),
      s.folder,
    ).split(path.sep).join('/');
    out.push({
      uri: buildUri({ kind: 'script', space: s.space, relPath }),
      name: `Script: ${s.space}/${relPath}`,
      description: `${s.class} — source + summary`,
      mimeType: 'text/markdown',
    });
    if (out.length >= MAX_LIST) return out;
  }

  // Conversations — Workshop archive.
  const sessionDir = path.join(universe, '.eustress', 'knowledge', 'sessions');
  try {
    const files = await fs.readdir(sessionDir);
    const jsons = files.filter(f => f.endsWith('.json')).sort().reverse();
    for (const file of jsons.slice(0, 20)) {
      const id = file.slice(0, -'.json'.length);
      out.push({
        uri: buildUri({ kind: 'conversation', relPath: id }),
        name: `Workshop conversation: ${id}`,
        description: 'Persisted Workshop chat history',
        mimeType: 'application/json',
      });
      if (out.length >= MAX_LIST) return out;
    }
  } catch { /* no sessions yet, fine */ }

  // Briefs — `ideation_brief.toml` files anywhere in any Space.
  const briefs = await findBriefs(universe);
  for (const b of briefs) {
    out.push({
      uri: buildUri({ kind: 'brief', relPath: b.product }),
      name: `Brief: ${b.product}`,
      description: `Product ideation brief at ${b.relPath}`,
      mimeType: 'application/toml',
    });
    if (out.length >= MAX_LIST) return out;
  }

  return out;
}

// ═══════════════════════════════════════════════════════════════════════
// `resources/read` — resolve one URI → content block.
// ═══════════════════════════════════════════════════════════════════════

export async function readResource(
  universe: string,
  raw: string,
): Promise<ResourceBlock> {
  const uri = parseUri(raw);
  if (!uri) throw new Error(`Malformed Eustress URI: ${raw}`);

  switch (uri.kind) {
    case 'space':         return await readSpace(universe, uri);
    case 'script':        return await readScript(universe, uri);
    case 'entity':        return await readEntity(universe, uri);
    case 'file':          return await readFile(universe, uri);
    case 'conversation':  return await readConversation(universe, uri);
    case 'brief':         return await readBrief(universe, uri);
  }
}

// ─── space ─────────────────────────────────────────────────────────────

async function readSpace(universe: string, uri: EustressUri): Promise<ResourceBlock> {
  const space = uri.space!;
  const spaceDir = path.join(universe, 'Spaces', space);
  if (!fsSync.existsSync(spaceDir)) {
    throw new Error(`Space not found: ${space}`);
  }

  const services: string[] = [];
  try {
    const entries = await fs.readdir(spaceDir, { withFileTypes: true });
    for (const e of entries) {
      if (e.isDirectory() && !e.name.startsWith('.')) services.push(e.name);
    }
  } catch { /* empty Space — fine */ }

  const scripts = (await listScripts(universe, space)).filter(s => s.space === space);

  // Markdown overview — what the AI would want to see when pinning a Space.
  const parts: string[] = [];
  parts.push(`# Space: ${space}`);
  parts.push('');
  parts.push(`**Root:** \`${spaceDir}\``);
  parts.push('');
  parts.push(`## Services (${services.length})`);
  for (const s of services.sort()) parts.push(`- ${s}`);
  parts.push('');
  parts.push(`## Scripts (${scripts.length})`);
  for (const s of scripts) {
    const rel = path.relative(spaceDir, s.folder).split(path.sep).join('/');
    parts.push(`- [${s.class}] \`${rel}\``);
  }
  return {
    uri: uri.raw,
    mimeType: 'text/markdown',
    text: parts.join('\n'),
  };
}

// ─── script ────────────────────────────────────────────────────────────

async function readScript(universe: string, uri: EustressUri): Promise<ResourceBlock> {
  const space = uri.space!;
  const folder = path.join(universe, 'Spaces', space, uri.relPath);
  if (!fsSync.existsSync(folder)) {
    throw new Error(`Script folder not found: ${folder}`);
  }

  const name = path.basename(folder);
  const sourcePath = await findFirstExisting([
    path.join(folder, `${name}.rune`),
    path.join(folder, `${name}.luau`),
    path.join(folder, `${name}.soul`),
    path.join(folder, 'Source.rune'),
  ]);
  const summaryPath = await findFirstExisting([
    path.join(folder, `${name}.md`),
    path.join(folder, 'Summary.md'),
  ]);
  const instanceToml = path.join(folder, '_instance.toml');

  const parts: string[] = [];
  parts.push(`# Script: ${space}/${uri.relPath}`);
  parts.push('');
  parts.push(`**Folder:** \`${folder}\``);

  if (fsSync.existsSync(instanceToml)) {
    const toml = await fs.readFile(instanceToml, 'utf8');
    const klass = extractToml(toml, 'class_name') ?? '(unknown)';
    parts.push(`**Class:** ${klass}`);
  }
  parts.push('');

  if (summaryPath) {
    parts.push('## Summary');
    parts.push('');
    const { text, truncated } = await readCapped(summaryPath);
    parts.push(text.trimEnd());
    if (truncated) parts.push(`\n_[truncated at ${MAX_FILE_BYTES} bytes]_`);
    parts.push('');
  }

  if (sourcePath) {
    parts.push('## Source');
    parts.push('');
    const lang = sourcePath.endsWith('.luau') ? 'luau'
               : sourcePath.endsWith('.rune') ? 'rust'  // no hljs rune highlighter; rust is closest
               : '';
    parts.push('```' + lang);
    const { text, truncated } = await readCapped(sourcePath);
    parts.push(text.trimEnd());
    parts.push('```');
    if (truncated) parts.push(`\n_[source truncated at ${MAX_FILE_BYTES} bytes]_`);
  } else {
    parts.push('_(no source file found)_');
  }

  return {
    uri: uri.raw,
    mimeType: 'text/markdown',
    text: parts.join('\n'),
  };
}

// ─── entity ────────────────────────────────────────────────────────────

async function readEntity(universe: string, uri: EustressUri): Promise<ResourceBlock> {
  const space = uri.space!;
  const target = path.join(universe, 'Spaces', space, uri.relPath);

  // Entities can be folder-based (read `_instance.toml` inside) OR
  // flat-file (read the `.part.toml` / `.model.toml` / etc. directly).
  let tomlPath: string;
  if (fsSync.existsSync(target) && fsSync.statSync(target).isDirectory()) {
    tomlPath = path.join(target, '_instance.toml');
  } else {
    tomlPath = target;
  }

  if (!fsSync.existsSync(tomlPath)) {
    throw new Error(`Entity not found: ${target}`);
  }

  const { text, truncated } = await readCapped(tomlPath);
  const klass = extractToml(text, 'class_name') ?? '(unknown)';
  const name  = extractToml(text, 'name') ?? path.basename(path.dirname(tomlPath));

  const parts: string[] = [];
  parts.push(`# Entity: ${name}`);
  parts.push('');
  parts.push(`**Class:** ${klass}`);
  parts.push(`**Path:** \`${tomlPath}\``);
  parts.push('');
  parts.push('## _instance.toml');
  parts.push('');
  parts.push('```toml');
  parts.push(text.trimEnd());
  parts.push('```');
  if (truncated) parts.push(`\n_[truncated at ${MAX_FILE_BYTES} bytes]_`);
  return {
    uri: uri.raw,
    mimeType: 'text/markdown',
    text: parts.join('\n'),
  };
}

// ─── file ──────────────────────────────────────────────────────────────

async function readFile(universe: string, uri: EustressUri): Promise<ResourceBlock> {
  const space = uri.space!;
  const target = path.join(universe, 'Spaces', space, uri.relPath);
  if (!fsSync.existsSync(target)) {
    throw new Error(`File not found: ${target}`);
  }

  // Minimal binary-file rejection — MCP clients expect text resources.
  // A production system would sniff more thoroughly (e.g. libmagic).
  const ext = path.extname(target).toLowerCase();
  const binaryExt = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.mp3',
    '.wav', '.ogg', '.glb', '.gltf', '.fbx', '.obj', '.stl', '.ktx2']);
  if (binaryExt.has(ext)) {
    throw new Error(`Binary file not exposed as text resource: ${target}`);
  }

  const { text, truncated } = await readCapped(target);
  const mimeType = ext === '.md' ? 'text/markdown'
                 : ext === '.toml' ? 'application/toml'
                 : ext === '.json' ? 'application/json'
                 : 'text/plain';

  return {
    uri: uri.raw,
    mimeType,
    text: truncated ? text + `\n\n[truncated at ${MAX_FILE_BYTES} bytes]` : text,
  };
}

// ─── conversation ──────────────────────────────────────────────────────

async function readConversation(universe: string, uri: EustressUri): Promise<ResourceBlock> {
  const id = uri.relPath;
  const sessionFile = path.join(
    universe, '.eustress', 'knowledge', 'sessions', `${id}.json`,
  );
  if (!fsSync.existsSync(sessionFile)) {
    throw new Error(`No such conversation: ${id}`);
  }
  const text = await fs.readFile(sessionFile, 'utf8');
  return { uri: uri.raw, mimeType: 'application/json', text };
}

// ─── brief ─────────────────────────────────────────────────────────────

async function readBrief(universe: string, uri: EustressUri): Promise<ResourceBlock> {
  const product = uri.relPath;
  const briefs = await findBriefs(universe);
  const match = briefs.find(b => b.product === product);
  if (!match) throw new Error(`No ideation brief for product: ${product}`);
  const { text, truncated } = await readCapped(match.path);
  return {
    uri: uri.raw,
    mimeType: 'application/toml',
    text: truncated ? text + `\n\n# [truncated at ${MAX_FILE_BYTES} bytes]` : text,
  };
}

// ═══════════════════════════════════════════════════════════════════════
// Reverse mapping — filesystem path → URI. Used by the watcher so a
// change to `<universe>/Spaces/Space1/SoulService/foo/foo.rune` emits
// `resources/updated` for `eustress://script/Space1/SoulService/foo`.
// ═══════════════════════════════════════════════════════════════════════

export function pathToUri(universe: string, absPath: string): string | null {
  const rel = path.relative(universe, absPath).split(path.sep).join('/');

  // .eustress/knowledge/sessions/<id>.json → conversation
  const sessionMatch = rel.match(/^\.eustress\/knowledge\/sessions\/(.+)\.json$/);
  if (sessionMatch) {
    return buildUri({ kind: 'conversation', relPath: sessionMatch[1] });
  }

  // Spaces/<space>/.../ideation_brief.toml → brief
  const briefMatch = rel.match(/^Spaces\/([^\/]+)\/(.+)\/ideation_brief\.toml$/);
  if (briefMatch) {
    // Product name = parent folder of ideation_brief.toml.
    const product = path.basename(briefMatch[2]);
    return buildUri({ kind: 'brief', relPath: product });
  }

  // Spaces/<space>/<relpath>
  const spaceMatch = rel.match(/^Spaces\/([^\/]+)\/(.+)$/);
  if (!spaceMatch) return null;
  const space = spaceMatch[1];
  const innerRel = spaceMatch[2];

  // script: innerRel ends at a folder that contains `<folder>.<ext>` or
  // `Source.<ext>`. Rather than re-deriving, check if the PARENT folder
  // of the changed file is a script folder.
  const fileName = path.basename(innerRel);
  const parentRel = path.dirname(innerRel);
  if (parentRel && parentRel !== '.') {
    const parentAbs = path.join(universe, 'Spaces', space, parentRel);
    const parentName = path.basename(parentRel);
    const isScriptFile = fileName === `${parentName}.rune`
      || fileName === `${parentName}.md`
      || fileName === 'Source.rune'
      || fileName === 'Summary.md'
      || fileName === '_instance.toml';
    if (isScriptFile && fsSync.existsSync(path.join(parentAbs, '_instance.toml'))) {
      return buildUri({ kind: 'script', space, relPath: parentRel.split(path.sep).join('/') });
    }
  }

  // entity: the file itself is `_instance.toml` inside a folder.
  if (fileName === '_instance.toml' && parentRel && parentRel !== '.') {
    return buildUri({ kind: 'entity', space, relPath: parentRel.split(path.sep).join('/') });
  }

  // Everything else that's a text-ish file is a generic `file` resource.
  return buildUri({ kind: 'file', space, relPath: innerRel.split(path.sep).join('/') });
}

// ═══════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════

async function findFirstExisting(candidates: string[]): Promise<string | null> {
  for (const c of candidates) {
    if (fsSync.existsSync(c)) return c;
  }
  return null;
}

type BriefEntry = { product: string; path: string; relPath: string };

async function findBriefs(universe: string): Promise<BriefEntry[]> {
  const out: BriefEntry[] = [];
  const spacesDir = path.join(universe, 'Spaces');
  if (!fsSync.existsSync(spacesDir)) return out;

  // Shallow-ish walk. `ideation_brief.toml` files live under
  // `Spaces/*/Workspace/<product>/` or similar; up to 4 levels deep
  // covers every layout Workshop produces.
  await walkForBriefs(spacesDir, universe, 4, out);
  return out;
}

async function walkForBriefs(
  dir: string, universe: string, depth: number, out: BriefEntry[],
): Promise<void> {
  if (depth < 0 || out.length >= 100) return;
  let entries: import('node:fs').Dirent[] = [];
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch { return; }

  for (const e of entries) {
    if (e.isFile() && e.name === 'ideation_brief.toml') {
      out.push({
        product: path.basename(dir),
        path: path.join(dir, e.name),
        relPath: path.relative(universe, path.join(dir, e.name)),
      });
      continue;
    }
    if (e.isDirectory() && !e.name.startsWith('.')) {
      await walkForBriefs(path.join(dir, e.name), universe, depth - 1, out);
    }
  }
}
