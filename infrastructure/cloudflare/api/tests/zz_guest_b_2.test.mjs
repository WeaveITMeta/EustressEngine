// PROBE 2: one guest lease mints any number of join tickets, and each is admitted as a separate guest.
// "A ticket pasted into a chat serves one player" (GUEST_PLAY.md section 4.3) does not hold for hosted play: one allowance
// (one address) puts several concurrent players into a session, all charged as one lease, each counted as a new visit, and one
// person can fill the session's MAX_GUESTS. Expected: one lease holds at most one hosted seat. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  environment, hostedWithGuest, guestTicket, admit, hostBeat, call, withClock, SIM, IP, T0,
} from './zz_guest_b_common.mjs';
import { DAILY_SECONDS } from '../src/guest.mjs';

test('PROBE 2: one guest lease admits only one guest at a time', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    // Three join tickets from the same lease, minted from three different addresses (the route "works from any address").
    const t1 = await guestTicket(env, lease);
    const t2 = (await call(env, 'POST', '/api/guest/ticket', { headers: { 'CF-Connecting-IP': '198.51.100.1' }, body: { lease, audience: 'ab'.repeat(32) } })).body;
    const t3 = (await call(env, 'POST', '/api/guest/ticket', { headers: { 'CF-Connecting-IP': '198.51.100.2' }, body: { lease, audience: 'ab'.repeat(32) } })).body;
    const results = [];
    for (const t of [t1, t2, t3]) results.push((await admit(env, t.ticket)).status);
    const storage = (env.LIVE_HOSTS.instances.get(`guest:${SIM}`) ?? env.LIVE_HOSTS.instances.get(SIM)) /* ADJUSTED (fix): the guest-only session object, or the one object before guest-only sessions */.storage;
    const gadm = [...(await storage.list({ prefix: 'gadm:' })).values()];
    console.log('admit statuses:', results, '| gadm entries:', gadm.length, '| distinct leases:', new Set(gadm.map((g) => g.tid)).size);

    for (let i = 0; i < 4; i++) {
      clock.t += 30_000;
      await hostBeat(env, { players: 4, admitted: [t1.id, t2.id, t3.id] });
    }
    const left = (await call(env, 'GET', '/api/guest/allowance', { headers: IP })).body.seconds_left;
    console.log('three players for 120 s charged', DAILY_SECONDS - left, 's; visits counted:', await env.SOCIAL.get(`visits:${SIM}`));
    assert.deepEqual(results, [200, 403, 403], 'one lease, three concurrent guests admitted');
  });
});
