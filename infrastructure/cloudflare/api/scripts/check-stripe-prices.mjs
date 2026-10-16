// Checks the Stripe prices Delta and the subscription plans need, against the
// table in docs/monetization/DELTA_STRIPE_SETUP.md. It reads JSON on stdin (the
// output of `stripe prices list ...`) and calls no network. Exit 0 when every
// price is right, 1 with a line per problem otherwise.
//
//   stripe prices list --limit 100 | node scripts/check-stripe-prices.mjs

import { fileURLToPath } from 'node:url';

const SUBSCRIPTION = (key, cents, interval, plan, grants) => ({
  lookup_key: key, unit_amount: cents, currency: 'usd', recurring: interval,
  metadata: { eustress_plan: plan, ...grants },
});
const PACK = (key, cents, delta) => ({
  lookup_key: key, unit_amount: cents, currency: 'usd', recurring: null,
  metadata: { type: 'delta_purchase', delta: String(delta) },
});

/// What each price must be. Grants are in metadata so the amounts can be tuned
/// in Stripe without code.
export const EXPECTED = [
  SUBSCRIPTION('eustress_player_plus_monthly', 499, 'month', 'player_plus', { tickets_per_invoice: '450' }),
  SUBSCRIPTION('eustress_player_plus_yearly', 4999, 'year', 'player_plus', { tickets_per_invoice: '5400' }),
  SUBSCRIPTION('eustress_creator_monthly', 999, 'month', 'creator', { delta_per_invoice: '1000' }),
  SUBSCRIPTION('eustress_creator_yearly', 9999, 'year', 'creator', { delta_per_invoice: '12000' }),
  SUBSCRIPTION('eustress_professional_monthly', 12900, 'month', 'professional', { delta_per_invoice: '16000' }),
  SUBSCRIPTION('eustress_professional_yearly', 129000, 'year', 'professional', { delta_per_invoice: '192000' }),
  SUBSCRIPTION('eustress_bundle_monthly', 1299, 'month', 'bundle', { tickets_per_invoice: '400', delta_per_invoice: '900' }),
  SUBSCRIPTION('eustress_bundle_yearly', 12999, 'year', 'bundle', { tickets_per_invoice: '4800', delta_per_invoice: '10800' }),
  PACK('eustress_delta_session', 1000, 1000),
  PACK('eustress_delta_project', 2500, 2600),
  PACK('eustress_delta_week', 5000, 5400),
  PACK('eustress_delta_sprint', 10000, 11200),
  PACK('eustress_delta_month', 20000, 23000),
  PACK('eustress_delta_studio', 50000, 60000),
];

/// The problems with `prices` (an array of Stripe price objects), as strings.
export function checkPrices(prices) {
  const problems = [];
  const active = (Array.isArray(prices) ? prices : []).filter((p) => p && p.active !== false);
  for (const want of EXPECTED) {
    const found = active.filter((p) => p.lookup_key === want.lookup_key);
    if (found.length === 0) { problems.push(`${want.lookup_key}: missing`); continue; }
    if (found.length > 1) { problems.push(`${want.lookup_key}: ${found.length} active prices share the key`); continue; }
    const got = found[0];
    const fail = (what, a, b) => problems.push(`${want.lookup_key}: ${what} is ${JSON.stringify(a)}, want ${JSON.stringify(b)}`);
    if (got.unit_amount !== want.unit_amount) fail('unit_amount', got.unit_amount, want.unit_amount);
    if (String(got.currency || '').toLowerCase() !== want.currency) fail('currency', got.currency, want.currency);
    if ((got.recurring?.interval ?? null) !== want.recurring) fail('recurring interval', got.recurring?.interval ?? null, want.recurring);
    if (want.recurring && (got.recurring?.interval_count ?? 1) !== 1) fail('interval_count', got.recurring?.interval_count, 1);
    for (const [k, v] of Object.entries(want.metadata)) {
      if ((got.metadata || {})[k] !== v) fail(`metadata.${k}`, (got.metadata || {})[k], v);
    }
  }
  const known = new Set(EXPECTED.map((w) => w.lookup_key));
  for (const p of active) {
    if (p.lookup_key && /^eustress_/.test(p.lookup_key) && !known.has(p.lookup_key)) {
      problems.push(`${p.lookup_key}: not in the table (an old or mistyped key)`);
    }
  }
  return problems;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const chunks = [];
  for await (const c of process.stdin) chunks.push(c);
  let json;
  try {
    json = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  } catch (e) {
    console.error('stdin is not JSON:', e.message);
    process.exit(2);
  }
  const problems = checkPrices(Array.isArray(json) ? json : json.data);
  if (problems.length) {
    for (const line of problems) console.log(line);
    process.exit(1);
  }
  console.log(`all ${EXPECTED.length} prices are right`);
}
