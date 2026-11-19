// =============================================================================
// Slack link: an account names the Slack account it owns
// =============================================================================
//
// Linking is "Sign in with Slack" (OpenID Connect), scopes `openid profile`. It
// reads who the person is on Slack and in which workspace, and nothing else: no
// email, no messages, no channels. The token Slack hands back is used once, for
// `openid.connect.userInfo`, and revoked at Slack right after (best effort).
// Nothing but the link is stored: no access token, no id_token, no refresh token.
// The link never signs anyone in: it is a fact the account states about itself,
// shown in Settings and listed among the credentials a key replacement may revoke
// (src/identity_routes.mjs).
//
// The id_token is not read. The identity comes from `openid.connect.userInfo`,
// called with the access token this Worker just received from Slack's token
// endpoint over TLS, in exchange for the code and the client secret. So no nonce
// is sent: a nonce guards an id_token, and one never checked guards nothing. The
// state, single-use and bound to the account that began, is what ties the code
// to this person.
//
//   POST /api/auth/slack/begin    signed in. Answers { authorize_url } for the
//                                 site to send the browser to. The redirect goes
//                                 back to the site that asked:
//                                 `{Origin}/auth/slack/callback`, for an Origin
//                                 this deployment admits (src/cors.mjs). Slack
//                                 takes HTTPS redirects only, so an http:// site
//                                 (localhost) is answered 400 slack_needs_https
//                                 instead of a link Slack would refuse.
//   POST /api/auth/slack/finish   signed in, { code, state }. The site's callback
//                                 page posts what Slack handed it. The state must
//                                 be one this same account began: a link started
//                                 by someone else, then finished in this browser,
//                                 is refused.
//   POST /api/auth/slack/unlink   signed in. Removes the link.
//
// A Slack user id is unique only within its workspace, so a Slack identity is
// the pair {team_id}:{user_id}: the same user id in two workspaces is two
// identities.
//
// Storage, USERS KV:
//
//   slacklink:{userId}           { team_id, user_id, name, team_name, linked_at }
//   slack:{teamId}:{slackUserId} userId: one Slack identity links one Eustress account
//
// and CHALLENGES KV, for the ten minutes a link is in flight:
//
//   slackstate:{sha256(state)}   { user_id, redirect_uri }
//
// The user record is not rewritten (see src/discord_link.mjs for why). A link
// made before the account object's `slack_unlinked_at` is not shown: a key
// replacement revoked it. A link made after is the owner's new choice.
//
// Secrets: SLACK_CLIENT_ID and SLACK_CLIENT_SECRET (wrangler secret put).
// Without both, every route here answers 503 slack_unavailable. The Slack app
// must list each site's `{origin}/auth/slack/callback` as a redirect URL.
// =============================================================================

import { allowedOrigins } from './cors.mjs';

export const SLACK_AUTHORIZE = 'https://slack.com/openid/connect/authorize';
export const SLACK_TOKEN = 'https://slack.com/api/openid.connect.token';
export const SLACK_USERINFO = 'https://slack.com/api/openid.connect.userInfo';
export const SLACK_REVOKE = 'https://slack.com/api/auth.revoke';
/// What a link asks for: who the person is, and nothing more.
export const SLACK_SCOPE = 'openid profile';

/// How long a begun link waits for its finish.
export const SLACK_STATE_TTL_S = 600;

const STATE_RE = /^[0-9a-f]{64}$/;
/// Slack's authorization codes are dotted runs of digits and hex.
const CODE_RE = /^[A-Za-z0-9._~-]{8,512}$/;
/// A Slack team id (T...) or user id (U... / W...): uppercase letters and digits.
export const SLACK_ID_RE = /^[A-Z0-9]{6,20}$/;

const linkKey = (userId) => `slacklink:${userId}`;
const ownerKey = (teamId, slackUserId) => `slack:${teamId}:${slackUserId}`;

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
const stateKey = async (state) => `slackstate:${await sha256Hex(state)}`;

function randomState() {
  return [...crypto.getRandomValues(new Uint8Array(32))].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export function slackConfigured(env) {
  return !!(env?.SLACK_CLIENT_ID && env?.SLACK_CLIENT_SECRET);
}

/// The page on the site Slack sends the browser back to.
export function slackRedirectUriFor(origin) {
  return `${origin}/auth/slack/callback`;
}

/// The account's own link record, or null.
async function readLink(env, userId) {
  const raw = await env.USERS.get(linkKey(userId));
  if (!raw) return null;
  try {
    const link = JSON.parse(raw);
    return link && SLACK_ID_RE.test(String(link.team_id)) && SLACK_ID_RE.test(String(link.user_id)) ? link : null;
  } catch {
    return null;
  }
}

/// The Slack identity this account shows as linked:
/// `{ team_id, user_id, name, team_name, linked_at }`, or null. `unlinkedAt` is
/// the account object's `slack_unlinked_at`: a link older than it was revoked by
/// a key replacement.
export async function currentSlackLink(env, user, unlinkedAt = null) {
  if (!user?.id || !env?.USERS) return null;
  const link = await readLink(env, user.id);
  if (!link) return null;
  if (unlinkedAt && !(String(link.linked_at || '') > String(unlinkedAt))) return null;
  return {
    team_id: String(link.team_id), user_id: String(link.user_id),
    name: link.name || null, team_name: link.team_name || null, linked_at: link.linked_at || null,
  };
}

/// Account deletion and unlinking: the link and its claim on the Slack identity go with it.
export async function deleteSlackLink(env, userId) {
  if (!env?.USERS || !userId) return;
  const link = await readLink(env, userId);
  if (link) {
    const key = ownerKey(link.team_id, link.user_id);
    if ((await env.USERS.get(key)) === userId) await env.USERS.delete(key);
  }
  await env.USERS.delete(linkKey(userId));
}

/// Revoke a token at Slack. Best effort: never throws. The token is never logged.
async function revokeAtSlack(token, fetchImpl) {
  if (!token) return false;
  try {
    const res = await fetchImpl(SLACK_REVOKE, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/x-www-form-urlencoded' },
      body: '',
    });
    const reply = await res.json().catch(() => ({}));
    return !!(res.ok && reply.ok);
  } catch {
    return false;
  }
}

/// Slack's token errors that mean the code itself is no good (used, expired, made
/// for another redirect): the person starts again. Anything else is Slack or this
/// server's setup failing.
const CODE_ERRORS = new Set(['invalid_code', 'code_already_used', 'code_expired', 'invalid_grant', 'bad_redirect_uri']);

function failure(message, code) {
  const err = new Error(message);
  err.code = code;
  return err;
}

/// Who Slack's token endpoint and `openid.connect.userInfo` say this code signs
/// in: `{ teamId, userId, name, teamName }`. The access token is revoked at Slack
/// as soon as userInfo has answered (or failed): nothing is kept. Throws an Error
/// with `.code` set for the reply on any failure. Slack answers HTTP 200 with
/// `ok: false` for most errors, so `ok` is what is checked.
async function identityFromCode(env, code, redirectUri, fetchImpl) {
  let tokenReply;
  try {
    const res = await fetchImpl(SLACK_TOKEN, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
      body: new URLSearchParams({
        client_id: env.SLACK_CLIENT_ID, client_secret: env.SLACK_CLIENT_SECRET,
        code, redirect_uri: redirectUri, grant_type: 'authorization_code',
      }).toString(),
    });
    tokenReply = await res.json().catch(() => ({}));
    if (!res.ok || tokenReply?.ok !== true || typeof tokenReply.access_token !== 'string' || !tokenReply.access_token) {
      // A token Slack issued alongside a refusal is still a token: revoke it.
      if (typeof tokenReply?.access_token === 'string') await revokeAtSlack(tokenReply.access_token, fetchImpl);
      const error = typeof tokenReply?.error === 'string' ? tokenReply.error : '';
      throw failure(`slack token ${res.status} ${error}`.trim(), CODE_ERRORS.has(error) ? 'slack_code_rejected' : 'slack_failed');
    }
  } catch (e) {
    if (e.code) throw e;
    throw failure(`slack token unreachable: ${e?.message || e}`, 'slack_failed');
  }

  const accessToken = tokenReply.access_token;
  let info = null;
  let infoError = null;
  try {
    const res = await fetchImpl(SLACK_USERINFO, { headers: { Authorization: `Bearer ${accessToken}`, Accept: 'application/json' } });
    const body = await res.json().catch(() => ({}));
    if (!res.ok || body?.ok !== true) infoError = failure(`slack userInfo ${res.status} ${typeof body?.error === 'string' ? body.error : ''}`.trim(), 'slack_failed');
    else info = body;
  } catch (e) {
    infoError = failure(`slack userInfo unreachable: ${e?.message || e}`, 'slack_failed');
  }
  // Nothing will use this token again, whatever userInfo said.
  await revokeAtSlack(accessToken, fetchImpl);
  if (infoError) throw infoError;

  const teamId = String(info['https://slack.com/team_id'] || '');
  const userId = String(info['https://slack.com/user_id'] || '');
  const sub = info.sub === undefined ? userId : String(info.sub);
  if (!SLACK_ID_RE.test(teamId) || !SLACK_ID_RE.test(userId) || sub !== userId) {
    throw failure('slack userInfo carried no usable team and user id', 'slack_failed');
  }
  const text = (v, max) => (typeof v === 'string' && v.trim() ? v.trim().slice(0, max) : null);
  return { teamId, userId, name: text(info.name, 80), teamName: text(info['https://slack.com/team_name'], 80) };
}

const FAILURES = {
  slack_code_rejected: [400, 'Slack did not accept that sign-in. Start again from Settings.'],
  slack_failed: [502, 'Slack could not be reached. Try again in a minute.'],
};

/// Handles `/api/auth/slack/*`, or answers null for any other path.
/// `deps`: { verifyAuth, authMethod, json, cors, publicUser, fetch? }.
export async function handleSlackRoute(request, url, env, deps) {
  const { verifyAuth, json, cors } = deps;
  const path = url.pathname;
  if (path !== '/api/auth/slack/begin' && path !== '/api/auth/slack/finish' && path !== '/api/auth/slack/unlink') return null;
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  const userRaw = await env.USERS.get(`user:${userId}`);
  if (!userRaw) return json({ error: 'Account not found' }, 404, cors);
  const user = JSON.parse(userRaw);
  const noStore = { ...cors, 'Cache-Control': 'no-store' };
  const fetchImpl = deps.fetch || fetch;

  // The account as /api/auth/me answers it, how this session signed in included,
  // so the site can store it in place of the one it holds.
  const reply = async () => {
    const shown = await deps.publicUser(user, env);
    const method = deps.authMethod ? await deps.authMethod(request, env) : null;
    return json({ ok: true, user: { ...shown, ...(method ? { auth_method: method } : {}) } }, 200, noStore);
  };

  if (path === '/api/auth/slack/unlink') {
    await deleteSlackLink(env, userId);
    return reply();
  }

  if (!slackConfigured(env) || !env.CHALLENGES) {
    return json({ error: 'Slack linking is not set up on this server yet.', code: 'slack_unavailable' }, 503, noStore);
  }

  if (path === '/api/auth/slack/begin') {
    const origin = request.headers.get('Origin');
    if (!origin || !allowedOrigins(env).has(origin)) {
      return json({ error: 'Start the link from the Eustress website.', code: 'bad_origin' }, 400, noStore);
    }
    if (!origin.startsWith('https://')) {
      return json({ error: 'Slack only returns to HTTPS sites. Link Slack from eustress.dev.', code: 'slack_needs_https' }, 400, noStore);
    }
    const state = randomState();
    const redirectUri = slackRedirectUriFor(origin);
    await env.CHALLENGES.put(await stateKey(state), JSON.stringify({ user_id: userId, redirect_uri: redirectUri }), { expirationTtl: SLACK_STATE_TTL_S });
    const authorize = new URL(SLACK_AUTHORIZE);
    authorize.searchParams.set('response_type', 'code');
    authorize.searchParams.set('scope', SLACK_SCOPE);
    authorize.searchParams.set('client_id', env.SLACK_CLIENT_ID);
    authorize.searchParams.set('state', state);
    authorize.searchParams.set('redirect_uri', redirectUri);
    return json({ authorize_url: authorize.toString(), expires_in: SLACK_STATE_TTL_S }, 200, noStore);
  }

  // finish
  const body = await request.json().catch(() => ({}));
  const code = typeof body.code === 'string' ? body.code : '';
  const state = typeof body.state === 'string' ? body.state : '';
  if (!STATE_RE.test(state) || !CODE_RE.test(code)) {
    return json({ error: 'That link is not complete. Start again from Settings.', code: 'bad_request' }, 400, noStore);
  }
  const key = await stateKey(state);
  const heldRaw = await env.CHALLENGES.get(key);
  if (!heldRaw) {
    return json({ error: 'That link expired. Start again from Settings.', code: 'state_expired' }, 400, noStore);
  }
  const held = JSON.parse(heldRaw);
  if (held.user_id !== userId) {
    // Not deleted: it is the other account's to finish or let lapse.
    return json({ error: 'That link was started by a different account. Start again from Settings.', code: 'state_mismatch' }, 403, noStore);
  }
  await env.CHALLENGES.delete(key);

  let slack;
  try {
    slack = await identityFromCode(env, code, held.redirect_uri, fetchImpl);
  } catch (e) {
    console.error('slack link failed:', e?.message || e);
    const [status, message] = FAILURES[e.code] || FAILURES.slack_failed;
    return json({ error: message, code: FAILURES[e.code] ? e.code : 'slack_failed' }, status, noStore);
  }

  const claim = ownerKey(slack.teamId, slack.userId);
  const owner = await env.USERS.get(claim);
  if (owner && owner !== userId) {
    // The other account may have deleted its link record without its claim
    // (a failed write): the claim stands only while that account still shows
    // this Slack identity.
    const ownerLink = await readLink(env, owner);
    if (ownerLink && ownerLink.team_id === slack.teamId && ownerLink.user_id === slack.userId) {
      return json({ error: 'That Slack account is already linked to another Eustress account.', code: 'slack_taken' }, 409, noStore);
    }
  }

  // Replacing an earlier link to another Slack identity releases its claim.
  const previous = await readLink(env, userId);
  if (previous && (previous.team_id !== slack.teamId || previous.user_id !== slack.userId)) {
    const prevKey = ownerKey(previous.team_id, previous.user_id);
    if ((await env.USERS.get(prevKey)) === userId) await env.USERS.delete(prevKey);
  }
  await env.USERS.put(claim, userId);
  await env.USERS.put(linkKey(userId), JSON.stringify({
    team_id: slack.teamId, user_id: slack.userId, name: slack.name, team_name: slack.teamName, linked_at: new Date().toISOString(),
  }));
  return reply();
}
