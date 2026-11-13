// =============================================================================
// Eustress API - Monthly books close
// =============================================================================
// One calendar month (UTC) of the money this Worker recorded, as a double-entry
// journal and a trial balance an accountant or auditor can test, and a check of
// that month against Stripe's own record of the balance.
//
//   GET /api/admin/books/close?month=YYYY-MM[&reconcile=1][&format=csv]   admin
//
// Where each line comes from:
//   - Receipts, fees and the Treasury's share: the Treasury's month totals
//     (treasury.mjs, agg:{YYYY-MM}), written in the same storage put as the
//     money they count.
//   - Refunds, disputes and disputes won: the same totals.
//   - Contributor payouts: the nightly payout records (payout:{date}).
//   - Costs: the cost ledger an administrator enters from the bills
//     (economics.mjs, econ:costs:{YYYY-MM}).
//
// `reconcile=1` reads Stripe's balance transactions for the month and compares
// them with the journal: charges with receipts, Stripe's fees with the fees
// recorded, refunds and disputes with reversals, and transfers with payouts. A
// difference is reported, never corrected: the close reads and writes nothing.
//
// What this is not: financial statements. It applies no accounting policy. In
// particular Ticket and Delta packs are prepaid and a plan is paid in advance,
// and whether those receipts are revenue when paid or when used is the
// accountant's decision. The journal names them "receipts" for that reason.
//
// Table of Contents:
// 1. Accounts
// 2. The journal and the trial balance
// 3. Stripe reconciliation
// 4. Route
// =============================================================================

import { treasuryCall } from './treasury.mjs';
import { sumSection, costsOf, payoutDates, resolvePeriod, DEPOSIT_KIND_NAMES, COST_CATEGORIES } from './economics.mjs';

// -----------------------------------------------------------------------------
// 1. Accounts
// -----------------------------------------------------------------------------

/// The chart of accounts: number, name, and the side a balance normally sits on.
export const ACCOUNTS = {
  1000: { name: 'Stripe balance', side: 'debit' },
  2000: { name: 'Contributor treasury (held for contributors)', side: 'credit' },
  2100: { name: 'Paid outside Stripe (bank or owner)', side: 'credit' },
  4000: { name: 'Receipts: Ticket packs', side: 'credit' },
  4010: { name: 'Receipts: Delta packs', side: 'credit' },
  4020: { name: 'Receipts: Plans', side: 'credit' },
  4030: { name: 'Receipts: Gifts to the Treasury', side: 'credit' },
  4040: { name: 'Receipts: Engine sponsorships', side: 'credit' },
  4090: { name: 'Receipts: other', side: 'credit' },
  4900: { name: 'Refunds and disputes', side: 'debit' },
  5000: { name: 'Payment fees', side: 'debit' },
  5200: { name: 'Treasury share of receipts', side: 'debit' },
  6000: { name: 'Cost: Cloudflare', side: 'debit' },
  6010: { name: 'Cost: Game servers and relay', side: 'debit' },
  6020: { name: 'Cost: Domain', side: 'debit' },
  6030: { name: 'Cost: Tools', side: 'debit' },
  6090: { name: 'Cost: Other', side: 'debit' },
};

const RECEIPT_ACCOUNT = { ticket_purchase: 4000, delta_purchase: 4010, subscription: 4020, treasury_fund: 4030, engine_sponsor: 4040 };
const COST_ACCOUNT = { cloudflare: 6000, servers: 6010, domain: 6020, tools: 6030, other: 6090 };

const MONTH = /^\d{4}-(0[1-9]|1[0-2])$/;
const toCents = (usd) => Math.round((Number(usd) || 0) * 100);
const kindName = (kind) => DEPOSIT_KIND_NAMES[kind] || kind;

// -----------------------------------------------------------------------------
// 2. The journal and the trial balance
// -----------------------------------------------------------------------------

/// One balanced entry: `lines` are [account, debit_cents, credit_cents]. Lines of zero are dropped, and an entry with none is null.
function entry(date, memo, lines) {
  const kept = lines.filter(([, d, c]) => d !== 0 || c !== 0).map(([account, debit_cents, credit_cents]) => ({ account, debit_cents, credit_cents }));
  return kept.length ? { date, memo, lines: kept } : null;
}

/// The month's journal from its parts, all in whole cents:
///   totals  the Treasury's month totals ({deposits, reversals, restored} by deposit kind)
///   paid    cents the Treasury paid contributors in the month
///   costs   the cost ledger by category, in cents
/// Every entry balances, so the journal does.
export function journalOf(month, { totals, paid_cents = 0, costs = {} }) {
  const last = lastDay(month);
  const out = [];
  const sorted = (rows) => Object.entries(rows || {}).sort(([a], [b]) => (a < b ? -1 : 1));

  for (const [kind, r] of sorted(totals?.deposits)) {
    const gross = r.gross_cents || 0;
    const fee = r.fee_cents || 0;
    const treasury = r.treasury_cents || 0;
    out.push(entry(last, `${kindName(kind)}: ${r.count || 0} payments`, [
      [1000, gross - fee, 0],
      [5000, fee, 0],
      [RECEIPT_ACCOUNT[kind] || 4090, 0, gross],
    ]));
    out.push(entry(last, `${kindName(kind)}: the Treasury's share`, [[5200, treasury, 0], [2000, 0, treasury]]));
  }
  // Money that left after it arrived. The refund leaves the Stripe balance whole; the Treasury gives back what it still held of
  // its share, and what it had already paid out stays an expense.
  for (const [kind, r] of sorted(totals?.reversals)) {
    const gross = r.gross_cents || 0;
    const treasury = r.treasury_cents || 0;
    out.push(entry(last, `${kindName(kind)}: refunds and disputes (${r.count || 0})`, [[4900, gross, 0], [1000, 0, gross]]));
    out.push(entry(last, `${kindName(kind)}: the Treasury's share given back`, [[2000, treasury, 0], [5200, 0, treasury]]));
  }
  for (const [kind, r] of sorted(totals?.restored)) {
    const gross = r.gross_cents || 0;
    const treasury = r.treasury_cents || 0;
    out.push(entry(last, `${kindName(kind)}: disputes won (${r.count || 0})`, [[1000, gross, 0], [4900, 0, gross]]));
    out.push(entry(last, `${kindName(kind)}: the Treasury's share restored`, [[5200, treasury, 0], [2000, 0, treasury]]));
  }
  out.push(entry(last, 'Paid to contributors from the Treasury', [[2000, paid_cents, 0], [1000, 0, paid_cents]]));
  for (const cat of COST_CATEGORIES) {
    const cents = costs[cat] || 0;
    out.push(entry(last, `Cost ledger: ${cat}`, [[COST_ACCOUNT[cat], cents, 0], [2100, 0, cents]]));
  }
  return out.filter(Boolean);
}

function lastDay(month) {
  const [y, m] = month.split('-').map(Number);
  return `${month}-${String(new Date(Date.UTC(y, m, 0)).getUTCDate()).padStart(2, '0')}`;
}

/// The trial balance of a journal: each account's debits and credits, and the two totals, which are equal when the journal balances.
export function trialBalanceOf(journal) {
  const by = new Map();
  for (const e of journal) {
    for (const l of e.lines) {
      const row = by.get(l.account) || { account: l.account, name: ACCOUNTS[l.account]?.name || String(l.account), debit_cents: 0, credit_cents: 0 };
      row.debit_cents += l.debit_cents;
      row.credit_cents += l.credit_cents;
      by.set(l.account, row);
    }
  }
  const accounts = [...by.values()].sort((a, b) => a.account - b.account).map((r) => {
    const net = r.debit_cents - r.credit_cents;
    return { ...r, balance_cents: Math.abs(net), balance_side: net >= 0 ? 'debit' : 'credit' };
  });
  const debit = accounts.reduce((a, r) => a + r.debit_cents, 0);
  const credit = accounts.reduce((a, r) => a + r.credit_cents, 0);
  return { accounts, debit_cents: debit, credit_cents: credit, balanced: debit === credit };
}

/// The journal as CSV: one line per journal line, amounts in dollars.
export function journalCsv(journal) {
  const cell = (v) => (/[",\n]/.test(String(v)) ? `"${String(v).replace(/"/g, '""')}"` : String(v));
  const money = (c) => (c ? (c / 100).toFixed(2) : '');
  const rows = [['date', 'account', 'account_name', 'debit', 'credit', 'memo']];
  for (const e of journal) {
    for (const l of e.lines) rows.push([e.date, l.account, ACCOUNTS[l.account]?.name || '', money(l.debit_cents), money(l.credit_cents), e.memo]);
  }
  return `${rows.map((r) => r.map(cell).join(',')).join('\n')}\n`;
}

// -----------------------------------------------------------------------------
// 3. Stripe reconciliation
// -----------------------------------------------------------------------------

/// Stripe returns at most 100 balance transactions a page. A month larger than this many pages is reported as incomplete.
export const STRIPE_MAX_PAGES = 100;

/// Stripe's balance transactions for the month, summed by what they are, in cents. `deps.stripeRequest` is index.js's.
export async function stripeMonth(env, month, deps) {
  const [y, m] = month.split('-').map(Number);
  const from = Math.floor(Date.UTC(y, m - 1, 1) / 1000);
  const to = Math.floor(Date.UTC(y, m, 1) / 1000);
  const sums = { charges_cents: 0, fees_cents: 0, refunds_cents: 0, disputes_cents: 0, transfers_cents: 0, bank_payouts_cents: 0, other_cents: 0, count: 0 };
  let after = '';
  for (let page = 0; page < STRIPE_MAX_PAGES; page++) {
    const q = `/balance_transactions?limit=100&created[gte]=${from}&created[lt]=${to}${after ? `&starting_after=${after}` : ''}`;
    const res = await deps.stripeRequest('GET', q, null, env);
    if (res?.error) return { ok: false, error: res.error.message || 'Stripe refused the request' };
    const rows = Array.isArray(res?.data) ? res.data : [];
    for (const t of rows) {
      const amount = Math.trunc(Number(t.amount) || 0);
      sums.count += 1;
      sums.fees_cents += Math.trunc(Number(t.fee) || 0);
      if (t.type === 'charge' || t.type === 'payment') sums.charges_cents += amount;
      else if (t.type === 'refund' || t.type === 'payment_refund') sums.refunds_cents += -amount;
      else if (t.type === 'adjustment') sums.disputes_cents += -amount;
      else if (t.type === 'transfer') sums.transfers_cents += -amount;
      else if (t.type === 'payout') sums.bank_payouts_cents += -amount;
      else if (t.type === 'stripe_fee') sums.fees_cents += -amount;
      else sums.other_cents += amount;
    }
    if (!res?.has_more || rows.length === 0) return { ok: true, complete: true, ...sums };
    after = rows[rows.length - 1].id;
  }
  return { ok: true, complete: false, ...sums };
}

/// The journal's side of each comparison, from the month's parts.
export function ledgerSide({ totals, paid_cents = 0 }) {
  const sum = (section, field) => Object.values(sumSection([totals || {}], section)).reduce((a, r) => a + (r[field] || 0), 0);
  return {
    receipts_cents: sum('deposits', 'gross_cents'),
    fees_cents: sum('deposits', 'fee_cents'),
    reversed_cents: sum('reversals', 'gross_cents') - sum('restored', 'gross_cents'),
    paid_cents,
  };
}

/// Each comparison as { ledger_cents, stripe_cents, difference_cents, matches }. Stripe nets a dispute won against disputes, as the
/// ledger does. A record made within a moment of midnight UTC can fall in different months on the two sides.
export function compare(ledger, stripe) {
  const line = (l, s) => ({ ledger_cents: l, stripe_cents: s, difference_cents: l - s, matches: l === s });
  const lines = {
    receipts: line(ledger.receipts_cents, stripe.charges_cents),
    fees: line(ledger.fees_cents, stripe.fees_cents),
    refunds_and_disputes: line(ledger.reversed_cents, stripe.refunds_cents + stripe.disputes_cents),
    contributor_payouts: line(ledger.paid_cents, stripe.transfers_cents),
  };
  return {
    lines,
    matches: stripe.complete === true && Object.values(lines).every((l) => l.matches),
    complete: stripe.complete === true,
    stripe_transactions: stripe.count,
    bank_payouts_cents: stripe.bank_payouts_cents,
    other_cents: stripe.other_cents,
  };
}

// -----------------------------------------------------------------------------
// 4. Route
// -----------------------------------------------------------------------------

/// What the Treasury paid contributors for the month's score dates, in cents.
async function paidInMonth(env, month, now) {
  const period = resolvePeriod('month', month, now);
  let cents = 0;
  for (const d of payoutDates(period, now)) {
    const raw = await env.PAYOUTS.get(`payout:${d}`);
    if (raw) cents += toCents(JSON.parse(raw).total_paid_usd);
  }
  return cents;
}

/// The month's close: the journal, the trial balance, and (when asked) the reconciliation.
export async function closeMonth(env, month, { reconcile = false, deps, now = new Date() } = {}) {
  const got = await treasuryCall(env, 'period_totals', { months: [month] });
  if (!got.ok) return { error: got.error || 'Treasury unavailable', status: got.status || 503 };
  const totals = got.months[0];
  const paid_cents = await paidInMonth(env, month, now);
  const ledger = await costsOf(env, [month]);
  const costs = Object.fromEntries(COST_CATEGORIES.map((c) => [c, toCents(ledger.by_category[c])]));
  const parts = { totals, paid_cents, costs };
  const journal = journalOf(month, parts);
  const out = {
    month,
    generated_at: now.toISOString(),
    open: month >= now.toISOString().slice(0, 7),
    journal,
    trial_balance: trialBalanceOf(journal),
    notes: [
      'Amounts are whole cents. Entries are monthly totals by kind, dated the last day of the month.',
      'Receipts are what was paid in the month. Whether prepaid Tickets, Delta and plans are revenue when paid or when used is an accounting decision this export does not make.',
      'A gift to the Treasury is recorded with no payment fee; the fee Stripe charged on it shows as a difference in the fees comparison.',
    ],
  };
  if (reconcile) {
    if (!env.STRIPE_SECRET_KEY) out.reconciliation = { error: 'Stripe not configured' };
    else {
      const stripe = await stripeMonth(env, month, deps);
      out.reconciliation = stripe.ok ? compare(ledgerSide(parts), stripe) : { error: stripe.error };
    }
  }
  return out;
}

/// Handle /api/admin/books/*; null for any other path. `deps`: { json, requireAdmin, stripeRequest }.
export async function handleBooksRoute(request, url, env, cors, deps) {
  if (!url.pathname.startsWith('/api/admin/books/')) return null;
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return deps.json({ error: 'Admin access required' }, 403, cors);
  if (url.pathname !== '/api/admin/books/close' || request.method !== 'GET') return deps.json({ error: 'Not found' }, 404, cors);
  const month = url.searchParams.get('month') || new Date().toISOString().slice(0, 7);
  if (!MONTH.test(month)) return deps.json({ error: 'month is YYYY-MM' }, 400, cors);
  const close = await closeMonth(env, month, { reconcile: url.searchParams.get('reconcile') === '1', deps });
  if (close.error) return deps.json({ error: close.error }, close.status, cors);
  if (url.searchParams.get('format') === 'csv') {
    return new Response(journalCsv(close.journal), {
      status: 200,
      headers: { ...cors, 'Content-Type': 'text/csv; charset=utf-8', 'Content-Disposition': `attachment; filename="eustress-journal-${month}.csv"`, 'Cache-Control': 'no-store' },
    });
  }
  return deps.json(close, 200, { ...cors, 'Cache-Control': 'no-store' });
}
