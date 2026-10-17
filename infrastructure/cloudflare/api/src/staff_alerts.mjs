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
// Storage (USERS):
//   mailcap:acct-staff:{address}:{day}, mailcap:acct-staff_priority:{address}:{day}   the lanes' counts (src/mail_limits.mjs)
//   staffmail-over:{address}:{alert}     an alert past its lane's allowance: { alert_id, kind, lane, reference, queued_at,
//                                        required, digested_at }, kept 7 days so a retry finds it
//   staffmail-queue:{address}:{alert}    the same while no digest has carried it
//   staffmail-digest:{address}:{hour}    the hour's digest was sent (one an hour)
// The address and the alert id are keyed by their SHA-256, never stored in a key in the clear.
// =============================================================================

import { sendNotice } from './account_notices.mjs';

/// The lanes, and the mail kind each is counted under (src/mail_limits.mjs).
export const STAFF_LANES = Object.freeze({ general: 'staff', priority: 'staff_priority', legal: 'staff_legal' });
/// A digest is sent at most this often per staff address.
export const STAFF_DIGEST_EVERY_MS = 3600000;
/// The most alerts one digest names; the rest are counted in it and named by the next.
export const STAFF_DIGEST_MAX_ENTRIES = 100;
/// How long an over-cap entry is kept, so a retry of its alert finds whether a digest carried it.
export const STAFF_OVER_CAP_TTL_SECONDS = 7 * 86400;

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

/// Sends one staff alert to each of `recipients` in `lane` ('general', 'priority' or 'legal'). Never throws. Past the lane's
/// allowance the alert goes on that address's over-cap list for the digest. `{ sent, queued, digested, failed, delivered }`:
/// `digested` counts the addresses a digest already carried this alert to; `delivered` is true when at least one address has
/// it (sent now, or carried by a digest that sent). An alert already queued for an address and not yet carried is sent
/// directly when the allowance allows (a new day), and leaves the list.
export async function sendStaffAlert(env, recipients, rendered, { lane = 'general', alertId = null, kind = null, reference = null, required = false, fromName } = {}) {
  const out = { sent: 0, queued: 0, digested: 0, failed: 0, delivered: false };
  const mailKind = STAFF_LANES[lane] || STAFF_LANES.general;
  const id = typeof alertId === 'string' && alertId ? alertId : `once:${crypto.randomUUID()}`;
  const alert = await alertTag(id);
  for (const to of recipients) {
    const addr = await addressTag(to);
    let held = null;
    try { held = env.USERS ? parse(await env.USERS.get(overKey(addr, alert))) : null; } catch { held = null; }
    if (held?.digested_at) { out.digested += 1; continue; }
    const [r] = await sendNotice(env, [to], rendered, { kind: mailKind, ...(fromName ? { fromName } : {}) });
    if (r?.sent) {
      out.sent += 1;
      if (held) {
        try { await env.USERS.delete(queueKey(addr, alert)); await env.USERS.delete(overKey(addr, alert)); } catch { /* the digest names it once more; nothing is lost */ }
      }
      continue;
    }
    if (r?.reason === 'daily_cap' && env.USERS) {
      if (held) { out.queued += 1; continue; }
      const entry = { alert_id: id.slice(0, 200), kind: typeof kind === 'string' ? kind.slice(0, 60) : null, lane, reference: typeof reference === 'string' ? reference.slice(0, 120) : null, queued_at: new Date().toISOString(), required: required === true, digested_at: null };
      try {
        await env.USERS.put(overKey(addr, alert), JSON.stringify(entry), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS });
        await env.USERS.put(queueKey(addr, alert), JSON.stringify(entry), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS });
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

/// From the hourly sweep: one digest per staff address, at most one an hour, naming how many alerts are waiting on its
/// over-cap list and the references of up to STAFF_DIGEST_MAX_ENTRIES of them. `render({ total, entries, more })` builds the
/// mail. Once a digest is SENT each entry it named is marked `digested_at` and leaves the queue; one that fails to send
/// changes nothing, so the next hour tries again. The digest itself is not counted against a lane. Never throws.
/// `{ digests, named, waiting, failed }`.
export async function sendStaffDigests(env, recipients, render, { now = Date.now(), fromName } = {}) {
  const out = { digests: 0, named: 0, waiting: 0, failed: 0 };
  if (!env.USERS) return out;
  for (const to of recipients) {
    try {
      const addr = await addressTag(to);
      const listed = await env.USERS.list({ prefix: queuePrefix(addr), limit: 1000 });
      const keys = listed?.keys || [];
      if (!keys.length) continue;
      out.waiting += keys.length;
      if (await env.USERS.get(digestKey(addr, now))) continue;
      const entries = [];
      for (const k of keys.slice(0, STAFF_DIGEST_MAX_ENTRIES)) {
        const e = parse(await env.USERS.get(k.name));
        if (e) entries.push({ key: k.name, alert: k.name.slice(queuePrefix(addr).length), ...e });
      }
      if (!entries.length) continue;
      const rendered = render({ total: keys.length, entries: entries.map(({ kind, lane, reference, queued_at, required }) => ({ kind, lane, reference, queued_at, required })), more: Math.max(0, keys.length - entries.length), at_least: listed?.list_complete === false });
      const [r] = await sendNotice(env, [to], rendered, { kind: 'staff_digest', ...(fromName ? { fromName } : {}) });
      if (!r?.sent) { out.failed += 1; continue; }
      out.digests += 1;
      await env.USERS.put(digestKey(addr, now), new Date(now).toISOString(), { expirationTtl: 2 * 86400 });
      const at = new Date(now).toISOString();
      for (const e of entries) {
        const { key, alert, ...entry } = e;
        await env.USERS.put(overKey(addr, alert), JSON.stringify({ ...entry, digested_at: at }), { expirationTtl: STAFF_OVER_CAP_TTL_SECONDS });
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
