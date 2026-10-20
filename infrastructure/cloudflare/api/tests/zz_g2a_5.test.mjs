// PROBE g2a-5 (G-1, the setting's reach): with GUEST_PLAY "off", a guest-only heartbeat is still taken (by design, to end
// guests), and a solo lease still opens. GET /live and POST /play with X-Eustress-Lease then answer about the guest-only
// session and hand the lease holder its join link (the host's address, port, key and pin), though nobody may join it. The
// setting gates the ticket and the admission, not this read. Expected: with hosted guest play off, a lease is shown no
// guest-only session link. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, call, hostBeat, openLease, liveOf, withClock, SIM, T0 } from './zz_guest_b_common.mjs';

test('PROBE g2a-5: with GUEST_PLAY off, a lease is not handed the guest-only session\'s join link', async () => {
  await withClock(T0, async () => {
    const env = environment({ GUEST_PLAY: 'off' });
    await seed(env);
    assert.equal((await hostBeat(env)).status, 200);
    const lease = (await openLease(env)).body.ticket;
    const live = await call(env, 'GET', liveOf(SIM), { headers: { 'X-Eustress-Lease': lease } });
    const play = await call(env, 'POST', `/api/simulations/${SIM}/play`, { headers: { 'X-Eustress-Lease': lease } });
    console.log('GET /live ->', live.status, JSON.stringify(live.body));
    console.log('POST /play ->', play.status, play.body?.status, play.body?.launch?.link);
    assert.equal(live.body?.link, undefined, 'GET /live handed a guest-only link while hosted guest play is off');
    assert.equal(play.body?.status, 'solo', 'POST /play handed a guest-only link while hosted guest play is off');
  });
});
