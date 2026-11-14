// =============================================================================
// Discord link: an account names the Discord account it owns
// =============================================================================
//
// Linking is Discord's OAuth2 authorization-code flow, scopes `identify` and
// `role_connections.write`. It reads who the person is on Discord, and writes
// the person's Linked Roles connection (ROLE_METADATA: verified, creator, spaces,
// member_since) so a server can grant a role by it. The refresh token is kept,
// sealed (AES-GCM under a key derived from DISCORD_CLIENT_SECRET), only so the
// connection can be updated when the account's listed Spaces change
// (src/discord_interactions.mjs sweep). Unlinking, linking another Discord
// account, a key replacement that revokes the link, and deleting the account
// each revoke it at Discord (best effort) and delete it. The link never signs
// anyone in: it is a fact the account states about itself, shown in Settings
// and listed among the credentials a key replacement may revoke
// (src/identity_routes.mjs).
//
//   POST /api/auth/discord/begin    signed in. Answers { authorize_url } for the
//                                   site to send the browser to. The redirect
//                                   goes back to the site that asked:
//                                   `{Origin}/auth/discord/callback`, for an
//                                   Origin this deployment admits (src/cors.mjs).
//   POST /api/auth/discord/finish   signed in, { code, state }. The site's
//                                   callback page posts what Discord handed it.
//                                   The state must be one this same account
//                                   began: a link started by someone else, then
//                                   finished in this browser, is refused, so no
//                                   one can attach their Discord to an account
//                                   that is not theirs (or attach this person's
//                                   Discord to an account that is).
//   POST /api/auth/discord/unlink   signed in. Removes the link.
//
// Storage, USERS KV:
//
//   discordlink:{userId}     { id, username, linked_at }: the account's link
//   discord:{discordId}      userId: one Discord account links one Eustress account
//   discordtoken:{userId}    { v, iv, ct, spaces, meta_hash, at }: the sealed
//                            refresh token (the account id is its associated
//                            data, so a record moved to another key does not open)
//
// and CHALLENGES KV, for the ten minutes a link is in flight:
//
//   discordstate:{sha256(state)}   { user_id, redirect_uri }
//
// The user record is not rewritten. Seven routes already read-modify-write it,
// and another could put back a ban or a key that landed meanwhile
// (docs/architecture/IDENTITY_KEY_ROTATION_AND_RECOVERY.md). An older record may
// still carry `discord_id`; it is read as a link with no time, and unlinking
// removes it from what the account shows by the account object's
// `discord_unlinked_at`, the same fact a key replacement writes.
//
// A link made before the account object's `discord_unlinked_at` is not shown:
// replacing the key revoked it. A link made after is the owner's new choice.
//
// Secrets: DISCORD_CLIENT_ID and DISCORD_CLIENT_SECRET (wrangler secret put).
// Without both, every route here answers 503 discord_unavailable. The Discord
// application must list each site's `{origin}/auth/discord/callback` as a
// redirect.
// =============================================================================

import { allowedOrigins } from './cors.mjs';
import { accountState } from './account_auth.mjs';

export const DISCORD_AUTHORIZE = 'https://discord.com/oauth2/authorize';
export const DISCORD_TOKEN = 'https://discord.com/api/oauth2/token';
export const DISCORD_REVOKE = 'https://discord.com/api/oauth2/token/revoke';
export const DISCORD_ME = 'https://discord.com/api/users/@me';
/// Discord's versioned API, for the Linked Roles connection and the app's own calls.
export const DISCORD_API = 'https://discord.com/api/v10';
/// What a link asks for: who the person is, and leave to write their Linked Roles connection.
export const DISCORD_SCOPE = 'identify role_connections.write';
/// The person's Linked Roles connection for this application.
export const roleConnectionUrl = (applicationId) => `${DISCORD_API}/users/@me/applications/${applicationId}/role-connection`;

/// The Linked Roles records a server can require (at most 5; keys lowercase). Types: 2 integer_greater_than_or_equal,
/// 6 datetime_greater_than_or_equal, 7 boolean_equal. Registered by the five-minute tick (src/discord_interactions.mjs).
export const ROLE_METADATA = Object.freeze([
  Object.freeze({ key: 'verified', name: 'Verified', description: 'Has a linked Eustress account with verified identity in good standing', type: 7 }),
  Object.freeze({ key: 'creator', name: 'Creator', description: 'Has at least one listed Space on Eustress', type: 7 }),
  Object.freeze({ key: 'spaces', name: 'Listed Spaces', description: 'Number of listed Spaces on Eustress', type: 2 }),
  Object.freeze({ key: 'member_since', name: 'Member since', description: 'Days since joining Eustress', type: 6 }),
]);

/// How long a begun link waits for its finish.
export const DISCORD_STATE_TTL_S = 600;

const STATE_RE = /^[0-9a-f]{64}$/;
/// Discord's authorization codes are short base62-like strings.
const CODE_RE = /^[A-Za-z0-9._~-]{8,256}$/;
/// A Discord id is a snowflake: a decimal integer.
const SNOWFLAKE_RE = /^[0-9]{5,25}$/;

const linkKey = (userId) => `discordlink:${userId}`;
const ownerKey = (discordId) => `discord:${discordId}`;

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
const stateKey = async (state) => `discordstate:${await sha256Hex(state)}`;

function randomState() {
  return [...crypto.getRandomValues(new Uint8Array(32))].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export function discordConfigured(env) {
  return !!(env?.DISCORD_CLIENT_ID && env?.DISCORD_CLIENT_SECRET);
}

/// The page on the site Discord sends the browser back to.
export function redirectUriFor(origin) {
  return `${origin}/auth/discord/callback`;
}

/// The account's own link record, or null.
async function readLink(env, userId) {
  const raw = await env.USERS.get(linkKey(userId));
  if (!raw) return null;
  try {
    const link = JSON.parse(raw);
    return link && SNOWFLAKE_RE.test(String(link.id)) ? link : null;
  } catch {
    return null;
  }
}

/// The Discord account this account shows as linked: `{ id, username, linked_at }`,
/// or null. `unlinkedAt` is the account object's `discord_unlinked_at`: a link
/// older than it was revoked by a key replacement or by the owner. An older
/// record's `discord_id` carries no time, so any unlink hides it.
export async function currentDiscordLink(env, user, unlinkedAt = null) {
  if (!user?.id || !env?.USERS) return null;
  const link = await readLink(env, user.id);
  if (link) {
    if (unlinkedAt && !(String(link.linked_at || '') > String(unlinkedAt))) return null;
    return { id: String(link.id), username: link.username || null, linked_at: link.linked_at || null };
  }
  if (user.discord_id && !unlinkedAt) return { id: String(user.discord_id), username: null, linked_at: null };
  return null;
}

/// Account deletion and unlinking: the link, its claim on the Discord id, and the
/// kept Discord token (revoked at Discord first, best effort) go with it.
export async function deleteDiscordLink(env, userId, fetchImpl = fetch) {
  await revokeStoredDiscordToken(env, userId, fetchImpl);
  const link = await readLink(env, userId);
  if (link) {
    const owner = await env.USERS.get(ownerKey(link.id));
    if (owner === userId) await env.USERS.delete(ownerKey(link.id));
  }
  await env.USERS.delete(linkKey(userId));
}

/// The Eustress account a Discord account is linked to RIGHT NOW: `{ id, user }`, or null. The claim
/// `discord:{discordId}` must name an account whose current link (currentDiscordLink, honouring the account object's
/// `discord_unlinked_at`) is this same Discord account. A banned account, or an account object that cannot be read,
/// is no link: every caller fails closed.
export async function linkedAccountFor(env, discordId) {
  if (!env?.USERS || !SNOWFLAKE_RE.test(String(discordId || ''))) return null;
  try {
    const owner = await env.USERS.get(ownerKey(String(discordId)));
    if (!owner) return null;
    const raw = await env.USERS.get(`user:${owner}`);
    if (!raw) return null;
    const user = JSON.parse(raw);
    if (!user || user.banned) return null;
    let unlinkedAt = null;
    if (env.ACCOUNT_AUTH) {
      const held = await accountState(env, owner);
      if (!held?.ok) return null;
      unlinkedAt = held.discord_unlinked_at || null;
    }
    const link = await currentDiscordLink(env, { ...user, id: owner }, unlinkedAt);
    return link && link.id === String(discordId) ? { id: owner, user: { ...user, id: owner } } : null;
  } catch {
    return null;
  }
}

// -----------------------------------------------------------------------------
// The kept token: sealed, refreshed when the Linked Roles connection changes
// -----------------------------------------------------------------------------

const tokenKey = (userId) => `discordtoken:${userId}`;
const TOKEN_KEY_INFO = 'eustress-discord-token-v1';
const enc = new TextEncoder();
const b64 = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

/// AES-GCM key: HKDF-SHA256 over DISCORD_CLIENT_SECRET, info `eustress-discord-token-v1`. Null without the secret.
async function sealKey(env) {
  const secret = env?.DISCORD_CLIENT_SECRET;
  if (typeof secret !== 'string' || !secret) return null;
  const base = await crypto.subtle.importKey('raw', enc.encode(secret), 'HKDF', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: enc.encode(TOKEN_KEY_INFO) },
    base, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt'],
  );
}

/// `{ v: 1, iv, ct }` for `token`, bound to the account by its key as associated data. A fresh random 12-byte IV each time.
export async function sealDiscordToken(env, userId, token) {
  const key = await sealKey(env);
  if (!key) throw new Error('DISCORD_CLIENT_SECRET is not set');
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: enc.encode(tokenKey(userId)) }, key, enc.encode(token));
  return { v: 1, iv: b64(iv), ct: b64(ct) };
}

/// The token a sealed record holds, or null when it does not open (another key, another account, tampered).
export async function openDiscordToken(env, userId, record) {
  try {
    const key = await sealKey(env);
    if (!key || record?.v !== 1) return null;
    const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: unb64(record.iv), additionalData: enc.encode(tokenKey(userId)) }, key, unb64(record.ct));
    return new TextDecoder().decode(plain);
  } catch {
    return null;
  }
}

async function readTokenRecord(env, userId) {
  try {
    const raw = await env.USERS.get(tokenKey(userId));
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

/// Keep `refreshToken` for the account, sealed, with what was last pushed (`spaces`, `meta_hash`).
async function storeToken(env, userId, refreshToken, pushed = {}) {
  const sealed = await sealDiscordToken(env, userId, refreshToken);
  await env.USERS.put(tokenKey(userId), JSON.stringify({ ...sealed, spaces: pushed.spaces ?? null, meta_hash: pushed.meta_hash ?? null, at: new Date().toISOString() }));
}

/// Revoke one token at Discord. Best effort: never throws. The token is never logged.
async function revokeAtDiscord(env, token, hint, fetchImpl) {
  if (!token || !discordConfigured(env)) return false;
  try {
    const basic = btoa(`${env.DISCORD_CLIENT_ID}:${env.DISCORD_CLIENT_SECRET}`);
    const res = await fetchImpl(DISCORD_REVOKE, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Authorization: `Basic ${basic}` },
      body: new URLSearchParams({ token, token_type_hint: hint }).toString(),
    });
    return !!res?.ok;
  } catch {
    return false;
  }
}

/// The account's kept token: the Linked Roles connection cleared first (so a server's roles stop matching at once),
/// then the token revoked at Discord and deleted. Every step is best effort. Nothing to do without a kept token.
export async function revokeStoredDiscordToken(env, userId, fetchImpl = fetch) {
  if (!env?.USERS || !userId) return;
  if (!(await readTokenRecord(env, userId))) return;
  const fresh = await refreshedAccess(env, userId, fetchImpl);
  if (fresh) await putRoleConnection(env, fresh.accessToken, { username: '' }, {}, fetchImpl);
  // The refresh rotated the kept token; read it again so the newest grant is the one revoked.
  const record = await readTokenRecord(env, userId);
  if (!record) return;
  const token = await openDiscordToken(env, userId, record);
  if (token) await revokeAtDiscord(env, token, 'refresh_token', fetchImpl);
  await env.USERS.delete(tokenKey(userId));
}

/// A fresh access token from the kept refresh token, keeping the new refresh token Discord hands back. Discord refusing
/// the refresh (4xx: revoked, expired, deauthorized) deletes the kept token; a failure to reach Discord keeps it for the
/// next try. `{ accessToken, record }` or null.
async function refreshedAccess(env, userId, fetchImpl) {
  if (!discordConfigured(env)) return null;
  const record = await readTokenRecord(env, userId);
  if (!record) return null;
  const refreshToken = await openDiscordToken(env, userId, record);
  if (!refreshToken) { await env.USERS.delete(tokenKey(userId)); return null; }
  let res;
  let reply;
  try {
    const basic = btoa(`${env.DISCORD_CLIENT_ID}:${env.DISCORD_CLIENT_SECRET}`);
    res = await fetchImpl(DISCORD_TOKEN, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Authorization: `Basic ${basic}`, Accept: 'application/json' },
      body: new URLSearchParams({ grant_type: 'refresh_token', refresh_token: refreshToken }).toString(),
    });
    reply = await res.json().catch(() => ({}));
  } catch {
    return null;
  }
  if (!res.ok || !reply.access_token) {
    if (res.status >= 400 && res.status < 500) await env.USERS.delete(tokenKey(userId));
    console.error(`discord token refresh for ${userId} answered ${res.status}`);
    return null;
  }
  if (reply.refresh_token) await storeToken(env, userId, reply.refresh_token, record);
  return { accessToken: reply.access_token, record };
}

/// The Linked Roles values for an account: `verified` (identity verified and not banned), `creator` and `spaces` (listed
/// Spaces), `member_since` (the account's creation, ISO 8601). Booleans are 1 or 0, as Discord takes them.
export function roleMetadataFor(user, listedSpaces) {
  const spaces = Number.isSafeInteger(listedSpaces) && listedSpaces > 0 ? listedSpaces : 0;
  const created = Date.parse(user?.created_at || '');
  return {
    verified: user?.kyc_verified === true && !user?.banned ? 1 : 0,
    creator: spaces > 0 ? 1 : 0,
    spaces,
    ...(Number.isFinite(created) ? { member_since: new Date(created).toISOString() } : {}),
  };
}

async function metaHash(metadata, username) {
  return sha256Hex(JSON.stringify({ metadata, username }));
}

/// PUT the person's Linked Roles connection with an access token. True on 2xx. Never throws, never logs the token.
async function putRoleConnection(env, accessToken, user, metadata, fetchImpl) {
  try {
    const res = await fetchImpl(roleConnectionUrl(env.DISCORD_CLIENT_ID), {
      method: 'PUT',
      headers: { Authorization: `Bearer ${accessToken}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ platform_name: 'Eustress', platform_username: String(user?.username || '').slice(0, 100), metadata }),
    });
    if (!res.ok) console.error(`discord role connection answered ${res.status}`);
    return !!res.ok;
  } catch (e) {
    console.error('discord role connection unreachable:', e?.message || e);
    return false;
  }
}

/// Push the account's Linked Roles connection again from its kept token (the five-minute sweep, when its listed Spaces
/// changed). A link that is no longer current (unlinked, or revoked by a key replacement) has its token revoked and
/// deleted instead. Returns 'pushed', 'unchanged', 'revoked', 'no_token' or 'failed'.
export async function repushRoleConnection(env, userId, listedSpaces, fetchImpl = fetch) {
  const record = await readTokenRecord(env, userId);
  if (!record) return 'no_token';
  const raw = await env.USERS.get(`user:${userId}`);
  const user = raw ? JSON.parse(raw) : null;
  let unlinkedAt = null;
  if (user && env.ACCOUNT_AUTH) {
    const held = await accountState(env, userId);
    if (!held?.ok) return 'failed';
    unlinkedAt = held.discord_unlinked_at || null;
  }
  const link = user ? await currentDiscordLink(env, user, unlinkedAt) : null;
  if (!link) { await revokeStoredDiscordToken(env, userId, fetchImpl); return 'revoked'; }
  const metadata = roleMetadataFor(user, listedSpaces);
  const hash = await metaHash(metadata, user.username);
  if (record.meta_hash === hash) return 'unchanged';
  const fresh = await refreshedAccess(env, userId, fetchImpl);
  if (!fresh) return 'failed';
  if (!(await putRoleConnection(env, fresh.accessToken, user, metadata, fetchImpl))) return 'failed';
  const now = await readTokenRecord(env, userId);
  if (now) await env.USERS.put(tokenKey(userId), JSON.stringify({ ...now, spaces: metadata.spaces, meta_hash: hash }));
  return 'pushed';
}

/// What Discord's token endpoint and `users/@me` say about this code:
/// `{ id, username, accessToken, refreshToken }`. Throws an Error with `.code` set
/// for the reply on any failure, after revoking the token it was given (best
/// effort). On success the caller owns the tokens: it pushes the Linked Roles
/// connection, keeps the refresh token sealed, or revokes them.
async function identityFromCode(env, code, redirectUri, fetchImpl) {
  const basic = btoa(`${env.DISCORD_CLIENT_ID}:${env.DISCORD_CLIENT_SECRET}`);
  const form = new URLSearchParams({ grant_type: 'authorization_code', code, redirect_uri: redirectUri });
  let tokenReply;
  try {
    const res = await fetchImpl(DISCORD_TOKEN, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Authorization: `Basic ${basic}`, Accept: 'application/json' },
      body: form.toString(),
    });
    tokenReply = await res.json().catch(() => ({}));
    if (!res.ok || !tokenReply.access_token) {
      // invalid_grant: the code was used, expired, or made for another redirect.
      const err = new Error(`discord token ${res.status} ${tokenReply.error || ''}`.trim());
      err.code = tokenReply.error === 'invalid_grant' ? 'discord_code_rejected' : 'discord_failed';
      throw err;
    }
  } catch (e) {
    if (e.code) throw e;
    const err = new Error(`discord token unreachable: ${e?.message || e}`);
    err.code = 'discord_failed';
    throw err;
  }

  const accessToken = tokenReply.access_token;
  try {
    const res = await fetchImpl(DISCORD_ME, { headers: { Authorization: `Bearer ${accessToken}`, Accept: 'application/json' } });
    const me = await res.json().catch(() => ({}));
    if (!res.ok || !SNOWFLAKE_RE.test(String(me.id || ''))) {
      const err = new Error(`discord users/@me ${res.status}`);
      err.code = 'discord_failed';
      throw err;
    }
    // `global_name` is the display name; `username` the unique handle. A
    // pre-2023 account may still carry a discriminator other than "0".
    const handle = String(me.username || '').slice(0, 64);
    const tag = me.discriminator && me.discriminator !== '0' ? `${handle}#${me.discriminator}` : handle;
    return { id: String(me.id), username: tag || null, accessToken, refreshToken: typeof tokenReply.refresh_token === 'string' ? tokenReply.refresh_token : null };
  } catch (e) {
    // Nothing will use this token: revoke it (best effort; one Discord still holds expires on its own in a week).
    await revokeAtDiscord(env, accessToken, 'access_token', fetchImpl);
    if (e.code) throw e;
    const err = new Error(`discord users/@me unreachable: ${e?.message || e}`);
    err.code = 'discord_failed';
    throw err;
  }
}

const FAILURES = {
  discord_code_rejected: [400, 'Discord did not accept that sign-in. Start again from Settings.'],
  discord_failed: [502, 'Discord could not be reached. Try again in a minute.'],
};

/// Handles `/api/auth/discord/*`, or answers null for any other path.
/// `deps`: { verifyAuth, authMethod, json, cors, publicUser, accountCall, fetch? }.
export async function handleDiscordRoute(request, url, env, deps) {
  const { verifyAuth, json, cors } = deps;
  const path = url.pathname;
  if (path !== '/api/auth/discord/begin' && path !== '/api/auth/discord/finish' && path !== '/api/auth/discord/unlink') return null;
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);

  const userId = await verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  const userRaw = await env.USERS.get(`user:${userId}`);
  if (!userRaw) return json({ error: 'Account not found' }, 404, cors);
  const user = JSON.parse(userRaw);
  const noStore = { ...cors, 'Cache-Control': 'no-store' };

  // The account as /api/auth/me answers it, how this session signed in included,
  // so the site can store it in place of the one it holds.
  const reply = async () => {
    const shown = await deps.publicUser(user, env);
    const method = deps.authMethod ? await deps.authMethod(request, env) : null;
    return json({ ok: true, user: { ...shown, ...(method ? { auth_method: method } : {}) } }, 200, noStore);
  };

  if (path === '/api/auth/discord/unlink') {
    await deleteDiscordLink(env, userId, deps.fetch || fetch);
    // An older record's `discord_id` is hidden by the account's word, the same
    // fact a key replacement writes.
    if (user.discord_id && deps.accountCall && env.ACCOUNT_AUTH) await deps.accountCall(env, userId, 'unlink_discord').catch(() => null);
    return reply();
  }

  if (!discordConfigured(env) || !env.CHALLENGES) {
    return json({ error: 'Discord linking is not set up on this server yet.', code: 'discord_unavailable' }, 503, noStore);
  }

  if (path === '/api/auth/discord/begin') {
    const origin = request.headers.get('Origin');
    if (!origin || !allowedOrigins(env).has(origin)) {
      return json({ error: 'Start the link from the Eustress website.', code: 'bad_origin' }, 400, noStore);
    }
    const state = randomState();
    const redirectUri = redirectUriFor(origin);
    await env.CHALLENGES.put(await stateKey(state), JSON.stringify({ user_id: userId, redirect_uri: redirectUri }), { expirationTtl: DISCORD_STATE_TTL_S });
    const authorize = new URL(DISCORD_AUTHORIZE);
    authorize.searchParams.set('response_type', 'code');
    authorize.searchParams.set('client_id', env.DISCORD_CLIENT_ID);
    authorize.searchParams.set('scope', DISCORD_SCOPE);
    authorize.searchParams.set('state', state);
    authorize.searchParams.set('redirect_uri', redirectUri);
    authorize.searchParams.set('prompt', 'consent');
    return json({ authorize_url: authorize.toString(), expires_in: DISCORD_STATE_TTL_S }, 200, noStore);
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

  let discord;
  try {
    discord = await identityFromCode(env, code, held.redirect_uri, deps.fetch || fetch);
  } catch (e) {
    console.error('discord link failed:', e?.message || e);
    const [status, message] = FAILURES[e.code] || FAILURES.discord_failed;
    return json({ error: message, code: e.code || 'discord_failed' }, status, noStore);
  }

  const owner = await env.USERS.get(ownerKey(discord.id));
  if (owner && owner !== userId) {
    // The other account may have deleted its link record without its claim
    // (a failed write): the claim stands only while that account still shows
    // this Discord account.
    const ownerLink = await readLink(env, owner);
    if (ownerLink && String(ownerLink.id) === discord.id) {
      // Nothing will use the token this code was exchanged for.
      await revokeAtDiscord(env, discord.accessToken, 'access_token', deps.fetch || fetch);
      return json({ error: 'That Discord account is already linked to another Eustress account.', code: 'discord_taken' }, 409, noStore);
    }
  }

  // Replacing an earlier link releases its claim, and revokes the token kept for
  // the other Discord account. Relinking the same Discord account only replaces
  // the kept token: revoking the old grant at Discord could end the new one too.
  const previous = await readLink(env, userId);
  if (previous && String(previous.id) !== discord.id) {
    await revokeStoredDiscordToken(env, userId, deps.fetch || fetch);
    const prevOwner = await env.USERS.get(ownerKey(previous.id));
    if (prevOwner === userId) await env.USERS.delete(ownerKey(previous.id));
  } else if (!previous && (await readTokenRecord(env, userId))) {
    await revokeStoredDiscordToken(env, userId, deps.fetch || fetch);
  }
  await env.USERS.put(ownerKey(discord.id), userId);
  await env.USERS.put(linkKey(userId), JSON.stringify({ id: discord.id, username: discord.username, linked_at: new Date().toISOString() }));

  // Linked Roles: push the connection with the access token just issued, then keep
  // the refresh token sealed for later updates. Either failing leaves the link
  // made; the five-minute sweep pushes again when the listed Spaces change.
  try {
    let listed = 0;
    if (deps.listViews) {
      const views = await deps.listViews('adult_21').catch(() => []);
      listed = Array.isArray(views) ? views.filter((v) => v?.author_id === userId).length : 0;
    }
    const metadata = roleMetadataFor(user, listed);
    const pushed = await putRoleConnection(env, discord.accessToken, user, metadata, deps.fetch || fetch);
    if (discord.refreshToken) {
      await storeToken(env, userId, discord.refreshToken, pushed ? { spaces: metadata.spaces, meta_hash: await metaHash(metadata, user.username) } : {});
    }
  } catch (e) {
    console.error('discord linked roles at link failed:', e?.message || e);
  }
  return reply();
}
