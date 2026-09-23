import test from 'node:test';
import assert from 'node:assert/strict';
import {
  cleanText, cleanIcon, cleanProductId, purchaseKey, fitMetadata, resolveAttribution,
  payableCreator, recordPurchase, listReceipts, aggregate, handlePurchases,
} from '../src/purchases.mjs';

// KV as the Worker sees it: keys listed in byte order, a cursor per page, and
// metadata over 1,024 bytes refused at write time the way Cloudflare refuses it.
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
const environment = () => ({ INVENTORY: kv(), SOCIAL: kv(), USERS: kv() });

const SIM_A = 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa';
const SIM_B = 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb';
function seed(env) {
  env.USERS.store.set('user:creator-1', { value: JSON.stringify({ id: 'creator-1', username: 'nova' }) });
  env.USERS.store.set('user:creator-2', { value: JSON.stringify({ id: 'creator-2', username: 'orbit' }) });
  env.SOCIAL.store.set(`sim:${SIM_A}`, { value: JSON.stringify({ id: SIM_A, name: 'Reactor Lab', author_id: 'creator-1', author_name: 'nova-old' }) });
  env.SOCIAL.store.set(`sim:${SIM_B}`, { value: JSON.stringify({ id: SIM_B, name: 'Tide Pools', author_id: 'creator-2', author_name: 'orbit' }) });
}

const deps = {
  verifyAuth: async (request) => {
    const h = request.headers.get('Authorization') || '';
    return h.startsWith('Bearer ') ? h.slice(7) : null;
  },
  json: (value, status, headers) => new Response(JSON.stringify(value), { status, headers }),
  blissUnit: 100,
};
const get = (token) => new Request('https://api.eustress.dev/api/purchases', {
  headers: token ? { Authorization: `Bearer ${token}` } : {},
});
const at = (second) => new Date(Date.UTC(2026, 8, 1, 12, 0, second)).toISOString();

test('display text loses control and bidi characters and is capped in bytes on a character boundary', () => {
  assert.equal(cleanText('  VIP\u202e Pass\n\t ', 120), 'VIP Pass');
  assert.equal(cleanText(42, 10), '');
  assert.equal(cleanText('é'.repeat(100), 11), 'ééééé');
  assert.equal(cleanText('😀😀😀', 7), '😀');
});

test('icons are kept only when Eustress serves them', () => {
  for (const ok of [
    'https://api.eustress.dev/api/simulations/abc/thumbnail',
    'https://eustress.dev/assets/icons/star.svg',
    '/assets/icons/star.svg',
  ]) assert.notEqual(cleanIcon(ok), '', ok);
  for (const bad of [
    'http://api.eustress.dev/x.png',
    'https://example.com/x.png',
    'https://eustress.dev.example.com/x.png',
    'https://notreallyeustress.dev/x.png',
    'javascript:alert(1)',
    'data:image/png;base64,AAAA',
    'https://user:pw@eustress.dev/x.png',
    'https://eustress.dev:8443/x.png',
    '/assets/../secret.png',
    '/assets//example.com/x.png',
    '//example.com/x.png',
    42,
  ]) assert.equal(cleanIcon(bad), '', String(bad));
});

test('product ids are engine numbers or short slugs', () => {
  assert.equal(cleanProductId(101), '101');
  assert.equal(cleanProductId('vip-pass'), 'vip-pass');
  for (const bad of ['has space', -1, 1.5, 'x'.repeat(65), '', null, '../etc']) assert.equal(cleanProductId(bad), '', String(bad));
});

test('receipt keys list newest first', () => {
  const older = purchaseKey('u', Date.parse(at(1)), 'a');
  const newer = purchaseKey('u', Date.parse(at(2)), 'b');
  assert.deepEqual([older, newer].sort(), [newer, older]);
});

test('metadata always fits the KV limit, dropping the icon before trimming text', () => {
  const plain = { v: 1, ts: at(0), c: 'TKT', a: 5, t: 'VIP Pass', i: '/assets/icons/star.svg' };
  assert.deepEqual(fitMetadata(plain), plain);
  const hostile = {
    v: 1, ts: at(0), c: 'BLS', a: 123456789,
    t: '"'.repeat(120), i: 'https://api.eustress.dev/' + 'a'.repeat(270), p: 'p'.repeat(64),
    s: SIM_A, sn: '\\'.repeat(96), k: 'k'.repeat(128), kn: '"'.repeat(64),
  };
  const fitted = fitMetadata(hostile);
  assert.ok(Buffer.byteLength(JSON.stringify(fitted)) <= 1024);
  assert.equal(fitted.i, undefined);
  assert.equal(fitted.a, 123456789);
});

test('a simulation names its own creator, and a caller cannot credit someone else', async () => {
  const env = environment(); seed(env);
  assert.deepEqual(await resolveAttribution(env, { simulation_id: SIM_A }), {
    simulation_id: SIM_A, simulation_name: 'Reactor Lab', creator_id: 'creator-1', creator_name: 'nova',
    verified: true,
  });
  assert.equal((await resolveAttribution(env, { simulation_id: SIM_A, creator_id: 'creator-1' })).creator_id, 'creator-1');
  assert.equal((await resolveAttribution(env, { simulation_id: SIM_A, creator_id: 'creator-2' })).status, 400);
  assert.equal((await resolveAttribution(env, { simulation_id: 'cccccccc-3333-4333-8333-cccccccccccc' })).status, 404);
  assert.equal((await resolveAttribution(env, { simulation_id: 'not a uuid!' })).status, 400);
  assert.equal((await resolveAttribution(env, { creator_id: 'creator-2' })).creator_name, 'orbit');
  assert.equal((await resolveAttribution(env, { creator_id: 'nobody' })).status, 404);
  assert.equal((await resolveAttribution(env, { creator_id: 'a:b' })).status, 400);
  assert.deepEqual(await resolveAttribution(env, {}), { simulation_id: null, simulation_name: '', creator_id: null, creator_name: '', verified: false });
});

test('a Ticket sale pays only a creator the server resolved from a simulation', async () => {
  const env = environment(); seed(env);

  // The exploit: name any existing account and omit simulation_id. The account
  // exists, so attribution resolves -- but nothing proves it is owed the sale.
  const claimed = await resolveAttribution(env, { creator_id: 'creator-2' });
  assert.equal(claimed.creator_id, 'creator-2');
  assert.equal(claimed.verified, false);
  assert.equal(payableCreator(claimed).status, 400, 'an unverified creator must never be paid');
  assert.equal(payableCreator(claimed).creator_id, undefined, 'and no recipient is returned');

  // The legitimate path: the simulation names its author, who is paid.
  const fromSim = await resolveAttribution(env, { simulation_id: SIM_A });
  assert.deepEqual(payableCreator(fromSim), { creator_id: 'creator-1' });

  // Naming the author alongside their own simulation still pays them.
  const both = await resolveAttribution(env, { simulation_id: SIM_A, creator_id: 'creator-1' });
  assert.deepEqual(payableCreator(both), { creator_id: 'creator-1' });

  // No creator at all is a platform sale: nothing to pay, nothing refused.
  assert.deepEqual(payableCreator(await resolveAttribution(env, {})), { creator_id: null });
});

async function buyFive(env, buyer = 'buyer-1') {
  const a = await resolveAttribution(env, { simulation_id: SIM_A });
  const b = await resolveAttribution(env, { simulation_id: SIM_B });
  const c2 = await resolveAttribution(env, { creator_id: 'creator-2' });
  const none = await resolveAttribution(env, {});
  await recordPurchase(env, buyer, { ts: at(1), currency: 'TKT', units: 250, title: 'VIP Pass', icon: '/assets/icons/star.svg', product_id: 'vip', attribution: a });
  await recordPurchase(env, buyer, { ts: at(2), currency: 'TKT', units: 100, title: 'Snorkel', product_id: '7', attribution: b });
  await recordPurchase(env, buyer, { ts: at(3), currency: 'BLS', units: 1250, title: 'Reactor Tour', attribution: a });
  await recordPurchase(env, buyer, { ts: at(4), currency: 'BLS', units: 500, title: 'Commission', attribution: c2 });
  await recordPurchase(env, buyer, { ts: at(5), currency: 'TKT', units: 40, title: 'Tip jar', icon: 'https://example.com/pixel.png', attribution: none });
}

test('spend is totalled per creator and per simulation, never mixing currencies', async () => {
  const env = environment(); seed(env);
  await buyFive(env);
  const { items, truncated } = await listReceipts(env, 'buyer-1');
  assert.equal(truncated, false);
  assert.deepEqual(items.map((i) => i.title), ['Tip jar', 'Commission', 'Reactor Tour', 'Snorkel', 'VIP Pass']);
  assert.equal(items[0].icon, '', 'an icon on another host is not stored');

  const { totals, by_creator, by_simulation } = aggregate(items);
  assert.deepEqual(totals, { tickets: 390, bliss_minor: 1750, purchases: 5, creators: 2, simulations: 2 });
  assert.deepEqual(by_creator.map((c) => [c.creator_id, c.name, c.tickets, c.bliss_minor, c.purchases]), [
    ['creator-1', 'nova', 250, 1250, 2],
    ['creator-2', 'orbit', 100, 500, 2],
    [null, '', 40, 0, 1],
  ]);
  assert.deepEqual(by_simulation.map((s) => [s.simulation_id, s.name, s.creator_id, s.tickets, s.bliss_minor, s.purchases]), [
    [SIM_A, 'Reactor Lab', 'creator-1', 250, 1250, 2],
    [SIM_B, 'Tide Pools', 'creator-2', 100, 0, 1],
    [null, '', null, 40, 500, 2],
  ]);
});

test('only the signed-in owner can read their purchases, and never from a shared cache', async () => {
  const env = environment(); seed(env);
  await buyFive(env, 'buyer-1');

  assert.equal((await handlePurchases(get(null), env, {}, deps)).status, 401);
  assert.equal((await handlePurchases(get('buyer-1:'), env, {}, deps)).status, 401);

  const res = await handlePurchases(get('buyer-1'), env, { 'Access-Control-Allow-Origin': 'https://eustress.dev' }, deps);
  assert.equal(res.status, 200);
  assert.equal(res.headers.get('Cache-Control'), 'private, no-store');
  const body = await res.json();
  assert.equal(body.purchases.length, 5);
  assert.deepEqual(body.totals, { tickets: 390, bliss_minor: 1750, bliss: 17.5, purchases: 5, creators: 2, simulations: 2 });
  const tour = body.purchases.find((p) => p.title === 'Reactor Tour');
  assert.deepEqual([tour.currency, tour.amount, tour.amount_minor, tour.simulation_name, tour.creator_name], ['BLS', 12.5, 1250, 'Reactor Lab', 'nova']);
  const vip = body.purchases.find((p) => p.title === 'VIP Pass');
  assert.equal(vip.amount, 250);
  assert.equal('amount_minor' in vip, false);
  assert.equal(body.by_creator[0].bliss, 12.5);

  const other = await (await handlePurchases(get('buyer-2'), env, {}, deps)).json();
  assert.equal(other.purchases.length, 0);
  assert.equal(other.totals.purchases, 0);

  const summary = await (await handlePurchases(get('buyer-1'), env, {}, deps, { summaryOnly: true })).json();
  assert.equal(summary.totals.tickets, 390);
  assert.equal('purchases' in summary, false);
  assert.equal('by_creator' in summary, false);
});

test('totals span every page of receipts and say when they stop short', async () => {
  const env = environment();
  const none = await resolveAttribution(env, {});
  const base = Date.UTC(2026, 0, 1);
  for (let i = 0; i < 2345; i++) {
    await recordPurchase(env, 'buyer-3', { ts: new Date(base + i * 1000).toISOString(), currency: 'TKT', units: 1, title: `Item ${i}`, attribution: none });
  }
  const all = await listReceipts(env, 'buyer-3');
  assert.equal(all.items.length, 2345);
  assert.equal(all.truncated, false);
  assert.equal(aggregate(all.items).totals.tickets, 2345);
  assert.equal(all.items[0].title, 'Item 2344');

  const capped = await listReceipts(env, 'buyer-3', { maxPages: 2 });
  assert.equal(capped.items.length, 2000);
  assert.equal(capped.truncated, true);
});
