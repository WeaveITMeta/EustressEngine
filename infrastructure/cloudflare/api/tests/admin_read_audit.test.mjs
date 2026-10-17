// Every administrator's read of a case, a queue, a report or a capture is on the audit log before it is answered
// (src/evidence_vault.mjs readEvidence is the model). Where the answer carries addresses, account ids, a requester's details or
// evidence, a read whose audit cannot be written is refused with `audit_unavailable` (503, the vault's shape) and nothing of the
// record leaves the Worker. A pure summary (the legal report list, the takedown queue) is served with its audit's loss logged.
// Each record names who (the administrator), what (the route and the id read) and when, and never copies what was read.
// Against the real Worker with fake storage.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, mintToken, fakeEnv, fakeKv, ctx, testPng } from './helpers/load_worker.mjs';
import { captureR2Key, WORKSPACE_KEY } from '../src/moderation.mjs';

const worker = await loadWorker();

const ADMIN = '33333333-3333-4333-8333-333333333333';
const AUTHOR = '11111111-1111-4111-8111-111111111111';
const MEMBER = '44444444-4444-4444-8444-444444444444';
const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const REPORT = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const TAKEDOWN = 'cccccccc-3333-4333-8333-cccccccccccc';
const TD_REFERENCE = 'EC-0000-1111-2222-3333-4444';
const CAPTURE = testPng(64, 36, 7);

// What the records hold that must never be in a refusal or in an audit record.
const REPORTED_EMAIL = 'reported.person@example.org';
const REPORTED_IP = '203.0.113.77';
const REQUESTER_NAME = 'Rowan Quillfeather';
const REQUESTER_EMAIL = 'rowan.quill@example.org';
const STATEMENT = 'A private photo of me used without my consent.';
const SECRETS = [AUTHOR, MEMBER, REPORTED_EMAIL, REPORTED_IP, REQUESTER_NAME, REQUESTER_EMAIL, STATEMENT, 'Verified Name Here'];

async function seeded() {
  const env = fakeEnv();
  for (const [id, username] of [[ADMIN, 'staff'], [AUTHOR, 'maker-one'], [MEMBER, 'member']]) {
    await env.USERS.put(`user:${id}`, JSON.stringify({ kyc_verified: true, id, username, email: `${username}@example.com` }));
  }
  await env.USERS.put(`admin:${ADMIN}`, '1');

  // A held listing case with its review capture, queued for a person in the Review Workspace.
  await env.SOCIAL.put(`modcase:${SIM}`, JSON.stringify({ sim_id: SIM, author_id: AUTHOR, status: 'held', created_at: '2026-10-01T00:00:00.000Z', updated_at: '2026-10-01T00:00:00.000Z', decision: { lane: 'harm', rating: 'teen_13', reasons: [{ code: 'review' }] }, legal_hold: null, evidence_epoch: 0, history: [] }));
  await env.SOCIAL.put(`modq:held:${SIM}`, '1');
  await env.SCENES.put(captureR2Key(SIM, 0, 'png', 0), CAPTURE, { httpMetadata: { contentType: 'image/png' } });
  await env.SOCIAL.put(`wsq:hold:${SIM}`, JSON.stringify({ sim_id: SIM, kind: 'hold', reference: null, reasons: ['review'], lane: 'harm', at: '2026-10-01T00:00:00.000Z' }));
  await env.SOCIAL.put(WORKSPACE_KEY, JSON.stringify({ space_id: 'dddddddd-4444-4444-8444-dddddddddddd', members: [MEMBER] }));

  // A legal report: who was reported, with the address and the account.
  await env.SOCIAL.put(`legalreport:${REPORT}`, JSON.stringify({
    id: REPORT, kind: 'scam', status: 'ready_to_send', created_at: '2026-10-01T00:00:00.000Z', route: { authority: 'ic3' },
    who: { account_id: AUTHOR, verified_name: 'Verified Name Here', email: REPORTED_EMAIL, upload_ips: [REPORTED_IP] },
    when_where: { case_reference: 'EC-9999-8888-7777-6666-5555' }, what: { category: 'scam' }, notice: { withheld: false },
  }));
  await env.SOCIAL.put(`legalq:ready_to_send:${REPORT}`, '1');

  // A TAKE IT DOWN request with its requester's details.
  await env.SOCIAL.put(`tdcase:${TAKEDOWN}`, JSON.stringify({
    id: TAKEDOWN, reference: TD_REFERENCE, status: 'removed', possible_csam: false, received_at: '2026-10-01T00:00:00.000Z', removed_at: '2026-10-01T00:00:01.000Z', updated_at: '2026-10-01T00:00:01.000Z',
    request: { requester_name: REQUESTER_NAME, requester_email: REQUESTER_EMAIL, description: STATEMENT, account_id: MEMBER },
    locations: [], targets: [{ sim_id: SIM, author_id: AUTHOR, via: 'request', locked_at: '2026-10-01T00:00:01.000Z' }], forum_targets: [], unresolved: [], hashes: [], pending: [], step_errors: {}, reviews: [], history: [],
  }));
  await env.SOCIAL.put(`tdq:removed:${TAKEDOWN}`, '1');
  return env;
}

/// The same storage with an audit log whose writes fail (its reads still answer).
function auditDown(env) {
  const log = fakeKv();
  log.put = async () => { throw new Error('the audit log is down'); };
  return { ...env, AUDIT_LOG: log };
}

async function call(env, method, path, body) {
  const headers = { Authorization: `Bearer ${mintToken(ADMIN)}` };
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) }), env, ctx);
  const bytes = new Uint8Array(await res.arrayBuffer());
  const text = new TextDecoder().decode(bytes);
  let json = null;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: res.status, headers: res.headers, text, json, bytes };
}

const entries = (env) => [...env.AUDIT_LOG.store.values()].map((v) => JSON.parse(v));

// Quiets the refusal's own console line, and gives back what the call answered.
async function quiet(fn) {
  const was = console.error;
  console.error = () => {};
  try { return await fn(); } finally { console.error = was; }
}

// [what the route is, method, path, body, the audit action, the target it names, a check that the read was answered]
const FAIL_CLOSED = [
  ['the legal report', 'GET', `/api/admin/legal/reports/${REPORT}`, undefined, 'LEGAL_REPORT_READ', REPORT, (r) => r.json.report.who.email === REPORTED_EMAIL],
  ['the moderation case', 'GET', `/api/admin/moderation/case/${SIM}`, undefined, 'moderation_get_case', SIM, (r) => r.json.case.author_id === AUTHOR],
  ['the moderation case through the tool route', 'POST', '/api/admin/moderation/tool', { name: 'moderation_get_case', args: { sim_id: SIM } }, 'moderation_get_case', SIM, (r) => r.json.case.author_id === AUTHOR],
  ['a review capture', 'GET', `/api/admin/moderation/captures/${SIM}/0`, undefined, 'moderation_capture_read', SIM, (r) => r.headers.get('Content-Type') === 'image/png' && r.bytes.length === CAPTURE.length],
  ['the listing queue', 'GET', '/api/admin/moderation/queue?status=held', undefined, 'moderation_list_queue', 'modq:held', (r) => r.json.items[0].author_id === AUTHOR],
  ['the listing queue through the tool route', 'POST', '/api/admin/moderation/tool', { name: 'moderation_list_queue', args: { status: 'held' } }, 'moderation_list_queue', 'modq:held', (r) => r.json.items[0].author_id === AUTHOR],
  ['the Review Workspace queue', 'GET', '/api/admin/moderation/workspace/queue', undefined, 'moderation_workspace_queue', 'wsq', (r) => r.json.items[0].sim_id === SIM && r.json.workspace.members[0] === MEMBER],
  ['the Review Workspace queue through the tool route', 'POST', '/api/admin/moderation/tool', { name: 'moderation_workspace_queue', args: {} }, 'moderation_workspace_queue', 'wsq', (r) => r.json.items[0].sim_id === SIM],
  ['the takedown case', 'GET', `/api/admin/takedowns/${TAKEDOWN}`, undefined, 'TAKEDOWN_READ', TD_REFERENCE, (r) => r.json.case.request.requester_name === REQUESTER_NAME],
];

for (const [what, method, path, body, action, target, answered] of FAIL_CLOSED) {
  test(`${what}: a read whose audit cannot be written is refused, and nothing of the record is in the answer`, async () => {
    const env = auditDown(await seeded());
    const r = await quiet(() => call(env, method, path, body));
    assert.equal(r.status, 503, r.text);
    assert.deepEqual(r.json, { error: 'audit_unavailable', code: 'audit_unavailable' });
    assert.equal(r.headers.get('Cache-Control'), 'private, no-store');
    for (const secret of SECRETS) assert.equal(r.text.includes(secret), false, `the refusal carries ${secret}`);
    assert.equal(r.bytes.length < 100, true, 'the refusal is the refusal only');
  });

  test(`${what}: a read writes exactly one audit record naming the administrator and the id, before it is answered`, async () => {
    const env = await seeded();
    const before = entries(env).length;
    const r = await call(env, method, path, body);
    assert.equal(r.status, 200, r.text);
    assert.ok(answered(r), 'the read was answered');
    const written = entries(env);
    assert.equal(written.length - before, 1, 'one record for one read');
    const [entry] = written;
    assert.equal(entry.action, action);
    assert.equal(entry.admin_id, ADMIN);
    assert.equal(entry.target, target);
    assert.ok(Number.isFinite(Date.parse(entry.timestamp)), 'when');
    assert.equal(typeof entry.details.route, 'string');
    const recorded = JSON.stringify(entry);
    for (const secret of SECRETS) assert.equal(recorded.includes(secret), false, `the audit record copies ${secret}`);
  });
}

// The takedown queue's own id is the case id it lists; the record names the queue asked for.
const FAIL_OPEN = [
  ['the legal report list', `/api/admin/legal/reports`, 'LEGAL_REPORTS_LISTED', 'ready_to_send', (r) => r.json.items[0].id === REPORT],
  ['the takedown queue', `/api/admin/takedowns`, 'TAKEDOWNS_LISTED', 'open', (r) => r.json.items[0].id === TAKEDOWN],
];

for (const [what, path, action, target, answered] of FAIL_OPEN) {
  test(`${what}: a summary with no address, account id or requester detail is served when its audit fails, and the loss is logged`, async () => {
    const env = auditDown(await seeded());
    const logged = [];
    const was = console.error;
    console.error = (...args) => { logged.push(args.join(' ')); };
    let r;
    try { r = await call(env, 'GET', path); } finally { console.error = was; }
    assert.equal(r.status, 200, r.text);
    assert.ok(answered(r));
    assert.ok(logged.some((line) => /audit/i.test(line)), 'the lost audit record is logged');
    for (const secret of SECRETS) assert.equal(r.text.includes(secret), false, `the summary carries ${secret}`);
  });

  test(`${what}: a read writes exactly one audit record naming the administrator and the queue`, async () => {
    const env = await seeded();
    const r = await call(env, 'GET', path);
    assert.equal(r.status, 200, r.text);
    assert.ok(answered(r));
    const written = entries(env);
    assert.equal(written.length, 1);
    assert.equal(written[0].action, action);
    assert.equal(written[0].admin_id, ADMIN);
    assert.equal(written[0].target, target);
    assert.equal(typeof written[0].details.route, 'string');
    for (const secret of SECRETS) assert.equal(JSON.stringify(written[0]).includes(secret), false);
  });
}

test('a capture whose read cannot be audited is never read from storage', async () => {
  const env = auditDown(await seeded());
  const reads = [];
  const get = env.SCENES.get.bind(env.SCENES);
  env.SCENES.get = async (key) => { reads.push(key); return get(key); };
  const r = await quiet(() => call(env, 'GET', `/api/admin/moderation/captures/${SIM}/0`));
  assert.equal(r.status, 503);
  assert.equal(reads.some((k) => k.includes('/moderation/capture-')), false, 'the capture was not opened');
});

test('the pipeline and the agent read cases without a person\'s audit record', async () => {
  const { makeToolExecutor } = await import('../src/moderation.mjs');
  const env = await seeded();
  const audits = [];
  const deps = { auditLog: async (...a) => { audits.push(a); } };
  for (const actor of ['agent', 'system']) {
    const r = await makeToolExecutor(env, deps, { actor, actorId: actor })('moderation_get_case', { sim_id: SIM });
    assert.equal(r.ok, true);
  }
  assert.equal(audits.length, 0);
  // A person's read through the executor is audited, and refused when no audit log is given.
  assert.equal((await makeToolExecutor(env, deps, { actor: 'admin', actorId: ADMIN })('moderation_get_case', { sim_id: SIM })).ok, true);
  assert.equal(audits.length, 1);
  const refused = await quiet(() => makeToolExecutor(env, {}, { actor: 'admin', actorId: ADMIN })('moderation_get_case', { sim_id: SIM }));
  assert.equal(refused.code, 'audit_unavailable');
  assert.equal(refused.case, undefined);
});
