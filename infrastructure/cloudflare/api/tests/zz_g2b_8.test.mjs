// Probe G2B-8: what one guest chat check writes, against GUEST_PLAY.md section 16 ("Chat check record | console log and
// review-cost ledger | none | Workers Logs retention; the cost ledger keeps token counts under the family forum, no text and
// no ids"). Each check that reaches Jev writes, in KV SOCIAL: `modcost:run:<day>:<uuid>` with a 400-day TTL (one record per
// chat line), and rewrites `modcost:forum` (no TTL) holding the last 20 checks with their start and finish times to the
// millisecond, mixed with nothing else (the account forum path meters under `forum:<account>`). No text and no guest id: the
// table is incomplete on retention and on the timestamps, not wrong on content.
// Expected (fails today): nothing a guest chat check writes is kept past 25 hours except aggregate counts.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, guestBeat, seat, chat, withClock, withFetch, jevAnswering, quietly } from './zz_g2b_common.mjs';

test('G2B-8: one guest chat line leaves a 400-day KV record and a timestamped ledger entry', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async () => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const writes = [];
    const put = env.SOCIAL.put.bind(env.SOCIAL);
    env.SOCIAL.put = async (key, value, opts) => { writes.push({ key, ttl: opts?.expirationTtl ?? null, value: String(value) }); return put(key, value, opts); };
    await quietly(() => withFetch(jevAnswering(), () => chat(env, id, 'nice jump')));
    for (const w of writes) console.log(`${w.key}  ttl=${w.ttl}  ${w.value.slice(0, 160)}`);
    const kept = writes.filter((w) => w.ttl === null || w.ttl > 25 * 3600);
    assert.deepEqual(kept.map((w) => w.key), [], 'kept past 25 hours');
  });
});
