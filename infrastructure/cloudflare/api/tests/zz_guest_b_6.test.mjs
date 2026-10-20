// PROBE 6: a guest the Worker forgets while the host still has it connected is never reported in ended_guests.
// The grace is measured from admission (`at`), not from the last time the host named the guest. After the first beat that omits
// the id (here at +100 s), the gadm entry is deleted; if the host names it again, nothing beats the lease, the lease ends by
// timeout, and ended_guests never names it. The host only lets a guest go on ended_guests, so the guest plays on, uncharged.
// Expected: an id the host names whose lease is not being beaten is reported ended. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, mint, withClock, T0 } from './zz_guest_b_common.mjs';

test('PROBE 6: a guest forgotten by the Worker but still named by the host is reported ended', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    clock.t += 60_000;
    await hostBeat(env, { players: 2, admitted: [id] }); // named
    clock.t += 40_000;
    await hostBeat(env, { players: 2, admitted: [] }); // one beat omits it (at +100 s): deleted at once, no grace from the last naming
    let report;
    for (let i = 0; i < 8; i++) {
      clock.t += 30_000;
      report = await hostBeat(env, { players: 2, admitted: [id] }); // named again on every beat after
    }
    const live = (await mint(env, lease)).status;
    console.log('lease still live?', live, '| last heartbeat answer:', JSON.stringify(report.body));
    // ADJUSTED (fix): the grace now runs from the last beat that named the guest, so the one omission no longer loses it and
    // its lease goes on being beaten (live, 200). The defect is a guest neither beaten nor reported.
    assert.ok(live === 200 || JSON.stringify(report.body.ended_guests) === JSON.stringify([id]), 'the host is never told to let this guest go');
  });
});
