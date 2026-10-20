// PROBE 3: a guest's id outlives the 25-hour promise in the LiveHost object.
// gadm:<join id> = {gid, tid, at, ended} is kept for as long as the host keeps naming the id, with no bound in time; an ended
// guest still named keeps its guest id stored. The guest's own allowance object deletes itself at day start + 25 h (its alarm),
// but this copy of the guest id does not. The privacy draft (GUEST_PLAY.md section 12) says "We delete all of it within 25
// hours". Expected: no guest id stored anywhere past 25 h after its day began. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, withClock, payloadOf, SIM, T0 } from './zz_guest_b_common.mjs';
import { dayStart } from '../src/guest.mjs';

async function fireDue(ns, now) {
  for (const obj of ns.instances.values()) {
    const at = obj.storage.alarm;
    if (at !== null && at !== undefined && at <= now) {
      await obj.storage.deleteAlarm();
      await obj.alarm();
    }
  }
}

test('PROBE 3: no guest id is stored past 25 hours after its day began', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    // ADJUSTED (fix): the ticket no longer carries the guest id; the one guest's allowance object is named by it.
    const gid = payloadOf(ticket).g ?? [...env.GUEST_ALLOWANCE.instances.keys()][0];
    assert.equal((await admit(env, ticket)).status, 200);

    const deadline = dayStart(T0) + 25 * 3600 * 1000;
    let endedSeenAt = null;
    // The host stays up and keeps listing the peer (a host that never drops the peer, or one that missed ended_guests).
    while (clock.t < deadline + 60 * 60 * 1000) {
      clock.t += 85_000;
      const r = await hostBeat(env, { players: 2, admitted: [id] });
      assert.equal(r.status, 200);
      if (r.body.ended_guests && endedSeenAt === null) endedSeenAt = clock.t;
      await fireDue(env.GUEST_ALLOWANCE, clock.t);
      await fireDue(env.LIVE_HOSTS, clock.t);
    }
    const storage = (env.LIVE_HOSTS.instances.get(`guest:${SIM}`) ?? env.LIVE_HOSTS.instances.get(SIM)) /* ADJUSTED (fix): the guest-only session object, or the one object before guest-only sessions */.storage;
    const kept = storage.map.get(`gadm:${id}`);
    console.log('ended_guests first reported at +', ((endedSeenAt - T0) / 3600000).toFixed(2), 'h');
    console.log('allowance object for the guest still has data:', env.GUEST_ALLOWANCE.instances.get(gid).storage.map.size > 0);
    console.log('at', ((clock.t - dayStart(T0)) / 3600000).toFixed(2), 'h after the guest day began, LiveHost holds', `gadm:${id}`, '=', JSON.stringify(kept));
    assert.ok(!kept || kept.gid === undefined, 'the guest id is still stored after 25 hours');
  });
});
