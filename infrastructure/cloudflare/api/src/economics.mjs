// =============================================================================
// Eustress API - Economics statements (Actual mode of /economics)
// =============================================================================
// The numbers behind the Economics page's Actual mode, for one calendar month or
// one quarter (UTC):
//
//   GET  /api/economics/actual?period=month|quarter&at=YYYY-MM|YYYY-Qn   public
//        The Treasury now, what it paid contributors in the period day by day,
//        and Bliss. These figures are public already (/api/payouts/rate and the
//        ledger); this gathers them for a period.
//
//   GET  /api/admin/economics/actual?period=...&at=...                   admin
//        The same plus receipts by kind (Ticket packs, Delta packs, plans,
//        gifts to the Treasury), fees, refunds and disputes, Eustress's half,
//        and where Eustress's half went: the cost ledger for those months and
//        labor at its set share.
//
//   PUT  /api/admin/economics/costs     {month, category, amount_usd, note}   admin
//        Set (not add to) one category of one month's costs; 0 removes it.
//   PUT  /api/admin/economics/settings  {labor_share}                         admin
//   POST /api/admin/economics/rebuild                                         admin
//        Recompute the Treasury's month totals from its records.
//
// Receipts come from the Treasury's month totals (treasury.mjs, agg:{YYYY-MM}),
// written in the same storage put as the money they count. Costs are what an
// administrator enters from the bills (Cloudflare, servers, domain, tools,
// other); nothing here guesses a cost. Labor is a share of Eustress's half that
// McKale sets, not a salary.
//
// Table of Contents:
// 1. Periods
// 2. Reading the period
// 3. Routes
// =============================================================================

import { treasuryCall, dripFor } from './treasury.mjs';
import { readSupplyMinor, readDistributedMinor, fromMinor } from './bliss.mjs';

// -----------------------------------------------------------------------------
// 1. Periods
// -----------------------------------------------------------------------------

export const COST_CATEGORIES = ['cloudflare', 'servers', 'domain', 'tools', 'other'];
export const DEPOSIT_KIND_NAMES = {
  ticket_purchase: 'Ticket packs',
  delta_purchase: 'Delta packs',
  subscription: 'Plans',
  treasury_fund: 'Gifts to the Treasury',
};
const DEFAULT_SETTINGS = { labor_share: 0 };
const pad = (n) => String(n).padStart(2, '0');

/// A period from the query: a month (YYYY-MM) or a quarter (YYYY-Qn), the current one when `at` is absent.
/// `now` is a Date. Null when the query names no valid period.
export function resolvePeriod(kind, at, now = new Date()) {
  const y = now.getUTCFullYear();
  const m = now.getUTCMonth() + 1;
  if (!kind || kind === 'month') {
    let year = y;
    let month = m;
    if (at) {
      const hit = /^(\d{4})-(0[1-9]|1[0-2])$/.exec(at);
      if (!hit) return null;
      year = Number(hit[1]);
      month = Number(hit[2]);
    }
    return periodOf('month', year, [month], `${year}-${pad(month)}`, now);
  }
  if (kind === 'quarter') {
    let year = y;
    let q = Math.ceil(m / 3);
    if (at) {
      const hit = /^(\d{4})-Q([1-4])$/.exec(at);
      if (!hit) return null;
      year = Number(hit[1]);
      q = Number(hit[2]);
    }
    const first = (q - 1) * 3 + 1;
    return periodOf('quarter', year, [first, first + 1, first + 2], `${year}-Q${q}`, now);
  }
  return null;
}

function periodOf(kind, year, monthNums, label, now) {
  const months = monthNums.map((mm) => `${year}-${pad(mm)}`);
  const from = `${months[0]}-01`;
  const last = monthNums[monthNums.length - 1];
  const lastDay = new Date(Date.UTC(year, last, 0)).getUTCDate();
  const to = `${months[months.length - 1]}-${pad(lastDay)}`;
  const today = now.toISOString().slice(0, 10);
  return { kind, label, months, from, to, current: today >= from && today <= to, future: today < from };
}

/// Every date of the period up to yesterday (a payout is recorded under its score date, after the day ends).
export function payoutDates(period, now = new Date()) {
  const out = [];
  const yesterday = new Date(now.getTime() - 86400e3).toISOString().slice(0, 10);
  for (let t = Date.parse(`${period.from}T00:00:00Z`); ; t += 86400e3) {
    const d = new Date(t).toISOString().slice(0, 10);
    if (d > period.to || d > yesterday) break;
    out.push(d);
  }
  return out;
}

// -----------------------------------------------------------------------------
// 2. Reading the period
// -----------------------------------------------------------------------------

const usd = (cents) => Math.round(Number(cents) || 0) / 100;

/// Sum a section of the month totals over the period, by kind.
export function sumSection(months, section) {
  const out = {};
  for (const t of months) {
    for (const [kind, row] of Object.entries(t[section] || {})) {
      const acc = out[kind] || (out[kind] = {});
      for (const [k, v] of Object.entries(row)) acc[k] = (acc[k] || 0) + (Number(v) || 0);
    }
  }
  return out;
}

const total = (byKind, field) => Object.values(byKind).reduce((a, r) => a + (r[field] || 0), 0);

/// The receipts of a period from its month totals: by kind, and the totals after refunds, disputes and disputes won.
export function receiptsOf(months) {
  const deposits = sumSection(months, 'deposits');
  const reversals = sumSection(months, 'reversals');
  const restored = sumSection(months, 'restored');
  const back = (field) => total(reversals, field) - total(restored, field);
  const gross = total(deposits, 'gross_cents');
  const fees = total(deposits, 'fee_cents');
  const treasury = total(deposits, 'treasury_cents');
  const platform = total(deposits, 'platform_cents');
  const refundedGross = back('gross_cents');
  // A reversal takes the treasury share back from the treasury, and what the treasury had already paid out from the platform.
  const treasuryBack = back('treasury_cents');
  const platformBack = back('platform_cents') + back('shortfall_cents');
  const byKind = Object.fromEntries(Object.entries(deposits).map(([kind, r]) => [kind, {
    name: DEPOSIT_KIND_NAMES[kind] || kind,
    count: r.count,
    gross_usd: usd(r.gross_cents),
    fees_usd: usd(r.fee_cents),
    treasury_usd: usd(r.treasury_cents),
    eustress_usd: usd(r.platform_cents),
    tickets: r.tickets || 0,
    delta: r.delta || 0,
    refunded_usd: usd((reversals[kind]?.gross_cents || 0) - (restored[kind]?.gross_cents || 0)),
  }]));
  return {
    by_kind: byKind,
    totals: {
      gross_usd: usd(gross),
      fees_usd: usd(fees),
      net_usd: usd(gross - fees),
      treasury_usd: usd(treasury),
      eustress_usd: usd(platform),
      refunded_usd: usd(refundedGross),
      treasury_after_refunds_usd: usd(treasury - treasuryBack),
      eustress_after_refunds_usd: usd(platform - platformBack),
      // Each plan invoice is one subscriber's month.
      plan_invoices: deposits.subscription?.count || 0,
    },
  };
}

/// The cost ledger for the period's months, by month and by category, in dollars.
export async function costsOf(env, months) {
  const byMonth = {};
  const byCategory = Object.fromEntries(COST_CATEGORIES.map((c) => [c, 0]));
  for (const m of months) {
    const raw = await env.PAYOUTS.get(`econ:costs:${m}`);
    const rows = raw ? JSON.parse(raw) : {};
    byMonth[m] = {};
    for (const [cat, row] of Object.entries(rows)) {
      const v = Number(row?.usd) || 0;
      byMonth[m][cat] = { usd: v, note: row?.note || '' };
      byCategory[cat] = Math.round(((byCategory[cat] || 0) + v) * 100) / 100;
    }
  }
  return { by_month: byMonth, by_category: byCategory, total_usd: Math.round(Object.values(byCategory).reduce((a, b) => a + b, 0) * 100) / 100 };
}

export async function readSettings(env) {
  const raw = await env.PAYOUTS.get('econ:settings');
  return { ...DEFAULT_SETTINGS, ...(raw ? JSON.parse(raw) : {}) };
}

/// What the Treasury paid contributors in the period, day by day, from the nightly payout records.
async function payoutsOf(env, period, now) {
  const days = [];
  let paid = 0;
  let contributors = 0;
  for (const d of payoutDates(period, now)) {
    const raw = await env.PAYOUTS.get(`payout:${d}`);
    if (!raw) continue;
    const rec = JSON.parse(raw);
    const p = Number(rec.total_paid_usd) || 0;
    const n = Number(rec.contributors_paid) || 0;
    days.push({ date: d, paid_usd: p, contributors: n, drip_usd: Number(rec.drip_usd) || 0 });
    paid += p;
    contributors += n;
  }
  return {
    paid_usd: Math.round(paid * 100) / 100,
    days_paid: days.length,
    contributors_per_day: days.length ? Math.round((contributors / days.length) * 10) / 10 : 0,
    days,
  };
}

/// The public part of a period's statement.
export async function publicStatement(env, period, now = new Date()) {
  const snap = await treasuryCall(env, 'snapshot');
  if (!snap.ok) return { error: snap.error || 'Treasury unavailable', status: snap.status || 503 };
  const view = snap.view;
  const { drip_cents, scarce } = dripFor(snap.state.cents, snap.state.hwm_cents);
  return {
    period,
    generated_at: now.toISOString(),
    treasury: {
      balance_usd: view.treasury_usd,
      high_water_usd: view.hwm_usd,
      daily_drip_usd: drip_cents / 100,
      scarcity_active: scarce,
      paid_all_time_usd: view.total_paid_usd,
    },
    contributors: await payoutsOf(env, period, now),
    bliss: {
      supply: fromMinor(await readSupplyMinor(env)),
      distributed: fromMinor(await readDistributedMinor(env)),
    },
  };
}

/// The whole statement, for an administrator: the public part, receipts, costs and labor.
export async function adminStatement(env, period, now = new Date()) {
  const pub = await publicStatement(env, period, now);
  if (pub.error) return pub;
  const totals = await treasuryCall(env, 'period_totals', { months: period.months });
  if (!totals.ok) return { error: totals.error || 'Treasury unavailable', status: totals.status || 503 };
  const receipts = receiptsOf(totals.months);
  const costs = await costsOf(env, period.months);
  const settings = await readSettings(env);
  const half = receipts.totals.eustress_after_refunds_usd;
  const labor = Math.round(Math.max(0, half) * settings.labor_share * 100) / 100;
  const spent = Math.round((costs.total_usd + labor) * 100) / 100;
  return {
    ...pub,
    receipts,
    expenses: {
      ...costs,
      labor: { share: settings.labor_share, usd: labor },
      total_usd: spent,
      eustress_half_usd: half,
      retained_usd: Math.round((half - spent) * 100) / 100,
    },
    settings,
  };
}

// -----------------------------------------------------------------------------
// 3. Routes
// -----------------------------------------------------------------------------

/// Handle an /api/economics or /api/admin/economics route; null for any other path.
/// `deps`: { json, requireAdmin, publicCors }.
export async function handleEconomicsRoute(request, url, env, cors, deps) {
  const { json, requireAdmin, publicCors } = deps;
  const p = url.pathname;
  if (p === '/api/economics/actual' && request.method === 'GET') {
    const headers = { ...publicCors(), 'Cache-Control': 'public, max-age=60' };
    const period = resolvePeriod(url.searchParams.get('period'), url.searchParams.get('at'));
    if (!period) return json({ error: 'period is month (at=YYYY-MM) or quarter (at=YYYY-Qn)' }, 400, headers);
    const s = await publicStatement(env, period);
    return s.error ? json({ error: s.error }, s.status, headers) : json(s, 200, headers);
  }
  if (!p.startsWith('/api/admin/economics/')) return null;
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);

  if (p === '/api/admin/economics/actual' && request.method === 'GET') {
    const period = resolvePeriod(url.searchParams.get('period'), url.searchParams.get('at'));
    if (!period) return json({ error: 'period is month (at=YYYY-MM) or quarter (at=YYYY-Qn)' }, 400, cors);
    const s = await adminStatement(env, period);
    return s.error ? json({ error: s.error }, s.status, cors) : json(s, 200, { ...cors, 'Cache-Control': 'no-store' });
  }
  if (p === '/api/admin/economics/costs' && request.method === 'PUT') {
    let body;
    try {
      body = await request.json();
    } catch {
      return json({ error: 'JSON body required' }, 400, cors);
    }
    const { month, category, amount_usd, note = '' } = body || {};
    if (!/^\d{4}-(0[1-9]|1[0-2])$/.test(month || '')) return json({ error: 'month is YYYY-MM' }, 400, cors);
    if (!COST_CATEGORIES.includes(category)) return json({ error: `category is one of ${COST_CATEGORIES.join(', ')}` }, 400, cors);
    const amount = Number(amount_usd);
    if (!Number.isFinite(amount) || amount < 0 || amount > 10_000_000) return json({ error: 'amount_usd is 0 to 10,000,000' }, 400, cors);
    const key = `econ:costs:${month}`;
    const raw = await env.PAYOUTS.get(key);
    const rows = raw ? JSON.parse(raw) : {};
    if (amount === 0) delete rows[category];
    else rows[category] = { usd: Math.round(amount * 100) / 100, note: String(note).slice(0, 200), at: new Date().toISOString(), by: adminId };
    await env.PAYOUTS.put(key, JSON.stringify(rows));
    return json({ ok: true, month, costs: rows }, 200, cors);
  }
  if (p === '/api/admin/economics/settings' && request.method === 'PUT') {
    let body;
    try {
      body = await request.json();
    } catch {
      return json({ error: 'JSON body required' }, 400, cors);
    }
    const share = Number(body?.labor_share);
    if (!Number.isFinite(share) || share < 0 || share > 1) return json({ error: 'labor_share is 0 to 1' }, 400, cors);
    const settings = { ...(await readSettings(env)), labor_share: Math.round(share * 10000) / 10000 };
    await env.PAYOUTS.put('econ:settings', JSON.stringify(settings));
    return json({ ok: true, settings }, 200, cors);
  }
  if (p === '/api/admin/economics/rebuild' && request.method === 'POST') {
    const r = await treasuryCall(env, 'rebuild_totals');
    return json(r, r.ok === false ? r.status || 500 : 200, cors);
  }
  return json({ error: 'Not found' }, 404, cors);
}
