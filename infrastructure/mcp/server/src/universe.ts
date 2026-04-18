// SPDX-License-Identifier: MIT
//
// Universe helpers — tiny fs-layer that maps Eustress's file-system-first
// conventions onto ergonomic tool inputs. Every function takes a
// pre-validated absolute path; the `resolveInUniverse` gatekeeper in
// index.ts is responsible for the path-safety check.

import * as fs from 'node:fs/promises';
import * as fsSync from 'node:fs';
import * as path from 'node:path';

// Cap result sizes so we never hand back megabytes to the client. MCP
// tools that blow past their natural bounds get truncated by the client
// silently — bounding up front gives us a clean "truncated" signal we
// can include in the response.
export const MAX_LIST_ITEMS = 500;
export const MAX_SEARCH_MATCHES = 200;
export const MAX_FILE_BYTES = 256 * 1024;

export type SpaceInfo = { name: string; path: string };
export type ScriptInfo = {
  name: string;
  folder: string;
  space: string;
  class: string;         // "Script" | "SoulScript" | "LocalScript" | "ModuleScript"
  sourcePath: string;    // absolute
  summaryPath: string | null;
};
export type EntityMatch = {
  name: string;
  class: string;
  space: string;
  path: string;          // absolute to _instance.toml
};

/**
 * Enumerate every Space under `{universe}/Spaces/`. Returns an empty list
 * if the directory doesn't exist (valid for a freshly-created universe).
 */
export async function listSpaces(universe: string): Promise<SpaceInfo[]> {
  const spacesDir = path.join(universe, 'Spaces');
  let entries: string[] = [];
  try {
    entries = await fs.readdir(spacesDir);
  } catch { return []; }
  const out: SpaceInfo[] = [];
  for (const name of entries) {
    const abs = path.join(spacesDir, name);
    try {
      const stat = await fs.stat(abs);
      if (stat.isDirectory()) {
        out.push({ name, path: abs });
      }
    } catch { /* broken symlink or race — skip */ }
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Walk one Space (or all Spaces) and return every folder-based Rune
 * script. Recognises a script by the presence of an `_instance.toml`
 * whose `class_name` sits in the script family AND a source file in the
 * same folder (canonical `<folder>.rune` or legacy `Source.rune`).
 */
export async function listScripts(
  universe: string,
  spaceName?: string,
): Promise<ScriptInfo[]> {
  const spaces = spaceName
    ? (await listSpaces(universe)).filter(s => s.name === spaceName)
    : await listSpaces(universe);

  const out: ScriptInfo[] = [];
  for (const space of spaces) {
    await walkForScripts(space.path, space.name, out);
    if (out.length >= MAX_LIST_ITEMS) break;
  }
  return out.slice(0, MAX_LIST_ITEMS);
}

async function walkForScripts(
  dir: string,
  spaceName: string,
  out: ScriptInfo[],
): Promise<void> {
  if (out.length >= MAX_LIST_ITEMS) return;
  let entries: fsSync.Dirent[] = [];
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch { return; }

  // Is this directory itself a script folder?
  const instanceToml = entries.find(e => e.isFile() && e.name === '_instance.toml');
  if (instanceToml) {
    const info = await maybeReadAsScript(dir, spaceName);
    if (info) out.push(info);
  }

  // Recurse into subdirectories. We only skip `.eustress/` and hidden
  // folders — everything else can host scripts.
  for (const e of entries) {
    if (!e.isDirectory()) continue;
    if (e.name === '.eustress' || e.name.startsWith('.')) continue;
    if (out.length >= MAX_LIST_ITEMS) return;
    await walkForScripts(path.join(dir, e.name), spaceName, out);
  }
}

async function maybeReadAsScript(
  folder: string,
  space: string,
): Promise<ScriptInfo | null> {
  const tomlPath = path.join(folder, '_instance.toml');
  let toml: string;
  try {
    toml = await fs.readFile(tomlPath, 'utf8');
  } catch { return null; }

  const klass = extractToml(toml, 'class_name');
  const scriptClasses = ['Script', 'SoulScript', 'LocalScript', 'ModuleScript'];
  if (!klass || !scriptClasses.includes(klass)) return null;

  const name = path.basename(folder);
  const sourcePath = await findScriptSource(folder, name);
  if (!sourcePath) return null;

  const summaryPath = await findScriptSummary(folder, name);

  return {
    name, folder, space,
    class: klass,
    sourcePath,
    summaryPath,
  };
}

/** Canonical `<folder>/<folder>.rune` preferred, legacy fallbacks otherwise. */
async function findScriptSource(folder: string, name: string): Promise<string | null> {
  const canonical = path.join(folder, `${name}.rune`);
  if (await exists(canonical)) return canonical;

  let entries: fsSync.Dirent[] = [];
  try {
    entries = await fs.readdir(folder, { withFileTypes: true });
  } catch { return null; }
  for (const e of entries) {
    if (!e.isFile()) continue;
    if (e.name.endsWith('.rune') || e.name.endsWith('.luau')
        || e.name.endsWith('.soul') || e.name.endsWith('.lua')) {
      return path.join(folder, e.name);
    }
  }
  return null;
}

async function findScriptSummary(folder: string, name: string): Promise<string | null> {
  const canonical = path.join(folder, `${name}.md`);
  if (await exists(canonical)) return canonical;
  const legacy = path.join(folder, 'Summary.md');
  if (await exists(legacy)) return legacy;
  return null;
}

async function exists(p: string): Promise<boolean> {
  try { await fs.access(p); return true; } catch { return false; }
}

/**
 * Find entities by name across a Space (or all Spaces). Matches the `name`
 * field in any `_instance.toml`. Case-insensitive substring.
 */
export async function findEntity(
  universe: string,
  query: string,
  spaceName?: string,
): Promise<EntityMatch[]> {
  const needle = query.toLowerCase();
  const spaces = spaceName
    ? (await listSpaces(universe)).filter(s => s.name === spaceName)
    : await listSpaces(universe);

  const out: EntityMatch[] = [];
  for (const space of spaces) {
    await walkForEntities(space.path, space.name, needle, out);
    if (out.length >= MAX_LIST_ITEMS) break;
  }
  return out.slice(0, MAX_LIST_ITEMS);
}

async function walkForEntities(
  dir: string,
  spaceName: string,
  needle: string,
  out: EntityMatch[],
): Promise<void> {
  if (out.length >= MAX_LIST_ITEMS) return;
  let entries: fsSync.Dirent[] = [];
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch { return; }

  for (const e of entries) {
    if (e.isDirectory()) {
      if (e.name === '.eustress' || e.name.startsWith('.')) continue;
      await walkForEntities(path.join(dir, e.name), spaceName, needle, out);
      continue;
    }
    if (!e.isFile()) continue;
    // We scan `_instance.toml` for folder-based entities, and any
    // `*.part.toml`, `*.glb.toml`, `*.model.toml` etc. as flat-file entities.
    const isInstance = e.name === '_instance.toml';
    const isFlatEntity = e.name.endsWith('.part.toml')
      || e.name.endsWith('.glb.toml')
      || e.name.endsWith('.model.toml')
      || e.name.endsWith('.textlabel.toml');
    if (!isInstance && !isFlatEntity) continue;

    const full = path.join(dir, e.name);
    let toml: string;
    try { toml = await fs.readFile(full, 'utf8'); } catch { continue; }

    const nameField = extractToml(toml, 'name');
    const klass = extractToml(toml, 'class_name') ?? 'Instance';
    // For folder-based entities the identity is the folder name; `name`
    // in the TOML is sometimes absent.
    const identity = nameField ?? (isInstance ? path.basename(dir) : e.name.split('.')[0]);
    if (!identity.toLowerCase().includes(needle)) continue;

    out.push({
      name: identity,
      class: klass,
      space: spaceName,
      path: full,
    });
    if (out.length >= MAX_LIST_ITEMS) return;
  }
}

export type SearchMatch = {
  path: string;
  line: number;          // 1-based
  preview: string;       // the matching line, trimmed
};

/**
 * Plain-text search across `.rune` and `.toml` files under the Universe.
 * Case-insensitive. Caps at MAX_SEARCH_MATCHES matches.
 */
export async function searchUniverse(
  universe: string,
  query: string,
  spaceName?: string,
): Promise<SearchMatch[]> {
  const roots = spaceName
    ? (await listSpaces(universe)).filter(s => s.name === spaceName).map(s => s.path)
    : [path.join(universe, 'Spaces')];

  const needle = query.toLowerCase();
  const out: SearchMatch[] = [];
  for (const root of roots) {
    await walkForSearch(root, needle, out);
    if (out.length >= MAX_SEARCH_MATCHES) break;
  }
  return out.slice(0, MAX_SEARCH_MATCHES);
}

async function walkForSearch(
  dir: string,
  needle: string,
  out: SearchMatch[],
): Promise<void> {
  if (out.length >= MAX_SEARCH_MATCHES) return;
  let entries: fsSync.Dirent[] = [];
  try {
    entries = await fs.readdir(dir, { withFileTypes: true });
  } catch { return; }

  for (const e of entries) {
    const abs = path.join(dir, e.name);
    if (e.isDirectory()) {
      if (e.name === '.eustress' || e.name.startsWith('.')) continue;
      await walkForSearch(abs, needle, out);
      if (out.length >= MAX_SEARCH_MATCHES) return;
      continue;
    }
    if (!e.isFile()) continue;
    if (!e.name.endsWith('.rune') && !e.name.endsWith('.toml')
        && !e.name.endsWith('.md')) continue;

    let contents: string;
    try { contents = await fs.readFile(abs, 'utf8'); } catch { continue; }
    const lines = contents.split('\n');
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].toLowerCase().includes(needle)) {
        out.push({ path: abs, line: i + 1, preview: lines[i].trim().slice(0, 200) });
        if (out.length >= MAX_SEARCH_MATCHES) return;
      }
    }
  }
}

/**
 * Extract a scalar string field from a TOML file. Not a real parser —
 * handles the single-value lines Eustress writes (`name = "foo"`,
 * `class_name = "Script"`). Returns `null` if not found.
 */
export function extractToml(toml: string, field: string): string | null {
  for (const line of toml.split('\n')) {
    const trimmed = line.trim();
    // match "field = "value"" — quotes required for string values
    const prefix = `${field} = "`;
    if (trimmed.startsWith(prefix)) {
      const rest = trimmed.slice(prefix.length);
      const end = rest.indexOf('"');
      if (end >= 0) return rest.slice(0, end);
    }
  }
  return null;
}

/**
 * Read a file with a byte cap so we don't blow up the MCP client with
 * 100 KB of unwanted content. Returns `{text, truncated}` — caller
 * includes the truncation flag in their response.
 */
export async function readCapped(
  abs: string,
): Promise<{ text: string; truncated: boolean }> {
  const stat = await fs.stat(abs);
  const truncated = stat.size > MAX_FILE_BYTES;
  const buf = await fs.readFile(abs);
  const slice = truncated ? buf.subarray(0, MAX_FILE_BYTES) : buf;
  return { text: slice.toString('utf8'), truncated };
}

/**
 * Path-safety gatekeeper. Resolves `input` against `universe`, rejects
 * anything that escapes the root. Returns the resolved absolute path on
 * success; throws a string on violation so the caller can surface a
 * meaningful MCP error.
 */
export function resolveInUniverse(universe: string, input: string): string {
  const universeAbs = path.resolve(universe);
  const resolved = path.isAbsolute(input)
    ? path.resolve(input)
    : path.resolve(universeAbs, input);
  const rel = path.relative(universeAbs, resolved);
  if (rel.startsWith('..') || path.isAbsolute(rel)) {
    throw `path '${input}' escapes the Universe root`;
  }
  return resolved;
}

// ─── Dynamic Universe discovery ──────────────────────────────────────

/**
 * Walk up from `startPath` looking for the enclosing Universe — any
 * directory that has both a `Spaces/` subdirectory and (optionally) a
 * `.eustress/` marker. Stops at filesystem root.
 *
 * The "has Spaces/" check alone is enough to call a folder a Universe
 * at the fs layer; `.eustress/` is a nice-to-have that signals the
 * engine has actually initialised there.
 *
 * Returns `null` if no Universe is found anywhere between `startPath`
 * and `/` — callers typically fall back to an explicit config or bail.
 */
export function findUniverseRoot(startPath: string): string | null {
  let cur = path.resolve(startPath);
  const seen = new Set<string>();
  // Bounded walk so a pathological cycle (Windows junction loops) can't
  // hang the tool call.
  for (let i = 0; i < 64; i++) {
    if (seen.has(cur)) return null;
    seen.add(cur);
    const spaces = path.join(cur, 'Spaces');
    if (fsSync.existsSync(spaces) && fsSync.statSync(spaces).isDirectory()) {
      return cur;
    }
    const parent = path.dirname(cur);
    if (parent === cur) return null;  // hit filesystem root
    cur = parent;
  }
  return null;
}

/**
 * Shallow scan of `roots` for directories that look like Universes.
 * Used by `eustress_list_universes`. Each root is checked one level
 * deep — `roots[i]/<child>/Spaces/` — which matches how users typically
 * organise multiple projects (e.g. `~/Eustress/MyGame/Spaces/`,
 * `~/Eustress/SideProject/Spaces/`).
 *
 * Also returns any root that is itself a Universe.
 */
export async function discoverUniverses(roots: string[]): Promise<string[]> {
  const out = new Set<string>();

  for (const raw of roots) {
    const root = path.resolve(raw);
    if (fsSync.existsSync(path.join(root, 'Spaces'))) {
      out.add(root);
    }
    let entries: fsSync.Dirent[] = [];
    try {
      entries = await fs.readdir(root, { withFileTypes: true });
    } catch { continue; }
    for (const e of entries) {
      if (!e.isDirectory() || e.name.startsWith('.')) continue;
      const child = path.join(root, e.name);
      if (fsSync.existsSync(path.join(child, 'Spaces'))) {
        out.add(child);
      }
    }
  }
  return Array.from(out).sort();
}
