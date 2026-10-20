// PROBE g2a-9 (chat refusal, fail-open bar): after a refused line, guest.mjs calls the allowance's `bar` and ignores its answer
// (`allowanceCall(...).catch(...)`; an error reply is `{ok:false}`, not a throw). When that call fails, the guest's day bar on
// the listing is never written and its lease is not ended in the allowance. The session's own `gbar:<tid>` bars only that one
// lease, so the same address opens a new lease, mints, and is admitted to the same guest-only session again. Expected: a
// refusal whose bar could not be recorded still keeps the guest out of that listing for the day (fail closed). FAILS on the
// current code.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, hostedWithGuest, guestTicket, admit, hostBeat, openLease, mint, call, tokenOf, withClock, AUTHOR, SIM, T0 } from './zz_guest_b_common.mjs';

test('PROBE g2a-9: a refused guest whose allowance bar failed cannot rejoin the listing that day', async () => {
  await withClock(T0, async (clock) => {
    const env = environment({ GUEST_CHAT_RATE_LIMITER: { limit: async () => ({ success: true }) } });
    const lease = await hostedWithGuest(env);
    const { ticket, id } = await guestTicket(env, lease);
    assert.equal((await admit(env, ticket)).status, 200);
    // The allowance answers an error to `bar` only (a Durable Object that is overloaded or resetting).
    const ns = env.GUEST_ALLOWANCE;
    const realGet = ns.get.bind(ns);
    ns.get = (oid) => {
      const stub = realGet(oid);
      return { fetch: (url, init) => (String(url).endsWith('/bar') ? Promise.resolve(new Response(JSON.stringify({ ok: false, status: 500, error: 'overloaded' }), { status: 500 })) : stub.fetch(url, init)) };
    };
    // Personal information is refused before Jev is asked.
    const refused = await call(env, 'POST', '/api/guest/chat/check', { token: tokenOf(AUTHOR), body: { sim_id: SIM, id, text: 'call me at 555 201 3344' } });
    assert.deepEqual([refused.status, refused.body.deliver, refused.body.end_guest], [200, false, true]);
    clock.t += 1_000;
    const again = await openLease(env);
    const minted = again.status === 200 ? await mint(env, again.body.ticket) : again;
    const back = minted.status === 200 ? await admit(env, minted.body.ticket) : minted;
    console.log('after a refusal with a failed bar: new lease', again.status, again.body?.code ?? '', '| mint', minted.status, '| admit', back.status, back.body?.guest ?? back.body?.code);
    assert.notEqual(back.status, 200, 'the refused guest was admitted to the same listing again the same day');
  });
});
