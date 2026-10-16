// =============================================================================
// Review costs: what the publishing review spends on models, and its quote
// =============================================================================
//
// McKale, 2026-10-08: a publish is charged for its review, less for an update
// than for the first one, at the AI cost of the review plus a 10% margin, and
// priced by the size of the Space. This module measures that cost per model and
// per MB, and quotes the price. It runs in shadow: nothing here charges anyone,
// and nothing it records is read by a moderation decision. Charging waits on
// the currency (Tickets, Bliss or Delta).
//
// Why the fee is paid before the review runs. Every review spends money at a
// vendor: Jev reads the dossier, the Grok researcher looks up public facts when
// a case turns on them, the Claude judge reads the captures, and a shadow judge
// may read them too (Guardian Code v2.1). A review anyone can request for free is a way to spend
// Eustress's money: a script publishes, and every publish runs the pipeline.
// A fee taken BEFORE the pipeline runs moves that spend onto whoever asks for
// it, so a flood of publishes costs its sender at least what it costs
// Eustress, plus the margin, and the bill cannot be run up from outside. That
// fee has to be known before anything runs, from the size and the kind of the
// publish alone, and that is what quoteReview gives: the expected cost of a
// review of that size, as measured here, plus 10%. The per-author and
// per-listing caps in moderation.mjs still bound a burst; the fee is what makes
// a sustained one pay for itself. Taking the payment is not built here.
//
// Measured per review run (runModerationCase; an appeal goes to a person and calls no model):
//   - every model call (stages jev, researcher, judge, shadow): the model string sent, the transport, the input, cached
//     input, cache-write and output tokens as the vendor reported them (null when it reported
//     none; a count is never made up), the images and their detail, the latency,
//     and whether the call succeeded;
//   - the bytes reviewed: the dossier, the captures the judge was sent, the Jev
//     state in characters and bytes, and the package;
//   - the kind (initial: the listing's first decided review; update: a review
//     of a listing decided before, after its content changed; appeal) and the
//     reason the pipeline ran (publish, sweep, rerun, backfill, appeal).
// A profile text check (moderation.mjs checkProfileText) is metered the same way as its own kind and reason, `profile`: its
// one Jev call and that call's usage, on the ledger `modcost:profile:{accountId}` (or `modcost:profile` without an
// account). The text it checked is never recorded. A forum text check (forum_title, forum_post) is metered the same way
// under kind and reason `forum`, on `modcost:forum:{accountId}` (or `modcost:forum`).
//
// Storage (KV SOCIAL):
//   modcost:{simId}            the case's ledger: its last LEDGER_RUNS runs, each
//                              call with its usage and its cost at the prices in
//                              force when it was recorded. It is kept out of the
//                              case record on purpose: the case record is what a
//                              person in the Review Workspace reads, and cost
//                              lines have no place in a decision about content.
//   modcost:run:{day}:{runId}  one compact line per run, written whole and never
//                              read back and rewritten, with the same line as its
//                              list metadata.
//   modcost:day:{day}          the day's aggregate, rebuilt from the day's lines
//                              by rollupReviewCosts (the hourly sweep). One writer,
//                              so two runs that finish together cannot lose each
//                              other's counts the way a shared KV counter would.
//
// Dollars in the aggregates are worked out when they are read, from the token
// counts and the price table in force then: a price set later prices the
// history it applies to, and a model with no price is reported as unknown,
// never as $0.
//
// None of it is booked into the admin books (POST /api/accounting/costs, or
// the economics cost ledger). Those hold what was billed: the vendors' invoices
// are the spend to enter there, and an estimate booked beside them would count
// the same calls twice. The `/api/accounting/costs` total is also added to and
// never set, so an estimate booked there could not be taken back out. The
// admin report's `days` are the figures to check each invoice against.
// =============================================================================

export const REVIEW_COST_VERSION = 1;

// The size unit of every per-MB figure: 1 MB is 1,048,576 bytes, the unit the
// capture and dossier limits in moderation.mjs are written in.
export const BYTES_PER_MB = 1024 * 1024;

// The largest package a quote is asked about: world.mjs MAX_WORLD_BYTES, the
// largest world a publish can commit (not imported: world.mjs imports
// moderation.mjs, which imports this module).
export const MAX_QUOTE_BYTES = 8 * 1024 * 1024 * 1024;

// Runs kept on a case's ledger, and how long a run's line is kept for a rollup.
export const LEDGER_RUNS = 20;
const LINE_TTL_SECONDS = 400 * 86400;
// Runs kept per day for the spread (median, 90th percentile). The sums behind
// the averages and the quote count every run; past this many in a day, the
// spread is taken over the first ones and the aggregate says how many it left out.
export const CASES_PER_DAY = 2000;
// The longest range the admin report reads.
export const MAX_REPORT_DAYS = 92;

// The runs a quote is priced from: a review a publish asked for (submit, or the
// sweep finishing one), of a listing's first review or of an update. An admin's
// re-run, the legacy backfill and an appeal are measured and reported, but they
// are not what a publish's fee pays for.
export const QUOTED_KINDS = ['initial', 'update'];
const QUOTED_REASONS = ['publish', 'sweep'];
const UNKNOWN_MODEL = '(model not recorded)';

export const costLedgerKey = simId => `modcost:${simId}`;
export const costLineKey = (day, runId) => `modcost:run:${day}:${runId}`;
export const costDayKey = day => `modcost:day:${day}`;

// ---------------------------------------------------------------------------
// Prices
// ---------------------------------------------------------------------------
// US dollars. The token rates are per million tokens; per_image and per_call
// are flat. A field that is ABSENT is a charge the vendor does not make that way
// (images billed as input tokens, no per-call fee; an absent cached rate means
// cached tokens are charged at the input rate). A field that is NULL is a charge
// whose price is not known: a cost that needs it is unknown, and a quote that
// rests on it is incomplete. Keys are the model string the Worker sends.
//
// Overridden or extended through MODERATION_PRICES, JSON like
// MODERATION_THRESHOLDS: {"jev-latest": {"input_per_mtok": 0.05, "output_per_mtok": 0}}.
//
// Every price below comes from the repo, cited, and none is a guess. Replace
// them with the vendors' invoices when those are in hand.
export const DEFAULT_REVIEW_PRICES = Object.freeze({
  // xAI's list price for Grok 4.6 (the model before 4.7), as
  // docs/architecture/MODERATION_PIPELINE.md section 9 "Cost model" records it
  // (September 2026): $2.00 per million input tokens, $0.50 cached, $6.00 per
  // million output, doubled for any single prompt at or above 200K tokens.
  // Images are billed as input tokens, so there is no per-image price. The doc
  // asks for it to be confirmed on the vendor dashboard before budgeting.
  'grok-4.6': Object.freeze({
    vendor: 'xai', billed_by: 'tokens', input_per_mtok: 2.00, cached_input_per_mtok: 0.50, output_per_mtok: 6.00,
    long_context_tokens: 200_000, long_context_multiplier: 2,
    source: 'docs/architecture/MODERATION_PIPELINE.md section 9: xAI list price, September 2026, to be confirmed on the dashboard',
  }),
  // The judge, Claude Haiku 5.5 (JUDGE_MODEL in moderation.mjs), at Anthropic's
  // first-party rates. Source: the Anthropic claude-api skill's model table,
  // cached 2026-10-06 ("Claude Haiku 5.5 | claude-haiku-5-5 | 1M | $0.10 |
  // $0.50", for prompts up to 100K tokens, $0.50 / $2.50 beyond), and the same
  // skill's Haiku 5.5 migration notes: cache reads are 0.1x the input rate and
  // 5-minute cache writes 1.25x. The judge's system prompt is cached, so both
  // are charged. "Up to 100K tokens" is a prompt of 100,000 tokens or fewer;
  // the long rate applies from 100,001 (the multiplier is 5).
  'claude-haiku-5-5': Object.freeze({
    vendor: 'anthropic', billed_by: 'tokens', input_per_mtok: 0.10, cached_input_per_mtok: 0.01, cache_write_input_per_mtok: 0.125, output_per_mtok: 0.50,
    long_context_tokens: 100_001, long_context_multiplier: 5,
    source: 'Anthropic claude-api skill model table, cached 2026-10-06; cache read 0.1x and 5-minute cache write 1.25x from its Haiku 5.5 notes',
  }),
  // A shadow judge (env JUDGE_SHADOW_MODEL) that McKale may run beside the judge:
  // Claude Sonnet 5.5 at $2.00 input and $10.00 output per million tokens, cache
  // reads $0.20 (the same skill's table and its Sonnet 5.5 notes, cached
  // 2026-10-06); cache writes at 1.25x the input rate, as for every model there.
  'claude-sonnet-5-5': Object.freeze({
    vendor: 'anthropic', billed_by: 'tokens', input_per_mtok: 2.00, cached_input_per_mtok: 0.20, cache_write_input_per_mtok: 2.50, output_per_mtok: 10.00,
    source: 'Anthropic claude-api skill model table, cached 2026-10-06',
  }),
  // Grok 4.7, DEFAULT_GROK_MODEL in src/grok_model.mjs and the model the
  // moderation researcher sends while it is the one resolved: $2.00 per
  // million input tokens, $0.50 cached, $6.00 output, under 200K tokens, and
  // $4 / $1 / $12 from 200K. Source: third-party release coverage (Presenc),
  // not xAI's own price list: CONFIRM ON THE xAI INVOICE before budgeting. xAI
  // also bills web-search tool invocations (docs.x.ai, Tools pricing); that
  // price is not recorded here, so a researcher call's cost is the token part.
  'grok-4.7': Object.freeze({
    vendor: 'xai', billed_by: 'tokens', input_per_mtok: 2.00, cached_input_per_mtok: 0.50, output_per_mtok: 6.00,
    long_context_tokens: 200_000, long_context_multiplier: 2,
    source: 'third-party release coverage (Presenc); confirm on the xAI invoice. Web-search tool invocations are billed apart and not priced here',
  }),
  // Jev over TypeSafe's API (JEV_MODEL in moderation.mjs). The rate Eustress pays, as McKale gave it on 2026-10-08:
  // $0.042 per million input tokens ($42 per billion), and output tokens are free.
  'jev-latest': Object.freeze({ vendor: 'typesafe', billed_by: 'tokens', input_per_mtok: 0.042, output_per_mtok: 0, source: 'McKale, 2026-10-08: TypeSafe bills $0.042 per million input tokens; output is free' }),
  // Jev through the Workers AI binding (JEV_WORKERS_AI_MODEL in moderation.mjs).
  // Unknown: Workers AI bills in neurons, and no rate for this model is
  // recorded anywhere in the repo.
  'typesafe/jev': Object.freeze({ vendor: 'cloudflare-workers-ai', billed_by: 'tokens', input_per_mtok: null, output_per_mtok: null, source: null }),
});

const PRICE_FIELDS = ['input_per_mtok', 'cached_input_per_mtok', 'cache_write_input_per_mtok', 'output_per_mtok', 'per_image', 'per_call', 'long_context_tokens', 'long_context_multiplier'];

export function reviewPricesFrom(env) {
  const out = {};
  for (const [model, entry] of Object.entries(DEFAULT_REVIEW_PRICES)) out[model] = { ...entry };
  const raw = env?.MODERATION_PRICES;
  if (typeof raw !== 'string' || !raw.trim()) return out;
  let parsed;
  try { parsed = JSON.parse(raw); } catch (e) { console.error('MODERATION_PRICES is not valid JSON; defaults kept:', e?.message); return out; }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) { console.error('MODERATION_PRICES is not an object of models; defaults kept'); return out; }
  for (const [model, patch] of Object.entries(parsed)) {
    if (!model || !patch || typeof patch !== 'object' || Array.isArray(patch)) continue;
    const entry = { ...(out[model] || { billed_by: 'tokens' }) };
    let priced = false;
    for (const k of PRICE_FIELDS) {
      if (!Object.hasOwn(patch, k)) continue;
      const v = patch[k];
      if (v === null || (typeof v === 'number' && Number.isFinite(v) && v >= 0)) { entry[k] = v; priced = true; }
    }
    if (patch.billed_by === 'tokens' || patch.billed_by === 'call') entry.billed_by = patch.billed_by;
    if (typeof patch.vendor === 'string') entry.vendor = patch.vendor.slice(0, 60);
    // A price set here is cited as set here, unless the override names its own source.
    if (typeof patch.source === 'string') entry.source = patch.source.slice(0, 300);
    else if (priced) entry.source = 'MODERATION_PRICES';
    out[model] = entry;
  }
  return out;
}

// What the quote is built from. `margin` is McKale's 10%. `floor_usd` is the
// least a quote asks, null for none until one is chosen; `min_samples` is how
// many measured reviews of a kind a quote needs before it is complete;
// `window_days` is how far back it looks. Overridden through MODERATION_QUOTE.
export const DEFAULT_QUOTE_CONFIG = Object.freeze({
  margin: 0.10,
  floor_usd: null,
  min_samples: 30,
  window_days: 30,
  quotes_per_hour: 60,
});

export function quoteConfigFrom(env) {
  const out = { ...DEFAULT_QUOTE_CONFIG };
  const raw = env?.MODERATION_QUOTE;
  if (typeof raw !== 'string' || !raw.trim()) return out;
  try {
    const p = JSON.parse(raw);
    const n = v => typeof v === 'number' && Number.isFinite(v);
    if (n(p.margin) && p.margin >= 0 && p.margin <= 10) out.margin = p.margin;
    if (p.floor_usd === null || (n(p.floor_usd) && p.floor_usd >= 0)) out.floor_usd = p.floor_usd ?? null;
    if (Number.isInteger(p.min_samples) && p.min_samples >= 1) out.min_samples = p.min_samples;
    if (Number.isInteger(p.window_days) && p.window_days >= 1 && p.window_days <= MAX_REPORT_DAYS) out.window_days = p.window_days;
    if (Number.isInteger(p.quotes_per_hour) && p.quotes_per_hour >= 1) out.quotes_per_hour = p.quotes_per_hour;
  } catch (e) {
    console.error('MODERATION_QUOTE is not valid JSON; defaults kept:', e?.message);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Usage, and what a call costs
// ---------------------------------------------------------------------------

const int = v => (typeof v === 'number' && Number.isFinite(v) && v >= 0 ? Math.round(v) : null);

// A vendor's usage object as token counts. The xAI Responses API reports
// input_tokens (cached tokens are a part of it, in input_tokens_details) and
// output_tokens; the chat-completions names are read as well. Null when neither
// count is there. Reasoning tokens are added to the output only when the
// vendor's own total says they were counted outside it. A call that does not
// report its cached tokens is priced as if none were cached: the dearer
// reading, so a quote never rests on a saving nobody reported.
export function normalizeUsage(u) {
  if (!u || typeof u !== 'object' || Array.isArray(u)) return null;
  // Anthropic's Messages API: `input_tokens` counts only the uncached part; cache reads and cache writes are counted apart
  // (`cache_read_input_tokens`, `cache_creation_input_tokens`), and thinking is inside `output_tokens`. The prompt is all three.
  if (Object.hasOwn(u, 'cache_read_input_tokens') || Object.hasOwn(u, 'cache_creation_input_tokens')) {
    const fresh = int(u.input_tokens);
    const read = int(u.cache_read_input_tokens) ?? 0;
    const write = int(u.cache_creation_input_tokens) ?? 0;
    const out = int(u.output_tokens);
    if (fresh === null && out === null) return null;
    return {
      input_tokens: fresh === null ? null : fresh + read + write,
      cached_input_tokens: read,
      cache_write_input_tokens: write,
      output_tokens: out,
      reasoning_tokens: null,
      reasoning_separate: null,
    };
  }
  const input = int(u.input_tokens ?? u.prompt_tokens);
  let output = int(u.output_tokens ?? u.completion_tokens);
  if (input === null && output === null) return null;
  const cached = int(u.input_tokens_details?.cached_tokens ?? u.prompt_tokens_details?.cached_tokens);
  const reasoning = int(u.output_tokens_details?.reasoning_tokens ?? u.completion_tokens_details?.reasoning_tokens);
  const total = int(u.total_tokens);
  let reasoningSeparate = null;
  if (reasoning && input !== null && output !== null && total !== null) {
    reasoningSeparate = total >= input + output + reasoning;
    if (reasoningSeparate) output += reasoning;
  }
  return {
    input_tokens: input,
    cached_input_tokens: cached === null ? null : (input === null ? cached : Math.min(cached, input)),
    output_tokens: output,
    reasoning_tokens: reasoning,
    reasoning_separate: reasoningSeparate,
  };
}

// The quantities a price applies to, for one call or a sum of calls. Dollars are
// linear in them, so a sum of vectors prices to the sum of its calls' costs, and
// a line fitted to each quantity fits their cost. `no_input` and `no_output`
// count calls that reported no such count.
// The cache-write components are last, so a vector stored before they existed reads as zero for them.
export const COMPONENTS = ['input', 'cached_input', 'output', 'images', 'calls', 'long_input', 'long_cached_input', 'long_output', 'no_input', 'no_output', 'cache_write_input', 'long_cache_write_input'];
const C = Object.fromEntries(COMPONENTS.map((k, i) => [k, i]));
const PRICED = ['input', 'cached_input', 'output', 'images', 'calls', 'long_input', 'long_cached_input', 'long_output', 'cache_write_input', 'long_cache_write_input'];
export const zeroVector = () => COMPONENTS.map(() => 0);
function addInto(a, b, scale = 1) {
  for (let i = 0; i < a.length; i++) a[i] += (Number(b?.[i]) || 0) * scale;
  return a;
}

// A call's vector. A prompt at or past the model's long-context size is counted
// in the long components, which carry the long-context multiplier.
export function callVector(call, prices) {
  const v = zeroVector();
  v[C.calls] = 1;
  v[C.images] = int(call?.images) ?? 0;
  const u = call?.usage || null;
  const input = u?.input_tokens ?? null;
  const output = u?.output_tokens ?? null;
  const price = prices?.[call?.model];
  const long = input !== null && Number.isFinite(price?.long_context_tokens) && input >= price.long_context_tokens;
  if (input === null) v[C.no_input] = 1;
  else {
    const cached = Math.min(u.cached_input_tokens ?? 0, input);
    const written = Math.min(u.cache_write_input_tokens ?? 0, input - cached);
    v[long ? C.long_input : C.input] = input - cached - written;
    v[long ? C.long_cached_input : C.cached_input] = cached;
    v[long ? C.long_cache_write_input : C.cache_write_input] = written;
  }
  if (output === null) v[C.no_output] = 1;
  else v[long ? C.long_output : C.output] = output;
  return v;
}

// Dollars per unit of each priced component, null where the price is unknown.
function ratesOf(price) {
  if (!price || typeof price !== 'object') return null;
  const flat = (v, absent) => (v === undefined ? absent : v);
  if (price.billed_by === 'call') {
    return { input: 0, cached_input: 0, output: 0, images: flat(price.per_image, 0), calls: flat(price.per_call, null), long_input: 0, long_cached_input: 0, long_output: 0, cache_write_input: 0, long_cache_write_input: 0 };
  }
  const perToken = v => (v === undefined || v === null ? null : v / 1e6);
  const input = perToken(price.input_per_mtok);
  const cached = price.cached_input_per_mtok === undefined ? input : perToken(price.cached_input_per_mtok);
  // An absent cache-write rate means cache writes are charged as input (every vendor here but Anthropic writes no cache apart).
  const written = price.cache_write_input_per_mtok === undefined ? input : perToken(price.cache_write_input_per_mtok);
  const output = perToken(price.output_per_mtok);
  const m = flat(price.long_context_multiplier, 1);
  const times = r => (r === null || m === null ? null : r * m);
  return { input, cached_input: cached, output, images: flat(price.per_image, 0), calls: flat(price.per_call, 0), long_input: times(input), long_cached_input: times(cached), long_output: times(output), cache_write_input: written, long_cache_write_input: times(written) };
}

// The dollars a vector costs at a price, or why there are none:
//   known               every quantity above zero has a price
//   no_price            the model is not in the table
//   price_unknown       a quantity above zero, or a count a call did not report, has a null price
//   usage_not_reported  a call reported no token count for a charge with a price
export function priceVector(v, price) {
  // Nothing called costs nothing, whatever the model's price.
  if (!COMPONENTS.some((_, i) => Number(v?.[i]) > 0)) return { usd: 0, status: 'known' };
  const r = ratesOf(price);
  if (!r) return { usd: null, status: 'no_price' };
  let usd = 0;
  let unknown = false;
  for (const k of PRICED) {
    const q = Number(v?.[C[k]]) || 0;
    if (!q) continue;
    if (r[k] === null) unknown = true;
    else usd += q * r[k];
  }
  let unreported = false;
  for (const [missing, k] of [['no_input', 'input'], ['no_output', 'output']]) {
    if (!(Number(v?.[C[missing]]) > 0) || r[k] === 0) continue;
    if (r[k] === null) unknown = true;
    else unreported = true;
  }
  if (unknown) return { usd: null, status: 'price_unknown' };
  if (unreported) return { usd: null, status: 'usage_not_reported' };
  return { usd, status: 'known' };
}

// The cost of one recorded call at the given prices.
export function callCost(call, prices) {
  return priceVector(callVector(call, prices), prices?.[call?.model]);
}

// Dollars and per-unit rates. Linear, so it prices regression coefficients as
// well as sums; only used once priceVector has said every rate needed is there.
function dot(rates, v) {
  let s = 0;
  for (const k of PRICED) {
    const q = Number(v?.[C[k]]) || 0;
    if (q) s += q * (rates[k] ?? 0);
  }
  return s;
}

function tokensOf(v) {
  const input = v[C.input] + v[C.cached_input] + v[C.long_input] + v[C.long_cached_input] + (Number(v[C.cache_write_input]) || 0) + (Number(v[C.long_cache_write_input]) || 0);
  const cached = v[C.cached_input] + v[C.long_cached_input];
  const output = v[C.output] + v[C.long_output];
  return { input, cached_input: cached, output, total: input + output };
}

const round = (x, places = 8) => (x === null || !Number.isFinite(x) ? null : Math.round(x * 10 ** places) / 10 ** places);
const ratio = (a, b) => (a === null || !(b > 0) ? null : round(a / b, 6));

// ---------------------------------------------------------------------------
// The meter: one per review run
// ---------------------------------------------------------------------------
// Every function here swallows its own failures: a meter that cannot record or
// cannot save leaves the review exactly as it would have been. REVIEW_METERING
// "off" runs a review with no meter at all (the control the tests compare to).

export function createReviewMeter(env, simId, reason, now = Date.now()) {
  if (env?.REVIEW_METERING === 'off') return null;
  return {
    v: REVIEW_COST_VERSION,
    run_id: crypto.randomUUID(),
    sim_id: simId,
    kind: null,
    reason: reason || null,
    started_at: new Date(now).toISOString(),
    finished_at: null,
    bytes: { package: null, package_source: null, dossier: null, captures_count: 0, captures: 0, jev_state_chars: null, jev_state: null },
    calls: [],
  };
}

// The kinds a run is recorded under. `profile` is a profile text check (moderation.mjs checkProfileText): one Jev call
// over a display name or a bio, measured and reported under its own kind and reason, never quoted, and never with the text.
// `forum` is the same check over a forum title or a forum post, kept apart from `profile` the same way.
export const METERED_KINDS = [...QUOTED_KINDS, 'appeal', 'profile', 'forum'];

// The run's kind, once the case says what it is.
export function meterKind(meter, kind) {
  if (meter && METERED_KINDS.includes(kind)) meter.kind = kind;
}

export function meterBytes(meter, patch) {
  if (!meter || !patch) return;
  for (const [k, v] of Object.entries(patch)) if (Object.hasOwn(meter.bytes, k)) meter.bytes[k] = v;
}

// One model call. `usage` is the vendor's object as it came back, or null.
export function meterCall(meter, call) {
  if (!meter) return;
  try {
    let raw = null;
    if (call.usage && typeof call.usage === 'object') {
      const text = JSON.stringify(call.usage);
      raw = text.length <= 2000 ? JSON.parse(text) : { clipped: text.slice(0, 2000) };
    }
    meter.calls.push({
      stage: call.stage,
      ...(call.round ? { round: call.round } : {}),
      ...(call.attempt ? { attempt: call.attempt } : {}),
      model: typeof call.model === 'string' && call.model ? call.model : null,
      model_reported: typeof call.model_reported === 'string' ? call.model_reported.slice(0, 120) : null,
      transport: call.transport || null,
      ok: call.ok === true,
      status: int(call.status),
      error: call.error || null,
      latency_ms: int(call.latency_ms),
      usage: normalizeUsage(call.usage),
      usage_raw: raw,
      images: int(call.images) ?? 0,
      image_detail: call.image_detail || null,
    });
  } catch (e) {
    console.error('review costs: a call was not recorded:', e?.message);
  }
}

const byteLength = text => new TextEncoder().encode(text).length;

// The run as one compact line: its kind, reason, bytes, and a vector per model.
function lineOf(meter, prices) {
  const m = {};
  for (const c of meter.calls) addInto(m[c.model || UNKNOWN_MODEL] ||= zeroVector(), callVector(c, prices));
  const b = meter.bytes;
  return {
    v: REVIEW_COST_VERSION, k: meter.kind, r: meter.reason, done: !!meter.finished_at,
    pb: int(b.package), db: int(b.dossier) ?? 0, cb: int(b.captures) ?? 0, cn: int(b.captures_count) ?? 0, sb: int(b.jev_state) ?? 0,
    m,
  };
}

function totalsOf(line, prices) {
  const by_model = {};
  let known = 0;
  let complete = true;
  for (const [model, v] of Object.entries(line.m)) {
    const t = tokensOf(v);
    const priced = priceVector(v, prices[model]);
    if (priced.usd === null) complete = false; else known += priced.usd;
    by_model[model] = { calls: v[C.calls], input_tokens: t.input, cached_input_tokens: t.cached_input, output_tokens: t.output, images: v[C.images], calls_without_usage: Math.max(v[C.no_input], v[C.no_output]), cost_usd: round(priced.usd), cost_status: priced.status };
  }
  return { cost_usd: complete ? round(known) : null, known_cost_usd: round(known), complete, by_model };
}

// Saves the run: its ledger entry (replacing an earlier save of the same run)
// and its line for the day. Called at the judge checkpoint (final: false), so a
// run the platform ends during the judge keeps what it spent before it, and
// again when the run ends. Never throws.
export async function persistReviewMeter(env, meter, { final = true } = {}) {
  if (!meter || !meter.kind || !env?.SOCIAL) return null;
  try {
    const prices = reviewPricesFrom(env);
    if (final) meter.finished_at = new Date().toISOString();
    const calls = meter.calls.map(c => {
      const priced = callCost(c, prices);
      return { ...c, cost_usd: round(priced.usd), cost_status: priced.status };
    });
    const line = lineOf(meter, prices);
    const record = {
      v: meter.v, run_id: meter.run_id, sim_id: meter.sim_id, kind: meter.kind, reason: meter.reason,
      started_at: meter.started_at, finished_at: meter.finished_at, bytes: { ...meter.bytes }, calls, totals: totalsOf(line, prices),
    };
    const key = costLedgerKey(meter.sim_id);
    let prev = null;
    try { prev = JSON.parse((await env.SOCIAL.get(key)) || 'null'); } catch { prev = null; }
    const runs = [...(Array.isArray(prev?.runs) ? prev.runs : []).filter(r => r?.run_id !== meter.run_id), record].slice(-LEDGER_RUNS);
    await env.SOCIAL.put(key, JSON.stringify({ sim_id: meter.sim_id, runs }));
    const text = JSON.stringify(line);
    await env.SOCIAL.put(costLineKey(meter.started_at.slice(0, 10), meter.run_id), text, { expirationTtl: LINE_TTL_SECONDS, ...(byteLength(text) <= 1024 ? { metadata: line } : {}) });
    return record;
  } catch (e) {
    console.error('review costs: the run was not saved:', e?.message);
    return null;
  }
}

// ---------------------------------------------------------------------------
// The day's aggregate
// ---------------------------------------------------------------------------

function emptyAggregate(day) {
  return {
    v: REVIEW_COST_VERSION, day, runs: 0, unfinished: 0,
    bytes: { package: 0, package_runs: 0, dossier: 0, captures: 0, capture_count: 0, jev_state: 0 },
    // Every run, per model.
    models: {},
    // Every run, by reason, per model.
    reasons: {},
    // The quoted runs (QUOTED_REASONS) with a known package size, by kind: the
    // sums a straight line through (package MB, cost) is fitted from. `sx` and
    // `sxx` are over every run of the kind, `y` and `xy` per model over the
    // runs that called it (a run that did not call a model adds zero to both).
    groups: {},
    unsized: 0,
    cases: [],
    cases_left_out: 0,
  };
}

function addLine(agg, l) {
  if (!l || typeof l !== 'object' || !l.m || typeof l.m !== 'object') return;
  agg.runs += 1;
  if (!l.done) agg.unfinished += 1;
  const pb = Number.isFinite(l.pb) && l.pb > 0 ? l.pb : null;
  if (pb !== null) { agg.bytes.package += pb; agg.bytes.package_runs += 1; }
  agg.bytes.dossier += Number(l.db) || 0;
  agg.bytes.captures += Number(l.cb) || 0;
  agg.bytes.capture_count += Number(l.cn) || 0;
  agg.bytes.jev_state += Number(l.sb) || 0;
  const reason = agg.reasons[l.r || 'unknown'] ||= { runs: 0, models: {} };
  reason.runs += 1;
  for (const [model, v] of Object.entries(l.m)) {
    addInto(agg.models[model] ||= zeroVector(), v);
    addInto(reason.models[model] ||= zeroVector(), v);
  }
  if (QUOTED_KINDS.includes(l.k) && QUOTED_REASONS.includes(l.r)) {
    if (pb === null) agg.unsized += 1;
    else {
      const x = pb / BYTES_PER_MB;
      const g = agg.groups[l.k] ||= { runs: 0, sx: 0, sxx: 0, x_min: null, x_max: null, models: {} };
      g.runs += 1; g.sx += x; g.sxx += x * x;
      g.x_min = g.x_min === null ? x : Math.min(g.x_min, x);
      g.x_max = g.x_max === null ? x : Math.max(g.x_max, x);
      for (const [model, v] of Object.entries(l.m)) {
        const e = g.models[model] ||= { runs: 0, y: zeroVector(), xy: zeroVector() };
        e.runs += 1;
        addInto(e.y, v);
        addInto(e.xy, v, x);
      }
    }
  }
  if (agg.cases.length < CASES_PER_DAY) agg.cases.push({ k: l.k, r: l.r, pb, rb: (Number(l.db) || 0) + (Number(l.cb) || 0), m: l.m });
  else agg.cases_left_out += 1;
}

export function aggregateLines(day, lines) {
  const agg = emptyAggregate(day);
  for (const l of lines) addLine(agg, l);
  return agg;
}

// Several days as one: every sum added, the cases put together.
export function mergeAggregates(days) {
  const out = emptyAggregate(null);
  for (const d of days) {
    if (!d || typeof d !== 'object') continue;
    out.runs += d.runs || 0;
    out.unfinished += d.unfinished || 0;
    for (const k of Object.keys(out.bytes)) out.bytes[k] += Number(d.bytes?.[k]) || 0;
    for (const [model, v] of Object.entries(d.models || {})) addInto(out.models[model] ||= zeroVector(), v);
    for (const [reason, r] of Object.entries(d.reasons || {})) {
      const o = out.reasons[reason] ||= { runs: 0, models: {} };
      o.runs += r.runs || 0;
      for (const [model, v] of Object.entries(r.models || {})) addInto(o.models[model] ||= zeroVector(), v);
    }
    for (const [kind, g] of Object.entries(d.groups || {})) {
      const o = out.groups[kind] ||= { runs: 0, sx: 0, sxx: 0, x_min: null, x_max: null, models: {} };
      o.runs += g.runs || 0; o.sx += g.sx || 0; o.sxx += g.sxx || 0;
      if (g.x_min !== null && g.x_min !== undefined) o.x_min = o.x_min === null ? g.x_min : Math.min(o.x_min, g.x_min);
      if (g.x_max !== null && g.x_max !== undefined) o.x_max = o.x_max === null ? g.x_max : Math.max(o.x_max, g.x_max);
      for (const [model, e] of Object.entries(g.models || {})) {
        const oe = o.models[model] ||= { runs: 0, y: zeroVector(), xy: zeroVector() };
        oe.runs += e.runs || 0;
        addInto(oe.y, e.y);
        addInto(oe.xy, e.xy);
      }
    }
    out.unsized += d.unsized || 0;
    out.cases.push(...(Array.isArray(d.cases) ? d.cases : []));
    out.cases_left_out += d.cases_left_out || 0;
  }
  return out;
}

// Rebuilds one day's aggregate from its lines. The lines are read from the
// list's metadata where it is there, and fetched otherwise.
export async function rollupReviewCosts(env, day) {
  const prefix = `modcost:run:${day}:`;
  const lines = [];
  let cursor;
  for (let page = 0; page < 200; page++) {
    const listed = await env.SOCIAL.list({ prefix, limit: 1000, ...(cursor ? { cursor } : {}) });
    for (const k of listed.keys || []) {
      let line = k.metadata && typeof k.metadata === 'object' ? k.metadata : null;
      if (!line) {
        try { line = JSON.parse((await env.SOCIAL.get(k.name)) || 'null'); } catch { line = null; }
      }
      if (line) lines.push(line);
    }
    if (listed.list_complete === false && listed.cursor) cursor = listed.cursor;
    else break;
  }
  if (!lines.length) return null;
  const agg = aggregateLines(day, lines);
  agg.rolled_up_at = new Date().toISOString();
  await env.SOCIAL.put(costDayKey(day), JSON.stringify(agg));
  return agg;
}

const dayOf = ms => new Date(ms).toISOString().slice(0, 10);

// The hourly sweep: today so far, and yesterday until it is final.
export async function rollupRecentReviewCosts(env, now = Date.now()) {
  let n = 0;
  for (const day of [dayOf(now - 86400000), dayOf(now)]) if (await rollupReviewCosts(env, day)) n += 1;
  return n;
}

const DAY_RE = /^\d{4}-\d{2}-\d{2}$/;
function daysBetween(from, to) {
  const out = [];
  for (let t = Date.parse(`${from}T00:00:00Z`); t <= Date.parse(`${to}T00:00:00Z`) && out.length <= MAX_REPORT_DAYS; t += 86400000) out.push(dayOf(t));
  return out;
}

export async function readCostDays(env, from, to) {
  const days = [];
  for (const day of daysBetween(from, to)) {
    const raw = await env.SOCIAL.get(costDayKey(day));
    if (!raw) continue;
    try { days.push(JSON.parse(raw)); } catch { /* a day that does not parse is left out */ }
  }
  return days;
}

// ---------------------------------------------------------------------------
// The metrics McKale asked for
// ---------------------------------------------------------------------------

function percentile(sorted, q) {
  if (!sorted.length) return null;
  if (q === 0.5) {
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
  }
  // Nearest rank.
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)];
}

function spread(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return { median: round(percentile(sorted, 0.5)), p90: round(percentile(sorted, 0.9)) };
}

const priceSummary = price => (price ? { ...price } : null);

// Per model and overall: tokens and dollars per MB, the average cost of an
// initial review and of an update, and the spread of both.
export function reviewCostMetrics(days, prices) {
  const all = mergeAggregates(days);
  const packageMb = all.bytes.package / BYTES_PER_MB;
  const reviewedMb = (all.bytes.dossier + all.bytes.captures) / BYTES_PER_MB;
  const quoted = all.cases.filter(c => QUOTED_KINDS.includes(c.k) && QUOTED_REASONS.includes(c.r));

  const kindStats = (kind, costOfCase, tokensOfCase) => {
    const cases = quoted.filter(c => c.k === kind);
    const g = all.groups[kind];
    const out = { reviews: g?.runs || 0, spread_over: cases.length };
    const costs = cases.map(costOfCase);
    out.complete = costs.every(c => c !== null);
    out.average_usd = null;
    out.median_usd = null;
    out.p90_usd = null;
    if (cases.length && out.complete) {
      out.average_usd = round(costs.reduce((a, b) => a + b, 0) / costs.length);
      Object.assign(out, Object.fromEntries(Object.entries(spread(costs)).map(([k, v]) => [`${k}_usd`, v])));
    }
    const tokens = cases.map(tokensOfCase);
    if (tokens.length) {
      out.average_tokens = round(tokens.reduce((a, b) => a + b, 0) / tokens.length, 2);
      Object.assign(out, Object.fromEntries(Object.entries(spread(tokens)).map(([k, v]) => [`${k}_tokens`, v])));
    }
    return out;
  };

  const models = {};
  let overallKnown = 0;
  let overallComplete = true;
  const overallTokens = { input: 0, cached_input: 0, output: 0, total: 0 };
  for (const [model, v] of Object.entries(all.models)) {
    const t = tokensOf(v);
    for (const k of Object.keys(overallTokens)) overallTokens[k] += t[k];
    const priced = priceVector(v, prices[model]);
    if (priced.usd === null) overallComplete = false; else overallKnown += priced.usd;
    const caseCost = c => priceVector(c.m?.[model] || zeroVector(), prices[model]).usd;
    const caseTokens = c => tokensOf(c.m?.[model] || zeroVector()).total;
    models[model] = {
      price: priceSummary(prices[model]),
      calls: v[C.calls],
      calls_without_input_tokens: v[C.no_input],
      calls_without_output_tokens: v[C.no_output],
      images: v[C.images],
      tokens: t,
      tokens_per_package_mb: ratio(t.total, packageMb),
      tokens_per_reviewed_mb: ratio(t.total, reviewedMb),
      cost_usd: round(priced.usd),
      cost_status: priced.status,
      usd_per_package_mb: priced.usd === null ? null : ratio(priced.usd, packageMb),
      usd_per_reviewed_mb: priced.usd === null ? null : ratio(priced.usd, reviewedMb),
      initial: kindStats('initial', caseCost, caseTokens),
      update: kindStats('update', caseCost, caseTokens),
    };
  }
  const caseTotal = c => {
    let s = 0;
    for (const [model, v] of Object.entries(c.m || {})) {
      const priced = priceVector(v, prices[model]);
      if (priced.usd === null) return null;
      s += priced.usd;
    }
    return s;
  };
  const caseTokensTotal = c => Object.values(c.m || {}).reduce((s, v) => s + tokensOf(v).total, 0);
  const reasons = Object.fromEntries(Object.entries(all.reasons).map(([reason, r]) => {
    let usd = 0, complete = true;
    for (const [model, v] of Object.entries(r.models)) { const p = priceVector(v, prices[model]); if (p.usd === null) complete = false; else usd += p.usd; }
    return [reason, { runs: r.runs, cost_usd: complete ? round(usd) : null, known_cost_usd: round(usd), complete }];
  }));
  return {
    runs: all.runs,
    unfinished_runs: all.unfinished,
    mb: { package: round(packageMb, 6), reviewed: round(reviewedMb, 6), dossier: round(all.bytes.dossier / BYTES_PER_MB, 6), captures: round(all.bytes.captures / BYTES_PER_MB, 6), capture_count: all.bytes.capture_count, jev_state: round(all.bytes.jev_state / BYTES_PER_MB, 6), runs_with_package_size: all.bytes.package_runs },
    overall: {
      tokens: overallTokens,
      tokens_per_package_mb: ratio(overallTokens.total, packageMb),
      tokens_per_reviewed_mb: ratio(overallTokens.total, reviewedMb),
      cost_usd: overallComplete ? round(overallKnown) : null,
      known_cost_usd: round(overallKnown),
      complete: overallComplete,
      usd_per_package_mb: overallComplete ? ratio(overallKnown, packageMb) : null,
      usd_per_reviewed_mb: overallComplete ? ratio(overallKnown, reviewedMb) : null,
      initial: kindStats('initial', caseTotal, caseTokensTotal),
      update: kindStats('update', caseTotal, caseTokensTotal),
    },
    models,
    reasons,
    days: days.map(d => {
      const m = reviewCostMetricsDay(d, prices);
      return { day: d.day, runs: d.runs, cost_usd: m.complete ? m.usd : null, known_cost_usd: m.usd, complete: m.complete };
    }),
    spread_note: all.cases_left_out ? `${all.cases_left_out} run(s) past ${CASES_PER_DAY} a day are in the sums but not in the spread` : null,
  };
}

function reviewCostMetricsDay(d, prices) {
  let usd = 0, complete = true;
  for (const [model, v] of Object.entries(d.models || {})) { const p = priceVector(v, prices[model]); if (p.usd === null) complete = false; else usd += p.usd; }
  return { usd: round(usd), complete };
}

// ---------------------------------------------------------------------------
// The quote
// ---------------------------------------------------------------------------
// The expected AI cost of a review of a Space of `sizeBytes`, per model, as a
// straight line through the measured reviews of the same kind: a fixed part
// per review (the calls every review makes, whatever its size) plus a part per
// MB. Fitted by least squares over the window's sums. A model's line is fitted
// over every review of the kind, so a model that only some reviews reach (the
// judge after triage, the agent on the gray band) is weighted by how often it
// is reached. A slope that comes out negative is taken as zero (the average),
// so a bigger Space never quotes below a smaller one, and no part is below zero.
// The price is that cost times (1 + margin), and never under the floor.
//
// Complete only when every model's price is known, every call reported its
// usage, and the window holds at least `min_samples` reviews of the kind. An
// incomplete quote has price_usd null; `known_part` shows what the known
// models alone would come to, and `reasons` says what is missing.
export function quoteReview({ sizeBytes, kind, days, prices, config = DEFAULT_QUOTE_CONFIG, window = null }) {
  const x = sizeBytes / BYTES_PER_MB;
  const g = mergeAggregates(days).groups[kind] || { runs: 0, sx: 0, sxx: 0, x_min: null, x_max: null, models: {} };
  const n = g.runs;
  const reasons = [];
  if (n < config.min_samples) reasons.push({ code: 'not_enough_samples', samples: n, min_samples: config.min_samples });
  const den = n * g.sxx - g.sx * g.sx;
  const sloped = n >= 2 && den > 1e-9 * Math.max(1, n * g.sxx);
  const basis = {};
  let cost = 0;
  let knownCost = 0;
  for (const [model, e] of Object.entries(g.models)) {
    const price = prices[model];
    const priced = priceVector(e.y, price);
    const t = tokensOf(e.y);
    const tSlopeY = tokensOf(e.xy).total;
    let tokensPerMb = sloped ? (n * tSlopeY - g.sx * t.total) / den : 0;
    let tokensFixed = (t.total - tokensPerMb * g.sx) / n;
    if (tokensPerMb < 0) { tokensPerMb = 0; tokensFixed = t.total / n; }
    const entry = {
      cost_status: priced.status,
      price: priceSummary(price),
      reviews_calling: e.runs,
      tokens_per_review: round(t.total / n, 2),
      tokens_fixed: round(tokensFixed, 2),
      tokens_per_mb: round(tokensPerMb, 4),
      calls_without_usage: Math.max(e.y[C.no_input], e.y[C.no_output]),
      fixed_usd: null,
      per_mb_usd: null,
      expected_usd: null,
    };
    if (priced.status === 'known') {
      const rates = ratesOf(price);
      const sy = dot(rates, e.y);
      let b = sloped ? (n * dot(rates, e.xy) - g.sx * sy) / den : 0;
      let a = (sy - b * g.sx) / n;
      if (b < 0) { b = 0; a = sy / n; }
      const expected = Math.max(0, a + b * x);
      Object.assign(entry, { fixed_usd: round(a), per_mb_usd: round(b), expected_usd: round(expected) });
      cost += expected;
      knownCost += expected;
    } else {
      reasons.push({ code: priced.status, model, ...(priced.status === 'usage_not_reported' ? { calls: entry.calls_without_usage } : {}) });
    }
    basis[model] = entry;
  }
  const complete = reasons.length === 0;
  const margin = config.margin;
  const withFloor = usd => {
    const priced = usd * (1 + margin);
    const floor = config.floor_usd;
    return floor !== null && floor !== undefined && priced < floor ? { usd: floor, floor_applied: true } : { usd: priced, floor_applied: false };
  };
  const final = complete ? withFloor(cost) : null;
  return {
    kind,
    size_bytes: sizeBytes,
    size_mb: round(x, 6),
    mb_bytes: BYTES_PER_MB,
    complete,
    reasons,
    price_usd: final ? round(final.usd) : null,
    cost_usd: complete ? round(cost) : null,
    margin,
    margin_usd: complete ? round(cost * margin) : null,
    floor_usd: config.floor_usd,
    floor_applied: final ? final.floor_applied : false,
    known_part: complete || n === 0 ? null : { cost_usd: round(knownCost), with_margin_usd: round(knownCost * (1 + margin)), models: Object.keys(basis).filter(m => basis[m].cost_status === 'known') },
    samples: {
      reviews: n, min_samples: config.min_samples,
      ...(window ? { from: window.from, to: window.to } : {}),
      size_range_mb: g.x_min === null ? null : { min: round(g.x_min, 6), max: round(g.x_max, 6) },
      size_in_range: g.x_min !== null && x >= g.x_min && x <= g.x_max,
    },
    basis,
    charged: false,
  };
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

const todayOf = (now = Date.now()) => dayOf(now);

// GET /api/admin/moderation/costs?from=YYYY-MM-DD&to=YYYY-MM-DD (moderation.mjs
// gates it like every admin moderation route). The last 30 days by default.
// The aggregates are rebuilt hourly by the sweep, so today's figures can be an
// hour behind.
export async function reviewCostReport(env, params, now = Date.now()) {
  const isDay = d => DAY_RE.test(d) && !Number.isNaN(Date.parse(`${d}T00:00:00Z`));
  const bad = { status: 400, body: { error: 'from and to are dates, YYYY-MM-DD' } };
  const to = params.get('to') || todayOf(now);
  if (!isDay(to)) return bad;
  const from = params.get('from') || dayOf(Date.parse(`${to}T00:00:00Z`) - 29 * 86400000);
  if (!isDay(from)) return bad;
  const span = (Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86400000 + 1;
  if (span < 1 || span > MAX_REPORT_DAYS) return { status: 400, body: { error: `from must be on or before to, and the range at most ${MAX_REPORT_DAYS} days` } };
  const prices = reviewPricesFrom(env);
  const days = await readCostDays(env, from, to);
  return {
    status: 200,
    body: {
      from, to, days_with_data: days.length, mb_bytes: BYTES_PER_MB,
      ...reviewCostMetrics(days, prices),
      prices,
      quote_config: quoteConfigFrom(env),
      charged: false,
    },
  };
}

// The aggregates a quote reads, kept for a minute per isolate, so a burst of
// quotes is one read of the window, not one per request.
const QUOTE_DAYS_CACHE = new WeakMap();
async function quoteDays(env, from, to, ttlMs = 60_000) {
  let byRange = QUOTE_DAYS_CACHE.get(env);
  if (!byRange) { byRange = new Map(); QUOTE_DAYS_CACHE.set(env, byRange); }
  const key = `${from}..${to}`;
  const hit = byRange.get(key);
  if (hit && Date.now() - hit.at < ttlMs) return hit.days;
  const days = await readCostDays(env, from, to);
  byRange.clear();
  byRange.set(key, { at: Date.now(), days });
  return days;
}

// Read then write, like takeMailSlot: a parallel burst can pass the cap by a
// few, and the reads behind it are cached above, so a burst cannot multiply them.
async function takeQuoteSlot(env, accountId, perHour) {
  const key = `modquote:${accountId}:${new Date().toISOString().slice(0, 13)}`;
  const used = parseInt((await env.SOCIAL.get(key)) || '0', 10) || 0;
  if (used >= perHour) return false;
  await env.SOCIAL.put(key, String(used + 1), { expirationTtl: 2 * 3600 });
  return true;
}

// GET /api/simulations/review-quote?size_bytes=N&kind=initial|update
// A signed-in account asks what a review of a publish that size would cost.
// Session-gated and rate-limited, because it reads the aggregates.
export async function handleReviewQuote(request, url, env, deps) {
  const { json, cors } = deps;
  const accountId = await deps.verifyAuth(request, env);
  if (!accountId) return json({ error: 'Unauthorized' }, 401, cors);
  const raw = url.searchParams.get('size_bytes');
  const size = raw !== null && /^\d{1,12}$/.test(raw) ? Number(raw) : NaN;
  if (!Number.isSafeInteger(size) || size < 1 || size > MAX_QUOTE_BYTES) return json({ error: `size_bytes is a whole number of bytes from 1 to ${MAX_QUOTE_BYTES}` }, 400, cors);
  const kind = url.searchParams.get('kind') || 'initial';
  if (!QUOTED_KINDS.includes(kind)) return json({ error: 'kind is initial or update' }, 400, cors);
  const config = quoteConfigFrom(env);
  if (!(await takeQuoteSlot(env, accountId, config.quotes_per_hour)))
    return json({ error: 'Too many quotes this hour. Try again later.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '3600' });
  const to = todayOf();
  const from = dayOf(Date.parse(`${to}T00:00:00Z`) - (config.window_days - 1) * 86400000);
  const days = await quoteDays(env, from, to);
  return json(quoteReview({ sizeBytes: size, kind, days, prices: reviewPricesFrom(env), config, window: { from, to } }), 200, cors);
}
