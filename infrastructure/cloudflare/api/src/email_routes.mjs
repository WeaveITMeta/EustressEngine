// =============================================================================
// The account's email: the routes
// =============================================================================
//
//   GET  /api/account/email                what is on file, verified, pending
//   POST /api/account/email/start          send the first verification link (to the address on file)
//   POST /api/account/email/confirm        the link was opened: { token }            (no session)
//   POST /api/account/email/change/begin   a one-time challenge for a new address
//   POST /api/account/email/change         ask for a new address: signed by the current key
//   POST /api/account/email/cancel         stop a pending change: { token } or a session
//   POST /api/account/email/apply          after the wait, make it the address on file
//
// The state and the rules live in the account's object (src/email_ops.mjs); this
// file is the HTTP side and the mail. A token that opens a link carries the
// account's id and a random secret; the object keeps only the secret's digest.
// The cancel link goes to the addresses the account already had and is never in
// a response. Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md,
// "Verified email".
// =============================================================================

import { accountCall, accountState } from './account_auth.mjs';
import { renderEmailNotice, maskAddress, sendNotice } from './account_notices.mjs';
import { accountTimeZone } from './account_time_zone.mjs';

const ROUTES = new Set([
  '/api/account/email',
  '/api/account/email/start',
  '/api/account/email/confirm',
  '/api/account/email/change/begin',
  '/api/account/email/change',
  '/api/account/email/cancel',
  '/api/account/email/apply',
]);

/// The routes that change the address or its verification. A passkey session is
/// refused on them (identity_file_required): the address is how the account is
/// recovered and told about key changes. Reading it, and cancelling a pending
/// change (which only stops one), stay open to any session.
const CHANGES_ADDRESS = new Set([
  '/api/account/email/start',
  '/api/account/email/change/begin',
  '/api/account/email/change',
  '/api/account/email/apply',
]);

const TOKEN = /^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.([0-9a-f]{64})$/;

function parseToken(value) {
  const m = typeof value === 'string' ? TOKEN.exec(value) : null;
  return m ? { accountId: m[1], secret: m[2] } : null;
}

const objectFailure = (r, json, cors) => {
  if (r.code === 'signature_invalid') return json({ error: 'Signature verification failed', code: 'signature_invalid' }, 401, cors);
  if (r.code === 'challenge_invalid') return json({ error: 'That challenge is not valid. Start again.', code: 'challenge_invalid' }, 400, cors);
  if (r.code === 'bad_address') return json({ error: 'That is not an email address.', code: 'bad_address' }, 400, cors);
  if (r.code === 'same_address') return json({ error: 'That is already the address on this account.', code: 'same_address' }, 409, cors);
  if (r.code === 'no_email_on_file') return json({ error: 'This account has no email address on file. Add one first.', code: 'no_email_on_file' }, 409, cors);
  if (r.code === 'no_pending') return json({ error: 'There is no address change waiting.', code: 'no_pending' }, 404, cors);
  if (r.code === 'not_yet') return json({ error: 'The waiting period has not passed.', code: 'not_yet', effective_at: r.effective_at }, 409, cors);
  if (r.code === 'rate_limited') return json({ error: 'Too many requests. Try again later.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '3600' });
  if (r.status === 503 || r.code === 'unavailable' || r.code === 'no_binding') return json({ error: 'This is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  return json({ error: 'The request failed', code: r.code || 'failed' }, r.status && r.status < 500 ? r.status : 500, cors);
};

const invalidLink = (json, cors) => json({ error: 'This link is not valid or has expired.', code: 'token_invalid' }, 400, cors);
const expiredLink = (json, cors) => json({ error: 'This link has expired.', code: 'token_expired' }, 410, cors);

async function usernameOf(env, userId) {
  try {
    return JSON.parse((await env.USERS.get(`user:${userId}`)) || '{}').username || '';
  } catch {
    return '';
  }
}

/// A response for a route here, or null when the path is not one of ours.
/// `deps`: authenticate (the session check for this route), requireIdentityFileSession,
/// json, cors, loginOrigins, siteOrigin.
export async function handleEmailRoute(request, url, env, deps) {
  const { json, cors } = deps;
  const path = url.pathname;
  if (!ROUTES.has(path)) return null;
  const isGet = path === '/api/account/email';
  if (request.method !== (isGet ? 'GET' : 'POST')) return null;
  if (!env.ACCOUNT_AUTH) return json({ error: 'This is not available on this deployment', code: 'not_configured' }, 501, cors);

  const body = isGet ? {} : await request.json().catch(() => ({}));

  // The two routes a link opens carry no session: the token is the proof.
  if (path === '/api/account/email/confirm') {
    const t = parseToken(body.token);
    if (!t) return invalidLink(json, cors);
    // An id nobody holds is not worth waking an object for.
    if (!(await env.USERS.get(`user:${t.accountId}`))) return invalidLink(json, cors);
    const r = await accountCall(env, t.accountId, 'email_verify_confirm', { token: t.secret });
    if (r.status === 503) return objectFailure(r, json, cors);
    if (r.code === 'token_expired') return expiredLink(json, cors);
    return r.ok ? json({ ok: true, verified_at: r.verified_at, aged_at: r.aged_at, address: maskAddress(r.address) }, 200, cors) : invalidLink(json, cors);
  }

  let userId = null;
  let viaLink = null;
  if (path === '/api/account/email/cancel' && body.token !== undefined) {
    viaLink = parseToken(body.token);
    if (!viaLink) return invalidLink(json, cors);
    if (!(await env.USERS.get(`user:${viaLink.accountId}`))) return invalidLink(json, cors);
    userId = viaLink.accountId;
  } else {
    userId = await deps.authenticate(request, env);
    if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
    if (CHANGES_ADDRESS.has(path)) {
      const refused = await deps.requireIdentityFileSession(request, env, cors);
      if (refused) return refused;
    }
    if (env.AUTH_RATE_LIMITER) {
      const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `account-email:${userId}` });
      if (!success) return json({ error: 'Too many requests. Try again in a minute.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '60' });
    }
  }

  const username = await usernameOf(env, userId);
  const origin = new URL(request.url).origin;
  // Every time in a notice is in the account's zone, and the logo comes from this Worker.
  const look = { timeZone: await accountTimeZone(env, userId), assetOrigin: origin };

  if (viaLink) {
    const r = await accountCall(env, userId, 'email_change_cancel', { token: viaLink.secret });
    if (!r.ok) return r.status === 503 ? objectFailure(r, json, cors) : invalidLink(json, cors);
    const rendered = renderEmailNotice('email_change_cancelled', { username, cancelledMasked: maskAddress(r.notify.cancelled_address), sessionsEnded: r.sessions_ended, ...look });
    await sendNotice(env, r.notify.existing, rendered, { accountId: userId });
    return json({ ok: true, cancelled: true, sessions_ended: r.sessions_ended }, 200, cors);
  }

  // Everything below needs the object seeded from KV.
  const held = await accountState(env, userId);
  if (!held.ok) return objectFailure(held, json, cors);

  if (isGet) {
    const r = await accountCall(env, userId, 'email_status');
    return r.ok ? json({ ok: true, email: r.email }, 200, cors) : objectFailure(r, json, cors);
  }

  if (path === '/api/account/email/start') {
    const r = await accountCall(env, userId, 'email_verify_start');
    if (!r.ok) return objectFailure(r, json, cors);
    if (r.already_verified) return json({ ok: true, already_verified: true, verified_at: r.verified_at }, 200, cors);
    const link = `${deps.siteOrigin(env)}/verify-email?token=${r.token}`;
    const sent = await sendNotice(env, [r.address], renderEmailNotice('email_verify', { username, link, expiresAt: r.expires_at, ...look }), { kind: 'verify' });
    if (sent[0]?.reason === 'daily_cap') return json({ error: 'Too many emails have gone to this address today. Try again tomorrow.', code: 'mail_limit' }, 429, { ...cors, 'Retry-After': '86400' });
    if (!sent[0]?.sent) return json({ error: 'The email could not be sent. Try again in a minute.', code: 'email_send_failed' }, 502, cors);
    return json({ ok: true, sent_to: maskAddress(r.address), expires_at: r.expires_at }, 200, cors);
  }

  if (path === '/api/account/email/apply') {
    const r = await accountCall(env, userId, 'email_change_apply');
    if (!r.ok) return objectFailure(r, json, cors);
    await sendNotice(env, r.notify.recipients, renderEmailNotice('email_changed', { username, newMasked: maskAddress(r.address), ...look }), { accountId: userId });
    return json({ ok: true, address: r.address, replaced_until: r.replaced_until }, 200, cors);
  }

  if (path === '/api/account/email/cancel') {
    const r = await accountCall(env, userId, 'email_change_cancel', { by_session: true });
    if (!r.ok) return objectFailure(r, json, cors);
    await sendNotice(env, r.notify.existing, renderEmailNotice('email_change_cancelled', { username, cancelledMasked: maskAddress(r.notify.cancelled_address), sessionsEnded: false, ...look }), { accountId: userId });
    return json({ ok: true, cancelled: true }, 200, cors);
  }

  // The two that the current key signs name the origin the request reached, and
  // it must be one this deployment signs for: the same list as sign-in.
  if (!deps.loginOrigins(env).has(origin)) return json({ error: 'This address does not accept this request', code: 'origin_not_allowed' }, 400, cors);

  if (path === '/api/account/email/change/begin') {
    const r = await accountCall(env, userId, 'email_change_begin');
    if (!r.ok) return objectFailure(r, json, cors);
    return json({
      challenge: r.challenge, expires_at: r.expires_at, origin, account_id: userId, public_key: r.public_key,
      format: 'eustress-email-change-v1',
    }, 200, cors);
  }

  // POST /api/account/email/change
  const r = await accountCall(env, userId, 'email_change_request', {
    challenge: body.challenge, new_address: body.new_address, signature: body.signature, origin,
  });
  if (!r.ok) return objectFailure(r, json, cors);
  const site = deps.siteOrigin(env);
  const effectiveAt = Date.parse(r.pending.effective_at);
  const common = { username, newMasked: maskAddress(r.notify.pending_address), effectiveAt, ...look };
  const warned = await sendNotice(env, r.notify.existing, renderEmailNotice('email_change_existing', {
    ...common, cancelLink: `${site}/cancel-email-change?token=${r.cancel_token}`,
  }), { accountId: userId });
  // The address a requester names is one a stranger can pick: its own small allowance.
  await sendNotice(env, [r.notify.pending_address], renderEmailNotice('email_change_pending', common), { kind: 'pending' });
  return json({ ok: true, pending: r.pending, existing_addresses_told: warned.filter((n) => n.sent).length }, 200, cors);
}
