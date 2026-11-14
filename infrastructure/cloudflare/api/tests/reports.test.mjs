// Drafts of Form 1-K, Form 1-SA and the offering circular (src/reports.mjs): the periods and due dates, the figures from the
// books, the ownership table from the holder ledger, the checks that mark a draft not ready, and HTML that keeps to what EDGAR
// accepts. Driven through the real Worker, the real Treasury and the real HolderLedger on in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { Treasury, treasuryCall, splitSale } from '../src/treasury.mjs';
import { HolderLedger } from '../src/holder_ledger.mjs';
import { journalOf } from '../src/books.mjs';
import { periodOf, dueDate, fiscalYearEndMonth, figuresOf, namedHolders, edgarText, draftHtml, draftOf, checksOf } from '../src/reports.mjs';

const worker = await loadWorker();
const ADMIN = '56565656-5656-4656-8656-565656565656';
const ADA = '12121212-1212-4212-8212-121212121212';
const BEN = '34343434-3434-4434-8434-343434343434';
const CAT = '90909090-9090-4090-8090-909090909090';
const ORIGIN = 'https://eustress.dev';
const YEAR = new Date().getUTCFullYear();

function world(extra = {}) {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN, ...extra };
  env.TREASURY = fakeDoNamespace(Treasury, env);
  env.HOLDER_LEDGER = fakeDoNamespace(HolderLedger, env);
  for (const id of [ADMIN, ADA]) env.USERS.store.set(`user:${id}`, JSON.stringify({ id, username: id.slice(0, 4), kyc_verified: true }));
  env.USERS.store.set(`admin:${ADMIN}`, '1');
  return env;
}

const get = async (env, path, as = ADMIN) => worker.fetch(new Request(`https://api.eustress.dev${path}`, {
  headers: { Origin: ORIGIN, ...(as ? { Authorization: `Bearer ${mintToken(as)}` } : {}) },
}), env, ctx);
const send = async (env, method, path, body) => (await worker.fetch(new Request(`https://api.eustress.dev${path}`, {
  method, headers: { Origin: ORIGIN, 'Content-Type': 'application/json', Authorization: `Bearer ${mintToken(ADMIN)}` }, body: JSON.stringify(body),
}), env, ctx)).json();

const web = (gross) => {
  const s = splitSale(gross, 'web');
  return { gross_cents: s.gross_cents, fee_cents: s.fee_cents, treasury_cents: s.treasury_cents, platform_cents: s.platform_cents };
};

async function holders(env) {
  for (const [id, name, acc] of [[ADA, 'Ada Lovelace', false], [BEN, 'Ben Franklin', true], [CAT, 'Cat Smith', false]]) {
    await send(env, 'PUT', '/api/admin/holders/record', { holder_id: id, legal_name: name, country: 'US', accredited: acc });
  }
  await send(env, 'POST', '/api/admin/holders/issue', { ref: 'a', to: ADA, units: 600 });
  await send(env, 'POST', '/api/admin/holders/issue', { ref: 'b', to: BEN, units: 300 });
  await send(env, 'POST', '/api/admin/holders/issue', { ref: 'c', to: CAT, units: 100 });
}

// ── Periods ──────────────────────────────────────────────────────────────────

test('a fiscal year and its first half, with the due dates the rules set', () => {
  assert.deepEqual(periodOf('1-K', 2027, 12), { from: '2027-01-01', to: '2027-12-31', months: Array.from({ length: 12 }, (_, i) => `2027-${String(i + 1).padStart(2, '0')}`) });
  const june = periodOf('1-K', 2027, 6);
  assert.deepEqual([june.from, june.to, june.months.length, june.months[0], june.months[11]], ['2026-07-01', '2027-06-30', 12, '2026-07', '2027-06']);
  const half = periodOf('1-SA', 2027, 6);
  assert.deepEqual([half.from, half.to, half.months.length], ['2026-07-01', '2026-12-31', 6]);
  assert.equal(periodOf('1-SA', 2028, 12).to, '2028-06-30');
  assert.equal(dueDate('1-K', '2027-12-31'), '2028-04-29', '120 days');
  assert.equal(dueDate('1-SA', '2027-06-30'), '2027-09-28', '90 days');
  assert.equal(dueDate('circular', '2027-12-31'), null);
  assert.deepEqual([fiscalYearEndMonth({}), fiscalYearEndMonth({ FISCAL_YEAR_END_MONTH: '6' }), fiscalYearEndMonth({ FISCAL_YEAR_END_MONTH: '13' })], [12, 6, 12]);
});

// ── Figures ──────────────────────────────────────────────────────────────────

test('the statement of receipts and costs adds up from the journals, across months', () => {
  const totals = (gross, fee, treasury) => ({ deposits: { ticket_purchase: { gross_cents: gross, fee_cents: fee, treasury_cents: treasury, count: 1 } } });
  const jan = journalOf('2027-01', { totals: { ...totals(10_000, 320, 1_000), reversals: { ticket_purchase: { gross_cents: 2_000, treasury_cents: 200, count: 1 } } }, paid_cents: 500, costs: { cloudflare: 2_500 } });
  const feb = journalOf('2027-02', { totals: totals(5_000, 175, 500), costs: { tools: 1_000 } });
  const f = figuresOf([jan, feb]);
  assert.equal(f.balanced, true);
  assert.deepEqual([f.receipts_cents, f.refunds_cents, f.net_receipts_cents, f.fees_cents, f.treasury_share_cents, f.costs_cents], [15_000, 2_000, 13_000, 495, 1_300, 3_500]);
  assert.equal(f.result_cents, 13_000 - 495 - 1_300 - 3_500);
  // The Stripe balance gained net receipts after fees, lost the refund and the payout.
  assert.equal(f.movement.stripe_balance_cents, 15_000 - 495 - 2_000 - 500);
  assert.equal(f.movement.contributor_treasury_cents, 1_300 - 500);
  assert.equal(f.movement.paid_outside_stripe_cents, 3_500);
  assert.deepEqual(figuresOf([]).result_cents, 0);
});

test('the ownership table names holders of 10% or more, and nobody when nothing is issued', () => {
  const hs = [{ legal_name: 'A', units: 600 }, { legal_name: 'B', units: 100 }, { legal_name: 'C', units: 99 }, { legal_name: 'D', units: 201 }];
  assert.deepEqual(namedHolders(hs, 1000).map((h) => [h.legal_name, h.percent]), [['A', 60], ['B', 10], ['D', 20.1]]);
  assert.deepEqual(namedHolders(hs, 0), []);
});

// ── EDGAR ────────────────────────────────────────────────────────────────────

test('text for EDGAR is ASCII: markup escaped, typography plain, Latin-1 as references, the rest counted as lost', () => {
  assert.deepEqual(edgarText('A & B <c> “q” — it’s'), { text: 'A &amp; B &lt;c&gt; "q" - it\'s', lost: 0 });
  assert.deepEqual(edgarText('Café'), { text: 'Caf&#233;', lost: 0 });
  assert.deepEqual(edgarText('中\u{1F600}x'), { text: '??x', lost: 2 });
});

const ALLOWED_TAGS = new Set(['html', 'head', 'title', 'body', 'h1', 'h2', 'p', 'b', 'ul', 'li', 'table', 'tr', 'th', 'td']);
function assertEdgarSafe(html) {
  assert.match(html, /^[\x09\x0A\x0D\x20-\x7E]*$/, 'ASCII only');
  for (const [, tag] of html.matchAll(/<\/?([a-zA-Z0-9]+)/g)) assert.ok(ALLOWED_TAGS.has(tag.toLowerCase()), `tag ${tag}`);
  assert.doesNotMatch(html, /<script|<img|<a\s|href=|src=|style=|<link|javascript:/i);
}

// ── Drafts through the Worker ────────────────────────────────────────────────

test('a 1-K draft carries the year\'s figures, the ownership table, a gap for every judgment, and says it is a draft', async () => {
  const env = world({ COMPANY_LEGAL_NAME: 'Eustress Test LLC' });
  await treasuryCall(env, 'deposit', { ref: 't1', kind: 'ticket_purchase', ...web(10_000), payment_intent: 'pi_t1', channel: 'web' });
  await holders(env);
  const res = await get(env, `/api/admin/reports/draft?kind=1-K&year=${YEAR}`);
  assert.equal(res.status, 200);
  const d = await res.json();
  assert.equal(d.title, 'Form 1-K');
  assert.equal(d.figures.receipts_cents, 10_000);
  assert.equal(d.due_on, dueDate('1-K', `${YEAR}-12-31`));
  assert.equal(d.ready, false, 'the year is not over');
  assert.deepEqual(d.checks.filter((c) => !c.ok).map((c) => c.name), ['The period is over']);
  assert.ok(d.gaps >= 8, `gaps ${d.gaps}`);
  const titles = d.sections.map((s) => s.title);
  for (const item of ['Item 1. Business', 'Item 4. Security Ownership of Management and Certain Securityholders', 'Item 7. Financial Statements', 'Item 8. Exhibits']) assert.ok(titles.includes(item), item);
  const ownership = d.sections.find((s) => s.title.startsWith('Item 4')).body.find((b) => b.type === 'table');
  assert.deepEqual(ownership.rows, [['Ada Lovelace', '600', '60.00%'], ['Ben Franklin', '300', '30.00%'], ['Cat Smith', '100', '10.00%']]);
  assert.match(d.html, /DRAFT\. NOT FILED\./);
  assert.match(d.html, /NOT READY\./);
  assert.match(d.html, /Eustress Test LLC/);
  assert.match(d.html, /\$100\.00/);
  assertEdgarSafe(d.html);
  assert.equal(d.characters_replaced, 0);
});

test('a broken ledger or unbalanced books mark the draft not ready, by name', () => {
  const base = {
    kind: '1-SA', period: periodOf('1-SA', 2027), period_over: true, due_on: '2027-09-28',
    months: [{ month: '2027-01', balanced: true }, { month: '2027-02', balanced: false }],
    figures: figuresOf([]), holders: { holders: [], issued_units: 0, counts: { holders_of_record: 0, non_accredited: 0 } },
    ledger_verify: { sound: false, problems: ['entry 3 was changed'] }, offerings: [{ offering_id: 'units-2027', counts_sound: false }],
  };
  const failed = checksOf(base).filter((c) => !c.ok);
  assert.deepEqual(failed.map((c) => [c.name, c.detail]), [
    ['Every month\'s trial balance balances', '2027-02'],
    ['The holder ledger verifies', 'entry 3 was changed'],
    ['Each offering\'s units match its subscriptions', 'units-2027'],
  ]);
  const sound = { ...base, months: [{ month: '2027-01', balanced: true }], ledger_verify: { sound: true, problems: [] }, offerings: [{ offering_id: 'units-2027', counts_sound: true }] };
  const draft = draftOf(sound);
  assert.equal(draft.ready, true);
  const { html } = draftHtml(draft);
  assert.doesNotMatch(html, /NOT READY/);
  assert.match(html, /\[TO BE WRITTEN: The accountant prepares the interim financial statements/);
  assertEdgarSafe(html);
});

test('a 1-SA covers the first half; the circular fills the offering\'s terms and plan of distribution', async () => {
  const env = world();
  await holders(env);
  const sa = await (await get(env, `/api/admin/reports/draft?kind=1-SA&year=${YEAR}`)).json();
  assert.deepEqual([sa.title, sa.period.from, sa.period.to], ['Form 1-SA', `${YEAR}-01-01`, `${YEAR}-06-30`]);
  assert.ok(sa.sections.some((s) => s.title === 'Item 3. Financial Statements'));

  const none = await (await get(env, `/api/admin/reports/draft?kind=circular&year=${YEAR}`)).json();
  assert.deepEqual(none.checks.filter((c) => !c.ok).map((c) => c.name).includes('An offering is named'), true);

  await send(env, 'PUT', '/api/admin/offerings', {
    offering_id: 'units-2027', title: 'Eustress units', price_cents: 500, max_units: 1000, min_units: 10, states: ['AZ', 'CA'],
    circular: { url: 'https://eustress.dev/c.pdf', sha256: 'a'.repeat(64) }, agreement: { url: 'https://eustress.dev/a.pdf', sha256: 'b'.repeat(64) },
    payment_instructions: 'Wire.',
  });
  const c = await (await get(env, `/api/admin/reports/draft?kind=circular&year=${YEAR}`)).json();
  assert.equal(c.title, 'Offering Circular');
  const terms = c.sections[0].body.find((b) => b.type === 'table').rows;
  assert.deepEqual(terms.find((r) => r[0] === 'Price per unit'), ['Price per unit', '$5.00']);
  assert.deepEqual(terms.find((r) => r[0] === 'SEC file number'), ['SEC file number', 'not qualified']);
  const plan = c.sections.find((s) => s.title === 'Plan of Distribution').body[0].text;
  assert.match(plan, /no underwriter and no commission/);
  assert.match(plan, /AZ, CA/);
  assert.match(plan, /10 units/);
  assertEdgarSafe(c.html);
  assert.equal((await get(env, `/api/admin/reports/draft?kind=circular&year=${YEAR}&offering_id=nope`)).status, 404);
});

test('the HTML download is an attachment, and only administrators get any of it', async () => {
  const env = world();
  const res = await get(env, `/api/admin/reports/draft?kind=1-K&year=${YEAR}&format=html`);
  assert.equal(res.status, 200);
  assert.match(res.headers.get('content-type'), /text\/html; charset=us-ascii/);
  assert.match(res.headers.get('content-disposition'), new RegExp(`eustress-1-k-${YEAR}-draft\\.htm`));
  assertEdgarSafe(await res.text());
  assert.equal((await get(env, `/api/admin/reports/draft?kind=1-K&year=${YEAR}`, ADA)).status, 403);
  assert.equal((await get(env, `/api/admin/reports/draft?kind=1-K&year=${YEAR}`, null)).status, 403);
  assert.equal((await get(env, '/api/admin/reports/draft?kind=10-K&year=2027')).status, 400);
  assert.equal((await get(env, '/api/admin/reports/draft?kind=1-K&year=27')).status, 400);
  assert.equal((await get(env, '/api/admin/reports/other')).status, 404);
});

test('a deployment without the ledger bound answers an error, never a draft with made-up ownership', async () => {
  const env = world();
  delete env.HOLDER_LEDGER;
  const res = await get(env, `/api/admin/reports/draft?kind=1-K&year=${YEAR}`);
  assert.equal(res.status, 503);
});
