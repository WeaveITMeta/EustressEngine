// =============================================================================
// Case history: what an account, and a reference's holder, may see of a case
// =============================================================================
//
//   GET  /api/account/cases?limit=&cursor=        session   the account's own cases, newest first
//   POST /api/account/cases/{reference}/review    session   a party of a takedown asks a person to review it
//   GET  /api/cases/{reference}                   public    the minimal status of one case, by its reference
//
// The account's cases are: the reviews of its own listings (moderation cases),
// the takedowns of its listings, and the takedowns it filed while signed in.
// Each entry carries its reference (src/case_reference.mjs), what it is about,
// its status and outcome, its dates, and the author-safe feedback. A review's
// feedback comes from the one catalogue every author surface uses (authorView in
// src/moderation.mjs): the reasons in author copy, the Code articles and tests
// those reasons cite, the suggested edit, and the tags the review chose when the
// author gave none ("Tags chosen for your Space: ..."). Nothing else from the case: never
// who reviewed or decided, never a score or a probability, never the other party
// of a takedown, never the evidence. A hold reads as a review in progress, as it
// does everywhere an author looks.
//
// The status by reference is for someone with no account (a takedown's
// requester) and says only where the case is: received, removed, under_review,
// decided or closed, with the dates. It names no listing and no person.
//
// Forum cases (src/moderation.mjs fileForumReport) give two kinds of entry:
//   - forum_report: a report the account filed. Its status only (received, or
//     decided once a person recorded a decision), with the post and the category the
//     account cited; never the outcome, which is a decision about another person.
//   - forum_notice: a case about the account's own post, once the post is hidden,
//     removed, gated or decided, and not before. It says what happened to the post
//     and how to ask for it back, and never who reported it, how many did or what
//     they wrote. It is the latest of the entries the case has shown its author
//     (`shown_entries`, written by src/moderation.mjs as each one is shown): an entry
//     once shown is never withdrawn and never changed by a later report or seal; only
//     a person's later decision shows a new one. The author always sees what happened
//     to the post (the thread page shows it), so no outcome is ever left without an
//     entry, and no entry tells csam from nonconsensual_sexual: every report hide, of any
//     category, is one generic entry, and a person's removal, a seal or a refusal at
//     posting in csam or nonconsensual_sexual is one generic entry, with no category and
//     no article. Other categories keep their named entries. No forum case sends the
//     author any other notice.
//
// Every answer is private and never stored by a cache.
// =============================================================================

import { loadCase, authorView, authorStatusOf, loadForumCase, forumShownEntriesOf } from './moderation.mjs';
import { assignCaseReference, resolveCaseReference, normalizeCaseReference } from './case_reference.mjs';
import { loadTakedown, takedownStatusView, takedownEntriesFor, takedownPartyOf, requestTakedownReview, STATEMENT_MIN, STATEMENT_MAX } from './takedown.mjs';

export const HISTORY_PAGE_DEFAULT = 20;
export const HISTORY_PAGE_MAX = 50;
/// How many of the account's listings are read for their reviews (the projects page reads the same number).
const LISTINGS_READ = 100;

const DECIDED_REVIEW = ['listed', 'approved_private', 'not_listed', 'changes_requested', 'removed'];
// The Code a catalogue title cites: "Hollow loop (Code B5)", "Objective (Code T2)", "(Code Part III)".
const CITED = /\(Code ((?:[AB]\d{1,2})|(?:T[1-4])|(?:Part [IVX]+))\)/;

const parse = (raw) => { try { return raw ? JSON.parse(raw) : null; } catch { return null; } };

/// The case as its author may read it. A hold that only a TAKE IT DOWN request placed is told to the author as its own entry
/// (a takedown_notice), so the review reads as it was decided. Every other hold, and a takedown flagged as possibly showing a
/// minor, reads as a review in progress, as it does everywhere an author looks.
const asAuthorReads = (rec) => (rec?.legal_hold?.source === 'takedown' && !rec.legal_hold.possible_csam && rec.status !== 'quarantined' ? { ...rec, legal_hold: null } : rec);

/// One review of the account's own listing, as its author sees it.
export function reviewEntry(reference, rec, sim) {
  const view = authorView(asAuthorReads(rec), sim);
  const decided = DECIDED_REVIEW.includes(view.status);
  const cited = [...new Set(view.reasons.map((r) => CITED.exec(r.title || '')?.[1]).filter(Boolean))];
  return {
    reference,
    kind: 'listing_review',
    listing: { id: sim.id, name: sim.name || null },
    status: view.status,
    outcome: decided ? view.status : null,
    headline: view.headline,
    opened_at: rec.created_at || null,
    decided_at: decided ? (view.updated_at || null) : null,
    updated_at: view.updated_at || null,
    feedback: {
      reasons: view.reasons.map(({ title, why, what_to_change }) => ({ title, why, what_to_change })),
      articles: cited.filter((c) => !c.startsWith('T')),
      tests: cited.filter((c) => c.startsWith('T')),
      suggested_edit: view.suggested_edit,
      note: view.note,
      // The tags the review chose because the author gave none, and the line that says so (Code v2.3 Part III).
      assigned_tags: view.assigned_tags,
      tags_chosen: view.tags_chosen,
    },
    can_appeal: view.can_appeal,
    appeal: view.appeal,
  };
}

const FORUM_SUPPORT_LINE = 'If you believe this is a mistake, write to support@eustress.dev with the case reference, and a person will look at whether to restore it.';
const FORUM_CATEGORY_WORDS = {
  csam: 'it was sexual content involving a minor (Code A1)',
  nonconsensual_sexual: 'it showed a real person sexually or intimately without their consent (Code A5)',
  personal_info: 'it published someone\'s private information (Code A6)',
  threat: 'it made a threat of violence (Code A2)',
  scam: 'it was a scam (Code A7)',
  spam: 'it was spam (Code B4)',
};

/// The categories whose removal, seal or refusal at posting is told to the author only generically (McKale): the same entry
/// for each, word for word, with no category and no article. The author sees the outcome either way (the thread page shows
/// "Removed by a moderator"), so a named entry for one and none for the other would tell them apart.
export const FORUM_GENERIC_CATEGORIES = Object.freeze(['csam', 'nonconsensual_sexual']);
/// The same list, by its earlier name (a refusal at posting).
export const FORUM_CHECK_GENERIC_CATEGORIES = FORUM_GENERIC_CATEGORIES;
/// The one entry for a post hidden or removed by reports, whatever the category.
export const FORUM_REPORT_HIDDEN_HEADLINE = 'Your forum post was hidden after a report while a person reviews it.';
/// The one entry for a person's removal or a seal in csam or nonconsensual_sexual.
export const FORUM_GENERIC_REMOVED_HEADLINE = 'Your forum post was removed while a person reviews it.';

/// A report the account filed, as its reporter sees it: where it is, never what was decided about the post or its author.
export function forumReportEntry(rec, reporterId) {
  const mine = (rec.reports || []).find((r) => r.reporter_id === reporterId) || null;
  // Decided once a person recorded a decision after this report. What reports did on their own (a hide, a removal) is an
  // outcome for the author, so it is not told here.
  const after = mine ? (rec.decisions || []).find((d) => d.round >= mine.round) : null;
  const decided = mine ? !!after : !rec.waiting;
  const decidedAt = after?.at || (decided ? (rec.decisions || []).at(-1)?.at || null : null);
  return {
    reference: rec.reference || null,
    kind: 'forum_report',
    post: { id: rec.post_id, thread_id: rec.thread_id },
    category: mine?.category || null,
    status: decided ? 'decided' : 'received',
    outcome: null,
    headline: decided ? 'A person looked at your report. Thank you for it.' : 'We received your report. Thank you for it.',
    opened_at: mine?.at || null,
    decided_at: decidedAt,
    updated_at: decidedAt || mine?.at || null,
    feedback: null,
  };
}

// The forum's csam silence (no entry at all while a post was reported, removed or sealed as csam) is retired (McKale,
// 2026-10-15): the author sees the outcome on the thread page either way, so "something happened and no entry" was itself the
// tell. Every outcome now has an entry, and the generic ones say the same for csam as for nonconsensual_sexual.

/// What a shown entry says, by its kind (src/moderation.mjs showForumEntry): `{ status, outcome, headline, why }`. The
/// generic kinds depend on nothing but their kind, so they are byte-identical whatever the category behind them.
export function forumEntryText(entry) {
  const words = (category) => (FORUM_GENERIC_CATEGORIES.includes(category) ? null : FORUM_CATEGORY_WORDS[category] || null);
  switch (entry?.kind) {
    case 'report_hidden':
      return { status: 'under_review', outcome: 'hidden', headline: FORUM_REPORT_HIDDEN_HEADLINE, why: `It was reported as content the forums do not allow, and a person reviews it. ${FORUM_SUPPORT_LINE}` };
    case 'report_gated':
      return { status: 'under_review', outcome: null, headline: 'Your forum post is shown only behind the Adult content gate while a person reviews it.', why: 'Members reported it as explicit adult content. It stays up for accounts that have turned Adult content on; a person decides.' };
    case 'check_removed': {
      const w = words(entry.category);
      return { status: 'decided', outcome: 'removed', headline: 'Your forum post was removed: it carried content the forums do not allow.', why: `${w ? `It was removed because ${w}. ` : ''}Your account is not affected. ${FORUM_SUPPORT_LINE}` };
    }
    case 'person_removed': {
      if (FORUM_GENERIC_CATEGORIES.includes(entry.category)) return { status: 'under_review', outcome: 'removed', headline: FORUM_GENERIC_REMOVED_HEADLINE, why: `Your account is not affected. ${FORUM_SUPPORT_LINE}` };
      const w = words(entry.category);
      return { status: 'decided', outcome: 'removed', headline: 'A moderator removed your forum post.', why: `${w ? `It was removed because ${w}. ` : ''}${FORUM_SUPPORT_LINE}` };
    }
    case 'person_decided': {
      const outcome = { restore: 'restored', gate: 'gated', ungate: 'ungated' }[entry.decision] || 'decided';
      const headline = { restored: 'A person reviewed your forum post and it is shown again.', gated: 'A person placed your forum post behind the Adult content gate.', ungated: 'A person took your forum post out from behind the Adult content gate.' }[outcome] || 'A person reviewed your forum post.';
      return { status: 'decided', outcome, headline, why: '' };
    }
    default:
      return null;
  }
}

/// A case about the account's own post, as its author sees it: the latest entry the case has shown (forumShownEntriesOf), or
/// null while nothing has happened to the post. Never a reporter, their number or a note.
export function forumNoticeEntry(rec) {
  const entries = forumShownEntriesOf(rec);
  const last = entries.at(-1);
  const text = last ? forumEntryText(last) : null;
  if (!text) return null;
  return {
    reference: rec.reference || null,
    kind: 'forum_notice',
    post: { id: rec.post_id, thread_id: rec.thread_id },
    status: text.status, outcome: text.outcome, headline: text.headline,
    opened_at: entries[0].at || null,
    decided_at: text.status === 'decided' ? last.at || null : null,
    updated_at: last.at || null,
    feedback: { reasons: text.why ? [{ title: 'Eustress Forums', why: text.why, what_to_change: '' }] : [], articles: [], tests: [], suggested_edit: null, note: null },
  };
}

async function accountCases(request, url, env, cors, deps) {
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const auth = await deps.verifyAuth(request, env);
  if (!auth) return deps.json({ error: 'Unauthorized' }, 401, headers);
  const asked = parseInt(url.searchParams.get('limit') || String(HISTORY_PAGE_DEFAULT), 10);
  const limit = Math.min(HISTORY_PAGE_MAX, Math.max(1, Number.isFinite(asked) ? asked : HISTORY_PAGE_DEFAULT));
  const cursorText = url.searchParams.get('cursor') || '0';
  const cursor = /^\d{1,6}$/.test(cursorText) ? parseInt(cursorText, 10) : 0;

  // Each row is sorted by when it last changed, and only the page asked for is built (a review gets its reference then).
  const rows = [];
  const mine = await env.SOCIAL.list({ prefix: `sim-author:${auth}:`, limit: LISTINGS_READ });
  for (const k of mine?.keys || []) {
    const simId = await env.SOCIAL.get(k.name);
    if (!simId) continue;
    const sim = parse(await env.SOCIAL.get(`sim:${simId}`));
    if (!sim || sim.author_id !== auth) continue;
    const rec = await loadCase(env, simId);
    if (!rec) continue;
    rows.push({ at: rec.updated_at || rec.created_at || '', build: async () => reviewEntry(await assignCaseReference(env, 'moderation', simId), rec, { ...sim, id: simId }) });
  }
  for (const entry of await takedownEntriesFor(env, auth)) rows.push({ at: entry.updated_at || entry.opened_at || '', build: async () => entry });
  // Forum cases: the reports this account filed, and the cases about its own posts.
  const withReference = (entry, postId) => async () => ({ ...entry, reference: entry.reference || await assignCaseReference(env, 'forum', postId) });
  for (const [prefix, entryOf] of [[`forumby:${auth}:`, (rec) => (rec.reporters?.includes(auth) ? forumReportEntry(rec, auth) : null)], [`forumagainst:${auth}:`, (rec) => (rec.author_id === auth ? forumNoticeEntry(rec) : null)]]) {
    const listed = await env.SOCIAL.list({ prefix, limit: 200 });
    for (const k of listed?.keys || []) {
      const postId = k.name.slice(prefix.length);
      const rec = await loadForumCase(env, postId);
      const entry = rec ? entryOf(rec) : null;
      if (entry) rows.push({ at: entry.updated_at || entry.opened_at || '', build: withReference(entry, postId) });
    }
  }
  rows.sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0));

  const cases = [];
  for (const row of rows.slice(cursor, cursor + limit)) cases.push(await row.build());
  return deps.json({ cases, total: rows.length, next_cursor: cursor + limit < rows.length ? String(cursor + limit) : null }, 200, headers);
}

const decodedReference = (raw) => { try { return decodeURIComponent(raw); } catch { return ''; } };

async function caseStatus(request, raw, env, cors, deps) {
  const headers = { ...cors, 'Cache-Control': 'private, no-store', 'X-Robots-Tag': 'noindex' };
  const ip = request.headers.get('CF-Connecting-IP');
  if (env.AUTH_RATE_LIMITER && ip) {
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `caseref:${ip}` });
    if (!success) return deps.json({ error: 'Too many lookups. Try again in a minute.', code: 'rate_limited' }, 429, { ...headers, 'Retry-After': '60' });
  }
  const text = decodedReference(raw);
  if (!normalizeCaseReference(text)) return deps.json({ error: 'This is not a case reference. A reference looks like EC-7K2M-9QXD-4TPF-HJ3W-8NBR.', code: 'bad_reference' }, 400, headers);
  const none = () => deps.json({ error: 'No case has this reference.', code: 'not_found' }, 404, headers);
  const found = await resolveCaseReference(env, text);
  if (!found) return none();
  if (found.kind === 'takedown') {
    const rec = await loadTakedown(env, found.id);
    return rec ? deps.json(takedownStatusView(rec), 200, headers) : none();
  }
  if (found.kind === 'forum') {
    // Where the case is, and nothing about the post, its author or its reporters.
    const rec = await loadForumCase(env, found.id);
    if (!rec) return none();
    const decided = !rec.waiting;
    return deps.json({
      reference: found.reference, kind: 'forum', status: decided ? 'decided' : 'received',
      received_at: rec.created_at || null, removed_at: null, decided_at: decided ? ((rec.decisions || []).at(-1)?.at || null) : null,
    }, 200, headers);
  }
  const rec = await loadCase(env, found.id);
  if (!rec) return none();
  const sim = parse(await env.SOCIAL.get(`sim:${found.id}`));
  // The author's status, so a hold reads as a review in progress here too.
  const status = authorStatusOf(rec.status, sim ? sim.is_public !== false : true, !!asAuthorReads(rec).legal_hold);
  const decided = DECIDED_REVIEW.includes(status);
  return deps.json({
    reference: found.reference, kind: 'listing_review',
    status: decided ? 'decided' : status === 'unreviewed' ? 'received' : 'under_review',
    received_at: rec.created_at || null, removed_at: null, decided_at: decided ? (rec.updated_at || null) : null,
  }, 200, headers);
}

async function askForReview(request, url, raw, env, cors, deps) {
  const headers = { ...cors, 'Cache-Control': 'private, no-store' };
  const auth = await deps.verifyAuth(request, env);
  if (!auth) return deps.json({ error: 'Unauthorized' }, 401, headers);
  // One answer for a reference that is not a case and a case that is not this account's.
  const none = () => deps.json({ error: 'No case of yours has this reference.', code: 'not_found' }, 404, headers);
  const found = await resolveCaseReference(env, decodedReference(raw));
  if (!found) return none();
  if (found.kind === 'forum') return deps.json({ error: 'To ask for a forum post to be restored, write to support@eustress.dev with the case reference.', code: 'use_support' }, 409, headers);
  if (found.kind !== 'takedown') return deps.json({ error: 'A review of a listing is appealed from the listing\'s own review.', code: 'use_appeal' }, 409, headers);
  const rec = await loadTakedown(env, found.id);
  const party = rec ? takedownPartyOf(rec, auth) : null;
  if (!party) return none();
  let body = {};
  try { body = await request.json(); } catch { body = {}; }
  const statement = typeof body.statement === 'string' ? body.statement.replace(/\s+/g, ' ').trim() : '';
  if (statement.length < STATEMENT_MIN || statement.length > STATEMENT_MAX) {
    return deps.json({ error: `Say why in ${STATEMENT_MIN} to ${STATEMENT_MAX} characters.`, code: 'invalid_request', missing: ['statement'] }, 400, headers);
  }
  const result = await requestTakedownReview(env, rec, { party, statement }, deps, url);
  if (!result.ok) return deps.json({ error: result.error, code: result.code }, 409, headers);
  return deps.json({ ok: true, reference: rec.reference, status: result.status }, 200, headers);
}

/// A Response for a case route, or null for any other path. `deps`: verifyAuth, json (and what takedown.mjs needs).
export async function handleCaseRoute(request, url, env, cors, deps) {
  const path = url.pathname;
  if (path === '/api/account/cases') {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return accountCases(request, url, env, cors, deps);
  }
  let m;
  if ((m = /^\/api\/account\/cases\/([^/]+)\/review$/.exec(path))) {
    if (request.method !== 'POST') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return askForReview(request, url, m[1], env, cors, deps);
  }
  if ((m = /^\/api\/cases\/([^/]+)$/.exec(path))) {
    if (request.method !== 'GET') return deps.json({ error: 'Method not allowed' }, 405, cors);
    return caseStatus(request, m[1], env, cors, deps);
  }
  return null;
}
