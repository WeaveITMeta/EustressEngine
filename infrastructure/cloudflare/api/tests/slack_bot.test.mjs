// The Eustress Slack app (src/slack_bot.mjs) through the real Worker: the request-signature gate on the three POST
// routes, the app id check, every subcommand of /eustress, the rating cap for each place a command can be used, the
// follow checks (installed, installer or admin), the OAuth v2 install with its one-time state and sealed bot token,
// uninstall and revoked tokens, the Home tab, and the follow posts the five-minute tick makes.
// Storage is in memory; Slack is a stand-in on globalThis.fetch, and nothing else is reachable.

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
  verifySlackSignature, sealSlackBotToken, openSlackBotToken, commandScope, sweepSlackFollows, slackTick,
  NOT_LINKED_PROFILE, NOT_LINKED_ME, NO_MATCH, POSTS_PER_RUN, SLACK_BOT_SCOPE, commandText,
} from '../src/slack_bot.mjs';

const worker = await loadWorker();
const API = 'https://api.eustress.dev';
const SITE = 'https://eustress.dev';
const APP_ID = 'A0CHNKPQCKX';
const SIGNING_SECRET = 'test-slack-signing-secret';
const CLIENT_SECRET = 'test-slack-client-secret';
const TEAM = 'T0TEAM0001';
const OTHER_TEAM = 'T0TEAM0002';
const ENTERPRISE = 'E0ORG00001';
const CHANNEL = 'C0CHAN0001';
const CHANNEL_2 = 'C0CHAN0002';
const BOT_TOKEN = 'xoxb-1111-2222-testbottoken';

// ── Signing ──────────────────────────────────────────────────────────────────

const nowSeconds = () => String(Math.floor(Date.now() / 1000));
const sign = (timestamp, body, secret = SIGNING_SECRET) => `v0=${nodeCrypto.createHmac('sha256', secret).update(`v0:${timestamp}:${body}`).digest('hex')}`;

// ── Slack, stood in ──────────────────────────────────────────────────────────

const calls = [];
/// (url, init) -> Response, or undefined for the default.
let responder = () => undefined;
function parsedBody(init) {
  const b = init?.body;
  if (typeof b !== 'string') return b;
  if (b.startsWith('{') || b.startsWith('[')) return JSON.parse(b);
  return Object.fromEntries(new URLSearchParams(b));
}
globalThis.fetch = async (input, init = {}) => {
  const url = typeof input === 'string' ? input : input.url;
  if (!url.startsWith('https://slack.com/') && !url.startsWith('https://hooks.slack.com/')) return new Response('{"error":"offline in tests"}', { status: 503 });
  calls.push({ url, init, body: parsedBody(init) });
  const custom = await responder(url, init);
  if (custom) return custom;
  if (url.startsWith('https://hooks.slack.com/')) return new Response('ok', { status: 200 });
  return Response.json({ ok: true });
};
const callsTo = (fragment) => calls.filter((c) => c.url.includes(fragment));
const slackOk = (body) => Response.json({ ok: true, ...body });
const slackError = (error) => Response.json({ ok: false, error });

// ── Accounts and listings ────────────────────────────────────────────────────

const ADA = 'a1111111-1111-4111-8111-111111111111'; // linked, verified adult, Adult content off
const BEA = 'a2222222-2222-4222-8222-222222222222'; // linked, verified adult, Adult content on
const CAM = 'a3333333-3333-4333-8333-333333333333'; // linked, no verified age
const DEE = 'a4444444-4444-4444-8444-444444444444'; // the creator, linked
const SLACK = { ada: 'U0ADA00001', bea: 'U0BEA00001', cam: 'U0CAM00001', dee: 'U0DEE00001', nobody: 'U0NOBODY01', admin: 'U0ADMIN001', installer: 'U0INSTALL1' };
const DM = { ada: 'D0ADA00001', bea: 'D0BEA00001', cam: 'D0CAM00001', nobody: 'D0NOB00001' };

const sim = (n) => `cccccccc-0000-4000-8000-00000000000${n}`;
const LISTINGS = [
  { id: sim(1), name: 'Alpha Garden', rating: 'all_ages' },
  { id: sim(2), name: 'Bravo Bridge', rating: 'ages_10' },
  { id: sim(3), name: 'Charlie Canyon', rating: 'teen_13' },
  { id: sim(4), name: 'Delta Dunes', rating: 'mature_17' },
  { id: sim(5), name: 'Echo Lounge', rating: 'adult_18' },
  { id: sim(6), name: 'Foxtrot Unknown', rating: null },
];
const T0 = '2026-11-01T00:00:00.000Z';

async function putListing(env, l, extra = {}) {
  await env.SOCIAL.put(`sim:${l.id}`, JSON.stringify({
    id: l.id, name: l.name, description: `${l.name}, a Space`, author_id: DEE, author_name: 'dee', is_public: true,
    moderation: { status: 'approved', ...(l.rating ? { rating: l.rating } : {}) },
    thumbnail_url: `${API}/api/simulations/${l.id}/thumbnail`, version: 1, published_at: T0, updated_at: T0, ...extra,
  }));
}

async function linkSlack(env, accountId, slackUser, team = TEAM) {
  await env.USERS.put(`slacklink:${accountId}`, JSON.stringify({ team_id: team, user_id: slackUser, name: 'x', team_name: 'Team', linked_at: new Date().toISOString() }));
  await env.USERS.put(`slack:${team}:${slackUser}`, accountId);
}

async function environment(extra = {}) {
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: API,
    SLACK_APP_ID: APP_ID, SLACK_SIGNING_SECRET: SIGNING_SECRET, SLACK_CLIENT_ID: '1111.2222', SLACK_CLIENT_SECRET: CLIENT_SECRET, ...extra,
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
    await linkSlack(env, id, SLACK[name]);
  }
  for (const id of [ADA, BEA, DEE]) await env.KYC_STATUS.put(`kyc-${id}-front`, JSON.stringify({ status: 'verified', extracted_dob: '1990-01-01' }));
  await env.USERS.put(`adult-content:${BEA}`, '1');
  for (const l of LISTINGS) await putListing(env, l);
  // Slack has told the app each member's DM with it (app_home_opened).
  for (const who of ['ada', 'bea', 'cam', 'nobody']) await env.SOCIAL.put(`slackappdm:${TEAM}:${SLACK[who]}`, DM[who]);
  return env;
}

/// An installation as the OAuth redirect stores it, for a workspace (T...) or an organisation (E...).
async function install(env, id = TEAM, { installer = SLACK.installer, enterprise = false, token = BOT_TOKEN } = {}) {
  await env.USERS.put(`slackinstall:${id}`, JSON.stringify({
    v: 1, install_id: id, is_enterprise_install: enterprise, team_id: enterprise ? null : id, enterprise_id: enterprise ? id : null,
    bot_user_id: 'U0BOT00001', scope: SLACK_BOT_SCOPE, installer_user_id: installer, installed_at: T0,
    token: await sealSlackBotToken(env, id, token),
  }));
}

async function goLive(env, simId, players = 3) {
  const stub = env.LIVE_INDEX.get(env.LIVE_INDEX.idFromName('all'));
  const now = Date.now();
  const res = await stub.fetch('https://live-index.internal/beat', { method: 'POST', body: JSON.stringify({ sim_id: simId, author: DEE, players, max_players: 10, region: null, since: now - 1000, now }) });
  assert.equal(res.status, 200);
}

// ── Requests ─────────────────────────────────────────────────────────────────

/// POST a body to a Slack route, signed (or not), through the real Worker. `settled` waits for deferred work.
async function post(env, path, text, { contentType = 'application/x-www-form-urlencoded', timestamp = nowSeconds(), sig, headers = {}, omit = [] } = {}) {
  const h = { 'Content-Type': contentType, 'X-Slack-Request-Timestamp': timestamp, 'X-Slack-Signature': sig ?? sign(timestamp, text), ...headers };
  for (const o of omit) delete h[o];
  const pending = [];
  const ctx = { waitUntil(p) { pending.push(p); }, passThroughOnException() {} };
  const res = await worker.fetch(new Request(`${API}${path}`, { method: 'POST', headers: h, body: text }), env, ctx);
  const raw = await res.text();
  let body = null;
  try { body = raw ? JSON.parse(raw) : null; } catch { body = raw; }
  return { status: res.status, body, raw, settled: () => Promise.all(pending) };
}

const RESPONSE_URL = 'https://hooks.slack.com/commands/T0TEAM0001/123/abcdef';

function commandForm({ text = '', command = '/eustress', user = SLACK.nobody, channel = CHANNEL, channelName = 'general', team = TEAM, enterprise, appId = APP_ID, responseUrl = RESPONSE_URL } = {}) {
  return new URLSearchParams({
    token: 'deprecated-verification-token', team_id: team, ...(enterprise ? { enterprise_id: enterprise } : {}), team_domain: 'team',
    channel_id: channel, channel_name: channelName, user_id: user, user_name: 'someone', command, text,
    api_app_id: appId, is_enterprise_install: 'false', response_url: responseUrl, trigger_id: '1.2.3',
  }).toString();
}

/// Run /eustress to its finished reply: the immediate answer, then what was posted to the response_url.
async function run(env, opts) {
  calls.length = 0;
  const r = await post(env, '/api/slack/commands', commandForm(opts));
  await r.settled();
  const posted = callsTo('hooks.slack.com/commands/');
  if (r.raw === '') {
    assert.equal(r.status, 200);
    assert.equal(posted.length, 1, 'one reply posted to the response_url');
    return { deferred: true, message: posted[0].body };
  }
  assert.equal(posted.length, 0);
  return { deferred: false, status: r.status, message: r.body };
}
const inApp = (who) => ({ user: SLACK[who], channel: DM[who], channelName: 'directmessage' });
const blockText = (message) => JSON.stringify(message.blocks || []);
const buttons = (message) => (message.blocks || []).flatMap((b) => (b.type === 'actions' ? b.elements : b.accessory?.type === 'button' ? [b.accessory] : []));
const images = (message) => (message.blocks || []).filter((b) => b.accessory?.type === 'image').map((b) => b.accessory.image_url);

/// The listings `space` can find for one place: each listing asked for by id.
async function visible(env, opts) {
  const out = [];
  for (const l of LISTINGS) {
    const r = await run(env, { ...opts, text: `space ${l.id}` });
    if (r.message.text !== NO_MATCH) out.push(l.id);
  }
  return out.sort();
}
const ids = (...n) => n.map(sim).sort();

// ── The routes and the signature gate ────────────────────────────────────────

test('every Slack route is public in the route policy: the signature or the one-time state is its gate', () => {
  for (const path of ['/api/slack/commands', '/api/slack/events', '/api/slack/interactivity']) {
    const p = routePolicy('POST', path);
    assert.equal(p.class, PUBLIC, path);
    assert.equal(p.explicit, true, path);
    assert.match(p.why, /signature/, path);
  }
  for (const path of ['/api/slack/install', '/api/slack/oauth_redirect']) {
    const p = routePolicy('GET', path);
    assert.equal(p.class, PUBLIC, path);
    assert.equal(p.explicit, true, path);
    assert.match(p.why, /state/, path);
  }
});

test('a validly signed request is answered; a tampered body, a stale or tampered timestamp, missing headers or another secret are 401', async () => {
  const env = await environment();
  const text = commandForm({ text: 'help' });
  assert.equal((await post(env, '/api/slack/commands', text)).status, 200);

  const timestamp = nowSeconds();
  const sig = sign(timestamp, text);
  const tampered = await post(env, '/api/slack/commands', text.replace('help', 'live'), { timestamp, sig });
  assert.equal(tampered.status, 401);
  assert.deepEqual(tampered.body, { error: 'invalid request signature' });
  assert.equal((await post(env, '/api/slack/commands', text, { timestamp: String(Number(timestamp) - 1), sig })).status, 401, 'tampered timestamp');
  // Signed correctly, but six minutes old or six minutes ahead: a replay.
  for (const skew of [-360, 360]) {
    const ts = String(Math.floor(Date.now() / 1000) + skew);
    assert.equal((await post(env, '/api/slack/commands', text, { timestamp: ts, sig: sign(ts, text) })).status, 401, `skew ${skew}`);
  }
  // Four minutes old is inside the window.
  const recent = String(Math.floor(Date.now() / 1000) - 240);
  assert.equal((await post(env, '/api/slack/commands', text, { timestamp: recent, sig: sign(recent, text) })).status, 200);
  for (const omit of [['X-Slack-Signature'], ['X-Slack-Request-Timestamp'], ['X-Slack-Signature', 'X-Slack-Request-Timestamp']]) {
    assert.equal((await post(env, '/api/slack/commands', text, { omit })).status, 401, omit.join('+'));
  }
  assert.equal((await post(env, '/api/slack/commands', text, { sig: sign(nowSeconds(), text, 'another-secret') })).status, 401);
  assert.equal((await post(env, '/api/slack/commands', text, { sig: 'v0=zz' })).status, 401);
  assert.equal((await post(env, '/api/slack/commands', text, { sig: sign(timestamp, text).replace('v0=', 'v1=') })).status, 401);
  for (const path of ['/api/slack/events', '/api/slack/interactivity']) {
    assert.equal((await post(env, path, '{"type":"url_verification","challenge":"c"}', { sig: 'v0=' + '0'.repeat(64) })).status, 401, path);
  }
  // Nothing was done for any refused request.
  assert.equal(calls.filter((c) => c.url.includes('hooks.slack.com')).length, 0);
});

test('without SLACK_SIGNING_SECRET (or SLACK_APP_ID) the POST routes answer 503; an oversize body 413; a GET 405', async () => {
  for (const missing of [{ SLACK_SIGNING_SECRET: '' }, { SLACK_APP_ID: '' }]) {
    const env = await environment(missing);
    for (const path of ['/api/slack/commands', '/api/slack/events', '/api/slack/interactivity']) {
      assert.equal((await post(env, path, commandForm({ text: 'help' }))).status, 503, `${path} ${Object.keys(missing)}`);
    }
  }
  const env = await environment();
  const big = 'x'.repeat(64 * 1024 + 1);
  assert.equal((await post(env, '/api/slack/commands', big)).status, 413);
  const res = await worker.fetch(new Request(`${API}/api/slack/commands`), env, { waitUntil() {}, passThroughOnException() {} });
  assert.equal(res.status, 405);
});

test('the verifier checks the exact bytes signed', async () => {
  const ts = nowSeconds();
  const sig = sign(ts, 'a=1');
  assert.equal(await verifySlackSignature(SIGNING_SECRET, sig, ts, 'a=1'), true);
  assert.equal(await verifySlackSignature(SIGNING_SECRET, sig, ts, new TextEncoder().encode('a=1')), true);
  assert.equal(await verifySlackSignature(SIGNING_SECRET, sig, ts, 'a=1 '), false);
  assert.equal(await verifySlackSignature(SIGNING_SECRET, sig, ts, 'a=2'), false);
  assert.equal(await verifySlackSignature('', sig, ts, 'a=1'), false);
  assert.equal(await verifySlackSignature(SIGNING_SECRET, sig.toUpperCase(), ts, 'a=1'), false);
  assert.equal(await verifySlackSignature(SIGNING_SECRET, sig, `${ts}0`, 'a=1'), false);
});

test('url_verification answers the challenge once signed; a request for another app id is refused 403', async () => {
  const env = await environment();
  const r = await post(env, '/api/slack/events', JSON.stringify({ token: 'x', challenge: 'chal-123', type: 'url_verification' }), { contentType: 'application/json' });
  assert.equal(r.status, 200);
  assert.deepEqual(r.body, { challenge: 'chal-123' });

  calls.length = 0;
  const wrongCommand = await post(env, '/api/slack/commands', commandForm({ text: 'live', appId: 'A0OTHERAPP1' }));
  await wrongCommand.settled();
  assert.equal(wrongCommand.status, 403);
  assert.equal(calls.length, 0, 'nothing posted');
  await install(env);
  const wrongEvent = await post(env, '/api/slack/events', JSON.stringify({ type: 'event_callback', api_app_id: 'A0OTHERAPP1', team_id: TEAM, event: { type: 'app_uninstalled' } }), { contentType: 'application/json' });
  assert.equal(wrongEvent.status, 403);
  assert.ok(await env.USERS.get(`slackinstall:${TEAM}`), 'not uninstalled by another app\'s event');
  const wrongAction = await post(env, '/api/slack/interactivity', new URLSearchParams({ payload: JSON.stringify({ type: 'block_actions', api_app_id: 'A0OTHERAPP1' }) }).toString());
  assert.equal(wrongAction.status, 403);
  const action = await post(env, '/api/slack/interactivity', new URLSearchParams({ payload: JSON.stringify({ type: 'block_actions', api_app_id: APP_ID, actions: [{ action_id: 'open_0' }] }) }).toString());
  assert.equal(action.status, 200);
  assert.equal(action.raw, '');
});

test('a response_url that is not Slack\'s is never posted to', async () => {
  const env = await environment();
  calls.length = 0;
  for (const responseUrl of ['https://evil.example/commands/x', 'http://hooks.slack.com/commands/x', 'https://hooks.slack.com.evil.example/commands/x', 'https://hooks.slack.com:8443/commands/x']) {
    const r = await post(env, '/api/slack/commands', commandForm({ text: 'live', responseUrl }));
    await r.settled();
    assert.equal(r.status, 200);
    assert.equal(r.body.response_type, 'ephemeral', responseUrl);
  }
  assert.equal(calls.length, 0);
});

// ── help ─────────────────────────────────────────────────────────────────────

test('help, an empty command and an unknown subcommand answer the command list at once, only to the asker', async () => {
  const env = await environment();
  for (const text of ['help', '', 'what']) {
    const r = await run(env, { text });
    assert.equal(r.deferred, false, text);
    assert.equal(r.message.response_type, 'ephemeral');
    assert.match(r.message.text, /\/eustress space/);
    assert.match(r.message.text, /simulation and data platform/);
    assert.match(r.message.text, new RegExp(`${SITE}/settings`));
    assert.ok(!/—/.test(r.message.text), 'no em dash');
    assert.ok(!/game engine/i.test(r.message.text));
  }
});

// ── The rating cap ───────────────────────────────────────────────────────────

test('the rating cap: ages_10 everywhere unless a linked verified adult asks in their own DM with the app', async () => {
  const env = await environment();
  const cmd = (who, over = {}) => ({ team_id: TEAM, user_id: SLACK[who], channel_id: CHANNEL, channel_name: 'general', ...over });
  const appDm = (who) => ({ channel_id: DM[who], channel_name: 'directmessage' });
  const cases = [
    ['unlinked, channel', cmd('nobody'), 'ages_10'],
    ['unlinked, app DM', cmd('nobody', appDm('nobody')), 'ages_10'],
    ['linked adult, channel', cmd('ada'), 'ages_10'],
    ['linked adult, private channel', cmd('ada', { channel_id: 'G0PRIV0001', channel_name: 'privategroup' }), 'ages_10'],
    ['linked adult, app DM', cmd('ada', appDm('ada')), 'mature_17'],
    ['linked adult, a DM with another person', cmd('ada', { channel_id: 'D0HUMAN001', channel_name: 'directmessage' }), 'ages_10'],
    ['linked adult, the app DM id but not named directmessage', cmd('ada', { channel_id: DM.ada, channel_name: 'privategroup' }), 'ages_10'],
    ['linked adult, a group DM', cmd('ada', { channel_id: 'G0MPDM0001', channel_name: 'mpdm-ada--bea-1' }), 'ages_10'],
    ['linked adult, another member\'s app DM', cmd('ada', appDm('bea')), 'ages_10'],
    ['linked adult, app DM in another workspace', cmd('ada', { ...appDm('ada'), team_id: OTHER_TEAM }), 'ages_10'],
    ['linked, no verified age, app DM', cmd('cam', appDm('cam')), 'ages_10'],
    ['linked adult with Adult content on, channel', cmd('bea'), 'ages_10'],
  ];
  for (const [label, c, want] of cases) assert.equal((await commandScope(env, c)).cap, want, label);
  const bea = await commandScope(env, cmd('bea', appDm('bea')));
  assert.ok(['adult_18', 'adult_21'].includes(bea.cap), `adult content on, app DM: ${bea.cap}`);
  // No app_home_opened seen for this member: no DM is known to be the app's, so ages_10.
  await env.SOCIAL.delete(`slackappdm:${TEAM}:${SLACK.ada}`);
  assert.equal((await commandScope(env, cmd('ada', appDm('ada')))).cap, 'ages_10', 'no recorded app DM');
});

test('the cap end to end: what space can find in a channel and in the app DM; unknown ratings never; adult cards carry no image', async () => {
  const env = await environment();
  assert.deepEqual(await visible(env, { user: SLACK.nobody }), ids(1, 2), 'unlinked, channel');
  assert.deepEqual(await visible(env, { user: SLACK.ada }), ids(1, 2), 'linked adult, channel');
  assert.deepEqual(await visible(env, inApp('ada')), ids(1, 2, 3, 4), 'linked adult, app DM');
  assert.deepEqual(await visible(env, inApp('cam')), ids(1, 2), 'no verified age, app DM');
  assert.deepEqual(await visible(env, inApp('bea')), ids(1, 2, 3, 4, 5), 'Adult content on, app DM; no rating never');
  const adult = await run(env, { ...inApp('bea'), text: 'space Echo Lounge' });
  assert.match(blockText(adult.message), /Echo Lounge/);
  assert.deepEqual(images(adult.message), [], 'no adult image in Slack');
  const mature = await run(env, { ...inApp('ada'), text: 'space Delta Dunes' });
  assert.deepEqual(images(mature.message), [`${API}/api/simulations/${sim(4)}/thumbnail`]);
});

test('a link revoked by a key replacement, a banned account, or a claim the account no longer shows is no link', async () => {
  const env = await environment();
  const adaDm = { team_id: TEAM, user_id: SLACK.ada, channel_id: DM.ada, channel_name: 'directmessage' };
  assert.equal((await commandScope(env, adaDm)).cap, 'mature_17');
  await new Promise((r) => setTimeout(r, 5));
  assert.equal((await accountCall(env, ADA, 'unlink_slack')).ok, true);
  assert.equal((await commandScope(env, adaDm)).cap, 'ages_10');
  assert.equal((await run(env, { text: `profile <@${SLACK.ada}|ada>` })).message.text, NOT_LINKED_PROFILE);

  const user = JSON.parse(await env.USERS.get(`user:${DEE}`));
  await env.USERS.put(`user:${DEE}`, JSON.stringify({ ...user, banned: true }));
  assert.equal((await run(env, { text: `profile <@${SLACK.dee}>` })).message.text, NOT_LINKED_PROFILE);
  await env.USERS.put(`user:${DEE}`, JSON.stringify(user));
  // Cam's claim stands, but Cam's own link now names another Slack member: no link.
  await env.USERS.put(`slacklink:${CAM}`, JSON.stringify({ team_id: TEAM, user_id: 'U0ELSE0001', linked_at: new Date().toISOString() }));
  assert.equal((await run(env, { text: `profile <@${SLACK.cam}>` })).message.text, NOT_LINKED_PROFILE);
});

// ── space ────────────────────────────────────────────────────────────────────

test('space posts in channel: the top match as a card with creator, rating, image, live players, Open and Play links, and up to four more', async () => {
  const env = await environment();
  for (let i = 0; i < 6; i++) await putListing(env, { id: `dddddddd-0000-4000-8000-0000000000${String(i).padStart(2, '0')}`, name: `Bravo Bridge Extra ${i}`, rating: 'all_ages' });
  await goLive(env, sim(2), 4);
  const r = await run(env, { text: 'space bravo bridge' });
  assert.equal(r.deferred, true);
  const m = r.message;
  assert.equal(m.response_type, 'in_channel');
  assert.equal(m.unfurl_links, false);
  const card = m.blocks.find((b) => b.type === 'section');
  assert.match(card.text.text, new RegExp(`<${SITE}/simulation/${sim(2)}\\|Bravo Bridge>`));
  assert.deepEqual(card.fields.map((f) => f.text), ['*Creator*\ndee', '*Rating*\nAges 10+', '*Live now*\n4 playing']);
  assert.equal(card.accessory.image_url, `${API}/api/simulations/${sim(2)}/thumbnail`);
  assert.deepEqual(buttons(m).map((b) => [b.text.text, b.url]), [['Open', `${SITE}/simulation/${sim(2)}`], ['Play', `${SITE}/play/${sim(2)}`]]);
  const more = m.blocks.at(-1).text.text;
  assert.match(more, /More matches/);
  assert.equal((more.match(/^• /gm) || []).length, 4, 'four more at most');
});

test('space never shows a listing above the channel cap, and a creator\'s text cannot mention or link anyone', async () => {
  const env = await environment();
  for (const query of [sim(3), 'Charlie Canyon', sim(4), sim(6), 'Foxtrot']) {
    const r = await run(env, { text: `space ${query}` });
    assert.equal(r.message.text, NO_MATCH, query);
    assert.equal(r.message.response_type, 'ephemeral');
    assert.equal(r.message.blocks, undefined);
  }
  await putListing(env, { id: sim(7), name: 'Zulu <!channel> <@U0ADA00001> <https://evil.example|x>', rating: 'all_ages' });
  const r = await run(env, { text: `space ${sim(7)}` });
  // Every mrkdwn text (an image alt_text is plain text, which Slack never reads as a mention or a link).
  const text = JSON.stringify(r.message.blocks.flatMap((x) => [x.text?.text, ...(x.fields || []).map((f) => f.text)]));
  assert.ok(!text.includes('<!channel>') && !text.includes('<@U0ADA00001>') && !text.includes('<https://evil'), text);
  assert.match(text, /&lt;!channel&gt;/);
});

test('each /eustress-<subcommand> reads as /eustress <subcommand>; other names pass their text through', async () => {
  assert.equal(commandText('/eustress-live', ''), 'live');
  assert.equal(commandText('/EUSTRESS-Space', 'Alpha Arena'), 'space Alpha Arena');
  assert.equal(commandText('/eustress-follow', 'creator someone'), 'follow creator someone');
  assert.equal(commandText('/eustress', 'live'), 'live');
  assert.equal(commandText('/eustress-what', 'live'), 'live');
  assert.equal(commandText('/eustress-live', undefined), 'live');
  const env = await environment();
  await goLive(env, sim(2), 2);
  const long = await run(env, { text: 'live' });
  const short = await run(env, { command: '/eustress-live', text: '' });
  assert.equal(short.deferred, true);
  assert.deepEqual(short.message, long.message);
  const help = await run(env, { command: '/eustress-help', text: '' });
  assert.equal(help.deferred, false);
  assert.match(help.message.text, /\/eustress-follow/);
  const follow = await run(env, { command: '/eustress-follow', text: 'list' });
  assert.match(follow.message.text, /must add Eustress|does not follow|Only the person/);
});

// ── live ─────────────────────────────────────────────────────────────────────

test('live lists listed live Spaces within the cap, each with a Play button', async () => {
  const env = await environment();
  const none = await run(env, { text: 'live' });
  assert.equal(none.message.text, 'No listed Spaces are live right now.');
  assert.equal(none.message.response_type, 'ephemeral');
  await goLive(env, sim(2), 2);
  await goLive(env, sim(4), 5);
  const channel = await run(env, { text: 'live' });
  assert.equal(channel.message.response_type, 'in_channel');
  assert.deepEqual(buttons(channel.message).map((b) => b.url), [`${SITE}/play/${sim(2)}`]);
  assert.ok(!blockText(channel.message).includes('Delta Dunes'));
  const dm = await run(env, { ...inApp('ada'), text: 'live' });
  assert.deepEqual(buttons(dm.message).map((b) => b.url).sort(), [`${SITE}/play/${sim(2)}`, `${SITE}/play/${sim(4)}`].sort());
});

// ── profile ──────────────────────────────────────────────────────────────────

test('profile shows a linked member\'s public profile in channel; an unlinked member, or one linked in another workspace, has not linked', async () => {
  const env = await environment();
  const r = await run(env, { text: `profile <@${SLACK.dee}|dee>` });
  assert.equal(r.message.response_type, 'in_channel');
  const card = r.message.blocks[0];
  assert.match(card.text.text, new RegExp(`<${SITE}/profile/dee\\|dee>`));
  assert.deepEqual(card.fields.map((f) => f.text), ['*Username*\ndee', '*Listed Spaces*\n2']);
  assert.deepEqual(buttons(r.message).map((b) => b.url), [`${SITE}/profile/dee`]);
  const unlinked = await run(env, { text: `profile <@${SLACK.nobody}|nobody>` });
  assert.equal(unlinked.message.text, NOT_LINKED_PROFILE);
  assert.equal(unlinked.message.response_type, 'ephemeral');
  const elsewhere = await run(env, { team: OTHER_TEAM, text: `profile <@${SLACK.dee}|dee>` });
  assert.equal(elsewhere.message.text, NOT_LINKED_PROFILE);
  const plain = await run(env, { text: 'profile dee' });
  assert.match(plain.message.text, /Mention someone/);
});

// ── me ───────────────────────────────────────────────────────────────────────

test('me is ephemeral: balances and unread notifications when linked, a Settings link when not', async () => {
  const env = await environment();
  const r = await run(env, { user: SLACK.ada, text: 'me' });
  assert.equal(r.message.response_type, 'ephemeral');
  const fields = r.message.blocks[0].fields.map((f) => f.text.split('\n')[0]);
  assert.deepEqual(fields, ['*Bliss*', '*Tickets*', '*Unread notifications*']);
  assert.match(r.message.blocks[0].fields[2].text, /\n0$/);
  const unlinked = await run(env, { text: 'me' });
  assert.equal(unlinked.message.response_type, 'ephemeral');
  assert.ok(unlinked.message.text.startsWith(NOT_LINKED_ME));
  assert.match(unlinked.message.text, new RegExp(`<${SITE}/settings\\|`));
});

// ── follow ───────────────────────────────────────────────────────────────────

const usersInfo = (flags) => (url, init) => {
  if (!url.endsWith('/users.info')) return undefined;
  const user = new URLSearchParams(init.body).get('user');
  return flags[user] === 'missing_scope' ? slackError('missing_scope') : slackOk({ user: { id: user, is_admin: flags[user] === 'admin', is_owner: flags[user] === 'owner' } });
};

test('follow needs the app installed, a channel, and the installer or a workspace admin or owner', async () => {
  const env = await environment();
  const notInstalled = await run(env, { user: SLACK.installer, text: 'follow space Bravo' });
  assert.match(notInstalled.message.text, /must add Eustress/);
  assert.equal(await env.SOCIAL.get(`slackfollowg:${TEAM}:${TEAM}:${CHANNEL}:space:${sim(2)}`), null);
  await install(env);
  responder = usersInfo({ [SLACK.admin]: 'admin', [SLACK.dee]: 'owner', [SLACK.ada]: 'member', [SLACK.cam]: 'missing_scope' });
  for (const who of [SLACK.ada, SLACK.cam]) {
    const refused = await run(env, { user: who, text: 'follow space Bravo' });
    assert.match(refused.message.text, /Only the person who added Eustress/, who);
    assert.equal(refused.message.response_type, 'ephemeral');
  }
  const info = callsTo('/api/users.info');
  assert.equal(info.at(-1).init.headers.Authorization, `Bearer ${BOT_TOKEN}`);
  const dm = await run(env, { ...inApp('ada'), user: SLACK.installer, text: 'follow space Bravo' });
  assert.match(dm.message.text, /in a channel/);
  for (const who of [SLACK.installer, SLACK.admin, SLACK.dee]) {
    const ok = await run(env, { user: who, text: 'follow space Bravo' });
    assert.match(ok.message.text, /now follows Bravo Bridge/, who);
    assert.match(ok.message.text, /\/invite @Eustress Engine\./);
  }
  assert.equal(callsTo('/api/users.info').filter((c) => c.body.user === SLACK.installer).length, 0, 'the installer is known without asking Slack');
  responder = () => undefined;
});

test('follow space, creator, list and remove; a Space rated above ages_10 cannot be followed', async () => {
  const env = await environment();
  await install(env);
  const as = (text, channel = CHANNEL) => run(env, { user: SLACK.installer, channel, text });
  assert.match((await as(`follow space ${sim(3)}`)).message.text, /rating is above/);
  assert.match((await as('follow space Bravo Bridge')).message.text, /now follows Bravo Bridge/);
  assert.match((await as('follow creator dee', CHANNEL_2)).message.text, /now follows dee/);
  assert.match((await as('follow creator nobody-here')).message.text, /No Eustress account/);
  const rec = JSON.parse(await env.SOCIAL.get(`slackfollow:space:${sim(2)}:${TEAM}:${TEAM}:${CHANNEL}`));
  assert.equal(rec.install_id, TEAM);
  assert.equal(rec.created_by, SLACK.installer);
  const list = await as('follow list');
  assert.match(list.message.text, new RegExp(`<#${CHANNEL}> space: Bravo Bridge`));
  assert.match(list.message.text, new RegExp(`<#${CHANNEL_2}> creator: dee`));
  assert.match((await as('follow remove dee')).message.text, /no follow by that name/, 'remove works in its own channel only');
  assert.match((await as('follow remove dee', CHANNEL_2)).message.text, /no longer follows dee/);
  assert.match((await as(`follow remove space:${sim(2)}`)).message.text, /no longer follows Bravo Bridge/);
  assert.match((await as('follow list')).message.text, /does not follow anything/);
  assert.match((await as('follow')).message.text, /follow space <name>/);
});

// ── Install ──────────────────────────────────────────────────────────────────

async function get(env, path, headers = {}) {
  return worker.fetch(new Request(`${API}${path}`, { headers }), env, { waitUntil() {}, passThroughOnException() {} });
}
const sha256 = (s) => nodeCrypto.createHash('sha256').update(s).digest('hex');

async function startInstall(env) {
  const res = await get(env, '/api/slack/install');
  assert.equal(res.status, 302);
  return new URL(res.headers.get('Location'));
}

const GRANT = {
  ok: true, app_id: APP_ID, authed_user: { id: SLACK.installer }, scope: 'commands,chat:write,users:read', token_type: 'bot',
  access_token: BOT_TOKEN, bot_user_id: 'U0BOT00001', team: { id: TEAM, name: 'Team One' }, enterprise: null, is_enterprise_install: false,
};

test('install start redirects to Slack with exactly the bot scopes, this Worker\'s redirect, and a state kept only hashed', async () => {
  const env = await environment();
  const to = await startInstall(env);
  assert.equal(`${to.origin}${to.pathname}`, 'https://slack.com/oauth/v2/authorize');
  assert.equal(to.searchParams.get('client_id'), '1111.2222');
  assert.equal(to.searchParams.get('scope'), 'commands,chat:write,users:read');
  assert.equal(to.searchParams.get('user_scope'), null);
  assert.equal(to.searchParams.get('redirect_uri'), `${API}/api/slack/oauth_redirect`);
  const state = to.searchParams.get('state');
  assert.match(state, /^[0-9a-f]{64}$/);
  const held = env.CHALLENGES.store.get(`slackinstallstate:${sha256(state)}`);
  assert.ok(held, 'the state is kept under its hash');
  assert.deepEqual(Object.keys(JSON.parse(held)).sort(), ['at', 'redirect_uri']);
  for (const [k, v] of env.CHALLENGES.store) assert.ok(!k.includes(state) && !v.includes(state), 'the raw state is not stored');
  const other = await startInstall(env);
  assert.notEqual(other.searchParams.get('state'), state);

  const unset = await environment({ SLACK_CLIENT_SECRET: '' });
  const res = await get(unset, '/api/slack/install');
  assert.equal(res.headers.get('Location'), `${SITE}/slack?install_error=slack_unavailable`);
  const limited = await environment({ AUTH_RATE_LIMITER: { limit: async () => ({ success: false }) } });
  assert.equal((await get(limited, '/api/slack/install', { 'CF-Connecting-IP': '203.0.113.9' })).headers.get('Location'), `${SITE}/slack?install_error=rate_limited`);
});

test('the redirect spends the state once, exchanges the code, keeps the bot token sealed, and sends the browser to the site', async () => {
  const env = await environment();
  const state = (await startInstall(env)).searchParams.get('state');
  responder = (url) => (url === 'https://slack.com/api/oauth.v2.access' ? Response.json(GRANT) : undefined);
  calls.length = 0;
  const res = await get(env, `/api/slack/oauth_redirect?code=1111.2222.abcdef&state=${state}`);
  assert.equal(res.status, 302);
  assert.equal(res.headers.get('Location'), `${SITE}/slack?installed=1`);
  const exchange = callsTo('/api/oauth.v2.access');
  assert.equal(exchange.length, 1);
  assert.deepEqual(exchange[0].body, { client_id: '1111.2222', client_secret: CLIENT_SECRET, code: '1111.2222.abcdef', redirect_uri: `${API}/api/slack/oauth_redirect` });

  const record = JSON.parse(await env.USERS.get(`slackinstall:${TEAM}`));
  assert.equal(record.install_id, TEAM);
  assert.equal(record.bot_user_id, 'U0BOT00001');
  assert.equal(record.installer_user_id, SLACK.installer);
  assert.equal(record.scope, 'commands,chat:write,users:read');
  assert.equal(record.is_enterprise_install, false);
  assert.ok(record.installed_at);
  assert.equal(record.access_token, undefined);
  // Sealed: nothing in any store holds the token in the clear.
  for (const kv of [env.USERS, env.SOCIAL, env.CHALLENGES]) for (const v of kv.store.values()) assert.ok(!v.includes(BOT_TOKEN) && !v.includes('xoxb-'), 'no plain token');
  assert.equal(await openSlackBotToken(env, TEAM, record.token), BOT_TOKEN);
  assert.equal(await openSlackBotToken(env, OTHER_TEAM, record.token), null, 'bound to its installation');
  assert.equal(await openSlackBotToken({ SLACK_CLIENT_SECRET: 'other' }, TEAM, record.token), null, 'bound to the secret');
  const tampered = { ...record.token, ct: Buffer.from(Buffer.from(record.token.ct, 'base64').map((b, i) => (i === 0 ? b ^ 1 : b))).toString('base64') };
  assert.equal(await openSlackBotToken(env, TEAM, tampered), null);

  // Single use: the same state again is refused before any exchange.
  calls.length = 0;
  const again = await get(env, `/api/slack/oauth_redirect?code=1111.2222.abcdef&state=${state}`);
  assert.equal(again.headers.get('Location'), `${SITE}/slack?install_error=state_expired`);
  assert.equal(callsTo('/api/oauth.v2.access').length, 0);
  assert.equal((await get(env, '/api/slack/oauth_redirect?code=1111.2222.abcdef&state=nothex')).headers.get('Location'), `${SITE}/slack?install_error=invalid_state`);
  assert.equal((await get(env, `/api/slack/oauth_redirect?code=1111.2222.abcdef&state=${'a'.repeat(64)}`)).headers.get('Location'), `${SITE}/slack?install_error=state_expired`);
  responder = () => undefined;
});

test('a refused exchange, a cancelled install, or a grant without a bot token stores nothing', async () => {
  const env = await environment();
  const attempt = async (grant, query = 'code=1111.2222.abcdef') => {
    const state = (await startInstall(env)).searchParams.get('state');
    responder = (url) => (url === 'https://slack.com/api/oauth.v2.access' ? Response.json(grant) : undefined);
    const res = await get(env, `/api/slack/oauth_redirect?${query}&state=${state}`);
    responder = () => undefined;
    assert.equal(env.CHALLENGES.store.get(`slackinstallstate:${sha256(state)}`), undefined, 'spent');
    return res.headers.get('Location');
  };
  assert.equal(await attempt({ ok: false, error: 'invalid_code' }), `${SITE}/slack?install_error=slack_code_rejected`);
  assert.equal(await attempt({ ok: false, error: 'internal_error' }), `${SITE}/slack?install_error=slack_failed`);
  assert.equal(await attempt({ ...GRANT, ok: 'true' }), `${SITE}/slack?install_error=slack_failed`);
  assert.equal(await attempt(GRANT, 'error=access_denied'), `${SITE}/slack?install_error=access_denied`);
  calls.length = 0;
  assert.equal(await attempt({ ...GRANT, token_type: 'user', access_token: 'xoxp-1-user' }), `${SITE}/slack?install_error=slack_failed`);
  assert.equal(callsTo('/api/auth.revoke').length, 1, 'a token that will not be kept is revoked');
  assert.equal(await attempt({ ...GRANT, app_id: 'A0OTHERAPP1' }), `${SITE}/slack?install_error=slack_failed`);
  assert.equal(await attempt({ ...GRANT, team: { id: 'not-a-team' } }), `${SITE}/slack?install_error=slack_failed`);
  assert.equal([...env.USERS.store.keys()].filter((k) => k.startsWith('slackinstall:')).length, 0);
});

test('an org-wide install is kept per enterprise and serves its workspaces', async () => {
  const env = await environment();
  const state = (await startInstall(env)).searchParams.get('state');
  responder = (url) => (url === 'https://slack.com/api/oauth.v2.access'
    ? Response.json({ ...GRANT, team: null, enterprise: { id: ENTERPRISE, name: 'Org' }, is_enterprise_install: true })
    : undefined);
  const res = await get(env, `/api/slack/oauth_redirect?code=1111.2222.abcdef&state=${state}`);
  responder = () => undefined;
  assert.equal(res.headers.get('Location'), `${SITE}/slack?installed=1`);
  assert.equal(await env.USERS.get(`slackinstall:${TEAM}`), null);
  const record = JSON.parse(await env.USERS.get(`slackinstall:${ENTERPRISE}`));
  assert.equal(record.is_enterprise_install, true);
  assert.equal(record.enterprise_id, ENTERPRISE);
  assert.equal(await openSlackBotToken(env, ENTERPRISE, record.token), BOT_TOKEN);
  const follow = await run(env, { team: OTHER_TEAM, enterprise: ENTERPRISE, user: SLACK.installer, text: 'follow space Bravo' });
  assert.match(follow.message.text, /now follows Bravo Bridge/);
  const without = await run(env, { team: OTHER_TEAM, user: SLACK.installer, text: 'follow space Bravo' });
  assert.match(without.message.text, /must add Eustress/, 'a workspace outside the organisation is not served');
});

// ── Events ───────────────────────────────────────────────────────────────────

const event = (env, ev, extra = {}, headers = {}) => post(env, '/api/slack/events', JSON.stringify({
  token: 'x', team_id: TEAM, api_app_id: APP_ID, type: 'event_callback', event_id: 'Ev01', event_time: 1, event: ev, ...extra,
}), { contentType: 'application/json', headers });

test('app_uninstalled and a revoked bot token remove the installation and its follows; a revoked user token alone does not', async () => {
  const env = await environment();
  await install(env);
  await run(env, { user: SLACK.installer, text: 'follow space Bravo' });
  assert.ok(await env.SOCIAL.get(`slackfollow:space:${sim(2)}:${TEAM}:${TEAM}:${CHANNEL}`));
  assert.equal((await event(env, { type: 'tokens_revoked', tokens: { oauth: ['U0ADA00001'] } })).status, 200);
  assert.ok(await env.USERS.get(`slackinstall:${TEAM}`));
  const r = await event(env, { type: 'app_uninstalled' });
  assert.equal(r.status, 200);
  assert.equal(await env.USERS.get(`slackinstall:${TEAM}`), null);
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('slackfollow')).length, 0);
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith(`slackappdm:${TEAM}:`)).length, 0);

  await install(env);
  await event(env, { type: 'tokens_revoked', tokens: { bot: ['U0BOT00001'] } }, {}, { 'X-Slack-Retry-Num': '1' });
  assert.equal(await env.USERS.get(`slackinstall:${TEAM}`), null, 'deleting is safe to repeat, so a retry is honoured');

  await install(env, ENTERPRISE, { enterprise: true });
  await event(env, { type: 'app_uninstalled' }, { enterprise_id: ENTERPRISE, is_enterprise_install: true });
  assert.equal(await env.USERS.get(`slackinstall:${ENTERPRISE}`), null);
});

test('app_home_opened records the member\'s app DM and publishes the Home tab with the bot token, once (not on a retry)', async () => {
  const env = await environment();
  await install(env);
  await env.SOCIAL.delete(`slackappdm:${TEAM}:${SLACK.ada}`);
  calls.length = 0;
  const r = await event(env, { type: 'app_home_opened', user: SLACK.ada, channel: DM.ada, tab: 'home' });
  await r.settled();
  assert.equal(r.status, 200);
  assert.equal(await env.SOCIAL.get(`slackappdm:${TEAM}:${SLACK.ada}`), DM.ada);
  const published = callsTo('/api/views.publish');
  assert.equal(published.length, 1);
  assert.equal(published[0].init.headers.Authorization, `Bearer ${BOT_TOKEN}`);
  assert.equal(published[0].body.user_id, SLACK.ada);
  assert.equal(published[0].body.view.type, 'home');
  const home = JSON.stringify(published[0].body.view);
  assert.match(home, /simulation and data platform/);
  assert.ok(home.includes(`${SITE}/slack`) && home.includes(`${SITE}/settings`));
  assert.ok(home.includes('/invite @Eustress Engine'));
  assert.ok(!/—/.test(home) && !/game engine/i.test(home));
  calls.length = 0;
  const retry = await event(env, { type: 'app_home_opened', user: SLACK.ada, channel: DM.ada, tab: 'home' }, {}, { 'X-Slack-Retry-Num': '1', 'X-Slack-Retry-Reason': 'http_timeout' });
  await retry.settled();
  assert.equal(retry.status, 200);
  assert.equal(callsTo('/api/views.publish').length, 0, 'a retry publishes nothing');
  // Slack refusing the token removes the installation.
  responder = (url) => (url.endsWith('/views.publish') ? slackError('invalid_auth') : undefined);
  await (await event(env, { type: 'app_home_opened', user: SLACK.ada, channel: DM.ada, tab: 'home' })).settled();
  responder = () => undefined;
  assert.equal(await env.USERS.get(`slackinstall:${TEAM}`), null);
});

// ── The five-minute tick ─────────────────────────────────────────────────────

async function tick(env, at) {
  const pending = [];
  await worker.scheduled({ cron: MODERATION_TICK_CRON, scheduledTime: at }, env, { waitUntil(p) { pending.push(p); }, passThroughOnException() {} });
  await Promise.all(pending);
}
const posts = () => callsTo('/api/chat.postMessage');
const followKey = (kind, target, channel) => `slackfollow:${kind}:${target}:${TEAM}:${TEAM}:${channel}`;
async function seedFollow(env, kind, target, channel, name) {
  await env.SOCIAL.put(followKey(kind, target, channel), JSON.stringify({ kind, target, name, install_id: TEAM, team_id: TEAM, channel_id: channel, created_by: SLACK.installer, created_at: T0 }));
  await env.SOCIAL.put(`slackfollowg:${TEAM}:${TEAM}:${channel}:${kind}:${target}`, '1');
}

test('the tick posts follow updates at ages_10 in every channel, once per channel and event, and pauses a channel Slack refuses', async () => {
  const env = await environment();
  await install(env);
  await seedFollow(env, 'space', sim(2), CHANNEL, 'Bravo Bridge');
  await seedFollow(env, 'creator', DEE, CHANNEL, 'dee');
  await seedFollow(env, 'creator', DEE, CHANNEL_2, 'dee');

  const start = Date.parse('2026-11-14T12:00:00.000Z');
  calls.length = 0;
  await tick(env, start);
  assert.equal(posts().length, 0, 'the first tick only sets the cursor');

  const later = new Date(start + 60_000).toISOString();
  for (const n of [2, 3, 4, 5, 6]) await putListing(env, LISTINGS[n - 1], { updated_at: later, version: 2 });
  calls.length = 0;
  await tick(env, start + 300_000);
  // Bravo (ages_10) only: Charlie (teen_13), Delta, Echo and Foxtrot (no rating) never reach a Slack channel.
  assert.deepEqual(posts().map((c) => [c.body.channel, c.body.text]).sort(), [
    [CHANNEL, 'Updated on Eustress: Bravo Bridge'],
    [CHANNEL_2, 'Updated on Eustress: Bravo Bridge'],
  ].sort());
  for (const c of posts()) {
    assert.equal(c.init.headers.Authorization, `Bearer ${BOT_TOKEN}`);
    assert.equal(c.body.unfurl_links, false);
    assert.deepEqual(buttons(c.body).map((b) => b.url), [`${SITE}/simulation/${sim(2)}`, `${SITE}/play/${sim(2)}`]);
  }

  calls.length = 0;
  await tick(env, start + 600_000);
  assert.equal(posts().length, 0, 'the overlap is read again and nothing is posted twice');

  await goLive(env, sim(2), 3);
  responder = (url, init) => (url.endsWith('/chat.postMessage') && JSON.parse(init.body).channel === CHANNEL_2 ? slackError('not_in_channel') : undefined);
  calls.length = 0;
  await tick(env, start + 900_000);
  responder = () => undefined;
  assert.deepEqual(posts().map((c) => [c.body.channel, c.body.text.split(':')[0]]).sort(), [[CHANNEL, 'Live now on Eustress'], [CHANNEL_2, 'Live now on Eustress']].sort());
  const paused = JSON.parse(await env.SOCIAL.get(followKey('creator', DEE, CHANNEL_2)));
  assert.equal(paused.disabled_reason, 'not_in_channel');
  calls.length = 0;
  await tick(env, start + 1_200_000);
  assert.equal(posts().length, 0, 'deduped, and the paused follow is not tried again');
  const list = await run(env, { user: SLACK.installer, text: 'follow list' });
  assert.match(list.message.text, /paused: the app is not in that channel\. Invite it with \/invite @Eustress Engine,/);
});

test('channel_not_found and is_archived pause a follow; invalid_auth removes the installation and its follows', async () => {
  const env = await environment();
  await install(env);
  await seedFollow(env, 'space', sim(2), CHANNEL, 'Bravo Bridge');
  await seedFollow(env, 'space', sim(2), CHANNEL_2, 'Bravo Bridge');
  const deps = { listViews: async () => [{ id: sim(2), name: 'Bravo Bridge', author_id: DEE, author_name: 'dee', rating: 'ages_10', version: 2, updated_at: '2026-11-14T12:01:00.000Z' }], liveSessions: async () => [] };
  await env.SOCIAL.put('slacksweep:cursor', '2026-11-14T12:00:00.000Z');
  responder = (url, init) => {
    if (!url.endsWith('/chat.postMessage')) return undefined;
    return slackError(JSON.parse(init.body).channel === CHANNEL ? 'channel_not_found' : 'is_archived');
  };
  const first = await sweepSlackFollows(env, deps, { now: Date.parse('2026-11-14T12:05:00.000Z') });
  assert.equal(first.disabled, 2);
  assert.equal(JSON.parse(await env.SOCIAL.get(followKey('space', sim(2), CHANNEL))).disabled_reason, 'channel_not_found');
  assert.equal(JSON.parse(await env.SOCIAL.get(followKey('space', sim(2), CHANNEL_2))).disabled_reason, 'is_archived');

  await seedFollow(env, 'space', sim(2), 'C0CHAN0003', 'Bravo Bridge');
  await env.SOCIAL.put('slacksweep:cursor', '2026-11-14T12:00:00.000Z');
  responder = (url) => (url.endsWith('/chat.postMessage') ? slackError('invalid_auth') : undefined);
  const second = await sweepSlackFollows(env, deps, { now: Date.parse('2026-11-14T12:10:00.000Z') });
  responder = () => undefined;
  assert.equal(second.removed_installs, 1);
  assert.equal(await env.USERS.get(`slackinstall:${TEAM}`), null);
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('slackfollow')).length, 0);
});

test('a run posts at most POSTS_PER_RUN and the rest on the next run; a failing read never throws out of the tick', async () => {
  const env = await environment();
  await install(env);
  for (let i = 0; i < POSTS_PER_RUN + 5; i++) await seedFollow(env, 'space', sim(2), `C1${String(i).padStart(8, '0')}`, 'Bravo Bridge');
  const deps = { listViews: async () => [{ id: sim(2), name: 'Bravo Bridge', author_id: DEE, author_name: 'dee', rating: 'ages_10', version: 2, updated_at: '2026-11-14T12:01:00.000Z' }], liveSessions: async () => [] };
  await env.SOCIAL.put('slacksweep:cursor', '2026-11-14T12:00:00.000Z');
  calls.length = 0;
  const first = await sweepSlackFollows(env, deps, { now: Date.parse('2026-11-14T12:05:00.000Z') });
  assert.equal(first.posted, POSTS_PER_RUN);
  assert.equal(first.budget_spent, true);
  assert.equal(await env.SOCIAL.get('slacksweep:cursor'), '2026-11-14T12:00:00.000Z', 'the unfinished update is read again');
  const second = await sweepSlackFollows(env, deps, { now: Date.parse('2026-11-14T12:10:00.000Z') });
  assert.equal(second.posted, 5);
  assert.equal(posts().length, POSTS_PER_RUN + 5);

  const broken = { listViews: async () => { throw new Error('storage down'); }, liveSessions: async () => { throw new Error('down'); } };
  const result = await slackTick(env, broken, { now: Date.now() });
  assert.deepEqual(result, { failed: true });
  assert.deepEqual(await slackTick({ ...env, SLACK_CLIENT_SECRET: '' }, broken), { skipped: true });
  const throwing = { ...env, SOCIAL: { get: async () => { throw new Error('kv down'); }, list: async () => { throw new Error('kv down'); }, put: async () => { throw new Error('kv down'); } } };
  await tick(throwing, Date.now());
});
