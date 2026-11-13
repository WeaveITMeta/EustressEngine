// =============================================================================
// Eustress API - Holder ledger (who holds how many units of the company)
// =============================================================================
// The record a transfer agent keeps: every holder of record and how many units
// each holds, with every issuance, transfer and repurchase as an entry that is
// never changed or removed. See docs/finance/REGULATION_A_TIER_2.md.
//
// NOTHING HERE SELLS ANYTHING. No route takes money, and no route a member of
// the public can reach changes the ledger. Every write is an administrator
// recording an act that already happened lawfully. Units may be issued to the
// public only after an offering is qualified; the code does not know whether
// that is true, so the person recording an issuance must.
//
// One Durable Object holds the whole ledger, because a balance must never be
// spent twice and KV, last writer wins, cannot promise that.
//
//   e:{seq}        an entry: { seq, at, kind, from, to, units, price_cents,
//                  ref, memo, by, prev_hash, hash }
//   h:{holder_id}  a holder of record: { holder_id, legal_name, country,
//                  accredited, units, since, updated_at }
//   ref:{ref}      the seq of the entry made under a ref, so a repeat of a
//                  request makes no second entry
//   state          { seq, head_hash, issued_units }
//   o: s: sh: i:   the offering, its subscriptions and indications of interest
//                  (src/subscriptions.mjs)
//
// Each entry carries the SHA-256 of its own fields and of the entry before it,
// so a changed or missing entry breaks the chain and `verify` finds it.
//
// Admin routes (the admin surface, /api/admin/*):
//   GET  /api/admin/holders                 holders, totals and the two counts
//                                           the Exchange Act registration test
//                                           is made on
//   GET  /api/admin/holders/entries         entries, oldest first (?after=&limit=)
//   GET  /api/admin/holders/verify          recompute the hash chain and balances
//   PUT  /api/admin/holders/record          {holder_id, legal_name, country, accredited}
//   POST /api/admin/holders/issue           {ref, to, units, price_cents?, memo?}
//   POST /api/admin/holders/transfer        {ref, from, to, units, memo?}
//   POST /api/admin/holders/repurchase      {ref, from, units, price_cents?, memo?}
// A holder's own view:
//   GET  /api/holdings                      the signed-in account's units and entries
//
// Table of Contents:
// 1. Rules
// 2. The object
// 3. Calling it
// 4. Routes
// =============================================================================

// -----------------------------------------------------------------------------
// 1. Rules
// -----------------------------------------------------------------------------

import { SUBSCRIPTION_OPS } from './subscriptions.mjs';

export const ENTRY_KINDS =['issue', 'transfer', 'repurchase'];
/// The holder counts past which a company with more than $10M in assets must register under the Exchange Act, unless an
/// exemption holds (REGULATION_A_TIER_2.md, "The holder-count trigger"). Shown beside the counts; nothing is enforced on them.
export const HOLDERS_OF_RECORD_LIMIT = 2000;
export const NON_ACCREDITED_LIMIT = 500;
/// The most units one entry may move, and the most that may ever be issued: far inside the integers a float holds exactly.
export const MAX_UNITS = 1_000_000_000_000;

const REF = /^[A-Za-z0-9_:.-]{1,200}$/;
const HOLDER_ID = /^[A-Za-z0-9_-]{1,64}$/;
const COUNTRY = /^[A-Z]{2}$/;
const GENESIS = '0'.repeat(64);
const PAGE = 1000;

const fail = (status, error, code) => ({ ok: false, status, error, code });
const isUnits = (n) => Number.isSafeInteger(n) && n > 0 && n <= MAX_UNITS;
const entryKey = (seq) => `e:${String(seq).padStart(12, '0')}`;
const text = (v, max) => (typeof v === 'string' ? v.trim().slice(0, max) : '');

async function sha256Hex(s) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(s));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// The hash of an entry: its fields in a fixed order, and the hash of the entry before it.
export function entryHash(e) {
  return sha256Hex(JSON.stringify([e.seq, e.at, e.kind, e.from, e.to, e.units, e.price_cents, e.ref, e.memo, e.by, e.prev_hash]));
}

// -----------------------------------------------------------------------------
// 2. The object
// -----------------------------------------------------------------------------

export class HolderLedger {
  constructor(ctx, env) {
    this.storage = ctx.storage;
    this.env = env;
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
      result = handler ? await handler.call(this, args) : fail(404, `unknown operation ${op}`, 'unknown_operation');
    } catch (e) {
      console.error(`HolderLedger ${op} failed:`, e?.message || e);
      result = fail(500, 'the holder ledger failed', 'internal');
    }
    return new Response(JSON.stringify(result), { status: 200, headers: { 'content-type': 'application/json' } });
  }

  /// One write at a time: every write reads the state, decides, and puts once.
  atomic(fn) {
    const run = (this.tail || Promise.resolve()).then(() => fn());
    this.tail = run.then(() => {}, () => {});
    return run;
  }

  async state() {
    return (await this.storage.get('state')) || { seq: 0, head_hash: GENESIS, issued_units: 0 };
  }

  async listAll(prefix) {
    const out = [];
    let startAfter;
    for (;;) {
      const page = await this.storage.list({ prefix, startAfter, limit: PAGE });
      for (const [k, v] of page) {
        out.push(v);
        startAfter = k;
      }
      if (page.size < PAGE) return out;
    }
  }

  /// Append one entry and the balances it changes, in one put.
  append(args) {
    const bad = entryProblem(args);
    if (bad) return bad;
    return this.atomic(async () => {
      const r = await this.build(args);
      if (r.ok === false || r.replay) return r;
      await this.storage.put(r.writes);
      return { ok: true, entry: r.entry, issued_units: r.issued_units };
    });
  }

  /// Everything one entry writes, without writing it: a refusal, a replay, or `{ writes, entry, issued_units }`. Call it only
  /// inside `atomic`, and put `writes` in one put. `holders` holds holder records the caller is writing in the same put, so a
  /// holder recorded and issued to in one step is seen here.
  async build({ kind, ref, from = null, to = null, units, price_cents = null, memo = '', by = null }, holders = {}) {
    const bad = entryProblem({ ref, from, to, units, price_cents });
    if (bad) return bad;
    const priorSeq = await this.storage.get(`ref:${ref}`);
    if (priorSeq) return { ok: true, replay: true, entry: await this.storage.get(entryKey(priorSeq)) };
    const state = await this.state();
    const writes = {};
    const now = new Date().toISOString();
    const holderOf = async (id) => holders[`h:${id}`] || (await this.storage.get(`h:${id}`));
    if (from !== null) {
      const h = await holderOf(from);
      if (!h) return fail(404, 'No such holder to take units from', 'holder_not_found');
      if (h.units < units) return fail(409, `The holder has ${h.units} units, fewer than ${units}`, 'insufficient_units');
      writes[`h:${from}`] = { ...h, units: h.units - units, updated_at: now };
    }
    if (to !== null) {
      const h = await holderOf(to);
      // A holder of record has a legal name on file before any unit is theirs.
      if (!h) return fail(409, 'Record the holder (legal name, country, accredited) before issuing or transferring units to them', 'holder_not_recorded');
      writes[`h:${to}`] = { ...h, units: h.units + units, since: h.since || now, updated_at: now };
    }
    let issued = state.issued_units;
    if (kind === 'issue') issued += units;
    if (kind === 'repurchase') issued -= units;
    if (issued > MAX_UNITS) return fail(409, `No more than ${MAX_UNITS} units may be issued`, 'issue_limit');
    const seq = state.seq + 1;
    const entry = { seq, at: now, kind, from, to, units, price_cents, ref, memo: text(memo, 300), by: typeof by === 'string' ? by : null, prev_hash: state.head_hash };
    entry.hash = await entryHash(entry);
    writes[entryKey(seq)] = entry;
    writes[`ref:${ref}`] = seq;
    writes.state = { seq, head_hash: entry.hash, issued_units: issued };
    return { ok: true, writes, entry, issued_units: issued };
  }
}

/// What is wrong with an entry's fields, or null.
function entryProblem({ ref, from = null, to = null, units, price_cents = null }) {
  if (typeof ref !== 'string' || !REF.test(ref)) return fail(400, 'ref is 1 to 200 of letters, digits and _ : . -', 'invalid_request');
  if (!isUnits(units)) return fail(400, `units is a whole number from 1 to ${MAX_UNITS}`, 'invalid_request');
  if (price_cents !== null && !(Number.isSafeInteger(price_cents) && price_cents >= 0)) return fail(400, 'price_cents is whole, non-negative cents per unit', 'invalid_request');
  for (const id of [from, to]) if (id !== null && (typeof id !== 'string' || !HOLDER_ID.test(id))) return fail(400, 'a holder id is 1 to 64 of letters, digits, _ and -', 'invalid_request');
  if (from !== null && from === to) return fail(400, 'a transfer is between two holders', 'invalid_request');
  return null;
}

const OPS = {
  // The offering and its subscriptions live in this object too, so accepting a subscription and issuing its units is one write.
  ...SUBSCRIPTION_OPS,

  /// Put a holder of record on file, or correct their details. Their units are never set here.
  async record({ holder_id, legal_name, country, accredited }) {
    if (typeof holder_id !== 'string' || !HOLDER_ID.test(holder_id)) return fail(400, 'holder_id is 1 to 64 of letters, digits, _ and -', 'invalid_request');
    const name = text(legal_name, 200);
    if (name.length < 2) return fail(400, 'legal_name is required', 'invalid_request');
    if (typeof country !== 'string' || !COUNTRY.test(country)) return fail(400, 'country is a two-letter code in capitals', 'invalid_request');
    if (typeof accredited !== 'boolean') return fail(400, 'accredited is true or false', 'invalid_request');
    return this.atomic(async () => {
      const prior = await this.storage.get(`h:${holder_id}`);
      const now = new Date().toISOString();
      const holder = { holder_id, units: 0, since: null, ...prior, legal_name: name, country, accredited, updated_at: now };
      await this.storage.put(`h:${holder_id}`, holder);
      return { ok: true, holder };
    });
  },

  issue({ ref, to, units, price_cents = null, memo, by }) {
    if (typeof to !== 'string') return fail(400, 'to is the holder the units are issued to', 'invalid_request');
    return this.append({ kind: 'issue', ref, to, units, price_cents, memo, by });
  },

  transfer({ ref, from, to, units, memo, by }) {
    if (typeof from !== 'string' || typeof to !== 'string') return fail(400, 'from and to are holders', 'invalid_request');
    return this.append({ kind: 'transfer', ref, from, to, units, memo, by });
  },

  /// The company buys units back; they are no longer issued.
  repurchase({ ref, from, units, price_cents = null, memo, by }) {
    if (typeof from !== 'string') return fail(400, 'from is the holder the units are bought from', 'invalid_request');
    return this.append({ kind: 'repurchase', ref, from, units, price_cents, memo, by });
  },

  /// Every holder with units, most units first, with the totals and the counts the registration test is made on.
  async holders() {
    const all = await this.listAll('h:');
    const holders = all.filter((h) => h.units > 0).sort((a, b) => b.units - a.units || (a.holder_id < b.holder_id ? -1 : 1));
    const state = await this.state();
    const nonAccredited = holders.filter((h) => !h.accredited).length;
    return {
      ok: true, holders, on_file: all.length, issued_units: state.issued_units, entries: state.seq,
      counts: {
        holders_of_record: holders.length, non_accredited: nonAccredited,
        holders_of_record_limit: HOLDERS_OF_RECORD_LIMIT, non_accredited_limit: NON_ACCREDITED_LIMIT,
      },
    };
  },

  async holder({ holder_id }) {
    if (typeof holder_id !== 'string' || !HOLDER_ID.test(holder_id)) return fail(400, 'invalid holder_id', 'invalid_request');
    const holder = await this.storage.get(`h:${holder_id}`);
    if (!holder) return { ok: true, holder: null, entries: [] };
    const entries = (await this.listAll('e:')).filter((e) => e.from === holder_id || e.to === holder_id);
    return { ok: true, holder, entries };
  },

  async entries({ after = 0, limit = 200 } = {}) {
    const from = Number.isSafeInteger(after) && after >= 0 ? after : 0;
    const n = Number.isSafeInteger(limit) ? Math.max(1, Math.min(limit, PAGE)) : 200;
    const page = await this.storage.list({ prefix: 'e:', startAfter: entryKey(from), limit: n });
    const entries = [...page.values()];
    const state = await this.state();
    return { ok: true, entries, next_after: entries.length ? entries[entries.length - 1].seq : from, more: entries.length > 0 && entries[entries.length - 1].seq < state.seq };
  },

  /// Recompute everything from the entries: the hash chain, each holder's units, and the units issued. Says where it breaks.
  async verify() {
    const entries = await this.listAll('e:');
    const state = await this.state();
    const problems = [];
    const units = new Map();
    let prev = GENESIS;
    let issued = 0;
    let expect = 1;
    for (const e of entries) {
      if (e.seq !== expect) problems.push(`entry ${expect} is missing`);
      expect = e.seq + 1;
      if (e.prev_hash !== prev) problems.push(`entry ${e.seq} does not follow the entry before it`);
      if ((await entryHash(e)) !== e.hash) problems.push(`entry ${e.seq} was changed`);
      prev = e.hash;
      if (e.from) units.set(e.from, (units.get(e.from) || 0) - e.units);
      if (e.to) units.set(e.to, (units.get(e.to) || 0) + e.units);
      if (e.kind === 'issue') issued += e.units;
      if (e.kind === 'repurchase') issued -= e.units;
    }
    if (state.seq !== entries.length) problems.push(`the state counts ${state.seq} entries and ${entries.length} are stored`);
    if (state.head_hash !== prev) problems.push('the newest entry is not the one the state names');
    if (state.issued_units !== issued) problems.push(`the state says ${state.issued_units} units are issued and the entries say ${issued}`);
    const holders = new Map((await this.listAll('h:')).map((h) => [h.holder_id, h.units]));
    for (const id of new Set([...units.keys(), ...holders.keys()])) {
      const fromEntries = units.get(id) || 0;
      if (fromEntries < 0) problems.push(`holder ${id} goes below zero`);
      if ((holders.get(id) || 0) !== fromEntries) problems.push(`holder ${id} is recorded with ${holders.get(id) || 0} units and the entries say ${fromEntries}`);
    }
    const held = [...holders.values()].reduce((a, b) => a + b, 0);
    if (held !== issued) problems.push(`holders hold ${held} units and ${issued} are issued`);
    return { ok: true, sound: problems.length === 0, entries: entries.length, issued_units: issued, head_hash: prev, problems: problems.slice(0, 50) };
  },
};

// -----------------------------------------------------------------------------
// 3. Calling it
// -----------------------------------------------------------------------------

/// Call one operation of the ledger. Never throws: an object that cannot be reached answers `{ ok: false, status: 503 }`.
export async function holderLedgerCall(env, op, args = {}) {
  if (!env.HOLDER_LEDGER) return fail(503, 'the holder ledger is not configured', 'no_binding');
  try {
    const stub = env.HOLDER_LEDGER.get(env.HOLDER_LEDGER.idFromName('units'));
    const res = await stub.fetch(`https://holder-ledger.internal/${op}`, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(args),
    });
    return await res.json();
  } catch (e) {
    console.error(`holderLedgerCall ${op} failed:`, e?.message || e);
    return fail(503, 'holder ledger unavailable', 'unavailable');
  }
}

// -----------------------------------------------------------------------------
// 4. Routes
// -----------------------------------------------------------------------------

const WRITES = { '/api/admin/holders/issue': 'issue', '/api/admin/holders/transfer': 'transfer', '/api/admin/holders/repurchase': 'repurchase' };

/// Handle /api/admin/holders*; null for any other path. `deps`: { json, requireAdmin }.
export async function handleHolderAdminRoute(request, url, env, cors, deps) {
  const p = url.pathname;
  if (p !== '/api/admin/holders' && !p.startsWith('/api/admin/holders/')) return null;
  const { json } = deps;
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  const headers = { ...cors, 'Cache-Control': 'no-store' };
  const answer = (r) => (r.ok === false ? json({ error: r.error, code: r.code }, r.status || 500, headers) : json(r, 200, headers));

  if (request.method === 'GET') {
    if (p === '/api/admin/holders') return answer(await holderLedgerCall(env, 'holders'));
    if (p === '/api/admin/holders/verify') return answer(await holderLedgerCall(env, 'verify'));
    if (p === '/api/admin/holders/entries') {
      const num = (name) => (url.searchParams.has(name) ? Number(url.searchParams.get(name)) : undefined);
      return answer(await holderLedgerCall(env, 'entries', { after: num('after'), limit: num('limit') }));
    }
    return json({ error: 'Not found' }, 404, cors);
  }
  let body;
  try {
    body = await request.json();
  } catch {
    body = null;
  }
  if (!body || typeof body !== 'object' || Array.isArray(body)) return json({ error: 'JSON body required' }, 400, cors);
  if (p === '/api/admin/holders/record' && request.method === 'PUT') {
    const { holder_id, legal_name, country, accredited } = body;
    return answer(await holderLedgerCall(env, 'record', { holder_id, legal_name, country, accredited }));
  }
  if (WRITES[p] && request.method === 'POST') {
    const { ref, from, to, units, price_cents, memo } = body;
    // Who recorded it is the administrator's session, never the body.
    return answer(await holderLedgerCall(env, WRITES[p], { ref, from, to, units, price_cents, memo, by: adminId }));
  }
  return json({ error: 'Not found' }, 404, cors);
}

/// GET /api/holdings: the signed-in account's own units and entries. A holder's id is their account id. Their legal name and the
/// administrator who made each entry are left out. `deps`: { json, verifyAuth }.
export async function handleHoldings(request, env, cors, deps) {
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return deps.json({ error: 'Unauthorized' }, 401, cors);
  const r = await holderLedgerCall(env, 'holder', { holder_id: userId });
  if (r.ok === false) return deps.json({ error: r.error }, r.status || 500, cors);
  const entries = r.entries.map((e) => ({
    seq: e.seq, at: e.at, kind: e.kind, units: e.units, price_cents: e.price_cents,
    direction: e.to === userId ? 'in' : 'out',
  }));
  return deps.json({ units: r.holder?.units || 0, since: r.holder?.since || null, entries }, 200, { ...cors, 'Cache-Control': 'no-store' });
}
