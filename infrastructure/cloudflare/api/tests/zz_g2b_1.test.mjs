// Probe G2B-1: the guest chat check has no daily cost cap. checkCommunityText is called without accountId, so the per-account
// forum cap (forum_checks_per_account_per_day, forumCheckKey) never applies, and the only limit is 20 lines a minute per SEAT.
// One host account, one listing, three seats from three addresses: 60 paid Jev calls in one minute with the cap set to 5.
// Expected (fails today): the Jev calls one host account causes in a day stay within a daily cap.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, seed, guestBeat, seat, chat, ipN, withClock, withFetch, jevAnswering, quietly, windowLimiter, AUTHOR } from './zz_g2b_common.mjs';

test('G2B-1: guest chat checks reach Jev with no per-host, per-listing or daily cap', async () => {
  const T = Date.UTC(2026, 9, 16, 12, 0, 0);
  await withClock(T, async (clock) => {
    const limiter = windowLimiter(20);
    // ADJUSTED (fix): round 3 (B-4) caps guest chat checks per host account per day with GUEST_CHAT_CAP_HOST (the forum's
    // per-account cap stays the forum's own), so the cap of 5 is set there.
    const env = environment({ JEV_API_KEY: 'jev-test', GUEST_CHAT_RATE_LIMITER: limiter, GUEST_CHAT_CAP_HOST: '5', MODERATION_THRESHOLDS: JSON.stringify({ forum_checks_per_account_per_day: 5 }) });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const ids = [];
    for (const n of [1, 2, 3]) ids.push((await seat(env, ipN(n))).id);
    const jev = jevAnswering();
    let delivered = 0;
    let limited = 0;
    await quietly(() => withFetch(jev, async () => {
      for (const id of ids) {
        for (let i = 0; i < 21; i++) {
          clock.t += 10;
          const r = await chat(env, id, `hello number ${i}`);
          if (r.status === 200 && r.body.deliver === true) delivered++;
          if (r.status === 429) limited++;
        }
      }
    }));
    console.log(`Jev calls: ${jev.calls.length}; delivered: ${delivered}; per-seat 429s: ${limited}; forumcheck keys: ${[...env.SOCIAL.store.keys()].filter((k) => k.startsWith('forumcheck:')).length}`);
    assert.equal(limited, 3, 'the per-seat limiter works (the 21st line of each seat)');
    assert.ok(jev.calls.length <= 5, `one host account caused ${jev.calls.length} paid Jev calls in one minute; forum_checks_per_account_per_day is 5 and is never consulted (no forumcheck:${AUTHOR} key)`);
  });
});

test('G2B-1 control: five lines from one seat stay within the cap the assertion reads (passes)', async () => {
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async (clock) => {
    const env = environment({ JEV_API_KEY: 'jev-test', GUEST_CHAT_RATE_LIMITER: windowLimiter(20) });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env, ipN(1));
    const jev = jevAnswering();
    await quietly(() => withFetch(jev, async () => { for (let i = 0; i < 5; i++) { clock.t += 10; await chat(env, id, `hi ${i}`); } }));
    assert.ok(jev.calls.length <= 5);
  });
});
