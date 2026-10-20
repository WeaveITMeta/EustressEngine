// PROBE 9: hosted guest time is under-charged by the host's beat cadence. Each beat debits at most 45 s (1.5 x 30 s) and the
// lease stays alive for 90 s without a beat, so a host beating every 85 s (still "continuing", which allows 90 s) charges 45 s
// per 85 s of play: ~1.9 x the allowance. The hosted lease also runs past its own ticket expiry (62 min). Funnel limit only.
// Expected: charged time tracks wall time within one beat. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, call, withClock, IP, T0 } from './zz_guest_b_common.mjs';
import { DAILY_SECONDS } from '../src/guest.mjs';

test('PROBE 9: a slow host beat does not stretch a guest\'s allowance', async () => {
  await withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    for (let i = 0; i < 20; i++) {
      clock.t += 85_000;
      assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).status, 200);
    }
    const charged = DAILY_SECONDS - (await call(env, 'GET', '/api/guest/allowance', { headers: IP })).body.seconds_left;
    console.log('played', (clock.t - T0) / 1000, 's hosted, charged', charged, 's');
    assert.ok(charged >= (clock.t - T0) / 1000 - 90, 'charged far less than the time played');
  });
});
