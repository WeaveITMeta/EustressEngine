// Probe G2B-5: the featured shelf (GET /api/gallery/featured) reads no session, so its cap is viewerCap(undefined), which
// round 2 changed from mature_17 to ages_10. A featured teen_13 or mature_17 listing has left the shelf for EVERY viewer,
// signed-in adults included (before round 2 they saw it). Not a leak: a behaviour change for signed-in accounts that no
// document names. Expected (fails today): a signed-in account still sees a featured teen_13 listing on the shelf.
import test from 'node:test';
import assert from 'node:assert/strict';
import { environment, listing, call, player, PLAYER, AUTHOR } from './zz_g2b_common.mjs';

test('G2B-5: a featured teen_13 listing is gone from the featured shelf for a signed-in account', async () => {
  const env = environment();
  await env.USERS.put(`user:${AUTHOR}`, JSON.stringify({ kyc_verified: true, id: AUTHOR, username: 'host' }));
  await env.USERS.put(`user:${PLAYER}`, JSON.stringify({ kyc_verified: true, id: PLAYER, username: 'ada' }));
  const TEEN = 'cccccccc-3333-4333-8333-cccccccccccc';
  const KIDS = 'dddddddd-4444-4444-8444-dddddddddddd';
  await env.SOCIAL.put(`sim:${TEEN}`, JSON.stringify(listing(TEEN, { rating: 'teen_13', featured: true }, { published_at: '2026-10-01T00:00:00.000Z' })));
  await env.SOCIAL.put(`sim:${KIDS}`, JSON.stringify(listing(KIDS, { rating: 'all_ages', featured: true }, { published_at: '2026-10-02T00:00:00.000Z' })));
  const signedIn = (await call(env, 'GET', '/api/gallery/featured', { token: player })).body.featured.map((s) => s.id);
  const gallery = (await call(env, 'GET', '/api/simulations', { token: player })).body.simulations.map((s) => s.id);
  console.log(`signed in: featured ${JSON.stringify(signedIn)}; gallery ${JSON.stringify(gallery)}`);
  assert.ok(gallery.includes(TEEN), 'the signed-in Gallery shows it');
  assert.ok(signedIn.includes(TEEN), 'the featured shelf drops a teen_13 listing for a signed-in account');
});
