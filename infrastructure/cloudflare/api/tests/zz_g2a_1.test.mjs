// PROBE g2a-1 (G-1 / G-2 / G-6): a heartbeat gap longer than 90 s wipes every guest seat (LIVE_OPS.beat: `prev && !continuing`
// -> clearAdmitted, and the alarm does the same), but the host's guests are still connected to it. When the host's beats
// resume and keep naming the guest in `admitted`, the Worker has no seat for that id, so the guest is never beaten, never
// re-checked for eligibility, and never reported in `ended_guests`: not when its lease has ended, not when the listing is
// re-rated adult, and not when GUEST_PLAY is switched off. An unmodified host needs only a Wi-Fi drop, a sleeping laptop or a
// two-minute Worker incident. Expected: an id a guest-only host names that the Worker holds no live seat for is reported
// ended. Each test has a control (a 30 s gap) that passes, so the probe can pass. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, withClock, listing, SIM, T0 } from './zz_guest_b_common.mjs';

async function scenario(gapMs, change) {
  return withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    clock.t += 30_000;
    assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).status, 200);
    clock.t += gapMs; // the host's heartbeat does not get through for this long; its guest stays connected to it
    assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).status, 200);
    await change(env);
    clock.t += 30_000;
    const after = await hostBeat(env, { players: 2, admitted: [id] });
    return { id, status: after.status, ended: after.body?.ended_guests || [] };
  });
}

const reRateAdult = async (env) => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM, { rating: 'adult_18' })));
const killSwitch = async (env) => { env.GUEST_PLAY = 'off'; };

test('PROBE g2a-1a: after a 100 s heartbeat gap, re-rating the listing adult_18 still ends the connected guest', async () => {
  const control = await scenario(30_000, reRateAdult);
  console.log('control (30 s gap): ended_guests =', JSON.stringify(control.ended));
  assert.ok(control.ended.includes(control.id), 'control: with no gap the guest is ended');
  const probe = await scenario(100_000, reRateAdult);
  console.log('probe (100 s gap): status', probe.status, 'ended_guests =', JSON.stringify(probe.ended));
  assert.ok(probe.ended.includes(probe.id), 'a guest still connected to the host is never told to leave an adult_18 listing');
});

test('PROBE g2a-1b: after a 100 s heartbeat gap, turning GUEST_PLAY off still ends the connected guest', async () => {
  const control = await scenario(30_000, killSwitch);
  console.log('control (30 s gap): ended_guests =', JSON.stringify(control.ended));
  assert.ok(control.ended.includes(control.id), 'control: with no gap the kill switch ends the guest');
  const probe = await scenario(100_000, killSwitch);
  console.log('probe (100 s gap): status', probe.status, 'ended_guests =', JSON.stringify(probe.ended));
  assert.ok(probe.ended.includes(probe.id), 'the GUEST_PLAY kill switch does not reach a guest whose seat was wiped');
});

const dropCaps = async () => {};
test('PROBE g2a-1c: after a 100 s heartbeat gap, a beat without the guest protections still ends the connected guest', async () => {
  // The change here is the next beat itself: it declares no guest_caps.
  const run = (gapMs) => withClock(T0, async (clock) => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    clock.t += 30_000;
    assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).status, 200);
    clock.t += gapMs;
    assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).status, 200);
    await dropCaps(env);
    clock.t += 30_000;
    const after = await hostBeat(env, { players: 2, admitted: [id], guest_caps: [] });
    return { id, ended: after.body?.ended_guests || [] };
  });
  const control = await run(30_000);
  console.log('control (30 s gap): ended_guests =', JSON.stringify(control.ended));
  assert.ok(control.ended.includes(control.id), 'control: with no gap, dropping the protections ends the guest');
  const probe = await run(100_000);
  console.log('probe (100 s gap): ended_guests =', JSON.stringify(probe.ended));
  assert.ok(probe.ended.includes(probe.id), 'a host that stops declaring the protections keeps a guest whose seat was wiped');
});
