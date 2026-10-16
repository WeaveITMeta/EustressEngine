// Put the hash of every inline script into the Content-Security-Policy of dist/_headers.
//
//   node scripts/csp-inline-hashes.mjs [dist]        (default: dist)
//
// Run it after `trunk build` and the prerender, before `wrangler pages deploy`. Trunk writes one inline
// <script type="module"> into index.html that starts the WebAssembly app; its text names the hashed .js and .wasm
// files, so it changes with every build and cannot be hashed by hand. The page's own scripts are files (/assets/*.js).
// This script finds every inline executable script in every .html under dist, hashes its exact text
// (sha256, base64: what the browser compares), and writes the hashes into the policy's script-src. Running it again
// replaces the hashes it wrote earlier, so the list never grows. A script that is not 'self', not hashed, and not
// in the list is what the policy reports; with `script-src 'self' 'wasm-unsafe-eval'` alone, Trunk's start-up script
// was reported on every page view.

import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

/// Every .html file under `dir`.
function htmlFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const info = statSync(path);
    if (info.isDirectory()) out.push(...htmlFiles(path));
    else if (name.endsWith('.html')) out.push(path);
  }
  return out;
}

/// The text of every inline script a browser would run: no src, and a type that runs
/// (none, module, text/javascript). Data blocks such as application/ld+json are not scripts.
export function inlineScripts(html) {
  const found = [];
  const re = /<script\b([^>]*)>([\s\S]*?)<\/script>/gi;
  let m;
  while ((m = re.exec(html))) {
    const attrs = m[1];
    if (/\bsrc\s*=/i.test(attrs)) continue;
    const type = /\btype\s*=\s*["']?([^"'\s>]+)/i.exec(attrs)?.[1]?.toLowerCase();
    if (type && !['module', 'text/javascript', 'application/javascript'].includes(type)) continue;
    if (m[2].trim() === '') continue;
    found.push(m[2]);
  }
  return found;
}

export function hashOf(text) {
  return `'sha256-${createHash('sha256').update(text, 'utf8').digest('base64')}'`;
}

/// The policy line with `hashes` as the only sha256 sources of its script-src.
export function withHashes(headers, hashes) {
  const lines = headers.split('\n');
  let changed = 0;
  const out = lines.map((line) => {
    if (!/^\s*Content-Security-Policy(-Report-Only)?:/i.test(line)) return line;
    return line.replace(/(script-src)([^;]*)(;|$)/i, (_all, name, sources, end) => {
      changed += 1;
      const kept = sources.split(/\s+/).filter((s) => s && !/^'sha256-/.test(s));
      return `${name} ${[...kept, ...hashes].join(' ')}${end}`;
    });
  });
  return { text: out.join('\n'), changed };
}

function main() {
  const dist = process.argv[2] || 'dist';
  const hashes = new Set();
  const files = htmlFiles(dist);
  for (const file of files) for (const script of inlineScripts(readFileSync(file, 'utf8'))) hashes.add(hashOf(script));
  const headersPath = join(dist, '_headers');
  const headers = readFileSync(headersPath, 'utf8');
  const crlf = headers.includes('\r\n');
  const { text, changed } = withHashes(headers.replace(/\r\n/g, '\n'), [...hashes].sort());
  if (changed === 0) {
    console.error(`${headersPath} has no Content-Security-Policy with a script-src: nothing written`);
    process.exit(1);
  }
  writeFileSync(headersPath, crlf ? text.replace(/\n/g, '\r\n') : text);
  console.log(`${files.length} pages, ${hashes.size} distinct inline script(s) hashed into ${headersPath}`);
}

if (import.meta.url === `file://${process.argv[1].replace(/\\/g, '/')}` || process.argv[1]?.endsWith('csp-inline-hashes.mjs')) main();
