// =============================================================================
// Solo play passes: the website's sign-in, carried to the Player for one listing
// =============================================================================
//
// McKale, 2026-11-13: "Studio is not the source of truth. I am signed in on
// staging website." A Player opened from the website's Play button plays a
// listing as the person signed in on the website, not as whatever Studio on the
// same computer happens to be signed in to.
//
//   POST /api/simulations/{id}/play-ticket   session          -> {ticket, expires}
//   POST /api/simulations/{id}/play-pass     public  {ticket} -> {pass, expires}
//
// 1. At the click, the signed-in Play page asks for a play ticket and appends it
//    to the solo link: `eustress-player://play/<id>?api=<name>&ticket=<t>`. The
//    ticket is an identity ticket (identity.mjs, the one a join link carries) whose
//    audience is `play:<id>`, so it names this listing and no host: no 64-hex
//    certificate pin is ever `play:...`, so /live/admit and /api/identity/verify
//    refuse it, and a join ticket is refused here. It lasts PLAY_TICKET_TTL_S (two minutes),
//    and one account holds at most MAX_PLAY_TICKETS_PER_ACCOUNT unexpired ones per listing (live.mjs note_play_ticket);
//    the mint is also rate limited per account.
//    The mint answers an unknown, unlisted or rating-gated listing exactly as
//    `GET /api/simulations/{id}` does, so it tells nobody more than that GET.
// 2. The Player redeems the ticket once. Its id is recorded in the listing's own
//    LiveHost object (`play:<id>`, live.mjs spend_play_ticket), the store that
//    spends join tickets at /live/admit, so a second redeem is refused.
// 3. The answer is a pass: `epp1.<base64url payload>.<base64url HMAC-SHA256>`,
//    payload {a: account, s: listing id, i: when the ticket was minted, e: expiry},
//    all times in unix seconds, valid PLAY_PASS_TTL_S for this listing only. The
//    HMAC key is HMAC(JWT_SECRET, PASS_KEY_LABEL): its own label, so a pass, an
//    identity ticket, a signed image URL and a session token can never pass for
//    one another.
// 4. The Player sends `Authorization: PlayPass <pass>` on the listing GET, its
//    world manifest and chunks, and the older `.pak` download. verifyAuth reads
//    only `Bearer `, so a pass is never a session, and a session is never a pass.
//    The pass resolves to the account's viewer exactly as a session would
//    (adult_content.mjs viewerFor: the account's own Adult content setting and
//    verified age), so the rating gate judges the real account.
//
// Fail closed: a pass that is missing, malformed, forged, expired, made for
// another listing, or issued before the account's session floor (a sign-out or
// a ban since) is no credential at all, never an error that lets anything
// through. Nothing here logs a ticket or a pass.
// =============================================================================

import { makeIdentityTicket, inspectIdentityTicket } from './identity.mjs';
import { simWithLegalLock } from './moderation.mjs';
import { refuseAdult } from './adult_content.mjs';
import { spendPlayTicket, notePlayTicket } from './live.mjs';
import { addressKey } from './guest.mjs';

/// How long a play ticket lasts. It rides in a launch URL and on a command line, and the Player redeems it within seconds
/// of launch, so anything longer only helps someone who copied it (Security review M2, 2026-11-13). Join tickets keep
/// identity.mjs TICKET_TTL_S.
export const PLAY_TICKET_TTL_S = 120;

/// The refusals the play object answers that the routes pass on, by code: each one's status and fixed words. Anything
/// else (a storage fault, an unreadable reply) is SPEND_UNAVAILABLE, so no internal message reaches a caller (review L1).
const KNOWN_REFUSALS = Object.freeze({
  used: [403, 'This Play link was already used. Play again from the gallery for a new one.'],
  expired: [401, 'This Play link has expired. Play again from the gallery.'],
  busy: [503, 'The gallery could not open this right now. Try again in a minute.'],
  too_many_play_tickets: [429, 'Too many Play links for this listing. Try again in two minutes.'],
});
const SPEND_UNAVAILABLE = [503, 'The gallery could not open this right now. Try again in a minute.', 'busy'];

/// `[status, words, code]` for a play object's answer that is not ok: a known refusal as it is, anything else
/// SPEND_UNAVAILABLE.
export function publicRefusal(answer) {
  const known = typeof answer?.code === 'string' && Object.hasOwn(KNOWN_REFUSALS, answer.code) ? KNOWN_REFUSALS[answer.code] : null;
  return known ? [known[0], known[1], answer.code] : SPEND_UNAVAILABLE;
}

/// A call into the play object that never throws: a failure is `{ ok: false }` with no code, so it reads as unavailable.
async function playObject(call) {
  try {
    return (await call()) || { ok: false };
  } catch {
    return { ok: false };
  }
}

/// How long a pass lasts: long enough to download a large world and play it, and
/// it reads only the one listing it names.
export const PLAY_PASS_TTL_S = 60 * 60;
/// The Authorization scheme a pass travels under: `Authorization: PlayPass <pass>`.
export const PLAY_PASS_SCHEME = 'PlayPass';

const PASS_PREFIX = 'epp1';
const PASS_KEY_LABEL = 'eustress-play-pass-v1';
/// A pass as this Worker makes it: the prefix, a payload of at most 1,024 characters and a 32-byte signature.
const PASS_SHAPE = /^epp1\.[A-Za-z0-9_-]{1,1024}\.[A-Za-z0-9_-]{43}$/;
const ROUTE = /^\/api\/simulations\/([a-f0-9-]+)\/(play-ticket|play-pass)$/;

const encoder = new TextEncoder();
const nowS = () => Math.floor(Date.now() / 1000);

/// The audience of a play ticket for listing `simId`.
export const playAudience = (simId) => `play:${simId}`;

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

/// The pass key: HMAC(JWT_SECRET, label), never the secret itself.
async function passKey(env) {
  if (!env?.JWT_SECRET) throw new Error('JWT_SECRET is not configured');
  return hmac(encoder.encode(env.JWT_SECRET), PASS_KEY_LABEL);
}

function equalBytes(a, b) {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

/// A pass for `accountId` to read listing `simId`. `issued` is when the ticket it was redeemed from was minted: a sign-out
/// or a ban after that ends the pass (aboveFloor).
export async function makePlayPass(env, { accountId, simId, issued, now = nowS() }) {
  const payload = { a: accountId, s: simId, i: issued, e: now + PLAY_PASS_TTL_S };
  const body = b64url(encoder.encode(JSON.stringify(payload)));
  const sig = await hmac(await passKey(env), `${PASS_PREFIX}.${body}`);
  return { pass: `${PASS_PREFIX}.${body}.${b64url(sig)}`, expires: payload.e };
}

/// Why a pass is refused, or the account it names. Codes: `no_pass`, `invalid` (not a genuine pass), `expired`,
/// `wrong_listing` (made for another listing).
export async function inspectPlayPass(env, pass, { simId, now = nowS() }) {
  if (pass === undefined || pass === null || pass === '') return { ok: false, code: 'no_pass' };
  if (typeof pass !== 'string' || !PASS_SHAPE.test(pass)) return { ok: false, code: 'invalid' };
  const parts = pass.split('.');
  let given;
  let payload;
  try {
    given = fromB64url(parts[2]);
    payload = JSON.parse(new TextDecoder().decode(fromB64url(parts[1])));
  } catch {
    return { ok: false, code: 'invalid' };
  }
  const expected = await hmac(await passKey(env), `${PASS_PREFIX}.${parts[1]}`);
  if (!equalBytes(given, expected)) return { ok: false, code: 'invalid' };
  if (!payload || typeof payload.a !== 'string' || !payload.a || typeof payload.s !== 'string'
    || !Number.isSafeInteger(payload.e) || !Number.isSafeInteger(payload.i)) return { ok: false, code: 'invalid' };
  if (payload.e <= now) return { ok: false, code: 'expired' };
  // No pass this Worker makes lives longer than PLAY_PASS_TTL_S.
  if (payload.e - now > PLAY_PASS_TTL_S) return { ok: false, code: 'invalid' };
  if (payload.s !== simId) return { ok: false, code: 'wrong_listing' };
  return { ok: true, accountId: payload.a, issued: payload.i, expires: payload.e };
}

/// The pass a request carries (`Authorization: PlayPass <pass>`), or ''. A `Bearer` header is never a pass.
export function playPassOf(request) {
  const header = request?.headers?.get?.('Authorization') || '';
  return header.startsWith(`${PLAY_PASS_SCHEME} `) ? header.slice(PLAY_PASS_SCHEME.length + 1).trim() : '';
}

/// Whether something issued at `issued` (unix seconds) for `accountId` is at or after the account's session floor, as
/// verifyAuth (index.js) judges a session: a sign-out or a ban moves the floor past it.
async function aboveFloor(env, accountId, issued) {
  if (!env?.USERS) return true;
  const floor = Number(await env.USERS.get(`authfloor:${accountId}`)) || 0;
  return !floor || issued >= floor;
}

/// The account this request's play pass names for listing `simId`, or null: no pass, or one that is malformed, forged,
/// expired, made for another listing, or issued before the account's session floor. Never throws: anything that goes wrong
/// is no credential.
export async function accountFromPlayPass(env, request, simId, now = nowS()) {
  try {
    const pass = playPassOf(request);
    if (!pass) return null;
    const seen = await inspectPlayPass(env, pass, { simId, now });
    if (!seen.ok) return null;
    return (await aboveFloor(env, seen.accountId, seen.issued)) ? seen.accountId : null;
  } catch {
    return null;
  }
}

/// `authenticate` (index.js verifyAuth) that also takes this listing's play pass. The pass is resolved once per wrapper,
/// so make one wrapper per request.
export function withPlayPass(authenticate, simId) {
  let passAccount;
  return async (request, env) => {
    if (passAccount === undefined) passAccount = await accountFromPlayPass(env, request, simId);
    return passAccount || authenticate(request, env);
  };
}

/// Route `/api/simulations/{id}/play-ticket` and `/play-pass`. Null for anything else.
/// `deps`: verifyAuth, requireAdmin, json and isListable from index.js.
export async function handlePlayPassRoute(request, url, env, cors, deps) {
  const match = url.pathname.match(ROUTE);
  if (!match) return null;
  const [, simId, route] = match;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const reply = (body, status = 200) => deps.json(body, status, headers);
  const error = (status, message, code) => reply({ error: message, code }, status);
  if (request.method !== 'POST') return error(405, 'Method not allowed', 'method_not_allowed');
  if (!env.JWT_SECRET) return error(503, 'Play passes are not configured on this deployment', 'play_pass_unavailable');
  return route === 'play-ticket'
    ? mintPlayTicket(request, simId, env, headers, deps, reply, error)
    : redeemPlayTicket(request, simId, env, reply, error);
}

/// The signed-in viewer's play ticket for a listing they may open. Every refusal is the listing GET's own answer
/// (index.js handleGetSimulation): `Simulation not found` for a listing that does not exist or that this viewer may not see
/// unlisted, and the rating gate's refusal for one above the viewer's band.
async function mintPlayTicket(request, simId, env, headers, deps, reply, error) {
  const accountId = await deps.verifyAuth(request, env);
  if (!accountId) return error(401, 'Sign in to play as yourself', 'unauthenticated');
  // Per account, before anything is read (Security review M1).
  // No limiter bound means no limit could be kept: refuse, as the guest routes do.
  if (!env.AUTH_RATE_LIMITER) return deps.json({ error: 'Play links are not available right now.', code: 'busy' }, 503, headers);
  {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `play-ticket:${accountId}` });
    if (!success) return deps.json({ error: 'Too many Play links. Try again in a minute.', code: 'rate_limited' }, 429, { ...headers, 'Retry-After': '60' });
  }
  const notFound = () => deps.json({ error: 'Simulation not found' }, 404, headers);
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return notFound();
  const sim = await simWithLegalLock(env, JSON.parse(raw));
  if (deps.isListable(sim)) {
    const refused = await refuseAdult(request, env, sim, { authenticate: deps.verifyAuth, json: deps.json, cors: headers });
    if (refused) return refused;
  } else {
    const admin = await deps.requireAdmin(request, env);
    if (accountId !== sim.author_id && !admin) return notFound();
  }
  // No username: the Player needs none, and the ticket sits in a launch argument.
  const { ticket, expires, id } = await makeIdentityTicket(env, { accountId, username: '', audience: playAudience(simId), ttl: PLAY_TICKET_TTL_S });
  // Recorded against the account before it is handed out; one over the account's share is never handed out at all.
  const noted = await playObject(() => notePlayTicket(env, simId, { id, account: accountId, expires, now: Date.now() }));
  if (!noted.ok) {
    const [status, words, code] = publicRefusal(noted);
    return deps.json({ error: words, code }, status, status === 429 ? { ...headers, 'Retry-After': '120' } : headers);
  }
  return reply({ ticket, expires });
}

/// Spend a play ticket once, for a pass. The ticket must be genuine, unexpired, made for exactly this listing, minted after
/// the account's session floor, and never redeemed before.
async function redeemPlayTicket(request, simId, env, reply, error) {
  let body;
  try {
    body = await request.json();
  } catch {
    body = null;
  }
  // Per address, before any work: defense in depth, since a forged ticket costs only one HMAC.
  const address = addressKey(request.headers.get('CF-Connecting-IP'));
  if (!env.AUTH_RATE_LIMITER) return error(503, 'Play links are not available right now.', 'busy');
  if (address) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `play-pass:${address}` });
    if (!success) return error(429, 'Too many attempts. Try again in a minute.', 'rate_limited');
  }
  if (!body || typeof body !== 'object' || Array.isArray(body)) return error(400, 'Body must be JSON', 'invalid_json');
  const now = nowS();
  const seen = await inspectIdentityTicket(env, body.ticket, { audience: playAudience(simId), now });
  const notValid = () => error(401, 'This Play link is not valid. Play again from the gallery.', 'invalid');
  if (!seen.ok) {
    if (seen.code === 'no_ticket') return error(400, 'ticket is required', 'no_ticket');
    if (seen.code === 'expired') return error(401, 'This Play link has expired. Play again from the gallery.', 'expired');
    // `wrong_host`: a ticket for another listing, or a join ticket. Said the same way as a forged one.
    return notValid();
  }
  // A ticket with no id cannot be spent once.
  if (!seen.id) return notValid();
  // A play ticket lives PLAY_TICKET_TTL_S; one that claims longer was not made here for play.
  if (seen.expires - now > PLAY_TICKET_TTL_S) return notValid();
  const issued = seen.expires - PLAY_TICKET_TTL_S;
  if (!(await aboveFloor(env, seen.accountId, issued))) return notValid();
  const spent = await playObject(() => spendPlayTicket(env, simId, { id: seen.id, expires: seen.expires, now: Date.now() }));
  if (!spent.ok) {
    const [status, words, code] = publicRefusal(spent);
    return error(status, words, code);
  }
  const { pass, expires } = await makePlayPass(env, { accountId: seen.accountId, simId, issued, now });
  return reply({ pass, expires });
}
