// Solo play passes (src/play_pass.mjs; McKale, 2026-11-13: "Studio is not the source of truth. I am signed in on staging
// website."). The website's Play page mints a one-time play ticket for a listing; the Player redeems it once for a pass that
// reads that one listing as the website's account, and sends `Authorization: PlayPass <pass>` with every request. Checked
// against the real Worker, with in-memory storage and the real LiveHost Durable Object, which spends each ticket once.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, fakeDoStorage, ctx, JWT_SECRET } from './helpers/load_worker.mjs';
import { LiveHost, MAX_PLAY_TICKETS_PER_ACCOUNT } from '../src/live.mjs';
import { manifestKey, chunkKey } from '../src/world.mjs';
import { makeIdentityTicket, isAudience, TICKET_TTL_S } from '../src/identity.mjs';
import { makePlayPass, inspectPlayPass, playAudience, PLAY_PASS_TTL_S, PLAY_TICKET_TTL_S } from '../src/play_pass.mjs';
import { routePolicy, PUBLIC, SESSION } from '../src/route_policy.mjs';
import { adultContentKey, SIGN_IN_REQUIRED, ADULT_CONTENT_OFF } from '../src/adult_content.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const PLAYER = '22222222-2222-4222-8222-222222222222'; // signed in on the website; Adult content off
const tokens = { author: mintToken(AUTHOR), player: mintToken(PLAYER) };

const TEEN = 'bbbbbbbb-0000-4000-8000-000000000001'; // Box Head's band: above what a signed-out visitor sees
const TEEN_B = 'bbbbbbbb-0000-4000-8000-000000000002';
const KIDS = 'bbbbbbbb-0000-4000-8000-000000000003';
const ADULT = 'bbbbbbbb-0000-4000-8000-000000000004';
const HELD = 'bbbbbbbb-0000-4000-8000-000000000005'; // in review: unlisted
const PAK = 'bbbbbbbb-0000-4000-8000-000000000006'; // an older .pak publish
const MISSING = 'bbbbbbbb-0000-4000-8000-0000000000ff';

const H = 'c'.repeat(64);
const WORLD_SHA = 'f'.repeat(64);
const ORIGIN = 'https://api-staging.eustress.dev';

async function listing(env, id, rating, { status = 'approved', pak = false } = {}) {
  const sim = {
    id, name: `World ${id.slice(-1)}`, description: 'a place', genre: 'all', author_id: AUTHOR, author_name: 'maker', is_public: true,
    max_players: 8, tags: ['shared'], play_count: 0, favorite_count: 0, version: 1,
    published_at: '2026-03-01T00:00:00.000Z', updated_at: '2026-03-01T00:00:00.000Z',
    moderation: { status, rating },
  };
  if (pak) {
    sim.format = 'pak';
    sim.r2_key = `universes/${id}/universe.pak`;
    await env.SCENES.put(sim.r2_key, 'the world');
  } else {
    sim.format = 'echk';
    sim.world = { manifest_key: manifestKey(id, WORLD_SHA), manifest_sha256: WORLD_SHA };
    sim.r2_key = sim.world.manifest_key;
    await env.SCENES.put(sim.world.manifest_key, JSON.stringify({ format: 'echk', spaces: [{ name: 'Main', chunks: [{ blake3: H }] }], assets: [] }));
    await env.SCENES.put(chunkKey(id, H), 'ECHK chunk bytes');
  }
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
}

async function environment() {
  const env = { ...fakeEnv(), KYC_STATUS: fakeKv() };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  // Bound in both deployments; a test that wants limits replaces it.
  env.AUTH_RATE_LIMITER = { limit: async () => ({ success: true }) };
  for (const [id, username] of [[AUTHOR, 'maker'], [PLAYER, 'mckale']]) await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username }));
  await listing(env, TEEN, 'teen_13');
  await listing(env, TEEN_B, 'teen_13');
  await listing(env, KIDS, 'all_ages');
  await listing(env, ADULT, 'adult_18');
  await listing(env, HELD, 'teen_13', { status: 'pending' });
  await listing(env, PAK, 'teen_13', { pak: true });
  return env;
}

async function call(env, method, path, { who, pass, auth, body } = {}) {
  const h = { 'CF-Connecting-IP': '203.0.113.9' };
  if (who) h.Authorization = `Bearer ${tokens[who] || who}`;
  if (pass !== undefined) h.Authorization = `PlayPass ${pass}`;
  if (auth !== undefined) h.Authorization = auth;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`${ORIGIN}${path}`, { method, headers: h, body: body === undefined ? undefined : (typeof body === 'string' ? body : JSON.stringify(body)) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, headers: res.headers, text, body: parsed };
}

const mint = (env, simId, who = 'player') => call(env, 'POST', `/api/simulations/${simId}/play-ticket`, { who });
const redeem = (env, simId, ticket) => call(env, 'POST', `/api/simulations/${simId}/play-pass`, { body: { ticket } });

/// A pass for `simId` the way the Player gets one: mint with the website's session, then redeem.
async function passFor(env, simId, who = 'player') {
  const minted = await mint(env, simId, who);
  assert.equal(minted.status, 200, minted.text);
  const redeemed = await redeem(env, simId, minted.body.ticket);
  assert.equal(redeemed.status, 200, redeemed.text);
  return redeemed.body.pass;
}

/// The world surfaces the Player reads.
const READS = (simId) => [`/api/simulations/${simId}`, `/api/simulations/${simId}/world/manifest`, `/api/simulations/${simId}/world/chunks/${H}`];

function signInRequired(r, what) {
  assert.equal(r.status, 403, `${what}: ${r.text}`);
  assert.deepEqual(r.body, { error: SIGN_IN_REQUIRED, code: 'sign_in_required', signed_in: false }, what);
}

// ── mint ─────────────────────────────────────────────────────────────────────

test('minting a play ticket needs the website session: none, a pass or a garbage header is refused', async () => {
  const env = await environment();
  const none = await call(env, 'POST', `/api/simulations/${TEEN}/play-ticket`);
  assert.equal(none.status, 401, none.text);
  assert.equal(none.body.code, 'unauthenticated');
  assert.equal(none.body.ticket, undefined);
  // A pass is not a session: it can never mint another ticket, so a pass cannot be stretched past its hour.
  const pass = await passFor(env, TEEN);
  const chained = await call(env, 'POST', `/api/simulations/${TEEN}/play-ticket`, { pass });
  assert.equal(chained.status, 401, chained.text);
  for (const auth of ['Bearer not-a-token', `Bearer ${pass}`, 'PlayPass', '']) {
    const r = await call(env, 'POST', `/api/simulations/${TEEN}/play-ticket`, { auth });
    assert.equal(r.status, 401, `${auth}: ${r.text}`);
  }
  // GET is not a mint.
  assert.equal((await call(env, 'GET', `/api/simulations/${TEEN}/play-ticket`, { who: 'player' })).status, 405);
});

test('a minted ticket names the account and this listing, lasts at most two minutes, and carries no username', async () => {
  const env = await environment();
  const r = await mint(env, TEEN);
  assert.equal(r.status, 200, r.text);
  assert.equal(r.headers.get('Cache-Control'), 'private, no-store');
  assert.match(r.body.ticket, /^eit1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]{43}$/);
  const payload = JSON.parse(Buffer.from(r.body.ticket.split('.')[1], 'base64url').toString());
  assert.equal(payload.a, PLAYER);
  assert.equal(payload.h, playAudience(TEEN));
  assert.equal(payload.u, '');
  const life = payload.e - Math.floor(Date.now() / 1000);
  assert.ok(PLAY_TICKET_TTL_S === 120 && life <= PLAY_TICKET_TTL_S && life >= PLAY_TICKET_TTL_S - 5, `a play ticket lives ${life} s`);
  assert.equal(r.body.expires, payload.e);
});

test('the mint refuses what the listing GET refuses, in the GET\'s own words', async () => {
  const env = await environment();
  for (const simId of [MISSING, HELD]) {
    const get = await call(env, 'GET', `/api/simulations/${simId}`, { who: 'player' });
    const minted = await mint(env, simId);
    assert.equal(get.status, 404, get.text);
    assert.deepEqual([minted.status, minted.body], [get.status, get.body], simId);
  }
  // The author may play their own listing in review, as the GET shows it to them.
  assert.equal((await mint(env, HELD, 'author')).status, 200);
  // Above the account's band: the rating gate's refusal, the same as the GET's.
  const get = await call(env, 'GET', `/api/simulations/${ADULT}`, { who: 'player' });
  const minted = await mint(env, ADULT);
  assert.deepEqual([minted.status, minted.body], [403, { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: true }]);
  assert.deepEqual([minted.status, minted.body], [get.status, get.body]);
});

// ── redeem and read ──────────────────────────────────────────────────────────

test('mint, then redeem, then a teen-rated listing, its manifest and its chunk are readable with the pass; signed out they are not', async () => {
  const env = await environment();
  for (const path of READS(TEEN)) signInRequired(await call(env, 'GET', path), `signed out ${path}`);
  const minted = await mint(env, TEEN);
  const redeemed = await redeem(env, TEEN, minted.body.ticket);
  assert.equal(redeemed.status, 200, redeemed.text);
  assert.equal(redeemed.headers.get('Cache-Control'), 'private, no-store');
  assert.match(redeemed.body.pass, /^epp1\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]{43}$/);
  const now = Math.floor(Date.now() / 1000);
  assert.ok(redeemed.body.expires > now && redeemed.body.expires <= now + PLAY_PASS_TTL_S, String(redeemed.body.expires));
  const { pass } = redeemed.body;
  const manifest = await call(env, 'GET', `/api/simulations/${TEEN}/world/manifest`, { pass });
  assert.equal(manifest.status, 200, manifest.text);
  assert.equal(manifest.body.format, 'echk');
  const chunk = await call(env, 'GET', `/api/simulations/${TEEN}/world/chunks/${H}`, { pass });
  assert.equal(chunk.status, 200, chunk.text);
  assert.equal(chunk.text, 'ECHK chunk bytes');
  const listed = await call(env, 'GET', `/api/simulations/${TEEN}`, { pass });
  assert.equal(listed.status, 200, listed.text);
  assert.equal(listed.body.id, TEEN);
  // The older .pak download takes it too.
  const pakPass = await passFor(env, PAK);
  const pak = await call(env, 'GET', `/api/simulations/${PAK}/download`, { pass: pakPass });
  assert.equal(pak.status, 200, pak.text);
  assert.equal(pak.text, 'the world');
  signInRequired(await call(env, 'GET', `/api/simulations/${PAK}/download`), 'signed out .pak');
});

test('a pass is judged as its account: an account with Adult content off is refused an adult listing, and one with it on is not', async () => {
  const env = await environment();
  // A pass made for the adult listing directly (the mint would already refuse this account).
  const { pass } = await makePlayPass(env, { accountId: PLAYER, simId: ADULT, issued: Math.floor(Date.now() / 1000) });
  const off = await call(env, 'GET', `/api/simulations/${ADULT}/world/manifest`, { pass });
  assert.deepEqual([off.status, off.body], [403, { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in: true }]);
  // The author always plays their own.
  const authorPass = await passFor(env, ADULT, 'author');
  assert.equal((await call(env, 'GET', `/api/simulations/${ADULT}/world/manifest`, { pass: authorPass })).status, 200);
  // The account's own setting decides, read at each request.
  await env.USERS.put(adultContentKey(PLAYER), '1');
  assert.equal((await call(env, 'GET', `/api/simulations/${ADULT}/world/manifest`, { pass })).status, 200);
});

test('a second redeem of the same ticket is refused', async () => {
  const env = await environment();
  const minted = await mint(env, TEEN);
  assert.equal((await redeem(env, TEEN, minted.body.ticket)).status, 200);
  const again = await redeem(env, TEEN, minted.body.ticket);
  assert.equal(again.status, 403, again.text);
  assert.equal(again.body.code, 'used');
  assert.equal(again.body.pass, undefined);
  // Its id is kept in the listing's own play object, until the ticket would have expired.
  const store = env.LIVE_HOSTS.instances.get(`play:${TEEN}`).storage;
  const kept = [...store.map.keys()].filter((k) => k.startsWith('ptk:'));
  assert.equal(kept.length, 1);
  assert.equal(store.map.get(kept[0]), minted.body.expires);
  assert.ok(store.alarm >= (minted.body.expires + 1) * 1000, 'the alarm drops it after the ticket expires');
  // A new hosting session on the same listing does not forget it.
  assert.equal(env.LIVE_HOSTS.instances.has(TEEN), false);
  assert.equal((await redeem(env, TEEN, minted.body.ticket)).body.code, 'used');
});

test('a ticket is redeemed only for its own listing, and a join ticket is never a play ticket', async () => {
  const env = await environment();
  const minted = await mint(env, TEEN);
  const elsewhere = await redeem(env, TEEN_B, minted.body.ticket);
  assert.equal(elsewhere.status, 401, elsewhere.text);
  assert.equal(elsewhere.body.code, 'invalid');
  // Refused there without being spent: it still redeems where it belongs.
  assert.equal((await redeem(env, TEEN, minted.body.ticket)).status, 200);
  // A join ticket (a host's pin as its audience) is not a play ticket, and a play ticket is not a join ticket.
  const join = await call(env, 'POST', '/api/identity/ticket', { who: 'player', body: { audience: 'ab'.repeat(32) } });
  assert.equal(join.status, 200, join.text);
  assert.equal((await redeem(env, TEEN, join.body.ticket)).body.code, 'invalid');
  const asJoin = await call(env, 'POST', '/api/identity/verify', { body: { ticket: minted.body.ticket, audience: playAudience(TEEN) } });
  assert.equal(asJoin.status, 400, asJoin.text);
  for (const bad of [undefined, '', 'eit1.abc.def', 42]) {
    const r = await redeem(env, TEEN, bad);
    assert.ok([400, 401].includes(r.status), `${bad}: ${r.text}`);
    assert.equal(r.body.pass, undefined);
  }
  assert.equal((await call(env, 'POST', `/api/simulations/${TEEN}/play-pass`, { body: 'not json' })).status, 400);
});

test('an expired ticket is refused at redeem', async () => {
  const env = await environment();
  const { ticket } = await makeIdentityTicket(env, { accountId: PLAYER, audience: playAudience(TEEN), now: Math.floor(Date.now() / 1000) - TICKET_TTL_S - 5 });
  const r = await redeem(env, TEEN, ticket);
  assert.equal(r.status, 401, r.text);
  assert.equal(r.body.code, 'expired');
});

test('a pass for listing A is no credential on listing B', async () => {
  const env = await environment();
  const pass = await passFor(env, TEEN);
  for (const path of READS(TEEN_B)) signInRequired(await call(env, 'GET', path, { pass }), `A's pass on ${path}`);
  assert.equal((await inspectPlayPass(env, pass, { simId: TEEN_B })).code, 'wrong_listing');
  // And it still reads A.
  assert.equal((await call(env, 'GET', `/api/simulations/${TEEN}/world/manifest`, { pass })).status, 200);
});

test('an expired pass is no credential', async () => {
  const env = await environment();
  const past = Math.floor(Date.now() / 1000) - PLAY_PASS_TTL_S - 60;
  const { pass } = await makePlayPass(env, { accountId: PLAYER, simId: TEEN, issued: past, now: past });
  assert.equal((await inspectPlayPass(env, pass, { simId: TEEN })).code, 'expired');
  for (const path of READS(TEEN)) signInRequired(await call(env, 'GET', path, { pass }), `expired pass on ${path}`);
});

test('a signed-out viewer still gets sign_in_required, and a malformed, forged or misnamed pass is the same as none', async () => {
  const env = await environment();
  const good = await passFor(env, TEEN);
  const [prefix, body, sig] = good.split('.');
  const payload = JSON.parse(Buffer.from(body, 'base64url').toString());
  const forged = `${prefix}.${Buffer.from(JSON.stringify({ ...payload, s: TEEN_B })).toString('base64url')}.${sig}`;
  const longLived = await makePlayPass({ JWT_SECRET }, { accountId: PLAYER, simId: TEEN, issued: payload.i, now: payload.i + PLAY_PASS_TTL_S });
  const otherKey = await makePlayPass({ JWT_SECRET: 'another-secret-not-real' }, { accountId: PLAYER, simId: TEEN, issued: payload.i });
  const headers = [
    undefined, '', 'PlayPass', 'PlayPass ', `PlayPass ${good}x`, `PlayPass ${forged}`, `playpass ${good}`, `PlayPass  ${good.slice(0, -2)}`,
    `PlayPass ${otherKey.pass}`, `PlayPass ${longLived.pass}`, `Bearer ${good}`, `PlayPass ${tokens.player}`, `PlayPass ${'a'.repeat(5000)}`,
  ];
  for (const auth of headers) {
    for (const path of READS(TEEN)) signInRequired(await call(env, 'GET', path, auth === undefined ? {} : { auth }), `${String(auth).slice(0, 40)} on ${path}`);
  }
  // A G-rated listing stays open to everyone, pass or none.
  assert.equal((await call(env, 'GET', `/api/simulations/${KIDS}/world/manifest`)).status, 200);
  assert.equal((await call(env, 'GET', `/api/simulations/${KIDS}/world/manifest`, { auth: `PlayPass ${forged}` })).status, 200);
});

test('a sign-out or a ban after the ticket was minted ends the ticket and every pass made from it', async () => {
  const env = await environment();
  const pass = await passFor(env, TEEN);
  const minted = await mint(env, TEEN);
  // The account's session floor moves past both (index.js endSessions).
  await env.USERS.put(`authfloor:${PLAYER}`, String(Math.floor(Date.now() / 1000) + 1));
  signInRequired(await call(env, 'GET', `/api/simulations/${TEEN}/world/manifest`, { pass }), 'pass after sign-out');
  const late = await redeem(env, TEEN, minted.body.ticket);
  assert.equal(late.status, 401, late.text);
  assert.equal(late.body.pass, undefined);
});

test('the routes are classed: minting is a session route, redeeming is public', () => {
  assert.equal(routePolicy('POST', `/api/simulations/${TEEN}/play-ticket`).class, SESSION);
  assert.equal(routePolicy('POST', `/api/simulations/${TEEN}/play-pass`).class, PUBLIC);
  assert.equal(routePolicy('POST', `/api/simulations/${TEEN}/play-pass`).explicit, true);
});

// ── Security review fixes (docs/architecture/PLAY_PASS_SECURITY_REVIEW.md) ───

const ATTACKER = '33333333-3333-4333-8333-333333333333';
tokens.attacker = mintToken(ATTACKER);

test('M1: one account cannot lock other accounts out of a listing: it holds at most 20 unexpired play tickets there', async () => {
  const env = await environment();
  await env.USERS.put(`user:${ATTACKER}`, JSON.stringify({ kyc_verified: true, id: ATTACKER, username: 'mallory' }));
  assert.equal(MAX_PLAY_TICKETS_PER_ACCOUNT, 20);
  // Minting and redeeming one at a time keeps no ticket unspent, and is still capped: a spent ticket counts until it expires.
  for (let i = 0; i < MAX_PLAY_TICKETS_PER_ACCOUNT; i++) {
    const m = await mint(env, TEEN, 'attacker');
    assert.equal(m.status, 200, `mint ${i}: ${m.text}`);
    assert.equal((await redeem(env, TEEN, m.body.ticket)).status, 200, `redeem ${i}`);
  }
  const over = await mint(env, TEEN, 'attacker');
  assert.equal(over.status, 429, over.text);
  assert.deepEqual(over.body, { error: 'Too many Play links for this listing. Try again in two minutes.', code: 'too_many_play_tickets' });
  assert.equal(over.body.ticket, undefined);
  // Its share of the spent table stops at the cap, and another account still plays.
  const store = env.LIVE_HOSTS.instances.get(`play:${TEEN}`).storage;
  assert.equal([...store.map.keys()].filter((k) => k.startsWith('ptk:')).length, MAX_PLAY_TICKETS_PER_ACCOUNT);
  assert.equal(await passFor(env, TEEN).then((p) => typeof p), 'string');
  // The cap is per listing: the same account still plays another one.
  assert.equal((await mint(env, TEEN_B, 'attacker')).status, 200);
  // Once its tickets have expired, the account may mint again.
  for (const key of [...store.map.keys()].filter((k) => k.startsWith(`ptm:${ATTACKER}:`))) store.map.set(key, Math.floor(Date.now() / 1000) - 1);
  assert.equal((await mint(env, TEEN, 'attacker')).status, 200);
});

test('M1: minting is rate limited per account through AUTH_RATE_LIMITER, and redeeming per address', async () => {
  const env = await environment();
  const keys = [];
  env.AUTH_RATE_LIMITER = { limit: async ({ key }) => { keys.push(key); return { success: keys.filter((k) => k === key).length <= 3 }; } };
  const statuses = [];
  for (let i = 0; i < 5; i++) statuses.push((await mint(env, TEEN)).status);
  assert.deepEqual(statuses, [200, 200, 200, 429, 429]);
  assert.ok(keys.every((k) => k === `play-ticket:${PLAYER}` || k.startsWith('play-pass:')), keys.join(' '));
  const limited = await mint(env, TEEN);
  assert.deepEqual([limited.status, limited.body.code, limited.headers.get('Retry-After')], [429, 'rate_limited', '60']);
  // Another account has its own budget.
  assert.equal((await mint(env, TEEN, 'author')).status, 200);
  // The redeem counts per address.
  const r = [];
  for (let i = 0; i < 4; i++) r.push((await redeem(env, TEEN, 'eit1.forged.x')).status);
  assert.deepEqual(r, [401, 401, 401, 429]);
  assert.ok(keys.includes('play-pass:203.0.113.9'), keys.join(' '));
});

test('with no rate limiter bound, minting and redeeming refuse instead of running unlimited', async () => {
  const env = await environment();
  const ticket = (await mint(env, TEEN)).body.ticket;
  delete env.AUTH_RATE_LIMITER;
  const minted = await mint(env, TEEN);
  assert.deepEqual([minted.status, minted.body.code, minted.body.ticket], [503, 'busy', undefined]);
  const redeemed = await redeem(env, TEEN, ticket);
  assert.deepEqual([redeemed.status, redeemed.body.code, redeemed.body.pass], [503, 'busy', undefined]);
});

test('M2: a play ticket lives two minutes, and the floor check and the spent record use the same life', async () => {
  const env = await environment();
  // Join tickets keep their ten minutes.
  const join = await call(env, 'POST', '/api/identity/ticket', { who: 'player', body: { audience: 'ab'.repeat(32) } });
  assert.ok(join.body.expires - Math.floor(Date.now() / 1000) > PLAY_TICKET_TTL_S, 'a join ticket still lasts TICKET_TTL_S');
  assert.equal(isAudience(playAudience(TEEN)), false, 'isAudience stays exactly 64 hex');
  assert.equal(isAudience('ab'.repeat(32)), true);
  await assert.rejects(makeIdentityTicket(env, { accountId: PLAYER, audience: playAudience(TEEN), ttl: TICKET_TTL_S + 1 }));
  // A ticket for this listing that claims a longer life than a play ticket's was not minted for play.
  const long = await makeIdentityTicket(env, { accountId: PLAYER, audience: playAudience(TEEN) });
  const refused = await redeem(env, TEEN, long.ticket);
  assert.deepEqual([refused.status, refused.body.code], [401, 'invalid']);
  // The account's floor at the very second the ticket was minted lets it through, as verifyAuth lets a session through.
  // (Both are minted first: a floor also ends the website session the mint uses.)
  const minted = await mint(env, TEEN);
  const late = await mint(env, TEEN);
  await env.USERS.put(`authfloor:${PLAYER}`, String(minted.body.expires - PLAY_TICKET_TTL_S));
  const ok = await redeem(env, TEEN, minted.body.ticket);
  assert.equal(ok.status, 200, ok.text);
  // And one second later it does not.
  await env.USERS.put(`authfloor:${PLAYER}`, String(late.body.expires - PLAY_TICKET_TTL_S + 1));
  assert.equal((await redeem(env, TEEN, late.body.ticket)).status, 401);
  // The spent record lasts exactly as long as the ticket.
  const store = env.LIVE_HOSTS.instances.get(`play:${TEEN}`).storage;
  const spent = [...store.map.entries()].filter(([k]) => k.startsWith('ptk:'));
  assert.deepEqual(spent.map(([, e]) => e), [minted.body.expires]);
});

test('L1: a storage fault in the play object reaches no caller: mint and redeem answer a plain 503 busy', async () => {
  const env = await environment();
  const minted = await mint(env, TEEN);
  class Exploding extends LiveHost {
    constructor(c, e) {
      const storage = fakeDoStorage();
      storage.list = async () => { throw new Error('internal storage detail: shard 7 at do-host-42'); };
      super({ ...c, storage }, e);
    }
  }
  env.LIVE_HOSTS = fakeDoNamespace(Exploding, env);
  for (const r of [await redeem(env, TEEN, minted.body.ticket), await mint(env, TEEN)]) {
    assert.equal(r.status, 503, r.text);
    assert.deepEqual(r.body, { error: 'The gallery could not open this right now. Try again in a minute.', code: 'busy' });
  }
  // A namespace that throws outright is the same.
  env.LIVE_HOSTS = { idFromName: (n) => n, get: () => ({ fetch: async () => { throw new Error('do-host-42 unreachable'); } }) };
  const r = await redeem(env, TEEN, (await call(env, 'POST', `/api/simulations/${TEEN}/play-ticket`, { who: 'author' })).body?.ticket ?? minted.body.ticket);
  assert.equal(r.status, 503, r.text);
  assert.ok(!r.text.includes('do-host-42'), r.text);
});
