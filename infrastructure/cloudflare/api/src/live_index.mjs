// =============================================================================
// The live index: every session hosted right now, in one place
// =============================================================================
//
// LiveHost (src/live.mjs) holds one simulation's host. This object holds the whole picture for the
// Gallery's "live now" strip: a heartbeat tells it { sim_id, author, players, max_players, region, since },
// a host's stop removes the entry, and an entry that is not renewed expires 90 seconds after its
// last beat by this object's own alarm, so a host that vanishes disappears by itself.
//
//     GET /api/simulations/live     public, cached for 10 seconds per rating slice; the viewer's Adult content
//                                   setting picks the slice (src/adult_content.mjs)
//       -> { sessions: [{ id, name, author_name, thumbnail_url, cover_url, tags, tags_source, genre,
//                         players, max_players, region, since }], players_total, sessions_total }
//
// What keeps the index small and honest:
//   - only a LISTED listing's heartbeat is told to it (live.mjs), so an author's unlisted listings never fill it;
//   - an author has at most LIVE_INDEX_PER_AUTHOR entries: a sixth drops the oldest;
//   - `players` is what the host claims, capped at the players the Worker actually admitted (live.mjs);
//   - the region is the host's COUNTRY only: a data centre names a city, and a home host would be published to its metro.
// The index never holds a join link, a key or a pin. A listing is shown only while it is LISTED (the same
// overlay the list route uses), so an unlisted, archived, pending or held listing never appears, and the totals count the
// same listings the strip may show. The author's id is held to apply the cap and is never returned.
// One instance, named by LIVE_INDEX_NAME. Design: docs/networking/LIVE_INDEX.md.

import { RATINGS, legalLockedIds, withLegalLockFrom } from './moderation.mjs';
import { cachedValue, defaultCache } from './body_cache.mjs';
import { ratingCap, ratingOf, viewerHeaders } from './adult_content.mjs';

export const LIVE_INDEX_NAME = 'all';
/// An entry with no beat for this long is gone. The same three missed 30 s beats as HEARTBEAT_TTL_MS in live.mjs
/// (a test holds the two together).
export const LIVE_INDEX_TTL_MS = 90_000;
/// The most sessions the strip names.
export const LIVE_SESSIONS_SHOWN = 24;
/// The most entries one read looks at: a listing is read for each, so this bounds a request.
export const LIVE_INDEX_READ_LIMIT = 500;
/// The most entries one author holds. A further session drops the author's oldest.
export const LIVE_INDEX_PER_AUTHOR = 5;
/// A failing index may not hold a heartbeat longer than this.
export const LIVE_INDEX_CALL_TIMEOUT_MS = 1500;
/// How long the computed strip is kept: a hammered URL costs one scan per window.
export const LIVE_INDEX_CACHE_SECONDS = 10;

const ENTRY_PREFIX = 's:';
const AUTHOR_PREFIX = 'a:';
const SIM_ID = /^[a-f0-9-]{1,64}$/;
const AUTHOR_ID = /^[A-Za-z0-9_.:-]{1,128}$/;

// -----------------------------------------------------------------------------
// Region: country only, never an address and never a data centre
// -----------------------------------------------------------------------------

/// The country a request came from, as the platform names it: two upper-case letters, or nothing. `XX` is the platform's
/// word for "unknown" and is no country. The data centre that answered (`colo`) is never kept: it names a city.
/// Returns { country } or null.
export function cleanRegion(country) {
  return typeof country === 'string' && /^[A-Z]{2}$/.test(country) && country !== 'XX' ? { country } : null;
}

/// The region of a request: what Cloudflare says about where it came in (`request.cf`), before validation.
export function regionOf(request) {
  return { country: request?.cf?.country };
}

/// A stored region as the public sees it: the country and nothing else, whatever a record carries.
export const publicRegion = (region) => cleanRegion(region?.country);

// -----------------------------------------------------------------------------
// The Durable Object
// -----------------------------------------------------------------------------

const count = (value, min, max) => (Number.isSafeInteger(value) && value >= min && value <= max ? value : null);
const expired = (entry, now) => now - entry.last_seen >= LIVE_INDEX_TTL_MS;

const OPS = {
  async beat({ sim_id, author, players, max_players, region, since, now }) {
    const refuse = (error) => ({ ok: false, status: 400, error });
    if (typeof sim_id !== 'string' || !SIM_ID.test(sim_id)) return refuse('sim_id must be a listing id');
    if (typeof author !== 'string' || !AUTHOR_ID.test(author)) return refuse('author must be an account id');
    const p = count(players, 0, 10_000);
    const m = count(max_players, 1, 10_000);
    if (p === null || m === null) return refuse('players and max_players must be whole numbers');
    if (!Number.isFinite(since) || !Number.isFinite(now)) return refuse('since and now must be times');

    // The author's other live entries, oldest first. Room is made for this one by dropping the oldest beyond the cap.
    const others = [];
    for (const id of (await this.storage.get(`${AUTHOR_PREFIX}${author}`)) || []) {
      if (id === sim_id) continue;
      const entry = await this.storage.get(`${ENTRY_PREFIX}${id}`);
      if (entry && entry.author_id === author && !expired(entry, now)) others.push(entry);
    }
    others.sort((a, b) => a.since - b.since || (a.sim_id < b.sim_id ? -1 : 1));
    while (others.length > LIVE_INDEX_PER_AUTHOR - 1) await this.storage.delete(`${ENTRY_PREFIX}${others.shift().sim_id}`);

    await this.storage.put({
      [`${ENTRY_PREFIX}${sim_id}`]: { sim_id, author_id: author, players: p, max_players: m, region: cleanRegion(region?.country), since, last_seen: now },
      [`${AUTHOR_PREFIX}${author}`]: [...others.map((e) => e.sim_id), sim_id],
    });
    // The alarm waits for the earliest expiry; it moves the deadline on when it finds an entry renewed.
    if ((await this.storage.getAlarm()) == null) await this.storage.setAlarm(now + LIVE_INDEX_TTL_MS);
    return { ok: true };
  },

  async stop({ sim_id }) {
    if (typeof sim_id !== 'string') return { ok: true };
    const entry = await this.storage.get(`${ENTRY_PREFIX}${sim_id}`);
    await this.storage.delete(`${ENTRY_PREFIX}${sim_id}`);
    if (entry?.author_id) await this.forget(entry.author_id, sim_id);
    return { ok: true };
  },

  /// The live entries, newest first, without the author they are capped by. An entry past its expiry is not live even
  /// before the alarm has cleared it.
  async list({ now }) {
    const sessions = [];
    for (const { author_id: _author, ...entry } of (await this.storage.list({ prefix: ENTRY_PREFIX })).values()) {
      if (now - entry.last_seen >= LIVE_INDEX_TTL_MS) continue;
      sessions.push(entry);
    }
    sessions.sort((a, b) => b.since - a.since || (a.sim_id < b.sim_id ? -1 : 1));
    return { ok: true, sessions: sessions.slice(0, LIVE_INDEX_READ_LIMIT) };
  },
};

/// Every session being hosted. Named LIVE_INDEX_NAME.
export class LiveIndex {
  constructor(ctx, env) {
    this.ctx = ctx;
    this.env = env;
    this.storage = ctx.storage;
  }

  /// Take one session off an author's list, and the list itself when it empties.
  async forget(author, simId) {
    const key = `${AUTHOR_PREFIX}${author}`;
    const rest = ((await this.storage.get(key)) || []).filter((id) => id !== simId);
    if (rest.length) await this.storage.put(key, rest);
    else await this.storage.delete(key);
  }

  async fetch(request) {
    const op = new URL(request.url).pathname.slice(1);
    let args = {};
    try {
      const v = await request.json();
      if (v && typeof v === 'object' && !Array.isArray(v)) args = v;
    } catch {
      args = {};
    }
    let result;
    try {
      const handler = OPS[op];
      result = handler ? await handler.call(this, args) : { ok: false, status: 404, error: `unknown operation ${op}` };
    } catch (e) {
      console.error(`LiveIndex.${op} failed:`, e);
      result = { ok: false, status: 500, error: String(e?.message || e) };
    }
    const status = result.ok === false ? result.status || 500 : 200;
    return new Response(JSON.stringify(result), { status, headers: { 'content-type': 'application/json' } });
  }

  /// Clear what has expired, and wake again for the next expiry. A beat that landed since the alarm was set has
  /// moved its entry's deadline, so each entry is checked against its own last beat.
  async alarm() {
    const now = Date.now();
    let next = null;
    for (const [key, entry] of await this.storage.list({ prefix: ENTRY_PREFIX })) {
      const due = entry.last_seen + LIVE_INDEX_TTL_MS;
      if (now >= due) {
        await this.storage.delete(key);
        if (entry.author_id) await this.forget(entry.author_id, entry.sim_id);
      } else if (next === null || due < next) next = due;
    }
    if (next !== null) await this.storage.setAlarm(next);
  }
}

// -----------------------------------------------------------------------------
// Calls into it
// -----------------------------------------------------------------------------

async function indexCall(env, op, args) {
  const stub = env.LIVE_INDEX.get(env.LIVE_INDEX.idFromName(LIVE_INDEX_NAME));
  let timer;
  const timeout = new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(`${op}: timed out`)), LIVE_INDEX_CALL_TIMEOUT_MS); });
  try {
    const res = await Promise.race([
      stub.fetch(`https://live-index.internal/${op}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args) }),
      timeout,
    ]);
    return await res.json();
  } finally {
    clearTimeout(timer);
  }
}

/// Tell the index about a host: op `beat` or `stop`. Never throws and never waits long: an index that is missing,
/// failing or slow costs the host nothing, since the heartbeat is what matters. Returns whether the index took it.
export async function tellLiveIndex(env, op, args) {
  try {
    if (!env?.LIVE_INDEX) return false;
    return (await indexCall(env, op, args)).ok !== false;
  } catch (e) {
    console.error(`live index ${op} failed:`, e?.message || e);
    return false;
  }
}

// -----------------------------------------------------------------------------
// GET /api/simulations/live
// -----------------------------------------------------------------------------

/// The strip for one age slice: the live entries that are LISTED, newest first, with the totals of all of them.
async function liveNowFor(env, maxIdx, isListable, view) {
  const held = await indexCall(env, 'list', { now: Date.now() });
  if (!held || held.ok === false || !Array.isArray(held.sessions)) throw new Error('the live index did not answer');

  const locked = await legalLockedIds(env);
  const shown = [];
  let playersTotal = 0;
  let sessionsTotal = 0;
  for (const entry of held.sessions) {
    const raw = await env.SOCIAL.get(`sim:${entry.sim_id}`);
    if (!raw) continue;
    let sim;
    try { sim = withLegalLockFrom(JSON.parse(raw), locked); } catch { continue; }
    if (!isListable(sim)) continue;
    // No rating, or one this Worker does not know, reads as adult_21, the most restrictive band (ratingOf).
    if (RATINGS.indexOf(ratingOf(sim)) > maxIdx) continue;
    sessionsTotal += 1;
    playersTotal += entry.players;
    if (shown.length >= LIVE_SESSIONS_SHOWN) continue;
    const card = await view(sim);
    shown.push({
      id: sim.id,
      name: card.name,
      author_name: card.author_name,
      thumbnail_url: card.thumbnail_url,
      cover_url: card.cover_url,
      // The listing's categories, as every listing a route returns carries them (src/tags.mjs listingTagFields).
      tags: card.tags,
      tags_source: card.tags_source,
      genre: card.genre,
      players: entry.players,
      max_players: entry.max_players,
      region: publicRegion(entry.region),
      since: new Date(entry.since).toISOString(),
    });
  }
  return { sessions: shown, players_total: playersTotal, sessions_total: sessionsTotal };
}

/// `deps` carries json and isListable (index.js), `view(sim)`, the Gallery's card for a listing (its name,
/// author, thumbnail and cover; awaited, so it may sign an adult card's image URLs), and `viewer`, who asked (src/adult_content.mjs viewerOf; signed out when absent).
/// `deps.cache` replaces the platform's cache (a test hands in its own).
export async function handleLiveNow(url, env, cors, deps) {
  const { json, isListable, view, viewer } = deps;
  const unavailable = () => json({ error: 'The live list is not available right now', code: 'live_unavailable' }, 503, { ...cors, 'Cache-Control': 'no-store' });
  if (!env.LIVE_INDEX) return unavailable();

  // The viewer's age slice, as the list route reads it: `rating_max`, never past mature_17 while the viewer's Adult content
  // is off, and never past adult_18 while it is on and the verified age is under 21 (ratingCap). It is worked out from the
  // viewer BEFORE the cache is read, so a strip computed for one viewer's slice is never handed to a viewer with a lower
  // one. The cache holds one strip per slice, keyed by nothing else.
  const slice = ratingCap(url?.searchParams?.get('rating_max'), viewer);
  const maxIdx = RATINGS.indexOf(slice);
  const cache = deps.cache === undefined ? defaultCache() : deps.cache;
  let body;
  try {
    body = await cachedValue(cache, `${url?.origin || 'https://api.eustress.dev'}/__cache/live?rating_max=${slice}`, LIVE_INDEX_CACHE_SECONDS, () => liveNowFor(env, maxIdx, isListable, view));
  } catch (e) {
    console.error('live index list failed:', e?.message || e);
    return unavailable();
  }
  return json(body, 200, viewerHeaders(cors, viewer, `public, max-age=${LIVE_INDEX_CACHE_SECONDS}`));
}
