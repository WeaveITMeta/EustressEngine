// =============================================================================
// Listing text a Studio may still hold after the listing changed elsewhere
// =============================================================================
//
// Studio sends the listing's title, description and tags with every commit, from its own copy (publish.toml). When the
// listing's text changes somewhere else, that copy is out of date until Studio reads the change:
//
//   - the review's automatic fix (moderation.mjs applyAutoFix) rewrites the description and tags;
//   - the link review (link_review.mjs) takes a link out of the description;
//   - the author edits the title or description on the website (listing_edit.mjs).
//
// Each writer records `sim.listing_fix`: `before` (the text it replaced, by field) and `superseded` (every text replaced
// before that, by field, kept by supersededAfter). A commit that sends one of those texts back, while the listing holds
// another, comes from a Studio that has not caught up: world.mjs applyListing keeps the listing's text for that field
// (staleText, staleTags). Any other text the author sends is their own choice. Once a commit sends nothing stale, the
// guard ends.
//
// So an author who wants an old text back sets it on the website, where it is the listing's text, not a stale copy.
// =============================================================================

import { sameTagSet } from './tags.mjs';

/// How many replaced texts a field keeps.
export const SUPERSEDED_MAX = 8;

const text = (s) => String(s ?? '').trim();
const strings = (list) => (Array.isArray(list) ? list.filter((s) => typeof s === 'string') : []);
const tagSets = (list) => (Array.isArray(list) ? list.filter((t) => Array.isArray(t) && t.every((x) => typeof x === 'string')) : []);
const guardOf = (sim) => (sim?.listing_fix && typeof sim.listing_fix === 'object' ? sim.listing_fix : null);

/// The texts a guard knows were replaced, for one field.
function replaced(fix, key) {
  if (!fix) return [];
  const before = fix.before && typeof fix.before === 'object' ? fix.before[key] : undefined;
  if (key === 'tags') return [...tagSets(fix.superseded?.tags), ...(Array.isArray(before) ? [before] : [])];
  return [...strings(fix.superseded?.[key]), ...(typeof before === 'string' ? [before] : [])];
}

/// The `superseded` a new guard carries: everything the guard it replaces knew was replaced (its `superseded` and its
/// `before`), less what the listing holds now (`current`: `{ name, description, tags }`), newest last, at most
/// SUPERSEDED_MAX a field. The new guard's own `before` is not in it; the reader checks `before` too.
export function supersededAfter(prev, current) {
  const out = {};
  for (const key of ['name', 'description']) {
    const seen = [];
    for (const s of replaced(prev, key).map(text)) {
      if (s === text(current?.[key])) continue;
      const at = seen.indexOf(s);
      if (at !== -1) seen.splice(at, 1);
      seen.push(s);
    }
    out[key] = seen.slice(-SUPERSEDED_MAX);
  }
  const sets = [];
  for (const t of replaced(prev, 'tags')) {
    if (Array.isArray(current?.tags) && sameTagSet(t, current.tags)) continue;
    const at = sets.findIndex((s) => sameTagSet(s, t));
    if (at !== -1) sets.splice(at, 1);
    sets.push([...t]);
  }
  out.tags = sets.slice(-SUPERSEDED_MAX);
  return out;
}

/// Records a write that replaced listing text: `old` holds the replaced value of each field the writer changed (`name`,
/// `description`, `tags`), `after` the new ones, `by` who wrote it. The guard it replaces is folded into `superseded`.
export function guardReplacedText(sim, { old, after = null, by, at }) {
  const prev = guardOf(sim);
  const before = { ...(prev?.before && typeof prev.before === 'object' ? prev.before : {}), ...old };
  const current = { name: sim.name, description: sim.description, tags: Array.isArray(sim.tags) ? sim.tags : [] };
  // A field that holds its `before` text again is no longer guarded by it.
  for (const key of Object.keys(before)) {
    if (key === 'tags' ? Array.isArray(before.tags) && sameTagSet(before.tags, current.tags) : text(before[key]) === text(current[key])) delete before[key];
  }
  sim.listing_fix = { at, by, before, ...(after ? { after } : {}), superseded: supersededAfter(prev, current) };
}

/// Whether text a commit sent for `key` (`name` or `description`) is one the listing replaced, while it holds another.
export function staleText(sim, key, sent) {
  const fix = guardOf(sim);
  if (!fix || typeof sent !== 'string') return false;
  const s = text(sent);
  if (s === text(sim[key])) return false;
  return replaced(fix, key).some((t) => text(t) === s);
}

/// Whether tags a commit sent (already checked) are a set the listing replaced, while it holds another.
export function staleTags(sim, sent, current) {
  const fix = guardOf(sim);
  if (!fix || !Array.isArray(sent) || sameTagSet(sent, current)) return false;
  return replaced(fix, 'tags').some((t) => sameTagSet(t, sent));
}
