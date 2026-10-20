// PROBE 1: the guest join ticket hands the host (another user) the guest's day-stable id.
// Two different hosts, two different listings, one guest (one address) on one day: the tickets each host receives in Hello
// carry the same `g`, readable without any key (base64url JSON). Expected (privacy): a host-visible ticket carries nothing a
// host could match to the same guest elsewhere. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  environment, seed, hostBeat, openLease, guestTicket, payloadOf, withClock, tokenOf,
  SIM, SIM_B, PIN, PIN2, AUTHOR2, T0,
} from './zz_guest_b_common.mjs';

test('PROBE 1: two hosts cannot match the same guest from the join tickets they receive', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    await seed(env, { simBAuthor: AUTHOR2 });
    assert.equal((await hostBeat(env)).status, 200); // host 1 hosts SIM at PIN
    assert.equal((await hostBeat(env, {}, SIM_B, PIN2, tokenOf(AUTHOR2))).status, 200); // host 2 hosts SIM_B at PIN2

    const leaseA = (await openLease(env, SIM)).body.ticket;
    const toHost1 = await guestTicket(env, leaseA, PIN);
    clock.t += 3 * 3600 * 1000; // hours later, same UTC day, same address, another listing
    assert.equal((await hostBeat(env)).status, 200);
    assert.equal((await hostBeat(env, {}, SIM_B, PIN2, tokenOf(AUTHOR2))).status, 200);
    const leaseB = (await openLease(env, SIM_B)).body.ticket;
    const toHost2 = await guestTicket(env, leaseB, PIN2);

    const p1 = payloadOf(toHost1.ticket);
    const p2 = payloadOf(toHost2.ticket);
    console.log('host 1 sees', JSON.stringify(p1));
    console.log('host 2 sees', JSON.stringify(p2));
    const gidName = [...env.GUEST_ALLOWANCE.instances.keys()];
    console.log('GuestAllowance object names (guest ids):', gidName);
    // ADJUSTED (fix): the tickets no longer carry `g` at all, so comparing p1.g with p2.g compares undefined with undefined.
    // What the probe means: no field both hosts read has the same value.
    const shared = Object.keys(p1).filter((k) => k in p2 && JSON.stringify(p1[k]) === JSON.stringify(p2[k]));
    assert.deepEqual(shared, [], 'host 1 and host 2 both read the same value from the ticket (the old field g, the guest id)');
  });
});
