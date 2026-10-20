// PROBE g2a-7 (G-1 / G-2, the end signal): every way the Worker ends a guest (GUEST_PLAY off, protections not declared, listing
// not eligible, lease over) reaches the host only in the 200 answer of a heartbeat. A guest-only heartbeat the Worker refuses
// carries no instruction at all: the author's session revoked or banned (verifyAuth / freshGate: 401), the listing record gone
// (404), a 5xx. GUEST_PLAY.md section 13 tells the host to honour `ended_guests` but says nothing about a refused or failed beat,
// so a host built to that spec keeps its guests. Expected: a refused guest-only heartbeat tells the host to let every guest go
// (or the host spec requires that on any non-200; then this probe documents the Worker side only). FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, withClock, tokenOf, SIM, PIN, AUTHOR, T0 } from './zz_guest_b_common.mjs';

async function connectedGuest(clock) {
  const env = environment();
  const lease = await hostedWithGuest(env);
  const { ticket, id } = await guestTicket(env, lease);
  assert.equal((await admit(env, ticket)).status, 200);
  clock.t += 30_000;
  assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).status, 200);
  return { env, id };
}

test('PROBE g2a-7: a refused guest-only heartbeat still tells the host to let its guests go', async () => {
  await withClock(T0, async (clock) => {
    const revoked = await connectedGuest(clock);
    const held = tokenOf(AUTHOR); // the token the host already holds
    // A ban or a sign-out moves the account's floor past every token it holds (index.js verifyAuth).
    await revoked.env.USERS.put(`authfloor:${AUTHOR}`, String(Math.floor(clock.t / 1000) + 1));
    clock.t += 30_000;
    const a = await hostBeat(revoked.env, { players: 2, admitted: [revoked.id] }, SIM, PIN, held);
    console.log('author session revoked: heartbeat ->', a.status, JSON.stringify(a.body));

    const gone = await connectedGuest(clock);
    await gone.env.SOCIAL.delete(`sim:${SIM}`);
    clock.t += 30_000;
    const b = await hostBeat(gone.env, { players: 2, admitted: [gone.id] });
    console.log('listing record gone: heartbeat ->', b.status, JSON.stringify(b.body));

    const told = (r, id) => r.body?.end_all_guests === true || (r.body?.ended_guests || []).includes(id);
    assert.ok(told(a, revoked.id) && told(b, gone.id), 'a refused guest-only heartbeat gives the host no instruction to drop its guests');
  });
});
