// =============================================================================
// Visits: one launch or one join, once a day per person, per listing
// =============================================================================
//
// A listing's visit count ("Total Visits" on the author's page) used to rise when the play page asked the Worker for its launch
// link, so a page view, a refresh or a click that opened nothing each counted. A visit is now counted where something is actually
// fetched or admitted:
//
//   - the Player fetches the world (the committed manifest of an .echk listing, or the .pak of an older one);
//   - the host admits a joining player to a live session.
//
// At most once per person or device per listing per UTC day, so retries, reloads and a Player that restarts do not inflate it:
//
//   - a signed-in person is one `u:<account id>`; a guest joining a hosted session is one `g:<hash>` (guestVisitKey: an HMAC of
//     its guest id under this purpose's key for the day, so one address counts once a day however many leases it opens or
//     tickets it mints, and the marker cannot be matched to the guest's allowance);
//   - anyone else is one ADDRESS (not address plus browser: a browser string costs nothing to vary), keyed by an HMAC under a key
//     derived for this purpose AND THIS UTC DAY from the Worker's secret, so no address is stored, the secret itself signs nothing
//     here, and one address's markers on two days cannot be matched (a signed-out visitor is often a guest: GUEST_PLAY.md section 12);
//   - every marker is deleted 25 hours after it is written, which is past the end of the day it names;
//   - new anonymous markers for one listing are capped per minute by the platform limiter (VISIT_RATE_LIMITER), so an unauthenticated
//     flood cannot turn into unbounded writes.
//
// The count lives in its own key, `visits:{simId}`, and is ADDED to the listing's stored `play_count` when the listing is read
// (playCountOf). A visit never rewrites the listing record: that record also holds the moderation marker, and a whole-record write
// from here could restore an older marker over a newer one, or make the moderation system's own writes to it fail while a flood runs.
// Counting never makes a launch fail: any error here is logged and the launch goes on.

const DAY_MS = 24 * 3600 * 1000;
/// A marker names its own UTC day, so it is needed only until that day ends: kept at most 25 hours, as a guest's records are.
export const KEY_TTL_SECONDS = 25 * 3600;
const PURPOSE = 'eustress-visit-v2';

const dayOf = (now) => new Date(now - (now % DAY_MS)).toISOString().slice(0, 10);

const hex = (bytes) => [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, '0')).join('');
const encode = (text) => new TextEncoder().encode(text);

async function hmac(keyBytes, message) {
  const key = await crypto.subtle.importKey('raw', keyBytes, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await crypto.subtle.sign('HMAC', key, encode(message)));
}

/// The key a device is known by on the UTC day of `now`: an HMAC of its address under a key derived from the Worker's secret
/// for this one purpose and that day. The same address has another key the next day.
export async function deviceKey(env, address, now = Date.now()) {
  if (!env?.JWT_SECRET || !address) return 'd:unknown';
  try {
    const subkey = await hmac(encode(String(env.JWT_SECRET)), `${PURPOSE}:${dayOf(now)}`);
    return `d:${hex(await hmac(subkey, String(address))).slice(0, 32)}`;
  } catch {
    return 'd:unknown';
  }
}

/// The key a guest joining a hosted session is known by on the UTC day of `now`: an HMAC of its guest id (guest.mjs, itself a
/// day-keyed hash of the address) under this purpose's key for that day, so one address counts one visit a day however many
/// leases it opens, and the marker cannot be matched to the guest's allowance or to another day.
export async function guestVisitKey(env, guestId, now = Date.now()) {
  if (!env?.JWT_SECRET || typeof guestId !== 'string' || !guestId) return null;
  try {
    const subkey = await hmac(encode(String(env.JWT_SECRET)), `${PURPOSE}:${dayOf(now)}`);
    return `g:${hex(await hmac(subkey, `guest:${guestId}`)).slice(0, 32)}`;
  } catch {
    return null;
  }
}

/// Who is launching, as a short key: the account when signed in, otherwise the address.
export async function viewerKey(env, request, accountId, now = Date.now()) {
  if (accountId) return `u:${accountId}`;
  return deviceKey(env, request?.headers?.get?.('CF-Connecting-IP') || '', now);
}

/// The visits counted for a listing, apart from the count its record carries.
export async function visitsOf(env, simId) {
  try {
    return parseInt((await env.SOCIAL.get(`visits:${simId}`)) || '0', 10) || 0;
  } catch {
    return 0;
  }
}

/// The visit count a reader sees: what the record carries (from before) plus what has been counted since.
export async function playCountOf(env, sim) {
  return (sim?.play_count || 0) + (await visitsOf(env, sim?.id));
}

/// Count a visit to `sim` by `who` (a key from viewerKey, or `g:<id>` / `u:<id>`), once per UTC day. Returns whether it was
/// counted. Never throws, and never writes the listing record.
export async function countVisit(env, sim, who, now = Date.now()) {
  try {
    if (!env?.SOCIAL || !sim?.id || !who) return false;
    const day = dayOf(now);
    const marker = `visit:${sim.id}:${day}:${who}`;
    if (await env.SOCIAL.get(marker)) return false;
    // A flood of new anonymous markers for one listing is held to the limiter's allowance; people and guests are not limited here.
    if (who.startsWith('d:') && env.VISIT_RATE_LIMITER) {
      const { success } = await env.VISIT_RATE_LIMITER.limit({ key: sim.id });
      if (!success) return false;
    }
    await env.SOCIAL.put(marker, '1', { expirationTtl: KEY_TTL_SECONDS });
    await env.SOCIAL.put(`visits:${sim.id}`, String((await visitsOf(env, sim.id)) + 1));
    if (sim.author_id) {
      const plays = parseInt((await env.SOCIAL.get(`totalPlays:${sim.author_id}`)) || '0', 10) + 1;
      await env.SOCIAL.put(`totalPlays:${sim.author_id}`, String(plays));
    }
    return true;
  } catch (e) {
    console.error('countVisit failed:', e?.message || e);
    return false;
  }
}
