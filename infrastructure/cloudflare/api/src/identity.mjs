// =============================================================================
// Identity tickets: who is at the other end of a multiplayer connection
// =============================================================================
//
// A Player joining a host, and a host welcoming a Player, each prove which
// account they are by handing over a ticket instead of a session token. The
// ticket names the account and the host connection it is for, and expires in
// minutes; the receiver asks this Worker whether it is genuine. Neither side
// ever holds anything it could spend Tickets with.
//
// A ticket's audience is the host's certificate pin: the SHA-256 of the
// certificate the connection is pinned to, which both ends know from the join
// link and which is new every time a host starts. A ticket shown to any other
// host, or replayed by a host that captured it, names the wrong pin and fails.
//
//   POST /api/identity/ticket   session  {audience}
//                                        -> {ticket, account_id, username, expires}
//   POST /api/identity/verify   public   {ticket, audience}
//                                        -> {account_id, username, expires}
//
// A ticket is `eit1.<base64url payload>.<base64url HMAC-SHA256>`, the payload
// {a: account, u: username, h: audience, e: expiry in unix seconds, j: a random
// id}. The id lets a host that admits players through the gallery use each
// ticket once (see live.mjs, POST /live/admit). The HMAC
// key is derived from JWT_SECRET under its own label, so a ticket and a
// session token signed with the same secret can never pass for each other.
// =============================================================================

const PREFIX = 'eit1';
const KEY_LABEL = 'eustress-identity-ticket-v1';

/// How long a ticket lasts: long enough to connect, too short to keep.
export const TICKET_TTL_S = 10 * 60;

const encoder = new TextEncoder();
const nowS = () => Math.floor(Date.now() / 1000);

function b64url(bytes) {
  let s = '';
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

function fromB64url(text) {
  const b64 = text.replace(/-/g, '+').replace(/_/g, '/');
  const bin = atob(b64 + '='.repeat((4 - (b64.length % 4)) % 4));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

async function hmac(keyBytes, message) {
  const key = await crypto.subtle.importKey('raw', keyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await crypto.subtle.sign('HMAC', key, encoder.encode(message)));
}

/// The ticket key: HMAC(JWT_SECRET, label), never the secret itself.
async function ticketKey(env) {
  if (!env.JWT_SECRET) throw new Error('JWT_SECRET is not configured');
  return hmac(encoder.encode(env.JWT_SECRET), KEY_LABEL);
}

function equalBytes(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

/// A host connection's name: its certificate pin, 64 hex characters.
export const isAudience = (v) => typeof v === 'string' && /^[0-9a-f]{64}$/.test(v);

/// A ticket id: 16 random bytes as 32 lowercase hex characters.
export const isTicketId = (v) => typeof v === 'string' && /^[0-9a-f]{32}$/.test(v);

/// A ticket for `accountId` to show on the host connection `audience`.
export async function makeIdentityTicket(env, { accountId, username = '', audience, now = nowS() }) {
  const id = [...crypto.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, '0')).join('');
  const payload = { a: accountId, u: username, h: audience, e: now + TICKET_TTL_S, j: id };
  const body = b64url(encoder.encode(JSON.stringify(payload)));
  const sig = await hmac(await ticketKey(env), `${PREFIX}.${body}`);
  return { ticket: `${PREFIX}.${body}.${b64url(sig)}`, expires: payload.e, id };
}

/// Why a ticket is refused, or the account it names. Codes: `no_ticket` (none
/// given), `invalid` (not a genuine ticket), `expired`, `wrong_host` (made for
/// another host connection).
export async function inspectIdentityTicket(env, ticket, { audience, now = nowS() }) {
  if (ticket === undefined || ticket === null || ticket === '') return { ok: false, code: 'no_ticket' };
  if (typeof ticket !== 'string' || ticket.length > 2048) return { ok: false, code: 'invalid' };
  const parts = ticket.split('.');
  if (parts.length !== 3 || parts[0] !== PREFIX) return { ok: false, code: 'invalid' };
  let given;
  let payload;
  try {
    given = fromB64url(parts[2]);
    payload = JSON.parse(new TextDecoder().decode(fromB64url(parts[1])));
  } catch {
    return { ok: false, code: 'invalid' };
  }
  const expected = await hmac(await ticketKey(env), `${PREFIX}.${parts[1]}`);
  if (!equalBytes(given, expected)) return { ok: false, code: 'invalid' };
  if (!payload || typeof payload.a !== 'string' || !Number.isSafeInteger(payload.e)) return { ok: false, code: 'invalid' };
  if (payload.e < now) return { ok: false, code: 'expired' };
  if (payload.h !== audience) return { ok: false, code: 'wrong_host' };
  return {
    ok: true,
    accountId: payload.a,
    username: typeof payload.u === 'string' ? payload.u : '',
    expires: payload.e,
    // Tickets made before ids existed carry none.
    id: isTicketId(payload.j) ? payload.j : null,
  };
}

/// The account a ticket names when it is genuine, unexpired and made for this
/// host connection; otherwise null.
export async function verifyIdentityTicket(env, ticket, { audience, now = nowS() }) {
  const r = await inspectIdentityTicket(env, ticket, { audience, now });
  return r.ok ? { accountId: r.accountId, username: r.username, expires: r.expires } : null;
}

async function readBody(request) {
  try {
    return await request.json();
  } catch {
    return null;
  }
}

/// Route `/api/identity/*`. Returns null for anything else.
/// `deps`: `verifyAuth` and `json` from index.js.
export async function handleIdentityRoute(request, url, env, cors, deps) {
  const path = url.pathname;
  if (path !== '/api/identity/ticket' && path !== '/api/identity/verify') return null;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const reply = (body, status = 200) => deps.json(body, status, headers);
  const error = (status, message, code) => reply({ error: message, code }, status);
  if (request.method !== 'POST') return error(405, 'Method not allowed', 'method_not_allowed');
  if (!env.JWT_SECRET) return error(503, 'Identity tickets are not configured on this deployment', 'identity_unavailable');

  const body = await readBody(request);
  if (!body) return error(400, 'Body must be JSON', 'invalid_json');
  if (!isAudience(body.audience)) return error(400, 'audience must be the host certificate pin (64 hex characters)', 'invalid_request');

  if (path === '/api/identity/ticket') {
    const accountId = await deps.verifyAuth(request, env);
    if (!accountId) return error(401, 'Sign in to connect as yourself', 'unauthenticated');
    let username = '';
    try {
      const raw = await env.USERS.get(`user:${accountId}`);
      if (raw) username = String(JSON.parse(raw).username || '').slice(0, 64);
    } catch {
      // A missing name costs the other side a display name, never the ticket.
    }
    const { ticket, expires, id } = await makeIdentityTicket(env, { accountId, username, audience: body.audience });
    return reply({ ticket, account_id: accountId, username, expires, id });
  }

  const who = await verifyIdentityTicket(env, body.ticket, { audience: body.audience });
  if (!who) return error(401, 'This ticket is not valid for this host connection', 'invalid_ticket');
  return reply({ account_id: who.accountId, username: who.username, expires: who.expires });
}
