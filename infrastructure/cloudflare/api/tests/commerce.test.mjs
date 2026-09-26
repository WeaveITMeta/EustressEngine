import test from 'node:test';
import assert from 'node:assert/strict';
import {
  Wallet, CommerceHub, handleCommerceRoute, walletBalance, walletCredit, walletDebit,
  randomToken, newId, signatureHeader, verifySignatureHeader, keyModeOf, generateApiKey,
  cleanPrice, productRef, cleanSpaceName, cleanWebhookUrl, cleanMetadata, publishedSpaces,
  commerceBlockedReason, hubStub, walletStub, REFUND_WINDOW_MS,
} from '../src/commerce.mjs';
import { resolveAttribution } from '../src/purchases.mjs';
import { isListable } from '../src/moderation.mjs';

// ── Fakes ──────────────────────────────────────────────────────────────────

// KV as the Worker sees it (see purchases.test.mjs).
function kv() {
  const store = new Map();
  return {
    store,
    get: async (key) => (store.has(key) ? store.get(key).value : null),
    put: async (key, value, opts = {}) => {
      if (opts.metadata !== undefined && Buffer.byteLength(JSON.stringify(opts.metadata)) > 1024)
        throw new Error('KV metadata exceeds 1024 bytes');
      store.set(key, { value, metadata: opts.metadata });
    },
    delete: async (key) => { store.delete(key); },
    list: async ({ prefix = '', limit = 1000, cursor } = {}) => {
      const names = [...store.keys()].filter((k) => k.startsWith(prefix)).sort();
      const start = cursor ? Number(cursor) : 0;
      const slice = names.slice(start, start + limit);
      const end = start + slice.length;
      const complete = end >= names.length;
      return {
        keys: slice.map((name) => ({ name, metadata: store.get(name).metadata })),
        list_complete: complete,
        cursor: complete ? undefined : String(end),
      };
    },
  };
}

// Durable Object storage: values are structured clones, so mutating what
// `get` returned changes nothing until it is `put` back, as in production.
function doStorage() {
  const map = new Map();
  let alarm = null;
  const clone = (v) => (v === undefined ? undefined : structuredClone(v));
  return {
    map,
    async get(key) {
      if (Array.isArray(key)) {
        const out = new Map();
        for (const k of key) if (map.has(k)) out.set(k, clone(map.get(k)));
        return out;
      }
      return clone(map.get(key));
    },
    async put(key, value) {
      if (typeof key === 'object') {
        const entries = Object.entries(key);
        if (entries.length > 128) throw new Error('put: more than 128 keys');
        for (const [k, v] of entries) map.set(k, clone(v));
      } else {
        map.set(key, clone(value));
      }
    },
    async delete(key) {
      if (Array.isArray(key)) {
        if (key.length > 128) throw new Error('delete: more than 128 keys');
        let n = 0;
        for (const k of key) if (map.delete(k)) n += 1;
        return n;
      }
      return map.delete(key);
    },
    async list({ prefix = '', start, startAfter, end, reverse = false, limit } = {}) {
      let keys = [...map.keys()].filter((k) => k.startsWith(prefix)).sort();
      if (start !== undefined) keys = keys.filter((k) => k >= start);
      if (startAfter !== undefined) keys = keys.filter((k) => k > startAfter);
      if (end !== undefined) keys = keys.filter((k) => k < end);
      if (reverse) keys.reverse();
      if (limit !== undefined) keys = keys.slice(0, limit);
      return new Map(keys.map((k) => [k, clone(map.get(k))]));
    },
    async getAlarm() { return alarm; },
    async setAlarm(at) { alarm = typeof at === 'number' ? at : at.getTime(); },
    async deleteAlarm() { alarm = null; },
  };
}

// A namespace whose stubs route fetch() into one instance per name.
// `blockConcurrencyWhile` holds back every later call until it is done, as
// the runtime does. `ns.intercept(id, op)` may answer a call instead of the
// object, to simulate an object that is briefly unavailable.
function namespace(Klass, envRef) {
  const instances = new Map();
  const ns = {
    instances,
    intercept: null,
    idFromName: (name) => name,
    get(id) {
      if (!instances.has(id)) {
        let blocked = Promise.resolve();
        const ctx = {
          storage: doStorage(),
          blockConcurrencyWhile(fn) {
            const run = blocked.then(() => fn());
            blocked = run.then(() => {}, () => {});
            return run;
          },
          waitUntil: () => {},
        };
        instances.set(id, new Klass(ctx, envRef.env));
      }
      const instance = instances.get(id);
      return {
        fetch: async (url, init) => {
          const answer = ns.intercept && (await ns.intercept(id, new URL(url).pathname.slice(1)));
          return answer || instance.fetch(new Request(url, init));
        },
      };
    },
  };
  return ns;
}

const SIM = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM_HELD = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
const SIM_OTHER = 'cccccccc-3333-4333-8333-cccccccccccc';

function world() {
  const ref = {};
  const env = {
    USERS: kv(), SOCIAL: kv(), INVENTORY: kv(),
    WALLETS: namespace(Wallet, ref), COMMERCE_HUBS: namespace(CommerceHub, ref),
  };
  ref.env = env;
  const user = (id, extra = {}) => env.USERS.store.set(`user:${id}`, { value: JSON.stringify({ id, username: id.replace(/-/g, ''), ...extra }) });
  user('creator-1');
  user('creator-2');
  user('buyer-1', { ticket_balance: 500 });
  user('buyer-2', { ticket_balance: 3 });
  const sim = (id, author, extra = {}) => {
    env.SOCIAL.store.set(`sim:${id}`, { value: JSON.stringify({
      id, name: `Sim ${id.slice(0, 4)}`, author_id: author, author_name: author, is_public: true,
      r2_key: `universes/${id}/universe.pak`, space_names: ['Lobby', 'Arena'],
      moderation: { status: 'approved' }, published_at: '2026-09-01T00:00:00Z', updated_at: '2026-09-02T00:00:00Z',
      ...extra,
    }) });
    env.SOCIAL.store.set(`sim-author:${author}:${id}`, { value: id });
  };
  sim(SIM, 'creator-1');
  sim(SIM_HELD, 'creator-1', { moderation: { status: 'quarantined' } });
  sim(SIM_OTHER, 'creator-2', { space_names: ['Main'] });
  return env;
}

const deps = {
  // Sessions are bearer user ids in these tests; API keys go through the index.
  verifyAuth: async (request) => {
    const h = request.headers.get('Authorization') || '';
    const t = h.startsWith('Bearer ') ? h.slice(7) : null;
    return t && !t.startsWith('ek_') ? t : null;
  },
  json: (value, status, headers) => new Response(JSON.stringify(value), { status, headers }),
  isListable,
  resolveAttribution,
};

function ctx() {
  const pending = [];
  return { pending, waitUntil: (p) => pending.push(p) };
}

/// Call the router: `who` is a user id (a session) or an API key. Sessions in
/// these tests act live unless a test passes another `mode`; `mode: null`
/// sends no Eustress-Mode header, which the API takes as test. An API key's
/// prefix decides its mode whatever the header says.
async function api(env, method, path, { who, body, mode = 'live' } = {}) {
  const headers = {};
  if (who) headers.Authorization = `Bearer ${who}`;
  if (mode) headers['Eustress-Mode'] = mode;
  if (body !== undefined) headers['content-type'] = 'application/json';
  const request = new Request(`https://api.eustress.dev${path}`, {
    method, headers, body: body === undefined ? undefined : JSON.stringify(body),
  });
  const c = ctx();
  const res = await handleCommerceRoute(request, new URL(request.url), env, c, {}, deps);
  await Promise.all(c.pending);
  if (!res) return { status: 0, body: null };
  return { status: res.status, body: await res.json() };
}

const hubOf = (env, account) => env.COMMERCE_HUBS.instances.get(`hub:${account}`);
const walletOf = (env, account) => env.WALLETS.instances.get(`wallet:${account}`);
const balance = (env, account) => walletBalance(env, account);

async function createProduct(env, fields = {}) {
  const r = await api(env, 'POST', '/api/commerce/products', {
    who: 'creator-1',
    body: { sim_id: SIM, space: 'Lobby', name: '100 Coins', price: 50, ...fields },
  });
  assert.equal(r.status, 201, JSON.stringify(r.body));
  return r.body;
}

async function activate(env, id) {
  const r = await api(env, 'POST', `/api/commerce/products/${id}`, { who: 'creator-1', body: { active: true } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body;
}

/// Buy `product`. `price` is the price shown to the player; pass null to send
/// none.
async function buy(env, product, { who = 'buyer-1', key = `order-${randomToken(10)}`, price = product.price, mode } = {}) {
  return api(env, 'POST', '/api/commerce/purchases', {
    who, mode,
    body: { sim_id: SIM, product: product.number, expected_price: price, idempotency_key: key, space: 'Lobby' },
  });
}

async function events(env, who, query = '') {
  const r = await api(env, 'GET', `/api/commerce/events${query}`, { who });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body.data;
}

/// The value scores filed for `creator`: bliss.mjs keeps one
/// `vscore:{day}:{creator}:{purchase}` per sale, its tickets and buyer in the
/// key's metadata.
function filedScores(env, creator) {
  return [...env.INVENTORY.store.entries()]
    .filter(([key]) => key.startsWith('vscore:') && key.split(':')[2] === creator)
    .map(([key, { metadata }]) => ({ ref: key.split(':')[3], tickets: metadata.t, buyer: metadata.b }));
}

/// A new account whose `tickets` were all bought with money.
async function paidBuyer(env, id, tickets) {
  env.USERS.store.set(`user:${id}`, { value: JSON.stringify({ id, username: id.replace(/-/g, '') }) });
  const credit = await walletCredit(env, id, { ref: `stripe:cs_${id}`, amount: tickets, reason: 'package', paid: true });
  assert.equal(credit.ok, true);
}

/// The outbox steps a wallet still owes, in the order they run.
function owedSteps(wallet) {
  return [...wallet.storage.map.entries()]
    .filter(([k]) => k.startsWith('out:'))
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([key, item]) => ({ key, ...item }));
}

/// Make every owed step due, as if its retry delay had passed.
function makeDue(wallet) {
  for (const { key, ...item } of owedSteps(wallet)) wallet.storage.map.set(key, { ...item, next_at: 0 });
}

async function until(condition) {
  for (let i = 0; i < 1000 && !condition(); i++) await new Promise((r) => setTimeout(r, 1));
  assert.ok(condition(), 'timed out waiting');
}

// ── Helpers and parsing ─────────────────────────────────────────────────────

test('random tokens are base62 of the requested length and ids carry their prefix', () => {
  const t = randomToken(40);
  assert.match(t, /^[A-Za-z0-9]{40}$/);
  assert.match(newId('prod'), /^prod_[A-Za-z0-9]{24}$/);
  assert.notEqual(randomToken(24), randomToken(24));
});

test('signatures verify, and fail when tampered or stale', async () => {
  const header = await signatureHeader('whsec_test', '{"a":1}', 1_000_000);
  assert.match(header, /^t=1000000,v1=[a-f0-9]{64}$/);
  assert.equal(await verifySignatureHeader('whsec_test', '{"a":1}', header, { now: 1_000_010 }), true);
  assert.equal(await verifySignatureHeader('whsec_test', '{"a":2}', header, { now: 1_000_010 }), false);
  assert.equal(await verifySignatureHeader('whsec_other', '{"a":1}', header, { now: 1_000_010 }), false);
  assert.equal(await verifySignatureHeader('whsec_test', '{"a":1}', header, { now: 1_001_000 }), false);
  assert.equal(await verifySignatureHeader('whsec_test', '{"a":1}', 'garbage', { now: 1_000_010 }), false);
});

test('keys carry their mode in the prefix, and nothing else passes for one', () => {
  assert.equal(keyModeOf(generateApiKey('test')), 'test');
  assert.equal(keyModeOf(generateApiKey('live')), 'live');
  assert.equal(keyModeOf('ek_test_short'), null);
  assert.equal(keyModeOf('eyJhbGciOiJIUzI1NiJ9.x.y'), null);
});

test('input cleaning refuses what would corrupt a price, a key or a webhook target', () => {
  assert.equal(cleanPrice(50), 50);
  assert.equal(cleanPrice('75'), 75);
  for (const bad of [0, -5, 1.5, '1.5', 'abc', 1_000_001, null, undefined, NaN]) assert.equal(cleanPrice(bad), null);
  assert.deepEqual(productRef(3), { number: 3 });
  assert.deepEqual(productRef('12'), { number: 12 });
  assert.equal(productRef('prod_short'), null);
  assert.equal(productRef(0), null);
  assert.equal(cleanSpaceName('  Lobby '), 'Lobby');
  for (const bad of ['', '..', 'a/b', 'a\\b', 'x\u0000', 42]) assert.equal(cleanSpaceName(bad), '');
  assert.equal(cleanWebhookUrl('https://hooks.example.com/eustress'), 'https://hooks.example.com/eustress');
  assert.equal(cleanWebhookUrl('https://Hooks.Example.com:443/e'), 'https://hooks.example.com/e');
  for (const bad of ['http://hooks.example.com/x', 'https://localhost/x', 'https://10.0.0.1/x', 'https://[::1]/x',
    'https://printer.local/x', 'https://user:pw@hooks.example.com/x', 'https://intranet/x', 'not a url',
    // IPv4 in the forms the URL parser rewrites to a dotted quad.
    'https://2130706433/x', 'https://0x7f.1/x', 'https://127.1/x',
    // Names that resolve to whatever address they spell, and this API itself.
    'https://127.0.0.1.nip.io/x', 'https://10-0-0-1.sslip.io/x', 'https://api.eustress.dev/api/keys', 'https://eustress.dev./x',
    'https://router.home.arpa/x', 'https://svc.cluster.internal/x', 'https://hooks.example.com:8443/x', 'https://a_b.example.com/x']) {
    assert.equal(cleanWebhookUrl(bad), '', bad);
  }
  assert.deepEqual(cleanMetadata({ tier: 'gold', n: 3 }).metadata, { tier: 'gold', n: '3' });
  assert.deepEqual(cleanMetadata({ tier: null }, { tier: 'gold', x: '1' }).metadata, { x: '1' });
  assert.equal(cleanMetadata({ 'bad key!': 'x' }).ok, false);
  assert.equal(cleanMetadata(Object.fromEntries([...Array(21)].map((_, i) => [`k${i}`, 'v']))).ok, false);
});

test('a simulation sells only after something is published, and never under review', () => {
  assert.deepEqual([...publishedSpaces({ spaces: { A: {} }, space_names: ['B', 'A'] })].sort(), ['A', 'B']);
  assert.equal(commerceBlockedReason({ r2_key: 'x', moderation: { status: 'approved' } }), null);
  assert.match(commerceBlockedReason({ moderation: { status: 'approved' } }), /publish the Universe first/);
  assert.match(commerceBlockedReason({ r2_key: 'x', moderation: { status: 'quarantined' } }), /under review/);
  assert.match(commerceBlockedReason({ r2_key: 'x', moderation: { status: 'rejected' } }), /rejected/);
});

// ── Wallet ──────────────────────────────────────────────────────────────────

test('a wallet opens with the balance USERS held, then owns it and copies it to its own key', async () => {
  const env = world();
  assert.equal(await balance(env, 'buyer-1'), 500);
  const credit = await walletCredit(env, 'buyer-1', { ref: 'stripe:cs_1', amount: 880, reason: 'Standard package' });
  assert.equal(credit.ok, true);
  assert.equal(credit.balance, 1380);
  const copy = JSON.parse(env.USERS.store.get('ticketbal:buyer-1').value);
  assert.equal(copy.balance, 1380);
  assert.equal(copy.version, 1);
  // The account record other routes edit is never rewritten by the wallet,
  // and a later change to it no longer changes the balance.
  const user = JSON.parse(env.USERS.store.get('user:buyer-1').value);
  assert.equal(user.ticket_balance, 500);
  user.ticket_balance = 999_999;
  env.USERS.store.set('user:buyer-1', { value: JSON.stringify(user) });
  assert.equal(await balance(env, 'buyer-1'), 1380);
});

test('a balance copy that fails is written by the alarm', async () => {
  const env = world();
  const put = env.USERS.put;
  let failures = 1;
  env.USERS.put = async (key, ...rest) => {
    if (key.startsWith('ticketbal:') && failures-- > 0) throw new Error('KV PUT failed: 429 Too Many Requests');
    return put(key, ...rest);
  };
  await walletCredit(env, 'buyer-1', { ref: 'stripe:cs_9', amount: 10, reason: 'x' });
  assert.equal(env.USERS.store.get('ticketbal:buyer-1'), undefined);
  const wallet = walletOf(env, 'buyer-1');
  assert.equal((await wallet.storage.get('meta')).mirror_pending, true);
  assert.ok(await wallet.storage.getAlarm(), 'a retry is scheduled');
  await wallet.alarm();
  assert.equal(JSON.parse(env.USERS.store.get('ticketbal:buyer-1').value).balance, 510);
  assert.equal((await wallet.storage.get('meta')).mirror_pending, undefined);
});

test('credits and debits happen once per ref, and a short balance refuses a debit', async () => {
  const env = world();
  await walletCredit(env, 'buyer-2', { ref: 'stripe:cs_2', amount: 10, reason: 'x' });
  const replay = await walletCredit(env, 'buyer-2', { ref: 'stripe:cs_2', amount: 10, reason: 'x' });
  assert.equal(replay.replay, true);
  assert.equal(await balance(env, 'buyer-2'), 13);
  const short = await walletDebit(env, 'buyer-2', { ref: 'spend:1', amount: 50, reason: 'x' });
  assert.equal(short.ok, false);
  assert.equal(short.status, 402);
  assert.equal(short.code, 'insufficient_tickets');
  assert.equal(await balance(env, 'buyer-2'), 13);
  const ok = await walletDebit(env, 'buyer-2', { ref: 'spend:2', amount: 13, reason: 'x' });
  assert.equal(ok.balance, 0);
  const again = await walletDebit(env, 'buyer-2', { ref: 'spend:2', amount: 13, reason: 'x' });
  assert.equal(again.replay, true);
  assert.equal(await balance(env, 'buyer-2'), 0);
  // A reversal the account cannot refuse, such as a chargeback, may go below zero.
  const chargeback = await walletDebit(env, 'buyer-2', { ref: 'dispute:dp_1', amount: 5, reason: 'x', allow_negative: true });
  assert.equal(chargeback.ok, true);
  assert.equal(await balance(env, 'buyer-2'), -5);
});

test('concurrent debits past the balance cannot both succeed', async () => {
  const env = world();
  // 500 Tickets, three 200-Ticket debits at once: exactly two go through.
  const results = await Promise.all([1, 2, 3].map((i) => walletDebit(env, 'buyer-1', { ref: `c:${i}`, amount: 200, reason: 'x' })));
  assert.equal(results.filter((r) => r.ok).length, 2);
  assert.equal(await balance(env, 'buyer-1'), 100);
});

// ── API keys ────────────────────────────────────────────────────────────────

test('keys are created from a session, listed without their secret, used, and revoked', async () => {
  const env = world();
  const created = await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { name: 'CLI', mode: 'test' } });
  assert.equal(created.status, 201);
  assert.match(created.body.key, /^ek_test_[A-Za-z0-9]{40}$/);
  assert.equal(created.body.key_type, 'commerce');

  const account = await api(env, 'GET', '/api/commerce/account', { who: created.body.key });
  assert.equal(account.status, 200);
  assert.equal(account.body.id, 'creator-1');
  assert.equal(account.body.livemode, false);
  assert.equal(account.body.via, 'key');

  const listed = await api(env, 'GET', '/api/keys', { who: 'creator-1' });
  assert.equal(listed.body.keys.length, 1);
  assert.equal(listed.body.keys[0].usage_count, 1);
  assert.ok(!JSON.stringify(listed.body).includes(created.body.key), 'the secret never comes back');

  // A key cannot manage keys, and a non-commerce key cannot use commerce.
  assert.equal((await api(env, 'POST', '/api/keys', { who: created.body.key, body: {} })).status, 403);
  const datastore = await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { key_type: 'datastore' } });
  assert.equal((await api(env, 'GET', '/api/commerce/account', { who: datastore.body.key })).body.code, 'wrong_key_type');

  const revoked = await api(env, 'DELETE', `/api/keys/${created.body.id}`, { who: 'creator-1' });
  assert.equal(revoked.status, 200);
  assert.equal((await api(env, 'GET', '/api/commerce/account', { who: created.body.key })).status, 401);
});

// ── Products ────────────────────────────────────────────────────────────────

test('products need a published Space in the creator\'s own sellable simulation', async () => {
  const env = world();
  const post = (who, body) => api(env, 'POST', '/api/commerce/products', { who, body: { name: 'X', price: 5, ...body } });

  assert.equal((await post('creator-1', { sim_id: SIM })).status, 400, 'space is required');
  const unpublished = await post('creator-1', { sim_id: SIM, space: 'Basement' });
  assert.equal(unpublished.status, 409);
  assert.equal(unpublished.body.code, 'space_not_published');
  assert.deepEqual(unpublished.body.published_spaces, ['Arena', 'Lobby']);
  assert.equal((await post('creator-2', { sim_id: SIM, space: 'Lobby' })).body.code, 'not_simulation_owner');
  assert.equal((await post('creator-1', { sim_id: SIM_HELD, space: 'Lobby' })).body.code, 'simulation_not_sellable');
  assert.equal((await post('creator-1', { sim_id: SIM, space: 'Lobby', price: 0 })).status, 400);
  assert.equal((await post('creator-1', { sim_id: SIM, space: 'Lobby', type: 'subscription' })).status, 400);

  const first = await createProduct(env);
  assert.equal(first.number, 1);
  assert.equal(first.active, false, 'new products are drafts');
  assert.equal(first.type, 'consumable');
  const second = await createProduct(env, { name: 'VIP', type: 'pass', price: 200 });
  assert.equal(second.number, 2);

  const listed = await api(env, 'GET', `/api/commerce/products?sim_id=${SIM}`, { who: 'creator-1' });
  assert.deepEqual(listed.body.data.map((p) => p.number), [2, 1]);

  const updated = await api(env, 'POST', `/api/commerce/products/${first.id}`, { who: 'creator-1', body: { price: 60, type: 'pass' } });
  assert.equal(updated.status, 400, 'type is immutable');
  const repriced = await api(env, 'POST', `/api/commerce/products/${first.id}`, { who: 'creator-1', body: { price: 60 } });
  assert.equal(repriced.body.price, 60);

  const archived = await api(env, 'DELETE', `/api/commerce/products/${second.id}`, { who: 'creator-1' });
  assert.equal(archived.body.active, false);

  const types = (await events(env, 'creator-1')).map((e) => e.type);
  assert.deepEqual(types, ['product.updated', 'product.created', 'product.created']);
  const priceEvent = (await events(env, 'creator-1'))[0];
  assert.deepEqual(priceEvent.data.previous_attributes, { price: 50 });
});

test('the catalog shows players active products, and the creator every product', async () => {
  const env = world();
  const draft = await createProduct(env, { name: 'Draft' });
  const live = await activate(env, (await createProduct(env, { name: 'Live' })).id);
  const publicView = await api(env, 'GET', `/api/commerce/catalog/${SIM}`);
  assert.equal(publicView.status, 200);
  assert.deepEqual(publicView.body.data.map((p) => p.id), [live.id]);
  const authorView = await api(env, 'GET', `/api/commerce/catalog/${SIM}`, { who: 'creator-1' });
  assert.deepEqual(authorView.body.data.map((p) => p.id).sort(), [draft.id, live.id].sort());
  assert.equal((await api(env, 'GET', `/api/commerce/catalog/${SIM_HELD}`)).status, 404);
});

// ── Live purchases ──────────────────────────────────────────────────────────

test('a live purchase charges the shown price once and pays the creator 70%', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);

  assert.equal((await buy(env, product, { price: null })).status, 400, 'expected_price is required live');
  const changed = await buy(env, product, { price: 40 });
  assert.equal(changed.status, 409);
  assert.equal(changed.body.code, 'price_changed');
  assert.equal(changed.body.price, 50);

  const bought = await buy(env, product, { key: 'order-000001' });
  assert.equal(bought.status, 201, JSON.stringify(bought.body));
  const p = bought.body.purchase;
  assert.match(p.id, /^pur_/);
  assert.equal(p.livemode, true);
  assert.equal(p.amount, 50);
  assert.equal(p.creator_amount, 35);
  assert.equal(p.platform_amount, 15);
  assert.equal(bought.body.balance, 450);
  assert.equal(await balance(env, 'buyer-1'), 450);
  assert.equal(await balance(env, 'creator-1'), 35);

  // The same idempotency key replays; nothing moves twice.
  const replay = await buy(env, product, { key: 'order-000001' });
  assert.equal(replay.status, 200);
  assert.equal(replay.body.replayed, true);
  assert.equal(replay.body.purchase.id, p.id);
  assert.equal(await balance(env, 'buyer-1'), 450);
  assert.equal(await balance(env, 'creator-1'), 35);

  // The creator's books: the sale and the event. buyer-1's Tickets predate
  // the wallet, so they count as earned and this sale schedules no value
  // score (a paid-for sale does; see the value score tests).
  const sale = await api(env, 'GET', `/api/commerce/purchases/${p.id}`, { who: 'creator-1' });
  assert.equal(sale.body.buyer_id, 'buyer-1');
  const live = await events(env, 'creator-1');
  assert.equal(live[0].type, 'purchase.succeeded');
  assert.equal(live[0].data.object.id, p.id);
  const hub = hubOf(env, 'creator-1');
  assert.equal((await hub.storage.get(`sale:${p.id}`)).paid_amount, 0);
  assert.equal([...hub.storage.map.keys()].filter((k) => k.startsWith('settle:')).length, 0);

  // The buyer's books: a receipt, the txn lines, and a receipt to process.
  const receipts = [...env.INVENTORY.store.keys()].filter((k) => k.startsWith('purchase:buyer-1:'));
  assert.equal(receipts.length, 1);
  assert.ok([...env.INVENTORY.store.keys()].some((k) => k.startsWith(`txn:creator-1:`) && k.endsWith(p.id)));
  const pending = await api(env, 'GET', `/api/commerce/me/pending?sim_id=${SIM}`, { who: 'buyer-1' });
  assert.deepEqual(pending.body.data.map((r) => r.purchase_id), [p.id]);
});

test('live purchases refuse drafts, creators buying their own, and unlisted simulations', async () => {
  const env = world();
  const draft = await createProduct(env);
  assert.equal((await buy(env, draft)).body.code, 'product_inactive');
  const product = await activate(env, draft.id);
  assert.equal((await buy(env, product, { who: 'creator-1' })).body.code, 'self_purchase');

  // Pull the listing back to pending review: live sales stop.
  const sim = JSON.parse(env.SOCIAL.store.get(`sim:${SIM}`).value);
  sim.moderation = { status: 'pending' };
  env.SOCIAL.store.set(`sim:${SIM}`, { value: JSON.stringify(sim) });
  assert.equal((await buy(env, product)).body.code, 'simulation_not_listed');
});

test('a short balance refuses the purchase and tells the creator', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  const r = await buy(env, product, { who: 'buyer-2' });
  assert.equal(r.status, 402);
  assert.equal(r.body.code, 'insufficient_tickets');
  assert.equal(r.body.balance, 3);
  assert.equal(await balance(env, 'buyer-2'), 3);
  const failed = (await events(env, 'creator-1')).find((e) => e.type === 'purchase.failed');
  assert.equal(failed.data.object.failure_code, 'insufficient_tickets');
});

test('a pass is owned once, and shows in the buyer\'s entitlements', async () => {
  const env = world();
  const pass = await activate(env, (await createProduct(env, { name: 'VIP', type: 'pass', price: 100 })).id);
  const first = await buy(env, pass);
  assert.equal(first.status, 201);
  const second = await buy(env, pass);
  assert.equal(second.status, 409);
  assert.equal(second.body.code, 'already_owned');
  assert.equal(await balance(env, 'buyer-1'), 400);
  const owned = await api(env, 'GET', `/api/commerce/me/entitlements?sim_id=${SIM}`, { who: 'buyer-1' });
  assert.deepEqual(owned.body.data.map((e) => e.number), [pass.number]);
  // A pass is granted by owning it; it never waits in the receipts queue.
  const pending = await api(env, 'GET', `/api/commerce/me/pending?sim_id=${SIM}`, { who: 'buyer-1' });
  assert.equal(pending.body.data.length, 0);
});

test('fulfilling clears the pending receipt once and tells the creator', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  const { purchase } = (await buy(env, product)).body;
  const done = await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/fulfill`, { who: 'buyer-1', body: { sim_id: SIM } });
  assert.equal(done.status, 200, JSON.stringify(done.body));
  assert.equal(done.body.fulfilled, true);
  const again = await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/fulfill`, { who: 'buyer-1', body: { sim_id: SIM } });
  assert.equal(again.status, 200);
  const pending = await api(env, 'GET', `/api/commerce/me/pending?sim_id=${SIM}`, { who: 'buyer-1' });
  assert.equal(pending.body.data.length, 0);
  const fulfilled = (await events(env, 'creator-1')).filter((e) => e.type === 'purchase.fulfilled');
  assert.equal(fulfilled.length, 1);
  // Nobody else can acknowledge it.
  assert.equal((await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/fulfill`, { who: 'buyer-2', body: { sim_id: SIM } })).status, 404);
});

test('a refund returns the Tickets, takes back the creator\'s share and cancels the value score', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env, { type: 'pass', name: 'VIP', price: 100 })).id);
  const { purchase } = (await buy(env, product)).body;
  assert.equal(await balance(env, 'buyer-1'), 400);
  assert.equal(await balance(env, 'creator-1'), 70);

  const refunded = await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/refund`, { who: 'creator-1' });
  assert.equal(refunded.status, 200, JSON.stringify(refunded.body));
  assert.equal(refunded.body.status, 'refunded');
  assert.equal(await balance(env, 'buyer-1'), 500);
  assert.equal(await balance(env, 'creator-1'), 0);
  const owned = await api(env, 'GET', `/api/commerce/me/entitlements?sim_id=${SIM}`, { who: 'buyer-1' });
  assert.equal(owned.body.data.length, 0, 'the pass goes with the refund');
  const hub = hubOf(env, 'creator-1');
  assert.equal([...hub.storage.map.keys()].filter((k) => k.startsWith('settle:')).length, 0);
  assert.equal((await events(env, 'creator-1'))[0].type, 'purchase.refunded');
  const refundReceipt = [...env.INVENTORY.store.values()].map((v) => v.metadata).find((m) => m && m.a === -100);
  assert.ok(refundReceipt, 'a negative receipt nets the buyer\'s totals');

  // Buyers cannot refund themselves.
  const other = await activate(env, (await createProduct(env, { name: 'Gem' })).id);
  const second = (await buy(env, other)).body.purchase;
  assert.equal((await api(env, 'POST', `/api/commerce/purchases/${second.id}/refund`, { who: 'buyer-1' })).status, 404);
});

test('a refund after the window is refused', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  const { purchase } = (await buy(env, product)).body;
  // Age the purchase past the window on both sides.
  const old = Math.floor((Date.now() - REFUND_WINDOW_MS - 60e3) / 1000);
  for (const [store, key] of [[hubOf(env, 'creator-1').storage, `sale:${purchase.id}`], [walletOf(env, 'buyer-1').storage, `pur:${purchase.id}`]]) {
    const rec = await store.get(key);
    rec.created = old;
    await store.put(key, rec);
  }
  const r = await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/refund`, { who: 'creator-1' });
  assert.equal(r.status, 409);
  assert.equal(r.body.code, 'refund_window_closed');
});

test('value score is credited when the refund window closes, and never for a refunded sale', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env, { price: 100 })).id);
  // A buyer whose Tickets were all bought, so every purchase can score.
  await paidBuyer(env, 'buyer-3', 300);
  const kept = (await buy(env, product, { who: 'buyer-3' })).body.purchase;
  const refunded = (await buy(env, product, { who: 'buyer-3' })).body.purchase;
  await api(env, 'POST', `/api/commerce/purchases/${refunded.id}/refund`, { who: 'creator-1' });

  const hub = hubOf(env, 'creator-1');
  // Not yet: the score is filed a minute after the window closes.
  await hub.settleDue(Date.now() + REFUND_WINDOW_MS);
  assert.deepEqual(filedScores(env, 'creator-1'), []);
  const later = Date.now() + REFUND_WINDOW_MS + 120e3;
  await hub.settleDue(later);
  assert.deepEqual(
    filedScores(env, 'creator-1').map((f) => [f.ref, f.tickets, f.buyer]),
    [[kept.id, 70, 'buyer-3']],
    'one kept sale of 70 creator Tickets',
  );
  const sale = await hub.storage.get(`sale:${kept.id}`);
  assert.ok(sale.value_score_credited);
  assert.deepEqual([...hub.storage.map.keys()].filter((k) => k.startsWith('settle:')), []);
  // Settling again files nothing more.
  await hub.settleDue(later);
  assert.equal(filedScores(env, 'creator-1').length, 1);
});

test('a value score that cannot be filed is tried again later', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env, { price: 100 })).id);
  await paidBuyer(env, 'buyer-3', 100);
  const { purchase } = (await buy(env, product, { who: 'buyer-3' })).body;
  const put = env.INVENTORY.put;
  let failures = 1;
  env.INVENTORY.put = async (key, ...rest) => {
    if (key.startsWith('vscore:') && failures-- > 0) throw new Error('KV PUT failed: 429 Too Many Requests');
    return put(key, ...rest);
  };
  const hub = hubOf(env, 'creator-1');
  const later = Date.now() + REFUND_WINDOW_MS + 120e3;
  await hub.settleDue(later);
  assert.equal(filedScores(env, 'creator-1').length, 0);
  assert.equal((await hub.storage.get(`sale:${purchase.id}`)).value_score_credited, undefined, 'not marked');
  assert.equal([...hub.storage.map.keys()].filter((k) => k.startsWith('settle:')).length, 1, 'still owed');
  await hub.settleDue(later);
  assert.deepEqual(filedScores(env, 'creator-1').map((f) => f.ref), [purchase.id]);
  assert.ok((await hub.storage.get(`sale:${purchase.id}`)).value_score_credited);
});

test('Tickets earned from sales score nothing when they are spent again', async () => {
  const env = world();
  const sold = await activate(env, (await createProduct(env, { price: 100 })).id);
  await paidBuyer(env, 'buyer-3', 100);
  await buy(env, sold, { who: 'buyer-3' });
  assert.equal(await balance(env, 'creator-1'), 70);

  // creator-1 spends the 70 it earned on creator-2's product.
  const made = await api(env, 'POST', '/api/commerce/products', {
    who: 'creator-2', body: { sim_id: SIM_OTHER, space: 'Main', name: 'Hat', price: 60 },
  });
  await api(env, 'POST', `/api/commerce/products/${made.body.id}`, { who: 'creator-2', body: { active: true } });
  const hop = await api(env, 'POST', '/api/commerce/purchases', {
    who: 'creator-1',
    body: { sim_id: SIM_OTHER, product: made.body.number, expected_price: 60, idempotency_key: 'hop-000001' },
  });
  assert.equal(hop.status, 201, JSON.stringify(hop.body));
  assert.equal(await balance(env, 'creator-2'), 42, 'the earned Tickets still pay creator-2');

  const later = Date.now() + REFUND_WINDOW_MS + 120e3;
  await hubOf(env, 'creator-1').settleDue(later);
  await hubOf(env, 'creator-2').settleDue(later);
  assert.deepEqual(filedScores(env, 'creator-1').map((f) => f.tickets), [70], 'the paid sale scores');
  assert.deepEqual(filedScores(env, 'creator-2'), [], 'the recycled sale does not');
});

test('spending draws earned Tickets first; a chargeback takes paid Tickets first', async () => {
  const env = world();
  const meta = async () => walletOf(env, 'buyer-2').storage.get('meta');
  // buyer-2 opens with 3 earned Tickets, then buys 60.
  await walletCredit(env, 'buyer-2', { ref: 'stripe:cs_mix', amount: 60, reason: 'x', paid: true });
  assert.deepEqual([(await meta()).balance, (await meta()).paid_balance], [63, 60]);
  const spend = await walletDebit(env, 'buyer-2', { ref: 'd:1', amount: 10, reason: 'x' });
  assert.equal(spend.paid_used, 7, 'the 3 earned Tickets go first');
  assert.deepEqual([(await meta()).balance, (await meta()).paid_balance], [53, 53]);
  await walletCredit(env, 'buyer-2', { ref: 'sale:x', amount: 20, reason: 'x' });
  const chargeback = await walletDebit(env, 'buyer-2', { ref: 'dispute:1', amount: 30, reason: 'x', allow_negative: true, paid_first: true });
  assert.equal(chargeback.paid_used, 30);
  assert.deepEqual([(await meta()).balance, (await meta()).paid_balance], [43, 23]);
  // Past zero, the paid part runs out before the balance does.
  await walletDebit(env, 'buyer-2', { ref: 'dispute:2', amount: 50, reason: 'x', allow_negative: true, paid_first: true });
  assert.deepEqual([(await meta()).balance, (await meta()).paid_balance], [-7, 0]);
});

// ── Finishing a purchase ────────────────────────────────────────────────────

test('a purchase finishes on the alarm when the creator\'s hub is briefly unavailable', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  env.COMMERCE_HUBS.intercept = (_id, op) =>
    op === 'record_sale' ? new Response(JSON.stringify({ ok: false, error: 'unavailable' }), { status: 503 }) : null;

  const bought = await buy(env, product);
  assert.equal(bought.status, 201, 'the buyer is charged and answered at once');
  const { purchase } = bought.body;
  assert.equal(await balance(env, 'buyer-1'), 450);
  assert.equal(await balance(env, 'creator-1'), 0, 'steps after the failing one wait for it');
  const wallet = walletOf(env, 'buyer-1');
  const owed = owedSteps(wallet);
  assert.deepEqual(owed.map((o) => o.step), ['hub', 'seller_credit', 'books']);
  assert.equal(owed[0].attempts, 1);
  assert.ok(await wallet.storage.getAlarm(), 'a retry is scheduled');
  assert.equal((await api(env, 'GET', `/api/commerce/purchases/${purchase.id}`, { who: 'creator-1' })).status, 404);

  env.COMMERCE_HUBS.intercept = null;
  makeDue(wallet);
  await wallet.alarm();
  assert.deepEqual(owedSteps(wallet), []);
  assert.equal(await balance(env, 'creator-1'), 35);
  assert.ok([...env.INVENTORY.store.keys()].some((k) => k.startsWith(`txn:buyer-1:`)), 'the books are written');
  const sale = await api(env, 'GET', `/api/commerce/purchases/${purchase.id}`, { who: 'creator-1' });
  assert.equal(sale.status, 200);
  assert.equal((await events(env, 'creator-1'))[0].type, 'purchase.succeeded');
});

test('a step added while a purchase is still finishing outlives that finish', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  // Hold the purchase's last step (its books) until released, and have the
  // hub refuse the fulfillment once, so the step fulfill adds is still owed
  // when the purchase's own drain deletes the steps it ran.
  let release;
  let atBooks = false;
  const held = new Promise((r) => { release = r; });
  const put = env.INVENTORY.put;
  env.INVENTORY.put = async (key, ...rest) => {
    if (key.startsWith('txn:buyer-1:')) {
      atBooks = true;
      await held;
    }
    return put(key, ...rest);
  };
  let refusals = 1;
  env.COMMERCE_HUBS.intercept = (_id, op) => (op === 'sale_fulfilled' && refusals-- > 0
    ? new Response(JSON.stringify({ ok: false, error: 'unavailable' }), { status: 503 })
    : null);

  const buying = buy(env, product);
  await until(() => atBooks);
  const wallet = walletOf(env, 'buyer-1');
  const purchaseId = [...wallet.storage.map.keys()].find((k) => k.startsWith('pur:')).slice(4);
  // Fulfill while the purchase's drain is still at its books. Fulfill's own
  // drain reaches the books too, so both wait for the release.
  const fulfilling = api(env, 'POST', `/api/commerce/purchases/${purchaseId}/fulfill`, { who: 'buyer-1', body: { sim_id: SIM } });
  await until(() => owedSteps(wallet).some((o) => o.step === 'hub_fulfilled'));
  release();
  assert.equal((await buying).status, 201);
  assert.equal((await fulfilling).status, 200);
  env.INVENTORY.put = put;

  const owed = owedSteps(wallet);
  assert.deepEqual(owed.map((o) => o.step), ['hub_fulfilled'], 'only the steps that ran were deleted');
  assert.equal(owed[0].attempts, 1);
  const hub = hubOf(env, 'creator-1');
  assert.equal((await hub.storage.get(`sale:${purchaseId}`)).fulfilled, false);

  makeDue(wallet);
  await wallet.alarm();
  assert.deepEqual(owedSteps(wallet), []);
  assert.equal((await hub.storage.get(`sale:${purchaseId}`)).fulfilled, true, 'the hub heard about the fulfillment');
  assert.equal((await events(env, 'creator-1')).filter((e) => e.type === 'purchase.fulfilled').length, 1);
});

// ── Receipts by purchase id (how a host settles a player's purchase) ────────

test('a purchase id alone lets a host read that purchase, and nothing more', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  const { purchase } = (await buy(env, product)).body;

  const read = await api(env, 'GET', `/api/commerce/receipts/${SIM}/${purchase.id}`);
  assert.equal(read.status, 200, JSON.stringify(read.body));
  assert.deepEqual(
    [read.body.buyer_id, read.body.status, read.body.fulfilled, read.body.product.number, read.body.product.type],
    ['buyer-1', 'succeeded', false, product.number, 'consumable'],
  );
  assert.equal(read.body.idempotency_key, undefined);
  assert.equal((await api(env, 'GET', `/api/commerce/receipts/${SIM_OTHER}/${purchase.id}`)).status, 404, 'another simulation');
  assert.equal((await api(env, 'GET', `/api/commerce/receipts/${SIM}/pur_${'x'.repeat(24)}`)).status, 404, 'an unknown id');

  // The id cannot mark the purchase granted: only the buyer or the creator can.
  const nobody = await api(env, 'POST', `/api/commerce/receipts/${SIM}/${purchase.id}/fulfill`);
  assert.notEqual(nobody.status, 200);
  const stranger = await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/fulfill`, { who: 'buyer-2', body: { sim_id: SIM } });
  assert.equal(stranger.status, 404);
  const creator = await api(env, 'POST', `/api/commerce/purchases/${purchase.id}/fulfill`, { who: 'creator-1', body: { sim_id: SIM } });
  assert.equal(creator.status, 200, 'the creator hosting the session may');
  assert.equal((await api(env, 'GET', `/api/commerce/receipts/${SIM}/${purchase.id}`)).body.fulfilled, true);

  const other = (await buy(env, product)).body.purchase;
  await api(env, 'POST', `/api/commerce/purchases/${other.id}/refund`, { who: 'creator-1' });
  assert.equal((await api(env, 'GET', `/api/commerce/receipts/${SIM}/${other.id}`)).body.status, 'refunded');
});

test('the creator\'s host reads a joined player\'s passes and ungranted purchases, and nobody else can', async () => {
  const env = world();
  const coins = await activate(env, (await createProduct(env)).id);
  const vip = await activate(env, (await createProduct(env, { name: 'VIP', type: 'pass', price: 100 })).id);
  const bought = (await buy(env, coins)).body.purchase;
  await buy(env, vip);

  const read = await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM}`, { who: 'creator-1' });
  assert.equal(read.status, 200, JSON.stringify(read.body));
  assert.equal(read.body.livemode, true);
  assert.deepEqual(read.body.pending.map((r) => [r.purchase_id, r.buyer_id, r.product.number]), [[bought.id, 'buyer-1', coins.number]]);
  assert.deepEqual(read.body.entitlements.map((e) => e.number), [vip.number]);

  // Modes stay apart: the creator's test mode sees none of the live purchases.
  const test = await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM}`, { who: 'creator-1', mode: 'test' });
  assert.deepEqual([test.body.pending.length, test.body.entitlements.length], [0, 0]);

  // Granting clears the purchase from what the next session reads.
  await api(env, 'POST', `/api/commerce/purchases/${bought.id}/fulfill`, { who: 'creator-1', body: { sim_id: SIM } });
  const after = await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM}`, { who: 'creator-1' });
  assert.equal(after.body.pending.length, 0);

  assert.equal((await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM}`, { who: 'creator-2' })).status, 403, 'another creator');
  assert.equal((await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM}`, { who: 'buyer-2' })).status, 403, 'another player');
  assert.equal((await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM}`)).status, 401, 'signed out');
  assert.equal((await api(env, 'GET', `/api/commerce/players/buyer-1?sim_id=${SIM_OTHER}`, { who: 'creator-1' })).status, 403, 'not its simulation');
  assert.equal((await api(env, 'GET', '/api/commerce/players/buyer-1', { who: 'creator-1' })).status, 404, 'no simulation named');
  assert.equal((await api(env, 'GET', `/api/commerce/players/nobody-9?sim_id=${SIM}`, { who: 'creator-1' })).status, 404);
  assert.equal(walletOf(env, 'nobody-9'), undefined, 'asking about an unknown account opens no wallet');
});

// ── Test mode ───────────────────────────────────────────────────────────────

test('a session that sends no Eustress-Mode is in test mode and cannot spend Tickets', async () => {
  const env = world();
  const product = await activate(env, (await createProduct(env)).id);
  const player = await buy(env, product, { mode: null });
  assert.equal(player.status, 403);
  assert.equal(player.body.code, 'test_purchase_not_owner');
  assert.match(player.body.error, /Eustress-Mode: live/, 'the refusal says how to buy for real');
  assert.equal(await balance(env, 'buyer-1'), 500);
  const creator = await buy(env, product, { who: 'creator-1', mode: null });
  assert.equal(creator.status, 201);
  assert.equal(creator.body.purchase.livemode, false);
  assert.equal((await api(env, 'GET', '/api/commerce/account', { who: 'creator-1', mode: null })).body.livemode, false);
  // A key's prefix decides, whatever the header says.
  const testKey = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  assert.equal((await api(env, 'GET', '/api/commerce/account', { who: testKey, mode: 'live' })).body.livemode, false);
});

test('test purchases are the creator\'s, move nothing, and stay out of live data', async () => {
  const env = world();
  const draft = await createProduct(env);
  const r = await buy(env, draft, { who: 'creator-1', mode: 'test', price: null });
  assert.equal(r.status, 201, JSON.stringify(r.body));
  assert.equal(r.body.purchase.livemode, false);
  assert.equal(r.body.balance, undefined);
  assert.equal(await balance(env, 'creator-1'), 0);
  assert.equal((await buy(env, draft, { who: 'buyer-1', mode: 'test' })).body.code, 'test_purchase_not_owner');

  const key = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  const liveKey = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'live' } })).body.key;
  const testEvents = await events(env, key);
  assert.ok(testEvents.some((e) => e.type === 'purchase.succeeded' && e.livemode === false));
  assert.ok(testEvents.some((e) => e.type === 'product.created'), 'product events show in both modes');
  const liveEvents = await events(env, liveKey);
  assert.ok(!liveEvents.some((e) => e.type === 'purchase.succeeded'));
  assert.ok(liveEvents.some((e) => e.type === 'product.created'));
  assert.equal((await api(env, 'GET', `/api/commerce/purchases/${r.body.purchase.id}`, { who: liveKey })).status, 404);
  assert.equal((await api(env, 'GET', `/api/commerce/purchases/${r.body.purchase.id}`, { who: key })).status, 200);
});

test('test mode shapes drafts; only live mode puts a product on sale or changes one on sale', async () => {
  const env = world();
  const key = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  const body = { sim_id: SIM, space: 'Lobby', name: 'Gem', price: 5 };
  assert.equal((await api(env, 'POST', '/api/commerce/products', { who: key, body: { ...body, active: true } })).body.code, 'livemode_required');
  const draft = await api(env, 'POST', '/api/commerce/products', { who: key, body });
  assert.equal(draft.status, 201);
  const edited = await api(env, 'POST', `/api/commerce/products/${draft.body.id}`, { who: key, body: { price: 7 } });
  assert.equal(edited.body.price, 7, 'a draft is test data');
  assert.equal((await api(env, 'POST', `/api/commerce/products/${draft.body.id}`, { who: key, body: { active: true } })).body.code, 'livemode_required');

  await activate(env, draft.body.id); // a live session
  for (const r of [
    await api(env, 'POST', `/api/commerce/products/${draft.body.id}`, { who: key, body: { price: 1 } }),
    await api(env, 'DELETE', `/api/commerce/products/${draft.body.id}`, { who: key }),
    await api(env, 'POST', `/api/commerce/products/${draft.body.id}`, { who: 'creator-1', mode: null, body: { price: 1 } }),
  ]) {
    assert.equal(r.status, 403);
    assert.equal(r.body.code, 'livemode_required');
  }
  assert.equal((await api(env, 'GET', `/api/commerce/products/${draft.body.id}`, { who: key })).body.price, 7);
});

test('triggers fire synthetic events in test mode only', async () => {
  const env = world();
  const product = await createProduct(env);
  const key = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  const liveKey = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'live' } })).body.key;
  const post = (who, body) => api(env, 'POST', '/api/commerce/test_helpers/trigger', { who, body: { sim_id: SIM, product: product.number, ...body } });

  assert.equal((await post(liveKey, { event: 'purchase.succeeded' })).body.code, 'livemode_not_allowed');
  assert.equal((await post(key, { event: 'charge.succeeded' })).status, 400);

  const ok = await post(key, { event: 'purchase.succeeded' });
  assert.equal(ok.status, 200, JSON.stringify(ok.body));
  assert.equal(ok.body.purchase.synthetic, true);
  assert.match(ok.body.purchase.buyer_id, /^test_buyer_/);
  const refunded = await post(key, { event: 'purchase.refunded' });
  assert.equal(refunded.body.purchase.status, 'refunded');
  const failed = await post(key, { event: 'purchase.failed' });
  assert.equal(failed.body.event.type, 'purchase.failed');

  // The creator (or their game) can acknowledge a synthetic purchase.
  const acked = await api(env, 'POST', `/api/commerce/purchases/${ok.body.purchase.id}/fulfill`, { who: key, body: { sim_id: SIM } });
  assert.equal(acked.body.fulfilled, true);
  assert.equal(await balance(env, 'creator-1'), 0);
});

// ── Events, listen and webhooks ─────────────────────────────────────────────

test('events page newest first and stream oldest first from a cursor', async () => {
  const env = world();
  const key = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  const start = await api(env, 'GET', '/api/commerce/events/stream?after=now', { who: key });
  assert.equal(start.body.data.length, 0);
  const cursor = start.body.cursor;

  const made = [];
  for (let i = 0; i < 3; i++) made.push(await createProduct(env, { name: `P${i}` }));

  const page1 = await api(env, 'GET', '/api/commerce/events?limit=2', { who: key });
  assert.equal(page1.body.data.length, 2);
  assert.equal(page1.body.has_more, true);
  assert.equal(page1.body.data[0].data.object.id, made[2].id);
  const page2 = await api(env, 'GET', `/api/commerce/events?limit=2&starting_after=${page1.body.data[1].id}`, { who: key });
  assert.equal(page2.body.data.length, 1);
  assert.equal(page2.body.has_more, false);
  assert.equal(page2.body.data[0].data.object.id, made[0].id);

  const streamed = await api(env, 'GET', `/api/commerce/events/stream?after=${cursor}`, { who: key });
  assert.deepEqual(streamed.body.data.map((e) => e.data.object.id), made.map((p) => p.id));
  const empty = await api(env, 'GET', `/api/commerce/events/stream?after=${streamed.body.cursor}`, { who: key });
  assert.equal(empty.body.data.length, 0);
  assert.equal(empty.body.cursor, streamed.body.cursor);

  const filtered = await api(env, 'GET', `/api/commerce/events/stream?after=${cursor}&types=purchase.succeeded`, { who: key });
  assert.equal(filtered.body.data.length, 0);
});

test('a waiting stream returns as soon as an event is published', async () => {
  const env = world();
  const key = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  const { cursor } = (await api(env, 'GET', '/api/commerce/events/stream?after=now', { who: key })).body;
  const began = Date.now();
  const waiting = api(env, 'GET', `/api/commerce/events/stream?after=${cursor}&wait=5`, { who: key });
  await new Promise((r) => setTimeout(r, 50));
  await createProduct(env, { name: 'Wakes the stream' });
  const woke = await waiting;
  assert.equal(woke.body.data.length, 1);
  assert.ok(Date.now() - began < 2000, 'returned on the event, not the timeout');
});

test('webhooks are signed, retried on failure, and managed per mode', async () => {
  const env = world();
  const key = (await api(env, 'POST', '/api/keys', { who: 'creator-1', body: { mode: 'test' } })).body.key;
  assert.equal((await api(env, 'POST', '/api/commerce/webhook_endpoints', { who: key, body: { url: 'http://localhost:4242/hook' } })).status, 400);
  assert.equal((await api(env, 'POST', '/api/commerce/webhook_endpoints', { who: key, body: { url: 'https://h.example.com/x', enabled_events: ['nope'] } })).status, 400);
  const created = await api(env, 'POST', '/api/commerce/webhook_endpoints', {
    who: key, body: { url: 'https://hooks.example.com/eustress', enabled_events: ['purchase.succeeded'] },
  });
  assert.equal(created.status, 201);
  assert.match(created.body.secret, /^whsec_/);
  const listed = await api(env, 'GET', '/api/commerce/webhook_endpoints', { who: key });
  assert.equal(listed.body.data.length, 1);
  assert.equal(listed.body.data[0].secret, undefined);

  const product = await createProduct(env);
  await api(env, 'POST', '/api/commerce/test_helpers/trigger', { who: key, body: { sim_id: SIM, product: product.id, event: 'purchase.succeeded' } });

  const calls = [];
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (url, init) => {
    calls.push({ url, init });
    return new Response('nope', { status: calls.length === 1 ? 500 : 200 });
  };
  try {
    const hub = hubOf(env, 'creator-1');
    await hub.deliverDue(Date.now());
    assert.equal(calls.length, 1, 'only the subscribed event is delivered');
    const body = calls[0].init.body;
    assert.equal(JSON.parse(body).type, 'purchase.succeeded');
    const sig = calls[0].init.headers['eustress-signature'];
    assert.equal(await verifySignatureHeader(created.body.secret, body, sig), true);
    // The failed attempt is rescheduled, and succeeds when retried.
    const retries = [...hub.storage.map.keys()].filter((k) => k.startsWith('wd:'));
    assert.equal(retries.length, 1);
    await hub.deliverDue(Date.now() + 3600e3);
    assert.equal(calls.length, 2);
    const endpoint = await hub.storage.get(`we:${created.body.id}`);
    assert.equal(endpoint.last_delivery.ok, true);
    assert.equal(endpoint.last_delivery.attempt, 2);
  } finally {
    globalThis.fetch = realFetch;
  }

  const removed = await api(env, 'DELETE', `/api/commerce/webhook_endpoints/${created.body.id}`, { who: key });
  assert.equal(removed.body.deleted, true);
});

test('the listen secret is stable per mode', async () => {
  const env = world();
  const a = await api(env, 'GET', '/api/commerce/listen/secret', { who: 'creator-1', mode: 'test' });
  const b = await api(env, 'GET', '/api/commerce/listen/secret', { who: 'creator-1', mode: 'test' });
  const live = await api(env, 'GET', '/api/commerce/listen/secret', { who: 'creator-1' });
  assert.match(a.body.secret, /^whsec_/);
  assert.equal(a.body.secret, b.body.secret);
  assert.notEqual(a.body.secret, live.body.secret);
});

test('the account lists the creator\'s simulations with their Spaces and products', async () => {
  const env = world();
  await createProduct(env);
  const r = await api(env, 'GET', '/api/commerce/account', { who: 'creator-1' });
  assert.equal(r.status, 200);
  const byId = Object.fromEntries(r.body.simulations.map((s) => [s.id, s]));
  assert.deepEqual(byId[SIM].spaces, ['Arena', 'Lobby']);
  assert.equal(byId[SIM].products, 1);
  assert.equal(byId[SIM].can_sell, true);
  assert.equal(byId[SIM_HELD].can_sell, false);
  assert.equal(byId[SIM_OTHER], undefined, 'another creator\'s simulation is not listed');
});

test('the router ignores paths it does not own and refuses the unauthenticated', async () => {
  const env = world();
  assert.equal((await api(env, 'GET', '/api/purchases', { who: 'buyer-1' })).status, 0);
  assert.equal((await api(env, 'GET', '/api/commerce/products')).status, 401);
  assert.equal((await api(env, 'GET', '/api/commerce/products', { who: 'buyer-1', mode: 'sideways' })).status, 400);
});
