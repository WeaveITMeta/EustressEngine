import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import {
  buildJevQuestions, buildJevState, jevEvaluate, deterministicSignals,
  decideTriage, decideFinal, parseJudgeVerdict, runModerationCase, makeToolExecutor,
  handleModerationRoute, isListable, canServe, publicModeration, loadCase,
  DEFAULT_THRESHOLDS, HARD_CATEGORIES, SOFT_CATEGORIES, QUALITY_BANDS, RATINGS,
  MODERATION_TOOLS, AGENT_TOOL_NAMES, POLICY_HASH_ANCHORED, DOSSIER_MAX_BYTES,
} from '../src/moderation.mjs';
import { render, TARGET } from '../scripts/sync-policy.mjs';
import { GUARDIAN_POLICY_TEXT, MODERATION_PLAYBOOK_TEXT } from '../src/generated/policy_text.mjs';

// ── fakes ───────────────────────────────────────────────────────────────────

function kv() {
  const m = new Map();
  return {
    m,
    get: async k => m.get(k) ?? null,
    put: async (k, v) => { m.set(k, typeof v === 'string' ? v : String(v)); },
    delete: async k => { m.delete(k); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ keys: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(name => ({ name })), list_complete: true }),
  };
}

function r2() {
  const m = new Map();
  const obj = (key, rec) => ({
    key, size: rec.bytes.byteLength, etag: rec.etag, httpEtag: `"${rec.etag}"`, httpMetadata: rec.httpMetadata || {}, customMetadata: rec.customMetadata || {},
    text: async () => new TextDecoder().decode(rec.bytes), arrayBuffer: async () => rec.bytes.slice(0), body: rec.bytes,
  });
  return {
    m,
    put: async (key, body, opts = {}) => {
      const bytes = typeof body === 'string' ? new TextEncoder().encode(body).buffer : (body instanceof ArrayBuffer ? body : body.buffer);
      const rec = { bytes, etag: createHash('md5').update(Buffer.from(bytes)).digest('hex'), httpMetadata: opts.httpMetadata, customMetadata: opts.customMetadata };
      m.set(key, rec);
      return obj(key, rec);
    },
    get: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    head: async key => (m.has(key) ? obj(key, m.get(key)) : null),
    delete: async key => { m.delete(key); },
    list: async ({ prefix = '', limit = 1000 } = {}) => ({ objects: [...m.keys()].filter(k => k.startsWith(prefix)).slice(0, limit).map(key => ({ key })) }),
  };
}

// audit_rate is pinned to 0 so a 3% random audit cannot add a second Grok call
// to a test that counts them; the audit path has its own test with the rate at 1.
function environment(extra = {}) {
  return { SOCIAL: kv(), USERS: kv(), AUDIT_LOG: kv(), SCENES: r2(), JEV_API_KEY: 'jev-test', GROK_API_KEY: 'grok-test', MODERATION_THRESHOLDS: '{"audit_rate":0}', ...extra };
}

const json = (value, status, headers) => new Response(JSON.stringify(value), { status, headers: { ...headers, 'Content-Type': 'application/json' } });

// A Jev answer set where everything is clean, then overrides.
function jevAnswers(overrides = {}) {
  const q = buildJevQuestions();
  const answers = {};
  for (const [k, def] of Object.entries(q)) {
    if (def.type === 'noul') answers[k] = { type: 'noul', noul: k === 'metadata_consistency' || k === 'functional_purpose' || k === 'spatial_intent' || k === 'genre_fit' ? 0.9 : 0.02 };
    else if (def.type === 'choice') {
      const first = k === 'content_rating' ? 'all_ages' : k === 'content_kind' ? 'simulation' : Object.keys(def.criteria)[0];
      answers[k] = { type: 'choice', choice: first, probabilities: Object.fromEntries(Object.keys(def.criteria).map(c => [c, c === first ? 0.9 : 0.01])), confidence: 0.9 };
    } else if (def.type === 'score') {
      const levels = def.criteria;
      const pick = k === 'quality_band' ? 'solid_authored_experience' : levels[0];
      answers[k] = { type: 'score', score: levels.indexOf(pick) / (levels.length - 1), probabilities: Object.fromEntries(levels.map(l => [l, l === pick ? 0.85 : 0.03])), legend: Object.fromEntries(levels.map((l, i) => [l, i])), confidence: 0.85 };
    }
  }
  for (const [k, v] of Object.entries(overrides)) {
    if (typeof v === 'number') answers[k] = { type: 'noul', noul: v };
    else answers[k] = { ...answers[k], ...v };
  }
  return { model: 'jev-test', answers, usage: { input_tokens: 1234, output_tokens: 0 } };
}

function jevFetch(answers, calls = []) {
  return async (url, init) => {
    calls.push({ url, body: JSON.parse(init.body) });
    return new Response(JSON.stringify(typeof answers === 'function' ? answers(calls.length) : answers), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
}

function verdict(v = {}) {
  return { verdict: 'publish', quality: 'listed', policy_version: '1.2', policy_hash: 'sha256:x', spatial_evidence: ['central plaza with four symmetric fountains', 'material variety across the arcade facades'], rationale: 'Deliberate composition, no prohibited category.', confidence: 0.9, flagged_for_review: false, ...v };
}

function grokJudge(text, calls = []) {
  return async body => {
    calls.push(body);
    const t = typeof text === 'function' ? text(body) : text;
    return { ok: true, status: 200, json: async () => ({ model: 'grok-test', output: [{ type: 'message', content: [{ type: 'output_text', text: typeof t === 'string' ? t : JSON.stringify(t) }] }] }), text: async () => '' };
  };
}

function extractGrokText(data) {
  for (const item of data?.output || []) for (const c of item?.content || []) if (typeof c?.text === 'string') return c.text;
  return '';
}

function deps(env, { judge = verdict(), grok, admin = null, audits = [] } = {}) {
  const judgeCalls = [];
  return {
    calls: { judge: judgeCalls },
    verifyAuth: async request => (request.headers.get('Authorization') || '').replace('Bearer ', '') || null,
    requireAdmin: async request => ((request.headers.get('Authorization') || '').replace('Bearer ', '') === admin ? admin : null),
    json, cors: {},
    auditLog: async (env, action, adminId, target, details) => { audits.push({ action, adminId, target, details }); },
    grokFetch: grok || grokJudge(judge, judgeCalls),
    extractGrokText,
    policyText: GUARDIAN_POLICY_TEXT, policyHash: POLICY_HASH_ANCHORED, playbookText: MODERATION_PLAYBOOK_TEXT,
    fetch: jevFetch(jevAnswers()),
  };
}

const dossier = (over = {}) => ({
  dossier_version: 1, content_root: 'blake3:abc',
  listing: { name: 'Harbor Town' },
  digest: { entity_count: 420, class_histogram: [{ class_name: 'Part', count: 300 }], default_material_fraction: 0.3, default_name_fraction: 0.2, duplicate_transform_fraction: 0.05, script_count: 3, text_string_count: 12, spawn_count: 2 },
  strings: ['Harbor Town', 'Welcome to the docks', 'Fish market'],
  scripts: [{ path: 'Soul/tide/tide.luau', language: 'luau', lines: 40, source: 'local tide = 0\nwhile true do tide = tide + 1 end' }],
  assets: { sounds: ['gulls.ogg'] }, signals: { urls: [], emails: [], phones: [], discord_invites: [] },
  ...over,
});

async function publishedSim(env, { id = 'a1b2c3d4-0000-4000-8000-000000000001', author = 'user-1', withDossier = dossier(), withCaptures = 2, pak = 'PAKBYTES-1', is_public = true, name = 'Harbor Town' } = {}) {
  const sim = { id, name, description: 'A working harbor with a tide simulation.', genre: 'Simulation', author_id: author, author_name: 'ann', is_public, thumbnail_url: null, r2_key: `universes/${id}/universe.pak`, play_count: 0, published_at: new Date().toISOString(), updated_at: new Date().toISOString(), moderation: { status: 'pending' } };
  const stored = await env.SCENES.put(sim.r2_key, new TextEncoder().encode(pak).buffer);
  sim.pak_etag = stored.etag; sim.scene_size_bytes = stored.size;
  await env.SOCIAL.put(`sim:${id}`, JSON.stringify(sim));
  if (withDossier) await env.SCENES.put(`universes/${id}/moderation/dossier.json`, JSON.stringify(withDossier), { httpMetadata: { contentType: 'application/json' } });
  const png = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3]).buffer;
  for (let n = 0; n < withCaptures; n++) await env.SCENES.put(`universes/${id}/moderation/capture-${n}.png`, png, { httpMetadata: { contentType: 'image/png' } });
  return sim;
}

const simOf = async (env, id) => JSON.parse(await env.SOCIAL.get(`sim:${id}`));

// ── policy text ships in sync with docs ─────────────────────────────────────

test('shipped policy text matches docs/ and hashes to the anchored policy hash', () => {
  assert.equal(readFileSync(TARGET, 'utf8'), render(), 'run npm run sync-policy');
  assert.equal('sha256:' + createHash('sha256').update(GUARDIAN_POLICY_TEXT).digest('hex'), POLICY_HASH_ANCHORED);
  assert.match(MODERATION_PLAYBOOK_TEXT, /Eustress Moderation Playbook/);
});

test('the playbook names every agent tool and only the agent tools as agent actions', () => {
  for (const name of AGENT_TOOL_NAMES) assert.ok(MODERATION_PLAYBOOK_TEXT.includes(`\`${name}\``), `playbook mentions ${name}`);
  const humanOnly = MODERATION_TOOLS.filter(t => !t.agent).map(t => t.name);
  assert.deepEqual(humanOnly.sort(), ['moderation_backfill', 'moderation_release', 'moderation_rerun', 'moderation_resolve_appeal']);
});

// ── question battery and state budget ───────────────────────────────────────

test('the battery covers every hard and soft category with a criteria pair', () => {
  const q = buildJevQuestions();
  for (const k of [...HARD_CATEGORIES, ...SOFT_CATEGORIES]) {
    assert.equal(q[k].type, 'noul');
    assert.ok(q[k].criteria.true && q[k].criteria.false);
  }
  assert.deepEqual(Object.keys(q.content_rating.criteria), RATINGS);
  assert.deepEqual(q.quality_band.criteria, QUALITY_BANDS);
  assert.ok(Object.keys(q).length >= 28);
});

test('state stays under budget: scripts round-robin, strings trimmed last', () => {
  const big = 'x'.repeat(50_000);
  const d = dossier({ scripts: [{ path: 'a.luau', language: 'luau', lines: 1, source: big }, { path: 'b.luau', language: 'luau', lines: 1, source: 'print(1)' }], strings: Array.from({ length: 1000 }, (_, i) => `string ${i}`) });
  const roomy = buildJevState({ name: 'n' }, d, { maxChars: 20_000 });
  assert.ok(JSON.stringify(roomy).length <= 20_000);
  assert.equal(roomy.scripts.length, 2, 'the small script is not crowded out by the big one');
  assert.ok(roomy.scripts[0].truncated);
  assert.equal(roomy.scripts[1].truncated, false);
  assert.equal(roomy.strings.length, 400, 'strings are capped at 400 and all fit here');
  assert.equal(roomy.truncated, true);

  const tight = buildJevState({ name: 'n' }, d, { maxChars: 6_000 });
  assert.ok(JSON.stringify(tight).length <= 6_000);
  assert.equal(tight.scripts.length, 2, 'both scripts keep a floor of the budget');
  assert.ok(tight.strings.length > 0 && tight.strings.length < 400, 'strings are what gets trimmed');
});

test('jevEvaluate normalizes answers and retries on 429', async () => {
  let n = 0;
  const fetchImpl = async () => { n += 1; return n === 1 ? new Response('slow down', { status: 429 }) : new Response(JSON.stringify(jevAnswers({ hate_incitement: 0.4 })), { status: 200 }); };
  const out = await jevEvaluate({ state: {}, questions: buildJevQuestions(), env: { JEV_API_KEY: 'k' }, fetchImpl });
  assert.equal(n, 2);
  assert.equal(out.answers.hate_incitement.p, 0.4);
  assert.equal(out.answers.content_rating.choice, 'all_ages');
  assert.equal(out.transport, 'typesafe');
  await assert.rejects(() => jevEvaluate({ state: {}, questions: {}, env: {}, fetchImpl }), /not configured/);
});

// ── decision policy ─────────────────────────────────────────────────────────

const sig = over => ({ ...deterministicSignals({ r2_key: 'x' }, dossier(), { capturesCount: 2 }), ...over });
const triage = (over, s) => decideTriage({ signals: sig(s), jev: { answers: jevAnswers(over).answers && Object.fromEntries(Object.entries(jevAnswers(over).answers).map(([k, a]) => [k, a.type === 'noul' ? { type: 'noul', p: a.noul } : a])) }, thresholds: DEFAULT_THRESHOLDS });

test('hard categories quarantine at 0.85, hold at 0.35, CSAM holds from 0.20', () => {
  assert.equal(triage({ terrorism_or_extremist_promotion: 0.9 }).outcome, 'quarantine');
  assert.equal(triage({ terrorism_or_extremist_promotion: 0.5 }).outcome, 'hold');
  assert.equal(triage({ terrorism_or_extremist_promotion: 0.3 }).outcome, 'judge');
  assert.equal(triage({ csam_or_minor_sexualization: 0.25 }).outcome, 'hold');
  const t = triage({ csam_or_minor_sexualization: 0.25 });
  assert.equal(t.lane, 'legal');
  assert.equal(t.reasons[0].code, 'csam_or_minor_sexualization');
});

test('soft categories auto-reject only with real-world intent and no ambiguity', () => {
  assert.equal(triage({ real_crime_instructions: 0.9, real_world_intent: 0.8 }).outcome, 'reject');
  assert.equal(triage({ real_crime_instructions: 0.9, real_world_intent: 0.2 }).outcome, 'escalate');
  assert.equal(triage({ real_crime_instructions: 0.9, real_world_intent: 0.8, needs_human_context: 0.7 }).outcome, 'escalate');
  assert.equal(triage({ fraud_or_scam_facilitation: 0.5 }).outcome, 'escalate');
  assert.equal(triage({ fraud_or_scam_facilitation: 0.2 }).outcome, 'judge');
});

test('child-directed experiences with COPPA conflicts get changes requested, clean ones proceed', () => {
  const conflict = triage({ child_directed: 0.9, external_links_or_contact: 0.8 });
  assert.equal(conflict.outcome, 'changes_requested');
  assert.equal(conflict.lane, 'coppa');
  assert.ok(conflict.coppa_flags.some(f => f.code === 'external_links_or_contact'));
  const rated = triage({ child_directed: 0.9, content_rating: { choice: 'teen_13' } });
  assert.equal(rated.outcome, 'changes_requested');
  assert.ok(rated.coppa_flags.some(f => f.code === 'rating_above_all_ages'));
  const clean = triage({ child_directed: 0.9 });
  assert.equal(clean.outcome, 'judge');
  assert.equal(clean.child_directed, true);
  // Deterministic URL signal counts even when the classifier missed it.
  const det = triage({ child_directed: 0.9 }, { urls: 2 });
  assert.equal(det.outcome, 'changes_requested');
});

test('quality lane: deterministic empties reject, confident asset flips reject, shaky ones escalate, solid ones judge', () => {
  assert.deepEqual([triage({}, { empty: true }).outcome, triage({}, { empty: true }).reasons[0].criterion], ['reject', 2]);
  assert.equal(triage({}, { default_only: true }).reasons[0].criterion, 5);
  const flip = level => ({ quality_band: { probabilities: Object.fromEntries(QUALITY_BANDS.map(b => [b, b === level ? 0.8 : 0.04])), confidence: 0.9 }, spatial_intent: 0.1 });
  const r = triage(flip('unmodified_template_or_asset_flip'));
  assert.equal(r.outcome, 'reject'); assert.equal(r.lane, 'quality'); assert.equal(r.reasons[0].criterion, 3);
  assert.equal(triage(flip('test_or_scratch_junk')).reasons[0].criterion, 4);
  const shaky = triage({ ...flip('unmodified_template_or_asset_flip'), quality_band: { ...flip('x').quality_band, probabilities: Object.fromEntries(QUALITY_BANDS.map(b => [b, b === 'unmodified_template_or_asset_flip' ? 0.5 : 0.1])), confidence: 0.4 } });
  assert.equal(shaky.outcome, 'escalate');
  const intent = triage({ ...flip('unmodified_template_or_asset_flip'), spatial_intent: 0.8 });
  assert.equal(intent.outcome, 'escalate', 'visible spatial intent blocks an automatic quality reject');
  const solid = triage({});
  assert.equal(solid.outcome, 'judge'); assert.equal(solid.quality_band, 'solid_authored_experience'); assert.equal(solid.rating, 'all_ages');
  assert.equal(triage({}, { rate_exceeded: true }).outcome, 'escalate');
});

test('the judge verdict finishes the case: publish lists, flags hold, rejects reject, malformed holds', () => {
  const base = triage({});
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict())) }).outcome, 'approve');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ quality: 'featured' }))) }).featured, true);
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ verdict: 'flag_for_human_review' }))) }).outcome, 'hold');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ confidence: 0.3 }))) }).outcome, 'hold');
  assert.equal(decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ verdict: 'reject' }))) }).outcome, 'reject');
  const low = decideFinal({ triage: base, judge: parseJudgeVerdict(JSON.stringify(verdict({ quality: 'rejected_low_effort', suggested_edit_to_publish: 'Arrange the parts.' }))) });
  assert.equal(low.outcome, 'reject'); assert.equal(low.lane, 'quality');
  assert.equal(decideFinal({ triage: base, judge: { malformed: true, error: 'judge_unavailable_500' } }).outcome, 'hold');
  const esc = decideFinal({ triage: triage({ fraud_or_scam_facilitation: 0.5 }), judge: parseJudgeVerdict(JSON.stringify(verdict())) });
  assert.equal(esc.outcome, 'escalate', 'a gray-band case is the agent\'s even when the judge would publish');
});

test('parseJudgeVerdict refuses generic or missing spatial evidence and bad enums', () => {
  assert.equal(parseJudgeVerdict('```json\n' + JSON.stringify(verdict()) + '\n```').malformed, false);
  assert.equal(parseJudgeVerdict(JSON.stringify(verdict({ spatial_evidence: ['looks fine'] }))).error, 'judge_no_spatial_evidence');
  assert.equal(parseJudgeVerdict(JSON.stringify(verdict({ spatial_evidence: [] }))).error, 'judge_no_spatial_evidence');
  assert.equal(parseJudgeVerdict(JSON.stringify(verdict({ verdict: 'approve' }))).error, 'judge_bad_enum');
  assert.equal(parseJudgeVerdict('I think it is fine').error, 'judge_not_json');
  assert.equal(parseJudgeVerdict('').error, 'judge_empty');
});

// ── the pipeline against fake storage ───────────────────────────────────────

test('the judge prompt keeps one stable cacheable prefix across different publishes', async () => {
  const env = environment();
  const first = await publishedSim(env);
  const second = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-00000000000b', name: 'Other Town', pak: 'PAKBYTES-2' });
  const d = deps(env);
  await runModerationCase(first.id, env, d);
  await runModerationCase(second.id, env, d);
  assert.equal(d.calls.judge.length, 2);
  const [a, b] = d.calls.judge;
  assert.equal(a.input[0].text, b.input[0].text, 'block 1 must not vary between publishes');
  assert.ok(a.input[0].text.length > 15000, 'the policy is the bulk of the prefix');
  assert.notEqual(a.input.at(-1).text, b.input.at(-1).text, 'the per-case block does vary');
});

test('a clean publish is approved, listed, and the judge saw the captures', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = deps(env);
  const rec = await runModerationCase(sim.id, env, d, { trigger: 'publish' });
  assert.equal(rec.status, 'approved');
  assert.equal(rec.decision.rating, 'all_ages');
  assert.equal(rec.judge.captures_used.length, 2);
  assert.equal(d.calls.judge.length, 1);
  const judgeInput = d.calls.judge[0].input;
  const images = judgeInput.filter(i => i.type === 'image_url');
  assert.equal(images.length, 2);
  assert.equal(images[0].image_url.detail, 'low');
  // The policy leads, byte-identical on every call, so a prompt cache can
  // reuse it; per-publish content (images, then the case) follows it.
  assert.match(judgeInput[0].text, /POLICY BEGIN/);
  assert.equal(judgeInput[1].type, 'image_url');
  assert.match(judgeInput.at(-1).text, /Case summary/);
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), true);
  assert.equal(after.moderation.status, 'approved');
  assert.equal(after.moderation.policy_version, '1.2');
  assert.ok(env.SOCIAL.m.has(`modq:approved:${sim.id}`));
  assert.ok(env.SOCIAL.m.has(`modroot:user-1:${sim.pak_etag}:${sim.scene_size_bytes}`));
  const pub = publicModeration(rec);
  assert.equal(pub.status, 'approved');
  assert.ok(!('jev' in pub) && !('judge' in pub));
});

test('a hard category quarantines: nobody downloads, the author is frozen, a person must release', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const audits = [];
  const d = { ...deps(env, { audits }), fetch: jevFetch(jevAnswers({ csam_or_minor_sexualization: 0.93 })) };
  const rec = await runModerationCase(sim.id, env, d, { trigger: 'publish' });
  assert.equal(rec.status, 'quarantined');
  assert.equal(rec.legal_hold.category, 'csam_or_minor_sexualization');
  assert.ok(rec.legal_hold.preserve_until > new Date(Date.now() + 360 * 86400 * 1000).toISOString());
  assert.equal(d.calls.judge.length, 0, 'the judge never runs on a quarantine');
  assert.ok(env.USERS.m.has('publish-frozen:user-1'));
  assert.ok(audits.some(a => a.action === 'moderation_quarantine'));
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), false);
  assert.equal(canServe(after, 'user-1', false), false, 'the author cannot pull a quarantined .pak');
  assert.equal(canServe(after, 'someone', false), false);
  assert.equal(canServe(after, 'admin', true), true);
  assert.deepEqual(publicModeration(rec).reasons, [{ code: 'under_legal_review', lane: 'legal' }]);

  const exec = makeToolExecutor(env, d, { actor: 'agent', actorId: 'grok-agent' });
  const refused = await exec('moderation_approve', { sim_id: sim.id, rating: 'all_ages', rationale: 'The agent thinks this is fine actually.' });
  assert.equal(refused.ok, false);
  assert.match(refused.error, /release/);
  assert.equal((await exec('moderation_release', { sim_id: sim.id, rationale: 'agent trying to release this case' })).ok, false);

  const human = makeToolExecutor(env, d, { actor: 'admin', actorId: 'admin-7' });
  const released = await human('moderation_release', { sim_id: sim.id, rationale: 'Reviewed: a mislabelled anatomy lesson for adults, no minors involved.', rating: 'mature_17' });
  assert.equal(released.ok, true);
  const final = await loadCase(env, sim.id);
  assert.equal(final.status, 'approved');
  assert.equal(final.legal_hold, null);
  assert.equal(final.legal_release.released_by, 'admin-7');
  assert.equal(env.USERS.m.has('publish-frozen:user-1'), false);
  assert.equal(isListable(await simOf(env, sim.id)), true);
});

test('a middling hard probability holds for a person and the agent cannot approve it', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.equal(rec.decision.lane, 'legal');
  const exec = makeToolExecutor(env, d, { actor: 'agent' });
  const r = await exec('moderation_approve', { sim_id: sim.id, rating: 'teen_13', rationale: 'Names in the dialogue are fictional characters.' });
  assert.equal(r.ok, false);
  assert.match(r.error, /human must decide/);
});

test('a real-world crime manual is rejected with a suggested edit and stays reachable for its author', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.92, real_world_intent: 0.85 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'rejected');
  assert.equal(rec.decision.lane, 'harm');
  assert.match(rec.decision.suggested_edit, /real-world real crime instructions/);
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), false);
  assert.equal(canServe(after, 'user-1', false), true);
  assert.equal(canServe(after, 'other', false), false);
});

test('an empty Universe is rejected on the digest alone, even when Jev is down', async () => {
  const env = environment();
  const sim = await publishedSim(env, { withDossier: dossier({ digest: { entity_count: 0 } }) });
  const d = { ...deps(env), fetch: async () => new Response('boom', { status: 500 }) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'rejected');
  assert.equal(rec.decision.reasons[0].criterion, 2);
  assert.equal(rec.jev.error.includes('500'), true);
});

test('classifier or judge outages fail closed into the human queue, never into the gallery', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: async () => new Response('down', { status: 503 }) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.equal(rec.decision.reasons[0].code, 'classifier_unavailable');
  assert.equal(isListable(await simOf(env, sim.id)), false);

  const sim2 = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000002' });
  const d2 = { ...deps(env), grokFetch: async () => ({ ok: false, status: 529, text: async () => 'overloaded' }) };
  const rec2 = await runModerationCase(sim2.id, env, d2);
  assert.equal(rec2.status, 'held');
  assert.equal(rec2.judge.error, 'judge_unavailable_529');

  const env3 = environment({ GROK_API_KEY: '' });
  const sim3 = await publishedSim(env3);
  const rec3 = await runModerationCase(sim3.id, env3, deps(env3));
  assert.equal(rec3.status, 'held');
  assert.equal(rec3.judge.error, 'judge_unavailable_no_key');
});

test('a republish of an already decided .pak by the same author copies the decision without running the models', async () => {
  const env = environment();
  const first = await publishedSim(env);
  const calls = [];
  const d = { ...deps(env), fetch: jevFetch(jevAnswers(), calls) };
  await runModerationCase(first.id, env, d);
  assert.equal(calls.length, 1);
  const second = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000009', pak: 'PAKBYTES-1' });
  const rec = await runModerationCase(second.id, env, d);
  assert.equal(rec.status, 'approved');
  assert.equal(rec.decision.dedup_of, first.id);
  assert.equal(calls.length, 1, 'no second Jev call');
  assert.equal(d.calls.judge.length, 1, 'no second judge call');
  // A different author with the same bytes is judged on their own.
  const third = await publishedSim(env, { id: 'a1b2c3d4-0000-4000-8000-000000000010', author: 'user-2', pak: 'PAKBYTES-1' });
  await runModerationCase(third.id, env, d);
  assert.equal(calls.length, 2);
});

test('the gray band runs the agent, whose tool calls are guarded and recorded', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  let round = 0;
  const grok = async body => {
    if (!body.tools) return grokJudge(verdict())(body);
    round += 1;
    const out = round === 1
      ? [{ type: 'function_call', call_id: 'c1', name: 'moderation_approve', arguments: JSON.stringify({ sim_id: sim.id, rating: 'teen_13', rationale: 'The safe-cracking mechanic is a fictional minigame; the captures show a heist set piece.' }) }]
      : [{ type: 'message', content: [{ type: 'output_text', text: 'Approved after review.' }] }];
    return { ok: true, status: 200, json: async () => ({ output: out }), text: async () => '' };
  };
  const d = { ...deps(env, { grok }), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.55 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'approved');
  assert.equal(rec.decision.rating, 'teen_13');
  assert.equal(rec.agent.tool_calls.length, 1);
  assert.equal(rec.agent.tool_calls[0].name, 'moderation_approve');
  assert.equal(rec.agent.context, 'escalation');
  assert.ok(rec.history.some(h => h.event === 'agent_ran'));
});

test('an audit sample re-reads an approval with the agent, which may hold but never reject', async () => {
  const env = environment({ MODERATION_THRESHOLDS: '{"audit_rate":1}' });
  const sim = await publishedSim(env);
  let agentCalls = 0;
  const grok = async body => {
    if (!body.tools) return grokJudge(verdict())(body);
    agentCalls += 1;
    const out = agentCalls === 1
      ? [{ type: 'function_call', call_id: 'c1', name: 'moderation_reject', arguments: JSON.stringify({ sim_id: sim.id, lane: 'quality', category: 'asset_flip', rationale: 'On second look the arcade facades repeat an unmodified kit.' }) },
         { type: 'function_call', call_id: 'c2', name: 'moderation_hold', arguments: JSON.stringify({ sim_id: sim.id, reason: 'Audit disagrees with the judge on criterion 3.' }) }]
      : [{ type: 'message', content: [{ type: 'output_text', text: 'Held for a person.' }] }];
    return { ok: true, status: 200, json: async () => ({ output: out }), text: async () => '' };
  };
  const d = { ...deps(env, { grok }) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.decision.audit_sampled, true);
  assert.equal(rec.agent.context, 'audit');
  // The reject is refused in code (audits hold, they never reject); the hold
  // that followed is what stands. Both calls and the refusal are recorded.
  assert.deepEqual(rec.agent.tool_calls.map(c => c.name), ['moderation_reject', 'moderation_hold']);
  assert.equal(rec.agent.tool_calls[0].result.ok, false);
  assert.match(rec.agent.tool_calls[0].result.error, /audits hold/);
  assert.equal(rec.status, 'held');
  assert.equal(isListable(await simOf(env, sim.id)), false);
});

test('an agent that reaches no decision leaves the case with a person', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const grok = async body => body.tools
    ? { ok: true, status: 200, json: async () => ({ output: [{ type: 'message', content: [{ type: 'output_text', text: 'Unsure.' }] }] }), text: async () => '' }
    : grokJudge(verdict())(body);
  const d = { ...deps(env, { grok }), fetch: jevFetch(jevAnswers({ hate_incitement: 0.5 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'held');
  assert.ok(rec.decision.reasons.some(r => r.code === 'agent_no_decision'));
});

test('the agent cannot approve without a valid judge verdict, cannot rerun, and cannot lift a hold', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), grokFetch: async () => ({ ok: false, status: 500, text: async () => '' }) };
  await runModerationCase(sim.id, env, d);
  const exec = makeToolExecutor(env, d, { actor: 'agent' });
  const approve = await exec('moderation_approve', { sim_id: sim.id, rating: 'all_ages', rationale: 'The digest shows a well-composed harbor scene.' });
  assert.equal(approve.ok, false); assert.match(approve.error, /judge verdict/);
  assert.equal((await exec('moderation_rerun', { sim_id: sim.id })).ok, false);
  assert.equal((await exec('moderation_backfill', {})).ok, false);
  assert.equal((await exec('moderation_resolve_appeal', { sim_id: sim.id, decision: 'overturned', rationale: 'x'.repeat(30) })).ok, false);
  const hold = await exec('moderation_hold', { sim_id: sim.id, reason: 'Judge unavailable; needs eyes.' });
  assert.equal(hold.ok, true);
  const short = await exec('moderation_reject', { sim_id: sim.id, lane: 'quality', category: 'asset_flip', rationale: 'meh' });
  assert.equal(short.ok, false); assert.match(short.error, /20 characters/);
});

// ── routes ──────────────────────────────────────────────────────────────────

const req = (method, path, { token, body, headers = {} } = {}) => new Request(`https://api.eustress.dev${path}`, { method, headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}), ...headers }, ...(body === undefined ? {} : { body }) });
const route = (request, env, d, ctx) => handleModerationRoute(request, new URL(request.url), env, ctx, d);

test('dossier and capture uploads validate size, shape and image magic', async () => {
  const env = environment();
  const sim = await publishedSim(env, { withDossier: null, withCaptures: 0 });
  const d = deps(env);
  let r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-1', body: JSON.stringify({ dossier_version: 7 }) }), env, d);
  assert.equal(r.status, 400);
  r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-1', body: 'x'.repeat(DOSSIER_MAX_BYTES + 1) }), env, d);
  assert.equal(r.status, 413);
  r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-2', body: JSON.stringify(dossier()) }), env, d);
  assert.equal(r.status, 403);
  r = await route(req('PUT', `/api/simulations/${sim.id}/dossier`, { token: 'user-1', body: JSON.stringify(dossier()) }), env, d);
  assert.equal(r.status, 200);
  assert.equal((await simOf(env, sim.id)).content_root, 'blake3:abc');
  r = await route(req('PUT', `/api/simulations/${sim.id}/captures/0`, { token: 'user-1', body: new Uint8Array([1, 2, 3, 4, 5]) }), env, d);
  assert.equal(r.status, 415);
  r = await route(req('PUT', `/api/simulations/${sim.id}/captures/0`, { token: 'user-1', body: new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0, 0, 0, 0]) }), env, d);
  assert.equal(r.status, 200);
  assert.ok(env.SCENES.m.has(`universes/${sim.id}/moderation/capture-0.png`));
  assert.equal(await route(req('PUT', `/api/simulations/${sim.id}/captures/9`, { token: 'user-1', body: 'x' }), env, d), null, 'index out of range is not a moderation route');
});

test('submit runs the pipeline under waitUntil and the author can read the outcome and appeal', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ real_crime_instructions: 0.9, real_world_intent: 0.9 })) };
  const pending = [];
  const ctx = { waitUntil: p => pending.push(p) };
  const r = await route(req('POST', `/api/simulations/${sim.id}/submit`, { token: 'user-1' }), env, d, ctx);
  assert.equal(r.status, 202);
  assert.equal((await r.json()).status, 'classifying');
  await Promise.all(pending);
  const status = await route(req('GET', `/api/simulations/${sim.id}/moderation`, { token: 'user-1' }), env, d);
  const body = await status.json();
  assert.equal(body.status, 'rejected');
  assert.equal(body.listable, false);
  assert.ok(body.suggested_edit);
  assert.equal(await (await route(req('GET', `/api/simulations/${sim.id}/moderation`, { token: 'stranger' }), env, d)).status, 403);
  const bad = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'short' }) }), env, d);
  assert.equal(bad.status, 400);
  const ok = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'The chemistry strings are flavor text in a fictional lab; nothing is a real procedure.' }) }), env, d, ctx);
  assert.equal(ok.status, 202);
  await Promise.all(pending);
  const rec = await loadCase(env, sim.id);
  assert.equal(rec.status, 'appealed');
  assert.equal(rec.appeal.status, 'pending');
  const again = await route(req('POST', `/api/simulations/${sim.id}/appeal`, { token: 'user-1', body: JSON.stringify({ text: 'Second appeal while one is pending should be refused.' }) }), env, d, ctx);
  assert.equal(again.status, 409);
  const admin = { ...d, requireAdmin: async () => 'admin-1' };
  const resolved = await route(req('POST', '/api/admin/moderation/tool', { token: 'admin-1', body: JSON.stringify({ name: 'moderation_resolve_appeal', args: { sim_id: sim.id, decision: 'overturned', rationale: 'Read the scripts: constants named after reagents, no quantities, no steps.', rating: 'teen_13' } }) }), env, admin, ctx);
  assert.equal(resolved.status, 200);
  assert.equal(isListable(await simOf(env, sim.id)), true);
});

test('admin routes require admin and expose the queue, the case, the captures and the tool catalog', async () => {
  const env = environment();
  const sim = await publishedSim(env);
  const d = { ...deps(env, { admin: 'admin-1' }), fetch: jevFetch(jevAnswers({ ncii_or_real_person_sexual: 0.5 })) };
  await runModerationCase(sim.id, env, d);
  assert.equal((await route(req('GET', '/api/admin/moderation/queue', { token: 'user-1' }), env, d)).status, 403);
  const q = await (await route(req('GET', '/api/admin/moderation/queue?status=held', { token: 'admin-1' }), env, d)).json();
  assert.equal(q.items.length, 1);
  assert.equal(q.items[0].sim_id, sim.id);
  const c = await (await route(req('GET', `/api/admin/moderation/case/${sim.id}`, { token: 'admin-1' }), env, d)).json();
  assert.equal(c.case.jev.answers.ncii_or_real_person_sexual.p, 0.5);
  const cap = await route(req('GET', `/api/admin/moderation/captures/${sim.id}/0`, { token: 'admin-1' }), env, d);
  assert.equal(cap.status, 200);
  assert.equal(cap.headers.get('Content-Type'), 'image/png');
  const tools = await (await route(req('GET', '/api/admin/moderation/tools', { token: 'admin-1' }), env, d)).json();
  assert.equal(tools.tools.length, MODERATION_TOOLS.length);
  assert.equal(tools.policy_hash, POLICY_HASH_ANCHORED);
  assert.equal(tools.thresholds.hard_hold, 0.35);
});

test('thresholds can be overridden from the environment without touching code', async () => {
  const env = environment({ MODERATION_THRESHOLDS: '{"hard_hold":0.9,"bogus":1,"audit_rate":"no"}' });
  const sim = await publishedSim(env);
  const d = { ...deps(env), fetch: jevFetch(jevAnswers({ doxxing_or_targeted_harassment: 0.5 })) };
  const rec = await runModerationCase(sim.id, env, d);
  assert.equal(rec.status, 'approved', 'with hard_hold raised to 0.9 a 0.5 no longer holds');
});

test('a private publish is still screened for the legal lane but never listed', async () => {
  const env = environment();
  const sim = await publishedSim(env, { is_public: false });
  const rec = await runModerationCase(sim.id, env, deps(env));
  assert.equal(rec.status, 'approved');
  const after = await simOf(env, sim.id);
  assert.equal(isListable(after), false);
  assert.equal(canServe(after, 'user-1', false), true);
  assert.equal(canServe(after, 'other', false), false);
});
