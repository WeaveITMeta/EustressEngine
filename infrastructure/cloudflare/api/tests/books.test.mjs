// The monthly books close (src/books.mjs): the journal and trial balance built from the Treasury's own records, and the comparison
// with Stripe's balance transactions. Driven through the real Worker and the real Treasury object, with Stripe's API stubbed.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { Treasury, treasuryCall, splitSale } from '../src/treasury.mjs';
import { splitSponsorship } from '../src/sponsor.mjs';
import { journalOf, trialBalanceOf, journalCsv, compare, ledgerSide, ACCOUNTS } from '../src/books.mjs';

const worker = await loadWorker();
const ADMIN = '56565656-5656-4656-8656-565656565656';
const PLAYER = '12121212-1212-4212-8212-121212121212';
const ORIGIN = 'https://eustress.dev';
const month = () => new Date().toISOString().slice(0, 7);

function world() {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN, STRIPE_SECRET_KEY: 'sk_test_not_real' };
  env.TREASURY = fakeDoNamespace(Treasury, env);
  env.USERS.store.set(`user:${ADMIN}`, JSON.stringify({ id: ADMIN, username: 'admin_user', kyc_verified: true }));
  env.USERS.store.set(`admin:${ADMIN}`, '1');
  env.USERS.store.set(`user:${PLAYER}`, JSON.stringify({ id: PLAYER, username: 'player', kyc_verified: true }));
  return env;
}

async function deposit(env, ref, kind, split) {
  const r = await treasuryCall(env, 'deposit', { ref, kind, ...split, payment_intent: `pi_${ref}`, channel: 'web' });
  assert.equal(r.ok, true, JSON.stringify(r));
}

const web = (gross) => {
  const s = splitSale(gross, 'web');
  return { gross_cents: s.gross_cents, fee_cents: s.fee_cents, treasury_cents: s.treasury_cents, platform_cents: s.platform_cents };
};

/// A month with a Ticket pack, a plan, a sponsorship and a partly refunded Ticket pack.
async function busyMonth(env) {
  await deposit(env, 't1', 'ticket_purchase', web(999));
  await deposit(env, 't2', 'ticket_purchase', web(1999));
  await deposit(env, 'p1', 'subscription', web(1299));
  await deposit(env, 's1', 'engine_sponsor', splitSponsorship(5000));
  const refund = await treasuryCall(env, 'refund', { payment_intent: 'pi_t2', refunded_total_cents: 1999 });
  assert.equal(refund.ok, true, JSON.stringify(refund));
}

const get = (env, path, token = mintToken(ADMIN)) => worker.fetch(new Request(`https://api.eustress.dev${path}`, {
  headers: { Origin: ORIGIN, ...(token ? { Authorization: `Bearer ${token}` } : {}) },
}), env, ctx);

/// Run `fn` with Stripe's balance transactions answered from `pages` (arrays of transactions), one page per call.
async function withStripe(pages, fn) {
  const real = globalThis.fetch;
  const asked = [];
  globalThis.fetch = async (url) => {
    asked.push(String(url));
    const i = asked.length - 1;
    return new Response(JSON.stringify({ data: pages[i] || [], has_more: i + 1 < pages.length }), { status: 200 });
  };
  try {
    return { result: await fn(), asked };
  } finally {
    globalThis.fetch = real;
  }
}

// ── The journal ──────────────────────────────────────────────────────────────

test('every entry balances, and the trial balance carries the month to the cent', async () => {
  const env = world();
  await busyMonth(env);
  const res = await get(env, `/api/admin/books/close?month=${month()}`);
  assert.equal(res.status, 200);
  const close = await res.json();
  for (const e of close.journal) {
    const d = e.lines.reduce((a, l) => a + l.debit_cents, 0);
    const c = e.lines.reduce((a, l) => a + l.credit_cents, 0);
    assert.equal(d, c, e.memo);
    assert.ok(d > 0, `no empty entries: ${e.memo}`);
  }
  const tb = close.trial_balance;
  assert.equal(tb.balanced, true);
  assert.equal(tb.debit_cents, tb.credit_cents);
  const acct = (n) => tb.accounts.find((a) => a.account === n) || { debit_cents: 0, credit_cents: 0 };
  // Receipts by kind are the gross of each kind.
  assert.equal(acct(4000).credit_cents, 999 + 1999);
  assert.equal(acct(4020).credit_cents, 1299);
  assert.equal(acct(4040).credit_cents, 5000);
  // Fees are the sum of each payment's fee.
  const fees = web(999).fee_cents + web(1999).fee_cents + web(1299).fee_cents + splitSponsorship(5000).fee_cents;
  assert.equal(acct(5000).debit_cents, fees);
  // The refund left the Stripe balance whole.
  assert.equal(acct(4900).debit_cents, 1999);
  assert.equal(acct(1000).debit_cents - acct(1000).credit_cents, 999 + 1999 + 1299 + 5000 - fees - 1999);
  // The Treasury holds its share of the sales that stand, and none of the sponsorship.
  const state = (await treasuryCall(env, 'snapshot')).state;
  assert.equal(acct(2000).credit_cents - acct(2000).debit_cents, state.cents);
  assert.equal(state.cents, web(999).treasury_cents + web(1299).treasury_cents);
});

test('contributor payouts and the cost ledger are entries of their own', () => {
  const journal = journalOf('2026-02', { totals: { deposits: {}, reversals: {}, restored: {} }, paid_cents: 1234, costs: { servers: 47000, domain: 83 } });
  assert.deepEqual(journal.map((e) => [e.date, e.memo]), [
    ['2026-02-28', 'Paid to contributors from the Treasury'], ['2026-02-28', 'Cost ledger: servers'], ['2026-02-28', 'Cost ledger: domain'],
  ]);
  const tb = trialBalanceOf(journal);
  assert.equal(tb.balanced, true);
  assert.deepEqual(tb.accounts.map((a) => [a.account, a.balance_cents, a.balance_side]), [
    [1000, 1234, 'credit'], [2000, 1234, 'debit'], [2100, 47083, 'credit'], [6010, 47000, 'debit'], [6020, 83, 'debit'],
  ]);
  assert.deepEqual(journalOf('2026-02', { totals: null }), [], 'an empty month has no entries');
});

test('a dispute won puts the money and the Treasury\'s share back', () => {
  const row = { count: 1, gross_cents: 1000, treasury_cents: 300, shortfall_cents: 0, platform_cents: 600 };
  const journal = journalOf('2026-03', { totals: { deposits: {}, reversals: { ticket_purchase: row }, restored: { ticket_purchase: row } } });
  const tb = trialBalanceOf(journal);
  assert.equal(tb.balanced, true);
  for (const a of tb.accounts) assert.equal(a.balance_cents, 0, `${a.account} nets to zero`);
});

test('an unknown deposit kind is kept, under other receipts', () => {
  const journal = journalOf('2026-03', { totals: { deposits: { mystery: { count: 1, gross_cents: 500, fee_cents: 45, treasury_cents: 0 } } } });
  assert.deepEqual(journal[0].lines, [{ account: 1000, debit_cents: 455, credit_cents: 0 }, { account: 5000, debit_cents: 45, credit_cents: 0 }, { account: 4090, debit_cents: 0, credit_cents: 500 }]);
  for (const e of journal) for (const l of e.lines) assert.ok(ACCOUNTS[l.account], `account ${l.account} is in the chart`);
});

test('the CSV has one line per journal line, in dollars, with a quoted memo when it needs one', async () => {
  const env = world();
  await busyMonth(env);
  const res = await get(env, `/api/admin/books/close?month=${month()}&format=csv`);
  assert.equal(res.status, 200);
  assert.match(res.headers.get('Content-Type'), /^text\/csv/);
  const lines = (await res.text()).trim().split('\n');
  assert.equal(lines[0], 'date,account,account_name,debit,credit,memo');
  const close = await (await get(env, `/api/admin/books/close?month=${month()}`)).json();
  assert.equal(lines.length - 1, close.journal.reduce((a, e) => a + e.lines.length, 0));
  assert.ok(lines.some((l) => l.includes(',4040,Receipts: Engine sponsorships,,50.00,')), lines.join('\n'));
  assert.equal(journalCsv([{ date: '2026-01-31', memo: 'a, "b"', lines: [{ account: 1000, debit_cents: 5, credit_cents: 0 }] }]).split('\n')[1], '2026-01-31,1000,Stripe balance,0.05,,"a, ""b"""');
});

// ── Reconciliation ───────────────────────────────────────────────────────────

/// Stripe's side of the busy month, as balance transactions.
function stripeSide() {
  const charge = (id, gross, fee) => ({ id, type: 'charge', amount: gross, fee });
  return [
    charge('txn_1', 999, web(999).fee_cents), charge('txn_2', 1999, web(1999).fee_cents), charge('txn_3', 1299, web(1299).fee_cents),
    charge('txn_4', 5000, splitSponsorship(5000).fee_cents), { id: 'txn_5', type: 'refund', amount: -1999, fee: 0 },
  ];
}

test('a month Stripe agrees with reconciles, across pages', async () => {
  const env = world();
  await busyMonth(env);
  const all = stripeSide();
  const { result, asked } = await withStripe([all.slice(0, 3), all.slice(3)], async () => (await get(env, `/api/admin/books/close?month=${month()}&reconcile=1`)).json());
  const rec = result.reconciliation;
  assert.equal(rec.matches, true, JSON.stringify(rec));
  assert.equal(rec.stripe_transactions, 5);
  assert.deepEqual([rec.lines.receipts.ledger_cents, rec.lines.refunds_and_disputes.stripe_cents], [9297, 1999]);
  assert.equal(asked.length, 2);
  assert.match(asked[0], /^https:\/\/api\.stripe\.com\/v1\/balance_transactions\?limit=100&created\[gte\]=\d+&created\[lt\]=\d+$/);
  assert.match(asked[1], /&starting_after=txn_3$/);
});

test('a payment Stripe has and the ledger does not is reported as a difference, in cents', async () => {
  const env = world();
  await busyMonth(env);
  const extra = [...stripeSide(), { id: 'txn_6', type: 'charge', amount: 2500, fee: 103 }];
  const { result } = await withStripe([extra], async () => (await get(env, `/api/admin/books/close?month=${month()}&reconcile=1`)).json());
  const rec = result.reconciliation;
  assert.equal(rec.matches, false);
  assert.deepEqual([rec.lines.receipts.difference_cents, rec.lines.fees.difference_cents], [-2500, -103]);
  assert.equal(rec.lines.refunds_and_disputes.matches, true);
});

test('a comparison that could not read all of Stripe never says it matches', () => {
  const ledger = ledgerSide({ totals: { deposits: {}, reversals: {}, restored: {} }, paid_cents: 0 });
  const stripe = { complete: false, charges_cents: 0, fees_cents: 0, refunds_cents: 0, disputes_cents: 0, transfers_cents: 0, bank_payouts_cents: 0, other_cents: 0, count: 10000 };
  assert.equal(compare(ledger, stripe).matches, false);
  assert.equal(compare(ledger, { ...stripe, complete: true }).matches, true);
});

test('Stripe refusing the read is reported, and the journal is still returned', async () => {
  const env = world();
  await busyMonth(env);
  const real = globalThis.fetch;
  globalThis.fetch = async () => new Response(JSON.stringify({ error: { message: 'Invalid API Key' } }), { status: 401 });
  try {
    const close = await (await get(env, `/api/admin/books/close?month=${month()}&reconcile=1`)).json();
    assert.deepEqual(close.reconciliation, { error: 'Invalid API Key' });
    assert.equal(close.trial_balance.balanced, true);
  } finally {
    globalThis.fetch = real;
  }
});

// ── Who may read it ──────────────────────────────────────────────────────────

test('the close is for administrators, takes a real month, and reads nothing from Stripe unless asked', async () => {
  const env = world();
  assert.equal((await get(env, '/api/admin/books/close', null)).status, 403);
  assert.equal((await get(env, '/api/admin/books/close', mintToken(PLAYER))).status, 403);
  assert.equal((await get(env, '/api/admin/books/close?month=2026-13')).status, 400);
  assert.equal((await get(env, '/api/admin/books/other')).status, 404);
  const { result, asked } = await withStripe([], async () => get(env, `/api/admin/books/close?month=${month()}`));
  assert.equal(result.status, 200);
  assert.equal(asked.length, 0);
  assert.equal((await result.json()).open, true, 'the current month is still open');
});
