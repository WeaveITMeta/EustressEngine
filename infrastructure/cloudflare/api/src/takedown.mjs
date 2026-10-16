// =============================================================================
// TAKE IT DOWN Act: notice and removal of intimate images shared without consent
// =============================================================================
//
// The TAKE IT DOWN Act (signed May 19, 2025; its platform duties are enforced by
// the FTC from May 19, 2026) requires a platform that hosts what its users make
// to run a process through which a person, or someone authorized to act for
// them, asks for a nonconsensual intimate visual depiction of them (real or
// made by a computer) to be removed, and to remove it within 48 hours of a valid
// request, with reasonable efforts to remove known identical copies. A valid
// request is written, signed (a physical or electronic signature), identifies
// the depiction well enough to find it, says briefly and in good faith that it
// is not consensual, and gives a way to reach the person. Removal in good faith
// is protected. Eustress's Gallery is such a platform.
//
//   POST /api/takedown/requests                  anyone; limited per address and per contact email
//   GET  /api/admin/takedowns?status=            an administrator: the queue
//   GET  /api/admin/takedowns/{id}               an administrator: one case, with the request
//   POST /api/admin/takedowns/{id}/confirm       a person upholds the removal
//   POST /api/admin/takedowns/{id}/invalid       a person finds the request invalid: the content is restored
//   POST /api/admin/takedowns/{id}/close         a person closes a request that named nothing on Eustress
//
// The account's case history, the status of a case by its reference, and the
// request for a person's review are in src/case_history.mjs.
//
// What a valid request does, at once, in this code, with no person in the loop:
//
//   1. Every listing it names is locked with the legal lock moderation keeps
//      (R2 `locks/legal/{id}`, kind serve_block). Every route that sends a
//      listing reads through that lock (simWithLegalLock, simForServing,
//      legalLockedIds), so from that write on the listing is not listed,
//      opened, played, downloaded or shown: its page, thumbnail, cover,
//      previews, package, world manifest, chunks, source world and website
//      manifest are all refused to everyone but an administrator. A link to
//      one of those objects is resolved to its listing. Nothing is deleted:
//      the bytes are kept as they are, as evidence.
//   2. The listing's moderation case carries a legal hold naming the request
//      (`legal_hold.source: 'takedown'`), so moderation agrees with the lock: a
//      review cannot drop it, an upload into the listing is refused, and a
//      person lifts it only through this module's review.
//   3. The SHA-256 of every stored file of the listing (the package or the
//      world manifests, each Space package, the thumbnail, the cover, every
//      review capture) goes on a blocklist, `ncii-hash:{sha256}`. Every upload
//      path refuses bytes on it (refuseBlockedUpload), and a bounded scan of the
//      current listings locks the ones that hold the same bytes (scanForCopies).
//      Exact hashes only: two files match when every byte matches. Perceptual
//      matching (the same picture re-encoded, resized or cropped) is NOT done.
//   4. The author of each listing is told, with the case reference and how to
//      ask a person to review it, and never who asked. The requester gets a
//      receipt with the reference. Staff are emailed.
//
// No silent failure. Each of those is a named step (`lock:{sim}`, `case:{sim}`,
// `hashes:{sim}`, `notice:{sim}`, `receipt`, `staff`, ...). A step that fails
// stays on the case with its error, the case is indexed under `tdpending:`, and
// the hourly cron (sweepTakedowns, from scheduled() in index.js) runs it again
// until it succeeds. Staff are emailed while a removal step is owed, and the
// night's cron record lists the cases still owing one.
//
// Content involving a minor (the request says the person depicted is under 18)
// is locked the same way and flagged `possible_csam` for the staff-only path:
// the legal hold is escalated and preserved with no end date (never deleted),
// its author is NOT told (the moderation rule: a suspected uploader learns
// nothing before a person decides whether to report), and staff are emailed at
// once. Its files are sealed into the evidence vault (`seal:{sim}`,
// src/evidence_vault.mjs). A request is a person's statement, not a confident
// detection, so nothing here writes a report to NCMEC; a listing the
// classifiers also found is reported from its moderation case
// (src/legal_reports.mjs).
//
// Restoration never happens on its own. Only a person's `invalid` decision lifts
// a takedown's lock and takes its hashes off the blocklist.
//
// A request may also name a post on the Eustress Forums (parseLocation: a thread's
// page with a #post- fragment, on eustress.dev or the staging site, the API's post or
// thread path, or a bare post id; a thread alone names its first post). It is removed
// like a listing, through a lock the forum routes treat as removed: SOCIAL
// `forum-post-lock:{postId}`, `{ reason: 'takedown', reference, locked_at }`. This
// module writes and deletes only that key and never touches forum storage, so it
// cannot see whether the post exists or who wrote it: the post's author is told by
// the forum ("Removed under a legal request"), not here. The hash blocklist does not
// apply to a text post (there is no file to hash). A person's `invalid` decision
// deletes the lock, and the hourly sweep writes it again if it disappears while the
// takedown is in force.
//
// Storage (SOCIAL unless said):
//   tdcase:{id}                the case, with the request (the requester's name, signature, email: administrators only)
//   tdq:{status}:{id}          the queue by status
//   tdpending:{id}             a case that still owes a step
//   tdactive:{id}              a case whose removal is in force (not invalid or closed)
//   tdby:{account}:{id}        filed by a signed-in account
//   tdagainst:{author}:{id}    a takedown of the author's listing (not a possible_csam one)
//   tdlisting:{sim}:{id}       a takedown of the listing
//   tdforum:{post}:{id}        a takedown of the forum post
//   forum-post-lock:{post}     the forum post's lock: { reason: 'takedown', reference, locked_at }
//   ncii-hash:{sha256}         the blocklist: { cases, kinds, size, first_at, possible_csam? }
//   ncii-attempt:{at}:{rand}   an upload refused for a blocked hash (evidence; kept)
//   ncii-scan:state            where the identical-copy scan is
//   ncii-enforce:cursor        where the sweep's check of the locks in force is
//   tdmail:{id}:{to}:{event}   an email of a case that was sent (sent once)
//   caserefs/{kind}/{id}       R2: the case's reference, claimed once (src/case_reference.mjs)
//   USERS takedown-cap:{ip|email}:{hash}:{day}   the daily allowance of requests
// =============================================================================

import {
  loadCase, saveCase, caseKey, legalLockKind, legalLockedIds, acquireLease, releaseLease, thresholdsFrom,
  notifPrefix, NOTIFY_TTL_SECONDS, CASE_STATUSES, MODERATION_VERSION, POLICY_VERSION, RATINGS, evidencePrefix, CAPTURE_MAX_BYTES,
  FORUM_POST_ID, FORUM_THREAD_ID, forumThreadOf, forumPostLockKey,
} from './moderation.mjs';
import { assignCaseReference } from './case_reference.mjs';
import { sendNotice, scrubAddresses } from './account_notices.mjs';
import { renderEmail, html, paragraph, note as noteLine, facts, formatWhen } from './email_shell.mjs';
import { readAccountEmail } from './email_ops.mjs';
import { accountTimeZone } from './account_time_zone.mjs';
import { sha256Hex, prepareUserImage } from './user_images.mjs';
import { defaultCache } from './body_cache.mjs';
import { siteOrigin } from './cors.mjs';
import { sealEvidence, listingEvidenceItems } from './evidence_vault.mjs';

export const TAKEDOWN_LAW = 'TAKE IT DOWN Act';
/// The law's bound on removal after a valid request. Removal here takes seconds; this is what a late one is measured against.
export const REMOVAL_DEADLINE_HOURS = 48;
export const MAX_LOCATIONS = 20;
export const MAX_LOCATION_CHARS = 2048;
export const NAME_MIN = 2;
export const NAME_MAX = 200;
export const DESCRIPTION_MIN = 10;
export const DESCRIPTION_MAX = 2000;
export const STATEMENT_MIN = 10;
export const STATEMENT_MAX = 2000;
/// Valid requests a day from one network address, and naming one contact email. A request past either is not filed, and the answer
/// says how to reach a person instead, so a real request is never stuck behind the limit.
export const REQUESTS_PER_ADDRESS_PER_DAY = 10;
export const REQUESTS_PER_EMAIL_PER_DAY = 5;
/// Identical copies of one file the scan locks on its own. A file shared by more listings than this is more likely a stock image
/// or an unchanged template than the depiction, so further matches are recorded for a person and not locked.
export const COPIES_LOCKED_PER_HASH_MAX = 5;
/// The most bytes hashed in memory where the platform has no streaming digest (tests, a local run). Workers stream any size.
export const HASH_IN_MEMORY_MAX = 64 * 1024 * 1024;
export const RELATIONSHIPS = ['self', 'authorized_representative'];
export const TAKEDOWN_STATUSES = ['received', 'removed', 'under_review', 'confirmed', 'invalid', 'closed'];
export const BLOCKED_UPLOAD_MESSAGE = 'This file cannot be published on Eustress. If you think this is a mistake, write to support@eustress.dev.';

const SUPPORT = 'support@eustress.dev';
const FROM_NAME = 'Eustress Trust and Safety';
const NCII_CATEGORY = 'ncii_or_real_person_sexual';
const CSAM_CATEGORY = 'csam_or_minor_sexualization';
const PRESERVE_DAYS = 366;
const DECIDED = ['confirmed', 'invalid', 'closed'];

const tdKey = (id) => `tdcase:${id}`;
const queueKey = (status, id) => `tdq:${status}:${id}`;
const pendingKey = (id) => `tdpending:${id}`;
const activeKey = (id) => `tdactive:${id}`;
const filedByKey = (accountId, id) => `tdby:${accountId}:${id}`;
const againstKey = (authorId, id) => `tdagainst:${authorId}:${id}`;
const onListingKey = (simId, id) => `tdlisting:${simId}:${id}`;
const onForumPostKey = (postId, id) => `tdforum:${postId}:${id}`;
/// The staging site, whose forum links name a post as eustress.dev's do. Accepted for forum locations only.
export const FORUM_SITE_HOSTS = new Set(['staging.eustress-cbb.pages.dev']);
export const blockKey = (sha) => `ncii-hash:${sha}`;
const SCAN_KEY = 'ncii-scan:state';
const ENFORCE_CURSOR_KEY = 'ncii-enforce:cursor';
// The legal lock moderation.mjs keeps (its legalLockKey, not exported): strongly consistent, and read by every route that serves a listing.
const legalLockKey = (simId) => `locks/legal/${simId}`;
const JSON_META = { httpMetadata: { contentType: 'application/json' } };

const EMAIL = /^[^\s@<>,;"']{1,64}@[^\s@<>,;"']{1,190}\.[^\s@<>,;"']{2,}$/;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const UUID_IN = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i;
const NAMESPACE = /^[a-z0-9][a-z0-9_-]{0,63}$/;
const isSha = (s) => typeof s === 'string' && /^[0-9a-f]{64}$/.test(s);
const hexOf = (buf) => [...new Uint8Array(buf)].map((x) => x.toString(16).padStart(2, '0')).join('');
const nowIso = () => new Date().toISOString();
const clean = (s) => (typeof s === 'string' ? s.normalize('NFKC').replace(/\s+/g, ' ').trim() : '');

// -----------------------------------------------------------------------------
// The request
// -----------------------------------------------------------------------------

/// What a location names, from the text a requester gave: a listing id, or a link on eustress.dev (the site, the API, any
/// subdomain, or `hosts`, the hosts this deployment answers on), or a Player link. `{ listing_id, object }` or `{ namespace,
/// object }` (a website manifest named by its namespace), or `{ error }` saying what is wrong with it. A link to one object of
/// a listing (its thumbnail, cover, a preview, a chunk) names that listing; `object` records which part was pointed at.
///
/// A forum post is `{ forum_post_id, thread_id, object: 'forum_post' }`, named by:
///   - its thread's page with a fragment, /community/t/{thread_id}#post-{post_id}, on eustress.dev, a subdomain, `hosts`, or the
///     staging site (FORUM_SITE_HOSTS);
///   - the API's /api/forums/posts/{post_id};
///   - a thread alone, /community/t/{thread_id} or /api/forums/threads/{thread_id}: its first post, {thread_id}-1;
///   - a bare post id.
/// A #post- fragment whose post id is not one, or is not in the URL's thread, names no post: `forum_post_id` is null and
/// `not_found` says why, so the location is recorded as not found, like a listing id that names nothing.
export function parseLocation(raw, hosts = new Set()) {
  if (typeof raw !== 'string' || !raw.trim()) return { error: 'is empty' };
  const text = raw.trim();
  if (text.length > MAX_LOCATION_CHARS) return { error: `is longer than ${MAX_LOCATION_CHARS} characters` };
  if (UUID.test(text)) return { listing_id: text.toLowerCase(), object: 'listing' };
  if (FORUM_POST_ID.test(text)) return { forum_post_id: text, thread_id: forumThreadOf(text), object: 'forum_post' };
  let u;
  try { u = new URL(text); } catch { return { error: 'is not a link to Eustress or a listing id' }; }
  if (u.protocol === 'eustress-player:') {
    const id = UUID_IN.exec(u.href)?.[0];
    return id ? { listing_id: id.toLowerCase(), object: 'listing' } : { error: 'does not name a listing' };
  }
  if (u.protocol !== 'https:' && u.protocol !== 'http:') return { error: 'is not a web link' };
  const host = u.hostname.toLowerCase();
  const known = host === 'eustress.dev' || host.endsWith('.eustress.dev') || hosts.has(host);
  const parts = u.pathname.split('/').filter(Boolean);
  const forum = forumLocation(parts, u.hash);
  if (forum && (known || FORUM_SITE_HOSTS.has(host))) return forum;
  if (!known) return { error: 'is not on Eustress (eustress.dev)' };
  // A website manifest: /api/simulation/{id or namespace}/manifest, or .../{namespace}/latest/manifest.
  if (parts[0] === 'api' && parts[1] === 'simulation' && parts[parts.length - 1] === 'manifest' && (parts.length === 4 || (parts.length === 5 && parts[3] === 'latest'))) {
    let segment;
    try { segment = decodeURIComponent(parts[2]).toLowerCase(); } catch { return { error: 'is not a link Eustress can read' }; }
    if (UUID.test(segment)) return { listing_id: segment, object: 'website_manifest' };
    return NAMESPACE.test(segment) ? { namespace: segment, object: 'website_manifest' } : { error: 'does not name a listing' };
  }
  // The API's own paths for a listing and each of its objects.
  if (parts[0] === 'api' && parts[1] === 'simulations' && UUID.test(parts[2] || '')) return { listing_id: parts[2].toLowerCase(), object: objectNamed(parts.slice(3)) };
  // The site's pages (/simulation/{id}, /play/{id}) and any other link that carries a listing id.
  const id = UUID_IN.exec(`${u.pathname} ${u.search}`)?.[0];
  return id ? { listing_id: id.toLowerCase(), object: 'listing' } : { error: 'does not name a listing on Eustress' };
}

/// The forum post a link's path and fragment name (see parseLocation), `{ error }` for a forum path with a malformed id, or
/// null for a path that is not a forum one.
function forumLocation(parts, hash) {
  const post = (id) => ({ forum_post_id: id, thread_id: forumThreadOf(id), object: 'forum_post' });
  const first = (thread) => (FORUM_THREAD_ID.test(thread) ? post(`${thread}-1`) : { error: 'does not name a forum thread' });
  if (parts[0] === 'community' && parts[1] === 't' && parts.length === 3) {
    const thread = parts[2].toLowerCase();
    if (!FORUM_THREAD_ID.test(thread)) return { error: 'does not name a forum thread' };
    if (!hash.startsWith('#post-')) return first(thread);
    let id;
    try { id = decodeURIComponent(hash.slice('#post-'.length)).toLowerCase(); } catch { id = ''; }
    if (FORUM_POST_ID.test(id) && forumThreadOf(id) === thread) return post(id);
    return { forum_post_id: null, thread_id: thread, object: 'forum_post', not_found: 'the post it names is not in that thread' };
  }
  if (parts[0] === 'api' && parts[1] === 'forums' && parts.length === 4) {
    const id = parts[3].toLowerCase();
    if (parts[2] === 'posts') return FORUM_POST_ID.test(id) ? post(id) : { error: 'does not name a forum post' };
    if (parts[2] === 'threads') return first(id);
  }
  return null;
}

function objectNamed(rest) {
  const r = rest.join('/');
  if (r === 'thumbnail' || r === 'cover' || r === 'previews') return r;
  if (r === 'download') return 'package';
  if (r === 'world/manifest') return 'world_manifest';
  if (r === 'world/source') return 'source_manifest';
  let m;
  if ((m = /^preview\/([0-7])$/.exec(r))) return `preview:${m[1]}`;
  if ((m = /^world\/chunks\/([0-9a-f]{64})$/.exec(r))) return `chunk:${m[1]}`;
  return 'listing';
}

/// Every problem with a request, each `{ field, message }` in words for the requester, and the request as it is kept. A request
/// with any problem is refused whole and nothing is removed.
export function checkTakedownRequest(body, hosts = new Set()) {
  const problems = [];
  const need = (field, message) => problems.push({ field, message });
  const b = body && typeof body === 'object' && !Array.isArray(body) ? body : {};

  const name = clean(b.requester_name);
  if (!name) need('requester_name', 'Your full name is required.');
  else if (name.length < NAME_MIN || name.length > NAME_MAX) need('requester_name', `Your full name must be ${NAME_MIN} to ${NAME_MAX} characters.`);

  // The electronic signature: the full name typed a second time, matching the first (case and spacing aside).
  const signature = clean(b.signature);
  if (!signature) need('signature', 'Your electronic signature is required: type your full name again.');
  else if (name && signature.toLowerCase() !== name.toLowerCase()) need('signature', 'The signature must be your full name, typed the same way as above.');

  const relationship = b.relationship;
  if (!RELATIONSHIPS.includes(relationship)) need('relationship', 'Say whether you are the person shown ("self") or someone authorized to act for them ("authorized_representative").');
  const depicted = clean(b.depicted_name);
  if (relationship === 'authorized_representative') {
    if (!depicted) need('depicted_name', 'The name of the person shown is required when you act for them.');
    else if (depicted.length < NAME_MIN || depicted.length > NAME_MAX) need('depicted_name', `The name of the person shown must be ${NAME_MIN} to ${NAME_MAX} characters.`);
  }

  const email = typeof b.contact_email === 'string' ? b.contact_email.trim() : '';
  if (!email) need('contact_email', 'An email address where we can reach you is required.');
  else if (email.length > 254 || !EMAIL.test(email)) need('contact_email', 'The email address is not valid.');

  const raw = typeof b.locations === 'string' ? [b.locations] : Array.isArray(b.locations) ? b.locations : [];
  const locations = [];
  if (!raw.length) need('locations', 'At least one location is required: a link to the content on Eustress, or its listing id.');
  else if (raw.length > MAX_LOCATIONS) need('locations', `At most ${MAX_LOCATIONS} locations fit in one request. Send the rest in another request.`);
  else {
    raw.forEach((item, i) => {
      const parsed = parseLocation(item, hosts);
      if (parsed.error) need(`locations[${i}]`, `Location ${i + 1} ${parsed.error}.`);
      else locations.push({ location: String(item).trim(), ...parsed });
    });
  }

  const description = clean(b.description);
  if (!description) need('description', 'A short description of the content is required, so it can be found.');
  else if (description.length < DESCRIPTION_MIN || description.length > DESCRIPTION_MAX) need('description', `The description must be ${DESCRIPTION_MIN} to ${DESCRIPTION_MAX} characters.`);

  if (b.good_faith !== true) need('good_faith', 'Confirm in good faith that the content is an intimate image or video of the person shown, shared without their consent.');
  if (b.depicts_minor !== undefined && b.depicts_minor !== null && typeof b.depicts_minor !== 'boolean') need('depicts_minor', 'depicts_minor must be true or false.');

  return {
    problems,
    value: {
      requester_name: name, signature, relationship, depicted_name: relationship === 'authorized_representative' ? depicted : null,
      contact_email: email, locations, description, good_faith: true, depicts_minor: b.depicts_minor === true,
    },
  };
}

/// The hosts this deployment answers on, besides eustress.dev and its subdomains.
function ownHosts(url, env) {
  const hosts = new Set();
  for (const origin of [url?.origin, env?.PUBLIC_API_ORIGIN, env?.PUBLIC_SITE_ORIGIN]) {
    try { if (origin) hosts.add(new URL(origin).hostname.toLowerCase()); } catch { /* not an origin */ }
  }
  return hosts;
}

const capKey = async (scope, value) => `takedown-cap:${scope}:${(await sha256Hex(new TextEncoder().encode(String(value).toLowerCase()))).slice(0, 32)}:${new Date().toISOString().slice(0, 10)}`;

/// One request's worth of the day's allowance for this address and this contact email, or false when either is used up. Read then
/// write: a burst can overrun it by a few, which the per-minute limiter in front of it bounds.
async function takeDailyAllowance(env, ip, email) {
  if (!env.USERS) return true;
  const counters = [[await capKey('email', email), REQUESTS_PER_EMAIL_PER_DAY]];
  if (ip) counters.push([await capKey('ip', ip), REQUESTS_PER_ADDRESS_PER_DAY]);
  const used = [];
  for (const [key, cap] of counters) {
    const n = parseInt((await env.USERS.get(key)) || '0', 10) || 0;
    if (n >= cap) return false;
    used.push([key, n]);
  }
  for (const [key, n] of used) await env.USERS.put(key, String(n + 1), { expirationTtl: 2 * 86400 });
  return true;
}

// -----------------------------------------------------------------------------
// The case
// -----------------------------------------------------------------------------

export async function loadTakedown(env, id) {
  if (typeof id !== 'string' || !UUID.test(id)) return null;
  const raw = await env.SOCIAL.get(tdKey(id));
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

async function saveTakedown(env, rec, previousStatus) {
  rec.updated_at = nowIso();
  await env.SOCIAL.put(tdKey(rec.id), JSON.stringify(rec));
  if (previousStatus && previousStatus !== rec.status) await env.SOCIAL.delete(queueKey(previousStatus, rec.id)).catch(() => {});
  await env.SOCIAL.put(queueKey(rec.status, rec.id), rec.updated_at);
  if (rec.pending.length) await env.SOCIAL.put(pendingKey(rec.id), rec.updated_at);
  else await env.SOCIAL.delete(pendingKey(rec.id)).catch(() => {});
  if (DECIDED.includes(rec.status) && rec.status !== 'confirmed') await env.SOCIAL.delete(activeKey(rec.id)).catch(() => {});
  else await env.SOCIAL.put(activeKey(rec.id), rec.id);
}

function record(rec, event, detail) {
  rec.history = (rec.history || []).slice(-100);
  rec.history.push({ at: nowIso(), event, ...(detail ? { detail } : {}) });
}

const targetOf = (rec, simId) => rec.targets.find((t) => t.sim_id === simId);
const readSim = async (env, simId) => {
  const raw = await env.SOCIAL.get(`sim:${simId}`);
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
};
const authorVisible = (rec) => !rec.possible_csam;

// -----------------------------------------------------------------------------
// Steps
// -----------------------------------------------------------------------------
// Each step can run again without harm: a lock already in force is not written twice, a feed item and an email are sent once
// (their keys say so), a hash already listed for the case is not added again.

const STEPS = {
  // The removal itself: the legal lock. A lock already serving as a block is left as it is; one that only froze content
  // (a hold in moderation's legal lane) is raised to a block.
  lock: async (env, rec, simId, ctx) => {
    const target = targetOf(rec, simId);
    if (!target || target.restored_at) return;
    const head = await env.SCENES.head(legalLockKey(simId));
    if (!head || head.customMetadata?.kind === 'content_lock') {
      await env.SCENES.put(legalLockKey(simId), JSON.stringify({ kind: 'serve_block', at: nowIso(), takedown: rec.id }), { ...JSON_META, customMetadata: { kind: 'serve_block' } });
      ctx.locksWritten = true;
    }
    target.locked_at = target.locked_at || nowIso();
    for (const l of rec.locations || []) if (l.listing_id === simId) l.result = 'removed';
    settleRemoval(rec);
    await env.SOCIAL.put(onListingKey(simId, rec.id), rec.id);
  },
  // A forum post's removal: the lock the forum routes treat as removed. Only the key is written; forum storage is the forum's.
  // There is no hashes step for a post: the NCII hash blocklist is for files, and a forum post is text.
  forum_lock: async (env, rec, postId) => {
    const target = forumTargetOf(rec, postId);
    if (!target || target.restored_at) return;
    // The post is marked locked only once the key is written, so a write that fails leaves the step owed and the removal unsettled.
    const at = target.locked_at || nowIso();
    if (!(await env.SOCIAL.get(forumPostLockKey(postId)))) {
      await env.SOCIAL.put(forumPostLockKey(postId), JSON.stringify({ reason: 'takedown', reference: rec.reference, locked_at: at }));
    }
    target.locked_at = at;
    for (const l of rec.locations || []) if (l.forum_post_id === postId) l.result = 'removed';
    settleRemoval(rec);
    await env.SOCIAL.put(onForumPostKey(postId, rec.id), rec.id);
  },
  // A person found the request invalid: the lock goes, unless another takedown in force names the post, which keeps it.
  forum_restore: async (env, rec, postId) => {
    const target = forumTargetOf(rec, postId);
    if (!target || target.restored_at) return;
    const other = await otherTakedownHoldingPost(env, postId, rec.id);
    if (other) {
      const kept = forumTargetOf(other, postId);
      await env.SOCIAL.put(forumPostLockKey(postId), JSON.stringify({ reason: 'takedown', reference: other.reference, locked_at: kept.locked_at || nowIso() }));
    } else {
      await env.SOCIAL.delete(forumPostLockKey(postId));
    }
    target.restored_at = nowIso();
  },
  // The listing's moderation case agrees with the lock.
  case: async (env, rec, simId) => {
    const target = targetOf(rec, simId);
    if (!target || target.restored_at) return;
    await holdModerationCase(env, rec, simId);
    target.case_held_at = target.case_held_at || nowIso();
  },
  // The listing's files on the blocklist.
  hashes: async (env, rec, simId, ctx) => {
    const target = targetOf(rec, simId);
    if (!target || target.restored_at) return;
    const sim = await readSim(env, simId);
    if (sim) for (const h of await listingHashes(env, sim)) await blockHash(env, rec, h);
    // New hashes: the identical-copy scan goes over every listing again, from the sweep if not from here.
    await restartScan(env);
  },
  // Possible CSAM: the listing's files are sealed into the evidence vault (src/evidence_vault.mjs) with the takedown's facts.
  // A seal that is not complete leaves the step owed, so the sweep runs it again; nothing is deleted either way. A request is
  // a person's statement, not a confident detection, so nothing is reported to an authority from here.
  seal: async (env, rec, simId) => {
    const target = targetOf(rec, simId);
    if (!target || target.restored_at || !rec.possible_csam) return;
    const sim = await readSim(env, simId);
    if (!sim) return;
    const r = await sealEvidence(env, {
      caseKind: 'takedown', caseId: rec.id, category: 'csam', subject: { kind: 'listing', sim_id: simId },
      uploader: { account_id: sim.author_id || null, uploaded_at: sim.updated_at || sim.published_at || null, upload_ip: null, upload_ip_note: 'The Worker does not record the address a listing was uploaded from.' },
      detection: { source: 'takedown_request', checks: [{ kind: 'request', source: 'takedown_request', category: 'csam', confidence: null, band: null }], band: null, viewed_by_staff: false },
      items: (await listingEvidenceItems(env, sim)).map((i) => ({ ...i, id: `${simId.slice(0, 8)}-${i.id}`.slice(0, 64) })),
    });
    target.sealed_at = r.ok ? (target.sealed_at || nowIso()) : null;
    if (!r.ok) throw new Error(`the evidence seal is not complete (${r.pending.length} item(s) pending)`);
  },
  // The first pages of the identical-copy scan run here, so a small Gallery is covered before the request is answered.
  scan: async (env, rec, _, ctx) => {
    await scanForCopies(env, ctx, { pages: 3 });
  },
  notice: async (env, rec, simId, ctx) => {
    const target = targetOf(rec, simId);
    if (!target || target.restored_at || !authorVisible(rec) || !target.author_id) return;
    await env.SOCIAL.put(againstKey(target.author_id, rec.id), rec.id);
    await tellAuthor(env, rec, target, target.via === 'identical_copy' ? 'removed_copy' : 'removed', ctx);
  },
  receipt: async (env, rec, _, ctx) => {
    rec.receipt = await sendRequester(env, rec, renderMail('receipt', receiptFacts(rec, ctx)), 'receipt');
  },
  staff: async (env, rec, _, ctx) => {
    rec.staff_told = await alertStaff(env, renderMail(rec.possible_csam ? 'staff_csam' : 'staff_new', staffFacts(rec, ctx)), { required: true });
  },
  staff_review: async (env, rec, _, ctx) => {
    await alertStaff(env, renderMail('staff_review', staffFacts(rec, ctx)), { required: true });
  },
  // After a person's decision.
  restore: async (env, rec, simId, ctx) => {
    const target = targetOf(rec, simId);
    if (!target || target.restored_at) return;
    await releaseModerationCase(env, rec, simId, ctx);
    target.restored_at = nowIso();
  },
  unblock: async (env, rec) => {
    for (const h of rec.hashes) await unblockHash(env, rec, h.sha256);
  },
  restored_notice: async (env, rec, simId, ctx) => {
    const target = targetOf(rec, simId);
    if (!target || !authorVisible(rec) || !target.author_id) return;
    await tellAuthor(env, rec, target, 'restored', ctx);
  },
  decision_notice: async (env, rec, _, ctx) => {
    if (rec.status === 'confirmed' && authorVisible(rec)) {
      for (const target of rec.targets) if (!target.restored_at && target.author_id) await tellAuthor(env, rec, target, 'upheld', ctx);
    }
    rec.decision_mail = await sendRequester(env, rec, renderMail('requester_decided', { ...receiptFacts(rec, ctx), outcome: rec.status }), `decided-${rec.status}`);
  },
};

const stepKind = (step) => step.split(':')[0];
const stepArg = (step) => step.split(':').slice(1).join(':') || null;
const REMOVAL_STEP = (step) => ['lock', 'forum_lock', 'case', 'hashes', 'notice', 'scan', 'seal'].includes(stepKind(step));
/// A step that removes content: while one is owed, something the request named may still be served.
const LOCK_STEP = (step) => ['lock', 'forum_lock'].includes(stepKind(step));

/// Runs every step the case owes, including those a step adds (a copy the scan finds), each once a pass. A step that throws stays
/// owed, with its error. Returns the steps that failed in this pass.
async function runSteps(env, rec, ctx) {
  const tried = new Set();
  const failed = [];
  for (;;) {
    const step = rec.pending.find((s) => !tried.has(s));
    if (!step) break;
    tried.add(step);
    const run = STEPS[stepKind(step)];
    try {
      if (!run) throw new Error(`unknown step ${step}`);
      await run(env, rec, stepArg(step), ctx);
      rec.pending = rec.pending.filter((s) => s !== step);
      if (rec.step_errors?.[step]) delete rec.step_errors[step];
    } catch (e) {
      const message = scrubAddresses(e?.message || String(e)).slice(0, 300);
      const prior = rec.step_errors?.[step];
      rec.step_errors = { ...(rec.step_errors || {}), [step]: { error: message, at: nowIso(), tries: (prior?.tries || 0) + 1 } };
      failed.push(step);
      console.error(`takedown ${rec.reference}: step ${step} failed:`, message);
    }
  }
  settleRemoval(rec);
  return failed;
}

/// The removal is complete when every listing the request named is locked: that moment is `removed_at`, recorded whatever the
/// case's status is by then (a person may have confirmed it while a lock was still being retried).
function settleRemoval(rec) {
  const listings = rec.targets.filter((t) => t.via === 'request');
  const posts = rec.forum_targets || [];
  const named = [...listings, ...posts];
  if (!named.length || rec.removed_at) return;
  if (named.every((t) => t.locked_at)) {
    rec.removed_at = named.map((t) => t.locked_at).sort().pop();
    if (rec.status === 'received') rec.status = 'removed';
    record(rec, 'removed', { listings: listings.length, forum_posts: posts.length, seconds_after_request: Math.round((Date.parse(rec.removed_at) - Date.parse(rec.received_at)) / 1000) });
  }
}

const forumTargetOf = (rec, postId) => (rec.forum_targets || []).find((t) => t.post_id === postId);

/// Another takedown in force that keeps the forum post locked, or null.
async function otherTakedownHoldingPost(env, postId, exceptId) {
  const listed = await env.SOCIAL.list({ prefix: `tdforum:${postId}:`, limit: 100 });
  for (const k of listed?.keys || []) {
    const id = k.name.slice(`tdforum:${postId}:`.length);
    if (id === exceptId) continue;
    const other = await loadTakedown(env, id);
    const target = other ? forumTargetOf(other, postId) : null;
    if (other && !['invalid', 'closed'].includes(other.status) && target && !target.restored_at) return other;
  }
  return null;
}

// -----------------------------------------------------------------------------
// The moderation case
// -----------------------------------------------------------------------------

/// A case for a listing that has none, in the shape moderation makes (its newCase is not exported), at the status the listing's
/// marker shows, so nothing about its review changes.
function caseFor(sim, simId) {
  const at = nowIso();
  const marker = sim?.moderation?.status;
  return {
    sim_id: simId, author_id: sim?.author_id || null, content_root: sim?.content_root || null,
    pak_etag: sim?.pak_etag || null, pak_size: sim?.scene_size_bytes || null,
    created_at: at, updated_at: at, status: CASE_STATUSES.includes(marker) ? marker : 'pending', trigger: 'takedown',
    moderation_version: MODERATION_VERSION, policy_version: POLICY_VERSION, policy_hash: null,
    signals: null, jev: null, triage: null, judge: null, agent: null, decision: null,
    human: null, appeal: null, legal_hold: null, author_notice: null, history: [{ at, event: 'created', trigger: 'takedown' }],
  };
}

/// The legal hold on the listing's case. Its status is left as it is (so an approval keeps its binding and a restored listing
/// serves again); the hold alone makes moderation lock the listing (legalLockKind). Written under moderation's lease, so a review
/// cannot interleave; while a review holds the lease this fails and is retried, and the lock already in force keeps the listing down.
async function holdModerationCase(env, rec, simId) {
  const lease = await acquireLease(env, simId, thresholdsFrom(env), 'takedown');
  if (!lease) throw new Error('a review of the listing holds its lease; the lock is in force and the case is written on the next attempt');
  try {
    const sim = await readSim(env, simId);
    const existing = await loadCase(env, simId);
    const prev = existing?.status;
    const mod = existing || caseFor(sim, simId);
    const at = nowIso();
    const held = mod.legal_hold;
    if (held?.takedowns?.includes(rec.id)) return;
    const csam = rec.possible_csam ? { possible_csam: true, escalated: true, escalated_at: held?.escalated_at || at, preserve_until: null, never_delete: true } : {};
    mod.legal_hold = held
      ? { ...held, takedowns: [...(held.takedowns || []), rec.id], ...csam }
      : {
        category: rec.possible_csam ? CSAM_CATEGORY : NCII_CATEGORY, source: 'takedown', takedowns: [rec.id], at, by: 'takedown', reported: false,
        preserve_until: new Date(Date.now() + PRESERVE_DAYS * 86400 * 1000).toISOString(), ...csam,
      };
    mod.history = (mod.history || []).slice(-60);
    mod.history.push({ at, event: 'takedown_hold', detail: { takedown: rec.reference, possible_csam: !!rec.possible_csam } });
    await saveCase(env, mod, prev);
  } finally {
    await releaseLease(env, lease);
  }
}

/// A person found the request invalid: the takedown's part of the hold goes, and the lock with it when nothing else holds the
/// listing (another takedown, a quarantine, a hold in the legal lane). The case is written before the lock is removed, so a
/// failure between the two leaves a lock too many, never one too few. Written directly: saveCase lets only moderation's own
/// person path drop a lock.
async function releaseModerationCase(env, rec, simId, ctx) {
  const lease = await acquireLease(env, simId, thresholdsFrom(env), 'takedown_release');
  if (!lease) throw new Error('a review of the listing holds its lease; the release runs on the next attempt');
  try {
    const mod = await loadCase(env, simId);
    if (mod?.legal_hold?.takedowns?.includes(rec.id)) {
      const rest = mod.legal_hold.takedowns.filter((id) => id !== rec.id);
      const at = nowIso();
      if (mod.legal_hold.source === 'takedown' && !rest.length) {
        mod.legal_release = { ...mod.legal_hold, released_at: at, released_by: 'takedown_review', release_rationale: 'a person found the takedown request invalid' };
        mod.legal_hold = null;
      } else {
        mod.legal_hold = { ...mod.legal_hold, takedowns: rest };
      }
      mod.updated_at = at;
      mod.history = (mod.history || []).slice(-60);
      mod.history.push({ at, event: 'takedown_released', detail: { takedown: rec.reference } });
      await env.SOCIAL.put(caseKey(simId), JSON.stringify(mod));
    }
    if (await otherTakedownHolds(env, simId, rec.id)) return;
    const kind = legalLockKind(mod);
    if (!kind) {
      // Only a lock a takedown wrote is removed here. One moderation wrote and its case no longer explains is a stray, which
      // moderation reports to a person (sweepLocks) and which this does not decide.
      if (await lockWrittenByTakedown(env, simId)) {
        await env.SCENES.delete(legalLockKey(simId));
        ctx.locksWritten = true;
      } else {
        record(rec, 'lock_left_for_a_person', { sim_id: simId });
      }
    } else if (kind === 'content_lock') {
      // The listing was held in the legal lane before the takedown raised its lock: it goes back to that.
      await env.SCENES.put(legalLockKey(simId), JSON.stringify({ kind, at: nowIso() }), { ...JSON_META, customMetadata: { kind } });
      ctx.locksWritten = true;
    }
  } finally {
    await releaseLease(env, lease);
  }
}

async function lockWrittenByTakedown(env, simId) {
  const obj = await env.SCENES.get(legalLockKey(simId));
  if (!obj) return false;
  try { return !!JSON.parse(await obj.text())?.takedown; } catch { return false; }
}

/// Whether another takedown still in force holds the listing. The case's list says so; this index is asked as well, for two
/// requests written at once.
async function otherTakedownHolds(env, simId, exceptId) {
  const listed = await env.SOCIAL.list({ prefix: `tdlisting:${simId}:`, limit: 100 });
  for (const k of listed?.keys || []) {
    const id = k.name.slice(`tdlisting:${simId}:`.length);
    if (id === exceptId) continue;
    const other = await loadTakedown(env, id);
    if (other && !['invalid', 'closed'].includes(other.status) && targetOf(other, simId) && !targetOf(other, simId).restored_at) return true;
  }
  return false;
}

// -----------------------------------------------------------------------------
// Hashes and the blocklist
// -----------------------------------------------------------------------------

/// `{ sha256, size }` of a stored object: the SHA-256 R2 keeps for it when it was stored with one (thumbnails, covers, captures,
/// single packages, manifests), otherwise its bytes hashed here, streamed where the platform can (Workers' DigestStream) and in
/// memory up to HASH_IN_MEMORY_MAX where it cannot. Null for no object, or one too large to hash here.
export async function hashStoredObject(env, key) {
  if (!key || !env?.SCENES?.head) return null;
  const head = await env.SCENES.head(key);
  if (!head) return null;
  const known = head.checksums?.sha256;
  if (known) return { sha256: typeof known === 'string' ? known.toLowerCase() : hexOf(known), size: head.size };
  const object = await env.SCENES.get(key);
  if (!object) return null;
  if (typeof crypto.DigestStream === 'function' && object.body) {
    const digest = new crypto.DigestStream('SHA-256');
    await object.body.pipeTo(digest);
    return { sha256: hexOf(await digest.digest), size: object.size };
  }
  if (object.size > HASH_IN_MEMORY_MAX) { await object.body?.cancel?.(); return null; }
  return { sha256: await sha256Hex(new Uint8Array(await object.arrayBuffer())), size: object.size };
}

/// The SHA-256 of every file stored for a listing that can be shown or played: the package (or the world and source manifests),
/// each Space package, the thumbnail, the cover, and every review capture. World chunks are named by their BLAKE3 in the
/// manifest, which is on the list; the chunks themselves are not hashed here.
export async function listingHashes(env, sim) {
  const out = [];
  const add = (sha256, kind, size) => { if (isSha(sha256) && !out.some((h) => h.sha256 === sha256)) out.push({ sha256, kind, size: Number.isSafeInteger(size) ? size : null, sim_id: sim.id }); };
  const own = (key) => typeof key === 'string' && key.startsWith(`universes/${sim.id}/`);
  if (own(sim.r2_key) && sim.format !== 'echk') {
    const known = [sim.pak_sha256, sim.pak_content_sha256].find(isSha);
    if (known) add(known, 'package', sim.scene_size_bytes);
    else { const h = await hashStoredObject(env, sim.r2_key); if (h) add(h.sha256, 'package', h.size); }
  }
  if (isSha(sim.world?.manifest_sha256)) add(sim.world.manifest_sha256, 'world_manifest');
  if (isSha(sim.world?.source_manifest_sha256)) add(sim.world.source_manifest_sha256, 'source_manifest');
  for (const space of Object.values(sim.spaces || {}).slice(0, 20)) {
    if (isSha(space?.sha256)) add(space.sha256, 'space_package', space.size_bytes);
    else if (own(space?.r2_key)) { const h = await hashStoredObject(env, space.r2_key); if (h) add(h.sha256, 'space_package', h.size); }
  }
  for (const ext of ['webp', 'png', 'jpg']) { const h = await hashStoredObject(env, `thumbnails/${sim.id}/thumb.${ext}`); if (h) add(h.sha256, 'thumbnail', h.size); }
  for (const ext of ['webp', 'png', 'jpg']) { const h = await hashStoredObject(env, `covers/${sim.id}/cover.${ext}`); if (h) add(h.sha256, 'cover', h.size); }
  const evidence = await env.SCENES.list({ prefix: evidencePrefix(sim.id, 0), limit: 500 });
  let captures = 0;
  for (const o of evidence?.objects || []) {
    if (!/\/capture-[0-7]\.(png|jpg|webp)$/.test(o.key) || captures >= 40) continue;
    captures += 1;
    const h = await hashStoredObject(env, o.key);
    if (h) add(h.sha256, 'capture', h.size);
  }
  return out;
}

async function readEntry(env, sha) {
  const raw = await env.SOCIAL.get(blockKey(sha));
  if (!raw) return null;
  try { return JSON.parse(raw) || {}; } catch { return {}; }
}

async function blockHash(env, rec, h) {
  const entry = (await readEntry(env, h.sha256)) || { first_at: nowIso(), kinds: [], cases: [] };
  entry.cases = Array.isArray(entry.cases) ? entry.cases : [];
  entry.kinds = Array.isArray(entry.kinds) ? entry.kinds : [];
  if (!entry.cases.includes(rec.id)) entry.cases.push(rec.id);
  if (!entry.kinds.includes(h.kind)) entry.kinds.push(h.kind);
  if (entry.size == null && h.size != null) entry.size = h.size;
  if (rec.possible_csam) entry.possible_csam = true;
  // The size rides in the key's metadata, so the scan reads the whole list in one call.
  await env.SOCIAL.put(blockKey(h.sha256), JSON.stringify(entry), { metadata: { size: entry.size ?? null } });
  if (!rec.hashes.some((x) => x.sha256 === h.sha256)) rec.hashes.push(h);
}

/// The case's hold on a hash ends; the hash leaves the blocklist only when no other case in force names it. The entry's own list
/// is one answer, and every active case's hashes are asked as well, for two cases that wrote the entry at once.
async function unblockHash(env, rec, sha) {
  const entry = await readEntry(env, sha);
  if (!entry) return;
  const others = (Array.isArray(entry.cases) ? entry.cases : []).filter((id) => id !== rec.id);
  const live = [];
  for (const id of others) {
    const other = await loadTakedown(env, id);
    if (other && !['invalid', 'closed'].includes(other.status)) live.push(id);
  }
  if (!live.length) {
    const active = await env.SOCIAL.list({ prefix: 'tdactive:', limit: 500 });
    for (const k of active?.keys || []) {
      const id = k.name.slice('tdactive:'.length);
      if (id === rec.id) continue;
      const other = await loadTakedown(env, id);
      if (other && !['invalid', 'closed'].includes(other.status) && other.hashes?.some((x) => x.sha256 === sha)) live.push(id);
    }
  }
  if (live.length) await env.SOCIAL.put(blockKey(sha), JSON.stringify({ ...entry, cases: live }), { metadata: { size: entry.size ?? null } });
  else await env.SOCIAL.delete(blockKey(sha));
}

/// The first of `hashes` (hex SHA-256) on the blocklist, `{ sha256, entry }`, or null. An entry that cannot be read still blocks.
export async function blockedHash(env, hashes) {
  for (const sha of [...new Set((hashes || []).filter(isSha))]) {
    const entry = await readEntry(env, sha);
    if (!entry) continue;
    if (!Array.isArray(entry.cases) || entry.cases.length) return { sha256: sha, entry };
  }
  return null;
}

/// The refusal for an upload whose bytes are on the blocklist, or null to let it through. Called by every upload path before
/// it stores anything: the .pak (single and multipart), each Space package, the world commit, the thumbnail, the cover and the
/// review captures. The attempt is kept as evidence and staff are told. The refusal says only that the file cannot be published,
/// never why, so a suspected uploader learns nothing about a case.
export async function refuseBlockedUpload(env, hashes, { json, cors, accountId = null, simId = null, route = null } = {}) {
  const hit = await blockedHash(env, hashes);
  if (!hit) return null;
  try {
    const attempt = { sha256: hit.sha256, cases: hit.entry.cases || [], possible_csam: !!hit.entry.possible_csam, account_id: accountId || null, sim_id: simId || null, route, at: nowIso() };
    await env.SOCIAL.put(`ncii-attempt:${attempt.at}:${crypto.randomUUID().slice(0, 8)}`, JSON.stringify(attempt));
    console.error(`takedown: an upload of a blocked file was refused (${route})`);
    await alertStaff(env, renderMail('staff_attempt', { ...attempt, references: await referencesOf(env, attempt.cases) }), { required: false });
  } catch (e) {
    console.error('takedown: recording a refused upload failed:', e?.message || e);
  }
  return json({ error: BLOCKED_UPLOAD_MESSAGE, code: 'content_blocked' }, 422, cors);
}

async function referencesOf(env, ids) {
  const out = [];
  for (const id of (ids || []).slice(0, 10)) { const rec = await loadTakedown(env, id); if (rec?.reference) out.push(rec.reference); }
  return out;
}

/// The review capture route is moderation's (src/moderation.mjs), which this module does not change, so index.js asks here first:
/// a capture of the author's own listing whose bytes, as sent or with their metadata dropped, are on the blocklist is refused.
/// Returns `{ response }` to answer with, or `{ request }` to hand on (rebuilt, since its body was read).
export async function checkCaptureUpload(request, env, cors, deps) {
  const declared = Number(request.headers.get('Content-Length'));
  // Over moderation's limit: moderation refuses it, unread.
  if (Number.isFinite(declared) && declared > CAPTURE_MAX_BYTES) return { request };
  const simId = new URL(request.url).pathname.split('/')[3];
  const auth = await deps.verifyAuth(request, env);
  const sim = auth ? await readSim(env, simId) : null;
  // Not the author: moderation refuses it, and nobody else learns from this route whether a file is blocked.
  if (!sim || sim.author_id !== auth) return { request };
  const bytes = new Uint8Array(await request.arrayBuffer());
  const hashes = [await sha256Hex(bytes)];
  const prepared = await prepareUserImage(bytes).catch(() => null);
  if (prepared?.ok) hashes.push(prepared.sha256);
  const refused = await refuseBlockedUpload(env, hashes, { json: deps.json, cors, accountId: auth, simId, route: 'capture' });
  if (refused) return { response: refused };
  return { request: new Request(request.url, { method: request.method, headers: request.headers, body: bytes }) };
}

// -----------------------------------------------------------------------------
// Known identical copies
// -----------------------------------------------------------------------------
// A bounded scan, like moderation's sweeps: a page of listing records, then pages of the thumbnails and the covers in R2, each run
// carrying on where the last stopped (`ncii-scan:state`), and starting over whenever new hashes join the list. A listing record
// carries its package's and its world's SHA-256, so no object is read for those. An image is read only when its size is the size
// of a blocked image. Review captures are not scanned across listings (they sit under each listing's own prefix); a capture is
// blocked at upload, and the listing it belongs to is scanned by its other files.

async function readScan(env) {
  const raw = await env.SOCIAL.get(SCAN_KEY);
  if (!raw) return null;
  try { return JSON.parse(raw); } catch { return null; }
}

async function restartScan(env) {
  const prev = await readScan(env);
  await env.SOCIAL.put(SCAN_KEY, JSON.stringify({ phase: 'listings', cursor: null, generation: (prev?.generation || 0) + 1, started_at: nowIso(), updated_at: nowIso() }));
}

/// Every blocked hash, with the size of its file when known.
async function blockedSet(env) {
  const out = new Map();
  let cursor;
  do {
    const page = await env.SOCIAL.list({ prefix: 'ncii-hash:', limit: 1000, ...(cursor ? { cursor } : {}) });
    for (const k of page?.keys || []) {
      const sha = k.name.slice('ncii-hash:'.length);
      if (!isSha(sha)) continue;
      let size = k.metadata?.size;
      if (size === undefined) {
        const entry = await readEntry(env, sha);
        if (Array.isArray(entry?.cases) && !entry.cases.length) continue;
        size = entry?.size ?? null;
      }
      out.set(sha, Number.isSafeInteger(size) ? size : null);
    }
    cursor = page?.list_complete === false && page.cursor ? page.cursor : undefined;
  } while (cursor);
  return out;
}

function recordHashes(sim) {
  const out = [];
  for (const [sha, kind] of [[sim.pak_sha256, 'package'], [sim.pak_content_sha256, 'package'], [sim.world?.manifest_sha256, 'world_manifest'], [sim.world?.source_manifest_sha256, 'source_manifest']]) if (isSha(sha)) out.push([sha, kind]);
  for (const space of Object.values(sim.spaces || {})) if (isSha(space?.sha256)) out.push([space.sha256, 'space_package']);
  return out;
}

/// Runs up to `pages` pages of the scan. `{ pages, locked, candidates, done }`.
export async function scanForCopies(env, ctx = {}, { pages = 3, listingsPerPage = 200, objectsPerPage = 500 } = {}) {
  const out = { pages: 0, locked: 0, candidates: 0, done: false };
  let state = await readScan(env);
  if (!state || state.phase === 'done') { out.done = true; return out; }
  const blocked = await blockedSet(env);
  if (!blocked.size) {
    await env.SOCIAL.put(SCAN_KEY, JSON.stringify({ ...state, phase: 'done', cursor: null, updated_at: nowIso() }));
    out.done = true;
    return out;
  }
  const sizes = new Set([...blocked.values()].filter((n) => n !== null));
  while (out.pages < pages && state.phase !== 'done') {
    out.pages += 1;
    if (state.phase === 'listings') {
      const page = await env.SOCIAL.list({ prefix: 'sim:', limit: listingsPerPage, ...(state.cursor ? { cursor: state.cursor } : {}) });
      for (const k of page?.keys || []) {
        if (!/^sim:[0-9a-f-]{36}$/.test(k.name)) continue;
        const sim = await readSim(env, k.name.slice(4));
        if (!sim?.id) continue;
        for (const [sha, kind] of recordHashes(sim)) if (blocked.has(sha)) await copyFound(env, ctx, sim, sha, kind, out);
      }
      state = page?.list_complete === false && page.cursor ? { ...state, cursor: page.cursor } : { ...state, phase: 'thumbnails', cursor: null };
    } else {
      const prefix = state.phase === 'thumbnails' ? 'thumbnails/' : 'covers/';
      const page = await env.SCENES.list({ prefix, limit: objectsPerPage, ...(state.cursor ? { cursor: state.cursor } : {}) });
      for (const o of page?.objects || []) {
        if (!sizes.has(o.size)) continue;
        const simId = o.key.split('/')[1];
        if (!UUID.test(simId || '')) continue;
        const h = await hashStoredObject(env, o.key);
        if (!h || !blocked.has(h.sha256)) continue;
        const sim = await readSim(env, simId);
        if (sim) await copyFound(env, ctx, sim, h.sha256, state.phase === 'thumbnails' ? 'thumbnail' : 'cover', out);
      }
      state = page?.truncated && page.cursor ? { ...state, cursor: page.cursor } : { ...state, phase: state.phase === 'thumbnails' ? 'covers' : 'done', cursor: null };
    }
    state.updated_at = nowIso();
    await env.SOCIAL.put(SCAN_KEY, JSON.stringify(state));
  }
  out.done = state.phase === 'done';
  return out;
}

/// A listing holding a blocked file joins the earliest case in force that blocked it, and is locked and its author told like a
/// listing the request named. Past COPIES_LOCKED_PER_HASH_MAX copies of one file, it is recorded for a person instead.
async function copyFound(env, ctx, sim, sha, kind, out) {
  const entry = await readEntry(env, sha);
  for (const caseId of Array.isArray(entry?.cases) ? entry.cases : []) {
    const open = ctx.open?.get(caseId);
    const rec = open || (await loadTakedown(env, caseId));
    if (!rec || ['invalid', 'closed'].includes(rec.status)) continue;
    if (targetOf(rec, sim.id)) return;
    const prev = rec.status;
    const copies = rec.targets.filter((t) => t.via === 'identical_copy' && t.matched_hash === sha).length;
    if (copies >= COPIES_LOCKED_PER_HASH_MAX) {
      rec.copy_candidates = (rec.copy_candidates || []).filter((c) => c.sim_id !== sim.id).slice(-99);
      rec.copy_candidates.push({ sim_id: sim.id, sha256: sha, kind, at: nowIso() });
      record(rec, 'identical_copy_for_a_person', { sim_id: sim.id, kind });
      out.candidates += 1;
      if (!rec.candidates_alerted_at) {
        rec.candidates_alerted_at = nowIso();
        await alertStaff(env, renderMail('staff_candidates', { reference: rec.reference })).catch((e) => console.error('takedown: alert failed:', e?.message || e));
      }
      if (!open) await saveTakedown(env, rec, prev);
      return;
    }
    rec.targets.push({ sim_id: sim.id, author_id: sim.author_id || null, via: 'identical_copy', matched_hash: sha, matched_kind: kind, found_at: nowIso(), locked_at: null, restored_at: null });
    rec.pending.push(`lock:${sim.id}`, `case:${sim.id}`, `notice:${sim.id}`, ...(rec.possible_csam ? [`seal:${sim.id}`] : []));
    record(rec, 'identical_copy_found', { sim_id: sim.id, kind });
    out.locked += 1;
    if (!open) {
      // The same context, so a lock written here refreshes the caches like any other; the open cases are put back after.
      const saved = ctx.open;
      ctx.open = new Map([[rec.id, rec]]);
      try { await runSteps(env, rec, ctx); } finally { ctx.open = saved; }
      await saveTakedown(env, rec, prev);
    }
    return;
  }
}

/// What serving keeps for a few seconds is refreshed where this ran: this isolate's list of locked listings (the Gallery reads
/// it), and this data centre's live strip. Another isolate's list is at most 10 seconds old (legalLockedIds); every route that
/// sends a listing's content asks R2 for the lock on each request.
async function refreshServingCaches(env, ctx) {
  try { await legalLockedIds(env, 0); } catch (e) { console.error('takedown: refreshing the locked-listing list failed:', e?.message || e); }
  const cache = ctx.cache === undefined ? defaultCache() : ctx.cache;
  if (!cache) return;
  for (const slice of RATINGS) {
    try { await cache.delete(new Request(`${ctx.api}/__cache/live?rating_max=${slice}`)); } catch { /* best effort: the strip's own cache lasts 10 seconds */ }
  }
}

// -----------------------------------------------------------------------------
// Telling people
// -----------------------------------------------------------------------------

function contextFor(env, deps, url) {
  const api = String(url?.origin || env.PUBLIC_API_ORIGIN || 'https://api.eustress.dev').replace(/\/$/, '');
  return { api, site: siteOrigin(env), deps: deps || {}, open: new Map(), locksWritten: false };
}

/// A listing's name as a subject line may carry it: no control characters (a line break would end the header), at most 80 characters.
const safeName = (value) => String(value || 'Untitled').replace(/[\x00-\x1f\x7f]+/g, ' ').trim().slice(0, 80) || 'Untitled';

async function tellAuthor(env, rec, target, event, ctx) {
  const sim = await readSim(env, target.sim_id);
  const name = safeName(sim?.name);
  const at = event === 'restored' ? target.restored_at : event === 'upheld' ? rec.decided_at : target.locked_at;
  // The feed item, once (its key says so): what the account page and Studio show.
  const feedKey = `${notifPrefix(target.author_id)}takedown:${rec.id}:${target.sim_id}:${event}`;
  if (!(await env.SOCIAL.get(feedKey))) {
    const item = {
      id: feedKey.slice(notifPrefix(target.author_id).length), kind: 'takedown', event, sim_id: target.sim_id, sim_name: name, reference: rec.reference,
      headline: AUTHOR_HEADLINES[event], at: at || nowIso(), read: false, expires_at: new Date(Date.now() + NOTIFY_TTL_SECONDS * 1000).toISOString(),
    };
    await env.SOCIAL.put(feedKey, JSON.stringify(item), { expirationTtl: NOTIFY_TTL_SECONDS });
  }
  // The email, once.
  const mailKey = `tdmail:${rec.id}:${target.sim_id}:${event}`;
  if (await env.SOCIAL.get(mailKey)) return;
  const user = JSON.parse((await env.USERS.get(`user:${target.author_id}`)) || 'null');
  const to = String((await readAccountEmail(env, user)) || '').trim();
  if (!EMAIL.test(to)) return;
  const timeZone = await accountTimeZone(env, target.author_id);
  const rendered = renderMail(`author_${event}`, { name, at, reference: rec.reference, site: ctx.site, timeZone });
  const [result] = await sendNotice(env, [to], rendered, { kind: 'notice', accountId: target.author_id, fromName: FROM_NAME });
  if (result?.reason === 'send_failed') throw new Error('the author email was not sent');
  await env.SOCIAL.put(mailKey, JSON.stringify({ at: nowIso(), sent: !!result?.sent, reason: result?.reason || null }));
}

/// Mail to the requester, at the address they gave, counted against that address's daily allowance. A send that failed is retried;
/// no mail binding, or an allowance used up, is recorded and not retried.
async function sendRequester(env, rec, rendered, tag) {
  const mailKey = `tdmail:${rec.id}:requester:${tag}`;
  const done = await env.SOCIAL.get(mailKey);
  if (done) return JSON.parse(done);
  const [result] = await sendNotice(env, [rec.request.contact_email], rendered, { kind: 'pending', fromName: FROM_NAME });
  if (result?.reason === 'send_failed') throw new Error('the requester email was not sent');
  const outcome = { at: nowIso(), sent: !!result?.sent, reason: result?.reason || null };
  await env.SOCIAL.put(mailKey, JSON.stringify(outcome));
  return outcome;
}

/// Where staff hear about takedowns: TAKEDOWN_ALERT_MAILBOX (comma separated), else the security mailbox, and every
/// administrator's address, as an administrator's key replacement is reported (src/identity_routes.mjs).
async function staffAddresses(env) {
  const out = new Set();
  for (const a of String(env.TAKEDOWN_ALERT_MAILBOX || env.SECURITY_MAILBOX || 'security@eustress.dev').split(',')) {
    if (EMAIL.test(a.trim())) out.add(a.trim().toLowerCase());
  }
  try {
    const admins = await env.USERS.list({ prefix: 'admin:', limit: 100 });
    for (const k of admins?.keys || []) {
      const raw = await env.USERS.get(`user:${k.name.slice('admin:'.length)}`);
      const address = raw ? await readAccountEmail(env, JSON.parse(raw)) : null;
      if (typeof address === 'string' && EMAIL.test(address.trim())) out.add(address.trim().toLowerCase());
    }
  } catch (e) {
    console.error('takedown: listing the administrators failed:', e?.message || e);
  }
  return [...out];
}

/// Emails staff. `required`: a send that reached nobody, with a mail binding present, throws so the step is retried. With no mail
/// binding the log is all there is, and it says so.
async function alertStaff(env, rendered, { required = false } = {}) {
  console.error(`takedown staff alert: ${rendered.subject}`);
  const results = await sendNotice(env, await staffAddresses(env), rendered, { kind: 'notice', accountId: 'takedown-alerts', fromName: FROM_NAME });
  const sent = results.filter((r) => r.sent).length;
  if (required && !sent && results.some((r) => r.reason === 'send_failed')) throw new Error('no staff address could be emailed');
  return { at: nowIso(), sent };
}

/// A forum post reported as csam (src/moderation.mjs fileForumReport), on this module's staff alert path: the same mailboxes,
/// the staff-only possible-CSAM notice. `facts`: reference, post_id, thread_id, at, queue_url. Never a reporter or a note.
export async function alertStaffForumReport(env, facts) {
  return alertStaff(env, renderMail('staff_forum_csam', facts), { required: false });
}

/// A seal into the evidence vault failed (src/evidence_vault.mjs), on this module's staff alert path. `facts`: reference,
/// case_kind, case_id, item, category, attempts, error, material_kept. Never the content, the uploader or an address.
export async function alertStaffEvidence(env, facts) {
  return alertStaff(env, renderMail('staff_evidence_failure', facts), { required: false });
}

/// A legal report is queued or held (src/legal_reports.mjs), on this module's staff alert path. `facts`: id, kind, status,
/// reference, destination, reason, admin_url. Never the content and never the reported account's identity.
export async function alertStaffLegalReport(env, facts) {
  return alertStaff(env, renderMail('staff_legal_report', facts), { required: false });
}

function staffFacts(rec, ctx) {
  return {
    reference: rec.reference, id: rec.id, received_at: rec.received_at, removed_at: rec.removed_at, possible_csam: !!rec.possible_csam,
    named: rec.targets.filter((t) => t.via === 'request').length, copies: rec.targets.filter((t) => t.via === 'identical_copy').length, forum_posts: (rec.forum_targets || []).length,
    not_found: rec.unresolved.length, pending: rec.pending, errors: rec.step_errors || {}, admin_url: `${ctx.api}/api/admin/takedowns/${rec.id}`,
    reviews: (rec.reviews || []).map((r) => r.party),
  };
}

function receiptFacts(rec, ctx) {
  const named = rec.locations || [];
  return {
    reference: rec.reference, received_at: rec.received_at, removed_at: rec.removed_at,
    removed: named.filter((l) => l.result === 'removed').length, total: named.length,
    listings_removed: named.filter((l) => l.result === 'removed' && l.listing_id).length,
    not_found: named.filter((l) => l.result === 'not_found').map((l) => l.location),
    pending: named.filter((l) => l.result === 'pending').map((l) => l.location),
    status_page: `${ctx.site}/cases/${rec.reference}`, decided_at: rec.decided_at,
  };
}

// -----------------------------------------------------------------------------
// The mail itself: plain, calm, specific. No email names the requester to anyone but staff, and none repeats what the request
// described. The footer has no links (`plain`).
// -----------------------------------------------------------------------------

export const AUTHOR_HEADLINES = {
  removed: 'Your listing was removed under a TAKE IT DOWN request.',
  removed_copy: 'Your listing was removed: it holds a file removed under a TAKE IT DOWN request.',
  restored: 'Your listing is available again.',
  upheld: 'A person upheld the removal of your listing.',
};

const LAW_LINE = 'The TAKE IT DOWN Act is a United States law. It requires platforms to remove intimate images of a person that were shared without their consent, within 48 hours of the person asking.';
const REVIEW_LINE = (site) => `If you believe this is a mistake, you can ask a person to review it: open Cases on your account page (${site}/account/cases) and choose "Ask for a review", or write to ${SUPPORT} with the case reference. Nothing is restored automatically; a person decides.`;

function mail({ subject, heading, label, paragraphs, rows = [], notes = [] }) {
  const text = [...paragraphs, '', ...rows.map(([k, v]) => `${k}: ${v}`), ...(notes.length ? ['', ...notes] : [])].join('\n');
  const body = html`${paragraphs.map((p) => paragraph(p))}${rows.length ? facts(rows) : ''}${notes.map((n) => noteLine(n))}`;
  const email = renderEmail({ title: heading, heading, label, preheader: paragraphs[0], body, text, footer: 'plain' });
  return { subject, text: email.text, html: email.html };
}

/// The subject, text and HTML of one takedown email. Exported for the tests, which read every kind.
export function renderMail(kind, d) {
  const when = (v) => formatWhen(v, d.timeZone);
  switch (kind) {
    case 'author_removed':
    case 'author_removed_copy':
      return mail({
        subject: `Your Eustress listing "${d.name}" was removed`, heading: 'Your listing was removed', label: 'Account Notice',
        paragraphs: [
          `Your listing "${d.name}" was removed from Eustress on ${when(d.at)}.`,
          kind === 'author_removed'
            ? 'We received a request under the TAKE IT DOWN Act that named this listing, so it was removed straight away, as the law requires.'
            : 'We received a request under the TAKE IT DOWN Act. This listing holds a file that is identical, byte for byte, to one the request named, so it was removed too.',
          LAW_LINE,
          'While the case is open the listing is not shown in the Gallery and cannot be opened, played or downloaded. Its files are kept unchanged and cannot be replaced, and the same files cannot be published again.',
          REVIEW_LINE(d.site),
        ],
        rows: [['Case reference', d.reference]],
      });
    case 'author_restored':
      return mail({
        subject: `Your Eustress listing "${d.name}" is available again`, heading: 'Your listing is available again', label: 'Account Notice',
        paragraphs: [
          `A person at Eustress reviewed case ${d.reference}, a request under the TAKE IT DOWN Act that removed your listing "${d.name}", and found that it could not be acted on.`,
          `The listing is available again as of ${when(d.at)}, as it was before. Nothing in it was changed.`,
        ],
        rows: [['Case reference', d.reference]],
      });
    case 'author_upheld':
      return mail({
        subject: `The removal of your Eustress listing "${d.name}" was upheld`, heading: 'The removal was upheld', label: 'Account Notice',
        paragraphs: [
          `A person at Eustress reviewed case ${d.reference} about your listing "${d.name}" and upheld its removal under the TAKE IT DOWN Act on ${when(d.at)}.`,
          `The listing stays removed. If you have information the reviewer did not have, write to ${SUPPORT} with the case reference.`,
        ],
        rows: [['Case reference', d.reference]],
      });
    case 'receipt':
      return mail({
        subject: `We received your request: case ${d.reference}`, heading: 'We received your request', label: 'TAKE IT DOWN Request',
        paragraphs: [
          `We received your request to remove content from Eustress under the TAKE IT DOWN Act on ${when(d.received_at)}.`,
          d.removed
            ? `We removed the content at ${d.removed} of the ${d.total} location${d.total === 1 ? '' : 's'} you gave${d.removed_at ? ` on ${when(d.removed_at)}` : ''}. ${d.listings_removed === 0 ? 'It cannot be seen on Eustress.' : 'It cannot be seen, played or downloaded on Eustress, and the same files cannot be published again.'}`
            : 'Nothing has been removed yet. A person at Eustress will look at your request.',
          ...(d.pending.length ? [`Removing the content at ${d.pending.length} location${d.pending.length === 1 ? '' : 's'} has not finished yet. It is retried automatically, and our staff have been told.`] : []),
          ...(d.not_found.length ? [`We could not find content at: ${d.not_found.join(', ')}. A person will look at these. If a link was mistyped, send a new request with the correct link.`] : []),
          `A person at Eustress reviews every request. You can check its status at any time at ${d.status_page}.`,
        ],
        rows: [['Case reference', d.reference]],
        notes: [
          'Keep this reference. It is how you follow your request without an account.',
          `To add information, write to ${SUPPORT} and include the reference. Do not send the images themselves. If you did not send this request, write to ${SUPPORT} and include the reference.`,
        ],
      });
    case 'requester_decided':
      return mail({
        subject: `Your request ${d.reference} was reviewed`, heading: 'Your request was reviewed', label: 'TAKE IT DOWN Request',
        paragraphs: d.outcome === 'confirmed'
          ? [`A person at Eustress reviewed your request ${d.reference} and confirmed the removal on ${when(d.decided_at)}. The content stays removed, and the same files cannot be published again.`]
          : d.outcome === 'closed'
            ? [`A person at Eustress reviewed your request ${d.reference} on ${when(d.decided_at)} and found nothing on Eustress at the locations it gave. If you have a correct link, send a new request or write to ${SUPPORT} with the reference.`]
            : [`A person at Eustress reviewed your request ${d.reference} on ${when(d.decided_at)} and could not act on it as it was sent, so the content it named is available again.`,
              `If you can add information that shows the content is an intimate image of you, or of the person you act for, shared without consent, write to ${SUPPORT} with the reference, or send a new request.`],
        rows: [['Case reference', d.reference]],
      });
    case 'staff_new':
    case 'staff_csam':
      return mail({
        subject: kind === 'staff_csam' ? `Possible CSAM in a TAKE IT DOWN request: ${d.reference}` : `TAKE IT DOWN request received: ${d.reference}`,
        heading: kind === 'staff_csam' ? 'Possible CSAM: a TAKE IT DOWN request' : 'A TAKE IT DOWN request was received', label: 'Staff Notice',
        paragraphs: [
          kind === 'staff_csam'
            ? 'The requester says the person shown is a minor. The content is locked and preserved with no end date; nothing was deleted, and its author was not told. A person decides any report to the NCMEC CyberTipline.'
            : 'The content the request named was locked at once. A person reviews the request: confirm it, or mark it invalid to restore the content.',
          `Open the case with an administrator session: ${d.admin_url}. The requester's details are in the case, not in this email.`,
        ],
        rows: [['Case', d.reference], ['Case id', d.id], ['Received', when(d.received_at)], ['Removed', d.removed_at ? when(d.removed_at) : 'not yet'],
          ['Listings named', String(d.named)], ['Forum posts named', String(d.forum_posts ?? 0)], ['Identical copies', String(d.copies)], ['Locations not found', String(d.not_found)], ['Steps still owed', d.pending.join(', ') || 'none']],
      });
    case 'staff_failure':
      return mail({
        subject: `A TAKE IT DOWN step is failing: ${d.reference}`, heading: 'A takedown step is failing', label: 'Staff Notice',
        paragraphs: [
          'A step of this takedown failed and is retried every hour until it succeeds. A removal step that is still owed means content the request named may still be served.',
          `The law allows 48 hours from the request: until ${when(d.deadline)}.`,
          `Open the case with an administrator session: ${d.admin_url}.`,
        ],
        rows: [['Case', d.reference], ['Received', when(d.received_at)], ['Steps still owed', d.pending.join(', ')], ['Last errors', Object.entries(d.errors).map(([s, e]) => `${s}: ${e.error}`).join('; ').slice(0, 600) || 'none']],
      });
    case 'staff_relocked':
      return mail({
        subject: `A takedown's lock was found lifted and was put back: ${d.reference}`, heading: 'A takedown lock was put back', label: 'Staff Notice',
        paragraphs: [
          'A listing or forum post this takedown removed was found without its lock, or with a weaker one, while the takedown is still in force. Something other than the takedown review moved it, for example a release of a moderation hold on the same listing. The lock was written again.',
          `To restore the listing, mark the takedown invalid. Open the case with an administrator session: ${d.admin_url}.`,
        ],
        rows: [['Case', d.reference], ['Steps still owed', d.pending.join(', ') || 'none']],
      });
    case 'staff_review':
      return mail({
        subject: `A review was asked for on takedown ${d.reference}`, heading: 'A person review was asked for', label: 'Staff Notice',
        paragraphs: [`The ${d.reviews.join(' and ')} of takedown ${d.reference} asked for a person to review it. The statement is in the case.`, `Open the case with an administrator session: ${d.admin_url}.`],
        rows: [['Case', d.reference]],
      });
    case 'staff_attempt':
      return mail({
        subject: d.possible_csam ? 'A blocked file (possible CSAM) was uploaded again' : 'A blocked file was uploaded again',
        heading: 'An upload of a blocked file was refused', label: 'Staff Notice',
        paragraphs: ['Someone tried to publish a file on the takedown blocklist. The upload was refused and the attempt is kept as evidence.'],
        rows: [['Account', d.account_id || 'none'], ['Listing', d.sim_id || 'none'], ['Route', d.route || 'unknown'], ['File SHA-256', d.sha256], ['Cases', (d.references || []).join(', ') || 'unknown'], ['When', when(d.at)]],
      });
    case 'staff_candidates':
      return mail({
        subject: `Identical copies need a person: ${d.reference}`, heading: 'Identical copies need a person', label: 'Staff Notice',
        paragraphs: ['More listings than the scan locks on its own hold a file from this takedown. They were recorded on the case and not locked.'],
        rows: [['Case', d.reference]],
      });
    case 'staff_forum_csam':
      return mail({
        subject: `Possible CSAM reported in a forum post: ${d.reference || d.post_id}`, heading: 'Possible CSAM: a forum post was reported', label: 'Staff Notice',
        paragraphs: [
          'A member reported a forum post as sexual content involving a minor (Code A1). The post was hidden at once, and no one is asked to read it. Its author was not told why. A person decides any report to the NCMEC CyberTipline.',
          `The case is a record, readable without opening the post: the moderation_forum_case tool, or the queue at ${d.queue_url}. It holds a sealed evidence pointer (the post id, a keyed hash of its body, the times), never the body. The reporter and their note are in the case, not in this email.`,
        ],
        rows: [['Case', d.reference || 'not yet referenced'], ['Forum post', d.post_id], ['Thread', d.thread_id], ['Reported', when(d.at)]],
      });
    case 'staff_evidence_failure':
      return mail({
        subject: `Evidence was not sealed: ${d.reference || `${d.case_kind} ${d.case_id}`}`, heading: 'Evidence was not sealed', label: 'Staff Notice',
        paragraphs: [
          'Content removed as illegal could not be sealed into the evidence vault. Nothing was deleted: the content is still removed, and the seal is tried again every hour until it holds.',
          d.material_kept
            ? 'What the seal needs is kept, so no one has to do anything unless this keeps failing.'
            : 'The content to seal is not kept by the Worker (a forum post\'s text with no vault key): keep the post in the forum\'s storage, and check that JWT_SECRET is configured.',
          'No one needs to open the content. This email names the case only.',
        ],
        rows: [['Case', d.reference || 'not yet referenced'], ['Case kind', d.case_kind], ['Case id', d.case_id], ['Item', d.item], ['Category', d.category], ['Attempts', String(d.attempts ?? 1)], ['Error', d.error || 'unknown']],
      });
    case 'staff_legal_report':
      return mail({
        subject: `A legal report is ${d.status === 'held_setting_off' ? 'held' : 'waiting to be sent'}: ${d.reference || d.id}`, heading: d.status === 'held_setting_off' ? 'A legal report is held' : 'A legal report is waiting to be sent', label: 'Staff Notice',
        paragraphs: [
          d.status === 'held_setting_off'
            ? 'A report was written and is held: its sending is switched off, so it goes to no one until that setting is turned on.'
            : 'A report was written and queued. Its sender is switched off or not yet registered, so it waits in the queue as ready to send.',
          'The report was written from the case record and the sealed evidence index. No one at Eustress viewed the content, and no one needs to.',
          `The queue, with an administrator session: ${d.admin_url}.`,
        ],
        rows: [['Report', d.id], ['Kind', d.kind], ['Case', d.reference || 'not yet referenced'], ['Destination', d.destination || 'none set'], ['Status', d.status], ['Why it waits', d.reason || 'the sender is off']],
      });
    default:
      throw new Error(`unknown takedown mail ${kind}`);
  }
}

// -----------------------------------------------------------------------------
// Filing a request
// -----------------------------------------------------------------------------

function tooMany(json, headers, seconds) {
  return json({
    error: `Too many requests from this address or for this email address. Your request was not filed. Write to ${SUPPORT} with the links and we will act on it.`,
    code: 'rate_limited', retry_after_seconds: seconds,
  }, 429, { ...headers, 'Retry-After': String(seconds) });
}

async function fileRequest(request, url, env, cors, deps) {
  const json = deps.json;
  const headers = { ...cors, 'Cache-Control': 'no-store' };
  const receivedAt = nowIso();
  const ip = request.headers.get('CF-Connecting-IP') || '';
  // A burst from one address is stopped before anything is read (the platform limiter, keyed apart from sign-in's).
  if (env.AUTH_RATE_LIMITER && ip) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `takedown:${ip}` });
    if (!success) return tooMany(json, headers, 60);
  }
  let body;
  try { body = await request.json(); } catch {
    return json({ error: 'The request must be JSON. Nothing was removed.', code: 'invalid_request', missing: ['body'], problems: [{ field: 'body', message: 'Send the request as JSON.' }] }, 400, headers);
  }
  const checked = checkTakedownRequest(body, ownHosts(url, env));
  if (checked.problems.length) {
    return json({
      error: 'The request is missing information or has a problem, so it was not filed and nothing was removed.', code: 'invalid_request',
      missing: [...new Set(checked.problems.map((p) => p.field))], problems: checked.problems,
    }, 400, headers);
  }
  const value = checked.value;
  if (!(await takeDailyAllowance(env, ip, value.contact_email))) {
    const midnight = new Date(); midnight.setUTCHours(24, 0, 0, 0);
    return tooMany(json, headers, Math.max(60, Math.ceil((midnight.getTime() - Date.now()) / 1000)));
  }
  const accountId = await deps.verifyAuth(request, env);
  const ctx = contextFor(env, deps, url);

  // The case first: everything it causes is recorded on it. A case that cannot be written changes nothing.
  let rec;
  try {
    rec = await openCase(env, value, { accountId, ip, receivedAt });
  } catch (e) {
    console.error('takedown: the case could not be written:', e?.message || e);
    return json({ error: `We could not record your request just now, so nothing was changed. Try again in a minute, or write to ${SUPPORT} with the links.`, code: 'unavailable' }, 503, headers);
  }
  ctx.open.set(rec.id, rec);
  await runSteps(env, rec, ctx);
  for (const l of rec.locations) {
    if (l.listing_id && l.result !== 'not_found') l.result = targetOf(rec, l.listing_id)?.locked_at ? 'removed' : 'pending';
    else if (l.forum_post_id && l.result !== 'not_found') l.result = forumTargetOf(rec, l.forum_post_id)?.locked_at ? 'removed' : 'pending';
  }
  await saveTakedown(env, rec, 'received');
  if (ctx.locksWritten) await refreshServingCaches(env, ctx);
  // A removal step that failed is the one thing staff must hear about now; the rest is retried by the sweep first.
  if (rec.pending.some(LOCK_STEP)) await alertStaff(env, renderMail('staff_failure', { ...staffFacts(rec, ctx), deadline: deadlineOf(rec) })).catch(() => {});
  try { await deps.auditLog?.(env, 'TAKEDOWN_RECEIVED', 'system', rec.reference, { case_id: rec.id, listings: rec.targets.length, removed_at: rec.removed_at, possible_csam: rec.possible_csam }); }
  catch (e) { console.error('takedown: the audit log write failed:', e?.message || e); }

  return json({
    reference: rec.reference,
    status: takedownPublicStatus(rec),
    received_at: rec.received_at,
    removed_at: rec.removed_at,
    locations: rec.locations.map(({ location, result }) => ({ location, result })),
    status_url: `${ctx.api}/api/cases/${rec.reference}`,
    message: rec.status === 'removed'
      ? 'We removed the content at the locations we could find. Keep your case reference: it is how you follow this request. A person reviews every request.'
      : 'We received your request. Keep your case reference: it is how you follow this request. A person reviews every request.',
  }, 201, headers);
}

const deadlineOf = (rec) => new Date(Date.parse(rec.received_at) + REMOVAL_DEADLINE_HOURS * 3600 * 1000).toISOString();

async function openCase(env, value, { accountId, ip, receivedAt }) {
  const id = crypto.randomUUID();
  const reference = await assignCaseReference(env, 'takedown', id);
  const rec = {
    v: 1, id, reference, kind: 'takedown', law: TAKEDOWN_LAW, status: 'received',
    received_at: receivedAt, removed_at: null, decided_at: null, updated_at: receivedAt,
    possible_csam: value.depicts_minor === true,
    request: {
      requester_name: value.requester_name, signature: value.signature, signed_at: receivedAt,
      relationship: value.relationship, depicted_name: value.depicted_name, contact_email: value.contact_email,
      description: value.description, good_faith: true, depicts_minor: value.depicts_minor === true,
      locations: value.locations.map((l) => l.location), account_id: accountId || null,
      // The address only as a keyed hash: enough to see one address behind many requests, not to read it.
      ip_hash: ip ? (await sha256Hex(new TextEncoder().encode(`${env.JWT_SECRET || ''}:${ip}`))).slice(0, 32) : null,
    },
    locations: [], targets: [], forum_targets: [], unresolved: [], hashes: [], pending: [], step_errors: {},
    reviews: [], decision: null, attempts: 0, history: [{ at: receivedAt, event: 'received' }],
  };
  // Each location to a listing or a forum post. A namespace is a website manifest's handle; it names the listing that publishes it.
  for (const l of value.locations) {
    if (l.object === 'forum_post') {
      // Forum storage is not read here, so a post id is locked as named; one the link could not name is not found.
      if (!l.forum_post_id) {
        rec.locations.push({ location: l.location, forum_post_id: null, thread_id: l.thread_id || null, object: l.object, result: 'not_found' });
        rec.unresolved.push({ location: l.location, reason: l.not_found || 'no forum post at this location' });
        continue;
      }
      rec.locations.push({ location: l.location, forum_post_id: l.forum_post_id, thread_id: l.thread_id, object: l.object, result: 'pending' });
      if (!forumTargetOf(rec, l.forum_post_id)) rec.forum_targets.push({ post_id: l.forum_post_id, thread_id: l.thread_id, via: 'request', locked_at: null, restored_at: null });
      continue;
    }
    let listingId = l.listing_id || null;
    if (!listingId && l.namespace) {
      try { listingId = JSON.parse((await env.SOCIAL.get(`website-ns:${l.namespace}`)) || 'null')?.sim_id || null; } catch { listingId = null; }
      if (listingId && !UUID.test(listingId)) listingId = null;
    }
    const sim = listingId ? await readSim(env, listingId) : null;
    if (!sim) {
      rec.locations.push({ location: l.location, listing_id: null, object: l.object, result: 'not_found' });
      rec.unresolved.push({ location: l.location, reason: 'no listing at this location' });
      continue;
    }
    rec.locations.push({ location: l.location, listing_id: listingId, object: l.object, result: 'pending' });
    if (!targetOf(rec, listingId)) rec.targets.push({ sim_id: listingId, author_id: sim.author_id || null, via: 'request', objects: [l.object], locked_at: null, restored_at: null });
    else if (!targetOf(rec, listingId).objects.includes(l.object)) targetOf(rec, listingId).objects.push(l.object);
  }
  const ids = rec.targets.map((t) => t.sim_id);
  const posts = rec.forum_targets.map((t) => t.post_id);
  // The locks first, every one, before anything slower.
  rec.pending = [...ids.map((s) => `lock:${s}`), ...posts.map((p) => `forum_lock:${p}`), ...ids.map((s) => `case:${s}`), ...(rec.possible_csam ? ids.map((s) => `seal:${s}`) : []), ...ids.map((s) => `hashes:${s}`), ...(ids.length ? ['scan'] : []), ...ids.map((s) => `notice:${s}`), 'receipt', 'staff'];
  // Nothing on Eustress at any location: a person looks at it.
  if (!ids.length && !posts.length) rec.status = 'under_review';
  await env.SOCIAL.put(tdKey(id), JSON.stringify(rec));
  if (accountId) await env.SOCIAL.put(filedByKey(accountId, id), id);
  return rec;
}

// -----------------------------------------------------------------------------
// A person's review
// -----------------------------------------------------------------------------

/// Asked by a party through the account page (src/case_history.mjs): the author of a listing the takedown removed, or the requester
/// who filed it while signed in. It changes nothing on its own; it puts the case in front of a person.
export async function requestTakedownReview(env, rec, { party, statement }, deps, url) {
  if (DECIDED.includes(rec.status)) return { ok: false, code: 'decided', error: `This case was decided. Write to ${SUPPORT} with the reference if you have new information.` };
  rec.reviews = rec.reviews || [];
  if (rec.reviews.some((r) => r.party === party && !r.answered_at)) return { ok: false, code: 'already_asked', error: 'A review was already asked for. A person will look at it.' };
  const prev = rec.status;
  rec.reviews.push({ party, statement: statement.slice(0, STATEMENT_MAX), at: nowIso() });
  rec.status = 'under_review';
  record(rec, 'review_requested', { party });
  rec.pending.push('staff_review');
  await runSteps(env, rec, contextFor(env, deps, url));
  await saveTakedown(env, rec, prev);
  return { ok: true, status: takedownPublicStatus(rec) };
}

async function decide(env, rec, action, adminId, noteText, deps, url) {
  const ctx = contextFor(env, deps, url);
  ctx.open.set(rec.id, rec);
  const prev = rec.status;
  const at = nowIso();
  if (action === 'confirm') {
    if (!['received', 'removed', 'under_review'].includes(rec.status)) return { status: 409, body: { error: `This case is ${rec.status} and cannot be confirmed.`, code: 'not_open' } };
    rec.status = 'confirmed';
    rec.pending.push('decision_notice');
  } else if (action === 'invalid') {
    if (!['received', 'removed', 'under_review', 'confirmed'].includes(rec.status)) return { status: 409, body: { error: `This case is ${rec.status}.`, code: 'not_open' } };
    rec.status = 'invalid';
    // What the removal still owed is moot now: no lock is written after a person ordered the content restored.
    rec.pending = rec.pending.filter((s) => !REMOVAL_STEP(s));
    const live = rec.targets.filter((t) => !t.restored_at).map((t) => t.sim_id);
    const posts = (rec.forum_targets || []).filter((t) => !t.restored_at).map((t) => t.post_id);
    rec.pending.push(...live.map((s) => `restore:${s}`), ...posts.map((p) => `forum_restore:${p}`), 'unblock', ...live.map((s) => `restored_notice:${s}`), 'decision_notice');
  } else {
    if (DECIDED.includes(rec.status)) return { status: 409, body: { error: `This case is ${rec.status}.`, code: 'not_open' } };
    if (rec.targets.some((t) => !t.restored_at) || (rec.forum_targets || []).some((t) => !t.restored_at)) return { status: 409, body: { error: 'This case removed listings: confirm it, or mark it invalid to restore them.', code: 'has_listings' } };
    rec.status = 'closed';
    rec.pending.push('decision_notice');
  }
  rec.decided_at = at;
  rec.decision = { outcome: rec.status, note: noteText, by: adminId, at };
  for (const r of rec.reviews || []) if (!r.answered_at) r.answered_at = at;
  record(rec, `decided_${rec.status}`, { by: adminId });
  await runSteps(env, rec, ctx);
  await saveTakedown(env, rec, prev);
  if (ctx.locksWritten) await refreshServingCaches(env, ctx);
  try { await deps.auditLog?.(env, `TAKEDOWN_${rec.status.toUpperCase()}`, adminId, rec.reference, { case_id: rec.id, note: noteText }); }
  catch (e) { console.error('takedown: the audit log write failed:', e?.message || e); }
  return { status: 200, body: { ok: true, case: adminView(rec), incomplete: rec.pending } };
}

function adminView(rec) {
  return { ...rec, public_status: takedownPublicStatus(rec), deadline: deadlineOf(rec), overdue: !rec.removed_at && (rec.targets.some((t) => t.via === 'request') || !!rec.forum_targets?.length) && Date.now() > Date.parse(deadlineOf(rec)) };
}

async function adminRoute(request, url, env, cors, deps) {
  const json = deps.json;
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  if (url.pathname === '/api/admin/takedowns') {
    if (request.method !== 'GET') return json({ error: 'Method not allowed' }, 405, cors);
    const asked = url.searchParams.get('status') || 'open';
    const statuses = asked === 'open' ? ['received', 'removed', 'under_review'] : asked === 'all' ? TAKEDOWN_STATUSES : TAKEDOWN_STATUSES.includes(asked) ? [asked] : null;
    if (!statuses) return json({ error: `status must be open, all, or one of ${TAKEDOWN_STATUSES.join(', ')}` }, 400, cors);
    const limit = Math.min(200, Math.max(1, parseInt(url.searchParams.get('limit') || '50', 10) || 50));
    const items = [];
    for (const status of statuses) {
      const listed = await env.SOCIAL.list({ prefix: `tdq:${status}:`, limit });
      for (const k of listed?.keys || []) {
        const rec = await loadTakedown(env, k.name.slice(`tdq:${status}:`.length));
        if (!rec || rec.status !== status) continue;
        items.push({
          id: rec.id, reference: rec.reference, status: rec.status, possible_csam: !!rec.possible_csam, received_at: rec.received_at, removed_at: rec.removed_at,
          listings: rec.targets.length, forum_posts: (rec.forum_targets || []).length, not_found: rec.unresolved.length, pending: rec.pending, reviews: (rec.reviews || []).length, deadline: deadlineOf(rec),
        });
      }
    }
    // Possible CSAM first, then the oldest: the order a person should work them in.
    items.sort((a, b) => (b.possible_csam - a.possible_csam) || (a.received_at < b.received_at ? -1 : 1));
    return json({ items: items.slice(0, limit) }, 200, headers);
  }
  const m = /^\/api\/admin\/takedowns\/([a-f0-9-]{36})(\/(confirm|invalid|close))?$/.exec(url.pathname);
  if (!m) return json({ error: 'Unknown takedown route' }, 404, cors);
  const rec = await loadTakedown(env, m[1]);
  if (!rec) return json({ error: 'No such takedown' }, 404, cors);
  if (!m[3]) {
    if (request.method !== 'GET') return json({ error: 'Method not allowed' }, 405, cors);
    return json({ case: adminView(rec) }, 200, headers);
  }
  if (request.method !== 'POST') return json({ error: 'Method not allowed' }, 405, cors);
  let body = {};
  try { body = await request.json(); } catch { body = {}; }
  const min = m[3] === 'invalid' ? 20 : 10;
  const noteText = typeof body.note === 'string' ? body.note.trim() : '';
  if (noteText.length < min || noteText.length > 4000) return json({ error: `note is required: ${min} to 4000 characters saying why`, code: 'note_required' }, 400, cors);
  const result = await decide(env, rec, m[3], adminId, noteText, deps, url);
  return json(result.body, result.status, headers);
}

// -----------------------------------------------------------------------------
// The sweep
// -----------------------------------------------------------------------------

/// Hourly, from scheduled() in index.js: every case that owes a step runs it again, staff are emailed while a removal step is
/// owed (every run) or another step keeps failing (on the 3rd, 12th and 24th run, then daily), and the identical-copy scan goes
/// on. It finishes what a request or a person's decision started; it never restores anything on its own.
export async function sweepTakedowns(env, deps = {}, { cases = 50, scanPages = 6 } = {}) {
  const out = { cases: 0, completed: 0, still_pending: 0, removal_owed: 0, copies_locked: 0, copy_candidates: 0, scan_done: null };
  const ctx = contextFor(env, deps, null);
  const listed = await env.SOCIAL.list({ prefix: 'tdpending:', limit: cases });
  for (const k of listed?.keys || []) {
    const rec = await loadTakedown(env, k.name.slice('tdpending:'.length));
    if (!rec?.pending?.length) { await env.SOCIAL.delete(k.name).catch(() => {}); continue; }
    out.cases += 1;
    const prev = rec.status;
    rec.attempts = (rec.attempts || 0) + 1;
    ctx.open = new Map([[rec.id, rec]]);
    await runSteps(env, rec, ctx);
    await saveTakedown(env, rec, prev);
    if (!rec.pending.length) { out.completed += 1; continue; }
    out.still_pending += 1;
    const removalOwed = rec.pending.some(LOCK_STEP);
    if (removalOwed) out.removal_owed += 1;
    if (removalOwed || [3, 12, 24].includes(rec.attempts) || rec.attempts % 24 === 0) {
      await alertStaff(env, renderMail('staff_failure', { ...staffFacts(rec, ctx), deadline: deadlineOf(rec) })).catch((e) => console.error('takedown: alert failed:', e?.message || e));
    }
  }
  ctx.open = new Map();
  out.relocked = await enforceLocks(env, ctx);
  const scan = await scanForCopies(env, ctx, { pages: scanPages });
  out.copies_locked = scan.locked;
  out.copy_candidates = scan.candidates;
  out.scan_done = scan.done;
  if (ctx.locksWritten) await refreshServingCaches(env, ctx);
  return out;
}

/// A takedown in force keeps its listings locked whatever else moved the lock (a person releasing a moderation hold on the same
/// listing, a lock removed by hand): a lock that is gone, or lowered to a content lock, is written again with the case's hold,
/// and staff are told. A forum post's `forum-post-lock:` key that is gone is written again the same way. Only a person's
/// `invalid` decision ends a takedown's lock.
async function enforceLocks(env, ctx, limit = 200) {
  let relocked = 0;
  // A page a run, carrying on where the last stopped, so every case in force is looked at however many there are.
  const cursor = (await env.SOCIAL.get(ENFORCE_CURSOR_KEY)) || undefined;
  const active = await env.SOCIAL.list({ prefix: 'tdactive:', limit, ...(cursor ? { cursor } : {}) });
  if (active?.list_complete === false && active.cursor) await env.SOCIAL.put(ENFORCE_CURSOR_KEY, active.cursor);
  else if (cursor) await env.SOCIAL.delete(ENFORCE_CURSOR_KEY).catch(() => {});
  for (const k of active?.keys || []) {
    const rec = await loadTakedown(env, k.name.slice('tdactive:'.length));
    if (!rec || ['invalid', 'closed'].includes(rec.status)) continue;
    const prev = rec.status;
    let moved = 0;
    for (const t of rec.targets) {
      // A target whose lock was never written still owes its lock step, which the loop above runs.
      if (t.restored_at || !t.locked_at) continue;
      const head = await env.SCENES.head(legalLockKey(t.sim_id));
      if (head && head.customMetadata?.kind !== 'content_lock') continue;
      for (const step of [`lock:${t.sim_id}`, `case:${t.sim_id}`]) if (!rec.pending.includes(step)) rec.pending.push(step);
      record(rec, 'lock_restored', { sim_id: t.sim_id });
      moved += 1;
    }
    // A forum post's lock that disappeared is written again.
    for (const t of rec.forum_targets || []) {
      if (t.restored_at || !t.locked_at) continue;
      if (await env.SOCIAL.get(forumPostLockKey(t.post_id))) continue;
      if (!rec.pending.includes(`forum_lock:${t.post_id}`)) rec.pending.push(`forum_lock:${t.post_id}`);
      record(rec, 'lock_restored', { post_id: t.post_id });
      moved += 1;
    }
    if (!moved) continue;
    relocked += moved;
    ctx.open = new Map([[rec.id, rec]]);
    await runSteps(env, rec, ctx);
    await saveTakedown(env, rec, prev);
    await alertStaff(env, renderMail('staff_relocked', staffFacts(rec, ctx))).catch((e) => console.error('takedown: alert failed:', e?.message || e));
  }
  ctx.open = new Map();
  return relocked;
}

// -----------------------------------------------------------------------------
// What the parties see (src/case_history.mjs)
// -----------------------------------------------------------------------------

/// The status anyone holding the reference may see: received, removed, under_review, decided or closed.
export function takedownPublicStatus(rec) {
  return { received: 'received', removed: 'removed', under_review: 'under_review', confirmed: 'decided', invalid: 'decided', closed: 'closed' }[rec?.status] || 'received';
}

const OUTCOMES = { confirmed: 'removal_upheld', invalid: 'content_restored', closed: 'closed_no_action' };

/// The minimal status of a takedown by its reference: the status and the dates, nothing else.
export function takedownStatusView(rec) {
  return { reference: rec.reference, kind: 'takedown', status: takedownPublicStatus(rec), received_at: rec.received_at, removed_at: rec.removed_at || null, decided_at: rec.decided_at || null };
}

/// Which party of the case this account is, or null: 'author' of a listing it removed (never for a possible_csam case), or
/// 'requester' when it filed the request while signed in.
export function takedownPartyOf(rec, accountId) {
  if (!accountId) return null;
  if (authorVisible(rec) && rec.targets.some((t) => t.author_id === accountId)) return 'author';
  if (rec.request?.account_id === accountId) return 'requester';
  return null;
}

/// The takedown entries of the account's case history, newest first: one per listing of theirs a takedown removed, and one per
/// request they filed while signed in. The requester is never named to an author, nor the author to a requester; neither sees
/// the request's description, the staff's note or who decided.
export async function takedownEntriesFor(env, accountId) {
  const entries = [];
  const against = await env.SOCIAL.list({ prefix: `tdagainst:${accountId}:`, limit: 200 });
  for (const k of against?.keys || []) {
    const rec = await loadTakedown(env, k.name.slice(`tdagainst:${accountId}:`.length));
    if (!rec || !authorVisible(rec)) continue;
    for (const target of rec.targets.filter((t) => t.author_id === accountId && t.locked_at)) {
      const sim = await readSim(env, target.sim_id);
      const decided = !!target.restored_at || DECIDED.includes(rec.status);
      entries.push({
        reference: rec.reference, kind: 'takedown_notice',
        listing: { id: target.sim_id, name: sim?.name || null },
        status: target.restored_at ? 'decided' : takedownPublicStatus(rec),
        outcome: target.restored_at ? 'content_restored' : (OUTCOMES[rec.status] || null),
        headline: AUTHOR_HEADLINES[target.restored_at ? 'restored' : rec.status === 'confirmed' ? 'upheld' : target.via === 'identical_copy' ? 'removed_copy' : 'removed'],
        opened_at: target.locked_at, removed_at: target.locked_at, decided_at: decided ? (target.restored_at || rec.decided_at) : null, updated_at: rec.updated_at,
        feedback: {
          reasons: [{
            title: 'Removed under the TAKE IT DOWN Act',
            why: `${target.via === 'identical_copy' ? 'This listing holds a file identical to one a request named. ' : 'A request named this listing. '}${LAW_LINE}`,
            what_to_change: decided ? '' : 'If you believe this is a mistake, ask for a person to review it. Nothing is restored automatically.',
          }],
          articles: [], tests: [], suggested_edit: null, note: null,
        },
        can_request_review: !decided && !(rec.reviews || []).some((r) => r.party === 'author' && !r.answered_at),
      });
    }
  }
  const filed = await env.SOCIAL.list({ prefix: `tdby:${accountId}:`, limit: 200 });
  for (const k of filed?.keys || []) {
    const rec = await loadTakedown(env, k.name.slice(`tdby:${accountId}:`.length));
    if (!rec || rec.request?.account_id !== accountId) continue;
    const decided = DECIDED.includes(rec.status);
    entries.push({
      reference: rec.reference, kind: 'takedown_request', listing: null,
      locations: (rec.locations || []).map(({ location, result }) => ({ location, result })),
      status: takedownPublicStatus(rec), outcome: OUTCOMES[rec.status] || null,
      headline: decided ? 'A person reviewed your request.' : rec.status === 'removed' ? 'We removed the content you named.' : 'We received your request.',
      opened_at: rec.received_at, removed_at: rec.removed_at || null, decided_at: rec.decided_at || null, updated_at: rec.updated_at,
      feedback: null,
      can_request_review: !decided && !(rec.reviews || []).some((r) => r.party === 'requester' && !r.answered_at),
    });
  }
  return entries;
}

// -----------------------------------------------------------------------------
// HTTP
// -----------------------------------------------------------------------------

/// A Response for a takedown route, or null for any other path. `deps`: verifyAuth, requireAdmin, json, auditLog.
export async function handleTakedownRoute(request, url, env, cors, deps) {
  if (url.pathname === '/api/takedown/requests') {
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return fileRequest(request, url, env, cors, deps);
  }
  if (url.pathname === '/api/admin/takedowns' || url.pathname.startsWith('/api/admin/takedowns/')) return adminRoute(request, url, env, cors, deps);
  return null;
}
