// The sim page: PageSpec v1 validation exactly per docs/design/SIM_PAGE_THEME_AND_LINKS.md section 2, the presets of section
// 11, and the five routes of section 3 (owner auth, image upload, the public view that shows only what the review approved,
// withheld with the listing).
import test from 'node:test';
import assert from 'node:assert/strict';
import { deflateSync } from 'node:zlib';
import { fakeKv, fakeBucket } from './helpers/load_worker.mjs';
import {
  validatePageSpec, contrastRatio, cleanLabel, handlePageRoute, publicPageView, PAGE_PRESETS, PAGE_COLOR_KEYS, LINK_CAPS, LINKS_MAX,
  PAGE_WITHHELD_STATUSES, pageImagePath,
} from '../src/page.mjs';
import { readLinkState, writeLinkState, stepLinkReview, pageImageR2Key, linkQueueKey } from '../src/link_review.mjs';
import { GUARDIAN_POLICY_TEXT } from '../src/generated/policy_text.mjs';

const SIM = 'aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee';
const base = () => ({ schema: 1, theme: { preset: 'studio', colors: {}, font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 }, images: { logo: 'logo', banner: 'banner', gallery: ['gallery-0', 'gallery-1'] }, links: [] });
const withTheme = over => ({ ...base(), theme: { ...base().theme, ...over } });
const link = (kind, url, label = 'A link') => ({ kind, url, label });
const refused = (spec, rule) => { const r = validatePageSpec(spec); assert.equal(r.ok, false, `${rule}: ${JSON.stringify(spec).slice(0, 200)}`); assert.equal(r.rule, rule); return r; };

// ── PageSpec v1 ──────────────────────────────────────────────────────────────

test('the six presets are the exact palettes of section 11, each readable at 4.5:1', () => {
  assert.deepEqual(Object.keys(PAGE_PRESETS), ['studio', 'daylight', 'blueprint', 'laboratory', 'arcade', 'paper']);
  assert.deepEqual(PAGE_PRESETS.studio, { primary: '#4F8CFF', accent: '#FFB547', background: '#0E1116', surface: '#171B22', text: '#E8ECF2' });
  assert.deepEqual(PAGE_PRESETS.paper, { primary: '#8A3B12', accent: '#2F6B4F', background: '#FAF6EE', surface: '#F0E8D6', text: '#2B2620' });
  for (const [name, c] of Object.entries(PAGE_PRESETS)) {
    assert.deepEqual(Object.keys(c), PAGE_COLOR_KEYS, name);
    for (const k of ['text', 'primary', 'accent']) assert.ok(contrastRatio(c[k], c.background) >= 4.5, `${name} ${k}`);
    assert.ok(contrastRatio(c.text, c.surface) >= 4.5, `${name} text on surface`);
  }
  assert.equal(contrastRatio('#E8ECF2', '#0E1116').toFixed(2), '15.95');
  assert.equal(contrastRatio('#FFFFFF', '#000000'), 21);
});

test('a valid spec is stored resolved: colours upper case over the preset, unknown keys dropped, URLs normalised', () => {
  const spec = { ...base(), extra: 'x', theme: { ...base().theme, preset: 'daylight', colors: { primary: '#1a4fb0' }, sneaky: 'url(x)' }, links: [link('website', 'https://Example-Robotics.com', 'Our lab')] };
  const r = validatePageSpec(spec);
  assert.equal(r.ok, true, r.error);
  assert.deepEqual(r.spec.theme.colors, { ...PAGE_PRESETS.daylight, primary: '#1A4FB0' });
  assert.ok(!('extra' in r.spec) && !('sneaky' in r.spec.theme));
  assert.deepEqual(r.spec.links, [{ kind: 'website', url: 'https://example-robotics.com/', label: 'Our lab' }]);
  assert.deepEqual(r.notes, []);
  // What Studio sends for unset fields is the defaults; a missing field reads the same way.
  assert.deepEqual(validatePageSpec({ schema: 1 }).spec.theme, { preset: 'studio', colors: PAGE_PRESETS.studio, font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 });
});

test('the contrast rule: text under 4.5:1 falls back to the preset text, then to the preset background and surface, and says so', () => {
  const one = validatePageSpec(withTheme({ colors: { text: '#20242B' } }));
  assert.equal(one.ok, true);
  assert.equal(one.spec.theme.colors.text, PAGE_PRESETS.studio.text);
  assert.match(one.notes[0], /4\.5:1.*studio preset's text colour is used/);
  const two = validatePageSpec(withTheme({ colors: { background: '#F0F0F0', surface: '#FFFFFF', text: '#EEEEEE' } }));
  assert.deepEqual([two.spec.theme.colors.text, two.spec.theme.colors.background, two.spec.theme.colors.surface], [PAGE_PRESETS.studio.text, PAGE_PRESETS.studio.background, PAGE_PRESETS.studio.surface]);
  assert.match(two.notes[0], /text, background and surface/);
  const fine = validatePageSpec(withTheme({ colors: { background: '#FFFFFF', surface: '#F5F5F5', text: '#111111' } }));
  assert.deepEqual([fine.spec.theme.colors.text, fine.notes], ['#111111', []]);
});

test('every theme rule refuses the whole spec, naming the rule', () => {
  refused({ ...base(), schema: 2 }, 'schema');
  refused('nope', 'spec');
  refused(withTheme({ preset: 'neon' }), 'theme.preset');
  refused(withTheme({ preset: '__proto__' }), 'theme.preset');
  refused(withTheme({ colors: { primary: 'red' } }), 'theme.colors.primary');
  refused(withTheme({ colors: { text: '#FFF' } }), 'theme.colors.text');
  refused(withTheme({ colors: { accent: 'url(javascript:x)' } }), 'theme.colors.accent');
  refused(withTheme({ colors: [] }), 'theme.colors');
  refused(withTheme({ font: 'comic-sans' }), 'theme.font');
  refused(withTheme({ layout: 'grid' }), 'theme.layout');
  refused(withTheme({ motion: 'wild' }), 'theme.motion');
  for (const v of [2, 31, 6.5, '6']) refused(withTheme({ slideshow_interval_s: v }), 'theme.slideshow_interval_s');
  for (const v of [3, 30]) assert.equal(validatePageSpec(withTheme({ slideshow_interval_s: v })).ok, true);
  for (const v of [-1, 2001, 1.5]) refused(withTheme({ banner_fade_ms: v }), 'theme.banner_fade_ms');
  for (const v of [0, 2000]) assert.equal(validatePageSpec(withTheme({ banner_fade_ms: v })).ok, true);
  for (const f of ['inter', 'space-grotesk', 'ibm-plex-sans', 'jetbrains-mono', 'merriweather']) assert.equal(validatePageSpec(withTheme({ font: f })).ok, true, f);
});

test('images are slot names only', () => {
  refused({ ...base(), images: { logo: 'https://example.com/logo.png' } }, 'images.logo');
  refused({ ...base(), images: { banner: 'logo' } }, 'images.banner');
  refused({ ...base(), images: { gallery: ['gallery-6'] } }, 'images.gallery');
  refused({ ...base(), images: { gallery: ['gallery-0', 'gallery-0'] } }, 'images.gallery');
  refused({ ...base(), images: { gallery: ['gallery-0', 'gallery-1', 'gallery-2', 'gallery-3', 'gallery-4', 'gallery-5', 'gallery-0'] } }, 'images.gallery');
  assert.deepEqual(validatePageSpec({ ...base(), images: { gallery: [] } }).spec.images, { logo: null, banner: null, gallery: [] });
});

test('links: the kinds and their caps, 17 in all, and the URL rules (https, no IP literal, port or credentials, a public host)', () => {
  const many = (kind, n) => Array.from({ length: n }, (_, i) => link(kind, `https://site${i}.example-${kind}.com/`));
  for (const [kind, cap] of Object.entries(LINK_CAPS)) {
    assert.equal(validatePageSpec({ ...base(), links: many(kind, cap) }).ok, true, kind);
    refused({ ...base(), links: many(kind, cap + 1) }, 'links');
  }
  assert.equal(validatePageSpec({ ...base(), links: [...many('website', 1), ...many('social', 8), ...many('shop', 4), ...many('other', 4)] }).spec.links.length, LINKS_MAX);
  refused({ ...base(), links: [link('store', 'https://example-shop.com/')] }, 'links[0].kind');
  for (const url of ['http://example-shop.com/', 'https://203.0.113.7/', 'https://[2001:db8::1]/', 'https://example-shop.com:8443/', 'https://user:pass@example-shop.com/',
    'https://user@example-shop.com/', 'https://localhost/', 'https://shop.internal/', 'https://api.eustress.dev/', 'https://127.0.0.1.nip.io/', 'example-shop.com',
    'ftp://example-shop.com/', `https://example-shop.com/${'a'.repeat(490)}`, 'https://0x7f.0x0.0x0.0x1/', 42]) {
    refused({ ...base(), links: [link('shop', url)] }, 'links[0].url');
  }
});

test('labels: plain text of 1 to 40 characters with no URL and no control characters', () => {
  assert.equal(cleanLabel('  Buy the kit  '), 'Buy the kit');
  assert.equal(cleanLabel('x'.repeat(40)), 'x'.repeat(40));
  for (const bad of ['', '   ', 'x'.repeat(41), 'Visit https://evil.example.com', 'www.evil.com', 'evil.com', 'tab\there', 'line\nbreak', 'bidi ‮evil', 'zero​width', null, 7]) {
    assert.equal(cleanLabel(bad), null, JSON.stringify(bad));
  }
  refused({ ...base(), links: [link('social', 'https://youtube.com/@example', 'go to example.com')] }, 'links[0].label');
});

// ── the routes ───────────────────────────────────────────────────────────────

function png(w = 4, h = 4) {
  const table = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = buf => { let c = 0xffffffff; for (const b of buf) c = table[(c ^ b) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => { const t = Buffer.from(type, 'ascii'); const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const c = Buffer.alloc(4); c.writeUInt32BE(crc(Buffer.concat([t, data]))); return Buffer.concat([len, t, data, c]); };
  const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h, 0x7f); for (let y = 0; y < h; y++) raw[y * (w * 3 + 1)] = 0;
  return new Uint8Array(Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0))]));
}
// src/index.js sniffFileType, for the routes' deps.
function sniffFileType(buf) {
  const b = new Uint8Array(buf.slice(0, 12));
  if (b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff) return 'image/jpeg';
  if (b[0] === 0x89 && b[1] === 0x50 && b[2] === 0x4e && b[3] === 0x47) return 'image/png';
  if (b[0] === 0x25 && b[1] === 0x50 && b[2] === 0x44 && b[3] === 0x46) return 'application/pdf';
  if (b[0] === 0x52 && b[1] === 0x49 && b[2] === 0x46 && b[3] === 0x46 && b[8] === 0x57 && b[9] === 0x45 && b[10] === 0x42 && b[11] === 0x50) return 'image/webp';
  return null;
}
const json = (data, status = 200, headers = {}) => new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json', ...headers } });
const deps = { verifyAuth: async req => ({ 'Bearer owner': 'owner-1', 'Bearer other': 'other-1' })[req.headers.get('Authorization')] || null, json, cors: {}, sniffFileType, policyText: GUARDIAN_POLICY_TEXT };

function setup(moderation = { status: 'approved', rating: 'teen_13' }) {
  const env = { SOCIAL: fakeKv(), SCENES: fakeBucket() };
  env.SOCIAL.store.set(`sim:${SIM}`, JSON.stringify({ id: SIM, author_id: 'owner-1', name: 'Arm', description: 'Arm', moderation, is_public: true }));
  return env;
}
const call = async (env, method, path, { who = 'owner', body = null } = {}) => {
  const url = new URL(`https://api.eustress.dev/api/simulations/${SIM}${path}`);
  const req = new Request(url, { method, headers: who ? { Authorization: `Bearer ${who}` } : {}, ...(body !== null ? { body } : {}) });
  return handlePageRoute(req, url, env, null, deps);
};

test('PUT /page: owner only, refused whole with the rule named, stored as the draft and queued for review', async () => {
  const env = setup();
  const spec = { ...base(), links: [link('shop', 'https://shop.example-robotics.com/kits', 'Buy the kit')] };
  assert.equal((await call(env, 'PUT', '/page', { who: null, body: JSON.stringify(spec) })).status, 401);
  assert.equal((await call(env, 'PUT', '/page', { who: 'other', body: JSON.stringify(spec) })).status, 403);
  const bad = await call(env, 'PUT', '/page', { body: JSON.stringify({ ...spec, theme: { preset: 'neon' } }) });
  assert.equal(bad.status, 400);
  assert.equal((await bad.json()).rule, 'theme.preset');
  assert.equal(env.SOCIAL.store.get(`simpage:draft:${SIM}`), undefined, 'nothing half-published');
  assert.equal((await call(env, 'PUT', '/page', { body: 'not json' })).status, 400);
  const ok = await call(env, 'PUT', '/page', { body: JSON.stringify(spec) });
  assert.equal(ok.status, 200);
  const out = await ok.json();
  // The review in the shape Studio reads; with no scanner configured the link waits.
  assert.deepEqual(out.review, { links: [{ label: 'Buy the kit', url: 'https://shop.example-robotics.com/kits', kind: 'shop', status: 'pending', reason: 'link check unavailable' }], images: [] });
  assert.ok(env.SOCIAL.store.get(`simpage:draft:${SIM}`));
  assert.ok(env.SOCIAL.store.get(linkQueueKey(SIM)));
});

test('GET /page: the bare object the website reads, with only approved links and images, as image paths', async () => {
  const env = setup();
  const spec = { ...base(), links: [link('website', 'https://example-robotics.com/', 'Our lab'), link('shop', 'https://shop.example-robotics.com/kits', 'Buy the kit')] };
  await call(env, 'PUT', '/page', { body: JSON.stringify(spec) });
  const state = await readLinkState(env, SIM);
  for (const e of Object.values(state.links)) if (e.url === 'https://example-robotics.com/') { e.status = 'approved'; e.verdict = { kind: 'business' }; }
  state.images.logo = { status: 'approved' };
  state.images['gallery-1'] = { status: 'approved' };
  state.images.banner = { status: 'pending' };
  await writeLinkState(env, SIM, state);
  const res = await call(env, 'GET', '/page', { who: null });
  assert.equal(res.status, 200);
  const view = await res.json();
  assert.deepEqual(Object.keys(view), ['theme', 'images', 'links']);
  assert.deepEqual(view.theme, { preset: 'studio', colors: PAGE_PRESETS.studio, font: 'inter', layout: 'classic', motion: 'subtle', slideshow_interval_s: 6, banner_fade_ms: 600 });
  assert.deepEqual(view.images, { logo: `/api/simulations/${SIM}/page/image/logo`, banner: null, gallery: [`/api/simulations/${SIM}/page/image/gallery-1`] });
  assert.deepEqual(view.links, [{ kind: 'website', url: 'https://example-robotics.com/', label: 'Our lab' }]);
  assert.equal(pageImagePath(SIM, 'logo'), `/api/simulations/${SIM}/page/image/logo`);
  // Section 7 at read time: a listing that turns child-directed shows no link, at once.
  const sim = JSON.parse(env.SOCIAL.store.get(`sim:${SIM}`));
  assert.deepEqual(publicPageView(SIM, JSON.parse(env.SOCIAL.store.get(`simpage:draft:${SIM}`)), state, { ...sim, moderation: { ...sim.moderation, child_directed: true } }).links, []);
});

test('GET /page is withheld (404) under the same listing statuses as the website manifest, and when there is no page', async () => {
  assert.deepEqual([...PAGE_WITHHELD_STATUSES], ['quarantined', 'held', 'rejected', 'changes_requested', 'appealed']);
  for (const status of PAGE_WITHHELD_STATUSES) {
    const env = setup({ status });
    env.SOCIAL.store.set(`simpage:draft:${SIM}`, JSON.stringify({ spec: validatePageSpec(base()).spec }));
    assert.equal((await call(env, 'GET', '/page', { who: null })).status, 404, status);
  }
  assert.equal((await call(setup(), 'GET', '/page', { who: null })).status, 404);
});

test('PUT /page/image/{slot}: PNG, JPEG or WebP sniffed from the bytes, at most 2 MB, stored at universes/{id}/page/{slot}, pending; GET serves it only once approved', async () => {
  const env = setup();
  await call(env, 'PUT', '/page', { body: JSON.stringify(base()) });
  assert.equal((await call(env, 'PUT', '/page/image/logo', { who: 'other', body: png() })).status, 403);
  assert.equal((await call(env, 'PUT', '/page/image/hero', { body: png() })).status, 400);
  assert.equal((await call(env, 'PUT', '/page/image/logo', { body: new TextEncoder().encode('<svg onload=alert(1)></svg>') })).status, 415);
  assert.equal((await call(env, 'PUT', '/page/image/logo', { body: new TextEncoder().encode('%PDF-1.4 not an image') })).status, 415);
  const big = new Uint8Array(2 * 1024 * 1024 + 1); big.set(png().subarray(0, 16));
  assert.equal((await call(env, 'PUT', '/page/image/logo', { body: big })).status, 413);
  const put = await call(env, 'PUT', '/page/image/logo', { body: png() });
  assert.equal(put.status, 200, await put.clone().text());
  assert.deepEqual(await put.json(), { ok: true, slot: 'logo', status: 'pending' });
  assert.ok(await env.SCENES.head(pageImageR2Key(SIM, 'logo')));
  assert.equal(pageImageR2Key(SIM, 'logo'), `universes/${SIM}/page/logo`);
  assert.equal((await call(env, 'GET', '/page/image/logo', { who: null })).status, 404, 'pending is not served');
  // The image judge (the whole Code) approves it on the next step.
  const asked = [];
  const anthropicFetch = async (url, init) => { asked.push(new URL(String(url)).host); const body = JSON.parse(init.body); assert.equal(body.messages[0].content[0].type, 'image'); return sse(JSON.stringify({ allowed: true, articles: [], reason: 'A plain logo.' })); };
  await stepLinkReview({ ...env, ANTHROPIC_API_KEY: 'sk-ant-test-not-real' }, SIM, { anthropicFetch, policyText: GUARDIAN_POLICY_TEXT });
  assert.deepEqual(asked, ['api.anthropic.com']);
  const served = await call(env, 'GET', '/page/image/logo', { who: null });
  assert.equal(served.status, 200);
  assert.equal(served.headers.get('Content-Type'), 'image/png');
  assert.equal(served.headers.get('X-Content-Type-Options'), 'nosniff');
  // Replaced bytes are not the reviewed bytes: not served until their own review.
  await env.SCENES.put(pageImageR2Key(SIM, 'logo'), png(5, 5), { sha256: '00'.repeat(32) });
  assert.equal((await call(env, 'GET', '/page/image/logo', { who: null })).status, 404);
  const review = await (await call(env, 'GET', '/page/review')).json();
  assert.deepEqual(review.images, [{ slot: 'logo', status: 'approved', reason: 'Approved.' }]);
  assert.equal((await call(env, 'GET', '/page/review', { who: 'other' })).status, 403);
});

test('an image the judge finds breaking the Code is removed and the author is told', async () => {
  const env = { ...setup(), ANTHROPIC_API_KEY: 'sk-ant-test-not-real' };
  await call(env, 'PUT', '/page', { body: JSON.stringify(base()) });
  await call(env, 'PUT', '/page/image/banner', { body: png() });
  await stepLinkReview(env, SIM, { anthropicFetch: async () => sse(JSON.stringify({ allowed: false, articles: [{ id: 'B4', evidence: 'A brainrot meme character fills the banner.' }], reason: 'Meme bait.' })), policyText: GUARDIAN_POLICY_TEXT });
  const review = await (await call(env, 'GET', '/page/review')).json();
  assert.equal(review.images[0].status, 'removed');
  assert.match(review.images[0].reason, /B4/);
  assert.ok([...env.SOCIAL.store.keys()].some(k => k.startsWith('notif:owner-1:')));
});

function sse(text) {
  const events = [
    ['message_start', { type: 'message_start', message: { id: 'msg_1', type: 'message', role: 'assistant', model: 'claude-haiku-5-5', content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 9, output_tokens: 1 } } }],
    ['content_block_start', { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } }],
    ['content_block_delta', { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } }],
    ['content_block_stop', { type: 'content_block_stop', index: 0 }],
    ['message_delta', { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 30 } }],
    ['message_stop', { type: 'message_stop' }],
  ];
  return new Response(events.map(([e, d]) => `event: ${e}\ndata: ${JSON.stringify(d)}\n\n`).join(''), { status: 200, headers: { 'content-type': 'text/event-stream' } });
}
