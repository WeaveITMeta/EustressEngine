// =============================================================================
// Staff alerts: their own daily allowance, an hourly digest past it, nothing dropped
// =============================================================================
//
// Every staff alert (src/takedown.mjs alertStaff: TAKE IT DOWN requests, possible CSAM, legal reports, stuck report debts,
// evidence seals) goes to the staff mailboxes through this module. Staff mail never shares an allowance with mail to users:
// the per-account 'notice' bucket of src/mail_limits.mjs is not used for it, so a burst of user notices cannot spend it and a
// burst of staff alerts cannot spend a user's.
//
// Lanes (per staff address, per UTC day; src/mail_limits.mjs MAIL_PER_DAY):
//   general    'staff', STAFF_ALERTS_PER_DAY. A TAKE IT DOWN request received, a review asked for, a failing step, a lock put
//              back, a blocked upload, copies that need a person. An anonymous filer causes these.
//   priority   'staff_priority', STAFF_PRIORITY_ALERTS_PER_DAY, a bucket of its own. Possible CSAM: a TAKE IT DOWN request whose
//              filer says the person shown is a minor (any anonymous filer can say so), a member's report of a forum post as
//              csam, a blocked possible-CSAM file uploaded again, an evidence seal that failed, and any takedown alert about a
//              possible-CSAM case. It never waits behind the general lane.
//   legal      'staff_legal', not capped. A legal report queued or held, and a stuck report debt. Only the system's legal
//              report path writes these (a confident detection, a person's decision, a debt the sweep could not pay); no filing
//              or member report creates one. Each is sent once per report and status, or once a day per stuck debt.
//
// Past a lane's allowance nothing is dropped. The alert goes on the over-cap list of that staff address (USERS
// `staffmail-over:{address}:{alert}`, with `staffmail-queue:{address}:{alert}` while no digest has carried it), and one digest
// an hour (sendStaffDigests, from the hourly takedown sweep) names how many are waiting and their references. A REQUIRED alert
// past the allowance counts as delivered only once a digest that carries it has SENT: until then its caller throws, and the
// caller's retry finds the digest's mark (`digested_at`) on the next try. An alert is identified by `alertId` (stable across
// retries, so a retry finds its own entry); without one it is listed once.
//
// A REQUIRED alert, every priority-lane alert (possible CSAM) and every legal-lane alert (a legal report, a report debt owed
// or stuck) goes on the same list when its send fails for any other reason too (the mail binding failed or threw, or there
// is none), so the digest or the caller's retry carries it and one mail failure never loses it. Its entry's `reason` says
// which (daily_cap or the failure). Only a general-lane alert that is not required is counted failed and dropped.
//
// A digest that cannot be sent (a mail outage) leaves its entries on the queue. Once any entry has waited there 24 hours,
// staff get one failure alert on the legal lane, which no allowance stops (alertOverdueStaffQueue, from the same hourly
// sweep, after the case steps and the digest): how many wait, the oldest's age and the count of each kind, never a reference,
// an account, an address or content. It is sent at most once a day while the queue stays overdue, and tried again every hour
// until one is sent. Entries are kept 30 days, so a queue that stays undelivered is alerted on every day of those 30 before
// any entry can expire. GET /api/admin/legal/staff-alerts shows the same state (staffQueueState), audited before it is
// answered.
//
// Storage (USERS):
//   mailcap:acct-staff:{address}:{day}, mailcap:acct-staff_priority:{address}:{day}   the lanes' counts (src/mail_limits.mjs)
//   staffmail-over:{address}:{alert}     an alert past its lane's allowance, or a required or legal one whose send failed:
//                                        { alert_id, kind, lane, reference, queued_at, required, reason, digested_at }, kept
//                                        30 days so a retry finds it
//   staffmail-queue:{address}:{alert}    the same while no digest has carried it; its KV metadata holds { kind, lane,
//                                        reference, queued_at, required, reason }, so the digest and the queue's state read
//                                        the list alone, never one read per entry
//   staffmail-digest:{address}:{hour}    the hour's digest was sent (one an hour)
//   staffmail-overdue-alerted            when the last overdue-queue alert was sent (kept 2 days; one a day)
// The address and the alert id are keyed by their SHA-256, never stored in a key in the clear.
// =============================================================================

import { sendNotice } from './account_notices.mjs';

/// The lanes, and the mail kind each is counted under (src/mail_limits.mjs).
export const STAFF_LANES = Object.freeze({ general: 'staff', priority: 'staff_priority', legal: 'staff_legal' });
/// A digest is sent at most this often per staff address.
export const STAFF_DIGEST_EVERY_MS = 3600000;
/// The most alerts the digests of one hourly run name, every staff address together (each address gets an equal share, at
/// least one); the rest are counted in each digest and named by the next.
export const STAFF_DIGEST_MAX_ENTRIES = 50;
/// The storage operations one run's digests may spend, however many staff addresses there are (staffDigestPlan).
export const STAFF_DIGEST_RUN_OPS = 200;
// What a digest costs: per address, the list, the hour's mark read, the digest mail's count (read and write) and the mark
// write; per named entry, at most a read (an entry with no metadata), its mark and the queue key's delete.
const DIGEST_ADDRESS_OPS = 5;
const DIGEST_ENTRY_OPS = 3;

/// How one run's digests spend STAFF_DIGEST_RUN_OPS over `addressCount` staff addresses: `addresses`, how many get a digest
/// this run (each costs DIGEST_ADDRESS_OPS and names at least one entry), and `share`, the entries each names, from what the
/// addresses leave of the budget (at most STAFF_DIGEST_MAX_ENTRIES together). Past `addresses`, the run's addresses rotate
/// by the hour, so every address has its digest within a few hours.
export function staffDigestPlan(addressCount) {
  const n = Math.max(0, Math.floor(addressCount) || 0);
  const addresses = Math.min(n, Math.floor(STAFF_DIGEST_RUN_OPS / (DIGEST_ADDRESS_OPS + DIGEST_ENTRY_OPS)));
  if (!addresses) return { addresses: 0, share: 0 };
  const entries = Math.min(STAFF_DIGEST_MAX_ENTRIES, Math.floor((STAFF_DIGEST_RUN_OPS - DIGEST_ADDRESS_OPS * addresses) / DIGEST_ENTRY_OPS));
  return { addresses, share: Math.max(1, Math.floor(entries / addresses)) };
}
/// How long an over-cap entry is kept, so a retry of its alert finds whether a digest carried it. Far longer than
/// STAFF_QUEUE_OVERDUE_MS, so an entry still undelivered is alerted on every day before it can expire.
export const STAFF_OVER_CAP_TTL_SECONDS = 30 * 86400;
/// An entry still on the queue (no digest carried it, and it was not sent) this long is overdue: staff are alerted.
export const STAFF_QUEUE_OVERDUE_MS = 24 * 3600000;
/// While the queue stays overdue, its alert is sent at most this often.
export const STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS = 86400000;
/// The most queue entries one read of the queue's state looks at; past it the state says `at_least`. The read is the list
/// alone (each entry's metadata), one operation per 1000 entries.
export const STAFF_QUEUE_SCAN_MAX = 5000;
/// A queue entry written before entries carried metadata (65a9db55) was kept 7 days: its list `expiration` minus this is
/// when it was queued. One with neither metadata nor an expiration is read, at most STAFF_QUEUE_LEGACY_READS a scan; past
/// that it is counted as `unknown_age`.
const LEGACY_TTL_MS = 7 * 86400000;
export const STAFF_QUEUE_LEGACY_READS = 50;
const QUEUE_ALL = 'staffmail-queue:';
export const STAFF_QUEUE_OVERDUE_ALERT_KEY = 'staffmail-overdue-alerted';

async function sha256Hex(text) {
  const bytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(String(text))));
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
}
const addressTag = async (address) => (await sha256Hex(String(address).toLowerCase())).slice(0, 32);
const alertTag = async (alertId) => (await sha256Hex(alertId)).slice(0, 32);
const parse = (raw) => { try { return raw ? JSON.parse(raw) : null; } catch { return null; } };
const overKey = (addr, alert) => `staffmail-over:${addr}:${alert}`;
const queuePrefix = (addr) => `staffmail-queue:${addr}:`;
const queueKey = (addr, alert) => `${queuePrefix(addr)}${alert}`;
const hourOf = (ms) => new Date(Math.floor(ms / STAFF_DIGEST_EVERY_MS) * STAFF_DIGEST_EVERY_MS).toISOString().slice(0, 13);
const digestKey = (addr, ms) => `staffmail-digest:${addr}:${hourOf(ms)}`;

/// The KV metadata of a queue key: what the digest names and the queue's state reads from the list alone, with no read per
/// entry. Within KV's 1024-byte limit: past 1000 bytes the reference is left out, and the digest reads that entry.
function queueMetadata(entry) {
  const meta = { kind: entry.kind, lane: entry.lane, reference: entry.reference, queued_at: entry.queued_at, required: entry.required, reason: entry.reason };
  if (new TextEncoder().encode(JSON.stringify(meta)).length <= 1000) return meta;
  const { reference, ...rest } = meta;
  return rest;
}
const hasMeta = (k) => typeof k?.metadata?.queued_at === 'string';

/// Sends one staff alert to each of `recipients` in `lane` ('general', 'priority' or 'legal'). Never throws. Past the lane's
/// allowance the alert goes on that address's over-cap list for the digest. `{ sent, queued, digested, failed, delivered }`:
/// `digested` counts the addresses a digest already carried this alert to; `delivered` is true when at least one address has
/// it (sent now, or carried by a digest that sent). An alert already queued for an address and not yet carried is sent
/// directly when the allowance allows (a new day), and leaves the list. A REQUIRED, priority-lane or legal-lane alert whose
/// send fails for any other reason goes on the list too (`queueOnFailure`, which the overdue-queue alert sets false: it describes the list).
export async function sendStaffAlert(env, recipients, rendered, { lane = 'general', alertId = null, kind = null, reference = null, required = false, fromName, queueOnFailure = null } = {}) {
  const out = { sent: 0, queued: 0, digested: 0, failed: 0, delivered: false };
  const mailKind = STAFF_LANES[lane] || STAFF_LANES.general;
  const holdOnFailure = typeof queueOnFailure === 'boolean' ? queueOnFailure : (required === true || lane === 'legal' || lane === 'priority');
  const id = typeof alertId === 'string' && alertId ? alertId : `once:${crypto.randomUUID()}`;
  const alert = await alertTag(id);
  for (const to of recipients) {
    const addr = await addressTag(to);
    let held = null;
    try { held = env.USERS ? parse(await env.USERS.get(overKey(addr, alert))) : null; } catch { held = null; }
    if (held?.digested_at) { out.digested += 1; continue; }
    let r;
    try { [r] = await sendNotice(env, [to], rendered, { kind: mailKind, ...(fromName ? { fromName } : {}) }); }
    catch (e) { console.error('staff alert: the send threw:', e?.name || 'Error'); r = { sent: false, reason: 'send_failed' }; }
    if (r?.sent) {
      out.sent += 1;
      if (held) {
        try { await env.USERS.delete(queueKey(addr, alert)); await env.USERS.delete(overKey(addr, alert)); } catch { /* the digest names it once more; nothing is lost */ }
      }
      continue;
    }
    const capped = r?.reason === 'daily_cap';
    if ((capped || holdOnFailure) && env.USERS) {
      if (held) { out.queued += 1; continue; }
      const reason = capped ? 'daily_cap' : (typeof r?.reason === 'string' ? r.reason.slice(0, 30) : 'send_failed');
      const entry = { alert_id: id.slice(0, 200), kind: typeof kind === 'string' ? kind.slice(0, 60) : null, lane, reference: typeof reference === 'string' ? reference.slice(0, 120) : null, queued_at: new Date().toISOString(), required: required === true, reason, digested_at: null };
      try {
        await env.USERS.put(overKey(addr, alert), JSON.stringify(entry), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS });
        await env.USERS.put(queueKey(addr, alert), JSON.stringify(entry), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS, metadata: queueMetadata(entry) });
        out.queued += 1;
      } catch (e) {
        // Not on the list: a failure, never a silent drop. A required alert's caller throws and retries.
        console.error('staff alert: the over-cap entry was not written:', e?.name || 'Error');
        out.failed += 1;
      }
      continue;
    }
    out.failed += 1;
  }
  out.delivered = out.sent + out.digested > 0;
  return out;
}

/// From the hourly sweep, after the case steps: one digest per staff address, at most one an hour, naming how many alerts are
/// waiting on its over-cap list and the references of its share of STAFF_DIGEST_MAX_ENTRIES of them (read from the queue
/// keys' metadata). `render({ total, entries, more })` builds the mail. Once a digest is SENT each entry it named is marked
/// `digested_at` and leaves the queue; one that fails to send changes nothing, so the next hour tries again. The digest itself
/// is not counted against a lane. The run stays within STAFF_DIGEST_RUN_OPS (staffDigestPlan): with more staff addresses than
/// it covers, `rotated` counts the ones whose turn is a later hour. Never throws. `{ digests, named, waiting, failed, rotated }`.
export async function sendStaffDigests(env, recipients, render, { now = Date.now(), fromName } = {}) {
  const out = { digests: 0, named: 0, waiting: 0, failed: 0, rotated: 0 };
  if (!env.USERS || !recipients.length) return out;
  const { addresses, share } = staffDigestPlan(recipients.length);
  const start = (Math.floor(now / STAFF_DIGEST_EVERY_MS) * addresses) % recipients.length;
  const turn = Array.from({ length: addresses }, (_, i) => recipients[(start + i) % recipients.length]);
  out.rotated = recipients.length - turn.length;
  for (const to of turn) {
    try {
      const addr = await addressTag(to);
      const listed = await env.USERS.list({ prefix: queuePrefix(addr), limit: 1000 });
      const keys = listed?.keys || [];
      if (!keys.length) continue;
      out.waiting += keys.length;
      if (await env.USERS.get(digestKey(addr, now))) continue;
      const entries = [];
      for (const k of keys.slice(0, share)) {
        const e = hasMeta(k) && 'reference' in k.metadata ? k.metadata : parse(await env.USERS.get(k.name));
        if (e) entries.push({ key: k.name, alert: k.name.slice(queuePrefix(addr).length), ...e });
      }
      if (!entries.length) continue;
      const rendered = render({ total: keys.length, entries: entries.map(({ kind, lane, reference, queued_at, required, reason }) => ({ kind, lane, reference, queued_at, required, reason: reason || 'daily_cap' })), more: Math.max(0, keys.length - entries.length), at_least: listed?.list_complete === false });
      const [r] = await sendNotice(env, [to], rendered, { kind: 'staff_digest', ...(fromName ? { fromName } : {}) });
      if (!r?.sent) { out.failed += 1; continue; }
      out.digests += 1;
      await env.USERS.put(digestKey(addr, now), new Date(now).toISOString(), { expirationTtl: 2 * 86400 });
      const at = new Date(now).toISOString();
      for (const e of entries) {
        const { key, alert, kind, lane, reference, queued_at, required, reason, alert_id } = e;
        await env.USERS.put(overKey(addr, alert), JSON.stringify({ ...(alert_id ? { alert_id } : {}), kind, lane, reference, queued_at, required, reason: reason || 'daily_cap', digested_at: at }), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS });
        await env.USERS.delete(key);
        out.named += 1;
      }
    } catch (e) {
      console.error('staff alert: the digest failed:', e?.name || 'Error');
      out.failed += 1;
    }
  }
  return out;
}

/// The over-cap queue as it stands, across every staff address: how many alerts wait undelivered, how many of them have waited
/// STAFF_QUEUE_OVERDUE_MS or longer, the oldest's time and age, and the count of each kind (and of the overdue ones). No
/// reference, account, address or content. Reads at most STAFF_QUEUE_SCAN_MAX entries (`at_least` past it) from the list
/// alone: each entry's metadata, or for an entry from before metadata its list expiration (kept 7 days then); only an entry
/// with neither is read, at most STAFF_QUEUE_LEGACY_READS a scan, and past that it counts as `unknown_age`. Throws when the
/// queue cannot be listed.
export async function staffQueueState(env, { now = Date.now() } = {}) {
  const out = { count: 0, overdue: 0, required: 0, oldest_queued_at: null, oldest_age_seconds: null, by_kind: {}, overdue_by_kind: {}, addresses: 0, unknown_age: 0, legacy_reads: 0, at_least: false };
  if (!env.USERS) return out;
  const addresses = new Set();
  let oldest = null;
  let cursor;
  const entryOf = async (k) => {
    if (hasMeta(k)) return k.metadata;
    if (Number.isFinite(k?.expiration)) return { kind: null, required: null, queued_at: new Date(k.expiration * 1000 - LEGACY_TTL_MS).toISOString() };
    if (out.legacy_reads >= STAFF_QUEUE_LEGACY_READS) return null;
    out.legacy_reads += 1;
    return parse(await env.USERS.get(k.name));
  };
  do {
    const listed = await env.USERS.list({ prefix: QUEUE_ALL, limit: 1000, ...(cursor ? { cursor } : {}) });
    for (const k of listed?.keys || []) {
      if (out.count >= STAFF_QUEUE_SCAN_MAX) { out.at_least = true; break; }
      out.count += 1;
      addresses.add(k.name.slice(QUEUE_ALL.length).split(':')[0]);
      const e = await entryOf(k);
      const kind = typeof e?.kind === 'string' && e.kind ? e.kind.slice(0, 60) : 'unknown';
      out.by_kind[kind] = (out.by_kind[kind] || 0) + 1;
      if (e?.required === true) out.required += 1;
      const at = Date.parse(e?.queued_at || '');
      if (!Number.isFinite(at)) { out.unknown_age += 1; continue; }
      if (oldest === null || at < oldest) oldest = at;
      if (now - at >= STAFF_QUEUE_OVERDUE_MS) {
        out.overdue += 1;
        out.overdue_by_kind[kind] = (out.overdue_by_kind[kind] || 0) + 1;
      }
    }
    cursor = !out.at_least && listed?.list_complete === false && listed.cursor ? listed.cursor : undefined;
  } while (cursor);
  out.addresses = addresses.size;
  if (oldest !== null) {
    out.oldest_queued_at = new Date(oldest).toISOString();
    out.oldest_age_seconds = Math.max(0, Math.floor((now - oldest) / 1000));
  }
  return out;
}

/// From the hourly sweep, after the case steps and the digest: when any alert has waited on the over-cap queue
/// STAFF_QUEUE_OVERDUE_MS or longer (its digest did not send), one failure alert to `recipients` on the legal lane, which no
/// allowance stops, and never itself queued (it describes the queue). `render(state)` builds it from staffQueueState:
/// counts, ages and kinds only. At most one per STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS while the condition stands; the mark is written only once an alert is SENT, so one that cannot be sent is tried again next hour.
/// Never throws. `{ overdue, alerted, skipped, sent, failed }`.
export async function alertOverdueStaffQueue(env, recipients, render, { now = Date.now(), fromName } = {}) {
  const out = { overdue: 0, alerted: false, skipped: null, sent: 0, failed: 0 };
  if (!env.USERS) return { ...out, skipped: 'no_storage' };
  try {
    const last = Date.parse((await env.USERS.get(STAFF_QUEUE_OVERDUE_ALERT_KEY)) || '');
    if (Number.isFinite(last) && now - last < STAFF_QUEUE_OVERDUE_ALERT_EVERY_MS) return { ...out, skipped: 'alerted_today' };
    const state = await staffQueueState(env, { now });
    out.overdue = state.overdue;
    if (!state.overdue) return out;
    console.error(`staff alert: ${state.overdue} staff alert(s) undelivered on the over-cap queue past 24 hours, the oldest ${state.oldest_age_seconds}s`);
    const day = new Date(now).toISOString().slice(0, 10);
    const r = await sendStaffAlert(env, recipients, render(state), { lane: 'legal', alertId: `staffqueue-overdue:${day}`, kind: 'staff_failure', reference: null, required: false, fromName, queueOnFailure: false });
    out.sent = r.sent;
    out.failed = r.failed;
    if (r.sent > 0) {
      await env.USERS.put(STAFF_QUEUE_OVERDUE_ALERT_KEY, new Date(now).toISOString(), { expirationTtl: 2 * 86400 });
      out.alerted = true;
    }
  } catch (e) {
    console.error('staff alert: the overdue-queue check failed:', e?.name || 'Error');
    out.failed += 1;
  }
  return out;
}
