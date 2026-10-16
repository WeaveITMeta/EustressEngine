// What Play hands the Player. With nobody hosting, an approved listing plays solo (play/<id>). While a Studio host
// is live, Play on that listing returns the host's own join link with status "live", to a visitor who may join
// (signed in, or holding a live guest lease) and, for an unapproved listing, only to its author. A visitor who may
// not have the link is told it is live and gets the solo link, never the host's. The real Worker runs against
// in-memory storage. See docs/networking/HOSTED_ADMISSION.md and docs/launch/GOAL_LIVE_GALLERY_PLAY.md.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost, checkJoinLink } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const PLAYER = '22222222-2222-4222-8222-222222222222';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const KEY = '0123456789abcdef0123456789abcdef';
const PIN = 'ab'.repeat(32);
const LINK = `eustress-player://join/192.168.1.20:7777?key=${KEY}&pin=${PIN}`;
const SOLO = `eustress-player://play/${SIM}`;

function environment({ liveBinding = true } = {}) {
  const env = { ...fakeEnv() };
  if (liveBinding) env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  return env;
}

async function seed(env, moderation = { status: 'approved', rating: 'all_ages' }) {
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Pong', author_id: AUTHOR, is_public: true, moderation, format: 'echk' }));
  return env;
}

async function call(env, method, path, { token, body, headers } = {}) {
  const h = { ...(headers || {}) };
  if (token) h.Authorization = `Bearer ${token}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let parsed = null;
  try { parsed = JSON.parse(text); } catch { /* not JSON */ }
  return { status: res.status, body: parsed, text, headers: res.headers };
}

const author = mintToken(AUTHOR);
const player = mintToken(PLAYER);
const live = `/api/simulations/${SIM}/live`;
const play = `/api/simulations/${SIM}/play`;
const host = (env, extra = {}) => call(env, 'POST', live, { token: author, body: { link: LINK, players: 2, max_players: 8, protocol: 3, ...extra } });

/// Players the Worker admits to the hosted session: real tickets from a signed-in account, admitted by the host's author.
async function admitPlayers(env, count) {
  for (let i = 0; i < count; i++) {
    const minted = await call(env, 'POST', '/api/identity/ticket', { token: player, body: { audience: PIN } });
    assert.equal(minted.status, 200, minted.text);
    const r = await call(env, 'POST', `${live}/admit`, { token: author, body: { ticket: minted.body.ticket, audience: PIN } });
    assert.equal(r.status, 200, r.text);
  }
}

test('nobody hosting: an approved listing plays solo, the answer names no live session, and nothing is cached', async () => {
  const env = await seed(environment());
  const r = await call(env, 'POST', play, { token: player });
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.status, 'solo');
  assert.equal(r.body.launch.link, SOLO);
  assert.equal(r.body.launch.solo_link, SOLO);
  assert.ok(!('live' in r.body) && !('link_hidden' in r.body));
  assert.equal(r.headers.get('Cache-Control'), 'no-store');
  // A deployment with no live registry answers the same.
  const bare = await seed(environment({ liveBinding: false }));
  assert.equal((await call(bare, 'POST', play, { token: player })).body.status, 'solo');
});

test('a live host: a signed-in visitor gets the host\'s own join link, status "live", and the solo link beside it', async () => {
  const env = await seed(environment());
  assert.equal((await host(env)).status, 200);
  await admitPlayers(env, 1);
  const r = await call(env, 'POST', play, { token: player });
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.status, 'live');
  assert.equal(r.body.launch.link, LINK, 'the exact string the host produced');
  assert.equal(checkJoinLink(r.body.launch.link).ok, true);
  assert.equal(r.body.launch.solo_link, SOLO);
  assert.deepEqual(r.body.live, { players: 2, max_players: 8 });
  assert.ok(!('link_hidden' in r.body));
  assert.equal(r.body.simulation.id, SIM);
});

test('a live host: a visitor with no account and no lease is told it is live and gets only the solo link', async () => {
  const env = await seed(environment());
  await host(env);
  await admitPlayers(env, 1);
  const r = await call(env, 'POST', play);
  assert.equal(r.status, 200, r.text);
  assert.equal(r.body.status, 'solo');
  assert.equal(r.body.launch.link, SOLO);
  assert.equal(r.body.link_hidden, true);
  assert.deepEqual(r.body.live, { players: 2, max_players: 8 });
  // The host's link, key and pin are nowhere in the answer.
  assert.ok(!r.text.includes(KEY) && !r.text.includes(PIN) && !r.text.includes('join/'), r.text);
  // A forged or made-up lease is no lease.
  for (const lease of ['egl1.junk.junk', 'nonsense']) {
    const forged = await call(env, 'POST', play, { headers: { 'X-Eustress-Lease': lease } });
    assert.equal(forged.body.link_hidden, true, lease);
    assert.ok(!forged.text.includes(PIN));
  }
});

test('the players the play route shows are what the host claims up to the players the Worker admitted plus the host, to a visitor with a link and one without', async () => {
  const env = await seed(environment());
  await host(env, { players: 10_000, max_players: 10_000 });
  const shown = async (token) => (await call(env, 'POST', play, { token })).body.live.players;
  assert.equal(await shown(player), 1, 'a claim of 10,000 with nobody admitted is the host alone');
  assert.equal(await shown(undefined), 1, 'the same to a visitor with no account');
  await admitPlayers(env, 3);
  assert.equal(await shown(player), 4, 'three admitted and the host');
  assert.equal(await shown(undefined), 4);
  await host(env, { players: 2, max_players: 10_000 });
  assert.equal(await shown(player), 2, 'a claim below the cap stands');
  await host(env, { players: 0, max_players: 10_000 });
  assert.equal(await shown(player), 0, 'a host that is not playing, with players in, shows what it claims');
  const r = await call(env, 'POST', play, { token: player });
  assert.deepEqual(r.body.live, { players: 0, max_players: 10_000 });
});

test('a live host: a guest holding a live lease gets the link', async () => {
  const env = await seed(environment());
  await host(env);
  const open = await call(env, 'POST', '/api/guest/play', { headers: { 'CF-Connecting-IP': '203.0.113.7' }, body: { sim_id: SIM, character: 'F' } });
  assert.equal(open.status, 200, open.text);
  const r = await call(env, 'POST', play, { headers: { 'X-Eustress-Lease': open.body.ticket } });
  assert.equal(r.body.status, 'live');
  assert.equal(r.body.launch.link, LINK);
});

test('an unapproved listing: its author plays and joins their own host; no one else can play it at all', async () => {
  const env = await seed(environment(), { status: 'pending' });
  await host(env);
  const own = await call(env, 'POST', play, { token: author });
  assert.equal(own.status, 200, own.text);
  assert.equal(own.body.status, 'live');
  assert.equal(own.body.launch.link, LINK);
  const other = await call(env, 'POST', play, { token: player });
  assert.equal(other.status, 403);
  assert.ok(!other.text.includes(PIN));
  const anon = await call(env, 'POST', play);
  assert.equal(anon.status, 401);
  assert.ok(!anon.text.includes(PIN));
});

test('a quarantined listing hands out nothing, live or not', async () => {
  const env = await seed(environment(), { status: 'quarantined' });
  await host(env);
  for (const token of [author, player, undefined]) {
    const r = await call(env, 'POST', play, { token });
    assert.ok([401, 403].includes(r.status), String(r.status));
    assert.ok(!r.text.includes(PIN));
  }
});

test('a host that has gone quiet is offline again: Play is solo once the heartbeat lapses', async () => {
  const env = await seed(environment());
  await host(env);
  assert.equal((await call(env, 'POST', play, { token: player })).body.status, 'live');
  assert.equal((await call(env, 'DELETE', live, { token: author })).status, 200);
  const after = await call(env, 'POST', play, { token: player });
  assert.equal(after.body.status, 'solo');
  assert.equal(after.body.launch.link, SOLO);
  assert.ok(!('live' in after.body));
});

test('a link in the registry that is not the exact join grammar is never handed out', async () => {
  const env = await seed(environment());
  await host(env);
  assert.equal((await call(env, 'POST', play, { token: player })).body.status, 'live');
  // Plant what a bug or an old build might have stored: another scheme, in the host's own record.
  const object = env.LIVE_HOSTS.instances.get(SIM);
  const record = await object.storage.get('host');
  assert.equal(record.link, LINK, 'the record the test is about to damage is the real one');
  for (const bad of ['javascript:alert(1)', 'https://example.com/', 'eustress://join/192.168.1.20:7777?key=' + KEY + '&pin=' + PIN, LINK + '&extra=1']) {
    await object.storage.put('host', { ...record, link: bad });
    const r = await call(env, 'POST', play, { token: player });
    assert.equal(r.body.status, 'solo', bad);
    assert.equal(r.body.launch.link, SOLO, bad);
    assert.ok(!r.text.includes('javascript:') && !r.text.includes('example.com') && !r.text.includes(PIN), bad);
  }
});

test('the author\'s review view says when a person has the case, and never why', async () => {
  // Held and quarantined cases read as "in review" with person_review set; a pending one does not; neither shows a reason.
  const { authorView } = await import('../src/moderation.mjs');
  const sim = { id: SIM, is_public: true };
  for (const [status, person] of [['pending', false], ['classifying', false], ['held', true], ['quarantined', true]]) {
    const v = authorView({ status, decision: { reasons: [{ action: 'reject', category: 'x' }] }, updated_at: 'now' }, sim);
    assert.equal(v.status, 'in_review', status);
    assert.equal(v.person_review, person, status);
    assert.deepEqual(v.reasons, [], status);
    assert.equal(v.suggested_edit, null);
    assert.equal(v.note, null);
  }
  for (const status of ['approved', 'rejected', 'changes_requested']) {
    assert.equal(authorView({ status, decision: {}, updated_at: 'now' }, sim).person_review, false, status);
  }
});
