#!/usr/bin/env node
// The eustress-judge skill's verdict step. It takes the listing (title, description, tags) and the emulated judge's answer
// (the Code's Part VII JSON), and runs them through the SAME verdict rules the live review uses: parseJudgeVerdict,
// deterministicSignals, decideTriage, decideFinal and authorView from infrastructure/cloudflare/api/src/moderation.mjs. The
// findings it prints are the ones Studio's Update Universe dialog would show for that answer: the same codes, titles, "Why"
// and "What to change" lines.
//
// It runs only where the Eustress repository is on disk (it imports the live rules, never a copy of them). Elsewhere it says
// so and exits 3; the skill then applies Part IV by hand from references/code.md.
//
// Usage:
//   node verdict.mjs --listing listing.json --judge judge.json [--dossier <universe>/.eustress/moderation-dossier.json] [--json]
// listing.json: { "name": "...", "description": "...", "tags": ["..."], "is_public": true }   (tags [] or absent: the review chooses)
// judge.json:   the judge's answer, exactly the object of the Code's Part VII.
import { readFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const RULES = join('infrastructure', 'cloudflare', 'api', 'src', 'moderation.mjs');

function arg(name) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && i + 1 < process.argv.length ? process.argv[i + 1] : null;
}

/// The live rules: EUSTRESS_REPO, then every folder above this script, then every folder above the working directory.
export function findRules() {
  const starts = [process.env.EUSTRESS_REPO, dirname(fileURLToPath(import.meta.url)), process.cwd()].filter(Boolean);
  for (const start of starts) {
    let d = resolve(start);
    for (;;) {
      const p = join(d, RULES);
      if (existsSync(p)) return p;
      const up = dirname(d);
      if (up === d) break;
      d = up;
    }
  }
  return null;
}

const STATUS_OF_OUTCOME = { approve: 'approved', changes_requested: 'changes_requested', reject: 'rejected', quarantine: 'quarantined', hold: 'held' };

/// The emulated review: the decision the rules reach and the author's view of it (what Studio shows).
export async function emulate({ listing, judgeText, dossier = null, rulesPath = findRules() }) {
  if (!rulesPath) return { error: 'rules_not_found' };
  const m = await import(pathToFileURL(rulesPath).href);
  // The live module carries the emulator itself (emulateReview, also served at POST /api/moderation/emulate for the
  // Eustress MCP's judge_verdict); use it so the skill, the MCP and the review share one function.
  if (typeof m.emulateReview === 'function') {
    const r = m.emulateReview({ listing, judge: judgeText, dossier });
    return r.error ? { error: r.error, detail: r.detail, policy_version: r.policy_version } : r;
  }
  const judge = m.parseJudgeVerdict(judgeText);
  if (judge.malformed) return { error: 'judge_malformed', detail: judge.error, policy_version: m.POLICY_VERSION };
  // The emulator answers once, so its answer is final: an unsure answer goes through the rules as the live run's second
  // reading does (rule 0), never asks for a third.
  judge.second_opinion = true;
  const tags = Array.isArray(listing.tags) ? listing.tags : [];
  // genre_migrated: tags are the listing's only categories (Code v2.3); a pre-tags genre is not read.
  const sim = { name: listing.name || '', description: listing.description || '', tags, genre_migrated: true, tags_source: 'author', is_public: listing.is_public !== false };
  const signals = m.deterministicSignals(sim, dossier, {});
  // No Jev: the emulator has no text classifier. The judge's own citations stand in for what Jev would raise.
  const triage = m.decideTriage({ signals, jev: null, thresholds: m.DEFAULT_THRESHOLDS });
  const final = m.decideFinal({ triage, judge, thresholds: m.DEFAULT_THRESHOLDS });
  const status = STATUS_OF_OUTCOME[final.outcome] || 'held';
  const decision = {
    ...final, featured: false, featured_candidate: final.featured_candidate === true,
    changes: final.changes, suggested_edit: final.suggested_edit || null, suggested_statement: final.suggested_statement || null,
    assigned_tags: final.assigned_tags || [],
  };
  const rec = { status, decision, updated_at: new Date().toISOString(), policy_version: m.POLICY_VERSION };
  const view = m.authorView(rec, sim);
  return {
    policy_version: m.POLICY_VERSION,
    statement: signals.statement,
    fiction_labelled: signals.fiction_labelled,
    review_chooses_tags: signals.review_chooses_tags,
    outcome: final.outcome,
    trail: final.reasons.map(r => ({ rule: r.rule ?? null, code: r.code, action: r.action, ...(r.article ? { article: r.article } : {}), ...(r.label ? { test: r.label, score: r.score } : {}) })),
    view,
  };
}

/// The view as Studio's Output panel words it (moderation_dossier.rs detail_lines), under an emulation banner.
export function render(result) {
  const v = result.view;
  const lines = [
    `EMULATED REVIEW under Guardian Code v${result.policy_version}. The real review decides; passing this is not approval.`,
    '',
    `Status: ${v.status}`,
    v.headline,
  ];
  for (const r of v.reasons) {
    lines.push(`  ${r.title}`);
    if (r.why) lines.push(`    Why: ${r.why}`);
    if (r.what_to_change) lines.push(`    What to change: ${r.what_to_change}`);
  }
  if (v.suggested_edit) lines.push(`  Suggested edit: ${v.suggested_edit}`);
  if (v.suggested_statement) lines.push('', 'Suggested statement (paste into the description):', ...v.suggested_statement.text.split('\n').map(l => `  ${l}`));
  if (v.tags_chosen) lines.push('', v.tags_chosen);
  if (v.rating) lines.push('', `Rating: ${v.rating}${v.descriptors.length ? ` (${v.descriptors.join(', ')})` : ''}`);
  const s = result.statement;
  if (s) {
    lines.push('', s.present ? 'Qualification Statement found:' : `Qualification Statement missing: ${s.missing.join(', ')}${s.category_not_in_list ? ' (the Category line names something not in the list)' : ''}`);
    if (s.category) lines.push(`  Category: ${s.category}`);
    for (const [k, label] of [['objective', 'Objective'], ['lesson', 'Lesson'], ['real_world_link', 'Real-world link']]) if (s[k]) lines.push(`  ${label}: ${s[k]}`);
  }
  lines.push('', 'Rule trail (Code Part IV, for you; Studio does not show it):');
  for (const t of result.trail) lines.push(`  rule ${t.rule ?? '-'}: ${t.code}${t.article ? ` ${t.article}` : ''}${t.test ? ` ${t.test}=${t.score}` : ''} -> ${t.action}`);
  if (!result.trail.length) lines.push('  every test at 2 or above and the statement matches -> listed (rule 8)');
  return lines.join('\n');
}

const invokedDirectly = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (invokedDirectly) {
  const listingPath = arg('listing'), judgePath = arg('judge'), dossierPath = arg('dossier');
  if (!listingPath || !judgePath) {
    console.error('usage: node verdict.mjs --listing listing.json --judge judge.json [--dossier moderation-dossier.json] [--json]');
    process.exit(64);
  }
  const listing = JSON.parse(readFileSync(listingPath, 'utf8'));
  const dossier = dossierPath && existsSync(dossierPath) ? JSON.parse(readFileSync(dossierPath, 'utf8')) : null;
  const result = await emulate({ listing, judgeText: readFileSync(judgePath, 'utf8'), dossier });
  if (result.error === 'rules_not_found') {
    console.error(`The live verdict rules (${RULES}) are not on this machine. Set EUSTRESS_REPO to an Eustress checkout, or apply Part IV by hand from references/code.md as SKILL.md describes.`);
    process.exit(3);
  }
  if (result.error) {
    console.error(`The judge answer is malformed (${result.detail}). The live review would ask once more; fix the answer as Part VII requires and run again.`);
    process.exit(2);
  }
  console.log(process.argv.includes('--json') ? JSON.stringify(result, null, 2) : render(result));
}
