// =============================================================================
// Replacing an identity key: the routes
// =============================================================================
//
//   POST /api/identity/rotate/begin   a one-time challenge and what the key owns
//   POST /api/identity/rotate         replace the key: two signatures, one step
//   POST /api/admin/accounts/replace-key/begin, /replace-key, GET /review-flags, POST /review-flags/resolve   an administrator replaces a key after a manual identity check
//   POST /api/account/key-restore     the owner's link puts the earlier key back
//
// Both need a signed-in session. The session alone is not enough to rotate: the
// old key signs too, so a stolen token cannot take the account. The state lives
// in the account's object (src/account_auth.mjs); this file is the HTTP side:
// checks the object cannot make (the origin the request reached, whether the
// new key is some other account's), what the key owns (API keys, webhook
// endpoints, the Discord link), the token for the device that rotated, and the
// notice. Design: docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md.
// =============================================================================

import { accountCall, accountState, adminReplaceMessage } from './account_auth.mjs';
import { readAccountEmail } from './email_ops.mjs';
import { isAdmin } from './admin_check.mjs';
import { walletBalance, hubStub, callObject } from './commerce.mjs';
import { ledgerBalanceMinor } from './bliss.mjs';
import { deltaCall } from './delta.mjs';
import { accountTimeZone } from './account_time_zone.mjs';
import { renderKeyNotice, renderAdminActionNotice, noticeRecipients, sendNotice } from './account_notices.mjs';
import { identityFileRequired } from './passkeys.mjs';

const MAX_REVOKE = 50;
const RECORD_ID = /^[A-Za-z0-9_]{1,80}$/;
/// A passkey's credential id: 16 to 1023 bytes, as base64url.
const PASSKEY_ID = /^[A-Za-z0-9_-]{22,1364}$/;

/// Whether the account holds anything worth stealing: a Bliss, Ticket or Delta
/// balance, a payout connection, or a published listing. A source that exists
/// and cannot be read counts as value (the longer freeze is the safe error); a
/// source this deployment does not have is not asked.
export async function accountHoldsValue(env, userId, user) {
  if (user?.stripe_connect_id) return true;
  const checks = [];
  if (env.PAYOUTS) checks.push(async () => (await ledgerBalanceMinor(env, userId)) > 0);
  if (env.WALLETS) checks.push(async () => (await walletBalance(env, userId)) > 0);
  if (env.DELTA_ACCOUNTS) {
    checks.push(async () => {
      const r = await deltaCall(env, userId, 'balance');
      if (!r.ok) throw new Error('delta balance unreadable');
      return Number(r.delta ?? r.balance ?? 0) > 0;
    });
  }
  if (env.SOCIAL) checks.push(async () => (parseInt((await env.SOCIAL.get(`simCount:${userId}`)) || '0', 10) || 0) > 0);
  for (const check of checks) {
    try {
      if (await check()) return true;
    } catch (e) {
      console.error('holds-value check failed, counting it as value:', e?.message || e);
      return true;
    }
  }
  return false;
}

/// What this key's owner has that acts as the account and does not end with the
/// session floor: API keys, webhook endpoints, the Discord link, the payout
/// connection. Listed so the owner can revoke them in the same step.
export async function listCredentials(env, userId, user, discordUnlinkedAt = null) {
  const out = { api_keys: [], webhooks: [], discord_linked: !!user?.discord_id && !discordUnlinkedAt, payout_connected: !!user?.stripe_connect_id, complete: true };
  if (!env.COMMERCE_HUBS) return out;
  try {
    const hub = hubStub(env, userId);
    const keys = await callObject(hub, 'list_keys', { account_id: userId });
    if (keys.ok) {
      out.api_keys = keys.data.map((k) => ({ id: k.id, name: k.name, mode: k.mode, key_prefix: k.key_prefix, last4: k.last4, created: k.created, last_used: k.last_used }));
    } else out.complete = false;
    for (const livemode of [true, false]) {
      const eps = await callObject(hub, 'list_endpoints', { account_id: userId, livemode });
      if (eps.ok) {
        for (const e of eps.data) {
          let host = '';
          try { host = new URL(e.url).host; } catch { /* not a URL */ }
          out.webhooks.push({ id: e.id, host, livemode: e.livemode, status: e.status, created: e.created, last_delivery: e.last_delivery });
        }
      } else out.complete = false;
    }
  } catch (e) {
    console.error('listing credentials failed:', e?.message || e);
    out.complete = false;
  }
  return out;
}

/// The part of a request that names credentials to revoke, or null when it is
/// not the shape we take. Ids only, bounded.
function revokeRequest(value) {
  if (value === undefined || value === null) return { api_keys: [], webhooks: [], discord: false, passkeys: [] };
  if (typeof value !== 'object' || Array.isArray(value)) return null;
  const list = (v, shape = RECORD_ID) => (v === undefined ? [] : Array.isArray(v) && v.length <= MAX_REVOKE && v.every((id) => typeof id === 'string' && shape.test(id)) ? v : null);
  const api_keys = list(value.api_keys);
  const webhooks = list(value.webhooks);
  const passkeys = list(value.passkeys, PASSKEY_ID);
  if (api_keys === null || webhooks === null || passkeys === null) return null;
  if (value.discord !== undefined && typeof value.discord !== 'boolean') return null;
  return { api_keys, webhooks, discord: value.discord === true, passkeys: [...new Set(passkeys)] };
}

async function revokeCredentials(env, userId, revoke) {
  const done = { api_keys: 0, webhooks: 0, discord: false, failed: [] };
  if (env.COMMERCE_HUBS) {
    const hub = hubStub(env, userId);
    for (const id of revoke.api_keys) {
      const r = await callObject(hub, 'revoke_key', { account_id: userId, id }).catch(() => ({ ok: false }));
      if (r.ok) done.api_keys++; else done.failed.push(id);
    }
    for (const id of revoke.webhooks) {
      const r = await callObject(hub, 'delete_endpoint', { account_id: userId, id }).catch(() => ({ ok: false }));
      if (r.ok) done.webhooks++; else done.failed.push(id);
    }
  }
  if (revoke.discord) {
    // Recorded in the account's object, not in the user record (see unlink_discord).
    const r = await accountCall(env, userId, 'unlink_discord');
    if (r.ok) done.discord = true; else done.failed.push('discord');
  }
  return done;
}

const objectFailure = (r, json, cors, { changing = false } = {}) => {
  if (r.code === 'signature_invalid') return json({ error: 'Signature verification failed', code: 'signature_invalid' }, 401, cors);
  if (r.code === 'challenge_invalid') return json({ error: 'That challenge is not valid. Start again.', code: 'challenge_invalid' }, 400, cors);
  if (r.code === 'key_unacceptable') return json({ error: 'That key cannot be used for this account.', code: 'key_unacceptable' }, r.status || 400, cors);
  if (r.code === 'no_current_key') return json({ error: 'This account has no usable key to replace. Contact support.', code: 'no_current_key' }, 409, cors);
  if (r.status === 503 || r.code === 'unavailable' || r.code === 'no_binding') {
    // A change that was asked for may have gone through before the answer was lost.
    // A retry with the same challenge would fail: the challenge is spent.
    return json({
      error: changing
        ? 'Key replacement is slow to answer. It may have gone through: try signing in with your NEW identity file before you try again, and start again from the beginning if that fails.'
        : 'Key replacement is temporarily unavailable. Try again in a minute.',
      code: 'account_unavailable',
    }, 503, cors);
  }
  return json({ error: 'Key replacement failed', code: r.code || 'failed' }, r.status && r.status < 500 ? r.status : 500, cors);
};

/// A response for a route here, or null when the path is not one of ours.
/// `deps`: verifyAuth, json, loginOrigins, createJwt, publicUser, cors.
export async function handleKeyRotationRoute(request, url, env, deps) {
  const { verifyAuth, json, cors } = deps;
  const path = url.pathname;
  const routes = ['/api/identity/rotate/begin', '/api/identity/rotate', '/api/identity/rotate/prompt-seen'];
  if (request.method !== 'POST' || !routes.includes(path)) return null;
  if (!env.ACCOUNT_AUTH) return json({ error: 'Key replacement is not available on this deployment', code: 'not_configured' }, 501, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  // Replacing the identity file is a key change: a passkey session cannot ask for it.
  if (path !== '/api/identity/rotate/prompt-seen' && (await deps.authMethod(request, env)) !== 'key') return identityFileRequired(json, cors);

  // The origin the request reached is part of what the keys sign, and it must be
  // one this deployment signs for: the same list as sign-in.
  const origin = new URL(request.url).origin;
  if (!deps.loginOrigins(env).has(origin)) return json({ error: 'This address does not accept key replacement', code: 'origin_not_allowed' }, 400, cors);

  if (env.AUTH_RATE_LIMITER) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `identity-rotate:${userId}` });
    if (!success) return json({ error: 'Too many requests. Try again in a minute.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '60' });
  }

  const state = await accountState(env, userId);
  if (!state.ok) return objectFailure(state, json, cors);
  const userRaw = await env.USERS.get(`user:${userId}`);
  if (!userRaw) return json({ error: 'Account not found' }, 404, cors);
  const user = JSON.parse(userRaw);

  if (path === '/api/identity/rotate/prompt-seen') {
    const seen = await accountCall(env, userId, 'mark_prompt_seen');
    return seen.ok ? json({ ok: true, key_prompt_seen_at: seen.prompt_seen_at }, 200, cors) : objectFailure(seen, json, cors);
  }

  if (path === '/api/identity/rotate/begin') {
    const begun = await accountCall(env, userId, 'begin_rotation');
    if (!begun.ok) return objectFailure(begun, json, cors);
    const credentials = await listCredentials(env, userId, user, state.discord_unlinked_at);
    // The passkeys, so the owner can remove them in the same step (the site ticks every one).
    const passkeys = await accountCall(env, userId, 'passkey_list');
    credentials.passkeys = passkeys.ok ? passkeys.passkeys : [];
    if (!passkeys.ok) credentials.complete = false;
    return json({
      challenge: begun.challenge,
      expires_at: begun.expires_at,
      origin,
      account_id: userId,
      public_key: begun.public_key,
      format: 'eustress-key-rotation-v1',
      credentials,
    }, 200, cors);
  }

  // POST /api/identity/rotate
  const body = await request.json().catch(() => ({}));
  const revoke = revokeRequest(body.revoke);
  if (!revoke) return json({ error: 'revoke must name credential ids', code: 'bad_request' }, 400, cors);

  // Another account's key, current or replaced, is never this account's new key.
  // KV can lag, so this is the early refusal; the object refuses the rest.
  const wanted = typeof body.new_public_key === 'string' ? body.new_public_key.toLowerCase() : '';
  if (/^[0-9a-f]{64}$/.test(wanted)) {
    const owner = (await env.USERS.get(`pubkey:${wanted}`)) || (await env.USERS.get(`retired-key:${wanted}`));
    if (owner && owner !== userId) return json({ error: 'That key cannot be used for this account.', code: 'key_unacceptable' }, 409, cors);
  }

  const holdsValue = await accountHoldsValue(env, userId, user);
  const rotated = await accountCall(env, userId, 'rotate', {
    new_public_key: body.new_public_key,
    challenge: body.challenge,
    old_signature: body.old_signature,
    new_signature: body.new_signature,
    origin,
    holds_value: holdsValue,
    username: user.username,
    // Removed in the same step as the key, by the object.
    revoke_passkeys: revoke.passkeys,
  });
  if (!rotated.ok) return objectFailure(rotated, json, cors, { changing: true });

  // The key has changed. What follows reports and tidies; none of it can undo it.
  const revoked = await revokeCredentials(env, userId, revoke);
  const passkeysRemoved = new Set(rotated.passkeys_removed || []);
  revoked.passkeys = passkeysRemoved.size;
  for (const id of revoke.passkeys) if (!passkeysRemoved.has(id)) revoked.failed.push(id);
  const fresh = JSON.parse((await env.USERS.get(`user:${userId}`)) || JSON.stringify(user));
  const rendered = renderKeyNotice('rotation', {
    username: user.username, at: rotated.rotated_at,
    oldFingerprint: rotated.old_key_fingerprint, newFingerprint: rotated.new_key_fingerprint,
    valueFrozenUntil: rotated.value_frozen_until, payoutPinnedUntil: rotated.payout_pinned_until,
    revoked, timeZone: await accountTimeZone(env, userId), assetOrigin: origin,
  });
  // One rule for every key notice: the verified address, a pending one, the one on
  // file, and a replaced one for 30 days (the object's list), plus what the user
  // record holds.
  const recipients = [...new Set([...(rotated.recipients || []), ...noticeRecipients(user)])];
  const notices = await sendNotice(env, recipients, rendered, { accountId: userId });
  // Say who was told. What is left is the object's to send: it holds the notice
  // from the same write as the key, and sends it itself after a minute.
  await accountCall(env, userId, 'ack_notice', { id: rotated.notice_id, delivered: recipients.filter((_, i) => notices[i]?.sent) });

  return json({
    ok: true,
    token: await deps.createJwt(userId, env.JWT_SECRET, rotated.floor),
    user: await deps.publicUser({ ...fresh, public_key: body.new_public_key.toLowerCase() }, env),
    key_epoch: rotated.key_epoch,
    rotated_at: rotated.rotated_at,
    value_frozen_until: rotated.value_frozen_until,
    payout_pinned_until: rotated.payout_pinned_until,
    holds_value: holdsValue,
    revoked,
    notices_sent: notices.filter((n) => n.sent).length,
  }, 200, cors);
}

// -----------------------------------------------------------------------------
// An administrator replaces a key
// -----------------------------------------------------------------------------

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const SECRET = /^[0-9a-f]{64}$/;

/// `ADMIN_REPLACE_KEY_ENABLED` is "true" where this route may be used. Production
/// leaves it off until the administrator's signature, the owner's way to undo it
/// and the new key's proof of possession are all reviewed and McKale says go.
const enabled = (env) => String(env.ADMIN_REPLACE_KEY_ENABLED || '').toLowerCase() === 'true';

/// The account an administrator's request names, or an error response. The
/// target is never the administrator, and never another administrator: an
/// administrator's own key is not replaced by this route.
async function adminTarget(env, body, adminId, json, cors) {
  let userId = null;
  if (typeof body.account_id === 'string' && UUID.test(body.account_id)) userId = body.account_id.toLowerCase();
  else if (typeof body.username === 'string' && body.username) userId = await env.USERS.get(`username:${body.username}`);
  if (!userId) return { response: json({ error: 'No such account', code: 'no_account' }, 404, cors) };
  const userRaw = await env.USERS.get(`user:${userId}`);
  if (!userRaw) return { response: json({ error: 'No such account', code: 'no_account' }, 404, cors) };
  if (userId === adminId || (await isAdmin(env, userId))) {
    return { response: json({ error: 'An administrator\'s key is not replaced by this route.', code: 'target_is_admin' }, 403, cors) };
  }
  const user = JSON.parse(userRaw);
  // Another account's key, current or replaced, is never this account's new key.
  const wanted = typeof body.new_public_key === 'string' ? body.new_public_key.toLowerCase() : '';
  if (/^[0-9a-f]{64}$/.test(wanted)) {
    const owner = (await env.USERS.get(`pubkey:${wanted}`)) || (await env.USERS.get(`retired-key:${wanted}`));
    if (owner && owner !== userId) return { response: json({ error: 'That key cannot be used for this account.', code: 'key_unacceptable' }, 409, cors) };
  }
  return { userId, user, wanted };
}

/// Where a summary of every administrator replacement goes besides the account's
/// own addresses: the security mailbox and the other administrators.
async function secondChannel(env, adminId) {
  const addresses = new Set([String(env.SECURITY_MAILBOX || 'security@eustress.dev').toLowerCase()]);
  try {
    const listed = await env.USERS.list({ prefix: 'admin:', limit: 100 });
    for (const k of listed.keys || []) {
      const id = k.name.slice('admin:'.length);
      if (id === adminId) continue;
      const raw = await env.USERS.get(`user:${id}`);
      if (!raw) continue;
      const address = await readAccountEmail(env, JSON.parse(raw));
      if (typeof address === 'string' && address) addresses.add(address.toLowerCase());
    }
  } catch (e) {
    console.error('listing the other administrators failed:', e?.message || e);
  }
  return [...addresses];
}

/**
 * POST /api/admin/accounts/replace-key/begin   the administrator's challenge
 * POST /api/admin/accounts/replace-key         the replacement
 * GET  /api/admin/accounts/review-flags        every open flag (a replacement undone, a revocation given up on), one entry each
 * POST /api/admin/accounts/review-flags/resolve  close one flag: { account_id, flag_id?, note }
 *
 * For an owner who has lost their key and cannot sign in: after a person has
 * checked who they are, an administrator gives the account a new public key,
 * which the owner made on their own computer (only its public half is sent here).
 *
 * The request is not a session alone. The administrator signs it with their OWN
 * identity key, through the same challenge flow as a key replacement, over the
 * administrator, the account, the new key and the case; and the NEW key signs the
 * username and the case, so a key nobody holds cannot retire the old one. At most
 * five replacements a day per administrator. The case and the reason go into the
 * account's audit chain and the admin log; every address is told, with a link that
 * puts the earlier key back for the length of the freeze (at least a week); a
 * summary goes to the security mailbox and the other administrators; every session
 * ends, value is frozen, the payout destination is pinned, and every API key,
 * webhook endpoint and the Discord link is revoked, durably. The administrator
 * gets no token: the owner signs in with the new file.
 */
export async function handleAdminReplaceKeyRoute(request, url, env, deps) {
  const { requireAdmin, json, cors, auditLog } = deps;
  const path = url.pathname;
  const begin = path === '/api/admin/accounts/replace-key/begin' && request.method === 'POST';
  const replace = path === '/api/admin/accounts/replace-key' && request.method === 'POST';
  const flags = path === '/api/admin/accounts/review-flags' && request.method === 'GET';
  const resolve = path === '/api/admin/accounts/review-flags/resolve' && request.method === 'POST';
  if (!begin && !replace && !flags && !resolve) return null;
  if (!enabled(env)) return json({ error: 'Not found', code: 'not_enabled' }, 404, cors);
  if (!env.ACCOUNT_AUTH) return json({ error: 'Key replacement is not available on this deployment', code: 'not_configured' }, 501, cors);

  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  if (flags) {
    const listed = await env.USERS.list({ prefix: 'admin-review:', limit: 100 });
    const out = [];
    for (const k of listed.keys || []) {
      try {
        const held = JSON.parse((await env.USERS.get(k.name)) || '[]');
        for (const flag of Array.isArray(held) ? held : [held]) out.push({ account_id: k.name.slice('admin-review:'.length), ...flag });
      } catch { /* skip */ }
    }
    return json({ ok: true, flags: out }, 200, cors);
  }

  if (resolve) {
    const body = await request.json().catch(() => ({}));
    const id = typeof body.account_id === 'string' && UUID.test(body.account_id) ? body.account_id.toLowerCase() : null;
    if (!id || !(await env.USERS.get(`user:${id}`))) return json({ error: 'No such account', code: 'no_account' }, 404, cors);
    const held = await accountState(env, id);
    if (!held.ok) return objectFailure(held, json, cors);
    const r = await accountCall(env, id, 'resolve_review', { admin_id: adminId, note: body.note, flag_id: body.flag_id });
    if (!r.ok) return json({ error: r.error || 'Could not resolve', code: r.code || 'failed' }, r.status && r.status < 500 ? r.status : 500, cors);
    await auditLog(env, 'ADMIN_RESOLVE_REVIEW', adminId, id, { note: String(body.note).trim(), flag_id: body.flag_id ?? null });
    return json({ ok: true, remaining: r.remaining }, 200, cors);
  }

  if (env.AUTH_RATE_LIMITER) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `admin-replace:${adminId}` });
    if (!success) return json({ error: 'Too many requests. Try again in a minute.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '60' });
  }
  const origin = new URL(request.url).origin;
  const body = await request.json().catch(() => ({}));
  const target = await adminTarget(env, body, adminId, json, cors);
  if (target.response) return target.response;
  const { userId, user, wanted } = target;
  const adminHeld = await accountState(env, adminId);
  if (!adminHeld.ok) return objectFailure(adminHeld, json, cors);

  if (begin) {
    const begun = await accountCall(env, adminId, 'begin_admin_action');
    if (!begun.ok) return objectFailure(begun, json, cors);
    return json({
      challenge: begun.challenge, expires_at: begun.expires_at, origin, admin_id: adminId, account_id: userId,
      new_public_key: wanted, case_ref: body.case_ref, public_key: begun.public_key, format: 'eustress-admin-replace-v1',
    }, 200, cors);
  }

  // The administrator's own signature, over what this request does.
  const message = adminReplaceMessage({
    origin, adminId, accountId: userId, newKey: wanted, caseRef: String(body.case_ref), challenge: String(body.challenge),
  });
  const signed = await accountCall(env, adminId, 'verify_admin_action', { challenge: body.challenge, message, signature: body.admin_signature });
  if (!signed.ok) {
    if (signed.code === 'rate_limited') return json({ error: 'Too many key replacements today for this administrator.', code: 'rate_limited' }, 429, cors);
    return objectFailure(signed, json, cors);
  }

  const held = await accountState(env, userId);
  if (!held.ok) return objectFailure(held, json, cors);
  const holdsValue = await accountHoldsValue(env, userId, user);
  // Everything that acts as the account is owed a revocation, in the same write as the key.
  const creds = await listCredentials(env, userId, user, held.discord_unlinked_at);
  const revoke = { api_keys: creds.api_keys.map((k) => k.id), webhooks: creds.webhooks.map((w) => w.id), discord: creds.discord_linked };
  const request_ = {
    admin_id: adminId, case_ref: body.case_ref, reason: body.reason, new_public_key: body.new_public_key,
    new_key_signature: body.new_key_signature, origin, holds_value: holdsValue, username: user.username, revoke,
  };
  const refused = (r) => {
    if (r.code === 'bad_request') return json({ error: r.error, code: 'bad_request' }, 400, cors);
    if (r.code === 'new_key_signature_invalid') return json({ error: r.error, code: 'new_key_signature_invalid' }, 401, cors);
    if (r.code === 'replacement_in_window') return json({ error: 'A replacement of this account is still open to undo. Wait for the owner\'s link to lapse, or for it to be used.', code: 'replacement_in_window', until: r.until }, 409, cors);
    return objectFailure(r, json, cors, { changing: true });
  };

  // Everything is checked first (a dry run), then the second channel is told, and only
  // then does anything change: a replacement nobody but the owner heard of is exactly
  // what a rogue administrator wants, so a second channel that did not go stops it.
  const dry = await accountCall(env, userId, 'admin_replace_key', { ...request_, dry_run: true });
  if (!dry.ok) return refused(dry);
  const adminUser = JSON.parse((await env.USERS.get(`user:${adminId}`)) || '{}');
  const channel = await secondChannel(env, adminId);
  const summary = await sendNotice(env, channel, renderAdminActionNotice({
    adminName: adminUser.username || adminId, adminId, targetName: user.username, targetId: userId,
    caseRef: String(body.case_ref), reason: String(body.reason).trim(), at: new Date().toISOString(),
  }), { accountId: adminId });
  if (!summary[0]?.sent) {
    return json({ error: 'The security mailbox could not be told, so nothing was changed. Try again when mail is working.', code: 'second_channel_unavailable' }, 503, cors);
  }
  const replaced = await accountCall(env, userId, 'admin_replace_key', request_);
  if (!replaced.ok) return refused(replaced);

  // The admin log first: a Worker that dies after this still leaves the record.
  await auditLog(env, 'ADMIN_REPLACE_KEY', adminId, user.username, { user_id: userId, case_ref: String(body.case_ref), reason: String(body.reason).trim() });

  // Revoke now; what fails is owed by the object and retried by its alarm.
  const revoked = await revokeCredentials(env, userId, revoke);
  // Every passkey went with the key, in the object's same write.
  revoked.passkeys = (replaced.passkeys_removed || []).length;
  const failed = new Set(revoked.failed);
  await accountCall(env, userId, 'ack_revoked', {
    api_keys: revoke.api_keys.filter((id) => !failed.has(id)), webhooks: revoke.webhooks.filter((id) => !failed.has(id)),
  });
  // A key made between the list and the change.
  const late = await listCredentials(env, userId, user, true);
  const stragglers = { api_keys: late.api_keys.map((k) => k.id), webhooks: late.webhooks.map((w) => w.id), discord: false };
  if (stragglers.api_keys.length || stragglers.webhooks.length) {
    const more = await revokeCredentials(env, userId, stragglers);
    revoked.api_keys += more.api_keys;
    revoked.webhooks += more.webhooks;
    revoked.failed.push(...more.failed);
  }
  const state = await accountCall(env, userId, 'state');
  const revokedComplete = !!state.ok && state.credentials_pending === 0 && revoked.failed.length === 0;

  const rendered = renderKeyNotice('admin_replacement', {
    username: user.username, at: replaced.rotated_at, caseRef: String(body.case_ref),
    oldFingerprint: replaced.old_key_fingerprint, newFingerprint: replaced.new_key_fingerprint,
    valueFrozenUntil: replaced.value_frozen_until, payoutPinnedUntil: replaced.payout_pinned_until, revoked,
    undoLink: replaced.undo_link, undoUntil: replaced.undo_until ? Date.parse(replaced.undo_until) : null,
    timeZone: await accountTimeZone(env, userId), assetOrigin: new URL(request.url).origin,
  });
  const recipients = [...new Set([...(replaced.recipients || []), ...noticeRecipients(user)])];
  const notices = await sendNotice(env, recipients, rendered, { accountId: userId });
  await accountCall(env, userId, 'ack_notice', { id: replaced.notice_id, delivered: recipients.filter((_, i) => notices[i]?.sent) });

  return json({
    ok: true,
    account_id: userId,
    username: user.username,
    key_epoch: replaced.key_epoch,
    replaced_at: replaced.rotated_at,
    value_frozen_until: replaced.value_frozen_until,
    payout_pinned_until: replaced.payout_pinned_until,
    undo_until: replaced.undo_until,
    holds_value: holdsValue,
    revoked,
    revoked_complete: revokedComplete,
    second_channel_sent: summary.filter((n) => n.sent).length,
    notices_sent: notices.filter((n) => n.sent).length,
  }, 200, cors);
}

/**
 * POST /api/account/key-restore   { token }   no session: the link is the proof
 *
 * The link in the notice of an administrator's replacement. It puts the earlier
 * key back, ends every session, keeps value frozen and the payout pinned, marks
 * the case for review, and tells every address. It works once, for the length of
 * the freeze and at least a week.
 */
export async function handleKeyRestoreRoute(request, url, env, deps) {
  const { json, cors } = deps;
  if (url.pathname !== '/api/account/key-restore' || request.method !== 'POST') return null;
  if (!env.ACCOUNT_AUTH) return json({ error: 'This is not available on this deployment', code: 'not_configured' }, 501, cors);
  const body = await request.json().catch(() => ({}));
  const m = typeof body.token === 'string' ? /^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.([0-9a-f]{64})$/.exec(body.token) : null;
  const invalid = () => json({ error: 'This link is not valid or has expired.', code: 'token_invalid' }, 400, cors);
  if (!m || !SECRET.test(m[2])) return invalid();
  const userId = m[1];
  const userRaw = await env.USERS.get(`user:${userId}`);
  if (!userRaw) return invalid();
  const restored = await accountCall(env, userId, 'restore_key', { token: m[2] });
  if (!restored.ok) return restored.status === 503 ? objectFailure(restored, json, cors) : invalid();

  const user = JSON.parse(userRaw);
  const rendered = renderKeyNotice('key_restored', {
    username: user.username, at: restored.restored_at, replacedAt: restored.replaced_at, valueFrozenUntil: restored.value_frozen_until,
    timeZone: await accountTimeZone(env, userId), assetOrigin: new URL(request.url).origin,
  });
  const recipients = [...new Set([...(restored.recipients || []), ...noticeRecipients(user)])];
  const notices = await sendNotice(env, recipients, rendered, { accountId: userId });
  await accountCall(env, userId, 'ack_notice', { id: restored.notice_id, delivered: recipients.filter((_, i) => notices[i]?.sent) });
  return json({ ok: true, key_epoch: restored.key_epoch }, 200, cors);
}
