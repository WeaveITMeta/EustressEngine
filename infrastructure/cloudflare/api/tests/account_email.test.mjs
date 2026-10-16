// An account's email: the first verification link goes to the address on file and
// no other, a change needs the current key's signature and waits seven days with a
// cancel link to the addresses the account already had, a verified address counts
// after thirty days, and every key notice goes to ALL of the account's addresses
// (docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "Verified email").
// The object runs against in-memory storage; the routes run the real Worker.

import test from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { AccountAuth, accountCall, accountState, rotationMessage } from '../src/account_auth.mjs';
import {
  emailChangeMessage, cleanAddress, EMAIL_CHANGE_WAIT_MS, VERIFIED_AGE_MS, VERIFY_LINK_MS, EXPIRED_LINK_KEPT_MS,
} from '../src/email_ops.mjs';
import { renderEmailNotice, maskAddress, sendNotice } from '../src/account_notices.mjs';
import { MAIL_PER_DAY } from '../src/mail_limits.mjs';
import { readAccountEmail } from '../src/email_ops.mjs';

const worker = await loadWorker();
const DAY = 24 * 3600 * 1000;
const HOUR = 3600 * 1000;
const T0 = Date.UTC(2026, 9, 1, 12, 0, 0);
const ACCOUNT = '77777777-7777-4777-8777-777777777777';
const OTHER = '88888888-8888-4888-8888-888888888888';
const ORIGIN = 'https://api.eustress.dev';
const SITE = 'https://staging.example.pages.dev';

function keyPair() {
  const { publicKey, privateKey } = crypto.generateKeyPairSync('ed25519');
  const hex = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url').toString('hex');
  return { hex, sign: (text) => crypto.sign(null, Buffer.from(text), privateKey).toString('hex') };
}

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

/// Plain text of an email the Worker built.
function bodyOf(message) {
  return [...message.raw.matchAll(/Content-Transfer-Encoding: base64\r\n\r\n([A-Za-z0-9+/=\r\n]+?)(?=\r\n--)/g)]
    .map((m) => Buffer.from(m[1].replace(/\r\n/g, ''), 'base64').toString())
    .join('\n');
}

// ── Object level ─────────────────────────────────────────────────────────────

async function setup({ email = 'owner@example.com' } = {}) {
  const env = { USERS: fakeKv() };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  const k0 = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: k0.hex, email }));
  await env.USERS.put(`pubkey:${k0.hex}`, ACCOUNT);
  await accountState(env, ACCOUNT);
  return { env, k0 };
}
const call = (env, op, args = {}) => accountCall(env, ACCOUNT, op, args);
const secretOf = (token) => token.split('.')[1];

async function verify(env, now = T0) {
  const s = await call(env, 'email_verify_start', { now });
  assert.equal(s.ok, true, JSON.stringify(s));
  const c = await call(env, 'email_verify_confirm', { token: secretOf(s.token), now });
  assert.equal(c.ok, true, JSON.stringify(c));
  return c;
}

/// begin, sign, request. `sign` is the key that signs; `mutate` changes the request.
async function requestChange(env, key, address, { now = T0, origin = ORIGIN, sign = key, mutate } = {}) {
  const begun = await call(env, 'email_change_begin', { now });
  assert.equal(begun.ok, true, JSON.stringify(begun));
  const message = emailChangeMessage({ origin, accountId: ACCOUNT, key: key.hex, address: cleanAddress(address) || address, challenge: begun.challenge });
  const request = { challenge: begun.challenge, new_address: address, signature: sign.sign(message), origin, now };
  if (mutate) mutate(request, { message, begun });
  return { begun, message, result: await call(env, 'email_change_request', request) };
}

test('the message the current key signs for an address change is the documented one, line by line', () => {
  const m = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: 'a'.repeat(64), address: 'new@example.com', challenge: 'c'.repeat(64) });
  assert.deepEqual(m.split('\n'), ['eustress-email-change-v1', ORIGIN, ACCOUNT, 'a'.repeat(64), 'new@example.com', 'c'.repeat(64)]);
});

test('an address is one plain address: no name, no list, no line break', () => {
  assert.equal(cleanAddress('  Owner@Example.COM '), 'owner@example.com');
  for (const bad of ['a@b.c, d@e.fg', 'Name <a@b.cd>', 'a@b.cd\r\nBcc: x@y.zz', 'no-at-sign', '', null, 7, 'a@b', 'a b@c.de']) {
    assert.equal(cleanAddress(bad), null, String(bad));
  }
});

// ── Verification ─────────────────────────────────────────────────────────────

test('the first verification link goes to the address on file and to no other, whatever the request says', async () => {
  const { env } = await setup();
  const r = await call(env, 'email_verify_start', { now: T0, address: 'thief@evil.test', email: 'thief@evil.test', to: 'thief@evil.test' });
  assert.equal(r.ok, true, JSON.stringify(r));
  assert.equal(r.address, 'owner@example.com');
  assert.match(r.token, new RegExp(`^${ACCOUNT}\\.[0-9a-f]{64}$`));
  assert.equal(r.expires_at, new Date(T0 + VERIFY_LINK_MS).toISOString());
});

test('an account with no address on file gets no verification link', async () => {
  const { env } = await setup({ email: null });
  const r = await call(env, 'email_verify_start', { now: T0 });
  assert.equal(r.ok, false);
  assert.equal(r.code, 'no_email_on_file');
});

test('a verification link works once, for the address it was made for, and not after 24 hours', async () => {
  const { env } = await setup();
  const s = await call(env, 'email_verify_start', { now: T0 });
  const ok = await call(env, 'email_verify_confirm', { token: secretOf(s.token), now: T0 + HOUR });
  assert.equal(ok.ok, true);
  assert.equal(ok.verified_at, new Date(T0 + HOUR).toISOString());
  const again = await call(env, 'email_verify_confirm', { token: secretOf(s.token), now: T0 + HOUR });
  assert.equal(again.ok, false);
  assert.equal(again.code, 'token_invalid');

  const { env: env2 } = await setup();
  const late = await call(env2, 'email_verify_start', { now: T0 });
  const expired = await call(env2, 'email_verify_confirm', { token: secretOf(late.token), now: T0 + VERIFY_LINK_MS + 1 });
  assert.equal(expired.code, 'token_expired');
  assert.equal(expired.status, 410);
  assert.equal((await call(env2, 'email_status', { now: T0 + VERIFY_LINK_MS + 1 })).email.verified, false);

  for (const junk of [undefined, null, '', 'zz', 'a'.repeat(63), 'a'.repeat(65), 'A'.repeat(64), 7, {}]) {
    assert.equal((await call(env2, 'email_verify_confirm', { token: junk, now: T0 })).code, 'token_invalid', String(junk));
  }
});

test('an expired verification link says it expired for a while, then is just not valid; used and wrong links never say expired', async () => {
  const { env } = await setup();
  const s = await call(env, 'email_verify_start', { now: T0 });
  const secret = secretOf(s.token);
  // Expired, and told so again on a second try: the answer does not spend the link's memory.
  assert.equal((await call(env, 'email_verify_confirm', { token: secret, now: T0 + VERIFY_LINK_MS + 1 })).code, 'token_expired');
  // The state may have been pruned by anything else in between; the memory lasts two weeks.
  await call(env, 'email_status', { now: T0 + 3 * DAY });
  const later = await call(env, 'email_verify_confirm', { token: secret, now: T0 + 5 * DAY });
  assert.equal(later.ok, false);
  assert.equal(later.code, 'token_expired', 'asked again, still expired: nothing was spent');
  assert.equal((await call(env, 'email_status', { now: T0 + 5 * DAY })).email.verified, false, 'an expired link verifies nothing, however it is asked');

  // After the memory is gone it is simply not valid.
  assert.equal((await call(env, 'email_verify_confirm', { token: secret, now: T0 + VERIFY_LINK_MS + EXPIRED_LINK_KEPT_MS + HOUR })).code, 'token_invalid');

  // A link that was used is "not valid", never "expired".
  const { env: env2 } = await setup();
  const used = await call(env2, 'email_verify_start', { now: T0 });
  assert.equal((await call(env2, 'email_verify_confirm', { token: secretOf(used.token), now: T0 + HOUR })).ok, true);
  assert.equal((await call(env2, 'email_verify_confirm', { token: secretOf(used.token), now: T0 + 2 * DAY })).code, 'token_invalid');

  // A second link replaces the first: the first is not "expired", it is gone.
  const { env: env3 } = await setup();
  const first = await call(env3, 'email_verify_start', { now: T0 });
  const second = await call(env3, 'email_verify_start', { now: T0 + 1000 });
  assert.equal((await call(env3, 'email_verify_confirm', { token: secretOf(first.token), now: T0 + VERIFY_LINK_MS + HOUR })).code, 'token_invalid');
  assert.equal((await call(env3, 'email_verify_confirm', { token: secretOf(second.token), now: T0 + VERIFY_LINK_MS + HOUR })).code, 'token_expired');

  // A cancel link that ran out is never told apart: it is not a verification link.
  const { env: env4, k0 } = await setup();
  const { result } = await requestChange(env4, k0, 'new@example.com');
  assert.equal(result.ok, true);
  assert.equal((await call(env4, 'email_verify_confirm', { token: secretOf(result.cancel_token), now: T0 + 2 * DAY })).code, 'token_invalid');
});

test('a cancel link is not a verification link, and trying it that way does not spend it', async () => {
  const { env, k0 } = await setup();
  const { result } = await requestChange(env, k0, 'new@example.com');
  assert.equal(result.ok, true, JSON.stringify(result));
  const asVerify = await call(env, 'email_verify_confirm', { token: secretOf(result.cancel_token), now: T0 });
  assert.equal(asVerify.code, 'token_invalid');
  const cancelled = await call(env, 'email_change_cancel', { token: secretOf(result.cancel_token), now: T0 });
  assert.equal(cancelled.ok, true, 'the cancel link still works');
});

test('a verification link made for an address that has since been replaced verifies nothing', async () => {
  const { env, k0 } = await setup();
  await requestChange(env, k0, 'new@example.com');
  const at = T0 + EMAIL_CHANGE_WAIT_MS;
  const link = await call(env, 'email_verify_start', { now: at });
  assert.equal(link.address, 'owner@example.com');
  assert.equal((await call(env, 'email_change_apply', { now: at + 1000 })).ok, true);
  const stale = await call(env, 'email_verify_confirm', { token: secretOf(link.token), now: at + 2000 });
  assert.equal(stale.code, 'token_invalid');
  const view = (await call(env, 'email_status', { now: at + 3000 })).email;
  assert.equal(view.on_file, 'new@example.com');
  assert.equal(view.verified, false, 'the new address is not verified by a link sent to the old one');
  // A link to the new address verifies it.
  const fresh = await call(env, 'email_verify_start', { now: at + 4000 });
  assert.equal(fresh.address, 'new@example.com');
  assert.equal((await call(env, 'email_verify_confirm', { token: secretOf(fresh.token), now: at + 5000 })).ok, true);
});

test('a verification link cannot cancel an address change', async () => {
  const { env, k0 } = await setup();
  const link = await call(env, 'email_verify_start', { now: T0 });
  await requestChange(env, k0, 'new@example.com', { now: T0 + 1000 });
  const r = await call(env, 'email_change_cancel', { token: secretOf(link.token), now: T0 + 2000 });
  assert.equal(r.code, 'token_invalid');
  assert.equal((await call(env, 'email_status', { now: T0 + 3000 })).email.pending.address, 'new@example.com');
});

test('asking for a second link replaces the first, and an address that is already verified says so', async () => {
  const { env } = await setup();
  const a = await call(env, 'email_verify_start', { now: T0 });
  const b = await call(env, 'email_verify_start', { now: T0 + 1000 });
  assert.equal((await call(env, 'email_verify_confirm', { token: secretOf(a.token), now: T0 + 2000 })).code, 'token_invalid', 'the first link no longer works');
  assert.equal((await call(env, 'email_verify_confirm', { token: secretOf(b.token), now: T0 + 2000 })).ok, true);
  const again = await call(env, 'email_verify_start', { now: T0 + 3000 });
  assert.equal(again.ok, true);
  assert.equal(again.already_verified, true);
  assert.equal(again.token, undefined, 'no link is issued for a verified address');
});

test('verification links are limited to three an hour', async () => {
  const { env } = await setup();
  for (let i = 0; i < 3; i++) assert.equal((await call(env, 'email_verify_start', { now: T0 + i })).ok, true);
  const fourth = await call(env, 'email_verify_start', { now: T0 + 10 });
  assert.equal(fourth.ok, false);
  assert.equal(fourth.code, 'rate_limited');
  assert.equal((await call(env, 'email_verify_start', { now: T0 + HOUR + 1000 })).ok, true, 'an hour later it works again');
});

test('a verified address counts as verified once it is thirty days old, not at twenty-nine', async () => {
  const { env } = await setup();
  await verify(env, T0);
  const at = async (now) => (await call(env, 'email_status', { now })).email;
  assert.equal((await at(T0)).verified, true);
  assert.equal((await at(T0)).aged, false);
  assert.equal((await at(T0 + 29 * DAY)).aged, false);
  assert.equal((await at(T0 + VERIFIED_AGE_MS - 1)).aged, false);
  assert.equal((await at(T0 + VERIFIED_AGE_MS)).aged, true);
  assert.equal((await at(T0)).aged_at, new Date(T0 + VERIFIED_AGE_MS).toISOString());
});

// ── Asking for a new address ─────────────────────────────────────────────────

test('an address change takes the current key\'s signature, waits seven days, and names the people to warn', async () => {
  const { env, k0 } = await setup();
  await verify(env, T0 - 40 * DAY);
  const { result } = await requestChange(env, k0, 'New@Example.com');
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.equal(result.pending.address, 'new@example.com');
  assert.equal(result.pending.effective_at, new Date(T0 + EMAIL_CHANGE_WAIT_MS).toISOString());
  assert.equal(result.pending.ready, false);
  assert.deepEqual(result.notify.existing, ['owner@example.com'], 'the warning goes to the address the account has, not the new one');
  assert.equal(result.notify.pending_address, 'new@example.com');
  assert.match(result.cancel_token, new RegExp(`^${ACCOUNT}\\.[0-9a-f]{64}$`));
  const view = (await call(env, 'email_status', { now: T0 })).email;
  assert.equal(view.on_file, 'owner@example.com', 'nothing has changed yet');
  assert.equal(view.verified, true);
});

test('an address change is refused for each wrong signature, origin, address or challenge, and spends the challenge', async () => {
  const { env, k0 } = await setup();
  const stranger = keyPair();
  const cases = [
    ['another key signs', { sign: stranger }, 'signature_invalid'],
    ['the origin differs', { mutate: (req) => { req.origin = 'https://evil.example'; } }, 'signature_invalid'],
    ['the address differs from the one signed', { mutate: (req) => { req.new_address = 'other@example.com'; } }, 'signature_invalid'],
    ['the signature is missing', { mutate: (req) => { delete req.signature; } }, 'signature_invalid'],
    ['the signature is not hex', { mutate: (req) => { req.signature = 'zz'.repeat(64); } }, 'signature_invalid'],
    ['the challenge is invented', { mutate: (req) => { req.challenge = 'ab'.repeat(32); } }, 'challenge_invalid'],
    ['the challenge is missing', { mutate: (req) => { delete req.challenge; } }, 'challenge_invalid'],
  ];
  let day = 0;
  for (const [name, options, code] of cases) {
    // A day apart: five challenges a day is its own limit.
    const { result } = await requestChange(env, k0, 'new@example.com', { now: T0 + (day++) * DAY, ...options });
    assert.equal(result.ok, false, name);
    assert.equal(result.code, code, name);
  }
  assert.equal((await call(env, 'email_status', { now: T0 })).email.pending, null, 'nothing was recorded');

  // The challenge a failed attempt used is gone.
  const begun = await call(env, 'email_change_begin', { now: T0 });
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: k0.hex, address: 'new@example.com', challenge: begun.challenge });
  const bad = await call(env, 'email_change_request', { challenge: begun.challenge, new_address: 'new@example.com', signature: stranger.sign(message), origin: ORIGIN, now: T0 });
  assert.equal(bad.code, 'signature_invalid');
  const retry = await call(env, 'email_change_request', { challenge: begun.challenge, new_address: 'new@example.com', signature: k0.sign(message), origin: ORIGIN, now: T0 });
  assert.equal(retry.code, 'challenge_invalid');
});

test('an address that is not an address, or is the one already there, is refused', async () => {
  const { env, k0 } = await setup();
  assert.equal((await requestChange(env, k0, 'a@b.cd, c@d.ef')).result.code, 'bad_address');
  assert.equal((await requestChange(env, k0, 'not an address')).result.code, 'bad_address');
  assert.equal((await requestChange(env, k0, 'OWNER@example.com')).result.code, 'same_address');
  const first = await requestChange(env, k0, 'new@example.com');
  assert.equal(first.result.ok, true);
  assert.equal((await requestChange(env, k0, 'new@example.com')).result.code, 'same_address', 'the pending address too');
});

test('an address-change challenge is not a rotation challenge, and the other way round', async () => {
  const { env, k0 } = await setup();
  const forEmail = await call(env, 'email_change_begin', { now: T0 });
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: newKey.hex, challenge: forEmail.challenge });
  const asRotation = await call(env, 'rotate', { new_public_key: newKey.hex, challenge: forEmail.challenge, origin: ORIGIN, old_signature: k0.sign(m), new_signature: newKey.sign(m), now: T0 });
  assert.equal(asRotation.code, 'challenge_invalid');

  const forRotation = await call(env, 'begin_rotation', { now: T0 });
  const msg = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: k0.hex, address: 'new@example.com', challenge: forRotation.challenge });
  const asEmail = await call(env, 'email_change_request', { challenge: forRotation.challenge, new_address: 'new@example.com', signature: k0.sign(msg), origin: ORIGIN, now: T0 });
  assert.equal(asEmail.code, 'challenge_invalid');
});

test('address changes are limited to five a day, counted where the key signed, and asking for a challenge costs nothing', async () => {
  const { env, k0 } = await setup();
  // A session without the key can ask for challenges all day and use up nothing.
  for (let i = 0; i < 20; i++) assert.equal((await call(env, 'email_change_begin', { now: T0 + i })).ok, true);
  for (let i = 0; i < 5; i++) {
    const { result } = await requestChange(env, k0, `change${i}@example.com`, { now: T0 + 100 + i });
    assert.equal(result.ok, true, JSON.stringify(result));
  }
  const sixth = await requestChange(env, k0, 'sixth@example.com', { now: T0 + 200 });
  assert.equal(sixth.result.code, 'rate_limited');
  const next = await requestChange(env, k0, 'sixth@example.com', { now: T0 + DAY + 1000 });
  assert.equal(next.result.ok, true, 'a day later it works again');
});

test('a new request replaces the pending one, and the earlier cancel link stops working', async () => {
  const { env, k0 } = await setup();
  const a = await requestChange(env, k0, 'first@example.com');
  const b = await requestChange(env, k0, 'second@example.com', { now: T0 + 1000 });
  assert.equal(b.result.ok, true);
  assert.equal((await call(env, 'email_status', { now: T0 + 1000 })).email.pending.address, 'second@example.com');
  const old = await call(env, 'email_change_cancel', { token: secretOf(a.result.cancel_token), now: T0 + 2000 });
  assert.equal(old.code, 'token_invalid');
  assert.equal((await call(env, 'email_change_cancel', { token: secretOf(b.result.cancel_token), now: T0 + 2000 })).ok, true);
});

// ── Applying, and the addresses that stay on the list ────────────────────────

test('the change does nothing for seven days, then makes the new address the one on file, unverified', async () => {
  const { env, k0 } = await setup();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: k0.hex, email: 'owner@example.com' }));
  await verify(env, T0 - 40 * DAY);
  await requestChange(env, k0, 'new@example.com');
  const early = await call(env, 'email_change_apply', { now: T0 + EMAIL_CHANGE_WAIT_MS - 1 });
  assert.equal(early.ok, false);
  assert.equal(early.code, 'not_yet');
  const applied = await call(env, 'email_change_apply', { now: T0 + EMAIL_CHANGE_WAIT_MS });
  assert.equal(applied.ok, true, JSON.stringify(applied));
  assert.equal(applied.address, 'new@example.com');
  const view = (await call(env, 'email_status', { now: T0 + EMAIL_CHANGE_WAIT_MS })).email;
  assert.equal(view.on_file, 'new@example.com');
  assert.equal(view.verified, false, 'the new address starts unverified');
  assert.equal(view.pending, null);
  // The user record is NOT rewritten (a read-modify-write of it can undo a ban that
  // landed meanwhile); the address the account's mail goes to is a key of its own.
  const user = JSON.parse(await env.USERS.get(`user:${ACCOUNT}`));
  assert.equal(user.email, 'owner@example.com');
  assert.equal(user.email_verified_at, undefined);
  assert.deepEqual(JSON.parse(await env.USERS.get(`email:${ACCOUNT}`)), { address: 'new@example.com', verified_at: null });
  assert.equal(await readAccountEmail(env, user), 'new@example.com', 'and that is what readers get');
  assert.equal((await call(env, 'email_change_apply', { now: T0 + EMAIL_CHANGE_WAIT_MS + 1 })).code, 'no_pending');
});

test('a change nobody confirms lapses a week after it could have taken effect', async () => {
  const { env, k0 } = await setup();
  await requestChange(env, k0, 'new@example.com');
  const lapsed = T0 + EMAIL_CHANGE_WAIT_MS + 7 * DAY;
  assert.equal((await call(env, 'email_status', { now: lapsed - 1 })).email.pending.address, 'new@example.com');
  assert.equal((await call(env, 'email_status', { now: lapsed })).email.pending, null);
  assert.equal((await call(env, 'email_change_apply', { now: lapsed })).code, 'no_pending');
});

test('the notice list holds the verified, the pending, the one on file and a replaced address for thirty days, never one of them', async () => {
  const { env, k0 } = await setup();
  const recipients = async (now) => (await call(env, 'notice_recipients', { now })).recipients;
  assert.deepEqual(await recipients(T0), ['owner@example.com']);
  await verify(env, T0 - 40 * DAY);
  await requestChange(env, k0, 'new@example.com');
  assert.deepEqual((await recipients(T0)).sort(), ['new@example.com', 'owner@example.com'], 'the pending address is told too');

  const applyAt = T0 + EMAIL_CHANGE_WAIT_MS;
  await call(env, 'email_change_apply', { now: applyAt });
  assert.deepEqual((await recipients(applyAt)).sort(), ['new@example.com', 'owner@example.com'], 'the replaced address stays on the list');
  assert.ok((await recipients(applyAt + 30 * DAY - 1)).includes('owner@example.com'), 'just under thirty days');
  assert.deepEqual(await recipients(applyAt + 30 * DAY), ['new@example.com'], 'at thirty days it drops off');
});

test('a thief who verifies their own address later does not displace the owner\'s from the list', async () => {
  const { env, k0 } = await setup();
  await verify(env, T0 - 60 * DAY);
  await requestChange(env, k0, 'thief@evil.example', { now: T0 });
  const applyAt = T0 + EMAIL_CHANGE_WAIT_MS;
  await call(env, 'email_change_apply', { now: applyAt });
  await verify(env, applyAt + HOUR); // the thief verifies the address on file
  const list = (await call(env, 'notice_recipients', { now: applyAt + 2 * HOUR })).recipients;
  assert.ok(list.includes('owner@example.com'), 'the owner is still told');
  assert.ok(list.includes('thief@evil.example'));
});

// ── Cancelling ───────────────────────────────────────────────────────────────

test('the cancel link stops the change and ends every sign-in, once', async () => {
  const { env, k0 } = await setup();
  const before = (await call(env, 'state')).floor;
  const { result } = await requestChange(env, k0, 'new@example.com');
  const wrong = await call(env, 'email_change_cancel', { token: 'f'.repeat(64), now: T0 + 1000 });
  assert.equal(wrong.code, 'token_invalid');
  assert.equal((await call(env, 'email_status', { now: T0 + 1000 })).email.pending.address, 'new@example.com', 'a wrong link changes nothing');

  const cancelled = await call(env, 'email_change_cancel', { token: secretOf(result.cancel_token), now: T0 + 2000 });
  assert.equal(cancelled.ok, true, JSON.stringify(cancelled));
  assert.equal(cancelled.sessions_ended, true);
  assert.ok(cancelled.floor > before);
  assert.deepEqual(cancelled.notify.existing, ['owner@example.com']);
  assert.equal(cancelled.notify.cancelled_address, 'new@example.com');
  assert.equal((await call(env, 'state')).floor, cancelled.floor);
  assert.equal(await env.USERS.get(`authfloor:${ACCOUNT}`), String(cancelled.floor), 'mirrored');
  assert.equal((await call(env, 'email_status', { now: T0 + 3000 })).email.pending, null);
  assert.equal((await call(env, 'email_change_cancel', { token: secretOf(result.cancel_token), now: T0 + 4000 })).code, 'token_invalid', 'it works once');
});

test('cancelling from a signed-in session stops the change without ending sessions', async () => {
  const { env, k0 } = await setup();
  await requestChange(env, k0, 'new@example.com');
  const before = (await call(env, 'state')).floor;
  const r = await call(env, 'email_change_cancel', { by_session: true, now: T0 + 1000 });
  assert.equal(r.ok, true);
  assert.equal(r.sessions_ended, false);
  assert.equal((await call(env, 'state')).floor, before);
  assert.equal((await call(env, 'email_change_cancel', { by_session: true, now: T0 + 2000 })).code, 'no_pending');
  assert.equal((await call(env, 'email_change_cancel', { now: T0 + 2000 })).code, 'bad_request', 'neither a link nor a session');
});

// ── A rotation and a pending change ──────────────────────────────────────────

test('replacing the key drops an address change the old key asked for, and the notice names every address', async () => {
  const { env, k0 } = await setup();
  await verify(env, T0 - 40 * DAY);
  await requestChange(env, k0, 'thief@evil.example');
  const begun = await call(env, 'begin_rotation', { now: T0 + 1000 });
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: k0.hex, newKey: newKey.hex, challenge: begun.challenge });
  const rotated = await call(env, 'rotate', { new_public_key: newKey.hex, challenge: begun.challenge, origin: ORIGIN, old_signature: k0.sign(m), new_signature: newKey.sign(m), now: T0 + 1000 });
  assert.equal(rotated.ok, true, JSON.stringify(rotated));
  assert.equal(rotated.dropped_email_change, true);
  assert.deepEqual(rotated.recipients.sort(), ['owner@example.com', 'thief@evil.example'], 'told on every address, including the one that was asked for');
  assert.equal((await call(env, 'email_status', { now: T0 + 2000 })).email.pending, null);
  assert.equal((await call(env, 'email_change_apply', { now: T0 + EMAIL_CHANGE_WAIT_MS })).code, 'no_pending');
});

// ── The audit ────────────────────────────────────────────────────────────────

test('every email step is in the audit chain, with a short print of the address and never the address', async () => {
  const { env, k0 } = await setup();
  await verify(env, T0);
  const { result } = await requestChange(env, k0, 'new@example.com', { now: T0 + 1000 });
  await call(env, 'email_change_cancel', { token: secretOf(result.cancel_token), now: T0 + 2000 });
  const audit = await call(env, 'audit', { limit: 50 });
  const types = audit.records.map((r) => r.type);
  for (const t of ['email_verify_requested', 'email_verified', 'email_change_requested', 'email_change_cancelled']) assert.ok(types.includes(t), t);
  const text = JSON.stringify(audit.records);
  assert.ok(!text.includes('owner@example.com') && !text.includes('new@example.com'), 'no address in the audit');
  assert.ok(!text.includes(secretOf(result.cancel_token)), 'no token in the audit');
});

// ── The notices ──────────────────────────────────────────────────────────────

test('the notices escape what they were given, carry the cancel link only to the addresses that had the account, and mask the new address', () => {
  const existing = renderEmailNotice('email_change_existing', {
    username: '<b>x</b>', newMasked: maskAddress('thief@evil.example'), effectiveAt: T0, cancelLink: `${SITE}/cancel-email-change?token=abc`,
  });
  assert.ok(!existing.html.includes('<b>x</b>'));
  assert.ok(existing.html.includes('&lt;b&gt;x&lt;/b&gt;'));
  assert.ok(existing.text.includes(`${SITE}/cancel-email-change?token=abc`));
  assert.ok(!existing.text.includes('thief@evil.example'), 'the new address is masked');
  assert.ok(existing.text.includes('t***@evil.example'));
  const pending = renderEmailNotice('email_change_pending', { username: 'owner', newMasked: 't***@evil.example', effectiveAt: T0 });
  // The header gear is the one remote address in a notice (a static asset, no query string); every other address is a link.
  const gear = /<img src="https:\/\/api\.eustress\.dev\/assets\/eustress-gear\.png" [^>]*>/;
  assert.ok(!/https?:\/\//.test(pending.text) && !/https?:\/\//.test(pending.html.replace(gear, '').replace(/<a href="mailto:[^"]*"/g, '')), 'the pending address is given no link');
  for (const kind of ['email_verify', 'email_changed', 'email_change_cancelled']) {
    const r = renderEmailNotice(kind, { username: 'owner', link: `${SITE}/verify-email?token=t`, expiresAt: new Date(T0).toISOString(), newMasked: 'n***@example.com', cancelledMasked: 'n***@example.com', sessionsEnded: true });
    assert.ok(r.subject && r.text && r.html, kind);
  }
  assert.throws(() => renderEmailNotice('nope', {}));
});

// ── Routes ───────────────────────────────────────────────────────────────────

async function environment(extra = {}) {
  const sent = [];
  const env = {
    ...fakeEnv(), CHALLENGES: fakeKv(), KYC_STATUS: fakeKv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN, PUBLIC_SITE_ORIGIN: SITE,
    EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage, ...extra,
  };
  env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
  const key = keyPair();
  await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: key.hex, email: 'owner@example.com' }));
  await env.USERS.put('username:owner', ACCOUNT);
  await env.USERS.put(`pubkey:${key.hex}`, ACCOUNT);
  return { env, key, sent, token: mintToken(ACCOUNT) };
}

async function request(env, method, route, body, token, origin = ORIGIN) {
  const headers = { 'Content-Type': 'application/json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const res = await worker.fetch(new Request(`${origin}${route}`, { method, headers, body: method === 'GET' ? undefined : JSON.stringify(body || {}) }), env, ctx);
  return { status: res.status, body: await res.json().catch(() => null) };
}
const post = (env, route, body, token, origin) => request(env, 'POST', route, body, token, origin);
const linkIn = (message, name) => new RegExp(`${SITE.replace(/[.]/g, '\\.')}/${name}\\?token=([0-9a-f-]{36}\\.[0-9a-f]{64})`).exec(bodyOf(message))?.[1];

test('the verification link is mailed to the address on file only, with the site\'s own address, and the response carries no token', async () => {
  const { env, sent, token } = await environment();
  assert.equal((await post(env, '/api/account/email/start', {})).status, 401);
  const r = await post(env, '/api/account/email/start', { email: 'thief@evil.test', address: 'thief@evil.test' }, token);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.sent_to, 'o***@example.com');
  assert.ok(!JSON.stringify(r.body).includes('token'), 'no token in the response');
  assert.equal(sent.length, 1);
  assert.equal(sent[0].to, 'owner@example.com');
  assert.ok(linkIn(sent[0], 'verify-email'), 'a link on the site');
});

test('a verification email that cannot be sent is an error the page can show, and a deployment without objects is told so', async () => {
  const down = await environment({ EMAIL: { send: async () => { throw new Error('mail is down'); } } });
  const r = await post(down.env, '/api/account/email/start', {}, down.token);
  assert.equal(r.status, 502);
  assert.equal(r.body.code, 'email_send_failed');
  const bare = await environment();
  bare.env.ACCOUNT_AUTH = undefined;
  assert.equal((await post(bare.env, '/api/account/email/start', {}, bare.token)).status, 501);
});

test('opening the link verifies the address with no session, once; any other token gets the same answer', async () => {
  const { env, sent, token } = await environment();
  await post(env, '/api/account/email/start', {}, token);
  const link = linkIn(sent[0], 'verify-email');
  const ok = await post(env, '/api/account/email/confirm', { token: link });
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
  assert.ok(ok.body.verified_at);
  assert.ok(ok.body.aged_at, 'the answer says from when the address counts for recovery');
  assert.equal(ok.body.address, maskAddress('owner@example.com'), 'and names the address, masked');
  assert.ok(!JSON.stringify(ok.body).includes('owner@example.com'), 'never in full');
  const used = await post(env, '/api/account/email/confirm', { token: link });
  assert.equal(used.status, 400);
  assert.equal(used.body.code, 'token_invalid');
  const garbage = await post(env, '/api/account/email/confirm', { token: 'nonsense' });
  const unknown = await post(env, '/api/account/email/confirm', { token: `${OTHER}.${'a'.repeat(64)}` });
  assert.deepEqual(garbage.body, used.body);
  assert.deepEqual(unknown.body, used.body, 'an account that does not exist answers the same');
  const status = await request(env, 'GET', '/api/account/email', null, token);
  assert.equal(status.body.email.verified, true);
});

test('an expired link answers 410 token_expired on the route, a used or wrong one 400 token_invalid, and neither verifies', async () => {
  const { env, sent, token } = await environment();
  await post(env, '/api/account/email/start', {}, token);
  const link = linkIn(sent[0], 'verify-email');
  const realNow = Date.now;
  Date.now = () => realNow() + VERIFY_LINK_MS + 60 * 1000;
  let expired;
  try { expired = await post(env, '/api/account/email/confirm', { token: link }); } finally { Date.now = realNow; }
  assert.equal(expired.status, 410, JSON.stringify(expired.body));
  assert.equal(expired.body.code, 'token_expired');
  assert.ok(!('verified_at' in expired.body));
  const status = await request(env, 'GET', '/api/account/email', null, token);
  assert.equal(status.body.email.verified, false);
  const wrong = await post(env, '/api/account/email/confirm', { token: `${ACCOUNT}.${'b'.repeat(64)}` });
  assert.equal(wrong.status, 400);
  assert.equal(wrong.body.code, 'token_invalid');
});

test('a link made for one account verifies nothing on another', async () => {
  const { env, sent, token } = await environment();
  const other = keyPair();
  await env.USERS.put(`user:${OTHER}`, JSON.stringify({ kyc_verified: true, id: OTHER, username: 'other', public_key: other.hex, email: 'other@example.com' }));
  await post(env, '/api/account/email/start', {}, token);
  const secret = linkIn(sent[0], 'verify-email').split('.')[1];
  const crossed = await post(env, '/api/account/email/confirm', { token: `${OTHER}.${secret}` });
  assert.equal(crossed.status, 400);
  const mine = await post(env, '/api/account/email/confirm', { token: `${ACCOUNT}.${secret}` });
  assert.equal(mine.status, 200, 'and it still works on its own account');
});

test('a change through the routes: signed by the current key, the existing address gets the cancel link, the new one gets none, and the response gives nobody a token', async () => {
  const { env, key, sent, token } = await environment();
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  assert.equal(begun.status, 200, JSON.stringify(begun.body));
  assert.equal(begun.body.format, 'eustress-email-change-v1');
  assert.equal(begun.body.origin, ORIGIN);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: key.hex, address: 'new@example.com', challenge: begun.body.challenge });
  const r = await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'new@example.com', signature: key.sign(message) }, token);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(r.body.pending.address, 'new@example.com');
  assert.ok(!JSON.stringify(r.body).includes('cancel'), 'the cancel link goes by email only');

  const toOwner = sent.find((m) => m.to === 'owner@example.com');
  const toNew = sent.find((m) => m.to === 'new@example.com');
  assert.ok(toOwner && toNew);
  assert.ok(linkIn(toOwner, 'cancel-email-change'), 'the address the account had is given the cancel link');
  assert.ok(!bodyOf(toNew).includes('cancel-email-change') && !/token=/.test(bodyOf(toNew)), 'the new address is given no link');
  assert.ok(!bodyOf(toOwner).includes('new@example.com'), 'and the new address is masked in the warning');

  const status = await request(env, 'GET', '/api/account/email', null, token);
  assert.equal(status.body.email.pending.address, 'new@example.com');
  const me = await request(env, 'GET', '/api/auth/me', null, token);
  assert.equal(me.body.user?.email_change_pending?.address ?? me.body.email_change_pending?.address, 'new@example.com', 'the signed-in site is told');
});

test('a session alone cannot change the address: the signature has to be the current key\'s', async () => {
  const { env, key, token } = await environment();
  const thief = keyPair();
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: key.hex, address: 'thief@evil.example', challenge: begun.body.challenge });
  const r = await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'thief@evil.example', signature: thief.sign(message) }, token);
  assert.equal(r.status, 401);
  assert.equal(r.body.code, 'signature_invalid');
  assert.equal((await request(env, 'GET', '/api/account/email', null, token)).body.email.pending, null);
});

test('the change routes need a session and an origin this deployment signs for', async () => {
  const { env, token } = await environment();
  assert.equal((await post(env, '/api/account/email/change/begin', {})).status, 401);
  assert.equal((await post(env, '/api/account/email/change', {})).status, 401);
  assert.equal((await post(env, '/api/account/email/apply', {})).status, 401);
  assert.equal((await request(env, 'GET', '/api/account/email', null)).status, 401);
  const workersDev = await post(env, '/api/account/email/change/begin', {}, token, 'https://eustress-api.example.workers.dev');
  assert.equal(workersDev.status, 400);
  assert.equal(workersDev.body.code, 'origin_not_allowed');
});

test('opening the cancel link cancels the change, ends the sign-ins, and tells the address the account had', async () => {
  const { env, key, sent, token } = await environment();
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: key.hex, address: 'new@example.com', challenge: begun.body.challenge });
  await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'new@example.com', signature: key.sign(message) }, token);
  const cancelToken = linkIn(sent.find((m) => m.to === 'owner@example.com'), 'cancel-email-change');
  sent.length = 0;
  const cancelled = await post(env, '/api/account/email/cancel', { token: cancelToken });
  assert.equal(cancelled.status, 200, JSON.stringify(cancelled.body));
  assert.equal(cancelled.body.sessions_ended, true);
  assert.equal((await request(env, 'GET', '/api/auth/me', null, token)).status, 401, 'the session the thief held is ended');
  assert.equal(sent.length, 1);
  assert.equal(sent[0].to, 'owner@example.com');
  assert.equal((await post(env, '/api/account/email/cancel', { token: cancelToken })).status, 400, 'once');
  assert.equal((await post(env, '/api/account/email/cancel', { token: 'nonsense' })).status, 400);
});

test('after the wait the owner confirms, the new address is on file, and everyone is told including the old one', async () => {
  const { env, key, sent, token } = await environment();
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: key.hex, address: 'new@example.com', challenge: begun.body.challenge });
  await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'new@example.com', signature: key.sign(message) }, token);
  const early = await post(env, '/api/account/email/apply', {}, token);
  assert.equal(early.status, 409);
  assert.equal(early.body.code, 'not_yet');

  sent.length = 0;
  const realNow = Date.now;
  Date.now = () => realNow() + EMAIL_CHANGE_WAIT_MS + HOUR;
  try {
    const later = mintToken(ACCOUNT);
    const applied = await post(env, '/api/account/email/apply', {}, later);
    assert.equal(applied.status, 200, JSON.stringify(applied.body));
    assert.equal(applied.body.address, 'new@example.com');
  } finally {
    Date.now = realNow;
  }
  assert.deepEqual(sent.map((m) => m.to).sort(), ['new@example.com', 'owner@example.com']);
  assert.equal(JSON.parse(await env.USERS.get(`user:${ACCOUNT}`)).email, 'owner@example.com', 'the user record is not rewritten');
  assert.equal(JSON.parse(await env.USERS.get(`email:${ACCOUNT}`)).address, 'new@example.com');
  const mine = await request(env, 'GET', '/api/auth/me', null, mintToken(ACCOUNT));
  assert.equal(mine.body.user?.email ?? mine.body.email, 'new@example.com', 'the signed-in site sees the new address');
});

test('a key replacement tells every address, the pending one included, and drops the pending change', async () => {
  const { env, key, sent, token } = await environment();
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: key.hex, address: 'thief@evil.example', challenge: begun.body.challenge });
  await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'thief@evil.example', signature: key.sign(message) }, token);
  sent.length = 0;

  const rb = await post(env, '/api/identity/rotate/begin', {}, token);
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: key.hex, newKey: newKey.hex, challenge: rb.body.challenge });
  const rot = await post(env, '/api/identity/rotate', { new_public_key: newKey.hex, challenge: rb.body.challenge, old_signature: key.sign(m), new_signature: newKey.sign(m) }, token);
  assert.equal(rot.status, 200, JSON.stringify(rot.body));
  assert.deepEqual(sent.map((s) => s.to).sort(), ['owner@example.com', 'thief@evil.example']);
  assert.equal(rot.body.notices_sent, 2);
  const status = await request(env, 'GET', '/api/account/email', null, rot.body.token);
  assert.equal(status.body.email.pending, null, 'the change the old key asked for is gone');
});

// ── Round 2 of the review ────────────────────────────────────────────────────

test('a record for an account that was deleted is not written back by a retry', async () => {
  const { env } = await setup();
  const started = await call(env, 'email_verify_start', { now: T0 });
  await env.USERS.delete(`user:${ACCOUNT}`);
  const confirmed = await call(env, 'email_verify_confirm', { token: secretOf(started.token), now: T0 });
  assert.equal(confirmed.ok, true);
  assert.equal(await env.USERS.get(`email:${ACCOUNT}`), null, 'nothing is kept for a deleted account');
  assert.equal((await call(env, 'state')).mirror_pending, 0, 'and nothing stays owed');
});

test('readAccountEmail prefers the account object\'s key, falls back to the user record, and survives garbage', async () => {
  const env = { USERS: fakeKv() };
  const user = { id: ACCOUNT, email: 'record@example.com' };
  assert.equal(await readAccountEmail(env, user), 'record@example.com');
  await env.USERS.put(`email:${ACCOUNT}`, JSON.stringify({ address: 'Object@Example.com', verified_at: null }));
  assert.equal(await readAccountEmail(env, user), 'object@example.com');
  await env.USERS.put(`email:${ACCOUNT}`, 'not json');
  assert.equal(await readAccountEmail(env, user), 'record@example.com');
  await env.USERS.put(`email:${ACCOUNT}`, JSON.stringify({ address: 'a@b.c, d@e.fg' }));
  assert.equal(await readAccountEmail(env, user), 'record@example.com', 'a stored value that is not one address is not used');
  assert.equal(await readAccountEmail(env, { id: ACCOUNT }), null);
  assert.equal(await readAccountEmail(undefined, { id: ACCOUNT, email: 'x@y.zz' }), 'x@y.zz');
});

test('the audit prints an address with a keyed hash: another key, another print; and not the plain hash', async () => {
  const printsWith = async (key) => {
    const env = { USERS: fakeKv(), AUDIT_PRINT_KEY: key };
    env.ACCOUNT_AUTH = fakeDoNamespace(AccountAuth, env);
    await env.USERS.put(`user:${ACCOUNT}`, JSON.stringify({ kyc_verified: true, id: ACCOUNT, username: 'owner', public_key: keyPair().hex, email: 'owner@example.com' }));
    await accountState(env, ACCOUNT);
    await call(env, 'email_verify_start', { now: T0 });
    const audit = await call(env, 'audit', { limit: 10 });
    return audit.records.find((r) => r.type === 'email_verify_requested').data.address;
  };
  const a = await printsWith('secret-one');
  const b = await printsWith('secret-two');
  const again = await printsWith('secret-one');
  assert.match(a, /^[0-9a-f]{16}$/);
  assert.equal(a, again);
  assert.notEqual(a, b);
  const plain = crypto.createHash('sha256').update('eustress-address:owner@example.com').digest('hex').slice(0, 16);
  assert.notEqual(a, plain, 'not the unsalted hash of the address');
});

test('a public link for an id nobody holds does not wake an account object', async () => {
  const { env, sent } = await environment();
  const probe = `${OTHER}`;
  assert.equal((await post(env, '/api/account/email/confirm', { token: `${probe}.${'a'.repeat(64)}` })).status, 400);
  assert.equal((await post(env, '/api/account/email/cancel', { token: `${probe}.${'b'.repeat(64)}` })).status, 400);
  assert.equal(env.ACCOUNT_AUTH.instances.has(probe), false, 'no object was created');
  void sent;
});

test('mail a stranger can cause has a small allowance per recipient, each kind apart; overflow is counted and the next day starts again', async () => {
  const sent = [];
  const env = { ...fakeEnv(), EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage };
  const rendered = renderEmailNotice('email_changed', { username: 'owner', newMasked: 'n***@example.com' });
  const run = async (kind, n, to = 'victim@example.com') => {
    const results = [];
    for (let i = 0; i < n; i++) results.push((await sendNotice(env, [to], rendered, { kind }))[0]);
    return results;
  };
  const verifies = await run('verify', MAIL_PER_DAY.verify + 3);
  assert.equal(verifies.filter((r) => r.sent).length, MAIL_PER_DAY.verify);
  assert.equal(verifies[MAIL_PER_DAY.verify].reason, 'daily_cap');
  const pendings = await run('pending', MAIL_PER_DAY.pending + 3);
  assert.equal(pendings.filter((r) => r.sent).length, MAIL_PER_DAY.pending, 'the notice to a named address has its own allowance');
  assert.equal((await run('verify', 1, 'someone-else@example.com'))[0].sent, true, 'another address is untouched');
  const overflow = Number(await env.USERS.get(`mailcap-overflow:${new Date().toISOString().slice(0, 10)}`));
  assert.equal(overflow, 6, 'the overflow is counted for an admin to see');
  assert.ok(MAIL_PER_DAY.verify <= 10 && MAIL_PER_DAY.pending <= 10, 'what a stranger can cause stays small');

  const realNow = Date.now;
  const realIso = Date.prototype.toISOString;
  Date.now = () => realNow() + 2 * DAY;
  Date.prototype.toISOString = function () { return realIso.call(new Date(this.getTime() + 2 * DAY)); };
  try {
    assert.equal((await run('verify', 1))[0].sent, true, 'two days later the allowance is new');
  } finally {
    Date.now = realNow;
    Date.prototype.toISOString = realIso;
  }
});

test('strangers using up an address\'s allowance do not stop the owner hearing of a key replacement, or of an address change', async () => {
  const { env, key, sent, token } = await environment();
  const rendered = renderEmailNotice('email_changed', { username: 'x', newMasked: 'n***@example.com' });
  // Everything a stranger can cause, spent on the owner's address and then some.
  for (let i = 0; i < 40; i++) {
    await sendNotice(env, ['owner@example.com'], rendered, { kind: 'verify' });
    await sendNotice(env, ['owner@example.com'], rendered, { kind: 'pending' });
  }
  sent.length = 0;
  const before = await post(env, '/api/account/email/start', {}, token);
  assert.equal(before.status, 429, 'a verification link is refused: the stranger allowance is used up');
  assert.equal(before.body.code, 'mail_limit');

  // The owner's own replacement still tells the owner.
  const rb = await post(env, '/api/identity/rotate/begin', {}, token);
  const newKey = keyPair();
  const m = rotationMessage({ origin: ORIGIN, accountId: ACCOUNT, oldKey: key.hex, newKey: newKey.hex, challenge: rb.body.challenge });
  const rot = await post(env, '/api/identity/rotate', { new_public_key: newKey.hex, challenge: rb.body.challenge, old_signature: key.sign(m), new_signature: newKey.sign(m) }, token);
  assert.equal(rot.status, 200, JSON.stringify(rot.body));
  assert.equal(rot.body.notices_sent, 1);
  assert.deepEqual(sent.map((x) => x.to), ['owner@example.com']);

  // And so does the warning of an address change.
  sent.length = 0;
  const begun = await post(env, '/api/account/email/change/begin', {}, rot.body.token);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: newKey.hex, address: 'new@example.com', challenge: begun.body.challenge });
  const asked = await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'new@example.com', signature: newKey.sign(message) }, rot.body.token);
  assert.equal(asked.status, 200, JSON.stringify(asked.body));
  assert.ok(sent.some((x) => x.to === 'owner@example.com' && bodyOf(x).includes('cancel-email-change')), 'the cancel link still reaches the owner');
});

test('the notice to an address a change names is mailed under its own allowance, not the notices\'', async () => {
  const { env, key, sent, token } = await environment();
  const rendered = renderEmailNotice('email_change_pending', { username: 'x', newMasked: 'n***@example.com', effectiveAt: T0 });
  for (let i = 0; i < 10; i++) await sendNotice(env, ['stranger@example.com'], rendered, { kind: 'pending' });
  sent.length = 0;
  const begun = await post(env, '/api/account/email/change/begin', {}, token);
  const message = emailChangeMessage({ origin: ORIGIN, accountId: ACCOUNT, key: key.hex, address: 'stranger@example.com', challenge: begun.body.challenge });
  const asked = await post(env, '/api/account/email/change', { challenge: begun.body.challenge, new_address: 'stranger@example.com', signature: key.sign(message) }, token);
  assert.equal(asked.status, 200, JSON.stringify(asked.body));
  assert.ok(!sent.some((x) => x.to === 'stranger@example.com'), 'nothing reached the stranger: their allowance was spent');
  assert.ok(sent.some((x) => x.to === 'owner@example.com'), 'the owner was still warned');
});

test('the identity-backup mail goes to the address the account\'s object keeps: the old address on the user record no longer qualifies', async () => {
  const { env, token } = await environment();
  await env.USERS.put(`email:${ACCOUNT}`, JSON.stringify({ address: 'new@example.com', verified_at: null }));
  const toOld = await post(env, '/api/identity/email-backup', { email: 'owner@example.com', toml_content: 'x' }, token);
  assert.equal(toOld.status, 403, JSON.stringify(toOld.body));
  assert.equal(toOld.body.code, 'email_not_on_account');
  const toNew = await post(env, '/api/identity/email-backup', { email: 'new@example.com', toml_content: 'x' }, token);
  assert.notEqual(toNew.body?.code, 'email_not_on_account', 'the new address passes the address check (the file is checked after)');
});

test('mail of a kind nobody named gets the smallest allowance, never the largest', async () => {
  const sent = [];
  const env = { ...fakeEnv(), EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage };
  const rendered = renderEmailNotice('email_changed', { username: 'owner', newMasked: 'n***@example.com' });
  let delivered = 0;
  for (let i = 0; i < 12; i++) delivered += (await sendNotice(env, ['x@example.com'], rendered, { kind: 'something-new' }))[0].sent ? 1 : 0;
  assert.equal(delivered, MAIL_PER_DAY.verify);
});

test('notices are counted per account and address: other accounts naming the same address cannot spend it', async () => {
  const sent = [];
  const env = { ...fakeEnv(), EMAIL: { send: async (m) => { sent.push(m); } }, __EmailMessage: StubMessage };
  const rendered = renderEmailNotice('email_changed', { username: 'owner', newMasked: 'n***@example.com' });
  const as = async (accountId, n) => {
    let delivered = 0;
    for (let i = 0; i < n; i++) delivered += (await sendNotice(env, ['victim@example.com'], rendered, { kind: 'notice', accountId }))[0].sent ? 1 : 0;
    return delivered;
  };
  // Any number of attacker accounts name the victim's address as theirs and fire notices at it.
  for (let a = 0; a < 20; a++) assert.equal(await as(`attacker-${a}`, 40), MAIL_PER_DAY.notice, 'each account gets its own allowance');
  assert.equal(await as('victim-account', 1), 1, 'and the victim\'s real notice still goes out');
  assert.equal(MAIL_PER_DAY.notice, 30);
  const day = new Date().toISOString().slice(0, 10);
  const hash = (await import('node:crypto')).createHash('sha256').update('victim@example.com').digest('hex').slice(0, 32);
  assert.equal(Number(await env.USERS.get(`mailcap-flood:${hash}:${day}`)), 20 * 40 + 1, 'the flood is counted per address for an admin to see');
});

test('a notice with no account named gets the smallest allowance, never the notice one', async () => {
  const env = { ...fakeEnv(), EMAIL: { send: async () => {} }, __EmailMessage: StubMessage };
  const rendered = renderEmailNotice('email_changed', { username: 'owner', newMasked: 'n***@example.com' });
  let delivered = 0;
  for (let i = 0; i < 12; i++) delivered += (await sendNotice(env, ['x@example.com'], rendered, { kind: 'notice' }))[0].sent ? 1 : 0;
  assert.equal(delivered, MAIL_PER_DAY.verify);
});
