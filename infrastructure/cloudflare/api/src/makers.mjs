// =============================================================================
// Makers: the people with a listed listing, for the Gallery's search
// =============================================================================
//
//     GET /api/makers?q=&limit=     public
//       -> { makers: [{ username, avatar_url, space_count, visits }] }
//
// A maker is an author with at least one LISTED listing. The list is computed from the same scan
// the Gallery's list route does (its views-only read), so a listing counts here exactly when the
// Gallery serves it: unlisted, archived, pending and quarantined listings count for nobody.
// Nothing about an account is shown beyond what a listing's author_name already shows publicly:
// no id, no email.
//
// Order: most listed Spaces first, then most visits, then username. `q` is a case-insensitive
// substring of the username (one leading @ ignored), compared as normalized text; text that
// matches nobody is an empty list, never an error. `limit` is 8 by default and at most 24.
//
// The scan is the cost, and a Worker response is not cached by `Cache-Control`. So the scan's RESULT, every maker for one age
// slice (the author's name, avatar and counts, with no id), is kept for 10 seconds (src/body_cache.mjs); `q` and `limit` are
// applied to that, so no query string can dodge it. A hammered URL costs one scan per window.

import { playCountOf } from './visits.mjs';
import { cachedValue, defaultCache } from './body_cache.mjs';
import { ratingCap, viewerHeaders } from './adult_content.mjs';
import { cleanAvatarUrl } from './account_profile.mjs';

export const MAKERS_DEFAULT_LIMIT = 8;
export const MAKERS_MAX_LIMIT = 24;
/// How long the scan's result is kept.
export const MAKERS_CACHE_SECONDS = 10;
/// Longer text than any username matches nobody, so it is cut rather than compared.
const MAX_QUERY_LENGTH = 64;

/// Text as it is compared: Unicode NFKC, lower-cased, trimmed.
const normalized = (text) => String(text ?? '').normalize('NFKC').toLowerCase().trim();

/// What was asked for: the substring (one leading @ dropped) and the count.
export function makersQuery(params) {
  let q = normalized(params?.get('q') || '').slice(0, MAX_QUERY_LENGTH);
  if (q.startsWith('@')) q = q.slice(1).trim();
  const asked = parseInt(params?.get('limit') ?? '', 10);
  const limit = Number.isFinite(asked) ? Math.min(MAKERS_MAX_LIMIT, Math.max(1, asked)) : MAKERS_DEFAULT_LIMIT;
  return { q, limit };
}

/// An avatar the Gallery may put in an image tag: an https address, or nothing.
// The website's own rule (account_profile.mjs cleanAvatarUrl): a path under /assets, never an address on any host.
const avatarOf = (user) => cleanAvatarUrl(user?.avatar_url ?? '');

function compareNames(a, b) {
  const x = normalized(a);
  const y = normalized(b);
  if (x !== y) return x < y ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

/// Every maker of one age slice, ranked: { username, avatar_url, space_count, visits }. Reads the listed listings once,
/// their visit counts, and each author's avatar. No id leaves this function.
async function scanMakers(env, scope, slice, deps) {
  const views = await deps.listViews(scope, slice);

  // One entry per author. The name is the one on the author's newest listed listing.
  const authors = new Map();
  for (const card of views) {
    const name = typeof card.author_name === 'string' ? card.author_name.trim() : '';
    const key = card.author_id || (name ? `name:${name}` : '');
    if (!name || !key) continue;
    let maker = authors.get(key);
    if (!maker) authors.set(key, (maker = { id: card.author_id || null, username: name, space_count: 0, visits: 0 }));
    maker.space_count += 1;
    maker.visits += await playCountOf(env, { id: card.id, play_count: card.play_count });
  }

  const ranked = [...authors.values()].sort((a, b) => b.space_count - a.space_count || b.visits - a.visits || compareNames(a.username, b.username));
  const makers = [];
  for (const m of ranked) {
    let user = null;
    try {
      const raw = m.id ? await env.USERS.get(`user:${m.id}`) : null;
      user = raw ? JSON.parse(raw) : null;
    } catch { user = null; }
    makers.push({ username: m.username, avatar_url: avatarOf(user), space_count: m.space_count, visits: m.visits });
  }
  return makers;
}

/// `deps.listViews(scope, slice)` is the list route's views-only read (the cards of every listed listing up to the rating
/// `slice`, newest first); `deps.json` is index.js's json; `deps.viewer` is who asked (src/adult_content.mjs viewerOf; signed
/// out when absent); `deps.cache` replaces the platform's cache (a test hands in its own).
export async function handleMakers(url, env, cors, deps) {
  const { q, limit } = makersQuery(url?.searchParams);
  // One cached scan per age slice, and the slice is one of a fixed list, so nothing a client sends can add a cache entry.
  // The slice is `rating_max`, never past mature_17 while the viewer's Adult content is off and never past adult_18 while
  // it is on and the verified age is under 21 (ratingCap), and it is worked out from the viewer BEFORE the cache is read:
  // a scan made for one viewer's slice is never handed to a viewer with a lower one.
  const slice = ratingCap(url?.searchParams?.get('rating_max'), deps.viewer);
  const scope = new URL('https://api.eustress.dev/api/simulations');
  scope.searchParams.set('rating_max', slice);
  const cache = deps.cache === undefined ? defaultCache() : deps.cache;
  const everyone = await cachedValue(cache, `${url?.origin || 'https://api.eustress.dev'}/__cache/makers?rating_max=${slice}`, MAKERS_CACHE_SECONDS, () => scanMakers(env, scope, slice, deps));

  const makers = everyone.filter((m) => !q || normalized(m.username).includes(q)).slice(0, limit);
  return deps.json({ makers }, 200, viewerHeaders(cors, deps.viewer, 'public, max-age=300'));
}
