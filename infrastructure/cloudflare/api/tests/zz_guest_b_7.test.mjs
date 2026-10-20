// PROBE 7 (policy evidence): a banned account, still sending its own (now refused) token, is handed a guest lease and is
// admitted to a live all-ages session as an anonymous "Guest" alongside guests of any age. Nothing ties the guest to the ban.
// Whether that is acceptable is the owner's call; the probe asserts the stricter reading (a banned account's token is not
// silently treated as "signed out"), and FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, hostBeat, openLease, guestTicket, admit, tokenOf, withClock, PLAYER, T0 } from './zz_guest_b_common.mjs';

test('PROBE 7: a banned account is not turned into a guest of a live session', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    await seed(env);
    assert.equal((await hostBeat(env)).status, 200);
    const token = tokenOf(PLAYER);
    // The admin ban: the record is marked and every token it holds ends (index.js handleAdminBan moves authfloor).
    await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada', banned: true }));
    clock.t += 1000;
    await env.USERS.put(`authfloor:${PLAYER}`, String(Math.floor(clock.t / 1000)));
    const lease = await openLease(env, undefined, undefined, token);
    console.log('banned account at /api/guest/play:', lease.status, lease.body?.code ?? 'lease issued');
    if (lease.status === 200) {
      const { ticket } = await guestTicket(env, lease.body.ticket);
      const r = await admit(env, ticket);
      console.log('then admitted to the live all-ages session as:', r.status, r.body?.username, 'guest =', r.body?.guest);
    }
    assert.notEqual(lease.status, 200, 'the banned account got a guest lease');
  });
});
