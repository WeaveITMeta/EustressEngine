// PROBE g2a-8 (G-3 / G-6, retention of the waiting ticket): `gtk:<j> = {gid, tid, gx, e}` is dropped only once its own ticket
// expiry `e` (mint + 10 min) has passed, never at `gx` (the guest id's day start + 25 h). A lease opened just before midnight UTC
// is still live just before 01:00 the next day, so a ticket minted then holds the previous day's guest id in the session object
// up to 10 minutes (plus one heartbeat) past its 25 hours. Expected: no gtk entry holds a guest id past its gx. FAILS on the
// current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, call, hostBeat, openLease, mint, withClock, SIM } from './zz_guest_b_common.mjs';
import { dayStart, RETENTION_MS } from '../src/guest.mjs';

test('PROBE g2a-8: a waiting guest ticket does not hold a guest id past its 25 hours', async () => {
  const start = Date.UTC(2026, 9, 16, 23, 59, 59);
  await withClock(start, async (clock) => {
    const env = environment();
    await seed(env);
    assert.equal((await hostBeat(env)).status, 200);
    const lease = (await openLease(env)).body.ticket;
    const gx = dayStart(start) + RETENTION_MS;
    // The guest plays solo and beats; the host stays up.
    while (clock.t + 30_000 < gx - 20_000) {
      clock.t += 30_000;
      await call(env, 'POST', '/api/guest/beat', { body: { ticket: lease } });
      await hostBeat(env);
    }
    const mintedAt = clock.t;
    const minted = await mint(env, lease);
    assert.equal(minted.status, 200, `mint at ${new Date(clock.t).toISOString()}: ${JSON.stringify(minted.body)}`);
    clock.t = gx + 30_000;
    await hostBeat(env); // the session's housekeeping runs on every heartbeat
    const storage = env.LIVE_HOSTS.instances.get(`guest:${SIM}`).storage;
    const held = [...storage.map].filter(([k, v]) => k.startsWith('gtk:') && typeof v?.gid === 'string');
    console.log('minted at', new Date(mintedAt).toISOString(),
      '| gx', new Date(gx).toISOString(), '| at', new Date(clock.t).toISOString(), 'gtk entries holding a gid:', held.length,
      held.length ? `(e = ${new Date(held[0][1].e * 1000).toISOString()})` : '');
    assert.equal(held.length, 0, 'a waiting ticket holds a guest id past its 25 hours');
  });
});
