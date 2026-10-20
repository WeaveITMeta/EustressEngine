// PROBE g2a-2 (G-1, guest-only sessions): the account session (object `<sim>`) and the guest-only session (object
// `guest:<sim>`) of one listing may name the SAME join link, so the same certificate pin, which is the same host process.
// The Worker then admits an account to it through the account object and a guest to it through the guest object: one
// physical session holds signed-in adults and guests. The pin is the only thing that identifies the session the Player
// connects to, and the Worker holds both pins, so it can refuse this. Expected: when both sessions of a listing are live at one
// pin, the Worker never admits both an account and a guest to it. FAILS on the current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, call, tokenOf, liveOf, linkFor, hostBeat, openLease, mint, admit, withClock, AUTHOR, PLAYER, SIM, PIN, PIN2, T0 } from './zz_guest_b_common.mjs';

async function run(accountPin) {
  return withClock(T0, async () => {
    const env = environment();
    await seed(env);
    // The account session's heartbeat (no guest_only) and the guest-only session's heartbeat (guest_only, guest_caps).
    const acctBeat = await call(env, 'POST', liveOf(SIM), { token: tokenOf(AUTHOR), body: { link: linkFor(accountPin), players: 1, max_players: 8, protocol: 3 } });
    assert.equal(acctBeat.status, 200);
    // ADJUSTED (fix): round 3 (R-1) refuses the guest-only heartbeat at the pin the account session holds (409 pin_in_use).
    const guestOnlyBeat = await hostBeat(env); // guest-only at PIN
    assert.ok(guestOnlyBeat.status === 200 || (accountPin === PIN && guestOnlyBeat.status === 409 && guestOnlyBeat.body?.code === 'pin_in_use'), JSON.stringify(guestOnlyBeat.body));
    const identity = (await call(env, 'POST', '/api/identity/ticket', { token: tokenOf(PLAYER), body: { audience: accountPin } })).body.ticket;
    const account = await call(env, 'POST', `${liveOf(SIM)}/admit`, { token: tokenOf(AUTHOR), body: { ticket: identity, audience: accountPin } });
    const lease = (await openLease(env)).body.ticket;
    const minted = await mint(env, lease, PIN);
    const guest = minted.status === 200 ? await admit(env, minted.body.ticket, { audience: PIN }) : minted;
    return { account: [account.status, account.body?.guest], guest: [guest.status, guest.body?.guest] };
  });
}

test('PROBE g2a-2: an account and a guest are never both admitted to the host at one certificate pin', async () => {
  const control = await run(PIN2);
  console.log('control (account session at another pin): account', JSON.stringify(control.account), 'guest', JSON.stringify(control.guest));
  const probe = await run(PIN);
  console.log('probe (both sessions at PIN):          account', JSON.stringify(probe.account), 'guest', JSON.stringify(probe.guest));
  assert.ok(!(probe.account[0] === 200 && probe.guest[0] === 200), 'an account and a guest were both admitted to the same host session (one pin)');
});
