// The Eustress Discord app (src/discord_interactions.mjs) through the real Worker: the Ed25519 signature gate on
// POST /api/discord/interactions, every command, the rating cap for each place a command can be used, /follow's
// server-side checks, command and Linked Roles registration from the five-minute tick, and the follow posts it makes.
// Storage is in memory; Discord is a stand-in on globalThis.fetch, and nothing else is reachable.

import test from 'node:test';
import assert from 'node:assert/strict';
import nodeCrypto from 'node:crypto';
import { loadWorker, fakeEnv, fakeKv, fakeDoNamespace } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall } from '../src/account_auth.mjs';
import { Wallet, CommerceHub } from '../src/commerce.mjs';
import { LiveIndex } from '../src/live_index.mjs';
import { MODERATION_TICK_CRON } from '../src/moderation.mjs';
import { routePolicy, PUBLIC } from '../src/route_policy.mjs';
import {
  COMMANDS, ROLE_METADATA, POSTS_PER_RUN, NOT_LINKED_PROFILE, NOT_LINKED_ME,
  registerDiscordDefinitions, sweepDiscordFollows, discordTick, verifyDiscordSignature,
} from '../src/discord_interactions.mjs';
import { sealDiscordToken, openDiscordToken, roleConnectionUrl, DISCORD_TOKEN } from '../src/discord_link.mjs';

const worker = await loadWorker();
const API = 'https://api.eustress.dev';
const SITE = 'https://eustress.dev';
const APP_ID = '1571137497331662879';
const GUILD = '300000000000000001';
const CHANNEL = '300000000000000002';
const RESTRICTED_CHANNEL = '300000000000000003';

// ── Signing ──────────────────────────────────────────────────────────────────

const keys = await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify']);
const PUBLIC_HEX = Buffer.from(await crypto.subtle.exportKey('raw', keys.publicKey)).toString('hex');
const other = await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify']);
const nowSeconds = () => String(Math.floor(Date.now() / 1000));
async function signature(text, key = keys.privateKey) {
  return Buffer.from(await crypto.subtle.sign({ name: 'Ed25519' }, key, new TextEncoder().encode(text))).toString('hex');
}

// ── Discord, stood in ─────────────────────────────────────────────────────────

const calls = [];
/// (url, init) -> Response, or undefined for the default 200 `{}`.
let responder = () => undefined;
globalThis.fetch = async (input, init = {}) => {
  const url = typeof input === 'string' ? input : input.url;
  if (!url.startsWith('https://discord.com/')) return new Response('{"error":"offline in tests"}', { status: 503 });
  calls.push({ url, init, body: typeof init.body === 'string' && init.body.startsWith('{') || typeof init.body === 'string' && init.body.startsWith('[') ? JSON.parse(init.body) : init.body });
  return (await responder(url, init)) || new Response('{}', { status: 200 });
};
const callsTo = (fragment) => calls.filter((c) => c.url.includes(fragment));

// ── Accounts and listings ────────────────────────────────────────────────────

const ADA = 'a1111111-1111-4111-8111-111111111111'; // linked, verified adult, Adult content off
const BEA = 'a2222222-2222-4222-8222-222222222222'; // linked, verified adult, Adult content on
const CAM = 'a3333333-3333-4333-8333-333333333333'; // linked, no verified age
const DEE = 'a4444444-4444-4444-8444-444444444444'; // the creator, linked
const DISCORD = { ada: '200000000000000001', bea: '200000000000000002', cam: '200000000000000003', dee: '200000000000000004', nobody: '299999999999999999' };

const sim = (n) => `cccccccc-0000-4000-8000-00000000000${n}`;
const LISTINGS = [
  { id: sim(1), name: 'Alpha Garden', rating: 'all_ages' },
  { id: sim(2), name: 'Bravo Bridge', rating: 'ages_10' },
  { id: sim(3), name: 'Charlie Canyon', rating: 'teen_13' },
  { id: sim(4), name: 'Delta Dunes', rating: 'mature_17' },
  { id: sim(5), name: 'Echo Lounge', rating: 'adult_18' },
  { id: sim(6), name: 'Foxtrot Unknown', rating: null },
];
const nameOf = Object.fromEntries(LISTINGS.map((l) => [l.id, l.name]));
const T0 = '2026-11-01T00:00:00.000Z';

async function putListing(env, l, extra = {}) {
  await env.SOCIAL.put(`sim:${l.id}`, JSON.stringify({
    id: l.id, name: l.name, description: `${l.name}, a Space`, author_id: DEE, author_name: 'dee', is_public: true,
    moderation: { status: 'approved', ...(l.rating ? { rating: l.rating } : {}) },
    thumbnail_url: `${API}/api/simulations/${l.id}/thumbnail`, version: 1, published_at: T0, updated_at: T0, ...extra,
  }));
}

async function linkDiscord(env, accountId, discordId) {
  await env.USERS.put(`discordlink:${accountId}`, JSON.stringify({ id: discordId, username: `d${discordId.slice(-3)}`, linked_at: new Date().toISOString() }));
  await env.USERS.put(`discord:${discordId}`, accountId);
}

async function environment(extra = {}) {
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: API,
    DISCORD_PUBLIC_KEY: PUBLIC_HEX, DISCORD_CLIENT_ID: APP_ID, DISCORD_CLIENT_SECRET: 'test-discord-secret', ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  env.WALLETS = fakeDoNamespace(Wallet, env);
  env.COMMERCE_HUBS = fakeDoNamespace(CommerceHub, env);
  env.LIVE_INDEX = fakeDoNamespace(LiveIndex, env);
  for (const [id, name] of [[ADA, 'ada'], [BEA, 'bea'], [CAM, 'cam'], [DEE, 'dee']]) {
    const key = nodeCrypto.randomBytes(32).toString('hex');
    await env.USERS.put(`user:${id}`, JSON.stringify({ id, username: name, public_key: key, kyc_verified: true, email: `${name}@example.com`, created_at: '2026-01-01T00:00:00Z' }));
    await env.USERS.put(`username:${name}`, id);
    await env.USERS.put(`pubkey:${key}`, id);
    await linkDiscord(env, id, DISCORD[name]);
  }
  for (const id of [ADA, BEA, DEE]) await env.KYC_STATUS.put(`kyc-${id}-front`, JSON.stringify({ status: 'verified', extracted_dob: '1990-01-01' }));
  await env.USERS.put(`adult-content:${BEA}`, '1');
  for (const l of LISTINGS) await putListing(env, l);
  return env;
}

async function goLive(env, simId, players = 3) {
  const stub = env.LIVE_INDEX.get(env.LIVE_INDEX.idFromName('all'));
  const now = Date.now();
  const res = await stub.fetch('https://live-index.internal/beat', { method: 'POST', body: JSON.stringify({ sim_id: simId, author: DEE, players, max_players: 10, region: null, since: now - 1000, now }) });
  assert.equal(res.status, 200);
}

// ── Interactions ─────────────────────────────────────────────────────────────

/// An interaction as Discord sends it. `context` 0 guild, 1 the app's DM, 2 a group DM.
function interaction({ type = 2, name, options, who = DISCORD.nobody, context = 0, nsfw = false, owners, permissions = '0', channel = CHANNEL } = {}) {
  const base = {
    id: '400000000000000001', application_id: APP_ID, type, token: 'interaction-token_ABC.123', version: 1, context,
    authorizing_integration_owners: owners || (context === 0 ? { 0: GUILD, 1: who } : { 1: who }),
    ...(name ? { data: { id: '500000000000000001', name, type: 1, ...(options ? { options } : {}) } } : {}),
  };
  if (context === 0) return { ...base, guild_id: GUILD, channel_id: channel, channel: { id: channel, type: 0, nsfw }, member: { user: { id: who, username: 'x' }, permissions } };
  return { ...base, channel_id: channel, channel: { id: channel, type: context === 1 ? 1 : 3 }, user: { id: who, username: 'x' } };
}

/// POST it signed (or not), through the real Worker. Returns { status, body, settled }: `settled` waits for the deferred work.
async function send(env, payload, { body: rawBody, timestamp = nowSeconds(), sig, headers = {}, omit = [] } = {}) {
  const text = rawBody ?? JSON.stringify(payload);
  const h = { 'Content-Type': 'application/json', 'X-Signature-Timestamp': timestamp, 'X-Signature-Ed25519': sig ?? await signature(timestamp + text), ...headers };
  for (const o of omit) delete h[o];
  const pending = [];
  const ctx = { waitUntil(p) { pending.push(p); }, passThroughOnException() {} };
  const res = await worker.fetch(new Request(`${API}/api/discord/interactions`, { method: 'POST', headers: h, body: text }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null), settled: () => Promise.all(pending) };
}

/// Run a command to its finished message: the deferred answer, then the edit of @original.
async function run(env, payload) {
  calls.length = 0;
  const r = await send(env, payload);
  if (r.body?.type !== 5) return { immediate: r.body, status: r.status };
  await r.settled();
  const edits = callsTo(`/webhooks/${APP_ID}/interaction-token_ABC.123/messages/@original`);
  assert.equal(edits.length, 1, 'one edit of the deferred reply');
  assert.equal(edits[0].init.method, 'PATCH');
  return { deferred: r.body, message: edits[0].body };
}

/// The listing ids /space's autocomplete offers.
async function offered(env, opts, typed = '') {
  const r = await send(env, interaction({ ...opts, type: 4, name: 'space', options: [{ type: 3, name: 'query', value: typed, focused: true }] }));
  assert.equal(r.status, 200);
  assert.equal(r.body.type, 8);
  return r.body.data.choices.map((c) => c.value).sort();
}
const ids = (...n) => n.map(sim).sort();

// ── The signature gate ───────────────────────────────────────────────────────

test('the endpoint is public in the route policy: the signature is its gate', () => {
  const p = routePolicy('POST', '/api/discord/interactions');
  assert.equal(p.class, PUBLIC);
  assert.equal(p.explicit, true);
  assert.match(p.why, /signature/);
});

test('a PING with a valid signature is answered PONG', async () => {
  const env = await environment();
  const r = await send(env, { type: 1 });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body, { type: 1 });
});

test('a tampered body is refused 401 and read no further', async () => {
  const env = await environment();
  const timestamp = nowSeconds();
  const text = JSON.stringify({ type: 1 });
  const sig = await signature(timestamp + text);
  const r = await send(env, null, { body: JSON.stringify({ type: 1, x: 1 }), timestamp, sig });
  assert.equal(r.status, 401);
  assert.equal(r.body.type, undefined);
});

test('a tampered timestamp is refused 401', async () => {
  const env = await environment();
  const timestamp = nowSeconds();
  const text = JSON.stringify({ type: 1 });
  const sig = await signature(timestamp + text);
  assert.equal((await send(env, null, { body: text, timestamp: String(Number(timestamp) - 1), sig })).status, 401);
});

test('missing signature headers, another key, a malformed signature or a stale one are refused 401', async () => {
  const env = await environment();
  assert.equal((await send(env, { type: 1 }, { omit: ['X-Signature-Ed25519'] })).status, 401);
  assert.equal((await send(env, { type: 1 }, { omit: ['X-Signature-Timestamp'] })).status, 401);
  assert.equal((await send(env, { type: 1 }, { omit: ['X-Signature-Ed25519', 'X-Signature-Timestamp'] })).status, 401);
  const timestamp = nowSeconds();
  const text = JSON.stringify({ type: 1 });
  assert.equal((await send(env, null, { body: text, timestamp, sig: await signature(timestamp + text, other.privateKey) })).status, 401);
  assert.equal((await send(env, null, { body: text, timestamp, sig: 'zz'.repeat(64) })).status, 401);
  assert.equal((await send(env, null, { body: text, timestamp, sig: '00'.repeat(64) })).status, 401);
  // Signed correctly, but ten minutes old: a replay.
  const stale = String(Math.floor(Date.now() / 1000) - 600);
  assert.equal((await send(env, null, { body: text, timestamp: stale, sig: await signature(stale + text) })).status, 401);
  // A bearer token changes nothing.
  assert.equal((await send(env, { type: 1 }, { omit: ['X-Signature-Ed25519'], headers: { Authorization: 'Bearer x' } })).status, 401);
});

test('without DISCORD_PUBLIC_KEY the endpoint answers 503 and processes nothing', async () => {
  const env = await environment({ DISCORD_PUBLIC_KEY: '' });
  assert.equal((await send(env, { type: 1 })).status, 503);
});

test('the verifier checks the exact bytes signed', async () => {
  const ts = nowSeconds();
  const sig = await signature(`${ts}{"type":1}`);
  assert.equal(await verifyDiscordSignature(PUBLIC_HEX, sig, ts, '{"type":1}'), true);
  assert.equal(await verifyDiscordSignature(PUBLIC_HEX, sig, ts, '{"type":1} '), false);
  assert.equal(await verifyDiscordSignature(PUBLIC_HEX.replace(/^./, (c) => (c === '0' ? '1' : '0')), sig, ts, '{"type":1}'), false);
  assert.equal(await verifyDiscordSignature('not-hex', sig, ts, '{"type":1}'), false);
});

// ── The rating cap ───────────────────────────────────────────────────────────

test('the rating cap: ages_10 everywhere unless a linked verified adult asks in an age-restricted channel or the app DM', async () => {
  const env = await environment();
  const G = ids(1, 2); // all_ages, ages_10
  const UP_TO_17 = ids(1, 2, 3, 4);
  const cases = [
    ['unlinked, normal channel', { who: DISCORD.nobody, context: 0 }, G],
    ['unlinked, age-restricted channel', { who: DISCORD.nobody, context: 0, nsfw: true }, G],
    ['unlinked, app DM', { who: DISCORD.nobody, context: 1 }, G],
    ['unlinked, group DM', { who: DISCORD.nobody, context: 2 }, G],
    ['linked adult, normal channel', { who: DISCORD.ada, context: 0 }, G],
    ['linked adult, age-restricted channel', { who: DISCORD.ada, context: 0, nsfw: true }, UP_TO_17],
    ['linked adult, app DM', { who: DISCORD.ada, context: 1 }, UP_TO_17],
    ['linked adult, group DM', { who: DISCORD.ada, context: 2 }, G],
    ['linked, no verified age, age-restricted channel', { who: DISCORD.cam, context: 0, nsfw: true }, G],
    ['linked, no verified age, app DM', { who: DISCORD.cam, context: 1 }, G],
    // Adult content on: the account's own cap reaches adult_18 and adult_21, and a listing with no rating is still never shown.
    ['linked adult with Adult content on, app DM', { who: DISCORD.bea, context: 1 }, ids(1, 2, 3, 4, 5)],
    ['linked adult with Adult content on, normal channel', { who: DISCORD.bea, context: 0 }, G],
    ['linked adult with Adult content on, group DM', { who: DISCORD.bea, context: 2 }, G],
  ];
  for (const [label, opts, want] of cases) assert.deepEqual(await offered(env, opts), want, label);
});

test('a link revoked by a key replacement no longer raises the cap or shows a profile', async () => {
  const env = await environment();
  assert.deepEqual(await offered(env, { who: DISCORD.ada, context: 1 }), ids(1, 2, 3, 4));
  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await accountCall(env, ADA, 'unlink_discord')).ok, true);
  assert.deepEqual(await offered(env, { who: DISCORD.ada, context: 1 }), ids(1, 2));
  const p = await run(env, interaction({ name: 'profile', options: [{ type: 6, name: 'user', value: DISCORD.ada }] }));
  assert.equal(p.message.content, NOT_LINKED_PROFILE);
});

test('autocomplete matches names and creators, and offers at most 25', async () => {
  const env = await environment();
  assert.deepEqual(await offered(env, {}, 'brav'), ids(2));
  assert.deepEqual(await offered(env, {}, 'DEE'), ids(1, 2));
  for (let i = 0; i < 30; i++) await putListing(env, { id: `dddddddd-0000-4000-8000-0000000000${String(i).padStart(2, '0')}`, name: `Space ${i}`, rating: 'all_ages' });
  assert.equal((await offered(env, {}, 'space')).length, 25);
});

// ── /space ───────────────────────────────────────────────────────────────────

test('/space answers deferred, then a card with the creator, rating, thumbnail, live players and real links', async () => {
  const env = await environment();
  await goLive(env, sim(2), 4);
  const r = await run(env, interaction({ name: 'space', options: [{ type: 3, name: 'query', value: sim(2) }] }));
  assert.deepEqual(r.deferred, { type: 5 }, 'public, not ephemeral');
  const [embed] = r.message.embeds;
  assert.equal(embed.title, 'Bravo Bridge');
  assert.equal(embed.url, `${SITE}/simulation/${sim(2)}`);
  assert.deepEqual(embed.fields.map((f) => [f.name, f.value]), [['Creator', 'dee'], ['Rating', 'Ages 10+'], ['Live now', '4 playing']]);
  assert.equal(embed.thumbnail.url, `${API}/api/simulations/${sim(2)}/thumbnail`);
  assert.deepEqual(r.message.components[0].components.map((b) => [b.style, b.label, b.url]), [[5, 'View on Eustress', `${SITE}/simulation/${sim(2)}`], [5, 'Play', `${SITE}/play/${sim(2)}`]]);
  assert.deepEqual(r.message.allowed_mentions, { parse: [] });
});

test('/space never shows a listing above the cap, by id or by name, and an adult card carries no image', async () => {
  const env = await environment();
  for (const query of [sim(4), 'Delta Dunes', sim(6), 'Foxtrot']) {
    const r = await run(env, interaction({ name: 'space', options: [{ type: 3, name: 'query', value: query }] }));
    assert.equal(r.message.content, 'No listed Space matches that.', query);
    assert.deepEqual(r.message.embeds, []);
  }
  const adult = await run(env, interaction({ name: 'space', who: DISCORD.bea, context: 1, options: [{ type: 3, name: 'query', value: sim(5) }] }));
  assert.equal(adult.message.embeds[0].title, 'Echo Lounge');
  assert.equal(adult.message.embeds[0].thumbnail, undefined, 'no adult image in Discord');
  const mature = await run(env, interaction({ name: 'space', who: DISCORD.ada, context: 0, nsfw: true, options: [{ type: 3, name: 'query', value: sim(4) }] }));
  assert.equal(mature.message.embeds[0].title, 'Delta Dunes');
  assert.ok(mature.message.embeds[0].thumbnail.url);
});

// ── /live ────────────────────────────────────────────────────────────────────

test('/live lists listed live Spaces within the cap, with Play links', async () => {
  const env = await environment();
  const none = await run(env, interaction({ name: 'live' }));
  assert.equal(none.message.content, 'No listed Spaces are live right now.');
  await goLive(env, sim(2), 2);
  await goLive(env, sim(4), 5);
  const normal = await run(env, interaction({ name: 'live' }));
  assert.deepEqual(normal.message.embeds.map((e) => e.title), ['Bravo Bridge']);
  assert.deepEqual(normal.message.components[0].components.map((b) => b.url), [`${SITE}/play/${sim(2)}`]);
  const restricted = await run(env, interaction({ name: 'live', who: DISCORD.ada, nsfw: true }));
  assert.deepEqual(restricted.message.embeds.map((e) => e.title).sort(), ['Bravo Bridge', 'Delta Dunes']);
  assert.ok(restricted.message.embeds.every((e) => e.fields.some((f) => f.name === 'Players')));
});

// ── /profile ─────────────────────────────────────────────────────────────────

test('/profile shows a linked person\'s public profile with the listed Spaces the cap allows; unlinked says so', async () => {
  const env = await environment();
  const r = await run(env, interaction({ name: 'profile', options: [{ type: 6, name: 'user', value: DISCORD.dee }] }));
  const [embed] = r.message.embeds;
  assert.equal(embed.title, 'dee');
  assert.equal(embed.url, `${SITE}/profile/dee`);
  assert.deepEqual(embed.fields.map((f) => [f.name, f.value]), [['Username', 'dee'], ['Listed Spaces', '2']]);
  const restricted = await run(env, interaction({ name: 'profile', who: DISCORD.ada, nsfw: true, options: [{ type: 6, name: 'user', value: DISCORD.dee }] }));
  assert.equal(restricted.message.embeds[0].fields.find((f) => f.name === 'Listed Spaces').value, '4');
  const unlinked = await run(env, interaction({ name: 'profile', options: [{ type: 6, name: 'user', value: DISCORD.nobody }] }));
  assert.equal(unlinked.message.content, NOT_LINKED_PROFILE);
  assert.deepEqual(unlinked.message.embeds, []);
});

// ── /me ──────────────────────────────────────────────────────────────────────

test('/me is ephemeral: balances and unread notifications when linked, a Settings link when not', async () => {
  const env = await environment();
  await env.SOCIAL.put(`notif:${ADA}:1`, JSON.stringify({ at: T0, read: false }));
  await env.SOCIAL.put(`notif:${ADA}:2`, JSON.stringify({ at: T0, read: true }));
  await env.SOCIAL.put(`notif:${ADA}:3`, JSON.stringify({ at: T0 }));
  const r = await run(env, interaction({ name: 'me', who: DISCORD.ada }));
  assert.deepEqual(r.deferred, { type: 5, data: { flags: 64 } });
  const fields = Object.fromEntries(r.message.embeds[0].fields.map((f) => [f.name, f.value]));
  assert.equal(fields.Bliss, '0.00');
  assert.equal(fields.Tickets, '0');
  assert.equal(fields['Unread notifications'], '2');
  const unlinked = await run(env, interaction({ name: 'me', who: DISCORD.nobody, context: 1 }));
  assert.deepEqual(unlinked.deferred, { type: 5, data: { flags: 64 } });
  assert.equal(unlinked.message.content, NOT_LINKED_ME);
  assert.equal(unlinked.message.components[0].components[0].url, `${SITE}/settings`);
});

// ── /follow ──────────────────────────────────────────────────────────────────

const follow = (sub, options = [], extra = {}) => interaction({ name: 'follow', who: DISCORD.ada, permissions: '32', ...extra, options: [{ type: 1, name: sub, options }] });

test('/follow is refused at once when the app is not installed to the server, outside a server, or without Manage Server', async () => {
  const env = await environment();
  const notInstalled = await send(env, follow('list', [], { owners: { 1: DISCORD.ada } }));
  assert.equal(notInstalled.body.type, 4);
  assert.equal(notInstalled.body.data.flags, 64);
  assert.match(notInstalled.body.data.content, /Add to server/);
  const noPermission = await send(env, follow('list', [], { permissions: String(1 << 4) }));
  assert.equal(noPermission.body.type, 4);
  assert.match(noPermission.body.data.content, /Manage Server/);
  const dm = await send(env, follow('list', [], { context: 1 }));
  assert.match(dm.body.data.content, /server channel/);
  for (const [permissions, why] of [[String(1 << 3), 'Administrator is enough'], [(1n << 40n | 32n).toString(), 'a large bitfield is read whole']]) {
    const r = await send(env, follow('list', [], { permissions }));
    assert.equal(r.body.type, 5, why);
    await r.settled();
  }
  // Nothing was stored by the refusals.
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('discordfollow')).length, 0);
});

test('/follow space, creator, list and remove; a Space rated above the channel is refused', async () => {
  const env = await environment();
  const spaced = await run(env, follow('space', [{ type: 3, name: 'space', value: sim(2) }]));
  assert.deepEqual(spaced.deferred, { type: 5, data: { flags: 64 } });
  assert.match(spaced.message.content, /now follows Bravo Bridge/);
  const record = JSON.parse(await env.SOCIAL.get(`discordfollow:space:${sim(2)}:${GUILD}:${CHANNEL}`));
  assert.equal(record.age_restricted, false);
  assert.equal(record.channel_id, CHANNEL);

  const tooHigh = await run(env, follow('space', [{ type: 3, name: 'space', value: sim(3) }]));
  assert.match(tooHigh.message.content, /not listed, or its rating is above/);
  const restricted = await run(env, follow('space', [{ type: 3, name: 'space', value: sim(3) }], { nsfw: true, channel: RESTRICTED_CHANNEL }));
  assert.match(restricted.message.content, /now follows Charlie Canyon/);
  assert.equal(JSON.parse(await env.SOCIAL.get(`discordfollow:space:${sim(3)}:${GUILD}:${RESTRICTED_CHANNEL}`)).age_restricted, true);
  // Never an adult listing, even for a viewer whose own cap is higher.
  const adult = await run(env, follow('space', [{ type: 3, name: 'space', value: sim(5) }], { who: DISCORD.bea, nsfw: true, channel: RESTRICTED_CHANNEL }));
  assert.match(adult.message.content, /not listed, or its rating is above/);

  const creator = await run(env, follow('creator', [{ type: 3, name: 'username', value: 'dee' }]));
  assert.match(creator.message.content, /now follows dee/);
  assert.ok(await env.SOCIAL.get(`discordfollow:creator:${DEE}:${GUILD}:${CHANNEL}`));
  const missing = await run(env, follow('creator', [{ type: 3, name: 'username', value: 'nobody-here' }]));
  assert.equal(missing.message.content, 'No Eustress account has that username.');

  const list = await run(env, follow('list'));
  assert.equal(list.message.content.split('\n').length, 3);
  assert.match(list.message.content, new RegExp(`<#${RESTRICTED_CHANNEL}> space: Charlie Canyon \\(age-restricted channel\\)`));

  // Remove autocompletes this channel's follows and takes the chosen one away.
  const ac = await send(env, interaction({ type: 4, name: 'follow', who: DISCORD.ada, permissions: '32', options: [{ type: 1, name: 'remove', options: [{ type: 3, name: 'name', value: '', focused: true }] }] }));
  assert.deepEqual(ac.body.data.choices.map((c) => c.value).sort(), [`creator:${DEE}`, `space:${sim(2)}`]);
  const removed = await run(env, follow('remove', [{ type: 3, name: 'name', value: `space:${sim(2)}` }]));
  assert.match(removed.message.content, /no longer follows Bravo Bridge/);
  assert.equal(await env.SOCIAL.get(`discordfollow:space:${sim(2)}:${GUILD}:${CHANNEL}`), null);
  assert.equal(await env.SOCIAL.get(`discordfollowg:${GUILD}:${CHANNEL}:space:${sim(2)}`), null);
});

// ── Definitions and registration ─────────────────────────────────────────────

test('the commands and Linked Roles records are what Discord is told', () => {
  const byName = Object.fromEntries(COMMANDS.map((c) => [c.name, c]));
  assert.deepEqual(Object.keys(byName).sort(), ['follow', 'live', 'me', 'profile', 'space']);
  for (const n of ['space', 'live', 'profile', 'me']) {
    assert.deepEqual(byName[n].integration_types, [0, 1], n);
    assert.deepEqual(byName[n].contexts, [0, 1, 2], n);
  }
  assert.deepEqual(byName.follow.integration_types, [0]);
  assert.deepEqual(byName.follow.contexts, [0]);
  assert.equal(byName.follow.default_member_permissions, '32');
  assert.deepEqual(byName.follow.options.map((o) => o.name), ['space', 'creator', 'list', 'remove']);
  for (const c of COMMANDS) {
    assert.match(c.name, /^[a-z]{1,32}$/);
    assert.ok(c.description.length <= 100 && !c.description.includes(String.fromCharCode(0x2014)), c.name);
  }
  assert.ok(ROLE_METADATA.length <= 5);
  assert.deepEqual(ROLE_METADATA.map((m) => [m.key, m.type]), [['verified', 7], ['creator', 7], ['spaces', 2], ['member_since', 6]]);
  for (const m of ROLE_METADATA) assert.match(m.key, /^[a-z0-9_]{1,50}$/);
});

test('registration PUTs only what changed, with the bot token, and keeps the hash only after a 2xx', async () => {
  const env = { SOCIAL: fakeKv(), DISCORD_CLIENT_ID: APP_ID, DISCORD_BOT_TOKEN: 'bot-token-test' };
  calls.length = 0;
  assert.deepEqual(await registerDiscordDefinitions({ SOCIAL: fakeKv(), DISCORD_CLIENT_ID: APP_ID }), { commands: 'skipped', metadata: 'skipped' });
  assert.deepEqual(await registerDiscordDefinitions({ SOCIAL: fakeKv(), DISCORD_BOT_TOKEN: 'x' }), { commands: 'skipped', metadata: 'skipped' });
  assert.equal(calls.length, 0);

  assert.deepEqual(await registerDiscordDefinitions(env), { commands: 'registered', metadata: 'registered' });
  assert.deepEqual(calls.map((c) => [c.init.method, c.url]), [
    ['PUT', `https://discord.com/api/v10/applications/${APP_ID}/commands`],
    ['PUT', `https://discord.com/api/v10/applications/${APP_ID}/role-connections/metadata`],
  ]);
  assert.ok(calls.every((c) => c.init.headers.Authorization === 'Bot bot-token-test'));
  assert.deepEqual(calls[0].body, JSON.parse(JSON.stringify(COMMANDS)));
  assert.deepEqual(calls[1].body, JSON.parse(JSON.stringify(ROLE_METADATA)));

  calls.length = 0;
  assert.deepEqual(await registerDiscordDefinitions(env), { commands: 'unchanged', metadata: 'unchanged' });
  assert.equal(calls.length, 0, 'no PUT when nothing changed');

  // The commands changed since they were registered (the stored hash is another's): only they are PUT, and Discord fails.
  await env.SOCIAL.put('discordreg:commands', 'f'.repeat(64));
  responder = (url) => (url.endsWith('/commands') ? new Response('{}', { status: 500 }) : undefined);
  calls.length = 0;
  assert.deepEqual(await registerDiscordDefinitions(env), { commands: 'failed', metadata: 'unchanged' });
  assert.equal(calls.length, 1);
  assert.equal(await env.SOCIAL.get('discordreg:commands'), 'f'.repeat(64), 'the hash is not saved on a failure');
  responder = () => undefined;
  calls.length = 0;
  assert.deepEqual(await registerDiscordDefinitions(env), { commands: 'registered', metadata: 'unchanged' });
  assert.equal(calls.length, 1);
});

// ── The sealed token ─────────────────────────────────────────────────────────

test('a kept token is sealed with a fresh IV, opens only for its account and secret, and not once tampered', async () => {
  const env = { DISCORD_CLIENT_SECRET: 'secret-one' };
  const a = await sealDiscordToken(env, ADA, 'refresh-token-value');
  const b = await sealDiscordToken(env, ADA, 'refresh-token-value');
  assert.equal(Buffer.from(a.iv, 'base64').length, 12);
  assert.notEqual(a.iv, b.iv);
  assert.notEqual(a.ct, b.ct);
  assert.ok(!JSON.stringify(a).includes('refresh-token-value'));
  assert.equal(await openDiscordToken(env, ADA, a), 'refresh-token-value');
  assert.equal(await openDiscordToken(env, BEA, a), null);
  assert.equal(await openDiscordToken({ DISCORD_CLIENT_SECRET: 'secret-two' }, ADA, a), null);
  const ct = Buffer.from(a.ct, 'base64');
  ct[0] ^= 1;
  assert.equal(await openDiscordToken(env, ADA, { ...a, ct: ct.toString('base64') }), null);
  await assert.rejects(sealDiscordToken({}, ADA, 'x'));
});

// ── The five-minute tick ─────────────────────────────────────────────────────

/// The five-minute tick through the real Worker's scheduled handler, at `at` (ms).
async function tick(env, at) {
  const pending = [];
  await worker.scheduled({ cron: MODERATION_TICK_CRON, scheduledTime: at }, env, { waitUntil(p) { pending.push(p); }, passThroughOnException() {} });
  await Promise.all(pending);
}
const posts = () => callsTo('/channels/').filter((c) => c.init.method === 'POST' && c.url.endsWith('/messages'));

test('the tick registers once, posts follow updates within each channel\'s rating, once per follow and event, and disables a follow Discord refuses', async () => {
  const env = await environment({ DISCORD_BOT_TOKEN: 'bot-token-test' });
  // Follows: Bravo (ages_10) in a normal channel; the creator in a normal channel and in an age-restricted one.
  await run(env, follow('space', [{ type: 3, name: 'space', value: sim(2) }]));
  await run(env, follow('creator', [{ type: 3, name: 'username', value: 'dee' }]));
  await run(env, follow('creator', [{ type: 3, name: 'username', value: 'dee' }], { nsfw: true, channel: RESTRICTED_CHANNEL }));

  const start = Date.parse('2026-11-14T12:00:00.000Z');
  calls.length = 0;
  await tick(env, start);
  assert.equal(callsTo('/applications/').length, 2, 'commands and metadata registered on the first tick');
  assert.equal(posts().length, 0, 'the first tick only sets the cursor');

  // Bravo, Delta (mature_17), Echo (adult_18) and Foxtrot (no rating) are updated after the cursor.
  const later = new Date(start + 60_000).toISOString();
  for (const n of [2, 4, 5, 6]) await putListing(env, LISTINGS[n - 1], { updated_at: later, version: 2 });
  calls.length = 0;
  await tick(env, start + 300_000);
  assert.equal(callsTo('/applications/').length, 0, 'unchanged definitions are not PUT again');
  const made = posts().map((c) => [c.url.split('/')[6], c.body.embeds[0].title, c.body.content]).sort();
  assert.deepEqual(made, [
    // The normal channel follows both Bravo and its creator: one post. Delta (mature_17) only where age-restricted;
    // Echo (adult_18) and Foxtrot (no rating) nowhere.
    [CHANNEL, 'Bravo Bridge', 'Updated on Eustress'],
    [RESTRICTED_CHANNEL, 'Bravo Bridge', 'Updated on Eustress'],
    [RESTRICTED_CHANNEL, 'Delta Dunes', 'Updated on Eustress'],
  ].sort());
  assert.ok(posts().every((c) => c.init.headers.Authorization === 'Bot bot-token-test' && c.body.allowed_mentions.parse.length === 0));

  // The next tick re-reads the overlap and posts nothing twice.
  calls.length = 0;
  await tick(env, start + 600_000);
  assert.equal(posts().length, 0);

  // Bravo goes live: posted once to each follow, and not again while the same session lasts.
  await goLive(env, sim(2), 3);
  responder = (url) => (url.includes(`/channels/${RESTRICTED_CHANNEL}/`) ? new Response('{"message":"Missing Access"}', { status: 403 }) : undefined);
  calls.length = 0;
  await tick(env, start + 900_000);
  assert.deepEqual(posts().map((c) => [c.url.split('/')[6], c.body.content]).sort(), [[CHANNEL, 'Live now on Eustress'], [RESTRICTED_CHANNEL, 'Live now on Eustress']].sort());
  const disabled = JSON.parse(await env.SOCIAL.get(`discordfollow:creator:${DEE}:${GUILD}:${RESTRICTED_CHANNEL}`));
  assert.equal(disabled.disabled_status, 403);
  responder = () => undefined;
  calls.length = 0;
  await tick(env, start + 1_200_000);
  assert.equal(posts().length, 0, 'deduped, and the refused follow is not tried again');
  const list = await run(env, follow('list'));
  assert.match(list.message.content, /paused: Eustress cannot post there/);
});

test('a run posts at most POSTS_PER_RUN and the rest follow on the next run; a failing read never throws out of the tick', async () => {
  const env = await environment({ DISCORD_BOT_TOKEN: 'bot-token-test' });
  for (let i = 0; i < POSTS_PER_RUN + 5; i++) {
    await env.SOCIAL.put(`discordfollow:space:${sim(2)}:${GUILD}:3100000000000${String(i).padStart(5, '0')}`, JSON.stringify({ kind: 'space', target: sim(2), name: 'Bravo Bridge', guild_id: GUILD, channel_id: `3100000000000${String(i).padStart(5, '0')}`, age_restricted: false }));
  }
  const views = [{ id: sim(2), name: 'Bravo Bridge', author_id: DEE, author_name: 'dee', rating: 'ages_10', version: 2, updated_at: '2026-11-14T12:01:00.000Z' }];
  const deps = { listViews: async () => views, liveSessions: async () => [] };
  await env.SOCIAL.put('discordsweep:cursor', '2026-11-14T12:00:00.000Z');
  calls.length = 0;
  const first = await sweepDiscordFollows(env, deps, { now: Date.parse('2026-11-14T12:05:00.000Z') });
  assert.equal(first.posted, POSTS_PER_RUN);
  assert.equal(first.budget_spent, true);
  assert.equal(await env.SOCIAL.get('discordsweep:cursor'), '2026-11-14T12:00:00.000Z', 'the unfinished update is read again');
  const second = await sweepDiscordFollows(env, deps, { now: Date.parse('2026-11-14T12:10:00.000Z') });
  assert.equal(second.posted, 5);
  assert.equal(second.budget_spent, false);
  assert.equal(posts().length, POSTS_PER_RUN + 5);

  const broken = { listViews: async () => { throw new Error('storage down'); }, liveSessions: async () => { throw new Error('down'); } };
  const result = await discordTick(env, broken, { now: Date.now() });
  assert.equal(result.sweep, undefined);
});

test('the tick pushes Linked Roles again for a linked creator whose listed Spaces changed', async () => {
  const env = await environment({ DISCORD_BOT_TOKEN: 'bot-token-test' });
  // Dee linked with Linked Roles: a kept refresh token, last pushed with 0 Spaces.
  const sealed = await sealDiscordToken(env, DEE, 'rt-dee-1');
  await env.USERS.put(`discordtoken:${DEE}`, JSON.stringify({ ...sealed, spaces: 0, meta_hash: null }));
  await env.SOCIAL.put('discordsweep:counts', JSON.stringify({}));
  responder = (url, init) => {
    if (url === DISCORD_TOKEN) {
      const form = new URLSearchParams(init.body);
      return form.get('refresh_token') === 'rt-dee-1' ? new Response(JSON.stringify({ access_token: 'at-dee', refresh_token: 'rt-dee-2' }), { status: 200 }) : new Response('{"error":"invalid_grant"}', { status: 400 });
    }
    return undefined;
  };
  calls.length = 0;
  await tick(env, Date.now());
  responder = () => undefined;
  const pushes = calls.filter((c) => c.url === roleConnectionUrl(APP_ID));
  assert.equal(pushes.length, 1);
  assert.equal(pushes[0].init.headers.Authorization, 'Bearer at-dee');
  // Dee has six listings; every listed one counts, whatever its rating.
  assert.deepEqual(pushes[0].body.metadata, { verified: 1, creator: 1, spaces: 6, member_since: '2026-01-01T00:00:00.000Z' });
  assert.equal(await openDiscordToken(env, DEE, JSON.parse(await env.USERS.get(`discordtoken:${DEE}`))), 'rt-dee-2', 'the rotated refresh token is kept');
  assert.deepEqual(JSON.parse(await env.SOCIAL.get('discordsweep:counts')), { [DEE]: 6 });
  calls.length = 0;
  await tick(env, Date.now());
  assert.equal(calls.filter((c) => c.url === roleConnectionUrl(APP_ID)).length, 0, 'unchanged counts push nothing');
});
