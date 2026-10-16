// =============================================================================
// Adult content: off for every account until the account turns it on
// =============================================================================
//
// McKale, 2026-10-08: adult content is filtered off by default and is reached
// only through a setting in Profile. Adult content is the two top bands of the
// Guardian Code Part V: `adult_18` (sexual content) and `adult_21` (alcohol and
// tobacco, and showing the use of psychedelics). A listing in either band is
// invisible and unplayable for an account until that account turns Adult content
// on; signed out is off. An `adult_21` listing also needs a verified age of 21
// or over. A listing with no rating, or one this Worker does not know, is
// `adult_21` here, the most restrictive band. The author of a listing always
// sees and plays their own. The Worker enforces this on every surface that
// serves a listing or its content; the site only renders what it is given.
//
//     viewer                                         highest band shown
//     signed out, or Adult content off               mature_17
//     on, verified age under 21 (or none on record)  adult_18
//     on, verified age 21 or over                    adult_21
//
// The age is the date of birth requireVerifiedAdult (index.js) resolves: the one
// read off the identity document first, then the one pinned to the identity
// record, then the one declared at registration (resolveDob). It is computed at
// view time from that date, so an account becomes eligible on its 21st birthday
// with nobody doing anything. Only the date of birth is cached, briefly and per
// isolate; "under 21" is never kept as an answer (age21Eligible).
//
// The setting is `adult-content:{accountId}` in USERS: '1' when on, no key when
// off. Turning it on needs a signed-in account that is a verified adult
// (index.js requireVerifiedAdult: identity documents verified, and an age at or
// above the jurisdiction's minimum). Turning it off is always allowed. A write
// that changes nothing is not made.
//
//     GET  /api/account/adult-content   { enabled, can_enable, age_21_eligible, code?, reason? }
//     POST /api/account/adult-content   { enabled } -> { ok, enabled, changed }
//
// The lists (the Gallery, live now, makers, popular tags) show each viewer
// nothing above the band in the table; `?rating_max=` narrows that and never
// widens it. Their cached answers stay one per rating slice, never one per
// account, and the slice is worked out from the viewer before the cache is read,
// so a slice computed for a viewer of 21 or over is never handed to anyone else.
// Every answer computed for a signed-in viewer is `private, no-store`, and only
// a signed-in viewer can have the setting on, so no cache ever holds an answer
// with adult content in it (viewerHeaders).
//
// An adult listing's images (its thumbnail, its cover and its preview frames)
// are fetched by image tags, which carry no session. So each image route asks
// for a signed URL instead: an HMAC over the URL's path and an expiry, which
// only this Worker can make and which it writes into the listing's own
// `thumbnail_url`, `cover_url` and preview `url` fields for a viewer allowed to
// see the listing (mayView: one whose setting, and for `adult_21` whose verified
// age, allows it; or its author). A request
// without a valid signature is refused, the author's included: an image tag
// cannot say who it is. Every other listing's images keep their public,
// cached URLs exactly as they were.
//
// A signed URL is not bound to a viewer (an image tag could not present one),
// so whoever holds it can load that one image until it expires: its window is
// the exposure. It is issued only to an allowed viewer, it names one image of
// one listing, and it lives 15 to 30 minutes (IMAGE_URL_WINDOW_SECONDS).
//
// Not gated, because the client that asks sends no session: the commerce
// catalog (the Player reads it in game). It says so where it is served.

import { RATINGS } from './moderation.mjs';

const KEY = (accountId) => `adult-content:${accountId}`;
const ROUTE = '/api/account/adult-content';

/// Sexual content (Guardian Code Part V).
export const ADULT_RATING = 'adult_18';
/// Alcohol and tobacco, and showing the use of psychedelics (Part V). Also the most restrictive band, so it is what a
/// listing with no rating, or one this Worker does not know, reads as.
export const AGE_21_RATING = 'adult_21';
/// The bands the Adult content setting gates.
export const ADULT_RATINGS = Object.freeze([ADULT_RATING, AGE_21_RATING]);
/// The highest rating a viewer whose Adult content is off is shown.
export const OFF_CAP = 'mature_17';
/// The highest rating a viewer whose Adult content is on, and whose verified age is under 21 or not on record, is shown.
export const UNDER_21_CAP = 'adult_18';
/// The verified age the `adult_21` band needs.
export const AGE_21 = 21;
export const ADULT_CONTENT_OFF = 'This listing is adult content. Turn on Adult content in your Profile to see it.';
export const AGE_21_REQUIRED = 'This listing is for adults 21 and over. It opens once your verified age is 21.';

/// A visitor with no session: Adult content off.
export const SIGNED_OUT = Object.freeze({ accountId: null, signedIn: false, adult: false, age21: false });

/// The rating the gates read: the listing's when it is one of RATINGS, `adult_21` otherwise (none, or one this Worker does not know).
export function ratingOf(sim) {
  const rating = sim?.moderation?.rating;
  return RATINGS.includes(rating) ? rating : AGE_21_RATING;
}

/// Whether a listing is adult content: rated `adult_18` or `adult_21`, or not rated at all.
export const isAdultListing = (sim) => ADULT_RATINGS.includes(ratingOf(sim));

// -----------------------------------------------------------------------------
// The verified age
// -----------------------------------------------------------------------------

/// The date of birth on record, by strength of evidence, exactly as requireVerifiedAdult (index.js) resolves it:
///   1. `extracted_dob`, read off the identity document; always wins when present;
///   2. `age_dob`, resolved before and pinned to the identity record, so a later edit to a declared birthday cannot move it;
///   3. `user.birthday`, declared at registration (accounts verified under the older flow, which captured no document date).
/// `{ dob, source }`: `dob` is '' and `source` null when there is none.
export function resolveDob(kyc, user) {
  const dob = kyc?.extracted_dob || kyc?.age_dob || user?.birthday || '';
  const source = kyc?.extracted_dob ? 'document'
    : (kyc?.age_dob ? kyc.age_source || 'pinned' : (user?.birthday ? 'declared' : null));
  return { dob, source };
}

/// Whole years from `dob` (YYYY-MM-DD) to `nowMs`, in UTC. Null when the date is absent, malformed, not a real calendar date
/// (2009-02-30), or after `nowMs`. A caller treats null as "age unknown" and refuses.
export function ageAt(dob, nowMs = Date.now()) {
  if (typeof dob !== 'string') return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(dob.trim());
  if (!m) return null;
  const [y, mo, d] = [Number(m[1]), Number(m[2]), Number(m[3])];
  const t = Date.UTC(y, mo - 1, d);
  const dt = new Date(t);
  if (dt.getUTCFullYear() !== y || dt.getUTCMonth() !== mo - 1 || dt.getUTCDate() !== d) return null;
  const now = new Date(nowMs);
  if (t > now.getTime()) return null;
  let age = now.getUTCFullYear() - y;
  if (now.getUTCMonth() < mo - 1 || (now.getUTCMonth() === mo - 1 && now.getUTCDate() < d)) age -= 1;
  return age;
}

/// How long one isolate keeps an account's resolved date of birth. The date, never the answer: the age is computed from it
/// on every call, so a birthday takes effect at once even while the date is held.
export const DOB_CACHE_MS = 5 * 60 * 1000;
const DOB_CACHE_MAX = 2000;
/// KYC_STATUS binding -> Map(accountId -> { dob, at }). Keyed by the binding, so two environments never share an entry.
const dobCache = new WeakMap();

/// The account's verified date of birth, or '' when it has none: no identity record, documents not verified, no date on
/// record, or storage that cannot be read. Only a real date is cached; '' is read again next time.
async function verifiedDob(env, accountId, nowMs) {
  const store = env?.KYC_STATUS;
  if (!store || typeof accountId !== 'string' || !accountId) return '';
  let held = dobCache.get(store);
  if (!held) dobCache.set(store, (held = new Map()));
  const hit = held.get(accountId);
  if (hit && nowMs >= hit.at && nowMs - hit.at < DOB_CACHE_MS) return hit.dob;
  let kyc;
  try {
    const raw = await store.get(`kyc-${accountId}-front`);
    kyc = raw ? JSON.parse(raw) : null;
  } catch {
    return '';
  }
  // 'linked' is what registration writes when it attaches a session to a user; it keeps the original verified outcome.
  if (!kyc || (kyc.status !== 'verified' && kyc.status !== 'linked')) return '';
  let user = null;
  if (!kyc.extracted_dob && !kyc.age_dob) {
    try {
      const raw = await env.USERS?.get(`user:${accountId}`);
      user = raw ? JSON.parse(raw) : null;
    } catch {
      user = null;
    }
  }
  const { dob } = resolveDob(kyc, user);
  if (ageAt(dob, nowMs) !== null) {
    if (held.size >= DOB_CACHE_MAX) held.clear();
    held.set(accountId, { dob, at: nowMs });
  }
  return dob;
}

/// Whether the account's verified age is 21 or over at `nowMs`. Computed from the verified date of birth on every call, so
/// an account under 21 becomes eligible on its 21st birthday; an age that cannot be established is not eligible.
export async function age21Eligible(env, accountId, nowMs = Date.now()) {
  const age = ageAt(await verifiedDob(env, accountId, nowMs), nowMs);
  return age !== null && age >= AGE_21;
}

/// The stored setting: true, false, or null when storage could not be read.
async function readSetting(env, accountId) {
  if (!env?.USERS || typeof accountId !== 'string' || !accountId) return false;
  try {
    return (await env.USERS.get(KEY(accountId))) === '1';
  } catch {
    return null;
  }
}

/// Whether the account turned Adult content on. False with no account, and false when storage cannot be read.
export async function adultContentOn(env, accountId) {
  return (await readSetting(env, accountId)) === true;
}

/// The viewer an account is: `{ accountId, signedIn: true, adult, age21 }`, or SIGNED_OUT without one. `adult` is the
/// setting; `age21` is whether the verified age is 21 or over at `nowMs`, read only when the setting is on (the 21+ band is
/// adult content, so it can reach no one else). For a caller that already knows the account (a ticket, a purchase).
export async function viewerFor(env, accountId, nowMs = Date.now()) {
  if (typeof accountId !== 'string' || !accountId) return SIGNED_OUT;
  const adult = await adultContentOn(env, accountId);
  return { accountId, signedIn: true, adult, age21: adult ? await age21Eligible(env, accountId, nowMs) : false };
}

/// Who is asking, for a route that never refuses a visitor with no session: `{ accountId, signedIn, adult, age21 }`.
/// `authenticate` is index.js verifyAuth. A session that does not check out, or a check that throws, is signed out.
export async function viewerOf(request, env, authenticate, nowMs = Date.now()) {
  let accountId = null;
  try {
    accountId = await authenticate(request, env);
  } catch {
    accountId = null;
  }
  return viewerFor(env, accountId, nowMs);
}

/// The highest band this viewer is shown: `mature_17` while Adult content is off or signed out, `adult_18` when it is on and
/// the verified age is under 21 or not on record, and every band when it is on and the verified age is 21 or over.
export function viewerCap(viewer) {
  if (viewer?.adult !== true) return OFF_CAP;
  return viewer.age21 === true ? AGE_21_RATING : UNDER_21_CAP;
}

/// The highest rating a list shows this viewer: `asked` (a `?rating_max=` value) when it is a rating, else every rating,
/// and never past the viewer's own cap (viewerCap). Always one of RATINGS, so it can key a cache.
export function ratingCap(asked, viewer) {
  const wanted = RATINGS.includes(asked) ? asked : AGE_21_RATING;
  const cap = viewerCap(viewer);
  return RATINGS.indexOf(wanted) > RATINGS.indexOf(cap) ? cap : wanted;
}

/// Whether this viewer may see and play this listing: anything that is not adult content; adult content up to the viewer's
/// cap (`adult_18` with the setting on, `adult_21` too with a verified age of 21 or over); and the author's own listing always.
///
///     mayView(sim, viewer) -> boolean
///       sim     the listing record as stored (`sim:{id}`): reads `moderation.rating` and `author_id`
///       viewer  `{ accountId, signedIn, adult, age21 }` from viewerOf(request, env, verifyAuth); SIGNED_OUT or undefined is off
export function mayView(sim, viewer) {
  if (!isAdultListing(sim)) return true;
  if (!!viewer?.accountId && viewer.accountId === sim?.author_id) return true;
  return RATINGS.indexOf(ratingOf(sim)) <= RATINGS.indexOf(viewerCap(viewer));
}

/// What a refusal says to this viewer: `adult_content_off` while the setting is off, and `age_21_required` to a viewer who
/// has it on, since only the 21+ band can still be closed to them. `{ error, code, signed_in }`.
export function refusalOf(viewer) {
  const signed_in = viewer?.signedIn === true;
  if (viewer?.adult === true) return { error: AGE_21_REQUIRED, code: 'age_21_required', signed_in };
  return { error: ADULT_CONTENT_OFF, code: 'adult_content_off', signed_in };
}

/// The refusal every gated surface gives. Never stored by a cache, so turning the setting on, or a birthday, takes effect at once.
export function adultRefusal(json, cors, viewer) {
  return json(refusalOf(viewer), 403, { ...cors, 'Cache-Control': 'private, no-store' });
}

/// The refusal when `sim` is adult content this request's viewer may not see, else null. The session, the setting and the
/// age are read only for an adult listing, so every other listing costs nothing. `deps`: authenticate, json, cors.
export async function refuseAdult(request, env, sim, deps) {
  if (!isAdultListing(sim)) return null;
  const viewer = await viewerOf(request, env, deps.authenticate);
  return mayView(sim, viewer) ? null : adultRefusal(deps.json, deps.cors, viewer);
}

/// Headers for an answer that depends on the viewer's setting. One computed for a signed-in viewer is `private, no-store`:
/// only a signed-in viewer can have Adult content on, so no cache, shared or the browser's own, ever keeps an answer that
/// holds adult content, and turning the setting off or signing out takes effect on the next request. The signed-out answer
/// is the same for everyone signed out, so it keeps `publicCache`. `Vary: Authorization` keeps a cache that honours it from
/// handing that signed-out answer to a signed-in viewer. The client proves its session with the Authorization header and
/// never a cookie (src/cors.mjs), so that header is all the answer depends on.
export function viewerHeaders(cors, viewer, publicCache = null) {
  const vary = String(cors?.Vary || '').split(',').map((v) => v.trim()).filter(Boolean);
  if (!vary.some((v) => v.toLowerCase() === 'authorization')) vary.push('Authorization');
  const headers = { ...cors, Vary: vary.join(', ') };
  if (viewer?.signedIn === true) headers['Cache-Control'] = 'private, no-store';
  else if (publicCache) headers['Cache-Control'] = publicCache;
  return headers;
}

// -----------------------------------------------------------------------------
// Signed image URLs
// -----------------------------------------------------------------------------

/// Windows are this long. A URL issued in one window expires at the end of the next, so it lives 15 to 30 minutes and is
/// the same URL for every request in its window (a browser keeps it in its private cache).
export const IMAGE_URL_WINDOW_SECONDS = 900;
/// The key is derived from JWT_SECRET under its own label, never the secret itself: HMAC-SHA256(JWT_SECRET, label).
const IMAGE_KEY_LABEL = 'adult-image-url-v1';
/// The image routes of a listing: its thumbnail, its cover and its preview frames.
const isImagePath = (path) => /^\/api\/simulations\/[a-f0-9-]+\/(thumbnail|cover|preview\/[0-7])$/.test(path);

const encoder = new TextEncoder();
let derivedKey = { secret: null, key: null };

/// The signing key, derived once per isolate; null when JWT_SECRET is not configured (every adult image is then refused).
async function imageKey(env) {
  const secret = env?.JWT_SECRET;
  if (typeof secret !== 'string' || !secret) return null;
  if (derivedKey.secret !== secret) {
    const outer = await crypto.subtle.importKey('raw', encoder.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
    const raw = await crypto.subtle.sign('HMAC', outer, encoder.encode(IMAGE_KEY_LABEL));
    derivedKey = { secret, key: await crypto.subtle.importKey('raw', raw, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']) };
  }
  return derivedKey.key;
}

const toB64url = (bytes) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
function fromB64url(text) {
  const b64 = text.replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(b64 + '='.repeat((4 - (b64.length % 4)) % 4)), (c) => c.charCodeAt(0));
}

/// The HMAC of one image path and its expiry, 32 bytes.
async function imageMac(key, path, expires) {
  return new Uint8Array(await crypto.subtle.sign('HMAC', key, encoder.encode(`${path}\n${expires}`)));
}

/// When a URL issued at `nowMs` expires, in seconds: the end of the window after the current one.
export function imageUrlExpiry(nowMs = Date.now()) {
  return (Math.floor(Math.floor(nowMs / 1000) / IMAGE_URL_WINDOW_SECONDS) + 2) * IMAGE_URL_WINDOW_SECONDS;
}

/// A signed URL for one of a listing's image paths, or null (JWT_SECRET is not configured, or `path` is not an image path).
///
///     signedImageUrl(env, origin, path, nowMs?) -> Promise<string | null>
///       origin  the Worker's origin the URL names, e.g. 'https://api.eustress.dev' (new URL(request.url).origin)
///       path    one of a listing's image paths: /api/simulations/ID/thumbnail, /api/simulations/ID/cover, or
///               /api/simulations/ID/preview/N with N from 0 to 7; with or without a query of its own (the cover's `?v=`
///               is kept); only the path is signed
///       ->      `${origin}${path}` with `exp` (seconds) and `sig` added to its query, the same string for every call in one
///               15-minute window, valid 15 to 30 minutes
export async function signedImageUrl(env, origin, path, nowMs = Date.now()) {
  let url;
  try {
    url = new URL(path, origin);
  } catch {
    return null;
  }
  if (!isImagePath(url.pathname)) return null;
  const key = await imageKey(env);
  if (!key) return null;
  const expires = imageUrlExpiry(nowMs);
  url.searchParams.delete('exp');
  url.searchParams.delete('sig');
  url.searchParams.set('exp', String(expires));
  url.searchParams.set('sig', toB64url(await imageMac(key, url.pathname, expires)));
  return url.toString();
}

/// A signed URL for a listing's thumbnail, for a viewer the caller has already found may see the listing (mayView), or null
/// when the listing has no thumbnail or the URL cannot be signed. The URL is this Worker's thumbnail route for the listing.
///
///     signedThumbnailUrl(env, origin, sim, nowMs?) -> Promise<string | null>
///       sim  the listing record as stored; reads `id` and `thumbnail_url`
export async function signedThumbnailUrl(env, origin, sim, nowMs = Date.now()) {
  if (!sim?.id || !sim.thumbnail_url) return null;
  return signedImageUrl(env, origin, `/api/simulations/${sim.id}/thumbnail`, nowMs);
}

/// `url` signed when it is one of this listing's image routes on any host; any other URL (an external image the author
/// named) is returned as it is. Null when it should be signed and cannot be.
export async function signedListingUrl(env, url, sim, nowMs = Date.now()) {
  if (typeof url !== 'string' || !sim?.id) return url;
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return url;
  }
  if (!isImagePath(parsed.pathname) || parsed.pathname.split('/')[3] !== sim.id) return url;
  return signedImageUrl(env, parsed.origin, `${parsed.pathname}${parsed.search}`, nowMs);
}

/// A listing view (a Gallery card, the listing page, a project) with its `thumbnail_url` and `cover_url` signed when the
/// listing is adult content and `viewer` may see it; otherwise the view as it was. No field is added or renamed.
export async function withSignedImages(env, view, sim, viewer, nowMs = Date.now()) {
  if (!view || !isAdultListing(sim) || !mayView(sim, viewer)) return view;
  const out = { ...view };
  for (const field of ['thumbnail_url', 'cover_url']) {
    if (out[field]) out[field] = await signedListingUrl(env, out[field], sim, nowMs);
  }
  return out;
}

/// Whether a request URL carries a valid signature for its own path: `{ ok: true, expires }`, or `{ ok: false, code }` with
/// code `adult_image_unsigned` (missing, malformed or wrong) or `adult_image_expired`. The signature is compared in constant
/// time over its full 32 bytes.
export async function verifyImageUrl(env, url, nowMs = Date.now()) {
  const unsigned = { ok: false, code: 'adult_image_unsigned' };
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return unsigned;
  }
  const exp = parsed.searchParams.get('exp');
  const sig = parsed.searchParams.get('sig');
  if (!exp || !sig || !/^\d{1,12}$/.test(exp) || !/^[A-Za-z0-9_-]{43}$/.test(sig)) return unsigned;
  const expires = Number(exp);
  if (expires % IMAGE_URL_WINDOW_SECONDS !== 0) return unsigned;
  const key = await imageKey(env);
  if (!key) return unsigned;
  const given = fromB64url(sig);
  const wanted = await imageMac(key, parsed.pathname, expires);
  let diff = given.length ^ wanted.length;
  for (let i = 0; i < wanted.length; i++) diff |= (given[i] ?? 0) ^ wanted[i];
  if (diff !== 0) return unsigned;
  const now = Math.floor(nowMs / 1000);
  if (expires <= now) return { ok: false, code: 'adult_image_expired' };
  // No URL this Worker makes lives longer than two windows.
  if (expires - now > 2 * IMAGE_URL_WINDOW_SECONDS) return unsigned;
  return { ok: true, expires };
}

/// For one of a listing's image routes: null when the image may be served on this request, else the refusal (403,
/// `private, no-store`). A listing that is not adult content always passes. An adult listing's image needs a valid signed
/// URL for this exact path, whoever asks.
export async function refuseUnsignedImage(request, env, sim, json, cors, nowMs = Date.now()) {
  if (!isAdultListing(sim)) return null;
  const seen = await verifyImageUrl(env, request.url, nowMs);
  if (seen.ok) return null;
  const error = seen.code === 'adult_image_expired' ? 'This image link has expired. Load the listing again for a new one.' : 'This image needs the link its listing gives.';
  return json({ error, code: seen.code }, 403, { ...cors, 'Cache-Control': 'private, no-store' });
}

/// The Cache-Control for an image: `publicValue` (e.g. 'public, max-age=300') for a listing that is not adult content; for an
/// adult listing, whose request was signed, `private` with a max-age no longer than the signature has left or `publicValue`
/// allowed.
export function imageCacheControl(request, sim, publicValue, nowMs = Date.now()) {
  if (!isAdultListing(sim)) return publicValue;
  let left = 0;
  try {
    left = Number(new URL(request.url).searchParams.get('exp')) - Math.floor(nowMs / 1000);
  } catch {
    left = 0;
  }
  const allowed = Number(/max-age=(\d+)/.exec(publicValue || '')?.[1]) || 0;
  const maxAge = Math.max(0, Math.min(Number.isFinite(left) ? left : 0, allowed));
  return maxAge > 0 ? `private, max-age=${maxAge}` : 'private, no-store';
}

/// The reason a refusal gives, by requireVerifiedAdult's code. Its own reasons are written for payouts, so this setting
/// says what it means here instead.
const REFUSAL_REASONS = {
  kyc_required: 'Verify your identity to turn on Adult content.',
  kyc_incomplete: 'Your identity documents have not been verified yet. You can turn on Adult content once they are.',
  age_unverified: 'We could not confirm your date of birth. Contact support@eustress.dev.',
  underage: 'Adult content is for adults only.',
};

/// Whether the account may turn Adult content on: `{ ok: true }`, or `{ ok: false, status, error, reason, code }`. The user
/// record is read the way every caller of requireVerifiedAdult reads it; a check that cannot be made is a no.
async function verifiedAdult(env, userId, deps) {
  let raw;
  try {
    raw = await env.USERS.get(`user:${userId}`);
  } catch {
    raw = null;
  }
  if (!raw) return { ok: false, status: 404, error: 'User not found', code: 'user_not_found', reason: 'This account could not be found.' };
  try {
    const gate = await deps.requireVerifiedAdult(userId, JSON.parse(raw), env);
    if (gate?.ok === true) return { ok: true };
    const code = gate?.code || 'not_verified';
    return { ok: false, status: gate?.status || 403, error: 'Adult content is for verified adults only.', reason: REFUSAL_REASONS[code] || 'Adult content needs a verified adult account.', code };
  } catch {
    return { ok: false, status: 503, error: 'Identity verification could not be checked right now', reason: 'Try again in a minute.', code: 'verification_unavailable' };
  }
}

/// `GET` and `POST /api/account/adult-content`. A response for this route, or `null` for any other path or method.
/// `deps`: authenticate (the session check), json, cors, requireVerifiedAdult (index.js).
export async function handleAdultContentRoute(request, url, env, deps) {
  const { json, cors } = deps;
  if (url.pathname !== ROUTE || (request.method !== 'GET' && request.method !== 'POST')) return null;
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const userId = await deps.authenticate(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, headers);

  if (request.method === 'GET') {
    const enabled = await adultContentOn(env, userId);
    const gate = await verifiedAdult(env, userId, deps);
    // Whether the verified age is 21 or over today, whatever the setting, so the site can say why 21+ listings are hidden.
    const age21 = gate.ok ? await age21Eligible(env, userId) : false;
    return json({ enabled, can_enable: gate.ok, age_21_eligible: age21, ...(gate.ok ? {} : { code: gate.code, reason: gate.reason }) }, 200, headers);
  }

  const body = await request.json().catch(() => ({}));
  if (typeof body?.enabled !== 'boolean') return json({ error: 'enabled must be true or false', code: 'bad_request' }, 400, headers);
  // Unknown (storage unreadable) counts as a change, so turning it off still deletes the key.
  const was = await readSetting(env, userId);

  if (body.enabled === false) {
    if (was !== false) await env.USERS.delete(KEY(userId));
    return json({ ok: true, enabled: false, changed: was !== false }, 200, headers);
  }

  // Turning it on is checked every time it is asked for, even when it is already on.
  const gate = await verifiedAdult(env, userId, deps);
  if (!gate.ok) return json({ error: 'Adult content is for verified adults only.', code: gate.code, reason: gate.reason }, gate.status, headers);
  if (was !== true) await env.USERS.put(KEY(userId), '1');
  return json({ ok: true, enabled: true, changed: was !== true }, 200, headers);
}

export const adultContentKey = KEY;
