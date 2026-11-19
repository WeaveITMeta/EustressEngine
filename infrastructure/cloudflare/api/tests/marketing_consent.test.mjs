// The Marketing Emails box at sign-up and the switch in Settings (src/marketing_consent.mjs), through the real Worker.
// The box may start ticked only where Cloudflare places the request in an opt-out country (the United States); everywhere
// else, and where the country is unknown, it starts empty and only the person's own tick turns it on. The Worker works
// that out itself at registration; what the page claims about the box is never read.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx } from './helpers/load_worker.mjs';
import { seedVerifiedKyc, claimSecretOf } from './helpers/kyc_session.mjs';
import {
  OPT_OUT_COUNTRIES, MARKETING_CONSENT_VERSION, MARKETING_CONSENT_TEXT,
  requestCountry, marketingPrechecked, consentBasis,
} from '../src/marketing_consent.mjs';

const worker = await loadWorker();
const ORIGIN = 'https://api.eustress.dev';

function environment() {
  return { ...fakeEnv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), CHALLENGES: fakeKv(), NOTIFY_RATE_LIMITER: undefined };
}

async function call(env, method, route, { token, body, country } = {}) {
  const headers = {};
  if (country) headers['cf-ipcountry'] = country;
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`${ORIGIN}${route}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}

let made = 0;
async function signUp(env, { country, marketing_email } = {}) {
  made += 1;
  const kyc_session_id = await seedVerifiedKyc(env);
  const body = {
    username: `mkt_user_${made}`, public_key: crypto.randomBytes(32).toString('hex'), birthday: '1990-01-01',
    kyc_session_id, kyc_claim_secret: claimSecretOf(kyc_session_id),
    ...(marketing_email === undefined ? {} : { marketing_email }),
  };
  const r = await call(env, 'POST', '/api/auth/register', { body, country });
  assert.ok([200, 201].includes(r.status), JSON.stringify(r.body));
  return r.body.user.id;
}

const prefsOf = async (env, id) => JSON.parse((await env.USERS.get(`notify-prefs:${id}`)) || '{}');
const logOf = (env, id) => [...env.USERS.store.keys()].filter((k) => k.startsWith(`mkt-consent:${id}:`));

// ── The rule ─────────────────────────────────────────────────────────────────

test('only an opt-out country may start the box ticked, and an unknown country never does', () => {
  assert.deepEqual([...OPT_OUT_COUNTRIES], ['US']);
  assert.equal(marketingPrechecked('US'), true);
  for (const c of ['CA', 'GB', 'DE', 'FR', 'AU', 'BR', 'JP', 'MX', 'ZZ', null, undefined, '']) {
    assert.equal(marketingPrechecked(c), false, String(c));
  }
  const req = (country, header) => ({ cf: country === undefined ? undefined : { country }, headers: new Headers(header ? { 'cf-ipcountry': header } : {}) });
  assert.equal(requestCountry(req('us')), 'US');
  assert.equal(requestCountry(req(undefined, 'DE')), 'DE');
  assert.equal(requestCountry(req('GB', 'US')), 'GB', 'Cloudflare\'s own reading comes first');
  for (const unknown of ['XX', 'T1', 'USA', '', '1A']) assert.equal(requestCountry(req(unknown)), null, unknown);
  assert.equal(requestCountry(req(undefined)), null);
});

test('the basis says whether the person ticked it themselves', () => {
  assert.equal(consentBasis(true, true, 'signup'), 'prechecked');
  assert.equal(consentBasis(true, false, 'signup'), 'express');
  assert.equal(consentBasis(true, true, 'settings'), 'express', 'turning the switch on is always the person\'s act');
  assert.equal(consentBasis(false, true, 'signup'), 'declined');
  assert.equal(consentBasis(false, false, 'settings'), 'declined');
  assert.equal(consentBasis(false, false, 'unsubscribe_link'), 'unsubscribed');
  assert.ok(!/[–—]/.test(MARKETING_CONSENT_TEXT), 'no dashes the rubric fails');
});

// ── What the sign-up page is told ────────────────────────────────────────────

test('the jurisdiction answer tells the sign-up page to tick the box in the US and leave it empty elsewhere', async () => {
  const env = environment();
  const us = await call(env, 'GET', '/api/kyc/jurisdiction', { country: 'US' });
  assert.equal(us.status, 200);
  assert.deepEqual(us.body.marketing, { prechecked: true, text: MARKETING_CONSENT_TEXT, version: MARKETING_CONSENT_VERSION });
  for (const country of ['DE', 'GB', 'CA', 'XX']) {
    const r = await call(env, 'GET', '/api/kyc/jurisdiction', { country });
    assert.equal(r.status, 200, country);
    assert.equal(r.body.marketing.prechecked, false, country);
  }
  const none = await call(env, 'GET', '/api/kyc/jurisdiction');
  assert.equal(none.body.marketing.prechecked, false, 'no country: empty');
});

// ── Registration ─────────────────────────────────────────────────────────────

test('a US sign-up that leaves the ticked box is on, recorded as prechecked, indexed and logged', async () => {
  const env = environment();
  const id = await signUp(env, { country: 'US', marketing_email: true });
  const prefs = await prefsOf(env, id);
  assert.equal(prefs.marketing_email, true);
  assert.equal(prefs.marketing.basis, 'prechecked');
  assert.equal(prefs.marketing.country, 'US');
  assert.equal(prefs.marketing.source, 'signup');
  assert.equal(prefs.marketing.version, MARKETING_CONSENT_VERSION);
  assert.ok(await env.USERS.get(`mkt-optin:${id}`));
  assert.equal(logOf(env, id).length, 1);
});

test('a yes from a country that needs consent is recorded as the person\'s own tick, whatever the page says', async () => {
  const env = environment();
  for (const country of ['DE', 'GB', 'CA', undefined]) {
    const id = await signUp(env, { country, marketing_email: true });
    const prefs = await prefsOf(env, id);
    assert.equal(prefs.marketing_email, true, String(country));
    assert.equal(prefs.marketing.basis, 'express', String(country));
    assert.equal(prefs.marketing.country, country || null);
  }
});

test('a sign-up that unticks the box, or sends no choice, is off and not on the list', async () => {
  const env = environment();
  const declined = await signUp(env, { country: 'US', marketing_email: false });
  assert.equal((await prefsOf(env, declined)).marketing_email, false);
  assert.equal((await prefsOf(env, declined)).marketing.basis, 'declined');
  assert.equal(await env.USERS.get(`mkt-optin:${declined}`), null);
  assert.equal(logOf(env, declined).length, 1, 'a no is evidence too');

  const silent = await signUp(env, { country: 'US' });
  assert.deepEqual(await prefsOf(env, silent), {}, 'nothing recorded: off');
  assert.equal(await env.USERS.get(`mkt-optin:${silent}`), null);
});

test('a choice that is not true or false is ignored, and the account is still made', async () => {
  const env = environment();
  const id = await signUp(env, { country: 'US', marketing_email: 'yes' });
  assert.deepEqual(await prefsOf(env, id), {});
});

// ── Settings ─────────────────────────────────────────────────────────────────

test('the Settings switch reads off by default, turns on as express consent, and turns off from anywhere', async () => {
  const env = environment();
  const id = await signUp(env, { country: 'US' });
  const token = mintToken(id);
  const first = await call(env, 'GET', '/api/notifications/prefs', { token });
  assert.equal(first.status, 200);
  assert.deepEqual(first.body, { moderation_email: true, marketing_email: false });

  const on = await call(env, 'PUT', '/api/notifications/prefs', { token, body: { marketing_email: true }, country: 'FR' });
  assert.equal(on.status, 200, JSON.stringify(on.body));
  assert.deepEqual(on.body, { moderation_email: true, marketing_email: true });
  assert.equal((await prefsOf(env, id)).marketing.basis, 'express');
  assert.equal((await prefsOf(env, id)).marketing.country, 'FR');
  assert.ok(await env.USERS.get(`mkt-optin:${id}`));

  const off = await call(env, 'PUT', '/api/notifications/prefs', { token, body: { marketing_email: false } });
  assert.deepEqual(off.body, { moderation_email: true, marketing_email: false });
  assert.equal(await env.USERS.get(`mkt-optin:${id}`), null);
  assert.equal(logOf(env, id).length >= 1, true);
});

test('the review-email switch still works on its own and leaves marketing alone', async () => {
  const env = environment();
  const id = await signUp(env, { country: 'US', marketing_email: true });
  const token = mintToken(id);
  const r = await call(env, 'PUT', '/api/notifications/prefs', { token, body: { moderation_email: false } });
  assert.deepEqual(r.body, { moderation_email: false, marketing_email: true });
  assert.ok(await env.USERS.get(`mkt-optin:${id}`));
  for (const body of [{}, { marketing_email: 'on' }, { moderation_email: 1 }]) {
    assert.equal((await call(env, 'PUT', '/api/notifications/prefs', { token, body })).status, 400, JSON.stringify(body));
  }
  assert.equal((await call(env, 'GET', '/api/notifications/prefs')).status, 401);
});
