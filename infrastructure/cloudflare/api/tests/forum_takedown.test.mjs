// TAKE IT DOWN requests that name a forum post (src/takedown.mjs). The post is removed like a listing, through a lock the forum
// routes treat as removed (SOCIAL forum-post-lock:{postId}); a person's `invalid` decision deletes it; the hourly sweep writes it
// again if it disappears. Against the real Worker with fake storage.
//
// Source note: plain ASCII only.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, workerModule, mintToken, fakeEnv, ctx } from './helpers/load_worker.mjs';
import * as takedown from '../src/takedown.mjs';
import { forumPostLockKey, resolveForumCase, fileForumReport } from '../src/moderation.mjs';
import { REFERENCE_PATTERN } from '../src/case_reference.mjs';

const { parseLocation, sweepTakedowns, renderMail, FORUM_SITE_HOSTS } = takedown;
const worker = await loadWorker();
// The forums module registers its post check and its decision applier at load; here, in both the Worker's own module instance
// and the one this file imports (the sweep is called from it).
const workerTakedown = await workerModule('takedown.mjs');
const instances = [workerTakedown, takedown];
const postExists = (fn) => { for (const m of instances) m.registerForumPostExists(fn); };
const onDecision = (fn) => { for (const m of instances) m.registerForumTakedownDecision(fn); };
postExists(async () => true);

const ADMIN = '33333333-3333-4333-8333-333333333333';
const THREAD = '0123456789abcdef01234567';
const OTHER_THREAD = 'fedcba9876543210fedcba98';
const POST = `${THREAD}-7`;
const FIRST = `${THREAD}-1`;
const REQUESTER = { name: 'Rowan Quillfeather', email: 'rowan.quill@example.org' };
const EM_DASH = String.fromCharCode(0x2014);

class StubMessage { constructor(from, to, raw) { this.from = from; this.to = to; this.raw = raw; } }

function readMail(m) {
  const part = (type) => {
    const at = m.raw.indexOf(`Content-Type: ${type}; charset=utf-8`);
    const body = m.raw.slice(m.raw.indexOf('\r\n\r\n', at) + 4).split('\r\n--')[0];
    return Buffer.from(body.replace(/\r\n/g, ''), 'base64').toString('utf8');
  };
  const subjectLine = /^Subject: (.*)$/m.exec(m.raw)?.[1] || '';
  const encoded = /^=\?UTF-8\?B\?(.*)\?=$/.exec(subjectLine);
  return { to: m.to, subject: encoded ? Buffer.from(encoded[1], 'base64').toString('utf8') : subjectLine, text: part('text/plain') };
}

async function envWith() {
  const env = { ...fakeEnv(), __EmailMessage: StubMessage, SECURITY_MAILBOX: 'security@example.com', mail: [] };
  env.EMAIL = { send: async (m) => { env.mail.push(m); } };
  await env.USERS.put(`user:${ADMIN}`, JSON.stringify({ kyc_verified: true, id: ADMIN, username: 'staff', email: 'staff@example.com' }));
  await env.USERS.put(`admin:${ADMIN}`, '1');
  return env;
}

async function call(env, method, path, { who, body, headers = {} } = {}) {
  const h = { ...headers };
  if (who) h.Authorization = `Bearer ${mintToken(who)}`;
  let payload;
  if (body !== undefined) { payload = JSON.stringify(body); h['Content-Type'] = 'application/json'; }
  const res = await worker.fetch(new Request(`https://api.eustress.dev${path}`, { method, headers: h, body: payload }), env, ctx);
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { json = null; }
  return { status: res.status, json, text };
}

const file = (env, locations, ip = '203.0.113.9') => call(env, 'POST', '/api/takedown/requests', {
  body: { requester_name: REQUESTER.name, signature: REQUESTER.name, relationship: 'self', contact_email: REQUESTER.email, locations, description: 'A private photo of me posted in a forum thread without consent.', good_faith: true },
  headers: { 'CF-Connecting-IP': ip },
});
const caseOf = async (env, reference) => JSON.parse(await env.SOCIAL.get(`tdcase:${JSON.parse(await env.SOCIAL.get(`caseref:${reference}`)).id}`));
const lockOf = async (env, postId) => JSON.parse((await env.SOCIAL.get(forumPostLockKey(postId))) || 'null');
const quiet = async (fn) => {
  const saved = {};
  for (const k of ['log', 'info', 'warn', 'error', 'debug']) { saved[k] = console[k]; console[k] = () => {}; }
  try { return await fn(); } finally { Object.assign(console, saved); }
};

// -- where a location points ----------------------------------------------------------------------------------------------------

test('every forum location shape names its post: the thread page with #post-, on eustress.dev and on staging, the API post and thread, a bare id', () => {
  const post = (id) => ({ forum_post_id: id, thread_id: id.slice(0, 24), object: 'forum_post' });
  assert.ok(FORUM_SITE_HOSTS.has('staging.eustress-cbb.pages.dev'));
  const shapes = [
    [`https://eustress.dev/community/t/${THREAD}#post-${POST}`, post(POST)],
    [`https://staging.eustress-cbb.pages.dev/community/t/${THREAD}#post-${POST}`, post(POST)],
    [`https://www.eustress.dev/community/t/${THREAD}/#post-${POST}`, post(POST)],
    [`https://api.eustress.dev/api/forums/posts/${POST}`, post(POST)],
    [`https://api.eustress.dev/api/forums/posts/${FIRST}`, post(FIRST)],
    [POST, post(POST)],
    // A thread alone names its first post.
    [`https://eustress.dev/community/t/${THREAD}`, post(FIRST)],
    [`https://staging.eustress-cbb.pages.dev/community/t/${THREAD}`, post(FIRST)],
    [`https://eustress.dev/community/t/${THREAD}#replies`, post(FIRST)],
    [`https://api.eustress.dev/api/forums/threads/${THREAD}`, post(FIRST)],
  ];
  for (const [location, want] of shapes) assert.deepEqual(parseLocation(location), want, location);

  // A fragment naming a post of another thread, or no post at all, names nothing: not found.
  for (const fragment of [`#post-${OTHER_THREAD}-2`, '#post-', '#post-12', `#post-${THREAD}-0`]) {
    const r = parseLocation(`https://eustress.dev/community/t/${THREAD}${fragment}`);
    assert.deepEqual([r.forum_post_id, r.thread_id, r.object, typeof r.not_found], [null, THREAD, 'forum_post', 'string'], fragment);
    assert.equal(r.error, undefined);
  }
  // A forum path with a malformed id is refused; a forum path on a host that is not Eustress's is refused.
  for (const bad of [`https://eustress.dev/community/t/not-a-thread#post-${POST}`, 'https://api.eustress.dev/api/forums/posts/12', 'https://api.eustress.dev/api/forums/threads/abc', `https://evil.example/community/t/${THREAD}#post-${POST}`, `https://eustress-cbb.pages.dev/community/t/${THREAD}`]) {
    assert.ok(parseLocation(bad).error, bad);
  }
  // The staging host is accepted for forum links only: a listing link there is still not on Eustress.
  assert.ok(parseLocation('https://staging.eustress-cbb.pages.dev/simulation/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa').error);
  // Listing links are unchanged.
  assert.deepEqual(parseLocation('https://eustress.dev/simulation/aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa'), { listing_id: 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa', object: 'listing' });
});

// -- removal ------------------------------------------------------------------------------------------------------------------

test('a request naming a forum post URL locks the post at once: forum-post-lock with the reason, the reference and the time', async () => {
  const env = await envWith();
  const filed = await file(env, [`https://eustress.dev/community/t/${THREAD}#post-${POST}`, `https://staging.eustress-cbb.pages.dev/community/t/${OTHER_THREAD}`]);
  assert.equal(filed.status, 201, filed.text);
  assert.match(filed.json.reference, REFERENCE_PATTERN);
  assert.equal(filed.json.status, 'removed');
  assert.deepEqual(filed.json.locations.map((l) => l.result), ['removed', 'removed']);
  const lock = await lockOf(env, POST);
  assert.deepEqual(Object.keys(lock).sort(), ['locked_at', 'reason', 'reference']);
  assert.deepEqual([lock.reason, lock.reference], ['takedown', filed.json.reference]);
  assert.ok(Date.parse(lock.locked_at) > 0);
  assert.equal((await lockOf(env, `${OTHER_THREAD}-1`)).reason, 'takedown', 'a thread alone locks its first post');
  const rec = await caseOf(env, filed.json.reference);
  assert.deepEqual(rec.forum_targets.map((t) => t.post_id), [POST, `${OTHER_THREAD}-1`]);
  assert.ok(rec.removed_at);
  assert.deepEqual(rec.pending, []);
  assert.deepEqual(rec.hashes, [], 'a text post has no file for the hash blocklist');
  // The receipt and the staff notice.
  const mails = env.mail.map(readMail);
  const receipt = mails.find((m) => m.to === REQUESTER.email);
  assert.match(receipt.text, /We removed the content at 2 of the 2 locations/);
  assert.match(receipt.text, /It cannot be seen on Eustress\./);
  const staff = mails.find((m) => m.to === 'security@example.com');
  assert.match(staff.text, /Forum posts named: 2/);
  // The requester's history shows each location's result; nothing else is touched.
  assert.deepEqual([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('forum-post-lock:')).sort(), [forumPostLockKey(`${OTHER_THREAD}-1`), forumPostLockKey(POST)].sort());
});

test('a #post- fragment outside the URL\'s thread is not found: nothing is locked and a person looks at it', async () => {
  const env = await envWith();
  const location = `https://eustress.dev/community/t/${THREAD}#post-${OTHER_THREAD}-2`;
  const filed = await file(env, [location]);
  assert.equal(filed.status, 201, filed.text);
  assert.equal(filed.json.status, 'under_review');
  assert.deepEqual(filed.json.locations, [{ location, result: 'not_found' }]);
  assert.deepEqual([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('forum-post-lock:')), []);
  const rec = await caseOf(env, filed.json.reference);
  assert.equal(rec.unresolved.length, 1);
});

test('a forum post and a listing in one request are each removed, and a listing link alone still works as before', async () => {
  const env = await envWith();
  const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
  await env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ id: SIM, name: 'Harbor', author_id: '11111111-1111-4111-8111-111111111111', is_public: true, moderation: { status: 'approved', rating: 'all_ages' } }));
  const filed = await file(env, [`https://eustress.dev/simulation/${SIM}`, `https://api.eustress.dev/api/forums/posts/${POST}`]);
  assert.equal(filed.status, 201, filed.text);
  assert.deepEqual(filed.json.locations.map((l) => l.result), ['removed', 'removed']);
  assert.ok(env.SCENES.objects.has(`locks/legal/${SIM}`));
  assert.equal((await lockOf(env, POST)).reason, 'takedown');
});

// -- a person's review, and the sweep -----------------------------------------------------------------------------------------

test('a person marks the request invalid: the lock is deleted, and the requester is told', async () => {
  const env = await envWith();
  const filed = await file(env, [`https://eustress.dev/community/t/${THREAD}#post-${POST}`]);
  const rec = await caseOf(env, filed.json.reference);
  // Closing is refused while a post is removed: confirm it or mark it invalid.
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/close`, { who: ADMIN, body: { note: 'Nothing to act on here at all.' } })).status, 409);
  const restored = await call(env, 'POST', `/api/admin/takedowns/${rec.id}/invalid`, { who: ADMIN, body: { note: 'The post shows a landscape; nothing in it matches the request.' } });
  assert.equal(restored.status, 200, restored.text);
  assert.deepEqual(restored.json.incomplete, []);
  assert.equal(await env.SOCIAL.get(forumPostLockKey(POST)), null, 'the lock is gone');
  assert.ok(restored.json.case.forum_targets[0].restored_at);
  assert.ok(env.mail.map(readMail).some((m) => m.to === REQUESTER.email && /reviewed/.test(m.subject)));
  // The sweep never writes a restored post's lock again.
  await quiet(() => sweepTakedowns(env, {}));
  assert.equal(await env.SOCIAL.get(forumPostLockKey(POST)), null);
});

test('a confirmed request keeps the lock; the sweep writes it again if it disappears, and staff are told', async () => {
  const env = await envWith();
  const filed = await file(env, [`https://eustress.dev/community/t/${THREAD}#post-${POST}`]);
  const rec = await caseOf(env, filed.json.reference);
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'The requester is the person shown.' } })).status, 200);
  const before = await lockOf(env, POST);
  await env.SOCIAL.delete(forumPostLockKey(POST));
  assert.equal(await env.SOCIAL.get(forumPostLockKey(POST)), null, 'the hole the sweep closes');
  const out = await quiet(() => sweepTakedowns(env, {}));
  assert.equal(out.relocked, 1);
  const after = await lockOf(env, POST);
  assert.deepEqual([after.reason, after.reference, after.locked_at], ['takedown', filed.json.reference, before.locked_at]);
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'security@example.com' && /put back/.test(m.subject)));
  assert.ok((await caseOf(env, filed.json.reference)).history.some((h) => h.event === 'lock_restored' && h.detail.post_id === POST));
  // A lock in force is left as it is.
  assert.equal((await quiet(() => sweepTakedowns(env, {}))).relocked, 0);
});

test('a lock write that fails is owed, never reported as removed, and the sweep writes it', async () => {
  const env = await envWith();
  const realPut = env.SOCIAL.put;
  env.SOCIAL.put = async (key, value, opts) => {
    if (key.startsWith('forum-post-lock:')) throw new Error('KV is unavailable');
    return realPut.call(env.SOCIAL, key, value, opts);
  };
  const filed = await quiet(() => file(env, [`https://eustress.dev/community/t/${THREAD}#post-${POST}`]));
  assert.equal(filed.status, 201, filed.text);
  assert.equal(filed.json.status, 'received');
  assert.deepEqual(filed.json.locations.map((l) => l.result), ['pending']);
  const rec = await caseOf(env, filed.json.reference);
  assert.deepEqual(rec.pending.filter((s) => s.startsWith('forum_lock:')), [`forum_lock:${POST}`]);
  assert.equal(rec.removed_at, null);
  assert.ok(env.mail.map(readMail).some((m) => m.to === 'security@example.com' && /failing/.test(m.subject)), 'staff hear of a removal still owed');
  env.SOCIAL.put = realPut;
  await quiet(() => sweepTakedowns(env, {}));
  assert.equal((await lockOf(env, POST)).reason, 'takedown');
  assert.ok((await caseOf(env, filed.json.reference)).removed_at);
});

test('two requests naming one post: marking one invalid keeps the lock under the other', async () => {
  const env = await envWith();
  const a = await file(env, [`https://eustress.dev/community/t/${THREAD}#post-${POST}`], '203.0.113.10');
  const b = await file(env, [POST], '203.0.113.11');
  assert.equal(b.status, 201, b.text);
  const recA = await caseOf(env, a.json.reference);
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${recA.id}/invalid`, { who: ADMIN, body: { note: 'This request did not show the post was of the requester.' } })).status, 200);
  const lock = await lockOf(env, POST);
  assert.deepEqual([lock.reason, lock.reference], ['takedown', b.json.reference], 'the other request holds it');
  const recB = await caseOf(env, b.json.reference);
  assert.equal((await call(env, 'POST', `/api/admin/takedowns/${recB.id}/invalid`, { who: ADMIN, body: { note: 'Neither request showed the post was of the requester.' } })).status, 200);
  assert.equal(await env.SOCIAL.get(forumPostLockKey(POST)), null);
});

test('under the lock a person may remove the post through the forum case, and not restore it', async () => {
  const env = await envWith();
  await env.USERS.put('user:old-0001', JSON.stringify({ id: 'old-0001', created_at: '2026-01-01T00:00:00.000Z' }));
  const filed = await quiet(() => fileForumReport({ env, postId: POST, threadId: THREAD, postAuthorId: 'author-0001', reporterId: 'old-0001', category: 'adult_sexual', postBody: 'A post.' }));
  assert.equal(filed.ok, true);
  await file(env, [POST]);
  assert.equal((await quiet(() => resolveForumCase({ env, postId: POST, decision: 'restore', reviewerId: ADMIN }))).code, 'takedown_lock');
  assert.equal((await quiet(() => resolveForumCase({ env, postId: POST, decision: 'remove', reviewerId: ADMIN }))).action, 'remove');
});

test('the forum staff mails render without an em dash', () => {
  const d = { reference: 'EC-0000-1111-2222-3333-4444', post_id: POST, thread_id: THREAD, at: '2026-10-15T17:00:00.000Z', queue_url: 'https://api.eustress.dev/api/admin/moderation/workspace/queue?kind=forum' };
  const m = renderMail('staff_forum_csam', d);
  for (const part of [m.subject, m.text, m.html]) assert.equal(part.includes(EM_DASH), false);
  assert.match(m.text, new RegExp(POST));
  const n = renderMail('staff_new', { reference: 'EC-0', id: 'x', received_at: d.at, removed_at: d.at, named: 0, copies: 0, forum_posts: 2, not_found: 0, pending: [], admin_url: 'https://api.eustress.dev/x' });
  assert.match(n.text, /Forum posts named: 2/);
  assert.equal(n.text.includes(EM_DASH), false);
});

// -- R7-5: a lock only for a post the forum says exists ---------------------------------------------------------------------------

const lockKeys = (env) => [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('forum-post-lock:'));
const withCheck = async (fn, body) => { postExists(fn); try { return await body(); } finally { postExists(async () => true); } };

test('R7-5: the forum says the post does not exist: not_found, no lock key, and a person looks at it', async () => {
  const env = await envWith();
  const asked = [];
  const filed = await withCheck(async (e, id) => { asked.push([e === env, id]); return false; }, () => file(env, [`https://eustress.dev/community/t/${THREAD}#post-${POST}`]));
  assert.equal(filed.status, 201, filed.text);
  assert.deepEqual(asked, [[true, POST]], 'the check is asked with the env and the post id');
  assert.deepEqual(filed.json.locations.map((l) => l.result), ['not_found']);
  assert.equal(filed.json.status, 'under_review');
  assert.deepEqual(lockKeys(env), [], 'no lock key');
  const rec = await caseOf(env, filed.json.reference);
  assert.deepEqual([rec.forum_targets, rec.pending.filter((s) => s.startsWith('forum_lock:')), rec.removed_at], [[], [], null]);
  assert.ok(rec.unresolved.some((u) => /no post/.test(u.reason)));
  assert.deepEqual([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('tdforum:')), [], 'and no index of the post');
  assert.equal(await takedown.forumTakedownStatus(env, POST), null);
});

test('R7-5: the forum cannot say (null, a throw, or no check registered): the lock is owed, nothing is written, and the sweep retries', async () => {
  for (const unknown of [async () => null, async () => { throw new Error('forum down'); }, null, async () => 'yes']) {
    const env = await envWith();
    const filed = await withCheck(unknown, () => quiet(() => file(env, [POST])));
    assert.equal(filed.status, 201, filed.text);
    assert.deepEqual(filed.json.locations.map((l) => l.result), ['pending'], String(unknown));
    assert.deepEqual(lockKeys(env), []);
    const rec = await caseOf(env, filed.json.reference);
    assert.deepEqual(rec.pending.filter((s) => s.startsWith('forum_lock:')), [`forum_lock:${POST}`], 'kept owed');
    assert.equal(rec.removed_at, null);
    // Still unknown at the sweep: still owed.
    await withCheck(unknown, () => quiet(() => sweepTakedowns(env, {})));
    assert.deepEqual(lockKeys(env), []);
    // The forum answers at the next sweep: the post is locked.
    await quiet(() => sweepTakedowns(env, {}));
    assert.equal((await lockOf(env, POST)).reference, filed.json.reference);
    assert.ok((await caseOf(env, filed.json.reference)).removed_at);
  }
});

test('R7-5: 5,000 planted ids write no key when the forum says they do not exist', async () => {
  const env = await envWith();
  let asked = 0;
  await withCheck(async () => { asked += 1; return false; }, async () => {
    for (let r = 0; r < 250; r++) {
      const locations = Array.from({ length: 20 }, (_, i) => `${(r * 20 + i).toString(16).padStart(24, '0')}-1`);
      const filed = await call(env, 'POST', '/api/takedown/requests', {
        body: { requester_name: REQUESTER.name, signature: REQUESTER.name, relationship: 'self', contact_email: `planter${r}@example.org`, locations, description: 'A private photo of me posted in a forum thread without consent.', good_faith: true },
        headers: { 'CF-Connecting-IP': `198.51.${Math.floor(r / 200)}.${r % 200}` },
      });
      assert.equal(filed.status, 201, filed.text);
    }
  });
  assert.equal(asked, 5000, 'every planted id was asked about');
  assert.equal(lockKeys(env).length, 0, 'zero lock keys');
  assert.equal([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('tdforum:')).length, 0);
});

// -- "lock now, block later": the takedown's status, and the decision pushed to the forum -----------------------------------------

test('a confirmed or invalid decision is pushed to the forum per post, and forumTakedownStatus says where the request stands', async () => {
  const env = await envWith();
  const filed = await file(env, [POST]);
  const rec = await caseOf(env, filed.json.reference);
  assert.deepEqual(await takedown.forumTakedownStatus(env, POST), { status: 'removed', reference: filed.json.reference, confirmed_at: null }, 'locked at once, not yet confirmed');
  const calls = [];
  onDecision(async (a) => { calls.push(a); });
  try {
    assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'The requester is the person shown.' } })).status, 200);
    assert.equal(calls.length, 1);
    assert.deepEqual([calls[0].postId, calls[0].reference, calls[0].decision, typeof calls[0].env], [POST, filed.json.reference, 'confirmed', 'object']);
    const status = await takedown.forumTakedownStatus(env, POST);
    assert.deepEqual([status.status, status.reference], ['confirmed', filed.json.reference]);
    assert.ok(Date.parse(status.confirmed_at));
    assert.equal((await caseOf(env, filed.json.reference)).forum_targets[0].decision_push.status, 'decision_pushed');
    assert.equal(await env.SOCIAL.get(`ncii-hash:${'0'.repeat(64)}`), null);
    // Invalid after confirm: pushed as invalid; the request no longer blocks anything.
    assert.equal((await call(env, 'POST', `/api/admin/takedowns/${rec.id}/invalid`, { who: ADMIN, body: { note: 'New information shows the requester is not the person shown.' } })).status, 200);
    assert.deepEqual(calls.map((c) => c.decision), ['confirmed', 'invalid']);
    assert.deepEqual(await takedown.forumTakedownStatus(env, POST), { status: 'invalid', reference: filed.json.reference, confirmed_at: null });
  } finally {
    onDecision(null);
  }
  // No text hash is blocked for forum text on this side.
  assert.deepEqual([...env.SOCIAL.store.keys()].filter((k) => k.startsWith('ncii-hash:')), []);
});

test('Required 1: forumTakedownConfirmed reads one key, written on every confirm (true) and invalid (false)', async () => {
  const env = await envWith();
  assert.equal(await takedown.forumTakedownConfirmed(env, POST), false, 'no key: false');
  const filed = await file(env, [POST]);
  const rec = await caseOf(env, filed.json.reference);
  assert.equal(await takedown.forumTakedownConfirmed(env, POST), false, 'locked, not confirmed');
  await call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'The requester is the person shown.' } });
  assert.equal(await takedown.forumTakedownConfirmed(env, POST), true);
  const stored = JSON.parse(await env.SOCIAL.get(takedown.forumConfirmedKey(POST)));
  assert.deepEqual([stored.confirmed, stored.reference, !!Date.parse(stored.at)], [true, filed.json.reference, true]);
  assert.equal(takedown.forumConfirmedKey(POST), `tdforum-confirmed:${POST}`);
  // A second request on the same post, marked invalid, does not undo the first's confirmation.
  const second = await file(env, [POST], '203.0.113.44');
  const rec2 = await caseOf(env, second.json.reference);
  await call(env, 'POST', `/api/admin/takedowns/${rec2.id}/invalid`, { who: ADMIN, body: { note: 'A second request with nothing new in it at all.' } });
  assert.equal(await takedown.forumTakedownConfirmed(env, POST), true, 'the other confirmed request still holds it');
  // The confirmed one marked invalid: false.
  await call(env, 'POST', `/api/admin/takedowns/${rec.id}/invalid`, { who: ADMIN, body: { note: 'New information shows the requester is not the person shown.' } });
  assert.equal(await takedown.forumTakedownConfirmed(env, POST), false);
  // A KV that cannot be read: null.
  const broken = { ...env, SOCIAL: { ...env.SOCIAL, get: async () => { throw new Error('KV down'); } } };
  assert.equal(await quiet(() => takedown.forumTakedownConfirmed(broken, POST)), null);
});

test('Required 8: the forum check is asked as of the filing, the same received_at on every retry', async () => {
  const env = await envWith();
  const asked = [];
  let answer = null;
  postExists(async (e, id, filedBefore) => { asked.push([id, filedBefore]); return answer; });
  try {
    const filed = await quiet(() => file(env, [POST]));
    const rec = await caseOf(env, filed.json.reference);
    await quiet(() => sweepTakedowns(env, {}));
    answer = true;
    await quiet(() => sweepTakedowns(env, {}));
    assert.equal(asked.length, 3, 'at the filing and at each sweep until it locked');
    assert.ok(asked.every(([id, at]) => id === POST && at === rec.received_at), JSON.stringify(asked));
    assert.equal((await lockOf(env, POST)).reference, filed.json.reference);
  } finally {
    postExists(async () => true);
  }
});

test('an applier that throws never undoes the decision: push_failed is recorded', async () => {
  const env = await envWith();
  const filed = await file(env, [POST]);
  const rec = await caseOf(env, filed.json.reference);
  onDecision(async () => { throw new Error('forum down'); });
  try {
    const r = await quiet(() => call(env, 'POST', `/api/admin/takedowns/${rec.id}/confirm`, { who: ADMIN, body: { note: 'The requester is the person shown.' } }));
    assert.equal(r.status, 200);
  } finally {
    onDecision(null);
  }
  const after = await caseOf(env, filed.json.reference);
  assert.equal(after.status, 'confirmed', 'the decision stands');
  assert.equal(after.forum_targets[0].decision_push.status, 'push_failed');
  assert.ok(after.history.some((h) => h.event === 'push_failed'));
  assert.equal((await lockOf(env, POST)).reason, 'takedown', 'the lock stays');
  // Nothing registered: skipped, recorded.
  const env2 = await envWith();
  const filed2 = await file(env2, [POST]);
  const rec2 = await caseOf(env2, filed2.json.reference);
  await call(env2, 'POST', `/api/admin/takedowns/${rec2.id}/confirm`, { who: ADMIN, body: { note: 'The requester is the person shown.' } });
  assert.equal((await caseOf(env2, filed2.json.reference)).forum_targets[0].decision_push.status, 'decision_push_skipped');
});
