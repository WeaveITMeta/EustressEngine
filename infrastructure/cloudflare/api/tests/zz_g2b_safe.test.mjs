// Security, guest round 2: checks that PASS on the current code (the SAFE list). Not defects.
import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { JWT_SECRET } from './helpers/load_worker.mjs';
import { environment, seed, listing, call, player, guestBeat, accountBeat, seat, chat, withClock, withFetch, jevAnswering, quietly, IP, SIM, SIM_B, PIN, PIN2, PLAYER } from './zz_g2b_common.mjs';

const b64url = (b) => Buffer.from(b).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
function token(sub, { exp, secret = JWT_SECRET } = {}) {
  const now = Math.floor(Date.now() / 1000);
  const h = b64url(JSON.stringify({ alg: 'HS256', typ: 'JWT' }));
  const p = b64url(JSON.stringify({ sub, iat: now - 7200, exp: exp ?? now + 3600 }));
  return `${h}.${p}.${b64url(crypto.createHmac('sha256', secret).update(`${h}.${p}`).digest())}`;
}
const TEEN = 'cccccccc-3333-4333-8333-cccccccccccc';

function everythingStored(env) {
  const parts = [];
  for (const v of Object.values(env)) {
    if (v?.store instanceof Map) for (const [k, x] of v.store) parts.push(k, String(x));
    if (v?.objects instanceof Map) for (const [k, o] of v.objects) parts.push(k, new TextDecoder().decode(o.body));
    if (v?.instances instanceof Map) for (const [name, o] of v.instances) { parts.push(name); for (const [k, x] of o.storage.map) parts.push(k, JSON.stringify(x)); }
  }
  return parts.join('\n');
}

test('SAFE: an expired, forged or garbage token is signed out for the ages_10 cap on list, page, download, play and live', async () => {
  const env = environment();
  await seed(env);
  await env.SOCIAL.put(`sim:${TEEN}`, JSON.stringify(listing(TEEN, { rating: 'teen_13' }, { r2_key: `universes/${TEEN}/universe.pak` })));
  await env.SCENES.put(`universes/${TEEN}/universe.pak`, 'the world');
  const bad = { expired: token(PLAYER, { exp: Math.floor(Date.now() / 1000) - 10 }), forged: token(PLAYER, { secret: 'wrong' }), garbage: 'not.a.token' };
  for (const [what, t] of Object.entries(bad)) {
    const list = (await call(env, 'GET', '/api/simulations', { token: t })).body.simulations.map((s) => s.id);
    assert.ok(!list.includes(TEEN) && list.includes(SIM), `${what}: list`);
    for (const [m, path] of [['GET', `/api/simulations/${TEEN}`], ['GET', `/api/simulations/${TEEN}/download`], ['POST', `/api/simulations/${TEEN}/play`], ['GET', `/api/simulations/${TEEN}/live`]]) {
      const r = await call(env, m, path, { token: t, headers: IP });
      assert.deepEqual([r.status, r.body?.code], [403, 'sign_in_required'], `${what}: ${m} ${path}`);
    }
  }
  for (const [m, path] of [['GET', `/api/simulations/${TEEN}`], ['GET', `/api/simulations/${TEEN}/download`], ['POST', `/api/simulations/${TEEN}/play`]]) {
    assert.equal((await call(env, m, path, { token: player })).status, 200, `signed in: ${m} ${path}`);
  }
  // What a signed-out visitor learns: 404 for a missing or unlisted id, 403 sign_in_required for a listed one above ages_10.
  const missing = await call(env, 'GET', '/api/simulations/eeeeeeee-0000-4000-8000-000000000000');
  console.log(`signed out: missing ${missing.status}; teen ${(await call(env, 'GET', `/api/simulations/${TEEN}`)).status}`);
});

test('SAFE: the guest index lists only guest-only G to PG sessions and carries no link, key, pin, author id or guest datum', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async () => {
    const env = environment();
    await seed(env);
    await env.SOCIAL.put(`sim:${TEEN}`, JSON.stringify(listing(TEEN, { rating: 'teen_13' })));
    assert.equal((await guestBeat(env, {}, SIM, PIN)).status, 200);
    await seat(env);
    assert.equal((await accountBeat(env, {}, SIM_B, PIN2)).status, 200);
    assert.equal((await guestBeat(env, {}, TEEN, PIN2)).status, 200);
    const r = await call(env, 'GET', '/api/guest/live', { headers: IP });
    console.log(JSON.stringify(r.body));
    assert.deepEqual(r.body.sessions.map((s) => s.id), [SIM]);
    const text = JSON.stringify(r.body);
    for (const secret of ['eustress-player://', PIN, '0123456789abcdef0123456789abcdef', '11111111-1111-4111-8111-111111111111', 'gid', 'tid']) assert.ok(!text.includes(secret), secret);
    const allowed = ['author_name', 'cover_url', 'genre', 'id', 'max_players', 'name', 'players', 'region', 'since', 'tags', 'tags_source', 'thumbnail_url'];
    assert.deepEqual(Object.keys(r.body.sessions[0]).filter((k) => !allowed.includes(k)), []);
  });
});

test('SAFE: a refused and an unavailable chat check log and store no text', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async () => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const one = await seat(env, IP);
    const two = await seat(env, { 'CF-Connecting-IP': '198.51.100.9' });
    const { out: refused, lines: l1 } = await quietly(() => withFetch(jevAnswering({ threat: 0.99 }), () => chat(env, one.id, 'marker-quokka-391 refused')));
    assert.equal(refused.body.end_guest, true);
    const { out: down, lines: l2 } = await quietly(() => withFetch(async () => new Response('echo marker-quokka-392', { status: 500 }), () => chat(env, two.id, 'marker-quokka-392 down')));
    assert.deepEqual([down.status, down.body.code], [503, 'check_unavailable']);
    const logs = [...l1, ...l2].join('\n');
    console.log(logs);
    assert.ok(!logs.includes('quokka') && !everythingStored(env).includes('quokka'));
  });
});

test('SAFE: the world routes and the makers and tags lists apply the signed-out cap', async () => {
  const env = environment();
  await seed(env);
  await env.SOCIAL.put(`sim:${TEEN}`, JSON.stringify(listing(TEEN, { rating: 'teen_13' }, { format: 'echk', r2_key: `universes/${TEEN}/world/manifest.json`, tags: ['racing'] })));
  for (const path of [`/api/simulations/${TEEN}/world/manifest`, `/api/simulations/${TEEN}/world/chunks/${"a".repeat(64)}`, `/api/simulations/${TEEN}/world/source`]) {
    const r = await call(env, 'GET', path, { headers: IP });
    console.log(`signed out ${path.split(TEEN)[1]}: ${r.status} ${r.body?.code}`);
    assert.equal(r.status, 403, path);
  }
  const tags = await call(env, 'GET', '/api/simulations/tags');
  assert.ok(!JSON.stringify(tags.body).includes('racing'), 'tags signed out');
  const makers = await call(env, 'GET', '/api/makers');
  console.log(`makers signed out: ${makers.status} ${JSON.stringify(makers.body).slice(0, 200)}`);
});
