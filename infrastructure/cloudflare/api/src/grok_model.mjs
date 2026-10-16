// =============================================================================
// Which Grok model the Worker sends, and how it moves to a newer one
// =============================================================================
//
// McKale, 2026-10-08: "Update Grok 4.6 to 4.7 and build in a latest update
// feature so it is always current with XAI without injection attacks."
//
// One resolver. Every xAI call takes its model from resolveGrokModel(env),
// through grokClient(env): KYC, screening, search, the moderation judge and
// agent (moderationDeps in index.js), and /health. No call site holds a model
// string of its own.
//
// The model is, in order: the pinned model, the current model, DEFAULT_GROK_MODEL.
// State lives in KV SOCIAL under `config:grok-model`:
//
//   { current, previous, pinned, history: [...], adopted_by, adopted_at,
//     probation_until, blocked: { id: { until, reason } }, last_checked_at }
//
// Auto-update (sweepGrokModel, the hourly cron):
//   1. A model adopted automatically is on probation for PROBATION_MS. While it
//      is, every call's outcome is counted, and once ROLLBACK_MIN_CALLS calls
//      have been made, a failure rate at or above ROLLBACK_FAILURE_RATE puts the
//      previous model back, blocks the failed one for ROLLBACK_BLOCK_MS, and
//      alerts the administrators. The check runs on the cron and, at most once a
//      minute per isolate, right after a failed call.
//   2. A pin turns auto-update off. So does GROK_MODEL_AUTO_UPDATE=off.
//   3. xAI's model list is fetched from one fixed URL on xAI's own host.
//   4. The newest flagship id newer than the current model is the candidate.
//   5. A fixed canary call with the candidate must return a fixed JSON shape.
//      Pass: the candidate becomes current, the old one previous. Fail: the
//      candidate is blocked for CANARY_RETRY_MS. Both alert the administrators.
//
// The model list is untrusted data. What keeps it from steering the Worker:
//   - an id is accepted only when it matches ^grok-(\d{1,2})\.(\d{1,2})$, or the
//     same with a suffix GROK_MODEL_VARIANTS names (comma separated, letters and
//     digits only, e.g. "fast,mini"). Anything else is dropped, and never logged;
//   - the candidate must compare numerically greater than the current model;
//   - the candidate must be in the list xAI returned: the GET goes to
//     XAI_MODELS_URL over HTTPS, a redirect is never followed (redirect:
//     'manual', and anything but a 200 from https://api.x.ai is refused);
//   - the list's size and the time spent on it are capped;
//   - an id is only ever the `model` field of a request body: never prompt text,
//     never a URL path, and it reaches an email only through the escaping
//     `html` tag (after the pattern above has already passed it).
//
// A model with no price in src/review_costs.mjs leaves review cost quotes
// incomplete. That is correct, and the alert says so; no price is assumed.
//
// Endpoint (docs.x.ai/developers/rest-api-reference/inference/models):
//   GET https://api.x.ai/v1/language-models
//   -> { "models": [ { "id", "aliases": [...], "input_modalities": [...],
//                      "output_modalities": [...], "fingerprint", "version", ... } ] }
// The judge and KYC send images, so a model must take text and image input.
// =============================================================================

import { textPart, userMessage } from './xai.mjs';
import { sendNotice } from './account_notices.mjs';
import { renderEmail, html, paragraph, facts } from './email_shell.mjs';
import { readAccountEmail } from './email_ops.mjs';
import { reviewPricesFrom } from './review_costs.mjs';

export const DEFAULT_GROK_MODEL = 'grok-4.7';
export const GROK_MODEL_KEY = 'config:grok-model';
const HEALTH_PREFIX = 'config:grok-model:health:';

export const XAI_ORIGIN = 'https://api.x.ai';
export const XAI_MODELS_URL = `${XAI_ORIGIN}/v1/language-models`;
export const XAI_RESPONSES_URL = `${XAI_ORIGIN}/v1/responses`;

const HOUR = 3600 * 1000;
export const LIMITS = Object.freeze({
  max_id_length: 32,
  list_max_bytes: 256 * 1024,
  list_timeout_ms: 10_000,
  list_max_models: 200,
  list_max_aliases: 8,
  canary_max_bytes: 64 * 1024,
  canary_timeout_ms: 30_000,
  cache_ms: 60_000,
  history_max: 50,
  probation_ms: 72 * HOUR,
  rollback_min_calls: 10,
  rollback_failure_rate: 0.25,
  rollback_check_ms: 60_000,
  canary_retry_ms: 24 * HOUR,
  rollback_block_ms: 7 * 24 * HOUR,
});

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

const FLAGSHIP = /^grok-(\d{1,2})\.(\d{1,2})$/;
const VARIANT = /^grok-(\d{1,2})\.(\d{1,2})-([a-z0-9]{1,16})$/;
const SUFFIX = /^[a-z0-9]{1,16}$/;

/// The variant suffixes GROK_MODEL_VARIANTS enables. Empty by default: only the
/// flagship line is ever adopted.
export function variantAllowlist(env) {
  return new Set(String(env?.GROK_MODEL_VARIANTS || '').split(',').map((s) => s.trim().toLowerCase()).filter((s) => SUFFIX.test(s)));
}

/// `{ major, minor, variant }` for an id this Worker may send, else null.
/// `allow` is a Set of variant suffixes, or 'any' (a value already validated
/// when it was stored).
export function parseModelId(id, allow = new Set()) {
  if (typeof id !== 'string' || id.length === 0 || id.length > LIMITS.max_id_length) return null;
  const f = FLAGSHIP.exec(id);
  if (f) return { major: Number(f[1]), minor: Number(f[2]), variant: null };
  const v = VARIANT.exec(id);
  if (v && (allow === 'any' || allow.has(v[3]))) return { major: Number(v[1]), minor: Number(v[2]), variant: v[3] };
  return null;
}

/// Positive when `a` ranks above `b` by version digits. At one version the flagship ranks above a variant. Digits are NOT age:
/// xAI shipped grok-4.20 in March 2026 and grok-4.7 in September 2026, so ordering by digits read an older model as newer. Age comes
/// from each model's `created` time in xAI's own list (pickCandidate); this only breaks a tie between ids released together.
export function compareModels(a, b) {
  return (a.major - b.major) || (a.minor - b.minor) || ((a.variant ? 0 : 1) - (b.variant ? 0 : 1));
}

/// The newest acceptable id in `ids` that xAI released after `current` and that is not blocked, or null. `created` maps each listed id
/// to its release time in seconds, from xAI's list. Newer means released later, never larger digits. With no release time for the
/// current model (it is not on the list) or for a candidate, nothing is adopted: an unknown age is never a reason to switch.
export function pickCandidate(ids, current, allow, blocked = {}, now = Date.now(), created = null) {
  const cur = parseModelId(current, 'any');
  if (!cur || !created) return null;
  const curAt = created[current];
  if (!Number.isFinite(curAt)) return null;
  let best = null;
  for (const id of ids) {
    const p = parseModelId(id, allow);
    if (!p) continue;
    const at = created[id];
    if (!Number.isFinite(at) || at <= curAt) continue;
    if (isBlocked(blocked, id, now)) continue;
    if (!best || at > best.at || (at === best.at && compareModels(p, best.p) > 0)) best = { id, p, at };
  }
  return best ? best.id : null;
}

function isBlocked(blocked, id, now) {
  const b = blocked?.[id];
  if (!b) return false;
  const until = Date.parse(b.until);
  return !Number.isFinite(until) || until > now;
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const iso = (ms) => new Date(ms).toISOString();
const okId = (v) => (parseModelId(v, 'any') ? v : null);

export function emptyState() {
  return { current: null, previous: null, pinned: null, history: [], adopted_by: 'default', adopted_at: null, probation_until: null, blocked: {}, last_checked_at: null };
}

/// The stored state, every model field checked again on the way in.
export async function readState(env) {
  const s = emptyState();
  const raw = await env?.SOCIAL?.get(GROK_MODEL_KEY);
  if (!raw) return s;
  let p;
  try { p = JSON.parse(raw); } catch { console.error('grok model: the stored state is not JSON; the default model is used'); return s; }
  if (!p || typeof p !== 'object') return s;
  s.current = okId(p.current);
  s.previous = okId(p.previous);
  s.pinned = okId(p.pinned);
  s.history = Array.isArray(p.history) ? p.history.slice(-LIMITS.history_max) : [];
  s.adopted_by = ['default', 'auto', 'admin', 'rollback'].includes(p.adopted_by) ? p.adopted_by : 'default';
  s.adopted_at = typeof p.adopted_at === 'string' ? p.adopted_at : null;
  s.probation_until = typeof p.probation_until === 'string' ? p.probation_until : null;
  s.last_checked_at = typeof p.last_checked_at === 'string' ? p.last_checked_at : null;
  if (p.blocked && typeof p.blocked === 'object') {
    for (const [id, b] of Object.entries(p.blocked)) if (okId(id) && b && typeof b === 'object') s.blocked[id] = { until: String(b.until), reason: String(b.reason || '') };
  }
  return s;
}

async function writeState(env, state) {
  state.history = state.history.slice(-LIMITS.history_max);
  await env.SOCIAL.put(GROK_MODEL_KEY, JSON.stringify(state));
  cache.delete(env.SOCIAL);
}

/// The model a state sends.
export function modelOf(state) {
  return state.pinned || state.current || DEFAULT_GROK_MODEL;
}

function inProbation(state, now) {
  if (state.pinned || state.adopted_by !== 'auto' || !state.current) return false;
  const until = Date.parse(state.probation_until);
  return Number.isFinite(until) && until > now;
}

// Per isolate, keyed by the KV binding: a switch reaches every isolate within cache_ms.
const cache = new WeakMap();

async function cachedState(env, now = Date.now()) {
  const kv = env?.SOCIAL;
  if (!kv) return { state: emptyState(), model: DEFAULT_GROK_MODEL };
  const hit = cache.get(kv);
  if (hit && now - hit.at < LIMITS.cache_ms) return hit;
  let state;
  try { state = await readState(env); } catch (e) { console.error('grok model: the state could not be read; the default model is used:', e?.message || e); state = emptyState(); }
  const entry = { at: now, state, model: modelOf(state) };
  cache.set(kv, entry);
  return entry;
}

/// THE resolver: the model every xAI call sends.
export async function resolveGrokModel(env) {
  return (await cachedState(env)).model;
}

// ---------------------------------------------------------------------------
// The client every call site uses
// ---------------------------------------------------------------------------

/// A failure that may be the model's: a server error, or a refusal other than a
/// bad key (401) or a rate limit (429), which are the account's, not the model's.
export const isModelFailure = (status) => status >= 500 || (status >= 400 && status !== 401 && status !== 429);

const defaultFetch = (...args) => fetch(...args);

/// `{ model, fetch(body, apiKey, opts) }`: the resolved model, and a fetch that
/// sends it. `deps.fetch` replaces the network in tests; `deps.auditLog` records
/// a rollback the request path makes.
///
/// The Responses API is STATEFUL: it stores prompts and outputs for 30 days by
/// default, retrievable later by response id. For these calls that payload is
/// photographs of government identity documents plus applicants' legal names
/// and dates of birth, so `store: false` is mandatory, not a preference.
///
/// The model and the privacy field are applied AFTER the caller's body is
/// spread, so a call site cannot override them, and routing every request
/// through here means a new call site cannot forget them.
///
/// Two limits this cannot reach, both account-level rather than per-request:
///   - xAI retains API traffic for 30 days for abuse auditing (encrypted at
///     rest, then deleted). Removing that needs a Zero Data Retention
///     agreement arranged with xAI on the account.
///   - xAI states it does not train on API data.
export async function grokClient(env, deps = {}) {
  const model = await resolveGrokModel(env);
  const { state } = await cachedState(env);
  const watched = model === state.current && inProbation(state, Date.now());
  const send = deps.fetch || defaultFetch;
  async function grokFetch(body, apiKey, opts = {}) {
    let resp;
    try {
      resp = await send(XAI_RESPONSES_URL, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${apiKey}` },
        body: JSON.stringify({ ...body, model, store: false }),
        // Optional: moderation bounds each call so a stalled one is recorded as a
        // timeout instead of dying silently with the request.
        ...(opts.signal ? { signal: opts.signal } : {}),
      });
    } catch (e) {
      if (watched) await noteOutcome(env, state, false, deps);
      throw e;
    }
    if (watched) await noteOutcome(env, state, !isModelFailure(resp.status), deps);
    return resp;
  }
  return { model, fetch: grokFetch };
}

// ---------------------------------------------------------------------------
// Health while on probation, and rollback
// ---------------------------------------------------------------------------

// Made on first use, never at module load: the Workers runtime refuses random values in global scope (deploy error 10021).
let isolateId = null;
const isolate = () => (isolateId ||= crypto.randomUUID());
// Per KV binding, then per probation (model and adoption time).
const tallies = new WeakMap();
const lastCheck = new WeakMap();

const healthPrefix = (state) => `${HEALTH_PREFIX}${state.current}:${Date.parse(state.adopted_at) || 0}:`;

/// One call's outcome, added to this isolate's count for the model's probation.
/// Each isolate writes its own key, so no two writers share a counter.
async function noteOutcome(env, state, ok, deps) {
  const prefix = healthPrefix(state);
  if (!tallies.has(env.SOCIAL)) tallies.set(env.SOCIAL, new Map());
  const counts = tallies.get(env.SOCIAL);
  const t = counts.get(prefix) || { ok: 0, fail: 0 };
  if (ok) t.ok++; else t.fail++;
  counts.set(prefix, t);
  try {
    await env.SOCIAL.put(prefix + isolate(), JSON.stringify(t), { expirationTtl: Math.ceil(LIMITS.probation_ms / 1000) });
  } catch (e) {
    console.error('grok model: the call count was not saved:', e?.message || e);
  }
  if (ok) return;
  const now = Date.now();
  if (now - (lastCheck.get(env.SOCIAL) || 0) < LIMITS.rollback_check_ms) return;
  lastCheck.set(env.SOCIAL, now);
  await checkRollback(env, deps, now).catch((e) => console.error('grok model: the rollback check failed:', e?.message || e));
}

/// The calls and failures counted for the current model's probation, across isolates.
export async function probationCounts(env, state) {
  let ok = 0;
  let fail = 0;
  let cursor;
  for (let page = 0; page < 10; page++) {
    const listed = await env.SOCIAL.list({ prefix: healthPrefix(state), ...(cursor ? { cursor } : {}) });
    for (const k of listed?.keys || []) {
      try {
        const t = JSON.parse((await env.SOCIAL.get(k.name)) || '{}');
        ok += Number.isInteger(t.ok) && t.ok > 0 ? t.ok : 0;
        fail += Number.isInteger(t.fail) && t.fail > 0 ? t.fail : 0;
      } catch { /* a count that cannot be read is not counted */ }
    }
    if (listed?.list_complete !== false || !listed.cursor) break;
    cursor = listed.cursor;
  }
  return { calls: ok + fail, failures: fail };
}

/// Puts the previous model back when the current one, adopted automatically and
/// still on probation, fails at ROLLBACK_FAILURE_RATE or more over at least
/// ROLLBACK_MIN_CALLS calls.
export async function checkRollback(env, deps = {}, now = Date.now()) {
  const state = await readState(env);
  if (!inProbation(state, now)) return { rolled_back: false };
  const { calls, failures } = await probationCounts(env, state);
  if (calls < LIMITS.rollback_min_calls || failures / calls < LIMITS.rollback_failure_rate) return { rolled_back: false, calls, failures };
  const from = state.current;
  const to = state.previous || DEFAULT_GROK_MODEL;
  state.blocked[from] = { until: iso(now + LIMITS.rollback_block_ms), reason: 'rollback' };
  state.history.push({ at: iso(now), event: 'rollback', from, to, calls, failures });
  state.current = to;
  state.previous = from;
  state.adopted_by = 'rollback';
  state.adopted_at = iso(now);
  state.probation_until = null;
  await writeState(env, state);
  await alertAdmins(env, deps, 'rollback', { from, to, calls, failures, at: iso(now) });
  return { rolled_back: true, from, to, calls, failures };
}

// ---------------------------------------------------------------------------
// The model list
// ---------------------------------------------------------------------------

/// The body as text, or null when it is larger than `max` bytes.
async function readCapped(resp, max) {
  const declared = Number(resp.headers?.get?.('content-length'));
  if (Number.isFinite(declared) && declared > max) return null;
  if (!resp.body?.getReader) {
    const text = await resp.text();
    return new TextEncoder().encode(text).byteLength > max ? null : text;
  }
  const reader = resp.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > max) { await reader.cancel().catch(() => {}); return null; }
    chunks.push(value);
  }
  const all = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) { all.set(c, at); at += c.byteLength; }
  return new TextDecoder().decode(all);
}

/// A response that came from xAI's own host. With redirect 'manual' nothing is
/// followed; an empty url (a test double) is the fixed URL that was asked for.
function fromXai(resp) {
  if (resp.redirected) return false;
  if (!resp.url) return true;
  try { return new URL(resp.url).origin === XAI_ORIGIN; } catch { return false; }
}

const takesTextAndImage = (m) => Array.isArray(m.input_modalities) && m.input_modalities.includes('text') && m.input_modalities.includes('image');

/// The ids xAI lists for this key: `{ ok: true, ids }`, or `{ ok: false, error }`
/// with a fixed error code. Nothing from the body is ever put in the error.
export async function fetchModelList(apiKey, deps = {}) {
  const send = deps.fetch || defaultFetch;
  let resp;
  try {
    resp = await send(XAI_MODELS_URL, {
      method: 'GET',
      headers: { 'Authorization': `Bearer ${apiKey}`, 'Accept': 'application/json' },
      redirect: 'manual',
      signal: AbortSignal.timeout(LIMITS.list_timeout_ms),
    });
  } catch {
    return { ok: false, error: 'list_unreachable' };
  }
  if (resp.status >= 300 && resp.status < 400) return { ok: false, error: 'list_redirected' };
  if (resp.status !== 200) return { ok: false, error: `list_status_${Number(resp.status) || 0}` };
  if (!fromXai(resp)) return { ok: false, error: 'list_wrong_host' };
  let text;
  try { text = await readCapped(resp, LIMITS.list_max_bytes); } catch { return { ok: false, error: 'list_unreadable' }; }
  if (text === null) return { ok: false, error: 'list_too_large' };
  let data;
  try { data = JSON.parse(text); } catch { return { ok: false, error: 'list_not_json' }; }
  if (!data || typeof data !== 'object' || !Array.isArray(data.models)) return { ok: false, error: 'list_unexpected_shape' };
  const ids = [];
  // Each listed id's release time (`created`, seconds), the only measure of which model is newer. An alias shares its model's time.
  const created = Object.create(null);
  for (const m of data.models.slice(0, LIMITS.list_max_models)) {
    if (!m || typeof m !== 'object' || !takesTextAndImage(m)) continue;
    const at = Number.isInteger(m.created) && m.created > 0 && m.created < 4_102_444_800 ? m.created : null;
    const names = [m.id, ...(Array.isArray(m.aliases) ? m.aliases.slice(0, LIMITS.list_max_aliases) : [])];
    for (const id of names) {
      if (typeof id !== 'string' || id.length > LIMITS.max_id_length) continue;
      ids.push(id);
      if (at !== null && !(id in created)) created[id] = at;
    }
  }
  return { ok: true, ids, created };
}

// ---------------------------------------------------------------------------
// The canary
// ---------------------------------------------------------------------------

/// Fixed text: the candidate's id is never part of it.
export const CANARY_PROMPT = 'This is an automated health check. Reply with only this JSON object and no other text: {"canary":"ok","sum":S}, where S is the sum of 17 and 25 written as a number.';
const CANARY_SUM = 42;

/// The canary's request body. The id is the `model` field and nothing else.
export function canaryBody(model) {
  return { input: [userMessage([textPart(CANARY_PROMPT)])], model, store: false };
}

/// True when `text` is exactly the canary's answer: `{"canary":"ok","sum":42}`, in a code fence or not.
export function isCanaryAnswer(text) {
  if (typeof text !== 'string' || text.length > 2000) return false;
  const bare = text.trim().replace(/^```(?:json)?\s*/i, '').replace(/\s*```$/, '').trim();
  let v;
  try { v = JSON.parse(bare); } catch { return false; }
  if (!v || typeof v !== 'object' || Array.isArray(v)) return false;
  const keys = Object.keys(v).sort();
  return keys.length === 2 && keys[0] === 'canary' && keys[1] === 'sum' && v.canary === 'ok' && v.sum === CANARY_SUM;
}

function outputText(data) {
  for (const item of Array.isArray(data?.output) ? data.output : []) {
    for (const c of Array.isArray(item?.content) ? item.content : []) if (c?.type === 'output_text' && typeof c.text === 'string') return c.text;
  }
  return '';
}

/// One fixed call with `model`: `{ ok: true }` or `{ ok: false, reason }` with a fixed reason code.
export async function runCanary(model, apiKey, deps = {}) {
  if (!parseModelId(model, 'any')) return { ok: false, reason: 'bad_id' };
  const send = deps.fetch || defaultFetch;
  let resp;
  try {
    resp = await send(XAI_RESPONSES_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${apiKey}` },
      body: JSON.stringify(canaryBody(model)),
      redirect: 'manual',
      signal: AbortSignal.timeout(LIMITS.canary_timeout_ms),
    });
  } catch {
    return { ok: false, reason: 'unreachable' };
  }
  if (resp.status !== 200) return { ok: false, reason: `status_${Number(resp.status) || 0}` };
  if (!fromXai(resp)) return { ok: false, reason: 'wrong_host' };
  let text;
  try { text = await readCapped(resp, LIMITS.canary_max_bytes); } catch { return { ok: false, reason: 'unreadable' }; }
  if (text === null) return { ok: false, reason: 'too_large' };
  let data;
  try { data = JSON.parse(text); } catch { return { ok: false, reason: 'not_json' }; }
  return isCanaryAnswer(outputText(data)) ? { ok: true } : { ok: false, reason: 'wrong_answer' };
}

// ---------------------------------------------------------------------------
// The sweep (hourly cron)
// ---------------------------------------------------------------------------

export async function sweepGrokModel(env, deps = {}, now = Date.now()) {
  const rolled = await checkRollback(env, deps, now);
  if (rolled.rolled_back) return { rolled_back: rolled };
  const state = await readState(env);
  if (state.pinned) return { skipped: 'pinned' };
  if (String(env.GROK_MODEL_AUTO_UPDATE || '').trim().toLowerCase() === 'off') return { skipped: 'auto_update_off' };
  if (!env.GROK_API_KEY) return { skipped: 'no_key' };

  const listed = await fetchModelList(env.GROK_API_KEY, deps);
  if (!listed.ok) {
    console.error(`grok model: the model list was refused: ${listed.error}`);
    return { error: listed.error };
  }
  const current = modelOf(state);
  const candidate = pickCandidate(listed.ids, current, variantAllowlist(env), state.blocked, now, listed.created);
  if (!candidate) {
    const fresh = await readState(env);
    fresh.last_checked_at = iso(now);
    await writeState(env, fresh);
    return { checked: true, candidate: null };
  }

  const canary = await runCanary(candidate, env.GROK_API_KEY, deps);
  // Read again: a pin made while the canary ran wins.
  const fresh = await readState(env);
  fresh.last_checked_at = iso(now);
  if (!canary.ok) {
    fresh.blocked[candidate] = { until: iso(now + LIMITS.canary_retry_ms), reason: `canary_failed:${canary.reason}` };
    fresh.history.push({ at: iso(now), event: 'canary_failed', model: candidate, current, reason: canary.reason });
    await writeState(env, fresh);
    await alertAdmins(env, deps, 'canary_failed', { model: candidate, current, reason: canary.reason, at: iso(now) });
    return { checked: true, candidate, canary_failed: canary.reason };
  }
  if (fresh.pinned) { await writeState(env, fresh); return { skipped: 'pinned' }; }
  const from = modelOf(fresh);
  fresh.previous = from;
  fresh.current = candidate;
  fresh.adopted_by = 'auto';
  fresh.adopted_at = iso(now);
  fresh.probation_until = iso(now + LIMITS.probation_ms);
  fresh.history.push({ at: iso(now), event: 'switch', from, to: candidate });
  await writeState(env, fresh);
  await alertAdmins(env, deps, 'switch', { from, to: candidate, at: iso(now) });
  return { checked: true, candidate, switched: { from, to: candidate } };
}

// ---------------------------------------------------------------------------
// Alerts
// ---------------------------------------------------------------------------

const ALERT_FROM_NAME = 'Eustress Operations';
const EMAIL = /^[^\s@<>,;"']{1,64}@[^\s@<>,;"']{1,190}\.[^\s@<>,;"']{2,}$/;

/// True when src/review_costs.mjs (with MODERATION_PRICES) prices the model's input and output.
export function priceKnown(env, model) {
  const p = reviewPricesFrom(env)[model];
  return !!p && typeof p.input_per_mtok === 'number' && typeof p.output_per_mtok === 'number';
}

export const priceLine = (model) => `price unknown for ${model}; quotes incomplete until set`;

/// The subject, text and HTML of one alert. Every id in `d` has passed parseModelId.
export function renderGrokAlert(kind, d, env = {}) {
  const subjects = {
    switch: `Grok model switched to ${d.to}`,
    rollback: `Grok model rolled back from ${d.from} to ${d.to}`,
    canary_failed: `Grok model ${d.model} failed its canary; ${d.current} kept`,
    pin: `Grok model pinned to ${d.to}`,
    unpin: `Grok model unpinned; auto-update is on`,
    kyc_refused: `Grok refused KYC: ${d.status}${d.error ? ` ${String(d.error).slice(0, 80)}` : ''}`,
  };
  const lines = {
    switch: [`The Worker now sends ${d.to} on every xAI call. It was ${d.from}. The new model passed the canary call and is on probation for ${LIMITS.probation_ms / HOUR} hours: if ${Math.round(LIMITS.rollback_failure_rate * 100)}% or more of at least ${LIMITS.rollback_min_calls} calls fail, ${d.from} is put back automatically.`],
    rollback: [`${d.to} is back on every xAI call. ${d.from} failed ${d.failures} of ${d.calls} calls during its probation, at or past the ${Math.round(LIMITS.rollback_failure_rate * 100)}% threshold. ${d.from} is not tried again for ${LIMITS.rollback_block_ms / (24 * HOUR)} days.`],
    canary_failed: [`xAI lists ${d.model}, newer than ${d.current}, but its canary call did not return the expected answer (${d.reason}). ${d.current} is still in use, and ${d.model} is tried again in ${LIMITS.canary_retry_ms / HOUR} hours.`],
    pin: [`An administrator pinned ${d.to}. It was ${d.from}. Auto-update is off until the model is unpinned.`],
    unpin: [`An administrator unpinned the model. ${d.current} stays in use, and the hourly check adopts a newer flagship model after its canary passes.`],
    kyc_refused: [`xAI answered ${d.status} to an ID verification call. Applicants are told verification is temporarily unavailable and to try again; nobody is rejected, their uploads are kept, and the try is not counted. Check the xAI account's credits and the GROK_API_KEY secret. This alert is sent at most once an hour while it lasts, and /health shows the last failure.`],
  };
  const subject = subjects[kind];
  if (!subject) throw new Error(`unknown grok model alert ${kind}`);
  const paragraphs = [...lines[kind]];
  const model = kind === 'canary_failed' ? null : (d.to || d.current);
  if (model && !priceKnown(env, model)) paragraphs.push(`${priceLine(model)} (MODERATION_PRICES, or src/review_costs.mjs).`);
  const rows = Object.entries({ From: d.from, To: d.to, Candidate: d.model, Current: d.current, Reason: d.reason, Calls: d.calls, Failures: d.failures, Status: d.status, 'xAI error': d.error, By: d.by, At: d.at })
    .filter(([, v]) => v !== undefined && v !== null && v !== '')
    .map(([k, v]) => [k, String(v)]);
  const text = [...paragraphs, '', ...rows.map(([k, v]) => `${k}: ${v}`)].join('\n');
  const body = html`${paragraphs.map((p) => paragraph(p))}${rows.length ? facts(rows) : ''}`;
  const email = renderEmail({ title: subject, heading: subject, label: 'Operations', preheader: paragraphs[0], body, text, footer: 'plain' });
  return { subject, text: email.text, html: email.html };
}

/// The security mailbox (GROK_MODEL_ALERT_MAILBOX, else SECURITY_MAILBOX) and every administrator's address.
async function adminAddresses(env) {
  const out = new Set();
  for (const a of String(env.GROK_MODEL_ALERT_MAILBOX || env.SECURITY_MAILBOX || 'security@eustress.dev').split(',')) {
    if (EMAIL.test(a.trim())) out.add(a.trim().toLowerCase());
  }
  try {
    const admins = await env.USERS?.list({ prefix: 'admin:', limit: 100 });
    for (const k of admins?.keys || []) {
      const raw = await env.USERS.get(`user:${k.name.slice('admin:'.length)}`);
      const address = raw ? await readAccountEmail(env, JSON.parse(raw)) : null;
      if (typeof address === 'string' && EMAIL.test(address.trim())) out.add(address.trim().toLowerCase());
    }
  } catch (e) {
    console.error('grok model: listing the administrators failed:', e?.message || e);
  }
  return [...out];
}

/// Emails the administrators (the same sendNotice path as the takedown alerts) and writes the admin audit log.
export async function alertAdmins(env, deps, kind, d) {
  const rendered = renderGrokAlert(kind, d, env);
  console.error(`grok model alert: ${rendered.subject}`);
  let sent = 0;
  try {
    const results = await sendNotice(env, await adminAddresses(env), rendered, { kind: 'notice', accountId: 'grok-model-alerts', fromName: ALERT_FROM_NAME });
    sent = results.filter((r) => r.sent).length;
  } catch (e) {
    console.error('grok model: the alert was not sent:', e?.message || e);
  }
  if (deps?.auditLog) await Promise.resolve(deps.auditLog(env, `GROK_MODEL_${kind.toUpperCase()}`, d.by || 'system', d.to || d.model || d.current || null, d)).catch(() => {});
  return { sent };
}

// ---------------------------------------------------------------------------
// The admin routes
// ---------------------------------------------------------------------------

//   GET  /api/admin/grok-model          the state and the model in use
//   POST /api/admin/grok-model/pin      { model }: pin it (auto-update off); the canary must pass first
//   POST /api/admin/grok-model/unpin    auto-update on again
export async function handleGrokModelRoute(request, url, env, cors, deps) {
  const { requireAdmin, json } = deps;
  const p = url.pathname;
  if (p !== '/api/admin/grok-model' && p !== '/api/admin/grok-model/pin' && p !== '/api/admin/grok-model/unpin') return null;
  const adminId = await requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  const now = Date.now();

  if (p === '/api/admin/grok-model' && request.method === 'GET') {
    const state = await readState(env);
    const model = modelOf(state);
    return json({
      model, default_model: DEFAULT_GROK_MODEL, price_known: priceKnown(env, model),
      auto_update: state.pinned ? 'off: pinned' : (String(env.GROK_MODEL_AUTO_UPDATE || '').trim().toLowerCase() === 'off' ? 'off: GROK_MODEL_AUTO_UPDATE' : 'on'),
      variants_allowed: [...variantAllowlist(env)],
      thresholds: { probation_hours: LIMITS.probation_ms / HOUR, rollback_min_calls: LIMITS.rollback_min_calls, rollback_failure_rate: LIMITS.rollback_failure_rate },
      state,
    }, 200, cors);
  }

  if (p === '/api/admin/grok-model/pin' && request.method === 'POST') {
    let body;
    try { body = await request.json(); } catch { return json({ error: 'Send JSON: {"model": "grok-X.Y"}.', code: 'bad_request' }, 400, cors); }
    const model = body?.model;
    if (!parseModelId(model, variantAllowlist(env))) return json({ error: 'The model must be grok-X.Y, or grok-X.Y-suffix with the suffix in GROK_MODEL_VARIANTS.', code: 'bad_model' }, 400, cors);
    if (env.GROK_API_KEY) {
      const canary = await runCanary(model, env.GROK_API_KEY, deps);
      if (!canary.ok) {
        await alertAdmins(env, deps, 'canary_failed', { model, current: await resolveGrokModel(env), reason: `pin:${canary.reason}`, by: adminId, at: iso(now) });
        return json({ error: 'The canary call with that model did not return the expected answer, so it was not pinned.', code: 'canary_failed', reason: canary.reason }, 422, cors);
      }
    }
    const state = await readState(env);
    const from = modelOf(state);
    if (from !== model) state.previous = from;
    state.pinned = model;
    state.current = model;
    state.adopted_by = 'admin';
    state.adopted_at = iso(now);
    state.probation_until = null;
    delete state.blocked[model];
    state.history.push({ at: iso(now), event: 'pin', from, to: model, by: adminId });
    await writeState(env, state);
    await alertAdmins(env, deps, 'pin', { from, to: model, by: adminId, at: iso(now) });
    return json({ ok: true, model, state }, 200, cors);
  }

  if (p === '/api/admin/grok-model/unpin' && request.method === 'POST') {
    const state = await readState(env);
    if (!state.pinned) return json({ ok: true, model: modelOf(state), state }, 200, cors);
    const current = state.pinned;
    state.pinned = null;
    state.history.push({ at: iso(now), event: 'unpin', model: current, by: adminId });
    await writeState(env, state);
    await alertAdmins(env, deps, 'unpin', { current, by: adminId, at: iso(now) });
    return json({ ok: true, model: modelOf(state), state }, 200, cors);
  }

  return json({ error: 'Method not allowed' }, 405, cors);
}
