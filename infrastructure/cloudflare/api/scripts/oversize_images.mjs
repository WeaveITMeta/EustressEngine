#!/usr/bin/env node
// READ-ONLY. Approved listings with a bound image (a capture, the thumbnail or the cover) that the judge would have left out:
// over the per-image limit, past the request's image budget, of a type the API does not take, or past the most images a
// request carries. The report replays the judge's own plan (judgeImagePlan in src/moderation.mjs, the function judgeImages
// decides through) over each listing's images in the judge's order, by their stored size (base64 length 4 * ceil(n / 3)) and
// type, and names every image left out and why. A person re-reviews each listing it names (moderation_rerun).
//
// It also names each bound image an upload refuses or changes today (`problem`): an animation, an animated GIF, transparency
// or a second picture, which the Worker answers to the public as absent; or a colour transform or other dropped chunk
// alone, which the Worker serves as the rebuilt copy (served_as: stripped).
//
// It changes nothing: it does not unpublish, hold or re-review anything. What to do with a listing it names is McKale's
// decision. The Worker already answers an image over the per-image limit to the public as absent; one left out for any
// other reason (budget, type, count) is still served until a person acts.
//
// Type: wrangler's r2 object get returns the bytes and not the stored Content-Type the judge read, so the type is taken from
// the bytes (PNG, JPEG, WebP or GIF by their signature). A legacy object whose stored type disagrees with its bytes can be
// reported as sent when the judge left it out; each image in the report says `type_from: "bytes"` for that reason.
//
//   node scripts/oversize_images.mjs [--env=staging|production] [--limit=N] [--after=<sim id>] [--wrangler-js=<path>] > oversize.json
//
// --env=staging is the default. Production is read only when it is named; running it is McKale's call. This file does not run
// either environment by itself.
//
// Bounded: at most --limit approved listings (default 50, at most 500) per run, in the order of the modq:approved: index, from
// the one after --after. The report's next_after continues the scan.
//
// Every call is a wrangler read (kv key list, kv key get, r2 object get), checked by assertReadOnly from voided_approvals.mjs
// before it is spawned. wrangler has no r2 head, so an image's size is the length of what r2 object get returns. wrangler is
// run as `node <wrangler.js>` with no shell. An empty result (exit 0, `flagged: []`) and an error (exit 1, a message on stderr,
// nothing on stdout) are never confused: a read that fails for any reason but "not found" stops the run.

import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { assertReadOnly, kvGetArgs, kvListArgs, r2GetArgs, ENVIRONMENTS, DEFAULT_ENV, APPROVED_PREFIX } from './voided_approvals.mjs';
import { JUDGE_IMAGE_MAX_BYTES, evidencePrefix, judgeImagePlan, base64Length } from '../src/moderation.mjs';
import { storedImageProblem, pngColourType, imageVerdictKey } from '../src/user_images.mjs';

export const DEFAULT_LIMIT = 50;
export const MAX_LIMIT = 500;
const EXTS = ['webp', 'png', 'jpg'];
const CAPTURE_EXTS = ['png', 'jpg', 'webp'];
const SIM_ID = /^[0-9a-fA-F-]{36}$/;

/// The command line's options. Anything unknown is an error.
export function parseArgs(argv) {
  const opts = { env: DEFAULT_ENV, limit: DEFAULT_LIMIT, after: null, wranglerJs: null };
  for (const arg of argv) {
    if (arg.startsWith('--env=')) {
      const name = arg.slice('--env='.length);
      if (!Object.hasOwn(ENVIRONMENTS, name)) throw new Error(`--env must be staging or production, not "${name}"`);
      opts.env = name;
    } else if (arg.startsWith('--limit=')) {
      const n = Number(arg.slice('--limit='.length));
      if (!Number.isInteger(n) || n < 1 || n > MAX_LIMIT) throw new Error(`--limit must be a whole number from 1 to ${MAX_LIMIT}`);
      opts.limit = n;
    } else if (arg.startsWith('--after=')) {
      const id = arg.slice('--after='.length);
      if (!SIM_ID.test(id)) throw new Error('--after must be a listing id');
      opts.after = id;
    } else if (arg.startsWith('--wrangler-js=')) opts.wranglerJs = arg.slice('--wrangler-js='.length);
    else throw new Error(`unknown argument ${arg}; usage: [--env=staging|production] [--limit=N] [--after=<sim id>] [--wrangler-js=<path>]`);
  }
  return opts;
}

/// The listing ids to check this run: those after `after` (or from the start), at most `limit`, and where the next run starts.
/// Pure.
export function pageOf(ids, { after = null, limit = DEFAULT_LIMIT } = {}) {
  const sorted = [...new Set(ids)].sort();
  const start = after ? sorted.findIndex(id => id > after) : 0;
  const rest = start < 0 ? [] : sorted.slice(start);
  const page = rest.slice(0, limit);
  return { page, next_after: rest.length > limit ? page.at(-1) : null };
}

/// The type an image's bytes are, by their signature, or null.
export function typeOfBytes(bytes) {
  const b = bytes || [];
  if (b.length >= 8 && b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47 && b[4] === 0x0d && b[5] === 0x0a && b[6] === 0x1a && b[7] === 0x0a) return 'image/png';
  if (b.length >= 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'image/jpeg';
  if (b.length >= 12 && String.fromCharCode(...b.slice(0, 4)) === 'RIFF' && String.fromCharCode(...b.slice(8, 12)) === 'WEBP') return 'image/webp';
  if (b.length >= 6 && String.fromCharCode(...b.slice(0, 4)) === 'GIF8') return 'image/gif';
  return null;
}

/// How many frames a GIF holds (its image descriptors), by walking its blocks; 0 for bytes that are not a GIF. Pure and
/// bounded by the bytes: each step moves forward.
export function gifFrames(bytes) {
  const b = bytes || [];
  if (b.length < 13 || b[0] !== 0x47 || b[1] !== 0x49 || b[2] !== 0x46) return 0;
  let o = 13;
  if (b[10] & 0x80) o += 3 * (1 << ((b[10] & 7) + 1));
  const skipSubBlocks = () => { while (o < b.length) { const n = b[o++]; if (n === 0) return true; o += n; } return false; };
  let frames = 0;
  while (o < b.length) {
    const block = b[o++];
    if (block === 0x3b) break;
    if (block === 0x21) { o += 1; if (!skipSubBlocks()) break; } else if (block === 0x2c) {
      frames += 1;
      const flags = b[o + 8];
      o += 9;
      if (flags & 0x80) o += 3 * (1 << ((flags & 7) + 1));
      o += 1;
      if (!skipSubBlocks()) break;
    } else break;
  }
  return frames;
}

/// Why stored bytes are not listing art an upload would store today (src/user_images.mjs storedImageProblem), with a GIF
/// named for what it is: 'animated_gif' for more than one frame, 'gif' for one. Null when an upload would store them as
/// they are. Pure.
export function artProblem(bytes) {
  const problem = storedImageProblem(bytes);
  if (problem !== 'not_an_accepted_image') return problem;
  const frames = gifFrames(bytes);
  return frames > 1 ? 'animated_gif' : frames === 1 ? 'gif' : problem;
}

/// The judge's plan replayed over one listing's images, given in the judge's order (the thumbnail, then the cover, then the
/// captures by key): `images` are { kind, key, label, contentType, bytes, problem } (`problem`: artProblem of the stored
/// bytes, or null). Returns each image left out or refused by upload rules now: `why` is judgeImagePlan's reason (null when
/// the judge was sent it), `problem` the upload rule it fails (null when none). The Worker answers the public as if an image
/// were absent when it is over the per-image cap or has a problem (served_to_public false). Pure.
export function replayListing(images) {
  const list = Array.isArray(images) ? images : [];
  const plan = judgeImagePlan(list.map(i => ({ label: i.label, contentType: i.contentType, b64Length: Number.isFinite(i.bytes) ? base64Length(i.bytes) : null })));
  const why = new Map(plan.left.map(l => [l.index, l.why]));
  const out = [];
  for (const [index, i] of list.entries()) {
    const reason = why.get(index) ?? null;
    const problem = i.problem ?? null;
    if (!reason && !problem) continue;
    out.push({ kind: i.kind, key: i.key, label: String(i.label ?? `image ${index}`), bytes: i.bytes, why: reason, problem, type_from: 'bytes', served_to_public: !((problem && problem !== 'dropped_at_upload') || reason === 'size' || i.bytes > JUDGE_IMAGE_MAX_BYTES), served_as: problem === 'dropped_at_upload' ? 'stripped' : null });
  }
  return out;
}

/// A listing's images in the order the judge is sent them (src/moderation.mjs fetchCaptures): the thumbnail, then the cover,
/// then the captures sorted by key. The public art first, so the request's image budget can only ever cut a capture. Pure.
export function inJudgeOrder(images) {
  const list = Array.isArray(images) ? images : [];
  const captures = list.filter(i => i.kind === 'capture').sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
  return [...list.filter(i => i.kind === 'thumbnail'), ...list.filter(i => i.kind === 'cover'), ...captures];
}

/// How the upload's alpha measure (src/user_images.mjs measurePngAlpha, recorded in each image's verdict record) came out over
/// the RGBA PNG captures and thumbnails of the listings checked: how many read true (alpha used), false (opaque) and
/// 'unknown' (the measure stopped: past its time budget, an interlaced image, a stream past or short of its header), how many
/// have no measure (`unmeasured`: stored before measuring, or no verdict record), and the unknown rate over the measured
/// ones (null when none is measured). `images` carry `rgba` and `alpha_used` (undefined when unmeasured). Pure.
export function countAlpha(entries) {
  const out = { true: 0, false: 0, unknown: 0, unmeasured: 0, unknown_rate: null };
  for (const { images } of entries || []) {
    for (const i of images || []) {
      if (!i?.rgba || (i.kind !== 'capture' && i.kind !== 'thumbnail')) continue;
      if (i.alpha_used === true) out.true += 1;
      else if (i.alpha_used === false) out.false += 1;
      else if (i.alpha_used === 'unknown') out.unknown += 1;
      else out.unmeasured += 1;
    }
  }
  const measured = out.true + out.false + out.unknown;
  out.unknown_rate = measured ? Math.round((out.unknown / measured) * 10000) / 10000 : null;
  return out;
}

/// The report over every listing checked. `entries` are { id, sim, binding, images }, `images` in the judge's order. A
/// listing counts only when its record says approved and its binding is in force. Pure.
export function summarize(entries, { approvedIndexed = entries.length, next_after = null, max = JUDGE_IMAGE_MAX_BYTES } = {}) {
  const report = { approved_indexed: approvedIndexed, checked: 0, skipped: { not_approved: 0, unbound: 0, revoked: 0 }, max_bytes: max, flagged: [], by_kind: { capture: 0, thumbnail: 0, cover: 0 }, by_reason: {}, by_problem: { served_stripped: {}, refused: {} }, next_after };
  for (const { id, sim, binding, images } of entries) {
    if (sim?.moderation?.status !== 'approved') { report.skipped.not_approved += 1; continue; }
    if (!binding) { report.skipped.unbound += 1; continue; }
    if (binding.revoked) { report.skipped.revoked += 1; continue; }
    report.checked += 1;
    const left = replayListing(images);
    for (const o of left) {
      report.by_kind[o.kind] = (report.by_kind[o.kind] || 0) + 1;
      if (o.why) report.by_reason[o.why] = (report.by_reason[o.why] || 0) + 1;
      // Colour transforms, EXIF and the like alone: the public is served the rebuilt copy. Anything else: answered as absent.
      const side = o.problem === 'dropped_at_upload' ? report.by_problem.served_stripped : report.by_problem.refused;
      if (o.problem) side[o.problem] = (side[o.problem] || 0) + 1;
    }
    if (left.length) report.flagged.push({ sim_id: id, name: sim.name ?? null, author_id: sim.author_id ?? null, images: left });
  }
  report.alpha = countAlpha(entries.filter(e => e.sim?.moderation?.status === 'approved' && e.binding && !e.binding.revoked));
  return report;
}

/// The keys of a listing's images a review binds: the thumbnail and cover in each format, and each bound capture in each format.
export function imageKeys(id, binding) {
  const keys = [
    ...EXTS.map(ext => ({ kind: 'thumbnail', key: `thumbnails/${id}/thumb.${ext}`, group: 'thumbnail' })),
    ...EXTS.map(ext => ({ kind: 'cover', key: `covers/${id}/cover.${ext}`, group: 'cover' })),
  ];
  const epoch = Number.isInteger(binding?.evidence_epoch) ? binding.evidence_epoch : 0;
  for (const n of Object.keys(binding?.captures || {}).filter(k => /^[0-7]$/.test(k)).sort()) {
    for (const ext of CAPTURE_EXTS) keys.push({ kind: 'capture', key: `${evidencePrefix(id, epoch)}capture-${n}.${ext}`, group: `capture-${n}` });
  }
  return keys;
}

/// A wrangler runner: every call checked as a read, then spawned as `node <wrangler.js> ...args` with no shell.
export function makeRunner({ spawnImpl = spawn, wranglerJs, nodePath = process.execPath } = {}) {
  return function run(args) {
    assertReadOnly(args);
    if (!wranglerJs) throw new Error('wrangler is not installed here; pass --wrangler-js=<path to wrangler/bin/wrangler.js>');
    return new Promise((resolve, reject) => {
      const child = spawnImpl(nodePath, [wranglerJs, ...args.map(String)], { shell: false, windowsHide: true });
      const out = [];
      const err = [];
      child.stdout.on('data', d => out.push(d));
      child.stderr.on('data', d => err.push(d));
      child.on('error', reject);
      child.on('close', code => resolve({ code, stdout: Buffer.concat(out), stderr: Buffer.concat(err).toString('utf8') }));
    });
  };
}

function resolveWrangler(given) {
  if (given) return given;
  try { return createRequire(import.meta.url).resolve('wrangler/bin/wrangler.js'); } catch { return null; }
}

/// The stdout of a read, or null when the key or object is not there. Any other failure throws.
async function read(run, args) {
  const r = await run(args);
  if (r.code === 0) return r.stdout;
  if (/key|object/i.test(r.stderr) && /not ?found|does not exist|no such|NoSuchKey|10007|10009/i.test(r.stderr)) return null;
  throw new Error(`wrangler ${args.slice(0, 3).join(' ')} failed (exit ${r.code}): ${r.stderr.trim().slice(0, 400)}`);
}

const parseOrNull = text => { try { return JSON.parse(text); } catch { return null; } };

async function gather(run, cfg, id) {
  const simText = await read(run, kvGetArgs(cfg, `sim:${id}`));
  const sim = simText && simText.length ? parseOrNull(simText.toString('utf8')) : null;
  if (sim?.moderation?.status !== 'approved') return { id, sim, binding: null, images: [] };
  const bindingBytes = await read(run, r2GetArgs(cfg, `locks/approved/${id}.json`));
  const binding = bindingBytes && bindingBytes.length ? (parseOrNull(bindingBytes.toString('utf8')) || { revoked: true }) : null;
  if (!binding || binding.revoked) return { id, sim, binding, images: [] };
  // The judge's order: the first thumbnail and cover found, then every capture stored for a bound number, sorted by key.
  const captures = [];
  const art = [];
  const found = new Set();
  for (const k of imageKeys(id, binding)) {
    if (k.kind !== 'capture' && found.has(k.group)) continue;
    const bytes = await read(run, r2GetArgs(cfg, k.key));
    if (bytes === null) continue;
    found.add(k.group);
    const label = k.kind === 'capture' ? k.key.split('/').pop() : k.kind === 'thumbnail' ? 'listing thumbnail (the image the Gallery shows)' : 'listing cover (the box art the Gallery shows)';
    const image = { kind: k.kind, key: k.key, label, contentType: typeOfBytes(bytes) || 'application/octet-stream', bytes: bytes.length, problem: artProblem(new Uint8Array(bytes)) };
    // An RGBA PNG capture or thumbnail: what its upload measured of its alpha, from its verdict record (keyed by the SHA-256
    // the upload stored it with). A read like every other here.
    if (k.kind !== 'cover' && pngColourType(new Uint8Array(bytes)) === 6) {
      image.rgba = true;
      const ident = `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
      const record = parseOrNull(((await read(run, r2GetArgs(cfg, imageVerdictKey(ident)))) || Buffer.from('null')).toString('utf8'));
      image.alpha_used = record?.alpha_used;
    }
    (k.kind === 'capture' ? captures : art).push(image);
  }
  const images = inJudgeOrder([...art, ...captures]);
  return { id, sim, binding, images };
}

async function main(argv) {
  const opts = parseArgs(argv);
  const cfg = { name: opts.env, ...ENVIRONMENTS[opts.env] };
  const run = makeRunner({ wranglerJs: resolveWrangler(opts.wranglerJs) });
  const log = msg => process.stderr.write(`${msg}\n`);
  const listed = await read(run, kvListArgs(cfg, APPROVED_PREFIX));
  // An empty index is an answer; output that is not a list of keys is an error, never read as empty.
  const keys = listed === null ? null : parseOrNull(listed.toString('utf8'));
  if (!Array.isArray(keys)) throw new Error('kv key list did not return a list of keys');
  const ids = keys.map(k => String(k?.name ?? '').slice(APPROVED_PREFIX.length)).filter(id => SIM_ID.test(id));
  const { page, next_after } = pageOf(ids, opts);
  const entries = [];
  for (const id of page) {
    entries.push(await gather(run, cfg, id));
    log(`checked ${entries.length} of ${page.length}`);
  }
  const report = summarize(entries, { approvedIndexed: ids.length, next_after });
  process.stdout.write(`${JSON.stringify({ env: cfg.name, generated_at: new Date().toISOString(), status: 'ok', empty: report.flagged.length === 0, ...report, note: 'Read only; nothing was changed. Each flagged listing is approved with a bound image the judge would have left out (why: size, budget, type, count or unreadable), so that image was never judged. A person re-reviews it (moderation_rerun); whether to unpublish or hold anything is McKale\'s decision. served_to_public is false for an image over the per-image limit or with a problem that refuses it (animated, transparent, multiple_pictures, gif, animated_gif, dimensions, not_an_accepted_image): the Worker answers the public as if those were absent. A problem of dropped_at_upload alone (a colour transform, EXIF and the like) is served as the rebuilt copy (served_as: stripped); by_problem counts the two apart. The type is read from the bytes (type_from), not the stored Content-Type. alpha counts what each RGBA capture and thumbnail\'s upload measured of its alpha (true, false, unknown, unmeasured) and the unknown rate; it decides nothing.' }, null, 2)}\n`);
  log(`${report.flagged.length} flagged of ${report.checked} checked (${ids.length} indexed) on ${cfg.name}${next_after ? `; continue with --after=${next_after}` : ''}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  main(process.argv.slice(2)).catch(e => {
    process.stderr.write(`${e.message}\n`);
    process.exitCode = 1;
  });
}
