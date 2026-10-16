// Staging checks for the approval binding and the review lease, run by a person
// against STAGING with a test account's token. It refuses to run against
// production. It is not part of the test suite and nothing calls it.
//
//   STAGING_TOKEN=<test account JWT> STAGING_SIM=<listing id> node scripts/staging_checks.mjs
//
// Optional: STAGING_API (default https://api-staging.eustress.dev).
//
// What it does, in order:
//   1. Anonymous reads of the listing: the .pak download and the thumbnail. Prints status and
//      content type. A 200 download and an image (not image/svg+xml) are what an approved,
//      bound listing must give; a 403 on every listing would mean R2's get() and head()
//      disagree about checksums or version (see docs/launch/APPROVAL_BINDING_ROLLOUT.md).
//   2. Six parallel POST /submit as the author. Exactly one answer should say `rerun: true`
//      (one review started); the others answer `rerun: false` or 429. Only meaningful when the
//      listing is waiting for a review (publish a change first), otherwise all six say false.
//
// The lease takeover (a run killed mid-way is taken over after its lease expires) cannot be
// scripted from outside: start a review, stop the Worker's run (a deploy mid-run does it),
// wait `lease_seconds` (6 minutes), submit again, and read the case's history.

const api = (process.env.STAGING_API || 'https://api-staging.eustress.dev').replace(/\/$/, '');
const token = process.env.STAGING_TOKEN || '';
const sim = process.env.STAGING_SIM || '';

if (/^https:\/\/api\.eustress\.dev$/.test(api) || !/staging/.test(api)) {
  console.error(`Refusing to run against ${api}: this script is for staging only.`);
  process.exit(2);
}
if (!/^[a-f0-9-]{8,64}$/.test(sim)) {
  console.error('Set STAGING_SIM to the listing id.');
  process.exit(2);
}

const show = (label, res, extra = '') => console.log(`${label.padEnd(34)} ${String(res.status).padEnd(4)} ${res.headers.get('content-type') || ''} ${extra}`);

console.log(`API: ${api}\nListing: ${sim}\n`);
console.log('1. Anonymous reads');
const download = await fetch(`${api}/api/simulations/${sim}/download`);
show('   GET /download', download, `${download.headers.get('content-length') || ''} bytes`);
await download.body?.cancel();
const thumb = await fetch(`${api}/api/simulations/${sim}/thumbnail`);
show('   GET /thumbnail', thumb, /svg/.test(thumb.headers.get('content-type') || '') ? '(the placeholder: the stored image is not the reviewed one, or the listing has no binding where one is required)' : '');
await thumb.body?.cancel();

if (!token) {
  console.log('\n2. Skipped: set STAGING_TOKEN to a test account\'s JWT to run six parallel submits.');
  process.exit(0);
}

console.log('\n2. Six parallel POST /submit as the author');
const answers = await Promise.all(Array.from({ length: 6 }, async (_, n) => {
  const res = await fetch(`${api}/api/simulations/${sim}/submit`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } });
  const body = await res.json().catch(() => ({}));
  return { n, status: res.status, rerun: body.rerun, listing: body.status };
}));
for (const a of answers) console.log(`   request ${a.n}: HTTP ${a.status}, rerun=${a.rerun}, status=${a.listing}`);
const started = answers.filter((a) => a.rerun === true).length;
console.log(`\n   reviews started: ${started} (expected exactly 1 when the listing was waiting for a review, 0 otherwise)`);
process.exit(started <= 1 ? 0 : 1);
