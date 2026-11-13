// =============================================================================
// Listing-review latency, for staff (Technical Document 4.3)
// =============================================================================
//
// "Under a minute" cannot be held to account without a number. This module measures it from what the moderation case
// already records, and reports aggregates only: count, median, p90 and max, over the last 24 hours and the last 7 days.
// No listing id, author id, name or reference is ever part of an answer.
//
// What is measured (all from the case `modcase:{simId}`, src/moderation.mjs):
//   submitted_to_in_review   rec.review_clock.submitted_at -> rec.review_clock.started_at
//                            (the author's submit request -> the review run starting; a publish-triggered run only)
//   in_review_to_decided     rec.review_clock.started_at -> rec.review_clock.decided_at, grouped by the decision's outcome
//                            (approve, reject, changes_requested, quarantine, hold)
//   appeal_to_outcome        rec.appeal.at -> rec.appeal.resolved_at, grouped by overturned / upheld
// `review_clock` is written by moderation.mjs (runLeased at the run's start for a publish; setStatus at the decision).
// A sample belongs to a window by the time it ENDED (started_at, decided_at, resolved_at), so a decision made 2 hours ago
// counts in the last 24 hours however long its review ran.
//
// The scan is bounded: at most MAX_LIST_PAGES list calls and MAX_CASE_READS case reads per run, never unbounded KV. The
// queue keys (`modq:{status}:{simId}`) carry the case's updated_at as metadata, so a case not touched in 7 days is skipped
// without a read; a key from before that metadata is read within the same cap. `scan.truncated` says when a cap was hit.

export const WINDOWS = Object.freeze({ last_24h: 24 * 3600 * 1000, last_7d: 7 * 24 * 3600 * 1000 });
export const QUEUE_STATUSES = Object.freeze(['pending', 'classifying', 'approved', 'held', 'rejected', 'changes_requested', 'quarantined', 'appealed']);
export const MAX_LIST_PAGES = 24;
export const MAX_CASE_READS = 200;
export const LIST_PAGE_SIZE = 1000;

const ms = (iso) => {
  if (typeof iso !== 'string') return null;
  const t = Date.parse(iso);
  return Number.isFinite(t) ? t : null;
};

/// Nearest-rank percentile of an ascending array (p in 0..1).
export function percentile(sorted, p) {
  if (!sorted.length) return null;
  const rank = Math.min(sorted.length, Math.max(1, Math.ceil(p * sorted.length)));
  return sorted[rank - 1];
}

/// Count, median, p90 and max, in whole seconds (null when there is no sample). Median of an even count is the mean of the
/// two middle values.
export function summarize(valuesMs) {
  const v = valuesMs.filter((x) => Number.isFinite(x) && x >= 0).sort((a, b) => a - b);
  if (!v.length) return { count: 0, median_seconds: null, p90_seconds: null, max_seconds: null };
  const mid = v.length >> 1;
  const median = v.length % 2 ? v[mid] : (v[mid - 1] + v[mid]) / 2;
  return {
    count: v.length,
    median_seconds: Math.round(median / 100) / 10,
    p90_seconds: Math.round(percentile(v, 0.9) / 100) / 10,
    max_seconds: Math.round(v[v.length - 1] / 100) / 10,
  };
}

const OUTCOMES = ['approve', 'reject', 'changes_requested', 'quarantine', 'hold'];
const APPEAL_OUTCOMES = ['overturned', 'upheld'];

/// The samples one case gives: `{ metric, group, end, ms }`. Pure.
export function samplesOf(rec) {
  const out = [];
  if (!rec || typeof rec !== 'object') return out;
  const c = rec.review_clock;
  if (c && typeof c === 'object') {
    const sub = ms(c.submitted_at), start = ms(c.started_at), dec = ms(c.decided_at);
    if (sub !== null && start !== null && start >= sub) out.push({ metric: 'submitted_to_in_review', group: 'all', end: start, ms: start - sub });
    if (start !== null && dec !== null && dec >= start) {
      const o = OUTCOMES.includes(rec.decision?.outcome) ? rec.decision.outcome : 'other';
      out.push({ metric: 'in_review_to_decided', group: o, end: dec, ms: dec - start });
    }
  }
  const a = rec.appeal;
  if (a && typeof a === 'object') {
    const at = ms(a.at), res = ms(a.resolved_at);
    if (at !== null && res !== null && res >= at && APPEAL_OUTCOMES.includes(a.status)) out.push({ metric: 'appeal_to_outcome', group: a.status, end: res, ms: res - at });
  }
  return out;
}

/// Aggregates from samples at `now`. A sample counts in a window when `now - window <= end <= now`. Pure.
export function aggregate(samples, now) {
  const result = { submitted_to_in_review: {}, in_review_to_decided: {}, appeal_to_outcome: {} };
  const groups = { submitted_to_in_review: ['all'], in_review_to_decided: ['all', ...OUTCOMES, 'other'], appeal_to_outcome: ['all', ...APPEAL_OUTCOMES] };
  for (const [metric, names] of Object.entries(groups)) {
    for (const name of names) {
      const row = {};
      for (const [w, span] of Object.entries(WINDOWS)) {
        const inWin = samples.filter((s) => s.metric === metric && (name === 'all' || s.group === name) && s.end <= now && s.end >= now - span).map((s) => s.ms);
        row[w] = summarize(inWin);
      }
      result[metric][name] = row;
    }
  }
  // A group with no sample in either window is left out, so the answer stays short.
  for (const metric of Object.keys(result)) {
    for (const name of Object.keys(result[metric])) {
      const r = result[metric][name];
      if (name !== 'all' && !r.last_24h.count && !r.last_7d.count) delete result[metric][name];
    }
  }
  return result;
}

/// Reads the cases within the caps and returns `{ generated_at, windows, ...metrics, scan }`. Throws if the queue cannot be
/// listed at all. `env.SOCIAL` is the KV namespace.
export async function reviewLatency(env, { now = Date.now(), maxPages = MAX_LIST_PAGES, maxReads = MAX_CASE_READS } = {}) {
  const horizon = now - WINDOWS.last_7d;
  const scan = { listed: 0, skipped_old: 0, read: 0, truncated: false };
  const samples = [];
  let pages = 0;
  const todo = [];
  for (const status of QUEUE_STATUSES) {
    let cursor;
    for (;;) {
      if (pages >= maxPages) { scan.truncated = true; break; }
      pages += 1;
      const listed = await env.SOCIAL.list({ prefix: `modq:${status}:`, limit: LIST_PAGE_SIZE, ...(cursor ? { cursor } : {}) });
      for (const k of listed?.keys || []) {
        scan.listed += 1;
        const u = ms(k.metadata?.u);
        if (u !== null && u < horizon) { scan.skipped_old += 1; continue; }
        todo.push({ simId: k.name.slice(`modq:${status}:`.length), known: u !== null });
      }
      if (listed?.list_complete !== false || !listed?.cursor) break;
      cursor = listed.cursor;
    }
  }
  // Keys known to be recent are read first; the ones from before the metadata fill what is left of the cap.
  todo.sort((a, b) => Number(b.known) - Number(a.known));
  const seen = new Set();
  for (const t of todo) {
    if (seen.has(t.simId)) continue;
    seen.add(t.simId);
    if (scan.read >= maxReads) { scan.truncated = true; break; }
    scan.read += 1;
    let rec = null;
    try { const raw = await env.SOCIAL.get(`modcase:${t.simId}`); rec = raw ? JSON.parse(raw) : null; } catch { rec = null; }
    samples.push(...samplesOf(rec));
  }
  return { generated_at: new Date(now).toISOString(), windows: { last_24h: '24 hours', last_7d: '7 days' }, unit: 'seconds', ...aggregate(samples, now), scan };
}

/// Plain lines for the staff digest: one line per measure with a sample. Counts and seconds only.
export function digestLines(report) {
  const fmt = (s) => (s.count ? `${s.count} done, median ${s.median_seconds}s, p90 ${s.p90_seconds}s, max ${s.max_seconds}s` : 'no samples');
  const rows = [];
  const add = (label, row) => { if (row) rows.push([label, `24h: ${fmt(row.last_24h)}; 7d: ${fmt(row.last_7d)}`]); };
  add('Review latency: submitted to in review', report.submitted_to_in_review?.all);
  add('Review latency: in review to decided', report.in_review_to_decided?.all);
  for (const [name, row] of Object.entries(report.in_review_to_decided || {})) if (name !== 'all') add(`  decided as ${name}`, row);
  add('Review latency: appeal to outcome', report.appeal_to_outcome?.all);
  if (report.scan?.truncated) rows.push(['Review latency scan', 'capped: the numbers cover part of the cases']);
  return rows;
}

/// GET /api/staff/review-latency, administrator only. `deps`: `{ json, requireAdmin }`. Returns null for any other path.
export async function handleReviewLatencyRoute(request, url, env, cors, deps) {
  if (url.pathname !== '/api/staff/review-latency') return null;
  const { json, requireAdmin } = deps;
  if (request.method !== 'GET') return json({ error: 'Method not allowed' }, 405, cors);
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  if (!env.SOCIAL) return json({ error: 'This is not available on this deployment', code: 'not_configured' }, 501, cors);
  try { return json(await reviewLatency(env), 200, { ...cors, 'Cache-Control': 'no-store' }); }
  catch (e) { console.error('review latency: not read:', e?.name || 'Error'); return json({ error: 'The review latency could not be read', code: 'unavailable' }, 503, cors); }
}
