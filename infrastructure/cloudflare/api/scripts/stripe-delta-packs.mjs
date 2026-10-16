// Creates the "Eustress Delta" product and its six one-time pack prices in Stripe, the packs the /delta page sells, then checks them.
//
//   node infrastructure/cloudflare/api/scripts/stripe-delta-packs.mjs             test mode (what staging reads)
//   node infrastructure/cloudflare/api/scripts/stripe-delta-packs.mjs --live      live mode: asks you to type "live" first
//   node infrastructure/cloudflare/api/scripts/stripe-delta-packs.mjs --check     only check, creating nothing (add --live for live)
//   add --dry-run to any of them to print the Stripe CLI calls without making them
//
// It runs the same in PowerShell, Git Bash and cmd, because it calls the Stripe CLI itself: a `bash` script run from PowerShell is
// WSL's bash, which has neither node nor stripe. The Stripe CLI profile `default` is the Eustress account; the global active profile
// belongs to another business, so the profile is named on every call. A lookup key can belong to only one active price, so running this
// twice stops at the first duplicate instead of making a second set. The Worker finds each pack by its lookup key and reads what it
// grants from `metadata.delta`, so the amounts can be tuned in Stripe later (docs/monetization/DELTA_STRIPE_SETUP.md).

import { spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline/promises';
import { fileURLToPath } from 'node:url';
import { checkPrices, EXPECTED } from './check-stripe-prices.mjs';

export const PRODUCT_NAME = 'Eustress Delta';

/// The six packs: lookup key, price in cents, Delta granted. Read from the checker's table, so what this creates is what the checker accepts.
export const PACKS = EXPECTED.filter((p) => p.metadata.type === 'delta_purchase').map((p) => ({ key: p.lookup_key, cents: p.unit_amount, delta: p.metadata.delta }));

/// The profile goes first and `--live` goes LAST, after the subcommand: the Stripe CLI reads `stripe --project-name default --live prices list`
/// as an unknown command.
const call = (live, args) => ['--project-name', 'default', ...args, ...(live ? ['--live'] : [])];

/// The Stripe CLI arguments that create the product, one pack's price, and list the prices.
export const productArgs = (live) => call(live, ['products', 'create', '-d', `name=${PRODUCT_NAME}`]);
export const priceArgs = (live, product, pack) =>
  call(live, [
    'prices', 'create',
    '-d', `product=${product}`, '-d', 'currency=usd', '-d', `unit_amount=${pack.cents}`, '-d', `lookup_key=${pack.key}`,
    '-d', 'metadata[type]=delta_purchase', '-d', `metadata[delta]=${pack.delta}`,
  ]);
export const listArgs = (live) => call(live, ['prices', 'list', '--limit', '100']);

/// Run the Stripe CLI and return the JSON it printed. Throws with Stripe's own sentence when the call fails.
export function runStripe(args) {
  const r = spawnSync(process.env.STRIPE_BIN || 'stripe', args, { encoding: 'utf8', maxBuffer: 1 << 26 });
  if (r.error) {
    throw new Error(r.error.code === 'ENOENT' ? 'The Stripe CLI (stripe) is not on this terminal\'s PATH.' : String(r.error.message));
  }
  let json = null;
  try {
    json = JSON.parse(r.stdout);
  } catch {
    // Not JSON: the message below says what the CLI printed instead.
  }
  if (r.status !== 0 || json?.error) {
    throw new Error(json?.error?.message || (r.stderr || r.stdout || `stripe exited with ${r.status}`).trim());
  }
  return json;
}

/// The lines a person can read the call as, for --dry-run.
export const asCommand = (args) => 'stripe ' + args.map((a) => (/[\s[\]]/.test(a) ? `"${a}"` : a)).join(' ');

/// The problems with the six packs among the prices Stripe lists, as lines. Empty when they are all right.
export function packProblems(prices) {
  return checkPrices(prices).filter((line) => PACKS.some((p) => line.startsWith(`${p.key}:`)));
}

/// Create the product and the six prices, then check them. `ask` answers the live confirmation; `run` makes a Stripe CLI call.
/// Returns true when everything was created and the check is clean, false when nothing was done or the check found a problem. Throws at
/// the first call that fails.
export async function createPacks({ live, run, ask, log, dryRun = false }) {
  if (live && !dryRun) {
    log('This creates 1 product and 6 prices in LIVE mode on the Eustress Stripe account.');
    if ((await ask('Type "live" to go on: ')).trim() !== 'live') {
      log('Nothing was created.');
      return false;
    }
  }
  if (dryRun) {
    log(asCommand(productArgs(live)));
    for (const pack of PACKS) log(asCommand(priceArgs(live, '<the product id>', pack)));
    return true;
  }
  const product = run(productArgs(live)).id;
  log(`product ${product}`);
  for (const pack of PACKS) {
    let made;
    try {
      made = run(priceArgs(live, product, pack));
    } catch (e) {
      throw new Error(`Stopped at ${pack.key}: ${e.message} The product ${product} exists, with the packs listed above this line.`);
    }
    log(`${pack.key} ${made.id}`);
  }
  return checkAndReport({ live, run, log });
}

/// List the prices and say whether the six packs are right.
export function checkAndReport({ live, run, log }) {
  const problems = packProblems(run(listArgs(live)).data);
  if (problems.length === 0) {
    log(`The six Delta packs are right in ${live ? 'live' : 'test'} mode. The Worker caches the pack list for ten minutes.`);
    return true;
  }
  for (const line of problems) log(line);
  return false;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const flags = new Set(process.argv.slice(2));
  const live = flags.has('--live');
  const log = (line) => console.log(line);
  try {
    if (flags.has('--check')) {
      if (flags.has('--dry-run')) log(asCommand(listArgs(live)));
      else process.exitCode = checkAndReport({ live, run: runStripe, log }) ? 0 : 1;
    } else {
      const rl = createInterface({ input: process.stdin, output: process.stdout });
      const ask = (question) => rl.question(question);
      const done = await createPacks({ live, run: runStripe, ask, log, dryRun: flags.has('--dry-run') }).finally(() => rl.close());
      process.exitCode = done ? 0 : 1;
    }
  } catch (e) {
    console.error(e.message);
    process.exitCode = 1;
  }
}
