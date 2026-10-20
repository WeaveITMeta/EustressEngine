// Probe G2B-3: the "barred for the day" after a refused chat line is written into the allowance of the guest id the LEASE was
// opened under (the seat's gid). A lease is never cut at UTC midnight, so a guest who opened its lease before midnight and is
// refused after it has the bar written to yesterday's record; today's guest id (a new hash of the same address) has no bar,
// and the same address opens a new lease on the same listing a minute later.
// Expected (fails today): after a refusal, the same address cannot open a lease on that listing again that day.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, guestBeat, seat, chat, openLease, withClock, withFetch, jevAnswering, quietly } from './zz_g2b_common.mjs';

test('G2B-3: a refusal after UTC midnight, on a lease opened before it, bars nothing today', async () => {
  const LATE = Date.UTC(2026, 9, 16, 23, 50, 0);
  await withClock(LATE, async (clock) => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    // The host keeps the guest connected across midnight (30 s beats, naming the seat).
    while (clock.t < Date.UTC(2026, 9, 17, 0, 10, 0)) {
      clock.t += 30_000;
      const b = await guestBeat(env, { admitted: [id] });
      assert.equal(b.body.ended_guests, undefined, `still connected at ${new Date(clock.t).toISOString()}`);
    }
    const { out: r } = await quietly(() => withFetch(jevAnswering(), () => chat(env, id, 'call me at 555 201 3344')));
    assert.deepEqual([r.status, r.body.deliver, r.body.end_guest], [200, false, true], 'refused and kicked at 00:10 UTC');
    clock.t += 60_000;
    const again = await openLease(env);
    console.log(`00:11 UTC, same address, same listing: ${again.status} ${JSON.stringify(again.body?.code ?? null)}`);
    assert.deepEqual([again.status, again.body.code], [403, 'guest_barred'], 'the refused guest is back on the same listing one minute later');
  });
});
