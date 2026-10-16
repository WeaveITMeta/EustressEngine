// =============================================================================
// Listing tags: a listing's categories
// =============================================================================
//
// `tags` are a listing's only categories: free-form hashtags (#zombies #tycoon
// #coop), at most 8, each one a lowercase word of letters and digits, hyphens
// allowed between them. Studio's Publish dialog and the MCP publish tool send
// them; the site shows them as chips that filter the gallery. An author may
// leave them empty, and the review's judge then chooses up to 5
// (`tags_source: "judge"`). docs/networking/LISTING_TAGS.md is the contract.
//
// A listing from before tags carried one `genre`. It is read as a tag (lazily,
// on read: listingTags) and written into `tags` on the listing's next write
// (withTagsWrittenBack). `genre` is served one more release as a deprecated
// mirror (genreTag).
//
// This file is the one place the rules live. Nothing in it touches storage or
// the network, so the Worker, the commit route and the tests all read the same
// rules from here.

export const MAX_TAGS = 8;
export const TAG_MIN = 2;
export const TAG_MAX = 24;
/// The most tags the review's judge chooses, leaving room for 3 of the author's own.
export const JUDGE_TAGS_MAX = 5;
/// Who chose a listing's tags.
export const TAG_SOURCES = ['author', 'judge'];

/// Letters a to z and digits, one hyphen at a time between them: no space, no
/// emoji, no underscore, no hyphen at either end, no double hyphen.
const TAG_SHAPE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

// Personal information is never a tag. The check is narrow on purpose, and it is
// not a word list (slurs and insults are a reviewer's call, asked of Jev):
//   - a phone number: 7 or more digits in a row, where single hyphens between
//     digit groups do not break the row (`5551234`, `555-1234`, `call-555-1234`).
//     A year range (two groups of 4 digits, `1939-1945`) is not a phone number;
//   - an email address: an `@` with text on both sides and a dot after it, or
//     the same spelled with words (`ann-at-mail-com`, `ann-at-mail-dot-com`);
//   - a handle: text that starts with `@`.
// The tag shape already refuses `@` and `.`, so the email and handle checks read
// what was sent, before it is normalized, and say why in plain words.
const PHONE_DIGITS = 7;
const LONG_DIGITS = new RegExp(`\\d{${PHONE_DIGITS},}`);
const YEAR_RANGE = /^\d{4}$/;
const EMAIL_SHAPE = /[^\s@]@[^\s@]+\.[^\s@]/;
const HANDLE_SHAPE = /^@[^\s@]/;
const SPELLED_EMAIL = /(?:^|-)at-(?:[a-z0-9]+-)+(?:dot-)?(?:com|net|org|edu|gov|io|co|me|us|uk|de|fr|ca|au|info|biz)$/;

function phoneShaped(tag) {
  let run = [];
  const ends = () => {
    const digits = run.join('').length;
    const yearRange = run.length === 2 && run.every((g) => YEAR_RANGE.test(g));
    const phone = digits >= PHONE_DIGITS && !yearRange;
    run = [];
    return phone;
  };
  for (const part of tag.split('-')) {
    if (/^\d+$/.test(part)) { run.push(part); continue; }
    if (ends()) return true;
    if (LONG_DIGITS.test(part)) return true;
  }
  return ends();
}

/// What kind of personal information `raw` carries: 'phone', 'email', 'handle', or null. Read on the text as sent and on
/// the tag it normalizes to, so a full-width or `#`-prefixed form is caught too.
export function personalInfoIn(raw) {
  if (typeof raw !== 'string') return null;
  const text = raw.normalize('NFKC').trim();
  if (EMAIL_SHAPE.test(text)) return 'email';
  if (HANDLE_SHAPE.test(text.replace(/^#/, ''))) return 'handle';
  let tag = text.toLowerCase();
  if (tag.startsWith('#')) tag = tag.slice(1);
  if (!TAG_SHAPE.test(tag)) return null;
  if (SPELLED_EMAIL.test(tag)) return 'email';
  return phoneShaped(tag) ? 'phone' : null;
}

const PERSONAL_INFO_WORDS = { phone: 'a phone number', email: 'an email address', handle: 'a handle' };

/// A request that names more entries than this is refused as too many without
/// reading them. The limit that matters is MAX_TAGS after duplicates are
/// dropped; this only keeps a hostile list from costing a scan.
const MAX_ENTRIES_READ = 256;

/// The most of a refused tag a message repeats.
const ECHO_MAX = 40;

export const TAG_RULES = `A tag is ${TAG_MIN} to ${TAG_MAX} characters: lowercase letters a to z and digits, with single hyphens between them, and an optional leading #. A tag is never a phone number, an email address or a handle. A listing holds at most ${MAX_TAGS} tags.`;

/// One tag as the listing stores it: Unicode NFKC, trimmed, lower-cased, one
/// leading `#` removed. `null` when what is left is not a valid tag, or is
/// personal information (personalInfoIn). Full-width letters fold to ASCII; a
/// space inside, an emoji, an underscore, a letter outside a to z and a lone `#`
/// do not.
export function normalizeTag(raw) {
  if (typeof raw !== 'string') return null;
  let tag = raw.normalize('NFKC').trim().toLowerCase();
  if (tag.startsWith('#')) tag = tag.slice(1);
  if (tag.length < TAG_MIN || tag.length > TAG_MAX || !TAG_SHAPE.test(tag)) return null;
  return SPELLED_EMAIL.test(tag) || phoneShaped(tag) ? null : tag;
}

// What a message may repeat of a refused tag. Quotes, angle brackets, the ampersand, the backslash, the backtick,
// control characters, invisible and direction-changing characters and a lone surrogate become a six-character
// escape (a backslash, a u and four hex digits), so a message shown on a page cannot carry markup or hide text.
// Letters, digits and emoji stay readable. Written with code points, not escapes, so no tool that decodes an
// escape in this file can change what it means.
const BACKSLASH = String.fromCharCode(92);
const ELLIPSIS = String.fromCharCode(0x2026);
const UNSAFE_CHARS = new Set(['"', "'", '<', '>', '&', BACKSLASH, String.fromCharCode(96)]);
const UNSAFE_RANGES = [
  [0x00, 0x1f], // C0 controls
  [0x7f, 0x9f], // DEL and C1 controls
  [0x200b, 0x200f], // zero-width characters and direction marks
  [0x2028, 0x202e], // line and paragraph separators, direction overrides
  [0x2060, 0x2069], // word joiner and direction isolates
  [0xfeff, 0xfeff], // byte order mark
  [0xd800, 0xdfff], // a lone surrogate (a pair is one code point above this range)
];

function unsafeInMessage(point) {
  const code = point.codePointAt(0);
  return UNSAFE_CHARS.has(point) || UNSAFE_RANGES.some(([low, high]) => code >= low && code <= high);
}

/// At most ECHO_MAX characters of `raw`, escaped for display, with an ellipsis
/// when there was more.
function echo(raw) {
  const text = String(raw);
  const points = Array.from(text.slice(0, ECHO_MAX * 2));
  const more = points.length > ECHO_MAX || text.length > ECHO_MAX * 2;
  const shown = points.slice(0, ECHO_MAX)
    .map((point) => (unsafeInMessage(point) ? `${BACKSLASH}u${point.codePointAt(0).toString(16).padStart(4, '0')}` : point))
    .join('');
  return more ? shown + ELLIPSIS : shown;
}

const kindOf = (v) => (v === null ? 'null' : Array.isArray(v) ? 'a list' : `a ${typeof v}`);

const refuse = (error) => ({ ok: false, error });

/// `{ ok: true, tags }` for a list of strings that each normalize to a valid
/// tag and, with duplicates dropped, number at most MAX_TAGS (order kept), or
/// `{ ok: false, error }`. Nothing is dropped to make a list fit: the first bad
/// tag refuses the whole list, and the message names it. An empty list is valid
/// (it clears the tags). `null` and `undefined` are not lists: a caller treats
/// those as "not sent" before it asks.
export function checkTags(value) {
  if (!Array.isArray(value)) return refuse(`tags must be a list of strings. ${TAG_RULES}`);
  if (value.length > MAX_ENTRIES_READ) return refuse(`A listing holds at most ${MAX_TAGS} tags, and this request names ${value.length} entries. ${TAG_RULES}`);
  const tags = [];
  for (let i = 0; i < value.length; i++) {
    const raw = value[i];
    if (typeof raw !== 'string') return refuse(`Entry ${i + 1} of tags must be a string, not ${kindOf(raw)}. ${TAG_RULES}`);
    // Personal information is named by its kind and never repeated back.
    const personal = personalInfoIn(raw);
    if (personal) return refuse(`Entry ${i + 1} of tags looks like ${PERSONAL_INFO_WORDS[personal]}, and a tag never carries personal information. ${TAG_RULES}`);
    const tag = normalizeTag(raw);
    if (tag === null) return refuse(`Tag "${echo(raw)}" is not a valid tag. ${TAG_RULES}`);
    if (!tags.includes(tag)) tags.push(tag);
  }
  if (tags.length > MAX_TAGS) return refuse(`A listing holds at most ${MAX_TAGS} tags, and this request names ${tags.length}. ${TAG_RULES}`);
  return { ok: true, tags };
}

/// The genre a listing from before tags carried, as a tag: normalized like a tag, or null when there is none, when it is
/// "all" (the old default, which named no category), or when it is not a valid tag. This is also the deprecated `genre`
/// mirror a listing is served with for one release.
export function genreTag(sim) {
  const tag = normalizeTag(sim?.genre);
  return tag === 'all' ? null : tag;
}

/// The tags a stored listing carries: always a list, empty for a listing with none. A listing from before tags is read
/// with its genre as one more tag (genreTag), when that tag is not already there and the list has room under MAX_TAGS.
/// Once the listing has been written since (`genre_migrated`), its `tags` are the whole answer.
export function listingTags(sim) {
  const tags = Array.isArray(sim?.tags) ? sim.tags.filter((t) => typeof t === 'string') : [];
  if (sim?.genre_migrated === true) return tags;
  const genre = genreTag(sim);
  return genre !== null && !tags.includes(genre) && tags.length < MAX_TAGS ? [...tags, genre] : tags;
}

/// The listing as it is next written: a genre from before tags is written into `tags`, once, the way listingTags reads it.
/// Every route that writes a listing record calls this, so the lazy migration becomes the stored one on the next write.
/// Returns the same object.
export function withTagsWrittenBack(sim) {
  if (!sim || typeof sim !== 'object' || sim.genre_migrated === true || genreTag(sim) === null) return sim;
  sim.tags = listingTags(sim);
  sim.genre_migrated = true;
  return sim;
}

/// Who chose the listing's tags: "judge" when the review chose them, "author" otherwise (an empty list included).
export function tagsSource(sim) {
  return sim?.tags_source === 'judge' && listingTags(sim).length > 0 ? 'judge' : 'author';
}

/// Whether the review chooses this listing's tags: the author gave none, or the ones it has are the review's own.
export function reviewChoosesTags(sim) {
  return listingTags(sim).length === 0 || tagsSource(sim) === 'judge';
}

/// The category fields every listing a route returns carries: `tags` (the only category), `tags_source`, and `genre`, the
/// deprecated mirror (the migrated genre, or null).
export function listingTagFields(sim) {
  return { tags: listingTags(sim), tags_source: tagsSource(sim), genre: genreTag(sim) };
}

/// An author's tags written to a listing (create, commit, the tags route). `tags` are already checked (checkTags). The
/// same set the listing has keeps its source and is no change: a judge's tags sent back as they are stay the judge's, in
/// the order sent. Any other set is the author's, and is a change the review reads again. Returns whether it changed.
export function setAuthorTags(sim, tags) {
  const changed = !sameTagSet(listingTags(sim), tags);
  const judges = !changed && tagsSource(sim) === 'judge';
  sim.tags = tags;
  sim.tags_source = judges ? 'judge' : 'author';
  sim.genre_migrated = true;
  return changed;
}

/// The judge's suggested tags as they are written: each one normalized like an author's (personal information included),
/// the invalid ones dropped, duplicates dropped, at most `max` (JUDGE_TAGS_MAX), in the judge's order.
export function judgeTags(values, max = JUDGE_TAGS_MAX) {
  const tags = [];
  for (const value of Array.isArray(values) ? values : []) {
    const tag = normalizeTag(value);
    if (tag !== null && !tags.includes(tag)) tags.push(tag);
  }
  return tags.slice(0, max);
}

/// The tag a gallery request's deprecated `?genre=` asks for, as `?tag=` would: none for an empty value or "all".
export function genreAliasValues(values) {
  return (values || []).filter((v) => typeof v === 'string' && v.trim() !== '' && normalizeTag(v) !== 'all');
}

/// The tags as moderation shows them to the classifier and the judge, beside the
/// listing's name and description: the stored list, capped at the limits a
/// stored tag already meets, so a record written some other way cannot swell
/// the text a model reads.
export function tagsForReview(sim) {
  return listingTags(sim).slice(0, MAX_TAGS).map((t) => t.slice(0, TAG_MAX));
}

/// Whether two tag lists hold the same tags, in any order. Reordering the tags
/// of a listing is not a change a reviewer has to read again.
export function sameTagSet(a, b) {
  return a.length === b.length && a.every((t) => b.includes(t));
}

/// The tags a gallery request asks for (`?tag=` repeated; every one must be on
/// a listing). A value is normalized like a stored tag. A value that is empty
/// after trimming asks nothing, as an empty `genre=` does. Any other value that
/// is not a valid tag can match no listing: `none` is true and the request
/// answers with an empty list, never an error.
export function tagFilter(values) {
  const tags = [];
  for (const value of values || []) {
    if (typeof value !== 'string' || value.trim() === '') continue;
    const tag = normalizeTag(value);
    if (tag === null) return { none: true, tags: [] };
    if (!tags.includes(tag)) tags.push(tag);
  }
  return { none: false, tags };
}

/// Whether a free-text search (already lower-cased) matches any of a listing's
/// tags: a substring of a tag, with or without the leading `#`.
export function queryMatchesTag(query, tags) {
  const needle = String(query).normalize('NFKC').toLowerCase().replace(/^#/, '');
  return needle !== '' && tags.some((t) => t.includes(needle));
}
