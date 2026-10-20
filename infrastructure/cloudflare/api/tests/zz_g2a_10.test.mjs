// PROBE g2a-10 (G-4, verification, expected to PASS): /live/admit re-checks public and listable for a guest ticket minted while
// the listing was public: private, archived, a legal lock and a revoked binding each refuse the guest, and the ticket is unspent.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, withClock, listing, SIM, T0 } from './zz_guest_b_common.mjs';

test('PROBE g2a-10: admission re-checks public/listable for a guest', async () => {
  await withClock(T0, async () => {
    const env = environment();
    const lease = await hostedWithGuest(env);
    const { ticket } = await guestTicket(env, lease);
    const seen = [];
    const cases = [
      ['private', () => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...listing(SIM), is_public: false })), () => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)))],
      ['archived', () => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify({ ...listing(SIM), is_public: false, archived: true })), () => env.SOCIAL.put(`sim:${SIM}`, JSON.stringify(listing(SIM)))],
      ['legal lock', () => env.SCENES.put(`locks/legal/${SIM}`, '{}', { customMetadata: { kind: 'serve_block' } }), () => env.SCENES.delete(`locks/legal/${SIM}`)],
      ['binding revoked', () => env.SCENES.put(`locks/approved/${SIM}.json`, JSON.stringify({ revoked: true })), () => env.SCENES.delete(`locks/approved/${SIM}.json`)],
    ];
    for (const [name, apply, undo] of cases) {
      await apply();
      const r = await admit(env, ticket);
      seen.push(`${name}: ${r.status} ${r.body.code}`);
      assert.equal(r.status, 403, name);
      await undo();
    }
    const ok = await admit(env, ticket);
    seen.push(`restored: ${ok.status}`);
    console.log(seen.join(' | '));
    assert.equal(ok.status, 200, 'the refusals spent nothing');
  });
});
