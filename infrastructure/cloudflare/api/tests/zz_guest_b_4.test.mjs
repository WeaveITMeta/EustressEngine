// PROBE 4: a guest who plays with the desktop Player (client "player", new in this change) or the browser Player fetches the
// world anonymously, and that fetch stores `visit:<listing>:<day>:d:<HMAC(address)>` in SOCIAL KV for 2 days. The HMAC key is
// NOT day-keyed, so the same address has the same marker on every day and every listing: a cross-day, cross-listing record of
// which listings one address played, kept 48 h. The guest allowance itself is day-keyed (a control below shows its id changes).
// The guest privacy draft says "cannot be ... matched from one day to the next" and "We delete all of it within 25 hours".
// Expected: nothing the guest path stores matches across days, and nothing outlives 25 h. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, openLease, call, withClock, payloadOf, SIM, SIM_B, IP, T0 } from './zz_guest_b_common.mjs';

test('PROBE 4: a guest playing two days running leaves no marker that matches across the days, and none kept past 25 h', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    await seed(env);
    for (const id of [SIM, SIM_B]) {
      const sim = JSON.parse(await env.SOCIAL.get(`sim:${id}`));
      sim.r2_key = `universes/${id}/universe.pak`;
      await env.SCENES.put(sim.r2_key, 'the world');
      await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
    }
    const ttl = new Map();
    const realPut = env.SOCIAL.put.bind(env.SOCIAL);
    env.SOCIAL.put = async (k, v, o) => { if (k.startsWith('visit:')) ttl.set(k, o?.expirationTtl); return realPut(k, v, o); };

    // Day 1: the guest opens a lease for the desktop Player and the Player downloads the world.
    const l1 = await openLease(env, SIM);
    assert.equal(l1.status, 200);
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM}/download`, { headers: IP })).status, 200);
    // Day 2: the same address, another listing.
    clock.t += 24 * 3600 * 1000;
    const l2 = await openLease(env, SIM_B);
    assert.equal(l2.status, 200);
    assert.equal((await call(env, 'GET', `/api/simulations/${SIM_B}/download`, { headers: IP })).status, 200);

    const markers = [...env.SOCIAL.store.keys()].filter((k) => k.startsWith('visit:'));
    console.log('control, allowance guest ids day 1 / day 2:', payloadOf(l1.body.ticket).g, '/', payloadOf(l2.body.ticket).g);
    for (const k of markers) console.log('stored', k, 'expirationTtl', ttl.get(k), 's');
    const who = markers.map((k) => k.split(':').slice(3).join(':'));
    assert.ok(ttl.size === 0 || [...ttl.values()].every((s) => s <= 25 * 3600), 'a marker is kept longer than 25 hours');
    assert.notEqual(who[0], who[1], 'the same address marker on both days and both listings');
  });
});
