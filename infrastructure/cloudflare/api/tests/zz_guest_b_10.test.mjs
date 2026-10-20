// PROBE 10: asking what is left (GET /api/guest/allowance, which a signed-out listing page can call before any Play) creates
// and stores a guest record keyed by the address hash, with a 25 h alarm, for a visitor who never plays. The route is GET, so
// the per-address limiter (POST only) does not apply. Expected: asking stores nothing. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, call, withClock, T0 } from './zz_guest_b_common.mjs';

test('PROBE 10: a visitor who only asks what is left leaves nothing stored', async () => {
  await withClock(T0, async () => {
    const env = environment();
    await seed(env);
    const r = await call(env, 'GET', '/api/guest/allowance', { headers: { 'CF-Connecting-IP': '192.0.2.44' } });
    assert.equal(r.status, 200);
    const stored = [...env.GUEST_ALLOWANCE.instances.entries()].map(([name, o]) => ({ name, keys: [...o.storage.map.keys()], value: o.storage.map.get('a'), alarm: o.storage.alarm }));
    console.log('stored after one GET:', JSON.stringify(stored));
    assert.equal(stored.filter((s) => s.keys.length).length, 0, 'a record was stored for a visitor who never played');
  });
});
