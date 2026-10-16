// =============================================================================
// The public profile an account edits: display name, bio and avatar picture
// =============================================================================
//
//   POST /api/account/profile   { display_name?, bio?, avatar_url? }   session (fresh)
//     -> { ok, profile: { display_name, bio, avatar_url }, pending: { display_name?, bio? },
//          pending_notes: { display_name?: { state, reason? }, bio?: ... }, changed }
//
// What every visitor reads on /profile/{username} (handleUserProfile in
// index.js). The fields are kept under `profile:{accountId}` in USERS, a key of
// their own, so this route never rewrites the account record (a stale
// whole-record write there could undo a concurrent change to it).
//
// The rules, all on the Worker, whatever the client:
//
//   * A session is required, and the route is `fresh` (src/route_policy.mjs):
//     a session that was signed out or ended by a key replacement cannot change
//     what others see, even inside the KV mirror's minute.
//   * An account that must verify its identity first (`kyc_required`) is
//     refused. The kycGate in front of every mutating route refuses it already;
//     the handler checks again, so the rule holds if the route is ever reclassed.
//   * At most 10 edits an hour per account (every request that reaches the
//     checks counts, refused or not).
//   * Text: NFKC, then every format character (Unicode Cf: zero-width spaces and
//     joiners, word joiner, BOM, bidi controls) removed, except, in a bio, a
//     zero-width joiner between two emoji; at most 2 combining marks on any base
//     character; then the cleaning other display text gets (purchases.mjs
//     cleanText). Then measured: over the limit is refused, not cut.
//   * A display name may not pass for staff or for someone else. Folded (NFKC,
//     the common Cyrillic and Greek look-alikes read as the Latin letters they
//     imitate, lower case, accents dropped, the usual digit and symbol spellings
//     read as letters, separators removed), it is refused when any word of it is
//     a reserved word, when the whole of it is reserved words run together, when
//     it contains "eustress", or when one of its username forms is another
//     account's username (looked up directly: `username:{form}`).
//   * A new display name or bio is held for review before anyone else sees it.
//     It is saved as pending; visitors keep seeing the approved value (or the
//     username, or no bio); the owner sees the pending value marked "Pending
//     review". The text, as the owner sent it, goes to Jev's check
//     (moderation.mjs checkProfileText) with the account's id (the check allows
//     an account 50 a day). Only 'pass' publishes it. 'refuse' drops it and
//     answers with the category and the check's sentence; 'review' holds it for a
//     person, and the owner is told a person will look at it; 'unavailable' holds
//     it with the check's sentence for the owner, and it is sent again with the
//     owner's next save. Clearing a field is never sent: an empty bio and no
//     display name (the username shows) need no review. Nothing here logs the text.
//   * A display name is 1 to 32 characters and a bio 0 to 280, counted the way
//     the check counts them (moderation.mjs profileTextCleaned: NFKC, format
//     characters removed, whitespace runs folded to one space, code points), so
//     the route and the check agree on every limit.
//   * The picture is a path under the site's /assets, and nothing else. There is
//     no picture of the account's own to point at: the Worker's user images are
//     a listing's thumbnail, cover and review captures, approved with the listing
//     (cover.mjs, moderation.mjs), and an adult listing's art is only served as
//     signed URLs.
//   * What a visitor reads is checked again on the way out (publicProfileOf), so
//     a value that a later rule refuses stops being shown.
//   * A field left out keeps its value; an empty string or null clears it. A
//     write that changes nothing is not made.
// =============================================================================

import { cleanText } from './purchases.mjs';
import { needsKyc } from './kyc_identity.mjs';
import { checkProfileText, profileTextCleaned } from './moderation.mjs';

export const PROFILE_ROUTE = '/api/account/profile';
export const PROFILE_LIMITS = Object.freeze({
  display_name: 32,  // characters, after cleaning
  bio: 280,          // characters, after cleaning
  avatar_url: 512,   // bytes
  body: 8192,        // bytes of request body
  marks: 2,          // combining marks on one base character
  edits_per_hour: 10,
  review_ms: 4000,   // how long the text check may take before the value simply stays pending
});

/// Words a display name may not be, or be made of: staff, service and trust words.
export const RESERVED_WORDS = Object.freeze([
  'support', 'admin', 'administrator', 'moderator', 'mod', 'staff', 'official', 'eustress', 'system', 'security', 'help',
  'team', 'trust', 'safety',
]);

/// The categories a refusal may name. Anything else the check answers reads as 'other'.
export const REFUSAL_CATEGORIES = Object.freeze(['sexual', 'hate', 'harassment', 'violence', 'self_harm', 'personal_info', 'spam', 'impersonation', 'other']);
const CATEGORY_WORDS = {
  sexual: 'sexual content', hate: 'hate', harassment: 'harassment', violence: 'violence', self_harm: 'self-harm',
  personal_info: 'personal information', spam: 'spam', impersonation: 'impersonation', other: 'our rules',
};

const KEY = (accountId) => `profile:${accountId}`;
const EDITS_KEY = (accountId, hour) => `profile-edits:${accountId}:${hour}`;
const FIELDS = ['display_name', 'bio', 'avatar_url'];
const REVIEWED = ['display_name', 'bio'];
const chars = (s) => [...s].length;

const FORMAT = /\p{Cf}/u;
const MARK = /\p{M}/u;
const PICTOGRAPH = /\p{Extended_Pictographic}/u;
const EMOJI_TAIL = /[\uFE0F\u{1F3FB}-\u{1F3FF}]/u; // variation selector 16 and the skin tones, which may sit between an emoji and a joiner
const ZWJ = '\u200D';

/// NFKC, format characters removed (a bio keeps a joiner between two emoji), and at most `PROFILE_LIMITS.marks` combining
/// marks on one base character.
export function stripInvisible(value, { keepEmojiJoiner = false } = {}) {
  const cps = [...value.normalize('NFKC')];
  const out = [];
  let marks = 0;
  for (let i = 0; i < cps.length; i++) {
    const c = cps[i];
    if (FORMAT.test(c)) {
      if (keepEmojiJoiner && c === ZWJ && joinsEmoji(cps, i)) out.push(c);
      continue;
    }
    if (MARK.test(c) && !EMOJI_TAIL.test(c)) {
      if (++marks > PROFILE_LIMITS.marks) continue;
    } else {
      marks = 0;
    }
    out.push(c);
  }
  return out.join('');
}

/// The joiner at `i` stands between two emoji: an emoji (with its variation selector or skin tone) before it, one after.
function joinsEmoji(cps, i) {
  let j = i - 1;
  while (j >= 0 && EMOJI_TAIL.test(cps[j])) j--;
  return j >= 0 && PICTOGRAPH.test(cps[j]) && i + 1 < cps.length && PICTOGRAPH.test(cps[i + 1]);
}

/// How long the check counts a text: profileTextCleaned's code points.
export const checkedLength = (value) => [...profileTextCleaned(value)].length;

/// Display text, cleaned. `null` when it is longer than `maxChars` once cleaned. A non-string is `null`.
function cleanField(value, maxChars, opts) {
  if (typeof value !== 'string' || value.length > PROFILE_LIMITS.body) return null;
  const visible = stripInvisible(value, opts);
  const clean = cleanText(visible, new TextEncoder().encode(visible).length);
  return chars(clean) <= maxChars ? clean : null;
}

export const cleanDisplayName = (value) => cleanField(value, PROFILE_LIMITS.display_name);
export const cleanBio = (value) => cleanField(value, PROFILE_LIMITS.bio, { keepEmojiJoiner: true });

// ---------------------------------------------------------------------------
// Passing for staff or for someone else
// ---------------------------------------------------------------------------

/// The common Cyrillic and Greek look-alikes of Latin letters, read as the letter they imitate, case kept. An upper-case
/// Greek eta looks like H and its lower case like n, so the two cases are mapped on their own.
const CONFUSABLE = {
  // Cyrillic, lower case
  '\u0430': 'a', '\u0435': 'e', '\u043E': 'o', '\u0440': 'p', '\u0441': 'c', '\u0443': 'y', '\u0445': 'x', '\u0456': 'i',
  '\u0458': 'j', '\u0455': 's', '\u0501': 'd', '\u0432': 'b', '\u043D': 'h', '\u043A': 'k', '\u043C': 'm', '\u0442': 't',
  // Cyrillic, upper case
  '\u0410': 'A', '\u0415': 'E', '\u041E': 'O', '\u0420': 'P', '\u0421': 'C', '\u0423': 'Y', '\u0425': 'X', '\u0406': 'I',
  '\u0408': 'J', '\u0405': 'S', '\u0412': 'B', '\u041D': 'H', '\u041A': 'K', '\u041C': 'M', '\u0422': 'T',
  // Latin small script g
  '\u0261': 'g',
  // Greek, upper case
  '\u0391': 'A', '\u0392': 'B', '\u0395': 'E', '\u0396': 'Z', '\u0397': 'H', '\u0399': 'I', '\u039A': 'K', '\u039C': 'M',
  '\u039D': 'N', '\u039F': 'O', '\u03A1': 'P', '\u03A4': 'T', '\u03A5': 'Y', '\u03A7': 'X',
  // Greek, lower case
  '\u03B1': 'a', '\u03B2': 'b', '\u03B5': 'e', '\u03B6': 'z', '\u03B7': 'n', '\u03B9': 'i', '\u03BA': 'k', '\u03BC': 'u',
  '\u03BD': 'v', '\u03BF': 'o', '\u03C1': 'p', '\u03C4': 't', '\u03C5': 'u', '\u03C7': 'x',
};

/// NFKC, the look-alikes read as Latin letters, accents dropped. Case is kept.
export function unconfuse(value) {
  const latin = [...value.normalize('NFKC')].map((c) => CONFUSABLE[c] ?? c).join('');
  return latin.normalize('NFKD').replace(/\p{M}/gu, '').normalize('NFKC');
}

/// The usual spellings of letters: 0 o, 1 l i, 3 e, 4 a, 5 s, 7 t (and @ a, $ s, | i, ! i). l and 1 read as i, so the
/// three are one letter here.
const LEET = { 0: 'o', 1: 'i', l: 'i', '|': 'i', '!': 'i', 3: 'e', 4: 'a', '@': 'a', 5: 's', $: 's', 7: 't' };

/// The words of a name, folded: look-alikes read as Latin, lower case, accents dropped, the spellings above read as
/// letters, split at anything that is not a letter or a digit.
export function foldWords(value) {
  if (typeof value !== 'string') return [];
  const lower = unconfuse(value).toLowerCase();
  const mapped = [...lower].map((c) => LEET[c] ?? c).join('');
  return mapped.split(/[^\p{L}\p{N}]+/u).filter(Boolean);
}

/// The whole name folded, separators removed: what two names are compared by.
export const foldName = (value) => foldWords(value).join('');

const RESERVED_FOLDED = RESERVED_WORDS.map(foldName);

/// The name is reserved words run together ("EustressSupport", "admin.team"): every split of it is a reserved word.
function madeOfReserved(folded) {
  if (!folded) return false;
  const can = new Array(folded.length + 1).fill(false);
  can[0] = true;
  for (let end = 1; end <= folded.length; end++) {
    can[end] = RESERVED_FOLDED.some((w) => end >= w.length && can[end - w.length] && folded.slice(end - w.length, end) === w);
  }
  return can[folded.length];
}

/// True when a display name reads as staff or as Eustress itself.
export function isReservedName(value) {
  const words = foldWords(value);
  const whole = words.join('');
  return words.some((w) => RESERVED_FOLDED.includes(w)) || madeOfReserved(whole) || whole.includes(foldName('eustress'));
}

/// A username as registration takes it (handleRegister in index.js): 3 to 32 of letters, digits and `_`, stored as typed
/// under `username:{name}`, so the lookup is exact and case matters.
const USERNAME = /^[A-Za-z0-9_]{3,32}$/;

/// The usernames a display name could be read as, to look up directly: the name with the look-alikes and accents read as
/// Latin, its separators removed or made `_`, as typed and in lower case; and the fully folded name (the digit and symbol
/// spellings read as letters), with and without `_` between its words. Only forms a username could take.
export function usernameForms(value) {
  if (typeof value !== 'string') return [];
  const latin = unconfuse(value);
  const parts = latin.split(/[^A-Za-z0-9]+/).filter(Boolean);
  const words = foldWords(value);
  const forms = [parts.join(''), parts.join('_'), words.join(''), words.join('_')];
  const all = forms.flatMap((f) => [f, f.toLowerCase()]);
  return [...new Set(all)].filter((f) => USERNAME.test(f));
}

/// True when one of the name's username forms is the username of another account. A few direct reads, never a scan.
export async function isAnotherUsername(env, value, accountId) {
  for (const form of usernameForms(value)) {
    const owner = await env.USERS.get(`username:${form}`);
    if (owner && owner !== accountId) return true;
  }
  return false;
}

// ---------------------------------------------------------------------------
// The picture
// ---------------------------------------------------------------------------

/// The avatar address as stored, or `null`: a path under /assets of letters, digits, `/ _ . -`, with no `..` or `//`.
/// No URL on any host (see the top of this file).
export function cleanAvatarUrl(value) {
  if (typeof value !== 'string' || value.length === 0 || new TextEncoder().encode(value).length > PROFILE_LIMITS.avatar_url) return null;
  return /^\/assets\/[A-Za-z0-9/_.-]+$/.test(value) && !value.includes('..') && !value.includes('//') ? value : null;
}

// ---------------------------------------------------------------------------
// Review
// ---------------------------------------------------------------------------

/// The check's sentence (its `message`, which never holds the text), as the owner may read it: one line of plain text, at
/// most 200 characters.
export function cleanReason(value) {
  if (typeof value !== 'string') return '';
  return [...cleanText(stripInvisible(value), 1024)].slice(0, 200).join('').trim();
}

/// One field through the check, for `accountId`: 'pass'; `{ refuse: category, message? }`; or `{ hold: state }` with state
/// 'review' (a person will look) or 'unavailable' (with the check's sentence when it gave one; also an error, a timeout, or
/// an answer that is none of these). The text goes to the check and nowhere else.
export async function reviewText(check, env, field, text, accountId) {
  if (typeof check !== 'function') return { hold: 'unavailable' };
  let timer;
  try {
    const answer = await Promise.race([
      check({ text, field, env, accountId, timeoutMs: PROFILE_LIMITS.review_ms }),
      new Promise((resolve) => { timer = setTimeout(() => resolve({ verdict: 'unavailable' }), PROFILE_LIMITS.review_ms + 500); }),
    ]);
    if (answer?.verdict === 'pass') return 'pass';
    if (answer?.verdict === 'refuse') {
      const category = REFUSAL_CATEGORIES.includes(answer.category) ? answer.category : 'other';
      const message = cleanReason(answer.message);
      return message ? { refuse: category, message } : { refuse: category };
    }
    if (answer?.verdict === 'review') return { hold: 'review' };
    const reason = answer?.verdict === 'unavailable' ? cleanReason(answer.message) : '';
    return reason ? { hold: 'unavailable', reason } : { hold: 'unavailable' };
  } catch {
    return { hold: 'unavailable' };
  } finally {
    clearTimeout(timer);
  }
}

const NOTE_STATES = ['review', 'unavailable'];

/// A stored note, checked again: `{ state, reason? }`, or null.
function cleanNote(note) {
  if (!note || typeof note !== 'object' || !NOTE_STATES.includes(note.state)) return null;
  const reason = note.state === 'unavailable' ? cleanReason(note.reason) : '';
  return reason ? { state: note.state, reason } : { state: note.state };
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// A saved display name that is still allowed (clean, and not reserved), else ''.
const shownName = (value) => {
  const clean = cleanDisplayName(value ?? '') ?? '';
  return clean && !isReservedName(clean) ? clean : '';
};

/// What the account saved and was approved, each field checked again on the way out (a value that no longer passes reads
/// as none), and what is waiting for review; or `null` when nothing is saved or storage cannot be read.
export async function readProfile(env, accountId) {
  if (!env?.USERS || typeof accountId !== 'string' || !accountId) return null;
  let saved;
  try {
    saved = JSON.parse((await env.USERS.get(KEY(accountId))) || 'null');
  } catch {
    return null;
  }
  if (!saved || typeof saved !== 'object') return null;
  const pending = {};
  const p = saved.pending && typeof saved.pending === 'object' ? saved.pending : {};
  if (typeof p.display_name === 'string' && shownName(p.display_name)) pending.display_name = shownName(p.display_name);
  if (typeof p.bio === 'string' && cleanBio(p.bio)) pending.bio = cleanBio(p.bio);
  const notes = {};
  const n = saved.pending_notes && typeof saved.pending_notes === 'object' ? saved.pending_notes : {};
  for (const field of Object.keys(pending)) notes[field] = cleanNote(n[field]) || { state: 'unavailable' };
  return {
    display_name: shownName(saved.display_name),
    bio: cleanBio(saved.bio ?? '') ?? '',
    avatar_url: cleanAvatarUrl(saved.avatar_url ?? '') ?? '',
    pending,
    pending_notes: notes,
  };
}

/// The three fields a visitor sees: the approved profile, else what the account record has, by the same rules. Nothing
/// pending is ever in it.
export async function publicProfileOf(env, accountId, user) {
  const saved = await readProfile(env, accountId);
  const legacyBio = typeof user?.bio === 'string' ? cleanText(stripInvisible(user.bio, { keepEmojiJoiner: true }), PROFILE_LIMITS.bio * 4) : '';
  return {
    display_name: saved?.display_name || user?.username || '',
    bio: saved ? saved.bio : [...legacyBio].slice(0, PROFILE_LIMITS.bio).join(''),
    avatar_url: (saved ? saved.avatar_url : cleanAvatarUrl(user?.avatar_url ?? '')) || null,
  };
}

/// What is waiting for review, for the owner alone: `{ display_name?, bio? }`.
export async function pendingProfileOf(env, accountId) {
  return (await readProfile(env, accountId))?.pending || {};
}

/// What the owner's own profile read adds: `{ pending, pending_notes }`.
export async function ownerPendingOf(env, accountId) {
  const saved = await readProfile(env, accountId);
  return { pending: saved?.pending || {}, pending_notes: saved?.pending_notes || {} };
}

// ---------------------------------------------------------------------------
// The route
// ---------------------------------------------------------------------------

/// The request body, read up to the limit. `null` when it is larger, or is not a JSON object.
async function boundedJson(request) {
  if (Number(request.headers.get('Content-Length')) > PROFILE_LIMITS.body) return null;
  const reader = request.body?.getReader();
  if (!reader) return {};
  const chunks = [];
  let size = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.length;
    if (size > PROFILE_LIMITS.body) {
      await reader.cancel().catch(() => {});
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const c of chunks) { bytes.set(c, at); at += c.length; }
  try {
    const body = JSON.parse(new TextDecoder().decode(bytes) || '{}');
    return body && typeof body === 'object' && !Array.isArray(body) ? body : null;
  } catch {
    return null;
  }
}

/// Spend one of the hour's edits; false when they are used up. Read then write: a parallel burst can overrun by a few.
async function takeEdit(env, accountId, now = Date.now()) {
  const key = EDITS_KEY(accountId, Math.floor(now / 3_600_000));
  const used = parseInt((await env.USERS.get(key)) || '0', 10) || 0;
  if (used >= PROFILE_LIMITS.edits_per_hour) return false;
  await env.USERS.put(key, String(used + 1), { expirationTtl: 7200 });
  return true;
}

const stored = (rec) => JSON.stringify({ display_name: rec.display_name, bio: rec.bio, avatar_url: rec.avatar_url, pending: rec.pending, pending_notes: rec.pending_notes });

/// `POST /api/account/profile`. A response for this route, or `null` for any other path or method.
/// `deps`: authenticate (the session check), json, cors; `checkProfileText` replaces Jev's check in a test.
export async function handleProfileRoute(request, url, env, deps) {
  const { json, cors } = deps;
  if (url.pathname !== PROFILE_ROUTE || request.method !== 'POST') return null;
  const accountId = await deps.authenticate(request, env);
  if (!accountId) return json({ error: 'Unauthorized' }, 401, cors);

  let user;
  try {
    user = JSON.parse((await env.USERS.get(`user:${accountId}`)) || 'null');
  } catch {
    user = null;
  }
  if (!user) return json({ error: 'User not found' }, 404, cors);
  if (needsKyc(user)) {
    return json({ error: 'Verify your identity to keep using your account', code: 'kyc_required' }, 403, cors);
  }
  if (!(await takeEdit(env, accountId))) {
    return json({ error: 'You can change your profile 10 times an hour. Try again later.', code: 'rate_limited' }, 429, { ...cors, 'Retry-After': '3600' });
  }

  const body = await boundedJson(request);
  if (!body) return json({ error: 'Send a JSON object of at most 8 KB', code: 'bad_body' }, 400, cors);
  const asked = FIELDS.filter((f) => Object.hasOwn(body, f));
  if (asked.length === 0) return json({ error: 'Nothing to change', code: 'nothing_to_change' }, 400, cors);

  const before = (await readProfile(env, accountId)) || { display_name: '', bio: '', avatar_url: '', pending: {}, pending_notes: {} };
  const next = { ...before, pending: { ...before.pending }, pending_notes: { ...before.pending_notes } };
  const settle = (field) => { delete next.pending[field]; delete next.pending_notes[field]; };
  // The text as the owner sent it, for the check (which cleans and counts it the same way).
  const raw = {};
  for (const field of asked) {
    const value = body[field];
    if (value === null || value === '') {
      // Clearing needs no review, and drops anything waiting for it.
      next[field] = '';
      settle(field);
      continue;
    }
    if (typeof value !== 'string') return json({ error: `${field} must be text`, code: 'bad_field', field }, 400, cors);
    if (field === 'avatar_url') {
      const avatar = cleanAvatarUrl(value.trim());
      if (!avatar) return json({ error: 'The picture must be a path under /assets', code: 'bad_avatar', field }, 400, cors);
      next.avatar_url = avatar;
      continue;
    }
    const max = PROFILE_LIMITS[field];
    const clean = field === 'bio' ? cleanBio(value) : cleanDisplayName(value);
    if (clean === null || checkedLength(value) > max) return json({ error: `${field === 'bio' ? 'The bio' : 'The display name'} can be at most ${max} characters`, code: 'too_long', field, max }, 400, cors);
    if (field === 'display_name' && !clean) {
      return json({ error: 'A display name needs at least one visible character. Leave it empty to show your username.', code: 'too_short', field, min: 1 }, 400, cors);
    }
    if (clean === next[field]) { settle(field); continue; }
    if (field === 'display_name' && clean) {
      // A display name that passes for staff or for another account is refused. The reason is a category; the text is never
      // logged or echoed.
      if (isReservedName(clean)) {
        return json({ error: 'That display name is kept for Eustress staff and services. Choose another.', code: 'reserved_name', field }, 400, cors);
      }
      if (await isAnotherUsername(env, clean, accountId)) {
        return json({ error: 'That display name reads as another account\'s username. Choose another.', code: 'name_is_a_username', field }, 400, cors);
      }
    }
    if (clean) { next.pending[field] = clean; raw[field] = value; } else { next[field] = ''; settle(field); }
  }

  // Everything waiting goes to Jev's check (a test may hand the route a stand-in). Only 'pass' makes it public.
  const check = deps.checkProfileText ?? checkProfileText;
  let refused = null;
  for (const field of REVIEWED) {
    const text = next.pending[field];
    if (!text) continue;
    const verdict = await reviewText(check, env, field, raw[field] ?? text, accountId);
    if (verdict === 'pass') {
      next[field] = text;
      settle(field);
    } else if (verdict?.refuse) {
      settle(field);
      refused ??= { field, category: verdict.refuse, message: verdict.message };
    } else {
      next.pending_notes[field] = verdict.reason ? { state: verdict.hold, reason: verdict.reason } : { state: verdict.hold };
    }
  }

  const changed = stored(next) !== stored(before);
  if (changed) await env.USERS.put(KEY(accountId), JSON.stringify({ ...JSON.parse(stored(next)), updated_at: new Date().toISOString() }));
  const shownNow = { display_name: next.display_name, bio: next.bio, avatar_url: next.avatar_url || null };
  if (refused) {
    const label = refused.field === 'bio' ? 'Your bio' : 'Your display name';
    return json({
      error: refused.message || `${label} was not accepted: it reads as ${CATEGORY_WORDS[refused.category]}.`, code: 'refused', field: refused.field, category: refused.category,
      profile: shownNow, pending: next.pending, pending_notes: next.pending_notes,
    }, 400, cors);
  }
  return json({ ok: true, profile: shownNow, pending: next.pending, pending_notes: next.pending_notes, changed }, 200, { ...cors, 'Cache-Control': 'private, no-store' });
}

export const profileKey = KEY;
export const profileEditsKey = EDITS_KEY;
