// The two doors that take no sign-in: the auth routes, which are limited per
// address, and the Stripe webhook, which moves money and so trusts nothing
// without a valid signature. Driven through the real Worker with fake storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, fakeEnv, ctx } from './helpers/load_worker.mjs';

const worker = await loadWorker();
const base = 'https://api.eustress.dev';

// ── Auth routes ──────────────────────────────────────────────────────────────

/// A limiter that lets `allowed` calls per key through, then refuses.
function fakeLimiter(allowed) {
  const used = new Map();
  return {
    used,
    async limit({ key }) {
      const n = (used.get(key) || 0) + 1;
      used.set(key, n);
      return { success: n <= allowed };
    },
  };
}

const post = (env, path, headers = {}, body = '{}') =>
  worker.fetch(new Request(`${base}${path}`, { method: 'POST', headers: { 'Content-Type': 'application/json', ...headers }, body }), env, ctx);

test('the three token-free auth routes are limited per address, and the limit is the same for all three', async () => {
  const env = { ...fakeEnv(), AUTH_RATE_LIMITER: fakeLimiter(3), CHALLENGES: fakeEnv().SOCIAL };
  const ip = { 'CF-Connecting-IP': '203.0.113.7' };
  const paths = ['/api/auth/register', '/api/auth/challenge', '/api/auth/verify-challenge'];
  // Three calls in total, whichever routes: the handlers answer 400 for the empty body.
  for (let i = 0; i < 3; i++) assert.equal((await post(env, paths[i], ip)).status, 400, paths[i]);
  for (const path of paths) {
    const res = await post(env, path, ip);
    assert.equal(res.status, 429, path);
    assert.equal(res.headers.get('Retry-After'), '60');
    assert.equal((await res.json()).code, 'rate_limited');
  }
});

test('the limit is per address: another caller is not held back by a busy one', async () => {
  const env = { ...fakeEnv(), AUTH_RATE_LIMITER: fakeLimiter(1), CHALLENGES: fakeEnv().SOCIAL };
  const busy = { 'CF-Connecting-IP': '203.0.113.7' };
  await post(env, '/api/auth/challenge', busy);
  assert.equal((await post(env, '/api/auth/challenge', busy)).status, 429);
  assert.notEqual((await post(env, '/api/auth/challenge', { 'CF-Connecting-IP': '203.0.113.8' })).status, 429);
});

test('only the token-free auth routes are limited', async () => {
  const limiter = fakeLimiter(0);
  const env = { ...fakeEnv(), AUTH_RATE_LIMITER: limiter };
  const ip = { 'CF-Connecting-IP': '203.0.113.7' };
  const list = await worker.fetch(new Request(`${base}/api/simulations`, { headers: ip }), env, ctx);
  assert.equal(list.status, 200);
  const me = await worker.fetch(new Request(`${base}/api/auth/me`, { headers: ip }), env, ctx);
  assert.equal(me.status, 401, 'the signed-in lookup is not limited');
  assert.equal(limiter.used.size, 0, 'the limiter was not asked about either');
});

test('with no limiter bound, or no caller address (a local wrangler dev), nothing is limited', async () => {
  const unbound = { ...fakeEnv(), CHALLENGES: fakeEnv().SOCIAL };
  for (let i = 0; i < 5; i++) assert.notEqual((await post(unbound, '/api/auth/challenge', { 'CF-Connecting-IP': '203.0.113.7' })).status, 429);
  const noAddress = { ...fakeEnv(), AUTH_RATE_LIMITER: fakeLimiter(0), CHALLENGES: fakeEnv().SOCIAL };
  assert.notEqual((await post(noAddress, '/api/auth/challenge')).status, 429);
});

// ── Stripe webhook ───────────────────────────────────────────────────────────

const SECRET = 'whsec_test_not_real';
const EVENT = JSON.stringify({ id: 'evt_test', type: 'ping', livemode: false, data: { object: {} } });

function sign(body, { secret = SECRET, t = Math.floor(Date.now() / 1000) } = {}) {
  const v1 = crypto.createHmac('sha256', secret).update(`${t}.${body}`).digest('hex');
  return `t=${t},v1=${v1}`;
}

const webhook = (env, body, signature) =>
  worker.fetch(new Request(`${base}/api/stripe/webhook`, {
    method: 'POST', headers: signature === undefined ? {} : { 'Stripe-Signature': signature }, body,
  }), env, ctx);

const webhookEnv = () => ({ ...fakeEnv(), STRIPE_WEBHOOK_SECRET: SECRET });

test('a correctly signed delivery is accepted', async () => {
  const res = await webhook(webhookEnv(), EVENT, sign(EVENT));
  assert.equal(res.status, 200);
});

test('a delivery with no signature, a wrong one, or a changed body is refused', async () => {
  const env = webhookEnv();
  assert.equal((await webhook(env, EVENT)).status, 400, 'no header');
  assert.equal((await webhook(env, EVENT, '')).status, 400, 'empty header');
  assert.equal((await webhook(env, EVENT, 'garbage')).status, 400, 'not a signature header');
  assert.equal((await webhook(env, EVENT, sign(EVENT, { secret: 'whsec_other' }))).status, 400, 'signed with another secret');
  assert.equal((await webhook(env, EVENT + ' ', sign(EVENT))).status, 400, 'body changed after signing');
  const t = Math.floor(Date.now() / 1000);
  assert.equal((await webhook(env, EVENT, `t=${t}`)).status, 400, 'no v1 value');
  assert.equal((await webhook(env, EVENT, `v1=${'0'.repeat(64)}`)).status, 400, 'no timestamp');
  assert.equal((await webhook(env, EVENT, `t=${t},v1=${'0'.repeat(64)}`)).status, 400, 'right shape, wrong value');
  assert.equal((await webhook(env, EVENT, `t=${t},v1=short`)).status, 400, 'wrong length');
});

test('a correctly signed delivery outside the five-minute window is refused, so a captured one cannot be replayed', async () => {
  const env = webhookEnv();
  const now = Math.floor(Date.now() / 1000);
  assert.equal((await webhook(env, EVENT, sign(EVENT, { t: now - 301 }))).status, 400, 'too old');
  assert.equal((await webhook(env, EVENT, sign(EVENT, { t: now + 301 }))).status, 400, 'too far ahead');
  assert.equal((await webhook(env, EVENT, sign(EVENT, { t: now - 200 }))).status, 200, 'inside the window');
});

test('with no signing secret configured the webhook trusts nothing', async () => {
  const env = fakeEnv();
  assert.equal((await webhook(env, EVENT, sign(EVENT))).status, 503);
  assert.equal((await webhook(env, EVENT)).status, 503);
});

test('a refused delivery moves no money: the ledger and wallets stay untouched', async () => {
  const env = { ...webhookEnv(), PAYOUTS: fakeEnv().SOCIAL };
  const paid = JSON.stringify({
    id: 'evt_paid', type: 'checkout.session.completed', livemode: false,
    data: { object: { id: 'cs_test_forged', payment_status: 'paid', amount_total: 100000, metadata: { type: 'ticket_purchase', user_id: 'someone', tickets: '100000' } } },
  });
  const res = await webhook(env, paid, sign(paid, { secret: 'whsec_attacker' }));
  assert.equal(res.status, 400);
  assert.equal(env.PAYOUTS.store.size, 0, 'nothing was recorded');
});
