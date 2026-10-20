// PROBE g2a-6 (G-2, verification, expected to PASS): every eligibility input the heartbeat must act on, one at a time, against a
// connected guest that the host keeps naming. ages_10 keeps the guest; every other change ends it on the next heartbeat.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, withClock, listing, SIM, T0 } from './zz_guest_b_common.mjs';

const put = (env, sim) => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(sim));
const CASES = [
  ['ages_10 (stays)', false, (env) => put(env, listing(SIM, { rating: 'ages_10' }))],
  ['teen_13', true, (env) => put(env, listing(SIM, { rating: 'teen_13' }))],
  ['unrated', true, (env) => put(env, { ...listing(SIM), moderation: { status: 'approved' } })],
  ['adult_18', true, (env) => put(env, listing(SIM, { rating: 'adult_18' }))],
  ['quarantined', true, (env) => put(env, listing(SIM, { status: 'quarantined' }))],
  ['pending', true, (env) => put(env, listing(SIM, { status: 'pending' }))],
  ['private', true, (env) => put(env, { ...listing(SIM), is_public: false })],
  ['archived', true, (env) => put(env, { ...listing(SIM), is_public: false, archived: true, archived_was_public: true })],
  ['legal lock serve_block (TAKE IT DOWN, legal hold)', true, (env) => env.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } })],
  ['legal lock content_lock', true, (env) => env.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'content_lock' } })],
  ['approval binding revoked', true, (env) => env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ revoked: true }))],
];

test('PROBE g2a-6: each eligibility input ends (or keeps) a connected guest on the next heartbeat', async () => {
  const seen = [];
  for (const [name, ends, change] of CASES) {
    await withClock(T0, async (clock) => {
      const env = environment();
      const lease = await hostedWithGuest(env);
      const { ticket, id } = await guestTicket(env, lease);
      assert.equal((await admit(env, ticket)).status, 200);
      clock.t += 30_000;
      assert.equal((await hostBeat(env, { players: 2, admitted: [id] })).body.ended_guests, undefined, name);
      await change(env);
      clock.t += 30_000;
      const r = await hostBeat(env, { players: 2, admitted: [id] });
      const ended = (r.body.ended_guests || []).includes(id);
      seen.push(`${name}: ${ended ? 'ended' : 'kept'}`);
      assert.equal(ended, ends, name);
    });
  }
  console.log(seen.join(' | '));
});
