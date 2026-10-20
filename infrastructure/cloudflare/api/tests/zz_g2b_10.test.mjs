// Probe G2B-10: round 2 puts the guest reads (GET /api/guest/live, GET /api/guest/allowance) in the SAME per-address bucket as
// sign-in, sign-up and KYC (AUTH_RATE_LIMITER, 30 a minute, keyed on the IPv4 address or IPv6 /64). Guests polling their Play
// chooser behind one address (a school, a household, a carrier NAT) use up that address's sign-in minute.
// Expected (fails today): reading the guest index never stops someone at the same address from starting a sign-in.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, call, windowLimiter, withClock, IP } from './zz_g2b_common.mjs';

test('G2B-10: 30 guest index reads from one address lock that address out of sign-in for the minute', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async (clock) => {
    const env = environment({ AUTH_RATE_LIMITER: windowLimiter(30) });
    await seed(env);
    for (let i = 0; i < 30; i++) { clock.t += 100; assert.equal((await call(env, 'GET', '/api/guest/live', { headers: IP })).status, 200); }
    clock.t += 100;
    const signIn = await call(env, 'POST', '/api/auth/challenge', { headers: IP, body: {} });
    console.log(`sign-in challenge after 30 guest index reads: ${signIn.status} ${signIn.body?.code}`);
    assert.notEqual(signIn.status, 429, 'sign-in refused because guests read the guest index');
  });
});

test('G2B-10 control: 29 guest index reads leave the sign-in minute open (passes)', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async (clock) => {
    const env = environment({ AUTH_RATE_LIMITER: windowLimiter(30) });
    await seed(env);
    for (let i = 0; i < 29; i++) { clock.t += 100; await call(env, 'GET', '/api/guest/live', { headers: IP }); }
    clock.t += 100;
    assert.notEqual((await call(env, 'POST', '/api/auth/challenge', { headers: IP, body: {} })).status, 429);
  });
});
