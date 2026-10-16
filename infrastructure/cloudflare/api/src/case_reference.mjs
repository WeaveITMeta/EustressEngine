// =============================================================================
// Case references: one name for every case a person can be told about
// =============================================================================
//
// A review of a listing (`modcase:{id}`), a TAKE IT DOWN request (src/takedown.mjs),
// a reported forum post (`forumcase:{postId}`) and the safety cases that come later
// are each named by a reference:
//
//     EC-7K2M-9QXD-4TPF-HJ3W-8NBR
//
// "EC-" and twenty Crockford base32 symbols in five groups of four. Each symbol
// is five bits of the platform's cryptographic random source, so a reference is
// 100 random bits: it cannot be guessed, and it encodes nothing. Not the case's
// id, not the listing, not when it was made, and never who reported, reviewed or
// decided. Holding a reference is what lets someone with no account follow their
// case (GET /api/cases/{reference}, src/case_history.mjs), so it is handed only
// to the parties of the case.
//
// Storage (SOCIAL):
//   caseref:{reference}         -> { kind, id, at }   the case a reference names
//   caseref-of:{kind}:{id}      -> reference          the reference a case has
// and in R2 (SCENES), the arbiter:
//   caserefs/{kind}/{id}        the reference, created only if absent
//
// A case gets its reference when it is made (a takedown), or the first time it
// is read through these modules (a moderation case made before references
// existed). Two first reads at once must not hand out two references, and KV has
// no create-if-absent, so the first claim is an R2 object written with the
// precondition R2 enforces atomically; the loser reads the winner's.
// =============================================================================

/// The kinds of case a reference can name. `forum` is a reported forum post (`forumcase:{postId}`, src/moderation.mjs
/// fileForumReport), named by its post id.
export const CASE_KINDS = ['moderation', 'takedown', 'forum'];

/// Crockford base32: no I, L, O or U, so a reference read aloud or copied by hand survives.
export const REFERENCE_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
export const REFERENCE_SYMBOLS = 20;
/// Five bits a symbol.
export const REFERENCE_BITS = REFERENCE_SYMBOLS * 5;
const GROUP = '[0-9A-HJKMNP-TV-Z]{4}';
export const REFERENCE_PATTERN = new RegExp(`^EC-${GROUP}-${GROUP}-${GROUP}-${GROUP}-${GROUP}$`);

const refKey = (reference) => `caseref:${reference}`;
const refOfKey = (kind, id) => `caseref-of:${kind}:${id}`;
const claimKey = (kind, id) => `caserefs/${kind}/${id}`;

/// A new reference, from the cryptographic random source and nothing else. 256 is a
/// multiple of 32, so the low five bits of each byte make every symbol equally likely.
export function newCaseReference() {
  const bytes = crypto.getRandomValues(new Uint8Array(REFERENCE_SYMBOLS));
  let symbols = '';
  for (const b of bytes) symbols += REFERENCE_ALPHABET[b & 31];
  return `EC-${symbols.match(/.{4}/g).join('-')}`;
}

/// A reference as a person may type it (any case, spaces or dashes anywhere, with or
/// without "EC-", O for zero, I or L for one), written the one canonical way; null
/// when it cannot be one.
export function normalizeCaseReference(input) {
  if (typeof input !== 'string' || input.length > 64) return null;
  let s = input.toUpperCase().replace(/[\s-]/g, '');
  if (s.length === REFERENCE_SYMBOLS + 2 && s.startsWith('EC')) s = s.slice(2);
  s = s.replace(/O/g, '0').replace(/[IL]/g, '1');
  if (s.length !== REFERENCE_SYMBOLS || /[^0-9A-HJKMNP-TV-Z]/.test(s)) return null;
  return `EC-${s.match(/.{4}/g).join('-')}`;
}

function checkCase(kind, id) {
  if (!CASE_KINDS.includes(kind)) throw new Error(`unknown case kind ${kind}`);
  if (typeof id !== 'string' || !/^[A-Za-z0-9-]{1,64}$/.test(id)) throw new Error('a case id is 1 to 64 letters, digits or dashes');
}

/// The reference a case already has, or null.
export async function caseReferenceOf(env, kind, id) {
  checkCase(kind, id);
  const known = await env.SOCIAL.get(refOfKey(kind, id));
  return typeof known === 'string' && REFERENCE_PATTERN.test(known) ? known : null;
}

/// The case's reference, made and stored the first time it is asked for. Every later
/// call, from anywhere, returns the same one.
export async function assignCaseReference(env, kind, id) {
  const known = await caseReferenceOf(env, kind, id);
  if (known) return known;
  let reference = null;
  for (let attempt = 0; attempt < 4 && !reference; attempt++) {
    const candidate = newCaseReference();
    // Two cases drawing the same 100 bits will not happen; it is checked all the same.
    if (await env.SOCIAL.get(refKey(candidate))) continue;
    const claimed = await env.SCENES.put(claimKey(kind, id), candidate, { httpMetadata: { contentType: 'text/plain' }, onlyIf: { etagDoesNotMatch: '*' } });
    if (claimed) { reference = candidate; break; }
    // Another request claimed this case first: its reference is the case's.
    const winner = await env.SCENES.get(claimKey(kind, id));
    const text = winner ? (await winner.text()).trim() : '';
    if (REFERENCE_PATTERN.test(text)) reference = text;
  }
  if (!reference) throw new Error(`no reference could be assigned to ${kind} case ${id}`);
  // The reference's own record first, so a case never names a reference that names nothing.
  await env.SOCIAL.put(refKey(reference), JSON.stringify({ kind, id, at: new Date().toISOString() }));
  await env.SOCIAL.put(refOfKey(kind, id), reference);
  return reference;
}

/// `{ reference, kind, id }` for a reference that names a case, or null (not a reference,
/// or one no case has).
export async function resolveCaseReference(env, input) {
  const reference = normalizeCaseReference(input);
  if (!reference) return null;
  const raw = await env.SOCIAL.get(refKey(reference));
  if (!raw) return null;
  try {
    const { kind, id } = JSON.parse(raw);
    return CASE_KINDS.includes(kind) && typeof id === 'string' && id ? { reference, kind, id } : null;
  } catch {
    return null;
  }
}
