// PROBE (security review, guest join): the public mint route reads `body.lease` from `request.json().catch(() => ({}))`.
// A JSON body of `null` (valid JSON) is not caught, so `body.lease` throws and the Worker answers 500 with the JavaScript
// error text instead of a 4xx. Robustness only. FAILS on the current code.

import test from 'node:test';
import assert from 'node:assert/strict';
import { loadWorker, fakeEnv, fakeDoNamespace, ctx } from './helpers/load_worker.mjs';
import { LiveHost } from '../src/live.mjs';
import { GuestAllowance } from '../src/guest.mjs';

const worker = await loadWorker();

test('PROBE: a null JSON body on /api/guest/ticket is a 4xx, not a 500', async () => {
  // ADJUSTED (fix): hosted guest play is off unless GUEST_PLAY is "on" (and the default fakeEnv now binds a limiter).
  const env = { ...fakeEnv(), GUEST_PLAY: 'on' };
  env.LIVE_HOSTS = fakeDoNamespace(LiveHost, env);
  env.GUEST_ALLOWANCE = fakeDoNamespace(GuestAllowance, env);
  const res = await worker.fetch(new Request('https://api.eustress.dev/api/guest/ticket', { method: 'POST', headers: { 'Content-Type': 'application/json', 'CF-Connecting-IP': '203.0.113.7' }, body: 'null' }), env, ctx);
  const body = await res.json().catch(() => null);
  assert.ok(res.status >= 400 && res.status < 500, `status ${res.status}: ${JSON.stringify(body)}`);
});
