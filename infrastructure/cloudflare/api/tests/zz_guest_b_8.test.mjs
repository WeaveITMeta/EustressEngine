// PROBE 8: the per-address limiter on /api/guest/play and /api/guest/ticket is keyed on the full CF-Connecting-IP, while one
// guest is an IPv6 /64. A device steps through its own /64 and each address gets a fresh limiter budget for the same guest.
// The limiter here allows 1 call per key (a stand-in for "the minute is used up"). Expected: the second call from the same
// guest (/64) within the window is limited. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, openLease, withClock, payloadOf, SIM, T0 } from './zz_guest_b_common.mjs';

test('PROBE 8: one IPv6 /64 guest shares one limiter budget', async () => {
  const used = new Map();
  const env = environment({ AUTH_RATE_LIMITER: { limit: async ({ key }) => { const n = (used.get(key) || 0) + 1; used.set(key, n); return { success: n <= 1 }; } } });
  await withClock(T0, async () => {
    await seed(env);
    const a = await openLease(env, SIM, { 'CF-Connecting-IP': '2001:db8:1:2::1' });
    const b = await openLease(env, SIM, { 'CF-Connecting-IP': '2001:db8:1:2::2' });
    console.log('statuses', a.status, b.status, '| same guest id:', /* ADJUSTED (fix): b has no ticket once limited */ (b.body?.ticket ? payloadOf(a.body.ticket).g === payloadOf(b.body.ticket).g : 'n/a'), '| limiter keys:', [...used.keys()]);
    assert.equal(b.status, 429, 'the same /64 guest got a fresh limiter budget from the next address');
  });
});
