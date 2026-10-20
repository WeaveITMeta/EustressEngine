// PROBE g2a-3 (G-6, retention): the GuestAllowance object deletes itself at day start + 25 h (its alarm). Every op but status
// and holds goes through load(), which writes a fresh record and a fresh alarm (the NEXT day's start + 25 h) when it finds
// none. A lease ticket lives 62 minutes, so one opened at 23:59 UTC is still valid at 01:00:30 the next day; a beat or an end
// with it then (or the chat check's `bar`, between gx and the next heartbeat) writes a new record under the previous day's guest
// id, kept until 01:00 the day after: about 48 hours after the guest id's own day began. Expected: no record named by a
// guest id exists after its day start + 25 h. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, call, openLease, withClock } from './zz_guest_b_common.mjs';
import { dayStart, RETENTION_MS } from '../src/guest.mjs';

test('PROBE g2a-3: a beat on yesterday\'s lease after its 25 hours does not write a new record under yesterday\'s guest id', async () => {
  const late = Date.UTC(2026, 9, 16, 23, 59, 0);
  await withClock(late, async (clock) => {
    const env = environment();
    await seed(env);
    const lease = await openLease(env);
    assert.equal(lease.status, 200, JSON.stringify(lease.body));
    const [gid] = [...env.GUEST_ALLOWANCE.instances.keys()];
    const obj = env.GUEST_ALLOWANCE.instances.get(gid);
    const gx = dayStart(late) + RETENTION_MS;
    // The allowance's own alarm fires at gx and deletes everything (control).
    clock.t = gx + 30_000;
    assert.ok(obj.storage.alarm <= clock.t);
    await obj.storage.deleteAlarm();
    await obj.alarm();
    assert.equal(obj.storage.map.size, 0, 'control: the alarm deleted the record');
    // The guest's own Player beats with the lease ticket it still holds (valid until 01:01:00).
    const r = await call(env, 'POST', '/api/guest/beat', { body: { ticket: lease.body.ticket } });
    console.log('beat at', new Date(clock.t).toISOString(), '->', r.status, r.body?.code,
      '| record under yesterday\'s guest id:', JSON.stringify(obj.storage.map.get('a')),
      '| its new alarm:', obj.storage.alarm ? new Date(obj.storage.alarm).toISOString() : null);
    assert.equal(obj.storage.map.size, 0, 'a record named by yesterday\'s guest id was written after its 25 hours');
  });
});
