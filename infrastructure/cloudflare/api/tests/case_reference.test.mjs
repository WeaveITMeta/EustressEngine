// Case references and case history (src/case_reference.mjs, src/case_history.mjs). Every case is named by a reference of 100
// random bits that encodes nothing; an existing case gets one the first time it is read, once; an account sees only its own
// cases, with author-safe fields only; and a reference's holder sees only where a case is. Against the real Worker with fake
// storage.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import {
  newCaseReference, normalizeCaseReference, assignCaseReference, resolveCaseReference, caseReferenceOf,
  REFERENCE_PATTERN, REFERENCE_ALPHABET, REFERENCE_BITS,
} from '../src/case_reference.mjs';

const worker = await loadWorker();

const AUTHOR = '11111111-1111-4111-8111-111111111111';
const OTHER = '22222222-2222-4222-8222-222222222222';
const ADMIN = '33333333-3333-4333-8333-333333333333';
const FILER = '44444444-4444-4444-8444-444444444444';
const SIM_REJECTED = 'aaaaaaaa-0001-4111-8111-aaaaaaaaaaaa';
const SIM_CHANGES = 'aaaaaaaa-0002-4111-8111-aaaaaaaaaaaa';
const SIM_HELD = 'aaaaaaaa-0003-4111-8111-aaaaaaaaaaaa';
const SIM_OTHER = 'bbbbbbbb-0001-4222-8222-bbbbbbbbbbbb';

const FILER_NAME = 'Morgan Fairweather';
const FILER_EMAIL = 'morgan.fair@example.org';
const ANON_NAME = 'Sasha Brightwater';
const ANON_EMAIL = 'sasha.bright@example.org';
// What no author may see of a case: who reviewed, internal scores, the classifier and the judge, evidence, the hold.
const INTERNAL = ['INTERNAL-JUDGE-RATIONALE', 'REVIEWER-PRIVATE-NOTE', 'reviewer-zed', ADMIN, '0.031', '0.913', 'jev', 'judge', 'signals', 'triage', 'quarantin', 'legal', 'csam', 'capture-', 'universes/'];

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

const internals = {
  signals: { empty: false }, triage: { lane: 'quality', hard_max: 0.031 },
  jev: { answers: { csam_or_minor_sexualization: { p: 0.031 } } }, judge: { rationale: 'INTERNAL-JUDGE-RATIONALE', confidence: 0.913 },
  human: { decided_by: ADMIN, note: 'REVIEWER-PRIVATE-NOTE', at: '2026-10-02T00:00:00.000Z' },
};

async function listing(env, id, author, name, modStatus, rec) {
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify({ id, name, author_id: author, author_name: 'x', is_public: true, moderation: { status: modStatus, rating: 'all_ages' }, r2_key: `universes/${id}/universe.pak`, format: 'pak', updated_at: '2026-01-01T00:00:00.000Z' }));
  await env.SOCIAL.put(`sim-author:${author}:${id}`, id);
  await env.SCENES.put(`universes/${id}/universe.pak`, new Uint8Array([id.charCodeAt(5), 1]));
  if (rec) await env.SOCIAL.put(`modcase:${id}`, JSON.stringify({ sim_id: id, author_id: author, created_at: '2026-10-01T00:00:00.000Z', history: [], legal_hold: null, ...internals, ...rec }));
}

async function seeded() {
  const env = { ...fakeEnv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: 'security@example.com' };
  env.mail = [];
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  for (const [id, email] of [[AUTHOR, 'author@example.com'], [OTHER, 'other@example.com'], [ADMIN, 'admin@example.com'], [FILER, FILER_EMAIL]]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username: id.slice(0, 4), email }));
  }
  await env.USERS.put(`admin:${ADMIN}`, '1');
  await listing(env, SIM_REJECTED, AUTHOR, 'Clicker Farm', 'rejected', {
    status: 'rejected', updated_at: '2026-10-03T00:00:00.000Z',
    decision: { outcome: 'reject', lane: 'quality', reasons: [{ code: 'code_B5', article: 'B5', lane: 'quality', action: 'reject' }], suggested_edit: 'Remove the idle reward loop.', decided_by: 'reviewer-zed' },
  });
  await listing(env, SIM_CHANGES, AUTHOR, 'Lab Bench', 'changes_requested', {
    status: 'changes_requested', updated_at: '2026-10-04T00:00:00.000Z',
    decision: { outcome: 'changes_requested', lane: 'quality', changes: ['test_objective'], suggested_edit: 'Give the visitor a measurement to take.', decided_by: 'reviewer-zed' },
  });
  await listing(env, SIM_HELD, AUTHOR, 'Night Market', 'quarantined', {
    status: 'quarantined', updated_at: '2026-10-05T00:00:00.000Z',
    legal_hold: { category: 'csam_or_minor_sexualization', at: '2026-10-05T00:00:00.000Z', by: 'reviewer-zed' },
    decision: { outcome: 'quarantine', lane: 'legal', reasons: [{ code: 'csam_or_minor_sexualization', lane: 'legal', action: 'quarantine' }] },
  });
  await listing(env, SIM_OTHER, OTHER, 'Canal Locks', 'approved', { status: 'approved', updated_at: '2026-10-02T00:00:00.000Z', decision: { outcome: 'approve', rating: 'all_ages' } });
  return env;
}

async function call(env, method, path, { who, body, headers = {} } = {}) {
  const h = { ...headers };
  if (who) h.Authorization = `Bearer ${mintToken(who)}`;
  if (body !== undefined) h['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: res.status, json, text, headers: res.headers };
}

const fileTakedown = (env, { simId, name, email, who }) => call(env, 'POST', '/api/takedown/requests', {
  who, headers: { 'CF-Connecting-IP': '203.0.113.9' },
  body: { requester_name: name, signature: name, relationship: 'self', contact_email: email, locations: [`https://eustress.dev/simulation/${simId}`], description: 'An intimate photo of me shared without my consent.', good_faith: true },
});

const ENTRY_KEYS = {
  listing_review: ['appeal', 'can_appeal', 'decided_at', 'feedback', 'headline', 'kind', 'listing', 'opened_at', 'outcome', 'reference', 'status', 'updated_at'],
  takedown_notice: ['can_request_review', 'decided_at', 'feedback', 'headline', 'kind', 'listing', 'opened_at', 'outcome', 'reference', 'removed_at', 'status', 'updated_at'],
  takedown_request: ['can_request_review', 'decided_at', 'feedback', 'headline', 'kind', 'listing', 'locations', 'opened_at', 'outcome', 'reference', 'removed_at', 'status', 'updated_at'],
};

// -- the reference itself -------------------------------------------------------------------------------------------------------

test('a reference is EC- and twenty base32 symbols, 100 bits, and thousands drawn are all different', () => {
  assert.equal(REFERENCE_BITS, 100);
  const seen = new Set();
  const symbols = new Set();
  for (let i = 0; i < 5000; i++) {
    const ref = newCaseReference();
    assert.match(ref, REFERENCE_PATTERN);
    seen.add(ref);
    for (const ch of ref.slice(3).replace(/-/g, '')) symbols.add(ch);
  }
  assert.equal(seen.size, 5000, 'no two alike');
  assert.equal(symbols.size, 32, 'every symbol of the alphabet is used');
  for (const banned of ['I', 'L', 'O', 'U']) assert.equal(REFERENCE_ALPHABET.includes(banned), false);
});

test('a reference is drawn from the cryptographic random source and nothing else, so it cannot be guessed', () => {
  const real = globalThis.crypto.getRandomValues;
  try {
    globalThis.crypto.getRandomValues = (a) => a.fill(0);
    assert.equal(newCaseReference(), 'EC-0000-0000-0000-0000-0000');
    globalThis.crypto.getRandomValues = (a) => a.fill(255);
    assert.equal(newCaseReference(), 'EC-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ', 'each symbol is five bits of one random byte');
  } finally {
    globalThis.crypto.getRandomValues = real;
  }
});

test('a reference is read back however a person types it', () => {
  const ref = 'EC-7K2M-9QXD-4TPF-HJ3W-8NBR';
  for (const typed of [ref, ref.toLowerCase(), ref.replace(/-/g, ''), ` ec 7k2m 9qxd 4tpf hj3w 8nbr `, '7K2M9QXD4TPFHJ3W8NBR', 'EC-7K2M-9QXD-4TPF-HJ3W-8NBR'.replace('0', 'O')]) {
    assert.equal(normalizeCaseReference(typed), ref, typed);
  }
  assert.equal(normalizeCaseReference('EC-1111-OOOO-IIII-LLLL-0000'), 'EC-1111-0000-1111-1111-0000', 'O is zero, I and L are one');
  for (const bad of ['', 'EC-7K2M', 'EC-7K2M-9QXD-4TPF-HJ3W-8NBU', 'x'.repeat(80), null, 42]) assert.equal(normalizeCaseReference(bad), null, String(bad));
});

test('a case keeps the reference it is given, first claim wins, and the reference names the case and nothing about it', async () => {
  const env = fakeEnv();
  const [a, b] = await Promise.all([assignCaseReference(env, 'moderation', SIM_HELD), assignCaseReference(env, 'moderation', SIM_HELD)]);
  assert.equal(a, b, 'two first reads at once agree');
  assert.equal(await assignCaseReference(env, 'moderation', SIM_HELD), a, 'and every later one');
  assert.equal(await caseReferenceOf(env, 'moderation', SIM_HELD), a);
  assert.deepEqual(await resolveCaseReference(env, a.toLowerCase()), { reference: a, kind: 'moderation', id: SIM_HELD });
  const other = await assignCaseReference(env, 'moderation', SIM_OTHER);
  assert.notEqual(other, a);
  for (const fragment of SIM_HELD.split('-')) assert.equal(a.replace(/-/g, '').includes(fragment.toUpperCase()), false);
  await assert.rejects(() => assignCaseReference(env, 'nonsense', SIM_HELD));
  assert.equal(await resolveCaseReference(env, 'EC-0000-0000-0000-0000-0000'), null);
});

// -- the account's history ------------------------------------------------------------------------------------------------------

test('existing cases get a reference the first time they are read, once', async () => {
  const env = await seeded();
  for (const id of [SIM_REJECTED, SIM_CHANGES, SIM_HELD]) assert.equal(await env.SOCIAL.get(`caseref-of:moderation:${id}`), null, 'none before');
  const first = await call(env, 'GET', '/api/account/cases', { who: AUTHOR });
  assert.equal(first.status, 200);
  const refs = Object.fromEntries(first.json.cases.map((c) => [c.listing.id, c.reference]));
  for (const id of [SIM_REJECTED, SIM_CHANGES, SIM_HELD]) {
    assert.match(refs[id], REFERENCE_PATTERN);
    assert.equal(await env.SOCIAL.get(`caseref-of:moderation:${id}`), refs[id], 'written back');
    const named = JSON.parse(await env.SOCIAL.get(`caseref:${refs[id]}`));
    assert.equal(named.kind, 'moderation');
    assert.equal(named.id, id, 'the reference names its case');
  }
  const second = await call(env, 'GET', '/api/account/cases', { who: AUTHOR });
  assert.deepEqual(Object.fromEntries(second.json.cases.map((c) => [c.listing.id, c.reference])), refs, 'the same references the second time');
  assert.equal(new Set(Object.values(refs)).size, 3, 'each case its own');
});

test('an account sees only its own cases, newest first, with author-safe fields only', async () => {
  const env = await seeded();
  // A takedown of the author's listing, filed by someone with no account, and one of another author's, filed while signed in.
  const anon = await fileTakedown(env, { simId: SIM_REJECTED, name: ANON_NAME, email: ANON_EMAIL });
  assert.equal(anon.status, 201, anon.text);
  const filed = await fileTakedown(env, { simId: SIM_OTHER, name: FILER_NAME, email: FILER_EMAIL, who: FILER });
  assert.equal(filed.status, 201, filed.text);

  const mine = await call(env, 'GET', '/api/account/cases', { who: AUTHOR });
  assert.equal(mine.status, 200);
  assert.equal(mine.headers.get('Cache-Control'), 'private, no-store');
  const kinds = mine.json.cases.map((c) => `${c.kind}:${c.listing?.id}`).sort();
  assert.deepEqual(kinds, [`listing_review:${SIM_CHANGES}`, `listing_review:${SIM_HELD}`, `listing_review:${SIM_REJECTED}`, `takedown_notice:${SIM_REJECTED}`].sort(), 'nothing of another account');
  const stamps = mine.json.cases.map((c) => c.updated_at);
  assert.deepEqual(stamps, [...stamps].sort().reverse(), 'newest first');
  for (const c of mine.json.cases) assert.deepEqual(Object.keys(c).sort(), ENTRY_KEYS[c.kind], `${c.kind} carries only its fields`);

  const rejected = mine.json.cases.find((c) => c.kind === 'listing_review' && c.listing.id === SIM_REJECTED);
  assert.equal(rejected.status, 'not_listed');
  assert.equal(rejected.outcome, 'not_listed');
  assert.deepEqual(rejected.feedback.articles, ['B5']);
  assert.match(rejected.feedback.reasons[0].title, /Code B5/);
  assert.equal(rejected.feedback.suggested_edit, 'Remove the idle reward loop.');
  assert.equal(rejected.listing.name, 'Clicker Farm');
  const changes = mine.json.cases.find((c) => c.listing?.id === SIM_CHANGES);
  assert.deepEqual(changes.feedback.tests, ['T2']);
  // A hold reads as a review in progress, with no reason at all.
  const held = mine.json.cases.find((c) => c.listing?.id === SIM_HELD);
  assert.equal(held.status, 'in_review');
  assert.equal(held.outcome, null);
  assert.deepEqual(held.feedback.reasons, []);
  const notice = mine.json.cases.find((c) => c.kind === 'takedown_notice');
  assert.equal(notice.reference, anon.json.reference);
  assert.equal(notice.status, 'removed');
  assert.equal(notice.can_request_review, true);

  const text = JSON.stringify(mine.json);
  for (const secret of [...INTERNAL, ANON_NAME, ANON_EMAIL, 'Sasha', 'intimate photo', FILER, FILER_NAME, SIM_OTHER, '203.0.113']) assert.equal(text.includes(secret), false, `"${secret}" is not shown`);

  // The requester who was signed in sees their request, and nothing of the listing's author.
  const theirs = await call(env, 'GET', '/api/account/cases', { who: FILER });
  assert.deepEqual(theirs.json.cases.map((c) => c.kind), ['takedown_request']);
  const [request] = theirs.json.cases;
  assert.equal(request.reference, filed.json.reference);
  assert.equal(request.listing, null);
  assert.deepEqual(request.locations, [{ location: `https://eustress.dev/simulation/${SIM_OTHER}`, result: 'removed' }]);
  for (const secret of [OTHER, 'Canal Locks', 'other@example.com']) assert.equal(JSON.stringify(theirs.json).includes(secret), false, `the requester never sees "${secret}"`);

  // That listing's author sees the takedown, and not who filed it.
  const other = await call(env, 'GET', '/api/account/cases', { who: OTHER });
  assert.deepEqual(other.json.cases.map((c) => c.kind).sort(), ['listing_review', 'takedown_notice']);
  for (const secret of [FILER, FILER_NAME, FILER_EMAIL, 'Morgan']) assert.equal(JSON.stringify(other.json).includes(secret), false, `the author never sees "${secret}"`);

  assert.equal((await call(env, 'GET', '/api/account/cases')).status, 401);
});

test('the history is paged', async () => {
  const env = await seeded();
  const first = await call(env, 'GET', '/api/account/cases?limit=2', { who: AUTHOR });
  assert.equal(first.json.cases.length, 2);
  assert.equal(first.json.total, 3);
  assert.equal(first.json.next_cursor, '2');
  const rest = await call(env, 'GET', `/api/account/cases?limit=2&cursor=${first.json.next_cursor}`, { who: AUTHOR });
  assert.equal(rest.json.cases.length, 1);
  assert.equal(rest.json.next_cursor, null);
  assert.equal(new Set([...first.json.cases, ...rest.json.cases].map((c) => c.reference)).size, 3);
});

// -- the status by reference ----------------------------------------------------------------------------------------------------

test('the reference route shows the status and the dates, and nothing else', async () => {
  const env = await seeded();
  const filed = await fileTakedown(env, { simId: SIM_REJECTED, name: ANON_NAME, email: ANON_EMAIL });
  const td = await call(env, 'GET', `/api/cases/${filed.json.reference.toLowerCase()}`);
  assert.equal(td.status, 200);
  assert.deepEqual(Object.keys(td.json).sort(), ['decided_at', 'kind', 'received_at', 'reference', 'removed_at', 'status']);
  assert.equal(td.json.status, 'removed');
  assert.equal(td.json.reference, filed.json.reference);
  assert.ok(Date.parse(td.json.removed_at) >= Date.parse(td.json.received_at));
  assert.equal(td.headers.get('Cache-Control'), 'private, no-store');

  const heldRef = await assignCaseReference(env, 'moderation', SIM_HELD);
  const held = await call(env, 'GET', `/api/cases/${heldRef}`);
  assert.deepEqual(held.json, { reference: heldRef, kind: 'listing_review', status: 'under_review', received_at: '2026-10-01T00:00:00.000Z', removed_at: null, decided_at: null });
  const rejectedRef = await assignCaseReference(env, 'moderation', SIM_REJECTED);
  assert.equal((await call(env, 'GET', `/api/cases/${rejectedRef}`)).json.status, 'decided');

  for (const r of [td, held]) {
    for (const secret of [SIM_REJECTED, SIM_HELD, AUTHOR, 'Clicker', 'Night Market', ANON_NAME, ANON_EMAIL, ...INTERNAL]) assert.equal(r.text.includes(secret), false, `"${secret}" is not shown`);
  }
  assert.equal((await call(env, 'GET', '/api/cases/not-a-reference')).status, 400);
  assert.equal((await call(env, 'GET', '/api/cases/EC-0000-0000-0000-0000-0000')).status, 404);
});

test('a party of a takedown asks a person to review it; a stranger cannot, and a listing review is appealed instead', async () => {
  const env = await seeded();
  const filed = await fileTakedown(env, { simId: SIM_REJECTED, name: ANON_NAME, email: ANON_EMAIL });
  const ref = filed.json.reference;
  const path = `/api/account/cases/${ref}/review`;
  assert.equal((await call(env, 'POST', path, { body: { statement: 'This is my own landscape render.' } })).status, 401);
  assert.equal((await call(env, 'POST', path, { who: OTHER, body: { statement: 'This is my own landscape render.' } })).status, 404, 'a stranger learns nothing');
  assert.equal((await call(env, 'POST', path, { who: AUTHOR, body: { statement: 'short' } })).status, 400);
  const asked = await call(env, 'POST', path, { who: AUTHOR, body: { statement: 'This is my own landscape render; no person is shown in it.' } });
  assert.equal(asked.status, 200, asked.text);
  assert.equal(asked.json.status, 'under_review');
  assert.equal((await call(env, 'GET', `/api/cases/${ref}`)).json.status, 'under_review');
  assert.equal((await call(env, 'POST', path, { who: AUTHOR, body: { statement: 'Asking again for the same review.' } })).status, 409);
  assert.ok(env.mail.some((m) => m.to === 'security@example.com' && /review was asked for/.test(m.raw)), 'staff are told');
  // The listing stays removed: asking changes nothing on its own.
  assert.ok(env.SCENES.objects.has(`locks/legal/${SIM_REJECTED}`));

  const modRef = await assignCaseReference(env, 'moderation', SIM_CHANGES);
  const appeal = await call(env, 'POST', `/api/account/cases/${modRef}/review`, { who: AUTHOR, body: { statement: 'Please look at this listing again.' } });
  assert.equal(appeal.status, 409);
  assert.equal(appeal.json.code, 'use_appeal');
});
