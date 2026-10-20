// =============================================================================
// Guest chat: the Worker's own deterministic rules
// =============================================================================
//
// Every chat line in a guest-only session (a guest's, and the host's own to its
// guests) is checked before the host delivers it (guest.mjs, POST
// /api/guest/chat/check; docs/architecture/GUEST_PLAY.md sections 13 and 16).
// Guests have no age check and may be children, so personal information and
// contact-seeking are refused by these rules before Jev is asked, whatever Jev
// would answer, and at no cost. The rules are fixed text patterns over the line
// in NFKC, lowercased, with invisible format characters removed. They catch
// exactly what is listed below and nothing else; anything they miss goes to Jev.
//
//   empty     nothing left once whitespace and invisible format characters
//             (Unicode category Cf: zero-width spaces, joiners, soft hyphens) are
//             removed. Not a refusal of the guest: the line is dropped.
//   invite    discord.gg, discord.com/invite and discordapp.com/invite, also with
//             the dot spelled ("discord dot gg") or the letters spaced out.
//   link      http or https, "www.", a word joined by a dot (no space after it)
//             to a common top-level domain (TLDS: "site.com", "emma.k.2015.gg"),
//             the same with the dot spelled or bracketed ("site dot com",
//             "site (dot) com", "site[.]com", "site . com"), and "dot com", "dot
//             net", "dot org", "dot gg", "dot io" on their own.
//   handle    an @handle ("@emma_2015": @ then 2 or more letters, digits, dots or
//             underscores, not inside a word) and a Discord tag ("name#1234").
//   social    a contact or social network named: discord, snap (not "oh snap"),
//             snapchat, insta, instagram, tiktok, telegram, whatsapp, kik,
//             facebook, twitter, skype (the long names also with their letters
//             spaced out); asking for or offering contact ("dm me", "pm me", "text
//             me", "call me", "message me", "my number", "my username", "your
//             snap", "ur discord" and the like).
//   phone     7 or more digits with only spaces, dots, brackets or hyphens
//             between them (moderation.mjs profilePersonalInfo, the forum rule).
//   email     an email address, written out or spelled with "at" and "dot"
//             (moderation.mjs profilePersonalInfo).
//   address   a street address: a house number, one to three words, then a street
//             word (STREET_WORDS: street, st, avenue, ave, road, rd, lane, drive,
//             ...): "42 maple street".
//   school    "I / we go to (or attend) <one to four words> <elementary, middle,
//             high, primary, secondary, junior, academy, school, prep,
//             kindergarten>", "my school is", "what school", "where do you go to
//             school".
//   place     "I live in / at / on / near / by", "I'm from <place>", "my address",
//             "my house is", "my town is", "my city is", "my street", "my zip",
//             "where do you live", "where are you from", "what city are you in".
//
// The answer is a reason code for the Worker's own count and log. It is never
// handed to the host, which is told deliver, block or kick only. Known misfires
// (each kicks the guest, as any personal-information refusal does): a 7-digit
// score written with spaces reads as a phone number; "I'm from the future" reads
// as a place; "42 lap way" reads as an address.
// =============================================================================

import { profilePersonalInfo } from './moderation.mjs';

/// Common top-level domains a link in chat ends in. A word joined to one of these by a dot reads as a link.
export const TLDS = Object.freeze([
  'com', 'net', 'org', 'gg', 'io', 'co', 'me', 'tv', 'xyz', 'ly', 'app', 'dev', 'link', 'info', 'biz', 'uk', 'ca', 'de',
  'ru', 'cc', 'tk', 'ml', 'ga', 'gl', 'site', 'online', 'chat', 'club', 'shop', 'store', 'page', 'edu', 'gov', 'au', 'nz',
  'fr', 'nl', 'br', 'jp', 'cn', 'gay', 'lol',
]);
const TLD = `(?:${TLDS.join('|')})`;
/// A dot, written or spelled: "." with no space after it (so the end of a sentence is not one), " . ", " dot ",
/// "(dot)", "[dot]", "(.)", "[.]".
const DOT = '(?:\\.(?!\\s)|\\s+\\.\\s+|\\s*[(\\[]\\s*(?:\\.|dot)\\s*[)\\]]\\s*|\\s+dot\\s+)';

const LINK = [
  /\bhttps?\b/,
  /\bwww\s*\./,
  new RegExp(`[a-z0-9-]+${DOT}${TLD}(?![a-z0-9])`),
  /\bdot\s*(?:com|net|org|gg|io)\b/,
];

const INVITE = [/discord\s*(?:\.|dot)\s*gg/, /discord(?:app)?\s*(?:\.|dot)\s*com\s*\/?\s*invite/];
/// The same invites with everything but letters and digits removed ("d i s c o r d . g g").
const INVITE_SQUASHED = ['discordgg', 'discordcominvite', 'discordappcominvite'];

const HANDLE = [/(?:^|[^a-z0-9])@[a-z0-9_.]{2,}/, /[a-z0-9_.]#\d{4}(?![0-9])/];

const SOCIAL = new RegExp('(?:^|[^a-z0-9])(?:discord|(?<!oh )snap|snapchat|insta|instagram|tik\\s*tok|telegram|whats\\s*app|kik|facebook|twitter|skype)(?:$|[^a-z0-9])');
/// Long names that still read as themselves with the spaces and dots taken out ("s n a p c h a t").
const SOCIAL_SQUASHED = ['snapchat', 'instagram', 'tiktok', 'telegram', 'whatsapp', 'discord', 'facebook'];
const CONTACT = [
  /\b(?:dm|pm|text|call|message|msg|ring)\s+(?:me|u|you)\b/,
  /\bmy\s+(?:number|phone|cell|mobile|email|e-mail|address|addy|user\s*name|handle)\b/,
  /\b(?:your|ur|yur|you're)\s+(?:number|phone|cell|email|address|addy|snap|insta|discord|handle|username|user\s*name)\b/,
];

export const STREET_WORDS = Object.freeze([
  'street', 'st', 'avenue', 'ave', 'road', 'rd', 'lane', 'ln', 'drive', 'dr', 'boulevard', 'blvd', 'court', 'ct', 'way',
  'place', 'pl', 'circle', 'cir', 'terrace', 'parkway', 'pkwy', 'highway', 'hwy', 'crescent', 'close', 'apt', 'apartment',
]);
const ADDRESS = new RegExp(`\\b\\d{1,6}\\s+(?:[a-z]+\\s+){1,3}(?:${STREET_WORDS.join('|')})\\b`);

const SCHOOL_KINDS = '(?:elementary|middle|high|primary|secondary|junior|academy|school|prep|kindergarten)';
const SCHOOL = [
  new RegExp(`\\b(?:i|we)\\s+(?:go|goes|went)\\s+to\\s+(?:[a-z.'-]+\\s+){1,4}${SCHOOL_KINDS}\\b`),
  new RegExp(`\\b(?:i|we)\\s+attend\\s+(?:[a-z.'-]+\\s+){1,4}${SCHOOL_KINDS}\\b`),
  /\bmy\s+school\s+(?:is|name|called)\b/,
  /\b(?:what|which|wat|wut)\s+school\b/,
  /\bwhere\s+(?:do|d)\s*(?:you|u|ya)\s+go\s+to\s+school\b/,
];

const PLACE = [
  /\bi\s*(?:'?m|am)?\s*(?:live|living)\s+(?:in|at|on|near|by)\b/,
  /\bi\s*(?:'?m|am)\s+from\s+[a-z]/,
  /\bmy\s+(?:address|house\s+is|home\s+is|town\s+is|city\s+is|street|zip|postcode|post\s+code)\b/,
  /\bwhere\s+(?:do|d)?\s*(?:you|u|ya)\s+(?:live|stay|from)\b/,
  /\bwhere\s+(?:are|r)\s+(?:you|u|ya)\s+from\b/,
  /\b(?:what|which)\s+(?:city|town|state|country)\s+(?:do|are|r)\s+(?:you|u)\b/,
];

/// The line as the rules read it: NFKC, lowercase, invisible format characters removed, whitespace folded.
export function chatRulesText(text) {
  if (typeof text !== 'string') return '';
  return text.normalize('NFKC').toLowerCase().replace(/\p{Cf}/gu, '').replace(/\s+/g, ' ').trim();
}

/// Why the Worker's own rules refuse a guest chat line, or null when they let it go on to Jev. `empty` drops the line
/// without kicking anyone; every other code is a personal-information refusal (GUEST_PLAY.md section 13).
export function guestChatRule(text) {
  const t = chatRulesText(text);
  if (!t) return 'empty';
  const squashed = t.replace(/[^a-z0-9]/g, '');
  if (INVITE.some((r) => r.test(t)) || INVITE_SQUASHED.some((w) => squashed.includes(w))) return 'invite';
  if (LINK.some((r) => r.test(t))) return 'link';
  if (HANDLE.some((r) => r.test(t))) return 'handle';
  if (SOCIAL.test(t) || SOCIAL_SQUASHED.some((w) => squashed.includes(w)) || CONTACT.some((r) => r.test(t))) return 'social';
  const personal = profilePersonalInfo(t);
  if (personal) return personal;
  if (ADDRESS.test(t)) return 'address';
  if (SCHOOL.some((r) => r.test(t))) return 'school';
  if (PLACE.some((r) => r.test(t))) return 'place';
  return null;
}

/// Every reason code guestChatRule answers. `empty` is dropped without a kick; the rest are personal information.
export const GUEST_CHAT_RULES = Object.freeze(['empty', 'invite', 'link', 'handle', 'social', 'phone', 'email', 'address', 'school', 'place']);
