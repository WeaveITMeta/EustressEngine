// The offering and its subscriptions (src/subscriptions.mjs): nothing is offered until an administrator opens an offering, a person
// prepares and signs, an administrator records funds and accepts, and the units land on the holder ledger exactly once. Driven
// through the real Worker and the real HolderLedger object on in-memory storage.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { HolderLedger } from '../src/holder_ledger.mjs';
import { investmentLimitCents, ATTESTATIONS, TESTING_LEGEND, TIER2_CAP_CENTS } from '../src/subscriptions.mjs';

const worker = await loadWorker();
const ADMIN = '56565656-5656-4656-8656-565656565656';
const ADA = '12121212-1212-4212-8212-121212121212';
const BEN = '34343434-3434-4434-8434-343434343434';
const NEW = '78787878-7878-4878-8878-787878787878';
const ORIGIN = 'https://eustress.dev';
const CIRCULAR = { url: 'https://eustress.dev/offering/circular.pdf', sha256: 'a'.repeat(64) };
const AGREEMENT = { url: 'https://eustress.dev/offering/agreement.pdf', sha256: 'b'.repeat(64) };
const ALL_TRUE = Object.fromEntries(ATTESTATIONS.map((k) => [k, true]));

function world() {
  const env = { ...fakeEnv(), PAYOUTS: fakeKv(), LOGIN_ORIGINS: ORIGIN };
  env.HOLDER_LEDGER = fakeDoNamespace(HolderLedger, env);
  for (const [id, username, kyc] of [[ADMIN, 'admin_user', true], [ADA, 'ada', true], [BEN, 'ben', true], [NEW, 'newcomer', false]]) {
    env.USERS.store.set(`user:${id}`, JSON.stringify({ id, username, kyc_verified: kyc }));
  }
  env.USERS.store.set(`admin:${ADMIN}`, '1');
  // The sign-up verification, linked to each verified account as sign-up leaves it: the name the document check read, and the
  // name typed at sign-up as the fallback (which a subscription never uses).
  env.KYC_STATUS = fakeKv();
  for (const [id, name] of [[ADA, 'Ada Lovelace'], [BEN, 'Ben Franklin']]) {
    env.KYC_STATUS.store.set(`kyc-${id}-front`, JSON.stringify(signUpRecord(id, name)));
  }
  return env;
}

function signUpRecord(userId, documentName, extra = {}) {
  return {
    user_id: userId, status: 'linked', verified_at: '2026-10-01T12:00:00.000Z', jurisdiction: 'US', age_verified: true,
    ocr_name: documentName || 'Typed At Signup', grok_analysis: { doc_decision: 'APPROVE', extracted_name: documentName }, ...extra,
  };
}

const call = async (env, method, path, { as = ADMIN, body } = {}) => {
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, {
    method,
    headers: { Origin: ORIGIN, 'Content-Type': 'application/json', ...(as ? { Authorization: `Bearer ${mintToken(as)}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  }), env, ctx);
  return { status: res.status, json: await res.json() };
};

const TERMS = { offering_id: 'units-2027', title: 'Eustress units', price_cents: 500, max_units: 1000, min_units: 10, states: ['AZ', 'CA'], circular: CIRCULAR, agreement: AGREEMENT, payment_instructions: 'Wire to the offering account, quoting your reference.' };
const QUALIFIED = { offering_id: 'units-2027', status: 'open', sec_file_number: '024-12345', qualified_on: '2026-11-01' };

async function openOffering(env, terms = {}) {
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, ...terms } })).status, 200);
  const opened = await call(env, 'POST', '/api/admin/offerings/status', { body: QUALIFIED });
  assert.equal(opened.status, 200, JSON.stringify(opened.json));
}

const ADA_ASKS = { offering_id: 'units-2027', units: 100, legal_name: 'Ada Lovelace', country: 'US', state: 'AZ', accredited: false, annual_income_cents: 6_000_000, net_worth_cents: 2_000_000 };
const prepare = (env, as, over = {}) => call(env, 'POST', '/api/subscriptions', { as, body: { ...ADA_ASKS, ...over } });
const sign = (env, as, id, over = {}) => call(env, 'POST', `/api/subscriptions/${id}/sign`, {
  as, body: { typed_name: 'Ada Lovelace', circular_sha256: CIRCULAR.sha256, agreement_sha256: AGREEMENT.sha256, attest: ALL_TRUE, ...over },
});
// Recorded funds carry the administrator's check that the paying account is in the subscriber's name, unless a test says otherwise.
const admin = (env, id, act, body = {}) => call(env, 'POST', `/api/admin/subscriptions/${id}/${act}`, { body: act === 'funds' ? { payer_name_matches: true, ...body } : body });
const offering = async (env) => (await call(env, 'GET', '/api/admin/offerings')).json.offerings[0];

async function signed(env, as = ADA, over = {}, signOver = {}) {
  const made = await prepare(env, as, over);
  assert.equal(made.status, 200, JSON.stringify(made.json));
  const s = await sign(env, as, made.json.subscription.id, signOver);
  assert.equal(s.status, 200, JSON.stringify(s.json));
  return s.json.subscription;
}

// ── The whole path ───────────────────────────────────────────────────────────

test('a subscription is prepared, signed, funded and accepted, and its units are on the ledger once', async () => {
  const env = world();
  await openOffering(env);
  const made = await prepare(env, ADA);
  assert.deepEqual([made.json.subscription.status, made.json.subscription.amount_cents, made.json.subscription.limit_cents], ['draft', 50_000, 600_000]);
  assert.equal((await offering(env)).reserved_units, 0, 'a prepared subscription reserves nothing');
  const id = made.json.subscription.id;

  const s = await sign(env, ADA, id);
  assert.equal(s.json.subscription.status, 'awaiting_funds');
  assert.match(s.json.subscription.payment_ref, /^EUS-[0-9A-F]{12}$/);
  assert.equal(s.json.subscription.payment_instructions, TERMS.payment_instructions);
  assert.equal((await offering(env)).reserved_units, 100);

  assert.equal((await admin(env, id, 'accept')).json.code, 'funds_not_received', 'no acceptance before funds');
  assert.equal((await admin(env, id, 'funds', { amount_cents: 49_999, bank_ref: 'W1' })).json.code, 'amount_mismatch');
  assert.equal((await admin(env, id, 'funds', { amount_cents: 50_000, bank_ref: 'W1', payer_name_matches: false })).json.code, 'payer_not_confirmed', 'money from an account in another name is not used');
  const funded = await admin(env, id, 'funds', { amount_cents: 50_000, bank_ref: 'W1' });
  assert.equal(funded.status, 200);
  assert.equal(funded.json.subscription.funds.payer_confirmed_by, ADMIN);
  const accepted = await admin(env, id, 'accept');
  assert.equal(accepted.status, 200, JSON.stringify(accepted.json));
  assert.deepEqual([accepted.json.entry.kind, accepted.json.entry.units, accepted.json.entry.price_cents, accepted.json.entry.ref, accepted.json.entry.by], ['issue', 100, 500, `sub:${id}`, ADMIN]);
  assert.equal((await admin(env, id, 'accept')).json.replay, true, 'a second acceptance issues nothing');

  const holders = (await call(env, 'GET', '/api/admin/holders')).json;
  assert.deepEqual(holders.holders.map((h) => [h.holder_id, h.units, h.legal_name, h.accredited]), [[ADA, 100, 'Ada Lovelace', false]]);
  assert.deepEqual([holders.issued_units, holders.entries], [100, 1]);
  assert.equal((await call(env, 'GET', '/api/admin/holders/verify')).json.sound, true);
  const o = await offering(env);
  assert.deepEqual([o.reserved_units, o.sold_units, o.raised_cents, o.counts_sound], [0, 100, 50_000, true]);

  const mine = (await call(env, 'GET', '/api/subscriptions', { as: ADA })).json.subscriptions;
  assert.deepEqual([mine.length, mine[0].status, mine[0].accepted.entry_seq], [1, 'accepted', 1]);
  assert.equal(JSON.stringify(mine).includes(ADMIN), false, 'a subscriber never sees which administrator acted');
  assert.equal((await call(env, 'GET', '/api/holdings', { as: ADA })).json.units, 100);
});

// ── Nothing is offered until an administrator opens it ───────────────────────

test('with no offering, or a draft, the public sees none and nobody can subscribe', async () => {
  const env = world();
  assert.equal((await call(env, 'GET', '/api/offering', { as: null })).json.offering, null);
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: TERMS })).status, 200);
  assert.equal((await call(env, 'GET', '/api/offering', { as: null })).json.offering, null, 'a draft is not public');
  assert.equal((await prepare(env, ADA)).json.code, 'not_open');
  assert.equal((await call(env, 'PUT', '/api/offering/interest', { as: ADA, body: { amount_cents: 1000 } })).json.code, 'not_testing');
});

test('an offering opens only with the SEC file number, the qualification date, its documents, a state and payment instructions', async () => {
  const env = world();
  await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, circular: null, states: [], payment_instructions: '' } });
  const open = (over) => call(env, 'POST', '/api/admin/offerings/status', { body: { ...QUALIFIED, ...over } });
  assert.equal((await open({ sec_file_number: undefined })).json.code, 'qualification_required');
  assert.equal((await open({ sec_file_number: '333-12345' })).json.code, 'qualification_required', 'a Regulation A file number begins 024-');
  assert.equal((await open({ qualified_on: '2999-01-01' })).json.code, 'qualification_required', 'not a date in the future');
  assert.equal((await open({})).json.code, 'documents_required');
  await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, states: [], payment_instructions: '' } });
  assert.equal((await open({})).json.code, 'states_required');
  await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, payment_instructions: '' } });
  assert.equal((await open({})).json.code, 'payment_instructions_required');
  await call(env, 'PUT', '/api/admin/offerings', { body: TERMS });
  assert.equal((await open({})).status, 200);
  const seen = (await call(env, 'GET', '/api/offering', { as: null })).json.offering;
  assert.deepEqual([seen.status, seen.sec_file_number, seen.units_available, seen.legend], ['open', '024-12345', 1000, null]);
  assert.equal('payment_instructions' in seen, false, 'payment instructions go to a signed subscriber only');
});

test('an offering cannot be larger than Tier 2 allows, and only administrators change one', async () => {
  const env = world();
  const big = await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, price_cents: 1000, max_units: TIER2_CAP_CENTS / 1000 + 1 } });
  assert.equal(big.json.code, 'over_tier2_cap');
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, price_cents: 1000, max_units: TIER2_CAP_CENTS / 1000 } })).status, 200);
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { as: ADA, body: TERMS })).status, 403);
  assert.equal((await call(env, 'POST', '/api/admin/offerings/status', { as: ADA, body: QUALIFIED })).status, 403);
  assert.equal((await call(env, 'GET', '/api/admin/subscriptions', { as: ADA })).status, 403);
});

test('testing the waters takes indications of interest and no subscriptions', async () => {
  const env = world();
  await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, circular: null, agreement: null } });
  assert.equal((await call(env, 'POST', '/api/admin/offerings/status', { body: { offering_id: 'units-2027', status: 'testing' } })).status, 200);
  const seen = (await call(env, 'GET', '/api/offering', { as: null })).json.offering;
  assert.deepEqual([seen.status, seen.legend, seen.units_available], ['testing', TESTING_LEGEND, null]);
  const said = await call(env, 'PUT', '/api/offering/interest', { as: ADA, body: { amount_cents: 250_000 } });
  assert.deepEqual([said.status, said.json.legend], [200, TESTING_LEGEND]);
  await call(env, 'PUT', '/api/offering/interest', { as: ADA, body: { amount_cents: 300_000 } });
  await call(env, 'PUT', '/api/offering/interest', { as: BEN, body: { amount_cents: 100_000 } });
  assert.deepEqual((await offering(env)).interest, { people: 2, amount_cents: 400_000 }, 'one indication per person, the latest');
  assert.equal((await prepare(env, ADA)).json.code, 'not_open', 'no subscription while testing');
  assert.equal((await call(env, 'DELETE', '/api/offering/interest', { as: BEN })).status, 200);
  assert.equal((await offering(env)).interest.people, 1);
  assert.equal((await call(env, 'GET', '/api/admin/holders')).json.entries, 0, 'interest issues nothing');
});

// ── Who may subscribe ────────────────────────────────────────────────────────

test('a subscriber is signed in, identity-verified, in the United States and in a state the offering names', async () => {
  const env = world();
  await openOffering(env);
  assert.equal((await prepare(env, null)).status, 401);
  assert.equal((await prepare(env, NEW)).status, 403, 'no verified identity');
  // The platform refuses an unverified account's writes before this module sees them; a read reaches the module's own check.
  assert.equal((await call(env, 'GET', '/api/subscriptions', { as: NEW })).json.code, 'identity_required');
  assert.equal((await prepare(env, ADA, { country: 'CA' })).json.code, 'country_not_offered');
  assert.equal((await prepare(env, ADA, { state: 'NY' })).json.code, 'state_not_offered');
  assert.equal((await prepare(env, ADA, { units: 9 })).json.code, 'below_minimum');
  assert.equal((await prepare(env, ADA, { annual_income_cents: undefined })).status, 400, 'a subscriber who is not accredited states income and net worth');
  assert.equal((await call(env, 'GET', '/api/admin/subscriptions')).json.total, 0, 'a refusal leaves nothing');
});

test('the investment limit is 10% of the greater of income or net worth, across the offering', async () => {
  assert.equal(investmentLimitCents(6_000_000, 2_000_000), 600_000);
  assert.equal(investmentLimitCents(2_000_000, 9_000_001), 900_000);
  const env = world();
  await openOffering(env, { max_units: 100_000 });
  // The limit is $6,000: 1,200 units at $5.
  assert.equal((await prepare(env, ADA, { units: 1201 })).json.code, 'over_investment_limit');
  const first = await signed(env, ADA, { units: 1000 });
  assert.equal((await prepare(env, ADA, { units: 201 })).json.code, 'over_investment_limit', 'what is already committed counts');
  // Two drafts that each fit: the second cannot be signed once the first is.
  const a = (await prepare(env, ADA, { units: 150 })).json.subscription.id;
  const b = (await prepare(env, ADA, { units: 150 })).json.subscription.id;
  assert.equal((await sign(env, ADA, a)).status, 200);
  assert.equal((await sign(env, ADA, b)).json.code, 'over_investment_limit');
  // A cancelled subscription frees its room.
  assert.equal((await call(env, 'POST', `/api/subscriptions/${first.id}/cancel`, { as: ADA })).status, 200);
  assert.equal((await sign(env, ADA, b)).status, 200);
  // An accredited subscriber has no limit.
  assert.equal((await prepare(env, BEN, { units: 50_000, legal_name: 'Ben Franklin', accredited: true, annual_income_cents: undefined, net_worth_cents: undefined })).status, 200);
});

// ── The signature ────────────────────────────────────────────────────────────

test('a signature needs every statement, the legal name, and the documents now in force', async () => {
  const env = world();
  await openOffering(env);
  const id = (await prepare(env, ADA)).json.subscription.id;
  for (const k of ATTESTATIONS) {
    assert.equal((await sign(env, ADA, id, { attest: { ...ALL_TRUE, [k]: false } })).json.code, 'attestation_required', k);
  }
  assert.equal((await sign(env, ADA, id, { typed_name: 'A. Lovelace' })).json.code, 'name_mismatch');
  assert.equal((await sign(env, ADA, id, { circular_sha256: 'c'.repeat(64) })).json.code, 'documents_changed');
  assert.equal((await sign(env, ADA, id, { agreement_sha256: 'c'.repeat(64) })).json.code, 'documents_changed');
  assert.equal((await sign(env, BEN, id)).status, 404, 'nobody signs another account\'s subscription');
  assert.equal((await offering(env)).reserved_units, 0, 'a refused signature reserves nothing');
  const ok = await sign(env, ADA, id, { typed_name: '  ada   LOVELACE ' });
  assert.equal(ok.status, 200);
  assert.equal(ok.json.subscription.signatures.length, 1);
  assert.equal((await sign(env, ADA, id)).json.replay, true, 'signing twice reserves once');
  assert.equal((await offering(env)).reserved_units, 100);
});

test('when the circular changes, a signed subscription is not accepted until the subscriber signs the new one', async () => {
  const env = world();
  await openOffering(env);
  const sub = await signed(env);
  await admin(env, sub.id, 'funds', { amount_cents: 50_000, bank_ref: 'W1' });
  const NEW_CIRCULAR = { url: CIRCULAR.url, sha256: 'd'.repeat(64) };
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, circular: NEW_CIRCULAR } })).status, 200);
  assert.equal((await admin(env, sub.id, 'accept')).json.code, 'documents_changed');
  assert.equal((await sign(env, ADA, sub.id)).json.code, 'documents_changed', 'the old hash no longer signs');
  const again = await sign(env, ADA, sub.id, { circular_sha256: NEW_CIRCULAR.sha256 });
  assert.deepEqual([again.status, again.json.subscription.status, again.json.subscription.signatures.length], [200, 'funds_received', 2]);
  assert.equal((await offering(env)).reserved_units, 100, 'signing again reserves nothing more');
  assert.equal((await admin(env, sub.id, 'accept')).status, 200);
});

// ── Size, and leaving ────────────────────────────────────────────────────────

test('no more units are reserved or sold than the offering has', async () => {
  const env = world();
  await openOffering(env, { max_units: 150 });
  const rich = { legal_name: 'Ben Franklin', accredited: true };
  const a = (await prepare(env, ADA)).json.subscription.id;
  const b = (await prepare(env, BEN, rich)).json.subscription.id;
  assert.equal((await sign(env, ADA, a)).status, 200);
  assert.equal((await sign(env, BEN, b, { typed_name: 'Ben Franklin' })).json.code, 'sold_out', 'both were prepared while units were left; one signs');
  assert.equal((await prepare(env, BEN, rich)).json.code, 'sold_out');
  assert.equal((await prepare(env, BEN, { ...rich, units: 50 })).status, 200);
  const o = await offering(env);
  assert.deepEqual([o.reserved_units, o.sold_units, o.counts_sound], [100, 0, true]);
  assert.equal((await call(env, 'GET', '/api/offering', { as: null })).json.offering.units_available, 50);
});

test('a paused or closed offering accepts and signs nothing, and its price is fixed while open', async () => {
  const env = world();
  await openOffering(env);
  const sub = await signed(env);
  await admin(env, sub.id, 'funds', { amount_cents: 50_000, bank_ref: 'W1' });
  const draft = (await prepare(env, BEN, { legal_name: 'Ben Franklin', accredited: true })).json.subscription.id;
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, price_cents: 600 } })).json.code, 'offering_open');
  const status = (s) => call(env, 'POST', '/api/admin/offerings/status', { body: { offering_id: 'units-2027', status: s } });
  assert.equal((await status('paused')).status, 200);
  assert.equal((await admin(env, sub.id, 'accept')).json.code, 'not_open');
  assert.equal((await sign(env, BEN, draft, { typed_name: 'Ben Franklin' })).json.code, 'not_open');
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, price_cents: 600 } })).status, 200);
  assert.equal((await status('open')).status, 200, 'reopening needs no second qualification');
  assert.equal((await sign(env, BEN, draft, { typed_name: 'Ben Franklin' })).json.code, 'price_changed');
  // Units are never issued at a price the circular no longer states: the old subscription is refused, rejected and refunded.
  assert.equal((await admin(env, sub.id, 'accept')).json.code, 'price_changed');
  assert.equal((await sign(env, ADA, sub.id)).json.code, 'price_changed', 'signing again does not carry the old price through');
  assert.equal((await admin(env, sub.id, 'reject', { reason: 'the price changed' })).json.subscription.status, 'refund_due');
  assert.equal((await status('closed')).status, 200);
  assert.equal((await status('open')).json.code, 'bad_transition', 'closed is final');
  assert.equal((await call(env, 'GET', '/api/offering', { as: null })).json.offering, null);
});

test('cancelling, rejecting and expiring free the units, and funds that arrived are owed back', async () => {
  const env = world();
  await openOffering(env);
  const rich = { legal_name: 'Ben Franklin', accredited: true };
  const cancelled = await signed(env);
  assert.equal((await call(env, 'POST', `/api/subscriptions/${cancelled.id}/cancel`, { as: BEN })).status, 404, 'only its own subscriber cancels');
  assert.equal((await call(env, 'POST', `/api/subscriptions/${cancelled.id}/cancel`, { as: ADA })).json.subscription.status, 'cancelled');
  assert.equal((await offering(env)).reserved_units, 0);

  const paid = await signed(env, BEN, rich, { typed_name: 'Ben Franklin' });
  await admin(env, paid.id, 'funds', { amount_cents: 50_000, bank_ref: 'W2' });
  assert.equal((await admin(env, paid.id, 'reject', {})).status, 400, 'a rejection gives its reason');
  const rejected = await admin(env, paid.id, 'reject', { reason: 'name does not match the bank account' });
  assert.equal(rejected.json.subscription.status, 'refund_due');
  assert.equal((await admin(env, paid.id, 'accept')).json.code, 'funds_not_received');
  assert.equal((await admin(env, paid.id, 'refunded', { bank_ref: 'R2' })).json.subscription.status, 'refunded');
  assert.equal((await admin(env, cancelled.id, 'refunded', { bank_ref: 'R1' })).json.code, 'no_refund_due');

  const stale = await signed(env);
  const storage = env.HOLDER_LEDGER.instances.get('units').storage;
  const rec = await storage.get(`s:${stale.id}`);
  await storage.put(`s:${stale.id}`, { ...rec, signed_at: '2026-01-01T00:00:00.000Z' });
  const fresh = await signed(env, BEN, rich, { typed_name: 'Ben Franklin' });
  const swept = await call(env, 'POST', '/api/admin/subscriptions/expire', { body: { older_than_days: 14 } });
  assert.deepEqual(swept.json.expired, [stale.id]);
  const o = await offering(env);
  assert.deepEqual([o.reserved_units, o.sold_units, o.counts_sound], [100, 0, true]);
  assert.equal((await call(env, 'GET', `/api/admin/subscriptions?status=awaiting_funds`)).json.subscriptions[0].id, fresh.id);
  assert.equal((await call(env, 'GET', '/api/admin/holders')).json.entries, 0, 'none of this issued a unit');
});

test('an account that already holds units under another legal name is not accepted until the record is corrected', async () => {
  const env = world();
  await openOffering(env);
  await call(env, 'PUT', '/api/admin/holders/record', { body: { holder_id: ADA, legal_name: 'Augusta King', country: 'US', accredited: false } });
  await call(env, 'POST', '/api/admin/holders/issue', { body: { ref: 'founder-1', to: ADA, units: 5 } });
  const sub = await signed(env);
  await admin(env, sub.id, 'funds', { amount_cents: 50_000, bank_ref: 'W1' });
  assert.equal((await admin(env, sub.id, 'accept')).json.code, 'holder_name_differs');
  await call(env, 'PUT', '/api/admin/holders/record', { body: { holder_id: ADA, legal_name: 'Ada Lovelace', country: 'US', accredited: false } });
  assert.equal((await admin(env, sub.id, 'accept')).status, 200);
  const holders = (await call(env, 'GET', '/api/admin/holders')).json;
  assert.deepEqual([holders.holders[0].units, holders.issued_units], [105, 105]);
  assert.equal((await call(env, 'GET', '/api/admin/holders/verify')).json.sound, true);
});

// ── Review fixes (11-13) ─────────────────────────────────────────────────────

test('one account makes one set of statements per offering, and holds at most three unpaid subscriptions in it', async () => {
  const env = world();
  await openOffering(env, { max_units: 100_000 });
  await signed(env, ADA, { units: 10 });
  // Accredited this time, to escape the limit: refused.
  const story = await prepare(env, ADA, { units: 10, accredited: true, annual_income_cents: undefined, net_worth_cents: undefined });
  assert.deepEqual([story.status, story.json.code], [409, 'statements_differ']);
  assert.match(story.json.error, /accredited/);
  assert.equal((await prepare(env, ADA, { units: 10, net_worth_cents: 9_000_000 })).json.code, 'statements_differ');
  // Two drafts prepared before either is signed: the second signs only if it says the same.
  const ada2 = (await prepare(env, ADA, { units: 10 })).json.subscription.id;
  const ada3 = (await prepare(env, ADA, { units: 10 })).json.subscription.id;
  const ada4 = (await prepare(env, ADA, { units: 10 })).json.subscription.id;
  assert.equal((await sign(env, ADA, ada2)).status, 200);
  assert.equal((await sign(env, ADA, ada3)).status, 200);
  const fourth = await sign(env, ADA, ada4);
  assert.deepEqual([fourth.status, fourth.json.code], [429, 'too_many_unpaid']);
  assert.equal((await offering(env)).reserved_units, 30);
});

test('money that arrives after a subscription ended is recorded and owed back, never revives it', async () => {
  const env = world();
  await openOffering(env);
  const sub = await signed(env);
  await call(env, 'POST', `/api/subscriptions/${sub.id}/cancel`, { as: ADA });
  const late = await admin(env, sub.id, 'funds', { amount_cents: 50_000, bank_ref: 'W9' });
  assert.deepEqual([late.status, late.json.subscription.status, late.json.subscription.funds.after], [200, 'refund_due', 'cancelled']);
  assert.equal((await admin(env, sub.id, 'accept')).json.code, 'funds_not_received');
  assert.equal((await admin(env, sub.id, 'refunded', { bank_ref: 'R9' })).json.subscription.status, 'refunded');
  assert.equal((await offering(env)).reserved_units, 0);
});

test('an acceptance never succeeds without issuing its units, and verify sees any subscription the ledger does not back', async () => {
  const env = world();
  await openOffering(env);
  const sub = await signed(env);
  await admin(env, sub.id, 'funds', { amount_cents: 50_000, bank_ref: 'W1' });
  const storage = env.HOLDER_LEDGER.instances.get('units').storage;
  // The review's case: an entry already under this subscription's ref. Hand entries cannot take sub: refs now, so it is planted.
  await storage.put(`ref:sub:${sub.id}`, 1);
  const taken = await admin(env, sub.id, 'accept');
  // The planted ref names an entry unlike this subscription's issue, so the ledger refuses it as a conflict.
  assert.deepEqual([taken.status, taken.json.code], [409, 'ref_conflict']);
  assert.equal((await storage.get(`s:${sub.id}`)).status, 'funds_received', 'the subscription is not marked accepted');
  assert.equal((await offering(env)).sold_units, 0, 'nothing counted as sold');
  // An entry exactly like this subscription's issue already under its ref: still refused, since nothing would be issued now.
  await storage.put('e:000000000077', { seq: 77, kind: 'issue', from: null, to: ADA, units: 100, price_cents: 500, ref: `sub:${sub.id}` });
  await storage.put(`ref:sub:${sub.id}`, 77);
  const twin = await admin(env, sub.id, 'accept');
  assert.deepEqual([twin.status, twin.json.code], [409, 'ledger_ref_taken']);
  await storage.delete('e:000000000077');
  await storage.delete(`ref:sub:${sub.id}`);
  assert.equal((await admin(env, sub.id, 'accept')).status, 200);
  assert.equal((await call(env, 'GET', '/api/admin/holders/verify')).json.sound, true);
  // Drift the ledger cannot see on its own: an accepted subscription pointing at the wrong entry, and counts off by some units.
  const rec = await storage.get(`s:${sub.id}`);
  await storage.put(`s:${sub.id}`, { ...rec, accepted: { ...rec.accepted, entry_seq: 99 } });
  const o = await storage.get('o:units-2027');
  await storage.put('o:units-2027', { ...o, sold_units: o.sold_units + 5 });
  const v = (await call(env, 'GET', '/api/admin/holders/verify')).json;
  assert.equal(v.sound, false);
  assert.ok(v.problems.some((p) => p.includes(`subscription ${sub.id} is accepted`)), JSON.stringify(v.problems));
  assert.ok(v.problems.some((p) => p.includes('offering units-2027 counts 105 units sold')), JSON.stringify(v.problems));
});

test('Tier 2\'s twelve-month cap counts every offering qualified in the twelve months before', async () => {
  const env = world();
  const big = { price_cents: 1000, max_units: 5_000_000 }; // $50,000,000 each
  assert.equal((await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, ...big } })).status, 200);
  assert.equal((await call(env, 'POST', '/api/admin/offerings/status', { body: QUALIFIED })).status, 200);
  assert.equal((await call(env, 'POST', '/api/admin/offerings/status', { body: { offering_id: 'units-2027', status: 'closed' } })).status, 200);
  // Closed having sold nothing: it counts at what it sold, so a second $50M offering fits.
  await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, ...big, offering_id: 'units-2027b' } });
  assert.equal((await call(env, 'POST', '/api/admin/offerings/status', { body: { ...QUALIFIED, offering_id: 'units-2027b', sec_file_number: '024-12346' } })).status, 200);
  await call(env, 'POST', '/api/admin/offerings/status', { body: { offering_id: 'units-2027b', status: 'closed' } });
  // Give the first one $40M of sales: now a third $50M offering is over the cap.
  const storage = env.HOLDER_LEDGER.instances.get('units').storage;
  const first = await storage.get('o:units-2027');
  await storage.put('o:units-2027', { ...first, sold_units: 4_000_000 });
  await call(env, 'PUT', '/api/admin/offerings', { body: { ...TERMS, ...big, offering_id: 'units-2027c' } });
  const third = await call(env, 'POST', '/api/admin/offerings/status', { body: { ...QUALIFIED, offering_id: 'units-2027c', sec_file_number: '024-12347' } });
  assert.deepEqual([third.status, third.json.code], [409, 'over_tier2_cap']);
});

// ── The name comes from sign-up (McKale, 11-13: "We can use KYC from their sign up") ───────────────────────────────────────

test('a subscription carries the name read off the identity document at sign-up, whatever the body says', async () => {
  const env = world();
  await openOffering(env);
  const made = await prepare(env, ADA, { legal_name: 'Somebody Else Entirely' });
  assert.equal(made.status, 200);
  assert.equal(made.json.subscription.legal_name, 'Ada Lovelace');
  assert.deepEqual(made.json.subscription.identity, { source: 'sign_up_document', document_country: 'US', verified_at: '2026-10-01T12:00:00.000Z' });
  // The signature is the document's name, not the one in the body.
  assert.equal((await sign(env, ADA, made.json.subscription.id, { typed_name: 'Somebody Else Entirely' })).json.code, 'name_mismatch');
  assert.equal((await sign(env, ADA, made.json.subscription.id)).status, 200);
});

test('without a readable, linked sign-up verification there is no subscription', async () => {
  const env = world();
  await openOffering(env);
  // Unreadable on the document: the name typed at sign-up is never used in its place.
  env.KYC_STATUS.store.set(`kyc-${ADA}-front`, JSON.stringify(signUpRecord(ADA, '')));
  assert.equal((await prepare(env, ADA)).json.code, 'identity_name_unreadable');
  // No record linked to the account.
  env.KYC_STATUS.store.delete(`kyc-${ADA}-front`);
  assert.equal((await prepare(env, ADA)).json.code, 'identity_record_missing');
  // A record linked to someone else, or one that never passed the age check.
  env.KYC_STATUS.store.set(`kyc-${ADA}-front`, JSON.stringify(signUpRecord(BEN, 'Ben Franklin')));
  assert.equal((await prepare(env, ADA)).json.code, 'identity_record_missing');
  env.KYC_STATUS.store.set(`kyc-${ADA}-front`, JSON.stringify(signUpRecord(ADA, 'Ada Lovelace', { age_verified: false })));
  assert.equal((await prepare(env, ADA)).json.code, 'identity_record_missing');
  assert.equal((await call(env, 'GET', '/api/admin/subscriptions')).json.total, 0);
});
