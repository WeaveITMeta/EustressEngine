// Probe G2B-4: a guest id can outlive its 25 hours. GuestAllowance.load() creates a record (and a new alarm 25 hours after
// TODAY began) whenever none exists, and beat/end/bar call it with the guest id the lease names. A lease ticket lives 62
// minutes, so a lease opened in the last two minutes of a UTC day is still valid after its guest id's alarm (day start +
// 25 h) has deleted the record. One beat in that window re-creates the day-old guest id's object with an alarm another
// 24 hours out: the id is kept up to 49 hours after its day began. GUEST_PLAY.md section 16 says 25 hours or less.
// Expected (fails today): after the guest id's alarm has run, nothing recreates a record under that guest id.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, openLease, call, payloadOf, withClock } from './zz_g2b_common.mjs';

test('G2B-4: a beat after the allowance alarm recreates the day-old guest id with a fresh 25-hour alarm', async () => {
  const D = Date.UTC(2026, 9, 16);
  await withClock(D + 24 * 3600_000 - 60_000, async (clock) => { // 23:59:00 UTC
    const env = environment();
    await seed(env);
    const lease = (await openLease(env)).body.ticket;
    const gid = payloadOf(lease).g;
    const obj = env.GUEST_ALLOWANCE.instances.get(gid);
    assert.equal(obj.storage.alarm, D + 25 * 3600_000, 'alarm at day start + 25 h');
    // The alarm runs at 01:00 UTC the next day and deletes everything.
    clock.t = D + 25 * 3600_000;
    await obj.alarm();
    await obj.storage.deleteAlarm();
    assert.equal(obj.storage.map.size, 0, 'deleted on time');
    // 30 s later the lease ticket (62 minutes) is still genuine, so a beat reaches the same guest id.
    clock.t += 30_000;
    const r = await call(env, 'POST', '/api/guest/beat', { body: { ticket: lease } });
    console.log(`beat at ${new Date(clock.t).toISOString()}: ${r.status} ${r.body?.code}; record keys now: ${[...obj.storage.map.keys()]}; alarm: ${obj.storage.alarm && new Date(obj.storage.alarm).toISOString()}`);
    assert.equal(obj.storage.map.size, 0, `the ${gid.slice(0, 8)}... guest id was re-created after its 25 hours`);
    assert.ok(!(obj.storage.alarm > D + 25 * 3600_000), 'and given an alarm past day start + 25 h');
  });
});

test('G2B-4 control: a lease opened at noon cannot reach its guest id after the alarm (passes)', async () => {
  const D = Date.UTC(2026, 9, 16);
  await withClock(D + 12 * 3600_000, async (clock) => {
    const env = environment();
    await seed(env);
    const lease = (await openLease(env)).body.ticket;
    const obj = env.GUEST_ALLOWANCE.instances.get(payloadOf(lease).g);
    clock.t = D + 25 * 3600_000;
    await obj.alarm();
    await obj.storage.deleteAlarm();
    clock.t += 30_000;
    const r = await call(env, 'POST', '/api/guest/beat', { body: { ticket: lease } });
    assert.equal(r.body.code, 'bad_ticket');
    assert.equal(obj.storage.map.size, 0);
  });
});
