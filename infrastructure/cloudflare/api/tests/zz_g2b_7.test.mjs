// Probe G2B-7: "personal information is filtered out of chat before delivery" (GUEST_PLAY.md section 16) holds only for phone
// numbers and email addresses. The guest chat check uses the forum_post field, where checkCommunityText's deterministic rules
// refuse a phone number or an email address and nothing else: a contact handle, an invite link or a link (NAME_LINK) is refused
// only for display_name and forum_title. Everything else is left to Jev's forum personal_info question, which asks about
// DOXXING ("someone's private information ... so they can be found or reached against their will") and says a member
// sharing their own details is not it. A child giving out its own handle, invite, school or street is that case.
// With Jev answering every category low (a model reading its question as written), every line below is delivered.
// Expected (fails today): a guest's own contact handle or invite link is never delivered, whatever Jev answers.
import test from 'node:test';
import assert from 'node:assert/strict';
import { buildForumQuestions } from '../src/moderation.mjs';
import { environment, seed, guestBeat, seat, chat, withClock, withFetch, jevAnswering, quietly } from './zz_g2b_common.mjs';

const LINES = [
  'add me on discord kidgamer#1234',
  'my snap is @emma_2015',
  'join my server discord.gg/abc123',
  'im emma i go to lincoln elementary in austin',
  'i live at 42 maple street springfield',
  'whats ur insta? mine is emma.k.2015',
];

test('G2B-7: a guest\'s own handle, invite, school and street reach other guests unless Jev refuses them', async () => {
  const q = buildForumQuestions().personal_info.instructions;
  assert.ok(/against their will/.test(q) && /sharing their own public portfolio are not this/.test(q), 'the battery asks about doxxing');
  await withClock(Date.UTC(2026, 9, 16, 12, 0, 0), async (clock) => {
    const env = environment({ JEV_API_KEY: 'jev-test' });
    await seed(env);
    assert.equal((await guestBeat(env)).status, 200);
    const { id } = await seat(env);
    const jev = jevAnswering();
    const delivered = [];
    await quietly(() => withFetch(jev, async () => {
      for (const line of LINES) {
        clock.t += 1000;
        const r = await chat(env, id, line);
        if (r.body?.deliver === true) delivered.push(line);
      }
    }));
    console.log(`delivered with Jev answering low (${jev.calls.length} Jev calls):\n  ${delivered.join('\n  ')}`);
    const contact = delivered.filter((l) => /@|#\d{4}|discord\.gg/.test(l));
    assert.deepEqual(contact, [], 'contact handles and invite links pass the Worker\'s own rules in guest chat');
  });
});
