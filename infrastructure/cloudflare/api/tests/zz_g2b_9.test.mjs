// Probe G2B-9: "never both in one session" is not checked by the Worker. One host process (one certificate, one pin, one
// address) may beat BOTH the account session and the guest-only session of a listing with the same link; the Worker then
// admits an account ticket (through the account object) and a guest ticket (through the guest object) into the one server.
// Nothing compares the two objects' pins. A modified host can mix them anyway (it can skip /live/admit); this is about an
// honest host's bug failing safe.
// Expected (fails today): a guest-only heartbeat at the pin the account session holds (or the reverse) is refused, so one
// server never admits both kinds.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, guestBeat, accountBeat, openLease, mint, admit, call, player, withClock, PIN, SIM } from './zz_g2b_common.mjs';

test('G2B-9: one pin admits an account and a guest when the host beats both sessions with the same link', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async () => {
    const env = environment();
    await seed(env);
    const a = await accountBeat(env, {}, SIM, PIN);
    const g = await guestBeat(env, {}, SIM, PIN);
    console.log(`account beat at PIN: ${a.status}; guest-only beat at the same PIN: ${g.status}`);
    const accountTicket = (await call(env, 'POST', '/api/identity/ticket', { token: player, body: { audience: PIN } })).body.ticket;
    const acc = await admit(env, accountTicket, { audience: PIN, guestOnly: false });
    const lease = (await openLease(env)).body.ticket;
    const guest = await admit(env, (await mint(env, lease, PIN)).body.ticket, { audience: PIN });
    console.log(`account admitted at PIN: ${acc.status} guest=${acc.body?.guest}; guest admitted at PIN: ${guest.status} guest=${guest.body?.guest}`);
    assert.ok(!(acc.status === 200 && guest.status === 200), 'one server (one pin) admitted both an account and a guest');
  });
});
