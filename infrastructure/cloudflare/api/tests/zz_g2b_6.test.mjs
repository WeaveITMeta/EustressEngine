// Probe G2B-6: a chat line of nothing but invisible format characters (U+200B, U+2060, U+00AD) passes the route's own
// emptiness test (String.prototype.trim keeps them), is cleaned to '' by checkCommunityText, and comes back as a REFUSAL
// (category 'other', reason 'empty'). The route treats every refusal as a moderation refusal: the guest is kicked and the
// whole address is barred from the listing for the day, for a line with no content at all.
// Expected (fails today): an empty line is not delivered, and nobody is kicked for it.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, guestBeat, seat, chat, withClock, withFetch, jevAnswering, quietly } from './zz_g2b_common.mjs';

test('G2B-6: an invisible-only chat line kicks and bars the guest', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async () => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const jev = jevAnswering();
    const { out: r } = await quietly(() => withFetch(jev, () => chat(env, id, '​⁠­')));
    console.log(`invisible line: ${r.status} ${JSON.stringify(r.body)}; Jev calls: ${jev.calls.length}`);
    assert.notEqual(r.body.end_guest, true, 'a guest was kicked and barred for an empty line');
  });
});
