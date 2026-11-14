// =============================================================================
// The sim page: PageSpec v1, its routes, and the public view
// =============================================================================
//
// A creator themes their sim's public page (`/simulation/:id` on the website) from the Website service in Studio, which
// bakes it into one JSON document (PageSpec v1) and uploads it at publish. This module validates it exactly per the
// contract (docs/design/SIM_PAGE_THEME_AND_LINKS.md section 2), stores it as the draft, starts the link and image review
// (src/link_review.mjs), and serves only what the review approved. Default-deny: a pending or removed link or image is
// simply not shown.
//
// The theme is data, never CSS: colours are validated hex, applied by the website as CSS custom properties; fonts, layouts
// and motion are enums. No author HTML, CSS or script reaches the page.
//
// Routes (section 3):
//   PUT /api/simulations/{id}/page               owner   validate, store the draft, queue the review
//   PUT /api/simulations/{id}/page/image/{slot}  owner   PNG, JPEG or WebP, at most 2 MB, stored at R2 universes/{id}/page/{slot}
//   GET /api/simulations/{id}/page               public  the approved page, withheld with the listing
//   GET /api/simulations/{id}/page/image/{slot}  public  the image bytes, only when approved
//   GET /api/simulations/{id}/page/review        owner   each link and image: pending, approved or removed, with why

import { prepareUserImage, USER_IMAGE_HEADERS } from './user_images.mjs';
import { contentLockedFor, inReview, simWithLegalLock } from './moderation.mjs';
import {
  normalizeLinkUrl, hasUrl, PAGE_LINK_KINDS, PAGE_IMAGE_SLOTS, pageDraftKey, pageImageR2Key, readLinkState, writeLinkState,
  readPageDraft, syncLinkEntries, queueLinkReview, stepLinkReview, reviewView, listingFacts, linkKindPermitted,
} from './link_review.mjs';

export const PAGE_SCHEMA = 1;
export const PAGE_SPEC_MAX_BYTES = 64 * 1024;
export const PAGE_IMAGE_MAX_BYTES = 2 * 1024 * 1024;
export const LINK_LABEL_MAX = 40;
/// At most this many links of each kind, and 17 in all (section 2).
export const LINK_CAPS = Object.freeze({ website: 1, social: 8, shop: 4, other: 4 });
export const LINKS_MAX = 17;
export const GALLERY_MAX = 6;
export const MIN_CONTRAST = 4.5;

/// The six presets, each a full colour set (section 11; Studio's eustress/crates/common/src/page_spec.rs PRESETS must match).
export const PAGE_PRESETS = Object.freeze({
  studio: Object.freeze({ primary: '#4F8CFF', accent: '#FFB547', background: '#0E1116', surface: '#171B22', text: '#E8ECF2' }),
  daylight: Object.freeze({ primary: '#1F5FCC', accent: '#B93C0A', background: '#FFFFFF', surface: '#F1F4F9', text: '#111827' }),
  blueprint: Object.freeze({ primary: '#8DB8FF', accent: '#FFD166', background: '#0B2545', surface: '#13315C', text: '#EAF2FF' }),
  laboratory: Object.freeze({ primary: '#0B6E78', accent: '#B2400C', background: '#F3F9F8', surface: '#FFFFFF', text: '#0F2A2E' }),
  arcade: Object.freeze({ primary: '#FF4DB2', accent: '#3DF5E0', background: '#120B2E', surface: '#1E1450', text: '#F5F0FF' }),
  paper: Object.freeze({ primary: '#8A3B12', accent: '#2F6B4F', background: '#FAF6EE', surface: '#F0E8D6', text: '#2B2620' }),
});
export const PAGE_COLOR_KEYS = Object.freeze(['primary', 'accent', 'background', 'surface', 'text']);
export const PAGE_FONTS = Object.freeze(['inter', 'space-grotesk', 'ibm-plex-sans', 'jetbrains-mono', 'merriweather']);
export const PAGE_LAYOUTS = Object.freeze(['classic', 'wide', 'compact']);
export const PAGE_MOTIONS = Object.freeze(['none', 'subtle', 'full']);
// What Studio sends for a field left unset (section 11).
const THEME_DEFAULTS = Object.freeze({ preset: 'studio', font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 });
const HEX = /^#[0-9a-fA-F]{6}$/;
// Control characters, and the bidirectional overrides and isolates that can make a label read as something else.
const LABEL_FORBIDDEN = /[\p{Cc}​-‏‪-‮⁦-⁩﻿]/u;

// ── Contrast (WCAG 2.x) ──────────────────────────────────────────────────────

const channel = c => { const s = c / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
export function relativeLuminance(hex) {
  const n = parseInt(hex.slice(1), 16);
  return 0.2126 * channel((n >> 16) & 255) + 0.7152 * channel((n >> 8) & 255) + 0.0722 * channel(n & 255);
}
/// The WCAG contrast ratio of two `#RRGGBB` colours, (L1 + 0.05) / (L2 + 0.05).
export function contrastRatio(a, b) {
  const [hi, lo] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
const readable = c => contrastRatio(c.text, c.background) >= MIN_CONTRAST && contrastRatio(c.text, c.surface) >= MIN_CONTRAST;

// ── PageSpec v1 ──────────────────────────────────────────────────────────────

const isObject = v => !!v && typeof v === 'object' && !Array.isArray(v);
const refuse = (rule, error) => ({ ok: false, rule, error });

/// One link label: plain text, 1 to 40 characters, no URL, no control characters. The trimmed label, or null.
export function cleanLabel(value) {
  if (typeof value !== 'string') return null;
  const t = value.trim();
  if (!t || [...t].length > LINK_LABEL_MAX || LABEL_FORBIDDEN.test(t) || hasUrl(t)) return null;
  return t;
}

/// Validates a PageSpec v1 exactly per section 2. `{ ok: true, spec, notes }` with the spec as it will be stored (unknown
/// keys dropped, colours upper case, the full colour set resolved), or `{ ok: false, rule, error }` naming the rule broken:
/// a spec that fails a rule is refused whole, so a broken Studio never half-publishes. `notes` says what the Worker changed
/// (the contrast fallback).
export function validatePageSpec(raw) {
  if (!isObject(raw)) return refuse('spec', 'The page must be a JSON object.');
  if (raw.schema !== PAGE_SCHEMA) return refuse('schema', 'schema must be 1.');
  const theme = raw.theme === undefined ? {} : raw.theme;
  if (!isObject(theme)) return refuse('theme', 'theme must be an object.');
  const preset = theme.preset ?? THEME_DEFAULTS.preset;
  if (!Object.hasOwn(PAGE_PRESETS, preset)) return refuse('theme.preset', `theme.preset must be one of ${Object.keys(PAGE_PRESETS).join(', ')}.`);
  const overrides = theme.colors === undefined ? {} : theme.colors;
  if (!isObject(overrides)) return refuse('theme.colors', 'theme.colors must be an object.');
  const colors = { ...PAGE_PRESETS[preset] };
  for (const key of PAGE_COLOR_KEYS) {
    if (overrides[key] === undefined || overrides[key] === null) continue;
    if (typeof overrides[key] !== 'string' || !HEX.test(overrides[key])) return refuse(`theme.colors.${key}`, `theme.colors.${key} must be #RRGGBB.`);
    colors[key] = overrides[key].toUpperCase();
  }
  const notes = [];
  // The contrast rule: text on background and on surface at 4.5:1. Below it the preset's text is used; if even that does
  // not reach it on the colours chosen, the preset's background and surface are used too, so the page is always readable.
  if (!readable(colors)) {
    colors.text = PAGE_PRESETS[preset].text;
    if (readable(colors)) notes.push(`The text colour did not reach a contrast of 4.5:1 on the background and surface, so the ${preset} preset's text colour is used.`);
    else {
      colors.background = PAGE_PRESETS[preset].background;
      colors.surface = PAGE_PRESETS[preset].surface;
      notes.push(`The text colour did not reach a contrast of 4.5:1 on the background and surface, so the ${preset} preset's text, background and surface colours are used.`);
    }
  }
  const font = theme.font ?? THEME_DEFAULTS.font;
  if (!PAGE_FONTS.includes(font)) return refuse('theme.font', `theme.font must be one of ${PAGE_FONTS.join(', ')}.`);
  const layout = theme.layout ?? THEME_DEFAULTS.layout;
  if (!PAGE_LAYOUTS.includes(layout)) return refuse('theme.layout', `theme.layout must be one of ${PAGE_LAYOUTS.join(', ')}.`);
  const motion = theme.motion ?? THEME_DEFAULTS.motion;
  if (!PAGE_MOTIONS.includes(motion)) return refuse('theme.motion', `theme.motion must be one of ${PAGE_MOTIONS.join(', ')}.`);
  const interval = theme.slideshow_interval_s ?? THEME_DEFAULTS.slideshow_interval_s;
  if (!Number.isInteger(interval) || interval < 3 || interval > 30) return refuse('theme.slideshow_interval_s', 'theme.slideshow_interval_s must be a whole number from 3 to 30.');
  const fade = theme.banner_fade_ms ?? THEME_DEFAULTS.banner_fade_ms;
  if (!Number.isInteger(fade) || fade < 0 || fade > 2000) return refuse('theme.banner_fade_ms', 'theme.banner_fade_ms must be a whole number from 0 to 2000.');

  const imagesIn = raw.images === undefined ? {} : raw.images;
  if (!isObject(imagesIn)) return refuse('images', 'images must be an object.');
  const images = { logo: null, banner: null, gallery: [] };
  for (const slot of ['logo', 'banner']) {
    const v = imagesIn[slot];
    if (v === undefined || v === null || v === '') continue;
    if (v !== slot) return refuse(`images.${slot}`, `images.${slot} must be the slot name "${slot}".`);
    images[slot] = slot;
  }
  const gallery = imagesIn.gallery === undefined ? [] : imagesIn.gallery;
  if (!Array.isArray(gallery) || gallery.length > GALLERY_MAX) return refuse('images.gallery', `images.gallery must be a list of at most ${GALLERY_MAX} slot names.`);
  for (const v of gallery) {
    if (typeof v !== 'string' || !/^gallery-[0-5]$/.test(v) || images.gallery.includes(v)) return refuse('images.gallery', 'images.gallery holds the slot names gallery-0 to gallery-5, each at most once.');
    images.gallery.push(v);
  }

  const linksIn = raw.links === undefined ? [] : raw.links;
  if (!Array.isArray(linksIn)) return refuse('links', 'links must be a list.');
  if (linksIn.length > LINKS_MAX) return refuse('links', `links holds at most ${LINKS_MAX} links.`);
  const counts = {};
  const links = [];
  for (const [i, l] of linksIn.entries()) {
    if (!isObject(l)) return refuse(`links[${i}]`, `links[${i}] must be an object.`);
    if (!PAGE_LINK_KINDS.includes(l.kind)) return refuse(`links[${i}].kind`, `links[${i}].kind must be one of ${PAGE_LINK_KINDS.join(', ')}.`);
    counts[l.kind] = (counts[l.kind] || 0) + 1;
    if (counts[l.kind] > LINK_CAPS[l.kind]) return refuse('links', `links holds at most ${LINK_CAPS[l.kind]} ${l.kind} link${LINK_CAPS[l.kind] === 1 ? '' : 's'}.`);
    const url = normalizeLinkUrl(l.url);
    if (typeof l.url !== 'string' || !/^https:\/\//i.test(l.url.trim()) || !url) return refuse(`links[${i}].url`, `links[${i}].url must be https://, at most 500 characters, with no user or password, no port, no IP address, on a public host.`);
    const label = cleanLabel(l.label);
    if (label === null) return refuse(`links[${i}].label`, `links[${i}].label must be plain text of 1 to ${LINK_LABEL_MAX} characters, with no URL and no control characters.`);
    links.push({ kind: l.kind, url, label });
  }
  return {
    ok: true,
    notes,
    spec: {
      schema: PAGE_SCHEMA,
      theme: { preset, colors, font, layout, motion, slideshow_interval_s: interval, banner_fade_ms: fade },
      images,
      links,
    },
  };
}

// ── The public view ──────────────────────────────────────────────────────────

/// The listing statuses under which the page is withheld: the website manifest's (src/index.js handleGetWebsiteManifest).
export const PAGE_WITHHELD_STATUSES = Object.freeze(['quarantined', 'held', 'rejected', 'changes_requested', 'appealed']);
export const pageWithheld = listing => !listing || PAGE_WITHHELD_STATUSES.includes(listing.moderation?.status) || !!listing.deletion;

export const pageImagePath = (simId, slot) => `/api/simulations/${simId}/page/image/${slot}`;

/// The page as the public is shown it, in the shape the website reads: the validated theme, the approved images as
/// `/api/simulations/{id}/page/image/{slot}` paths (null, or left out of the gallery, while pending or removed), and only
/// the approved links the listing may carry now (section 7 checked again at read time). Pure.
export function publicPageView(simId, draft, state, listing) {
  const spec = draft.spec;
  const facts = listingFacts(listing);
  const approvedImage = slot => state.images?.[slot]?.status === 'approved';
  const approvedLinks = new Map(Object.values(state.links || {}).filter(e => e?.source === 'page' && e.status === 'approved').map(e => [`${e.kind}\u0000${e.url}`, e]));
  const links = spec.links.filter(l => {
    const e = approvedLinks.get(`${l.kind}\u0000${l.url}`);
    return e && linkKindPermitted({ declaredKind: l.kind, aiKind: e.verdict?.kind, listing: facts }).ok;
  }).map(l => ({ kind: l.kind, url: l.url, label: l.label }));
  return {
    theme: { ...spec.theme, colors: { ...spec.theme.colors } },
    images: {
      logo: spec.images.logo && approvedImage('logo') ? pageImagePath(simId, 'logo') : null,
      banner: spec.images.banner && approvedImage('banner') ? pageImagePath(simId, 'banner') : null,
      gallery: spec.images.gallery.filter(approvedImage).map(slot => pageImagePath(simId, slot)),
    },
    links,
  };
}

// ── Routes ───────────────────────────────────────────────────────────────────

async function ownedListing(request, simId, env, deps) {
  const { json, cors } = deps;
  const auth = await deps.verifyAuth(request, env);
  if (!auth) return { error: json({ error: 'Unauthorized' }, 401, cors) };
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return { error: json({ error: 'Simulation not found' }, 404, cors) };
  const sim = JSON.parse(raw);
  if (sim.author_id !== auth) return { error: json({ error: 'Not your simulation' }, 403, cors) };
  return { auth, sim };
}

// The listing as serving sees it (its legal lock read from R2), for the public routes.
async function servingListing(env, simId) {
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return null;
  return simWithLegalLock(env, JSON.parse(raw));
}

// The review is started without holding the response: in the Worker on waitUntil, elsewhere awaited.
async function startReview(env, simId, ctx, deps) {
  const work = stepLinkReview(env, simId, deps, { submitOnly: true, budget: { links: 17, images: 0 } }).catch(e => console.error('page: review start failed', simId, e?.message));
  if (ctx && typeof ctx.waitUntil === 'function') ctx.waitUntil(work);
  else await work;
}

/// A Response for a sim page route, or null when the path is not one. `deps`: verifyAuth, json, cors, sniffFileType
/// (src/index.js), and policyText for the review.
export async function handlePageRoute(request, url, env, ctx, deps) {
  const { json, cors } = deps;
  const path = url.pathname;
  const m = request.method;
  let match;

  if ((match = path.match(/^\/api\/simulations\/([a-f0-9-]+)\/page$/))) {
    const simId = match[1];
    if (m === 'PUT') {
      const owned = await ownedListing(request, simId, env, deps);
      if (owned.error) return owned.error;
      // The page is stored content like the package: not written while the listing's content is locked.
      if (await contentLockedFor(env, owned.sim, owned.auth)) return inReview(json, cors);
      const text = await request.text();
      if (new TextEncoder().encode(text).length > PAGE_SPEC_MAX_BYTES) return json({ error: `The page is too large (at most ${PAGE_SPEC_MAX_BYTES} bytes).`, rule: 'size' }, 413, cors);
      let raw;
      try { raw = JSON.parse(text); } catch { return json({ error: 'The page is not valid JSON.', rule: 'json' }, 400, cors); }
      const checked = validatePageSpec(raw);
      if (!checked.ok) return json({ error: checked.error, rule: checked.rule }, 400, cors);
      const draft = { spec: checked.spec, notes: checked.notes, put_at: new Date().toISOString(), author_id: owned.auth };
      await env.SOCIAL.put(pageDraftKey(simId), JSON.stringify(draft));
      // New or changed links are pending from now; links still there keep their review.
      const state = await readLinkState(env, simId);
      await syncLinkEntries(state, owned.sim, draft);
      await writeLinkState(env, simId, state);
      await queueLinkReview(env, simId);
      await startReview(env, simId, ctx, deps);
      const after = await readLinkState(env, simId);
      return json({ ok: true, theme_notes: checked.notes, review: reviewView(after, draft) }, 200, cors);
    }
    if (m === 'GET') {
      const listing = await servingListing(env, simId);
      const draft = listing && !pageWithheld(listing) ? await readPageDraft(env, simId) : null;
      if (!draft?.spec) return json({ error: 'page_not_found' }, 404, { ...cors, 'Cache-Control': 'no-store' });
      const state = await readLinkState(env, simId);
      return json(publicPageView(simId, draft, state, listing), 200, { ...cors, 'Cache-Control': 'public, max-age=60' });
    }
    return null;
  }

  if ((match = path.match(/^\/api\/simulations\/([a-f0-9-]+)\/page\/review$/)) && m === 'GET') {
    const owned = await ownedListing(request, match[1], env, deps);
    if (owned.error) return owned.error;
    const [state, draft] = [await readLinkState(env, match[1]), await readPageDraft(env, match[1])];
    return json(reviewView(state, draft), 200, { ...cors, 'Cache-Control': 'no-store' });
  }

  if ((match = path.match(/^\/api\/simulations\/([a-f0-9-]+)\/page\/image\/([a-z0-9-]+)$/))) {
    const [, simId, slot] = match;
    if (m === 'PUT') {
      if (!PAGE_IMAGE_SLOTS.includes(slot)) return json({ error: `The slot must be one of ${PAGE_IMAGE_SLOTS.join(', ')}.` }, 400, cors);
      const owned = await ownedListing(request, simId, env, deps);
      if (owned.error) return owned.error;
      if (await contentLockedFor(env, owned.sim, owned.auth)) return inReview(json, cors);
      const declared = Number(request.headers.get('Content-Length'));
      if (Number.isFinite(declared) && declared > PAGE_IMAGE_MAX_BYTES) return json({ error: 'The image is larger than 2 MB.' }, 413, cors);
      const body = await request.arrayBuffer();
      if (!body.byteLength) return json({ error: 'Empty body' }, 400, cors);
      if (body.byteLength > PAGE_IMAGE_MAX_BYTES) return json({ error: 'The image is larger than 2 MB.' }, 413, cors);
      // The type is taken from the bytes, never the header: PNG, JPEG or WebP only.
      const type = deps.sniffFileType(body);
      if (!['image/png', 'image/jpeg', 'image/webp'].includes(type)) return json({ error: 'The image must be a PNG, JPEG or WebP picture.' }, 415, cors);
      // Metadata stripped, animation and transparency refused, dimensions bounded: as every other user image.
      const prepared = await prepareUserImage(body);
      if (!prepared.ok) return json({ error: prepared.error }, prepared.status, cors);
      await env.SCENES.put(pageImageR2Key(simId, slot), prepared.bytes, { httpMetadata: { contentType: type }, sha256: prepared.sha256 });
      const state = await readLinkState(env, simId);
      state.images[slot] = { status: 'pending', code: 'pending', reason: 'Waiting for the image review.', sha256: prepared.sha256, content_type: type, bytes: prepared.bytes.byteLength, uploaded_at: new Date().toISOString(), next_at: null };
      await writeLinkState(env, simId, state);
      await queueLinkReview(env, simId);
      return json({ ok: true, slot, status: 'pending' }, 200, cors);
    }
    if (m === 'GET') {
      const notFound = () => json({ error: 'Image not found' }, 404, { ...cors, 'Cache-Control': 'no-store' });
      if (!PAGE_IMAGE_SLOTS.includes(slot)) return notFound();
      const listing = await servingListing(env, simId);
      if (pageWithheld(listing)) return notFound();
      const [state, draft] = [await readLinkState(env, simId), await readPageDraft(env, simId)];
      const img = state.images[slot];
      const named = [draft?.spec?.images?.logo, draft?.spec?.images?.banner, ...(draft?.spec?.images?.gallery || [])].includes(slot);
      if (!named || img?.status !== 'approved') return notFound();
      const object = await env.SCENES.get(pageImageR2Key(simId, slot));
      if (!object) return notFound();
      // Only the bytes that were reviewed: an object replaced since serves nothing until its own review passes.
      const sum = object.checksums?.sha256 ? [...new Uint8Array(object.checksums.sha256)].map(b => b.toString(16).padStart(2, '0')).join('') : null;
      if (sum !== img.sha256) { await object.body?.cancel?.(); return notFound(); }
      return new Response(object.body, { headers: { ...cors, ...USER_IMAGE_HEADERS, 'Content-Type': img.content_type, 'Cache-Control': 'public, max-age=300' } });
    }
    return null;
  }
  return null;
}
