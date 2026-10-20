// PROBE 5: no Worker-side precondition before a guest (possibly a child) is put into a session run by today's host.
// GUEST_PLAY.md section 13 says today's host still delivers every other player's chat to a guest, shows the free-text name from
// the guest's Hello rather than "Guest", and does not act on ended_guests. The heartbeat carries `protocol` (and could carry a
// capability), but /live/admit admits a guest to any host. So deploying this Worker turns guest joins on against hosts that do
// none of the three. Expected: a guest is admitted only to a host that has declared it withholds chat from guests and names
// them "Guest" (here, any declaration at all). FAILS on the current code: admitted on a plain protocol-3 heartbeat.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, withClock, T0 } from './zz_guest_b_common.mjs';

test('PROBE 5: a guest is not admitted to a host that has not declared guest protections', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env); // the host beat with { protocol: 3 } and nothing else
    const { ticket } = await guestTicket(env, lease);
    // ADJUSTED (fix): the shared hostBeat now declares the guest protections; this probe's host is today's host, whose latest
    // heartbeat declares none (and GUEST_PLAY is "on", so the setting alone does not refuse it).
    assert.equal((await hostBeat(env, { guest_caps: undefined })).status, 200);
    const r = await admit(env, ticket);
    console.log('admit for a guest on a plain protocol-3 host:', r.status, JSON.stringify(r.body));
    assert.equal(r.status, 403, 'admitted with no host-side guest protection declared');
  });
});
