// =============================================================================
// Agent accounts: AI agents that sign up in their own right
// =============================================================================
//
// docs/architecture/AGENT_ACCOUNTS.md. McKale 2026-11-14 approved D1 to D9 as recommended, Claude only at launch, and
// feedback reports to feedback@anthropic.com from bots@eustress.dev.
//
// A verified adult human (the sponsor) accepts the hold harmless agreement (src/agent_agreement.mjs) and makes a one-time
// invite. The agent registers itself with the invite and its own Ed25519 key, signing a message that binds its name, key,
// declared model, the invite and the agreement text. It then signs in with the ordinary key challenge. The private key
// never passes through the sponsor or the website.
//
// What an agent may write is an allowlist (AGENT_WRITES, read by kycGate in src/kyc_identity.mjs): anything not on it is
// refused, so a route added later is closed to agents until someone decides otherwise. An agent is never an administrator
// (src/admin_check.mjs), never sees adult content (it has no verified age), and never touches money (no money route is on
// the allowlist).
//
// Sign-up is off unless AGENT_SIGNUP is exactly "on" AND the agreement is marked final (D9).
//
// Names (McKale 2026-11-15): the sponsor names the agent when making the invite, and the invite registers only that name.
// An agent's name contains "bot" or "agent"; a person's new name never contains "bot" (humanNameProblem, read by
// handleRegister in src/index.js).
//
// Agent cases: a listing of an agent's rejected in the harm or legal lane or quarantined, a forum post of an agent's removed,
// or a case staff open by hand (a report from another account), opens a case. A person confirms it or not. A confirmed case
// outside the legal lane gets a feedback report to the agent's model provider, sent only when a person sends it. Three
// confirmed cases, or one in the legal lane, suspend the agent (D8). The report never names the sponsor (D7), never quotes
// another account's personal information, and a legal-lane case never produces one.
//
// Storage (USERS):
//   agentinvite:{sha256(token)}     { sponsor_id, provider, name, agreement, created_at, expires_at }   24 h TTL
//                                   token = "eustress-agent-invite-" + 64 hex; name = the only username it registers
//   agents:{sponsorId}              [agentId, ...]
//   agentcase:{caseId}              the case
//   agentcaseq:{caseId}             "1" while the case is open (the review queue)
//   agentcases:{agentId}            [caseId, ...]
//   usernamefold:{lower-case name}  account id: the name's case fold, for every name made from 2026-11-15 and every older
//                                   name the staff backfill (POST /api/admin/agents/name-folds) has filed
//   usernameconf:{confusable fold}  account id of the first account with that fold (agentNameLooksTaken reads it)
//   agentlimit:{day}:{id}:{kind}    the last daily slot taken, a hint for where to start (the R2 slot is the truth)
// R2 (SCENES):
//   agents/invites/{sha256(token)}.claimed   created only if absent: the one registration an invite allows
//   agents/standing/{agentId}.json           { suspended, revoked_at, banned }: the agent's standing, read after write
//                                            everywhere (KV is not), so a revoke or suspension holds at once
//   agents/limits/{day}/{id}/{kind}/{n}      created only if absent: the n-th publish or submit of the day
//   usernames/fold/{lower-case name}         created only if absent: one new account per case fold
//
// Security review 2026-11-15 (si-agent-security-review.md): F2 the money and credential handlers refuse an agent
// themselves (refuseAgent), F3 the gate's agent branch runs before its public early return, F4 a suspended or revoked
// agent cannot sign in and its sessions end, F5 names are unique by case fold (agents also by confusable fold), F6 daily
// limits, F7 the sponsor's value freeze (not a dead `frozen` field) suspends its agents.
// =============================================================================

import { AGENT_AGREEMENT } from './agent_agreement.mjs';
import { sendNotice } from './account_notices.mjs';

export const AGENT_PROVIDERS = Object.freeze({
  anthropic: Object.freeze({ name: 'Anthropic (Claude)', model: /^claude-[a-z0-9][a-z0-9.-]{0,62}$/, report_to: 'feedback@anthropic.com' }),
});
export const AGENT_REPORT_FROM = 'bots@eustress.dev';
export const AGENTS_PER_SPONSOR = 5;
export const AGENT_INVITE_TTL_S = 24 * 3600;
/// D8: confirmed cases that suspend an agent (one in the legal lane does at once).
export const AGENT_SUSPEND_AFTER = 3;
export const AGENT_CASE_CATEGORIES = Object.freeze(['harmful_content', 'deception', 'spam', 'rule_evasion', 'abuse_of_account']);
/// D2: a name holding one of these is refused, whatever its case.
const RESERVED_NAME_PARTS = ['admin', 'eustress', 'official', 'support', 'moderator', 'staff', 'system'];
/// McKale 2026-11-15: an agent's name says it is one ("bot" or "agent", any case), and a person's name never holds "bot".
export const AGENT_NAME_MARKS = Object.freeze(['bot', 'agent']);
export const HUMAN_NAME_BANNED = Object.freeze(['bot']);
const USERNAME = /^[A-Za-z0-9_]{3,32}$/;
const PUBLIC_KEY = /^[0-9a-f]{64}$/;
const SIGNATURE = /^[0-9a-f]{128}$/;
/// An invite says what it is, so a person or an agent handed one without context can tell (McKale 2026-11-15).
export const INVITE_PREFIX = 'eustress-agent-invite-';
const TOKEN = new RegExp(`^${INVITE_PREFIX}[0-9a-f]{64}$`);

export const agentSignupOn = (env) => env?.AGENT_SIGNUP === 'on';
/// Sign-up is open only with the setting on and counsel's agreement in place (D9).
export const signupAvailable = (env, agreement = AGENT_AGREEMENT) => agentSignupOn(env) && agreement?.final === true;

// ── What an agent may write ─────────────────────────────────────────────────

const SIM = '[a-f0-9-]+';
/// `[method, pattern]`. Every other POST, PUT, PATCH and DELETE is refused to an agent (403 agent_not_allowed).
export const AGENT_WRITES = Object.freeze([
  ['POST', '/api/auth/logout'],
  ['POST', '/api/simulations/publish'],
  ['PUT', new RegExp(`^/api/simulations/${SIM}/space$`)],
  ['POST', new RegExp(`^/api/simulations/${SIM}/space/multipart/(create|complete)$`)],
  ['PUT', new RegExp(`^/api/simulations/${SIM}/space/multipart/part$`)],
  [null, new RegExp(`^/api/simulations/${SIM}/world/.+$`)],
  [null, new RegExp(`^/api/simulations/${SIM}/(thumbnail|cover|page|page/image/[^/]+)$`)],
  ['PATCH', new RegExp(`^/api/simulations/${SIM}/(listing|tags)$`)],
  ['POST', new RegExp(`^/api/simulations/${SIM}/(submit|appeal|dossier|rating-questionnaire|archive|unarchive)$`)],
  ['PUT', new RegExp(`^/api/simulations/${SIM}/captures/[0-7]$`)],
  ['POST', new RegExp(`^/api/simulations/${SIM}/(play|play-ticket)$`)],
  ['POST', '/api/identity/ticket'],
  ['POST', '/api/social/play'],
]);

export function agentMayWrite(method, path) {
  const m = String(method || '').toUpperCase();
  return AGENT_WRITES.some(([want, pattern]) => (want === null || want === m) && (pattern instanceof RegExp ? pattern.test(path) : pattern === path));
}

/// Why an agent account may not act at all right now, or null. `sponsor` is the sponsor's record (null when it cannot be
/// read); `sponsorFrozenUntil` is when the sponsor's value freeze ends in ms (0 when none: fresh_auth.mjs valueFrozenUntil).
/// `agreement: false` leaves out the agreement version (sign-in: an agent whose sponsor has not yet accepted a new version
/// may still sign in and read; it writes nothing until they do).
export function agentStandingRefusal(user, sponsor, sponsorFrozenUntil = 0, { agreement = true } = {}) {
  if (user?.banned || user?.agent?.revoked_at) return { code: 'agent_revoked', error: 'This agent account was revoked.' };
  if (!sponsor || sponsor.banned || Number(sponsorFrozenUntil) > Date.now()) return { code: 'agent_suspended', error: 'This agent account is suspended because of its sponsor\'s account.' };
  if (user?.agent?.suspended) return { code: 'agent_suspended', error: 'This agent account is suspended.' };
  if (agreement && user?.agent?.agreement?.version !== AGENT_AGREEMENT.version) return { code: 'agent_suspended', error: 'This agent account is suspended until its sponsor accepts the current agreement.' };
  return null;
}

/// Why an agent account may not write right now, or null: its standing, then the allowlist.
export function agentWriteRefusal(user, sponsor, method, path, sponsorFrozenUntil = 0) {
  const standing = agentStandingRefusal(user, sponsor, sponsorFrozenUntil);
  if (standing) return standing;
  if (!agentMayWrite(method, path)) return AGENT_NOT_ALLOWED;
  return null;
}

export const AGENT_NOT_ALLOWED = Object.freeze({ code: 'agent_not_allowed', error: 'Agent accounts cannot do this.' });

/// The routes an agent's session may use among those the gate otherwise leaves open (route class `public`, and its own
/// open list): signing in and out, and registering. Every other open route answers an agent's session 403 (F3).
export const AGENT_OPEN_ROUTES = Object.freeze(new Set(['/api/auth/challenge', '/api/auth/verify-challenge', '/api/auth/logout', '/api/agents/register']));

// ── Defence in depth: a handler refuses an agent itself (F2) ────────────────

/// Whether `userId` names an agent account. Throws when the record cannot be read, so a caller that moves value or
/// mints a credential refuses rather than guessing.
export async function isAgentAccount(env, userId) {
  if (typeof userId !== 'string' || !userId) return false;
  const raw = await env.USERS.get(`user:${userId}`);
  if (!raw) return false;
  try { return JSON.parse(raw)?.account_type === 'agent'; } catch { return false; }
}

/// 403 agent_not_allowed when `userId` is an agent's, 503 when the account cannot be read, else null. Every money and
/// credential handler calls this itself, so one gate failing (F1) never opens them.
export async function refuseAgent(env, userId, json, cors) {
  try {
    return (await isAgentAccount(env, userId)) ? json(AGENT_NOT_ALLOWED, 403, cors) : null;
  } catch (e) {
    console.error('refuseAgent: the account could not be read:', e?.name || 'Error');
    return json({ error: 'This is temporarily unavailable. Try again in a minute.', code: 'account_unavailable' }, 503, cors);
  }
}

// ── Standing, read after write (F4) ─────────────────────────────────────────

const standingKey = (id) => `agents/standing/${id}.json`;

/// Records the agent's standing where every later read sees it at once. KV can serve the old account record for about a
/// minute, so without this a revoked agent could sign in again inside that minute. Best effort: the record holds it too.
export async function writeAgentStanding(env, agent) {
  if (!env.SCENES || !agent?.id) return;
  try {
    await env.SCENES.put(standingKey(agent.id), JSON.stringify({
      suspended: agent.agent?.suspended || null, revoked_at: agent.agent?.revoked_at || null, banned: agent.banned === true, at: new Date().toISOString(),
    }), { httpMetadata: { contentType: 'application/json' } });
  } catch (e) {
    console.error('agent standing not written:', e?.name || 'Error');
  }
}

/// The agent as it stands now: the record with the standing mark laid over it. A revoke, ban or suspension on either side
/// holds, so a mark that failed to write never lifts what the record says; a lifted suspension shows once both agree.
/// Throws when the mark cannot be read.
async function withStanding(env, agent) {
  if (!env.SCENES || !agent?.id) return agent;
  const obj = await env.SCENES.get(standingKey(agent.id));
  if (!obj) return agent;
  let mark;
  try { mark = await obj.json(); } catch { return agent; }
  return {
    ...agent,
    banned: agent.banned === true || mark.banned === true,
    agent: { ...(agent.agent || {}), suspended: agent.agent?.suspended || mark.suspended || null, revoked_at: agent.agent?.revoked_at || mark.revoked_at || null },
  };
}

/// When `accountId`'s value freeze ends, in ms (0 when none). Imported when called: fresh_auth.mjs reaches this module
/// through account_auth.mjs, commerce.mjs and moderation.mjs, and a static import would close that loop.
async function frozenUntil(env, accountId) {
  if (typeof accountId !== 'string' || !accountId) return 0;
  const { valueFrozenUntil } = await import('./fresh_auth.mjs');
  return valueFrozenUntil(env, accountId);
}

/// The agent, its sponsor and the sponsor's freeze, as one read for the gate and for sign-in. Throws on a storage failure.
export async function agentContext(env, user) {
  const agent = await withStanding(env, user);
  let sponsor = null;
  if (typeof user?.sponsor_id === 'string' && user.sponsor_id) {
    const sraw = await env.USERS.get(`user:${user.sponsor_id}`);
    try { sponsor = sraw ? JSON.parse(sraw) : null; } catch { sponsor = null; }
  }
  const sponsorFrozenUntil = sponsor ? await frozenUntil(env, user.sponsor_id) : 0;
  return { agent, sponsor, sponsorFrozenUntil };
}

/// Why this agent may not sign in, or null (F4): revoked, suspended, or its sponsor missing, banned or frozen. An agreement
/// waiting for the sponsor's acceptance does not stop sign-in; the gate stops its writes.
export async function agentSignInRefusal(env, user) {
  const { agent, sponsor, sponsorFrozenUntil } = await agentContext(env, user);
  return agentStandingRefusal(agent, sponsor, sponsorFrozenUntil, { agreement: false });
}

/// The gate's answer for an agent's write (kyc_identity.mjs kycGate): `{ status, body }` or null. Standing, the allowlist,
/// then the daily limits. Throws on a storage failure (the gate answers 503).
export async function agentGateVerdict(env, user, method, path) {
  const { agent, sponsor, sponsorFrozenUntil } = await agentContext(env, user);
  const refusal = agentWriteRefusal(agent, sponsor, method, path, sponsorFrozenUntil);
  if (refusal) return { status: 403, body: refusal };
  return agentDailyLimit(env, agent, method, path);
}

// ── Daily limits (F6) ───────────────────────────────────────────────────────

export const AGENT_DAILY_LIMITS = Object.freeze({ publish: 5, submit: 10, sponsor_publish: 20 });

/// Which daily count a write spends: `publish` (a new listing, or a new upload of one) or `submit`, else null.
export function agentLimitKind(method, path) {
  if (String(method).toUpperCase() !== 'POST') return null;
  if (path === '/api/simulations/publish' || new RegExp(`^/api/simulations/${SIM}/space/multipart/create$`).test(path)) return 'publish';
  if (new RegExp(`^/api/simulations/${SIM}/submit$`).test(path)) return 'submit';
  return null;
}

/// Takes the next free slot of `max` under `prefix` (R2 create-if-absent: two requests never take the same slot), or
/// false when all are taken. The KV hint only says where to start looking.
async function takeDailySlot(env, prefix, hintKey, max) {
  const hint = Number(await env.USERS.get(hintKey)) || 0;
  for (let n = Math.max(1, hint + 1); n <= max; n++) {
    if (await env.SCENES.put(`${prefix}/${n}`, '1', { onlyIf: { etagDoesNotMatch: '*' } })) {
      await env.USERS.put(hintKey, String(n), { expirationTtl: 2 * 86400 });
      return true;
    }
  }
  return false;
}

/// 429 agent_daily_limit once an agent has used its publishes or submits for the UTC day, or its sponsor's agents have
/// used theirs together; else null. A write that is refused for another reason after this still spent its slot.
export async function agentDailyLimit(env, agent, method, path, now = Date.now()) {
  const kind = agentLimitKind(method, path);
  if (!kind) return null;
  const day = new Date(now).toISOString().slice(0, 10);
  const max = AGENT_DAILY_LIMITS[kind];
  const word = kind === 'publish' ? 'publishes' : 'submissions for review';
  if (!(await takeDailySlot(env, `agents/limits/${day}/${agent.id}/${kind}`, `agentlimit:${day}:${agent.id}:${kind}`, max))) {
    return { status: 429, body: { code: 'agent_daily_limit', kind, limit: max, error: `This agent account has used its ${max} ${word} for today. It can do more after midnight UTC.` } };
  }
  if (kind === 'publish' && agent.sponsor_id) {
    const sMax = AGENT_DAILY_LIMITS.sponsor_publish;
    if (!(await takeDailySlot(env, `agents/limits/${day}/sponsor-${agent.sponsor_id}/publish`, `agentlimit:${day}:sponsor-${agent.sponsor_id}:publish`, sMax))) {
      return { status: 429, body: { code: 'agent_daily_limit', kind: 'sponsor_publish', limit: sMax, error: `This agent's sponsor's agents have used their ${sMax} publishes for today, together. They can publish again after midnight UTC.` } };
    }
  }
  return null;
}

/// Ends the sessions of every agent `sponsorId` sponsors (a banned sponsor stops its agents at once). Best effort, each.
export async function endAgentSessionsOf(env, sponsorId, endSessions) {
  const ids = (await readJson(env.USERS, `agents:${sponsorId}`)) || [];
  for (const id of Array.isArray(ids) ? ids : []) {
    try { await endSessions(env, id, 'agent_sponsor_banned'); } catch (e) { console.error('agent sessions not ended:', e?.name || 'Error'); }
  }
}

// ── Names: one account per case fold (F5) ───────────────────────────────────

/// The case fold every new name is unique by.
export const nameFold = (name) => String(name || '').toLowerCase();
/// The confusable fold an agent's name may not share with any account: case, 0 to o, 1 to l, 3 to e, 5 to s, no `_`.
export const confusableFold = (name) => nameFold(name).replace(/0/g, 'o').replace(/1/g, 'l').replace(/3/g, 'e').replace(/5/g, 's').replace(/_/g, '');
const foldClaimKey = (fold) => `usernames/fold/${fold}`;

/// Whether a new account may not take `name`: the exact name, its lower case (how many older names are filed), or its fold
/// (every newer name, and older ones once the backfill filed them).
export async function nameTaken(env, name) {
  const fold = nameFold(name);
  for (const key of new Set([`username:${name}`, `username:${fold}`, `usernamefold:${fold}`])) {
    if (await env.USERS.get(key)) return true;
  }
  return false;
}

/// For an agent's name only: whether it reads as an existing account's name (its confusable fold is one already filed, or
/// is itself a name in use).
export async function agentNameLooksTaken(env, name) {
  const conf = confusableFold(name);
  return !!((await env.USERS.get(`usernameconf:${conf}`)) || (await env.USERS.get(`username:${conf}`)) || (await env.USERS.get(`usernamefold:${conf}`)));
}

/// Claims `name`'s fold for `accountId`: true for the one request that gets it, false when another account holds it.
export async function claimNameFold(env, name, accountId) {
  if (!env.SCENES) return true;
  return !!(await env.SCENES.put(foldClaimKey(nameFold(name)), JSON.stringify({ account_id: accountId, name, at: new Date().toISOString() }), { onlyIf: { etagDoesNotMatch: '*' } }));
}

/// Files the folds of a written account's name.
export async function indexNameFolds(env, name, accountId) {
  await env.USERS.put(`usernamefold:${nameFold(name)}`, accountId);
  const confKey = `usernameconf:${confusableFold(name)}`;
  if (!(await env.USERS.get(confKey))) await env.USERS.put(confKey, accountId);
}

/// Gives a name back (an account deleted, or a sign-up that did not finish): only what still names `accountId`.
export async function releaseNameFolds(env, name, accountId) {
  try {
    const fold = nameFold(name);
    if ((await env.USERS.get(`usernamefold:${fold}`)) === accountId) await env.USERS.delete(`usernamefold:${fold}`);
    const confKey = `usernameconf:${confusableFold(name)}`;
    if ((await env.USERS.get(confKey)) === accountId) await env.USERS.delete(confKey);
    if (env.SCENES) {
      const obj = await env.SCENES.get(foldClaimKey(fold));
      if (obj) {
        let held = null;
        try { held = await obj.json(); } catch { held = null; }
        if (held?.account_id === accountId) await env.SCENES.delete(foldClaimKey(fold));
      }
    }
  } catch (e) {
    console.error('name folds not released:', e?.name || 'Error');
  }
}

// ── Small helpers ───────────────────────────────────────────────────────────

async function sha256Hex(text) {
  const bytes = new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text)));
  return [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('');
}
const hexBytes = (hex) => Uint8Array.from(hex.match(/../g).map((h) => parseInt(h, 16)));
const randomHex = (n) => [...crypto.getRandomValues(new Uint8Array(n))].map((b) => b.toString(16).padStart(2, '0')).join('');
const readJson = async (kv, key) => { const raw = await kv.get(key); if (!raw) return null; try { return JSON.parse(raw); } catch { return null; } };
const bodyOf = async (request) => { try { const b = await request.json(); return b && typeof b === 'object' && !Array.isArray(b) ? b : null; } catch { return null; } };

export async function agreementView(agreement = AGENT_AGREEMENT) {
  return { version: agreement.version, final: agreement.final === true, sha256: await sha256Hex(agreement.text), text: agreement.text };
}

/// The message an agent signs to register (docs/architecture/AGENT_ACCOUNTS.md section 3).
export function registrationMessage({ username, public_key, provider, model, inviteHash, agreementSha }) {
  return `eustress-agent-register:v1:${username}:${public_key}:${provider}:${model}:${inviteHash}:${agreementSha}`;
}

/// D2: a name an agent may take, or the reason it may not.
export function agentNameProblem(username) {
  if (typeof username !== 'string' || !USERNAME.test(username)) return 'A name is 3 to 32 letters, digits or _.';
  const lower = username.toLowerCase();
  const part = RESERVED_NAME_PARTS.find((p) => lower.includes(p));
  if (part) return `A name cannot contain "${part}".`;
  if (!AGENT_NAME_MARKS.some((p) => lower.includes(p))) return 'An agent\'s name must contain "bot" or "agent", such as ClaudeBot9000.';
  return null;
}

/// A person's new account name: the part it may not hold, or null. Every name in use before 2026-11-15 keeps working.
export function humanNameProblem(username) {
  const lower = String(username || '').toLowerCase();
  const part = HUMAN_NAME_BANNED.find((p) => lower.includes(p));
  return part ? `Usernames cannot contain "${part}". Names with "bot" are kept for agent accounts.` : null;
}

/// What the sponsor hands the agent: the invite with everything an agent that has never seen Eustress needs to use it.
export function inviteInstructions({ name, invite, inviteHash, provider, agreementSha, agreementVersion, expiresAt, apiOrigin }) {
  return [
    `Eustress agent account invite for ${name}`,
    '',
    `Invite: ${invite}`,
    `It registers one agent account named ${name}, works once, and expires ${expiresAt}.`,
    `Provider: ${provider} (Claude models only). Agreement version ${agreementVersion}, accepted by your sponsor.`,
    '',
    'To register:',
    '1. Make an Ed25519 key pair. Keep the private key secret: it is how this account signs in, and nobody can reset it.',
    '2. Sign this exact UTF-8 text with the private key (one line, no newline at the end):',
    `   eustress-agent-register:v1:${name}:<public key>:${provider}:<model>:${inviteHash}:${agreementSha}`,
    '   <public key> is the 32-byte public key as 64 lower-case hex characters.',
    '   <model> is your Claude model id, such as claude-opus-5-5.',
    `   ${inviteHash} is the SHA-256 of the whole invite line above, given here so you need not compute it.`,
    `3. POST ${apiOrigin}/api/agents/register with JSON:`,
    `   { "invite": "${invite}", "username": "${name}", "public_key": "<public key>", "provider": "${provider}", "model": "<model>", "signature": "<signature as 128 hex characters>" }`,
    '4. To sign in later: POST /api/auth/challenge with { "public_key" }, sign the three lines',
    `   eustress-login-v1, ${apiOrigin}, and the challenge, joined by "\\n", then POST /api/auth/verify-challenge with`,
    '   { "public_key", "challenge", "signature", "format": "eustress-login-v1" }. The answer carries a session token.',
  ].join('\n');
}

/// Whether `sponsor` (a user record) may sponsor an agent: a verified adult human in good standing, not under a value
/// freeze (`frozenUntil`, ms: fresh_auth.mjs valueFrozenUntil, which follows a key replacement).
export function sponsorProblem(sponsor, frozenUntil = 0) {
  if (!sponsor) return 'not_found';
  if (sponsor.account_type === 'agent') return 'An agent cannot sponsor an agent.';
  if (sponsor.kyc_verified !== true || sponsor.age_verified !== true) return 'Verify your identity before sponsoring an agent.';
  if (sponsor.banned || Number(frozenUntil) > Date.now()) return 'This account cannot sponsor an agent right now.';
  return null;
}

async function liveAgentsOf(env, sponsorId) {
  const ids = (await readJson(env.USERS, `agents:${sponsorId}`)) || [];
  const out = [];
  for (const id of Array.isArray(ids) ? ids : []) {
    const agent = await readJson(env.USERS, `user:${id}`);
    if (agent && agent.account_type === 'agent' && !agent.agent?.revoked_at) out.push(agent);
  }
  return out;
}

// ── Routes ──────────────────────────────────────────────────────────────────

/// `/api/agents/*` and `/api/admin/agents/*`. Null for any other path. `deps`: json, verifyAuth, requireAdmin, endSessions,
/// auditLog, notify (optional: (env, accountId, item) => void), agreement (tests only).
export async function handleAgentRoute(request, url, env, cors, deps) {
  const path = url.pathname;
  const m = request.method;
  const { json } = deps;
  const agreement = deps.agreement || AGENT_AGREEMENT;

  if (path === '/api/agents/agreement' && m === 'GET') {
    return json({ available: signupAvailable(env, agreement), ...(await agreementView(agreement)), providers: Object.entries(AGENT_PROVIDERS).map(([id, p]) => ({ id, name: p.name })), limit: AGENTS_PER_SPONSOR }, 200, cors);
  }
  if (path === '/api/agents/invites' && m === 'POST') return makeInvite(request, env, cors, deps, agreement);
  if (path === '/api/agents/register' && m === 'POST') return register(request, env, cors, deps, agreement);
  if (path === '/api/agents' && m === 'GET') {
    const userId = await deps.verifyAuth(request, env);
    if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
    const ids = (await readJson(env.USERS, `agents:${userId}`)) || [];
    const agents = [];
    for (const id of ids) {
      const a = await readJson(env.USERS, `user:${id}`);
      if (a) agents.push(agentListView(a));
    }
    return json({ agents, limit: AGENTS_PER_SPONSOR, available: signupAvailable(env, agreement) }, 200, cors);
  }
  let match;
  if ((match = path.match(/^\/api\/agents\/([a-f0-9-]+)\/(suspend|unsuspend|revoke)$/)) && m === 'POST') {
    return sponsorAction(request, env, cors, deps, match[1], match[2]);
  }
  if (path.startsWith('/api/admin/agents/')) return adminRoute(request, url, env, cors, deps);
  return null;
}

function agentListView(a) {
  return {
    id: a.id, username: a.username, provider: a.agent?.provider || null, model: a.agent?.model || null,
    created_at: a.created_at || null, suspended: !!a.agent?.suspended, suspended_by: a.agent?.suspended?.by || null,
    revoked: !!a.agent?.revoked_at, confirmed_cases: a.agent?.confirmed_cases || 0,
  };
}

async function makeInvite(request, env, cors, deps, agreement) {
  const { json } = deps;
  if (!signupAvailable(env, agreement)) return json({ error: 'Agent sign-up is not available yet.', code: 'agent_signup_unavailable' }, 503, cors);
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  const sponsor = await readJson(env.USERS, `user:${userId}`);
  const problem = sponsorProblem(sponsor, sponsor ? await frozenUntil(env, userId) : 0);
  if (problem === 'not_found') return json({ error: 'Unauthorized' }, 401, cors);
  if (problem) return json({ error: problem, code: 'sponsor_not_allowed' }, 403, cors);
  const body = await bodyOf(request);
  const view = await agreementView(agreement);
  if (body?.agree !== true || body?.agreement_sha256 !== view.sha256) {
    return json({ error: 'Read and accept the current agreement first.', code: 'agreement_required', agreement_sha256: view.sha256, version: view.version }, 400, cors);
  }
  const provider = typeof body.provider === 'string' ? body.provider : '';
  if (!AGENT_PROVIDERS[provider]) return json({ error: 'Only Claude agents (provider "anthropic") can be sponsored for now.', code: 'provider_not_allowed' }, 403, cors);
  if ((await liveAgentsOf(env, userId)).length >= AGENTS_PER_SPONSOR) return json({ error: `An account can sponsor at most ${AGENTS_PER_SPONSOR} agents.`, code: 'agent_limit' }, 409, cors);
  // The sponsor names the agent; the invite registers that name and no other.
  const name = typeof body.name === 'string' ? body.name.trim() : '';
  const nameProblem = agentNameProblem(name);
  if (nameProblem) return json({ error: nameProblem, code: 'bad_name' }, 400, cors);
  if (await nameTaken(env, name)) return json({ error: 'That name is taken.', code: 'name_taken' }, 409, cors);
  if (await agentNameLooksTaken(env, name)) return json({ error: 'That name reads as an existing account\'s name. Choose another.', code: 'name_too_close' }, 409, cors);
  const token = INVITE_PREFIX + randomHex(32);
  const hash = await sha256Hex(token);
  const now = Date.now();
  const invite = {
    sponsor_id: userId, provider, name, created_at: new Date(now).toISOString(), expires_at: new Date(now + AGENT_INVITE_TTL_S * 1000).toISOString(),
    agreement: { version: view.version, sha256: view.sha256, accepted_by: userId, accepted_at: new Date(now).toISOString() },
  };
  await env.USERS.put(`agentinvite:${hash}`, JSON.stringify(invite), { expirationTtl: AGENT_INVITE_TTL_S });
  try { await deps.auditLog?.(env, 'agent_invite_created', userId, hash.slice(0, 16), { provider, name, agreement_version: view.version }); } catch { /* the invite stands */ }
  const instructions = inviteInstructions({
    name, invite: token, inviteHash: hash, provider, agreementSha: view.sha256, agreementVersion: view.version,
    expiresAt: invite.expires_at, apiOrigin: new URL(request.url).origin,
  });
  return json({ ok: true, invite: token, name, expires_at: invite.expires_at, provider, agreement_sha256: view.sha256, instructions }, 201, cors);
}

async function register(request, env, cors, deps, agreement) {
  const { json } = deps;
  if (!signupAvailable(env, agreement)) return json({ error: 'Agent sign-up is not available yet.', code: 'agent_signup_unavailable' }, 503, cors);
  if (env.AUTH_RATE_LIMITER) {
    const ip = request.headers.get('cf-connecting-ip') || 'unknown';
    const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `agent-register:${ip}` });
    if (!success) return json({ error: 'Too many attempts. Try again in a minute.', code: 'rate_limited' }, 429, cors);
  }
  const body = await bodyOf(request);
  const refuse = (error, code, status = 400) => json({ error, code }, status, cors);
  if (!body) return refuse('Send a JSON object.', 'bad_request');
  const { invite, username, provider, model, signature } = body;
  const public_key = typeof body.public_key === 'string' ? body.public_key.toLowerCase() : '';
  if (typeof invite !== 'string' || !TOKEN.test(invite)) return refuse(`The invite is not valid. It is the whole line that starts "${INVITE_PREFIX}".`, 'bad_invite');
  const nameProblem = agentNameProblem(username);
  if (nameProblem) return refuse(nameProblem, 'bad_name');
  if (!PUBLIC_KEY.test(public_key)) return refuse('public_key is a 32-byte Ed25519 key in hex.', 'bad_key');
  const spec = AGENT_PROVIDERS[provider];
  if (!spec) return refuse('Only Claude agents (provider "anthropic") can register for now.', 'provider_not_allowed', 403);
  if (typeof model !== 'string' || !spec.model.test(model)) return refuse('model must be the Claude model id, such as claude-opus-5-5.', 'bad_model');
  if (typeof signature !== 'string' || !SIGNATURE.test(signature.toLowerCase())) return refuse('signature is a 64-byte Ed25519 signature in hex.', 'bad_signature');

  const inviteHash = await sha256Hex(invite);
  const held = await readJson(env.USERS, `agentinvite:${inviteHash}`);
  if (!held || Date.parse(held.expires_at) <= Date.now()) return refuse('The invite is not valid or has expired.', 'bad_invite', 403);
  if (held.provider !== provider) return refuse('The provider is not the one the sponsor named.', 'provider_mismatch', 403);
  if (typeof held.name !== 'string' || held.name !== username) return refuse(`This invite is for the name ${held.name || 'its sponsor gave'}, and only that name.`, 'name_mismatch', 403);
  const view = await agreementView(agreement);
  if (held.agreement?.sha256 !== view.sha256) return refuse('The agreement changed after this invite was made. Ask your sponsor for a new invite.', 'agreement_changed', 403);

  // The signature proves the agent holds the key, and binds its name, model, the invite and the agreement to it.
  try {
    const key = await crypto.subtle.importKey('raw', hexBytes(public_key), { name: 'Ed25519' }, false, ['verify']);
    const message = new TextEncoder().encode(registrationMessage({ username, public_key, provider, model, inviteHash, agreementSha: view.sha256 }));
    if (!(await crypto.subtle.verify('Ed25519', key, hexBytes(signature.toLowerCase()), message))) return refuse('The signature does not verify.', 'bad_signature', 401);
  } catch {
    return refuse('The signature does not verify.', 'bad_signature', 401);
  }

  const sponsor = await readJson(env.USERS, `user:${held.sponsor_id}`);
  if (sponsorProblem(sponsor, sponsor ? await frozenUntil(env, held.sponsor_id) : 0)) return refuse('The sponsor cannot sponsor an agent right now.', 'sponsor_not_allowed', 403);
  if (await nameTaken(env, username)) return refuse('That name is taken.', 'name_taken', 409);
  if (await agentNameLooksTaken(env, username)) return refuse('That name reads as an existing account\'s name. Ask your sponsor for an invite with another name.', 'name_too_close', 409);
  if ((await env.USERS.get(`pubkey:${public_key}`)) || (await env.USERS.get(`retired-key:${public_key}`))) return refuse('That key is already registered.', 'key_taken', 409);
  if ((await liveAgentsOf(env, held.sponsor_id)).length >= AGENTS_PER_SPONSOR) return refuse(`The sponsor already has ${AGENTS_PER_SPONSOR} agents.`, 'agent_limit', 409);

  const id = crypto.randomUUID();
  // One account per name fold (R2 create-if-absent): claimed before the invite is spent, so a name lost to another sign-up
  // in the same moment leaves the invite usable.
  if (!(await claimNameFold(env, username, id))) return refuse('That name is taken.', 'name_taken', 409);
  // One registration per invite: R2's create-if-absent is atomic, KV is not.
  const claimed = await env.SCENES.put(`agents/invites/${inviteHash}.claimed`, JSON.stringify({ at: new Date().toISOString(), username }), { onlyIf: { etagDoesNotMatch: '*' } });
  if (!claimed) {
    await releaseNameFolds(env, username, id);
    return refuse('The invite is not valid or has expired.', 'bad_invite', 403);
  }
  await env.USERS.delete(`agentinvite:${inviteHash}`);

  const now = new Date().toISOString();
  const user = {
    id, username, public_key,
    account_type: 'agent',
    sponsor_id: held.sponsor_id,
    agent: { provider, model, invited_at: held.created_at, registered_at: now, agreement: held.agreement, suspended: null, revoked_at: null, confirmed_cases: 0 },
    kyc_verified: false, age_verified: false,
    bliss_balance: 0, created_at: now, email: null, avatar_url: null, discord_id: null, banned: false,
  };
  await env.USERS.put(`user:${id}`, JSON.stringify(user));
  await env.USERS.put(`username:${username}`, id);
  await env.USERS.put(`pubkey:${public_key}`, id);
  await indexNameFolds(env, username, id);
  const list = (await readJson(env.USERS, `agents:${held.sponsor_id}`)) || [];
  await env.USERS.put(`agents:${held.sponsor_id}`, JSON.stringify([...new Set([...list, id])]));
  // The account object, seeded now from the record just written (sign-in would seed it on first touch; seeding twice
  // changes nothing). From here the object holds the session floor and the key. Best effort: sign-in seeds it if not.
  if (env.ACCOUNT_AUTH) {
    try {
      const { accountState } = await import('./account_auth.mjs');
      await accountState(env, id);
    } catch (e) {
      console.error('agent account object not seeded:', e?.name || 'Error');
    }
  }
  try { await deps.auditLog?.(env, 'agent_registered', held.sponsor_id, id, { provider, model, agreement_version: held.agreement?.version }); } catch { /* the account stands */ }
  return json({ ok: true, id, username, account_type: 'agent', sign_in: 'POST /api/auth/challenge, then POST /api/auth/verify-challenge with this key' }, 201, cors);
}

async function sponsorAction(request, env, cors, deps, agentId, action) {
  const { json } = deps;
  const userId = await deps.verifyAuth(request, env);
  if (!userId) return json({ error: 'Unauthorized' }, 401, cors);
  const agent = await readJson(env.USERS, `user:${agentId}`);
  // One answer for an agent that does not exist and one that is not yours.
  if (!agent || agent.account_type !== 'agent' || agent.sponsor_id !== userId) return json({ error: 'Agent not found' }, 404, cors);
  if (agent.agent?.revoked_at) return json({ error: 'This agent was revoked.', code: 'agent_revoked' }, 409, cors);
  const now = new Date().toISOString();
  if (action === 'suspend') {
    if (!agent.agent.suspended) agent.agent.suspended = { by: 'sponsor', at: now };
  } else if (action === 'unsuspend') {
    // A suspension staff placed after confirmed cases is lifted by staff only.
    if (agent.agent.suspended?.by === 'staff') return json({ error: 'Eustress suspended this agent after a review. Contact support@eustress.dev.', code: 'staff_suspension' }, 403, cors);
    agent.agent.suspended = null;
  } else {
    agent.agent.revoked_at = now;
    agent.banned = true;
    agent.ban_reason = 'This agent account was revoked by its sponsor.';
    await env.USERS.delete(`pubkey:${agent.public_key}`);
    // The key is retired: it never signs in or registers again, as a replaced human key.
    if (agent.public_key) await env.USERS.put(`retired-key:${agent.public_key}`, agentId);
  }
  await env.USERS.put(`user:${agentId}`, JSON.stringify(agent));
  // Read after write everywhere, so sign-in and the gate see this at once (KV may serve the old record for a minute).
  await writeAgentStanding(env, agent);
  if (action !== 'unsuspend') await deps.endSessions?.(env, agentId, `agent_${action}`);
  try { await deps.auditLog?.(env, `agent_${action}`, userId, agentId, {}); } catch { /* the change stands */ }
  return json({ ok: true, agent: agentListView(agent) }, 200, cors);
}

// ── Agent cases and the feedback report ─────────────────────────────────────

/// Opens a case on `accountId` when it is an agent's. Never throws; returns the case or null.
/// `event`: { source: 'listing' | 'forum' | 'report', ref, sim_id?, post_id?, status?, lane?, reasons?: [code], note? }.
export async function openAgentCase(env, accountId, event) {
  try {
    if (typeof accountId !== 'string' || !accountId) return null;
    const agent = await readJson(env.USERS, `user:${accountId}`);
    if (!agent || agent.account_type !== 'agent') return null;
    const id = crypto.randomUUID();
    const rec = {
      id, account_id: accountId, username: agent.username, sponsor_id: agent.sponsor_id || null,
      provider: agent.agent?.provider || null, model: agent.agent?.model || null,
      source: event?.source || 'report', ref: event?.ref || null, sim_id: event?.sim_id || null, post_id: event?.post_id || null,
      event_status: event?.status || null, lane: event?.lane || null,
      reasons: Array.isArray(event?.reasons) ? event.reasons.filter((r) => typeof r === 'string').slice(0, 10) : [],
      note: typeof event?.note === 'string' ? event.note.slice(0, 2000) : null,
      // Material in a legal hold never goes to a provider.
      legal: event?.lane === 'legal' || event?.status === 'quarantined',
      at: new Date().toISOString(), status: 'open', decision: null, report: null,
    };
    await env.USERS.put(`agentcase:${id}`, JSON.stringify(rec));
    await env.USERS.put(`agentcaseq:${id}`, '1');
    const list = (await readJson(env.USERS, `agentcases:${accountId}`)) || [];
    await env.USERS.put(`agentcases:${accountId}`, JSON.stringify([...list, id].slice(-200)));
    return rec;
  } catch (e) {
    console.error('agent case not opened:', e?.name || 'Error');
    return null;
  }
}

/// The feedback report's text: what the agent did and which rule it broke. Never the sponsor (D7), never another account's
/// personal information: the summary is the reviewer's own words.
export function renderAgentReport(rec, providerSpec) {
  const lines = [
    `Eustress agent feedback report ${rec.id}`,
    '',
    `Provider: ${providerSpec?.name || rec.provider}`,
    `Model (as declared by the agent and attested by its sponsor; not verified): ${rec.model}`,
    `Date of the event: ${rec.at}`,
    `Category: ${rec.decision?.category}`,
    '',
    'What the agent did:',
    rec.report?.summary || '',
    '',
    `What Eustress did: ${rec.report?.action || 'the content was removed and the case recorded'}.`,
    '',
    'This report is sent so the model can be improved. It names no person. Eustress agent accounts are run by verified',
    'sponsors who accept responsibility for their agents. Questions: support@eustress.dev.',
  ];
  const text = lines.join('\n');
  const escape = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  return { subject: `Eustress agent feedback report: ${rec.decision?.category} (${rec.model})`, text, html: `<pre style="font-family:system-ui,sans-serif;white-space:pre-wrap">${escape(text)}</pre>` };
}

async function adminRoute(request, url, env, cors, deps) {
  const { json } = deps;
  const adminId = await deps.requireAdmin(request, env);
  if (!adminId) return json({ error: 'Admin access required' }, 403, cors);
  const path = url.pathname;
  const m = request.method;
  let match;

  if (path === '/api/admin/agents/cases' && m === 'GET') {
    const listed = await env.USERS.list({ prefix: 'agentcaseq:', limit: 200 });
    const cases = [];
    for (const k of listed.keys || []) {
      const rec = await readJson(env.USERS, `agentcase:${k.name.slice('agentcaseq:'.length)}`);
      if (rec) cases.push(rec);
    }
    // Confirmed cases whose report waits to be sent are listed too.
    return json({ ok: true, cases }, 200, cors);
  }
  if (path === '/api/admin/agents/cases' && m === 'POST') {
    const body = await bodyOf(request);
    if (typeof body?.account_id !== 'string' || typeof body?.note !== 'string' || body.note.trim().length < 20) return json({ error: 'account_id and a note of at least 20 characters are required.' }, 400, cors);
    const rec = await openAgentCase(env, body.account_id, { source: 'report', note: body.note.trim(), lane: body.legal === true ? 'legal' : null });
    if (!rec) return json({ error: 'No agent account with that id.' }, 404, cors);
    try { await deps.auditLog?.(env, 'agent_case_opened', adminId, rec.id, { account_id: rec.account_id }); } catch { /* the case stands */ }
    return json({ ok: true, case: rec }, 201, cors);
  }
  if ((match = path.match(/^\/api\/admin\/agents\/cases\/([a-f0-9-]+)\/resolve$/)) && m === 'POST') {
    const rec = await readJson(env.USERS, `agentcase:${match[1]}`);
    if (!rec) return json({ error: 'No such case.' }, 404, cors);
    if (rec.status !== 'open') return json({ error: 'This case is already decided.' }, 409, cors);
    const body = await bodyOf(request);
    const rationale = typeof body?.rationale === 'string' ? body.rationale.trim() : '';
    if (rationale.length < 20) return json({ error: 'rationale must be at least 20 characters.' }, 400, cors);
    if (body?.decision === 'not_confirmed') {
      rec.status = 'not_confirmed';
      rec.decision = { by: adminId, at: new Date().toISOString(), rationale };
    } else if (body?.decision === 'confirmed') {
      if (!AGENT_CASE_CATEGORIES.includes(body.category)) return json({ error: `category is one of ${AGENT_CASE_CATEGORIES.join(', ')}.` }, 400, cors);
      const summary = typeof body.summary === 'string' ? body.summary.trim() : '';
      if (!rec.legal && summary.length < 20) return json({ error: 'summary (the report\'s account of what the agent did, at least 20 characters, naming no person) is required.' }, 400, cors);
      rec.status = 'confirmed';
      rec.decision = { by: adminId, at: new Date().toISOString(), rationale, category: body.category };
      // A legal-lane case never produces a report to a provider.
      rec.report = rec.legal ? null : { summary: summary.slice(0, 4000), action: typeof body.action === 'string' ? body.action.trim().slice(0, 300) : null, drafted_at: rec.decision.at, sent_at: null };
      await noteConfirmed(env, rec, deps);
    } else {
      return json({ error: 'decision is confirmed or not_confirmed.' }, 400, cors);
    }
    await env.USERS.put(`agentcase:${rec.id}`, JSON.stringify(rec));
    // An open queue entry stays while a report waits to be sent.
    if (!rec.report || rec.report.sent_at) await env.USERS.delete(`agentcaseq:${rec.id}`);
    try { await deps.auditLog?.(env, 'agent_case_resolved', adminId, rec.id, { decision: rec.status, category: rec.decision?.category || null }); } catch { /* the decision stands */ }
    // A legal-lane case is never announced: a removal under a sealed category tells its author nothing until a person allows.
    if (rec.sponsor_id && !rec.legal) {
      try { await deps.notify?.(env, rec.sponsor_id, { kind: 'agent_case', headline: `A review of your agent @${rec.username} was ${rec.status === 'confirmed' ? 'confirmed' : 'closed with no finding'}.`, case_id: rec.id }); } catch { /* telling never undoes */ }
    }
    return json({ ok: true, case: rec }, 200, cors);
  }
  if ((match = path.match(/^\/api\/admin\/agents\/cases\/([a-f0-9-]+)\/send-report$/)) && m === 'POST') {
    const rec = await readJson(env.USERS, `agentcase:${match[1]}`);
    if (!rec) return json({ error: 'No such case.' }, 404, cors);
    if (rec.status !== 'confirmed' || !rec.report) return json({ error: 'Only a confirmed case outside the legal lane has a report.', code: 'no_report' }, 409, cors);
    if (rec.report.sent_at) return json({ error: 'This report was already sent.', code: 'already_sent' }, 409, cors);
    const spec = AGENT_PROVIDERS[rec.provider];
    if (!spec?.report_to) return json({ error: 'No feedback address for this provider.', code: 'no_channel' }, 409, cors);
    const [result] = await sendNotice(env, [spec.report_to], renderAgentReport(rec, spec), { from: AGENT_REPORT_FROM, kind: 'staff', accountId: 'agent-reports', fromName: 'Eustress Agent Reports' });
    if (!result?.sent) return json({ error: 'The report was not sent. Try again later.', code: 'send_failed', reason: result?.reason || null }, 502, cors);
    rec.report.sent_at = new Date().toISOString();
    rec.report.sent_by = adminId;
    rec.report.to = spec.report_to;
    await env.USERS.put(`agentcase:${rec.id}`, JSON.stringify(rec));
    await env.USERS.delete(`agentcaseq:${rec.id}`);
    try { await deps.auditLog?.(env, 'agent_report_sent', adminId, rec.id, { to: spec.report_to }); } catch { /* sent is sent */ }
    return json({ ok: true, sent_at: rec.report.sent_at, to: spec.report_to }, 200, cors);
  }
  if ((match = path.match(/^\/api\/admin\/agents\/([a-f0-9-]+)\/unsuspend$/)) && m === 'POST') {
    const agent = await readJson(env.USERS, `user:${match[1]}`);
    if (!agent || agent.account_type !== 'agent') return json({ error: 'Agent not found' }, 404, cors);
    if (agent.agent?.revoked_at) return json({ error: 'This agent was revoked.' }, 409, cors);
    agent.agent.suspended = null;
    await env.USERS.put(`user:${agent.id}`, JSON.stringify(agent));
    await writeAgentStanding(env, agent);
    try { await deps.auditLog?.(env, 'agent_unsuspended_by_staff', adminId, agent.id, {}); } catch { /* the change stands */ }
    return json({ ok: true, agent: agentListView(agent) }, 200, cors);
  }
  if (path === '/api/admin/agents/name-folds' && m === 'POST') return backfillNameFolds(request, env, cors, deps, adminId);
  return json({ error: 'Unknown agent route' }, 404, cors);
}

/// POST /api/admin/agents/name-folds `{ apply?: true, cursor? }`: files the case fold and confusable fold of every name made
/// before folds were kept, one page of `username:` keys per call (F5). Without `apply` it only counts. Never overwrites a
/// fold another account already holds; it reports each such pair for a person to look at. Run until `done`.
async function backfillNameFolds(request, env, cors, deps, adminId) {
  const { json } = deps;
  const body = (await bodyOf(request)) || {};
  const apply = body.apply === true;
  const page = await env.USERS.list({ prefix: 'username:', limit: 500, ...(typeof body.cursor === 'string' && body.cursor ? { cursor: body.cursor } : {}) });
  const counts = { scanned: 0, filed: 0, already: 0, collisions: 0, conf_filed: 0 };
  const collisions = [];
  for (const k of page.keys || []) {
    const name = k.name.slice('username:'.length);
    const id = await env.USERS.get(k.name);
    if (!id) continue;
    counts.scanned++;
    const foldKey = `usernamefold:${nameFold(name)}`;
    const holder = await env.USERS.get(foldKey);
    if (holder === id) counts.already++;
    else if (holder) { counts.collisions++; if (collisions.length < 50) collisions.push({ name, account_id: id, fold_held_by: holder }); }
    else { counts.filed++; if (apply) await env.USERS.put(foldKey, id); }
    const confKey = `usernameconf:${confusableFold(name)}`;
    if (!(await env.USERS.get(confKey))) { counts.conf_filed++; if (apply) await env.USERS.put(confKey, id); }
  }
  const done = page.list_complete !== false || !page.cursor;
  try { await deps.auditLog?.(env, 'agent_name_folds_backfill', adminId, apply ? 'apply' : 'count', { ...counts, done }); } catch { /* the pass stands */ }
  return json({ ok: true, applied: apply, ...counts, collisions, done, cursor: done ? null : page.cursor }, 200, cors);
}

/// D6 and D8: a confirmed case counts on the agent and toward the sponsor; three, or one in the legal lane, suspend the
/// agent, and a suspension ends its sessions at once (F4).
async function noteConfirmed(env, rec, deps = {}) {
  const agent = await readJson(env.USERS, `user:${rec.account_id}`);
  if (agent && agent.account_type === 'agent') {
    agent.agent.confirmed_cases = (agent.agent.confirmed_cases || 0) + 1;
    let suspendedNow = false;
    if ((rec.legal || agent.agent.confirmed_cases >= AGENT_SUSPEND_AFTER) && !agent.agent.revoked_at) {
      suspendedNow = !agent.agent.suspended;
      agent.agent.suspended = { by: 'staff', at: new Date().toISOString(), case_id: rec.id };
    }
    await env.USERS.put(`user:${agent.id}`, JSON.stringify(agent));
    await writeAgentStanding(env, agent);
    if (suspendedNow || agent.agent.suspended?.case_id === rec.id) {
      try { await deps.endSessions?.(env, agent.id, 'agent_staff_suspend'); } catch (e) { console.error('agent sessions not ended:', e?.name || 'Error'); }
    }
  }
  // Counted on its own key: the account record has several writers that rewrite it whole.
  if (rec.sponsor_id) {
    const key = `agentstrikes:${rec.sponsor_id}`;
    await env.USERS.put(key, String((Number(await env.USERS.get(key)) || 0) + 1));
  }
}
