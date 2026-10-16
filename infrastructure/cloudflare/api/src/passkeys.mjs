// =============================================================================
// Passkeys: the routes
// =============================================================================
//
//   GET  /api/passkeys               the account's passkeys (any session)
//   POST /api/passkeys/add/begin     options for navigator.credentials.create (identity-file session)
//   POST /api/passkeys/add           add one: the WebAuthn answer and the identity key's signature
//   POST /api/passkeys/remove/begin  a challenge to remove one (identity-file session)
//   POST /api/passkeys/remove        remove one: the identity key's signature
//   POST /api/passkeys/login/begin   options for navigator.credentials.get (no session)
//   POST /api/passkeys/login         sign in to the website with a passkey (no session)
//
// A passkey is a second key on the account. The identity file stays the master
// key: adding or removing a passkey, and replacing the identity file, need an
// identity-file session (a token whose `amr` is "key") and the identity key's
// signature. A passkey session can do everything else.
//
// The passkeys live in the account's object (src/account_auth.mjs), which checks
// a sign-in's signature and sign count. Which account holds a credential id is a
// PasskeyIndex object's to say (src/passkey_index.mjs), strongly consistent. The
// WebAuthn parsing is in src/webauthn.mjs. Every route answers 503
// `passkeys_unavailable` where PASSKEY_RP_ID or PASSKEY_ORIGINS is not set.
// =============================================================================

import { accountCall, accountState, sha256Hex, MAX_PASSKEYS } from './account_auth.mjs';
import { passkeyIndexCall } from './passkey_index.mjs';
import { passkeyConfig, verifyRegistration, checkAssertion, fromBase64url, toBase64url, hexOfBytes, bytesOfHex, PASSKEY_ALGS } from './webauthn.mjs';
import { renderKeyNotice, noticeRecipients, sendNotice } from './account_notices.mjs';
import { accountTimeZone } from './account_time_zone.mjs';
import { cleanText } from './purchases.mjs';

export const PASSKEY_TIMEOUT_MS = 300000;
const CHALLENGE_TTL_S = 300;
const NAME_MAX_BYTES = 64;

/// The answer to a key change asked for by a passkey session.
export function identityFileRequired(json, cors) {
  return json({ error: 'Sign in with your identity file to change your keys.', code: 'identity_file_required' }, 403, cors);
}

/// The KV key of a passkey sign-in challenge: under its digest, as the
/// identity-file sign-in challenges are, with a prefix of its own so neither
/// flow can spend the other's.
const loginChallengeKey = async (hex) => `pkchal:${await sha256Hex(hex)}`;

const WORDS = {
  challenge_invalid: 'That challenge is not valid. Start again.',
  signature_invalid: 'Signature verification failed',
  passkey_registered: 'This passkey is already registered.',
  too_many_passkeys: `An account can hold at most ${MAX_PASSKEYS} passkeys. Remove one first.`,
  passkey_not_found: 'That passkey is not on this account.',
  // One answer at sign-in for an unknown passkey, one that names another account, a
  // bad signature and a deleted account: the answer says nothing about who holds an id.
  passkey_not_accepted: 'This passkey did not sign in. Try again, or sign in with your identity file.',
  passkey_cloned: 'This passkey may have been copied, so it was refused. Sign in with your identity file and remove it.',
  no_current_key: 'This account has no usable identity key. Contact support.',
  bad_request: 'That passkey request is malformed.',
};

/// A response for a failed call to the account's object.
function objectFailure(r, json, cors) {
  if (r.status === 503 || r.code === 'unavailable' || r.code === 'no_binding') {
    return json({ error: 'Passkeys are temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  }
  const status = r.status && r.status >= 400 && r.status < 500 ? r.status : 500;
  return json({ error: WORDS[r.code] || r.error || 'Passkey request failed', code: r.code || 'failed' }, status, cors);
}

/// The 32 challenge bytes a body names (base64url), as hex, or null.
function challengeHex(value) {
  const bytes = fromBase64url(value, 32);
  return bytes && bytes.length === 32 ? hexOfBytes(bytes) : null;
}

const utf8 = new TextEncoder();

/// A response for a route here, or null when the path is not one of ours.
/// `deps`: verifyAuth, authMethod, json, cors, loginOrigins, createJwt, publicUser.
export async function handlePasskeyRoute(request, url, env, deps) {
  const { json, cors } = deps;
  const path = url.pathname;
  const post = request.method === 'POST';
  let route = null;
  if (request.method === 'GET' && path === '/api/passkeys') route = 'list';
  else if (post && path === '/api/passkeys/add/begin') route = 'add_begin';
  else if (post && path === '/api/passkeys/add') route = 'add';
  else if (post && path === '/api/passkeys/remove/begin') route = 'remove_begin';
  else if (post && path === '/api/passkeys/remove') route = 'remove';
  else if (post && path === '/api/passkeys/login/begin') route = 'login_begin';
  else if (post && path === '/api/passkeys/login') route = 'login';
  if (!route) return null;

  const config = passkeyConfig(env);
  const unavailable = () => json({ error: 'Passkeys are not available on this deployment.', code: 'passkeys_unavailable' }, 503, cors);
  if (!config || !env.ACCOUNT_AUTH || !env.PASSKEY_INDEX) return unavailable();

  if (route === 'login_begin' || route === 'login') {
    if (!env.CHALLENGES) return unavailable();
    return route === 'login_begin' ? loginBegin(env, config, deps) : login(request, env, config, deps);
  }

  const userId = await deps.verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);

  if (route === 'list') {
    const state = await accountState(env, userId);
    if (!state.ok) return state.code === 'no_account' ? json({ error: 'Unauthorized' }, 401, cors) : objectFailure(state, json, cors);
    const listed = await accountCall(env, userId, 'passkey_list');
    if (!listed.ok) return objectFailure(listed, json, cors);
    return json({ passkeys: listed.passkeys }, 200, { ...cors, 'Cache-Control': 'private, no-store' });
  }

  // Adding and removing a passkey are key changes: an identity-file session only.
  if ((await deps.authMethod(request, env)) !== 'key') return identityFileRequired(json, cors);

  // The origin the request reached is part of what the identity key signs, and it
  // must be one this deployment signs in on.
  const origin = new URL(request.url).origin;
  if (!deps.loginOrigins(env).has(origin)) return json({ error: 'This address does not accept key changes', code: 'origin_not_allowed' }, 400, cors);

  if (env.AUTH_RATE_LIMITER) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `passkeys:${userId}` });
    if (!success) return json({ error: 'Too many requests. Try again in a minute.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '60' });
  }

  const state = await accountState(env, userId);
  if (!state.ok) return state.code === 'no_account' ? json({ error: 'Account not found' }, 404, cors) : objectFailure(state, json, cors);
  const userRaw = await env.USERS.get(`user:${userId}`);
  if (!userRaw) return json({ error: 'Account not found' }, 404, cors);
  const user = JSON.parse(userRaw);
  const parsed = await request.json().catch(() => null);
  const body = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};

  if (route === 'add_begin') {
    const begun = await accountCall(env, userId, 'begin_passkey', { purpose: 'passkey_add' });
    if (!begun.ok) return objectFailure(begun, json, cors);
    return json({
      challenge: toBase64url(bytesOfHex(begun.challenge)),
      rp: { id: config.rpId, name: 'Eustress' },
      user: { id: toBase64url(utf8.encode(userId)), name: user.username, display_name: user.username },
      pub_key_cred_params: PASSKEY_ALGS,
      exclude_credentials: begun.passkeys,
      timeout_ms: PASSKEY_TIMEOUT_MS,
      authenticator_selection: { resident_key: 'required', user_verification: 'required' },
      attestation: 'none',
      api_origin: origin,
    }, 200, cors);
  }

  if (route === 'remove_begin') {
    if (typeof body.id !== 'string' || !body.id) return json({ error: WORDS.bad_request, code: 'bad_request' }, 400, cors);
    const begun = await accountCall(env, userId, 'begin_passkey', { purpose: 'passkey_remove', id: body.id });
    if (!begun.ok) return objectFailure(begun, json, cors);
    return json({ challenge: toBase64url(bytesOfHex(begun.challenge)), api_origin: origin, account_id: userId }, 200, cors);
  }

  const challenge = challengeHex(body.challenge);
  if (!challenge) return json({ error: WORDS.challenge_invalid, code: 'challenge_invalid' }, 400, cors);

  if (route === 'add') {
    const credential = body.credential;
    const credentialId = credential && typeof credential === 'object' && typeof credential.id === 'string' ? credential.id : '';
    // Everything the WebAuthn answer has wrong is found here, and reported by the
    // object only after it has spent the challenge and checked the identity key's
    // signature: the order the contract gives.
    // Whether another account holds the id is the object's question, asked of the index.
    const verified = await verifyRegistration({ credential, challenge: body.challenge, rpId: config.rpId, origins: config.origins });
    const refusal = verified.ok ? null : { status: verified.status, code: verified.code, error: verified.error };
    const name = cleanText(body.name, NAME_MAX_BYTES) || 'Passkey';
    const added = await accountCall(env, userId, 'passkey_add', {
      challenge, signature: body.signature, origin, credential_id: credentialId,
      record: verified.ok ? verified.record : null, refusal, name, username: user.username,
    });
    if (!added.ok) return objectFailure(added, json, cors);
    await tell(env, userId, user, 'passkey_added', added, { at: added.passkey.created_at, passkeyName: name, assetOrigin: origin });
    return json({ ok: true, passkey: added.passkey }, 200, cors);
  }

  // route === 'remove'
  const removed = await accountCall(env, userId, 'passkey_remove', {
    challenge, signature: body.signature, origin, credential_id: typeof body.id === 'string' ? body.id : '', username: user.username,
  });
  if (!removed.ok) return objectFailure(removed, json, cors);
  await tell(env, userId, user, 'passkey_removed', removed, { at: new Date().toISOString(), passkeyName: removed.name, assetOrigin: origin });
  return json({ ok: true, removed: removed.removed }, 200, cors);
}

/// Every address on the account is told; the object holds the notice and sends
/// what this did not.
async function tell(env, userId, user, kind, result, data) {
  const rendered = renderKeyNotice(kind, { username: user.username, ...data, timeZone: await accountTimeZone(env, userId) });
  const recipients = [...new Set([...(result.recipients || []), ...noticeRecipients(user)])];
  const notices = await sendNotice(env, recipients, rendered, { accountId: userId });
  await accountCall(env, userId, 'ack_notice', { id: result.notice_id, delivered: recipients.filter((_, i) => notices[i]?.sent) });
}

async function loginBegin(env, config, deps) {
  const hex = hexOfBytes(crypto.getRandomValues(new Uint8Array(32)));
  const expires_at = new Date(Date.now() + CHALLENGE_TTL_S * 1000).toISOString();
  await env.CHALLENGES.put(await loginChallengeKey(hex), JSON.stringify({ purpose: 'passkey_login', expires_at }), { expirationTtl: CHALLENGE_TTL_S });
  return deps.json({
    challenge: toBase64url(bytesOfHex(hex)),
    rp_id: config.rpId,
    timeout_ms: PASSKEY_TIMEOUT_MS,
    user_verification: 'required',
  }, 200, deps.cors);
}

async function login(request, env, config, deps) {
  const { json, cors } = deps;
  const parsed = await request.json().catch(() => null);
  const body = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
  const invalid = () => json({ error: WORDS.challenge_invalid, code: 'challenge_invalid' }, 400, cors);

  // The challenge first: the first attempt, right or wrong, uses it up.
  const hex = challengeHex(body.challenge);
  if (!hex) return invalid();
  const filed = await loginChallengeKey(hex);
  const stored = await env.CHALLENGES.get(filed);
  if (!stored) return invalid();
  await env.CHALLENGES.delete(filed);
  let held;
  try { held = JSON.parse(stored); } catch { return invalid(); }
  if (held?.purpose !== 'passkey_login' || !(Date.parse(held.expires_at) > Date.now())) return invalid();

  const checked = await checkAssertion({ credential: body.credential, challenge: body.challenge, rpId: config.rpId, origins: config.origins });
  if (!checked.ok) return json({ error: checked.error, code: checked.code }, checked.status, cors);

  // The index names the account; the user handle the authenticator signed in with
  // must name the same one. The object then decides.
  const notAccepted = () => json({ error: WORDS.passkey_not_accepted, code: 'passkey_not_accepted' }, 401, cors);
  const owner = await passkeyIndexCall(env, checked.id, 'owner');
  if (!owner.ok) return json({ error: 'Sign-in is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  const accountId = owner.account_id;
  if (!accountId) return notAccepted();
  if (checked.user !== accountId) return notAccepted();

  const result = await accountCall(env, accountId, 'passkey_login', {
    credential_id: checked.id,
    authenticator_data: checked.authenticator_data,
    client_data_hash: checked.client_data_hash,
    signature: checked.signature,
    challenge_digest: await sha256Hex(hex),
  });
  if (!result.ok) {
    if (['not_seeded', 'passkey_unknown', 'signature_invalid'].includes(result.code)) return notAccepted();
    if (result.status === 503 || result.code === 'unavailable' || result.code === 'no_binding') {
      return json({ error: 'Sign-in is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
    }
    if (result.code === 'rate_limited') return json({ error: 'Too many passkey sign-ins. Try again in a few minutes.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '300' });
    return objectFailure(result, json, cors);
  }

  // The checks the identity-file sign-in makes after its key check. A deleted
  // account signs nobody in, and says no more than an unknown passkey does.
  const userData = await env.USERS.get(`user:${accountId}`);
  if (!userData) return notAccepted();
  const user = JSON.parse(userData);
  if (user.banned) {
    return json({ error: 'Account suspended', reason: user.ban_reason || 'Contact support@eustress.dev' }, 403, cors);
  }

  // Not stamped before the account's session floor, as the identity-file sign-in.
  const floor = Math.max(Number(await env.USERS.get(`authfloor:${accountId}`)) || 0, Number(result.floor) || 0);
  const token = await deps.createJwt(accountId, env.JWT_SECRET, floor, 'passkey');
  return json({ token, user: await deps.publicUser(user, env), amr: 'passkey' }, 200, cors);
}
