// PROBE g2a-4 (G-5, visits): a guest's join visit is keyed `g:<lease id>`. One address may open any number of leases on the same
// listing in a day (`begin` replaces the lease and charges nothing for a listing already in `sims`), so each new lease that is
// admitted is a new visit and a new totalPlays. The author is the host who calls /live/admit, so the author can count visits to
// their own listing at the auth limiter's rate (30 calls a minute per address, two per visit). visits.mjs promises "at most
// once per person or device per listing per UTC day". Expected: one address joining one listing five times in a day counts
// at most one visit. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, hostBeat, openLease, mint, admit, withClock, SIM, AUTHOR, T0 } from './zz_guest_b_common.mjs';

test('PROBE g2a-4: one address re-opening leases on one listing counts at most one visit a day', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    await seed(env);
    assert.equal((await hostBeat(env)).status, 200);
    const statuses = [];
    for (let i = 0; i < 5; i++) {
      clock.t += 1_000;
      const lease = await openLease(env);
      const minted = await mint(env, lease.body.ticket);
      statuses.push((await admit(env, minted.body.ticket)).status);
    }
    const visits = Number(await env.SOCIAL.get(`visits:${SIM}`)) || 0;
    const plays = Number(await env.SOCIAL.get(`totalPlays:${AUTHOR}`)) || 0;
    console.log('admits', JSON.stringify(statuses), '| visits counted', visits, '| author totalPlays', plays);
    assert.ok(visits <= 1, `one address counted ${visits} visits to one listing in one day`);
  });
});
