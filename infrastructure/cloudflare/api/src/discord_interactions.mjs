// =============================================================================
// The Eustress Discord app: slash commands, follow posts, registration
// =============================================================================
//
//   POST /api/discord/interactions   Discord's interactions endpoint. No session:
//                                    every request must carry a valid Ed25519
//                                    signature (X-Signature-Ed25519 over
//                                    X-Signature-Timestamp + the raw body) under
//                                    DISCORD_PUBLIC_KEY, made within the last five
//                                    minutes, or it is answered 401 and nothing is
//                                    read from it. PING answers PONG.
//
// Commands (COMMANDS, registered by the five-minute tick):
//
//   /space query     a listed Space as a card: title, creator, rating, thumbnail,
//                    live players, and links to its page and its Play page.
//                    `query` autocompletes up to 25 listed Spaces.
//   /live            up to 10 listed Spaces live now, each with a Play link.
//   /profile user    the public profile of a Discord member who linked an
//                    Eustress account (src/discord_link.mjs); a member who did not
//                    is told so.
//   /me              only the person who asks sees it: Bliss, Tickets, unread
//                    notifications; or a link to Settings when not linked.
//   /follow          a server's Manage Server members make a channel follow a Space
//                    or a creator (space, creator, list, remove). The app must be
//                    installed to the server; the permission is checked here, not
//                    only by Discord's default.
//
// Ratings. Every surface starts at `ages_10` (SIGNED_OUT_CAP). A higher cap applies
// only to a person who is linked to an Eustress account whose verified age is 18 or
// over, AND who asks in an age-restricted server channel (channel.nsfw) or in a DM
// with the app (context 1). Then the cap is that account's own viewer cap
// (src/adult_content.mjs viewerCap). A group DM, a normal channel, and an
// ephemeral reply in a normal channel stay at `ages_10`. A listing whose rating is
// missing or unknown is never shown. An adult listing's image is never put in
// Discord: its image URLs are signed and short-lived on the site, and Discord keeps
// copies.
//
// Follow posts (sweepDiscordFollows, five-minute tick): listings listed or updated
// since the cursor, and listed listings that went live, are posted to each channel
// that follows the listing or its creator, once per channel and event. `ages_10` in
// a normal channel; up to `mature_17` in a channel that was age-restricted when it
// was followed; never above `mature_17`. Discord answering 403 or 404 disables the
// follow. The same sweep updates the Linked Roles connection of each creator whose
// listed Spaces changed (src/discord_link.mjs repushRoleConnection).
//
// Storage, SOCIAL KV:
//   discordfollow:{kind}:{target}:{guild}:{channel}   the follow record
//   discordfollowg:{guild}:{channel}:{kind}:{target}  the server's index of it
//   discordposted:{sha256(channel + event)}           a post made (30 days)
//   discordlisted:{simId}                             a listing posted as new once
//   discordsweep:cursor, discordsweep:counts          the sweep's place and the
//                                                     listed counts last pushed
//   discordreg:commands, discordreg:metadata          hashes of what was registered
//
// Config: DISCORD_PUBLIC_KEY ([vars], not secret), DISCORD_CLIENT_ID and
// DISCORD_CLIENT_SECRET (the link), DISCORD_BOT_TOKEN (registration and posts).
// =============================================================================

import { siteOrigin } from './cors.mjs';
import { BANDS as RATINGS } from './rating_questionnaire.mjs';
import { notifPrefix } from './moderation.mjs';
import { SIGNED_OUT_CAP, viewerFor, viewerCap, resolveDob, ageAt } from './adult_content.mjs';
import { ledgerBalanceMinor, ledgerDeriveMinor, formatBliss } from './bliss.mjs';
import { walletBalance } from './commerce.mjs';
import { publicProfileOf } from './account_profile.mjs';
import { cachedValue, defaultCache } from './body_cache.mjs';
import { DISCORD_API, ROLE_METADATA, linkedAccountFor, repushRoleConnection } from './discord_link.mjs';

export { ROLE_METADATA };

/// A signature older or newer than this, against the Worker's clock, is refused.
export const SIGNATURE_WINDOW_S = 300;
/// Discord's interaction payloads are a few kilobytes.
const MAX_BODY_BYTES = 64 * 1024;
const EPHEMERAL = 64;
const MANAGE_GUILD = 1n << 5n;
const ADMINISTRATOR = 1n << 3n;
/// The highest rating a follow post reaches in a channel that was age-restricted when followed; others get SIGNED_OUT_CAP.
export const POST_CAP_RESTRICTED = 'mature_17';
export const FOLLOWS_PER_GUILD = 25;
export const POSTS_PER_RUN = 50;
export const ROLE_PUSHES_PER_RUN = 20;
const POSTED_TTL_S = 30 * 86400;
/// The sweep reads again what changed in the two minutes before its last run: KV is eventually consistent.
const CURSOR_OVERLAP_MS = 120_000;
/// How long a listing read for autocomplete is kept, per rating slice.
const LISTING_CACHE_SECONDS = 60;
const BRAND_COLOR = 0x00bcd4;

const HEX64 = /^[0-9a-fA-F]{64}$/;
const HEX128 = /^[0-9a-fA-F]{128}$/;
const SNOWFLAKE = /^[0-9]{5,25}$/;
const INTERACTION_TOKEN = /^[A-Za-z0-9._-]{1,500}$/;
const TARGET_ID = /^[a-f0-9-]{1,64}$/;

const RATING_WORDS = { all_ages: 'All ages', ages_10: 'Ages 10+', teen_13: 'Ages 13+', mature_17: 'Ages 17+', adult_18: 'Adults only (18+)', adult_21: 'Adults only (21+)' };
const ratingIdx = (r) => RATINGS.indexOf(r);
/// Whether a card's rating is known and at or under `cap`. A missing or unknown rating is never within any cap here.
export const withinCap = (rating, cap) => RATINGS.includes(rating) && RATINGS.includes(cap) && ratingIdx(rating) <= ratingIdx(cap);
const lowerCap = (a, b) => (ratingIdx(a) <= ratingIdx(b) ? a : b);

const ANYWHERE = { integration_types: [0, 1], contexts: [0, 1, 2] };

/// The slash commands, as PUT to /applications/{id}/commands.
export const COMMANDS = Object.freeze([
  {
    name: 'space', type: 1, description: 'Show a listed Eustress Space', ...ANYWHERE,
    options: [{ type: 3, name: 'query', description: 'Space name or creator', required: true, autocomplete: true, max_length: 100 }],
  },
  { name: 'live', type: 1, description: 'Eustress Spaces live right now', ...ANYWHERE },
  {
    name: 'profile', type: 1, description: 'Show the Eustress profile a Discord member linked', ...ANYWHERE,
    options: [{ type: 6, name: 'user', description: 'The Discord member', required: true }],
  },
  { name: 'me', type: 1, description: 'Your Eustress balances and notifications (only you see this)', ...ANYWHERE },
  {
    name: 'follow', type: 1, description: 'Post Eustress Space updates in this channel',
    integration_types: [0], contexts: [0], default_member_permissions: '32',
    options: [
      { type: 1, name: 'space', description: 'Post when a Space is updated or goes live', options: [{ type: 3, name: 'space', description: 'The Space', required: true, autocomplete: true, max_length: 100 }] },
      { type: 1, name: 'creator', description: 'Post when a creator lists, updates or hosts a Space', options: [{ type: 3, name: 'username', description: 'The creator\'s Eustress username', required: true, max_length: 64 }] },
      { type: 1, name: 'list', description: 'Show what this server follows' },
      { type: 1, name: 'remove', description: 'Stop a follow in this channel', options: [{ type: 3, name: 'name', description: 'The follow to stop', required: true, autocomplete: true, max_length: 100 }] },
    ],
  },
]);
const COMMAND_NAMES = new Set(COMMANDS.map((c) => c.name));

// -----------------------------------------------------------------------------
// Signature
// -----------------------------------------------------------------------------

const hexBytes = (hex) => Uint8Array.from(hex.match(/../g).map((h) => parseInt(h, 16)));

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// Whether `signatureHex` is the Ed25519 signature of `timestamp + body` under `publicKeyHex`, and `timestamp` (Unix
/// seconds) is within SIGNATURE_WINDOW_S of `nowMs`. Any malformed part, or a failure inside WebCrypto, is false.
export async function verifyDiscordSignature(publicKeyHex, signatureHex, timestamp, body, nowMs = Date.now()) {
  try {
    if (!HEX64.test(publicKeyHex || '') || !HEX128.test(signatureHex || '') || !/^[0-9]{1,12}$/.test(timestamp || '')) return false;
    if (Math.abs(nowMs / 1000 - Number(timestamp)) > SIGNATURE_WINDOW_S) return false;
    const key = await crypto.subtle.importKey('raw', hexBytes(publicKeyHex), { name: 'Ed25519' }, false, ['verify']);
    return await crypto.subtle.verify({ name: 'Ed25519' }, key, hexBytes(signatureHex), new TextEncoder().encode(timestamp + body));
  } catch {
    return false;
  }
}

// -----------------------------------------------------------------------------
// Who asks, and the cap that follows
// -----------------------------------------------------------------------------

/// Whether the account's verified age is 18 or over: an identity record that passed, and an age it establishes. Any
/// failure is no.
export async function isVerifiedAdult(env, account) {
  try {
    const raw = await env?.KYC_STATUS?.get(`kyc-${account.id}-front`);
    if (!raw) return false;
    const kyc = JSON.parse(raw);
    if (kyc?.status !== 'verified' && kyc?.status !== 'linked') return false;
    if (kyc.age_verified === true) return true;
    const age = ageAt(resolveDob(kyc, account.user).dob);
    return age !== null && age >= 18;
  } catch {
    return false;
  }
}

const discordUserOf = (interaction) => interaction?.member?.user || interaction?.user || null;

/// `{ cap, account }` for one interaction. See the header: `ages_10` unless a linked, verified adult asks in an
/// age-restricted guild channel or the app's DM, and then that account's own viewer cap.
export async function interactionScope(env, interaction) {
  const discordId = discordUserOf(interaction)?.id;
  const account = discordId ? await linkedAccountFor(env, discordId) : null;
  const privateSurface = interaction?.context === 1 || (interaction?.context === 0 && interaction?.channel?.nsfw === true);
  let cap = SIGNED_OUT_CAP;
  if (account && privateSurface && (await isVerifiedAdult(env, account))) {
    const own = viewerCap(await viewerFor(env, account.id));
    if (RATINGS.includes(own)) cap = own;
  }
  return { cap, account };
}

/// The highest rating a follow in this channel may post: `mature_17` when the channel is age-restricted, else `ages_10`.
const channelPostCap = (ageRestricted) => (ageRestricted === true ? POST_CAP_RESTRICTED : SIGNED_OUT_CAP);

// -----------------------------------------------------------------------------
// Reading listings (index.js hands in the Gallery's own reads)
// -----------------------------------------------------------------------------

/// Listed listings' gallery cards at `cap`, re-checked here: a card whose rating is missing or above the cap is dropped.
async function listedViews(deps, cap) {
  if (!deps?.listViews) return [];
  const views = await deps.listViews(cap);
  return Array.isArray(views) ? views.filter((v) => v && typeof v.id === 'string' && withinCap(v.rating, cap)) : [];
}

/// listedViews, kept for LISTING_CACHE_SECONDS per rating slice (autocomplete has three seconds to answer).
async function cachedListedViews(deps, cap, origin) {
  const views = await cachedValue(deps?.cache === undefined ? defaultCache() : deps.cache, `${origin}/__cache/discord-listed?rating_max=${cap}`, LISTING_CACHE_SECONDS, () => listedViews(deps, cap));
  return Array.isArray(views) ? views.filter((v) => withinCap(v?.rating, cap)) : [];
}

/// The live sessions of listed listings at `cap`, each joined to its card (so its rating is checked here too).
async function liveWithViews(deps, cap, views) {
  if (!deps?.liveSessions) return [];
  const sessions = await deps.liveSessions(cap);
  if (!Array.isArray(sessions)) return [];
  const byId = new Map(views.map((v) => [v.id, v]));
  return sessions.filter((s) => byId.has(s?.id)).map((s) => ({ ...s, view: byId.get(s.id) }));
}

const norm = (text) => String(text || '').normalize('NFKC').trim().toLowerCase();

/// Cards matching `query`: the exact id first, then names that equal, start with, or contain it, then creators.
function matching(views, query) {
  const q = norm(query);
  if (!q) return views.slice();
  const exact = views.filter((v) => v.id === String(query).trim());
  if (exact.length) return exact;
  const score = (v) => {
    const name = norm(v.name);
    if (name === q) return 0;
    if (name.startsWith(q)) return 1;
    if (name.includes(q)) return 2;
    if (norm(v.author_name).includes(q)) return 3;
    return -1;
  };
  return views.map((v) => [score(v), v]).filter(([s]) => s >= 0).sort((a, b) => a[0] - b[0]).map(([, v]) => v);
}

// -----------------------------------------------------------------------------
// Messages
// -----------------------------------------------------------------------------

const clip = (text, max) => {
  const s = String(text ?? '').trim();
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
};

function message({ content, embeds, buttons, ephemeral } = {}) {
  const rows = [];
  for (let i = 0; i < (buttons || []).length && rows.length < 5; i += 5) rows.push({ type: 1, components: buttons.slice(i, i + 5) });
  return {
    ...(content ? { content: clip(content, 2000) } : {}),
    embeds: (embeds || []).slice(0, 10),
    components: rows,
    allowed_mentions: { parse: [] },
    ...(ephemeral ? { flags: EPHEMERAL } : {}),
  };
}

const link = (label, url) => ({ type: 2, style: 5, label: clip(label, 80), url });
const spacePage = (origin, id) => `${origin}/simulation/${encodeURIComponent(id)}`;
const playPage = (origin, id) => `${origin}/play/${encodeURIComponent(id)}`;

/// A public image for a card: an https URL, and never an adult listing's (see the header).
function safeImage(view) {
  const url = view?.thumbnail_url;
  if (typeof url !== 'string' || !url.startsWith('https://')) return null;
  return withinCap(view.rating, POST_CAP_RESTRICTED) ? url : null;
}

/// One listing as an embed and its two links.
export function spaceCard(view, origin, { players = 0 } = {}) {
  const fields = [
    { name: 'Creator', value: clip(view.author_name || 'Unknown', 256), inline: true },
    { name: 'Rating', value: RATING_WORDS[view.rating] || 'Not rated', inline: true },
  ];
  if (Number.isSafeInteger(players) && players > 0) fields.push({ name: 'Live now', value: `${players} playing`, inline: true });
  const image = safeImage(view);
  return {
    embed: {
      title: clip(view.name || 'Untitled Space', 256),
      url: spacePage(origin, view.id),
      ...(view.description ? { description: clip(view.description, 300) } : {}),
      color: BRAND_COLOR,
      fields,
      ...(image ? { thumbnail: { url: image } } : {}),
    },
    buttons: [link('View on Eustress', spacePage(origin, view.id)), link('Play', playPage(origin, view.id))],
  };
}

// -----------------------------------------------------------------------------
// Options
// -----------------------------------------------------------------------------

/// `{ sub, values, focused }`: the subcommand (if any), its option values by name, and the focused option's name.
function optionsOf(data) {
  let opts = Array.isArray(data?.options) ? data.options : [];
  let sub = null;
  if (opts.length === 1 && opts[0]?.type === 1) {
    sub = opts[0].name;
    opts = Array.isArray(opts[0].options) ? opts[0].options : [];
  }
  const values = {};
  let focused = null;
  for (const o of opts) {
    if (typeof o?.name !== 'string') continue;
    values[o.name] = o.value;
    if (o.focused) focused = o.name;
  }
  return { sub, values, focused };
}

// -----------------------------------------------------------------------------
// The commands
// -----------------------------------------------------------------------------

async function spaceCommand(env, deps, scope, values, origin) {
  const views = await listedViews(deps, scope.cap);
  const found = matching(views, values.query)[0];
  if (!found) return message({ content: 'No listed Space matches that.' });
  const live = await liveWithViews(deps, scope.cap, [found]).catch(() => []);
  const card = spaceCard(found, origin, { players: live[0]?.players || 0 });
  return message({ embeds: [card.embed], buttons: card.buttons });
}

async function liveCommand(env, deps, scope, origin) {
  const views = await listedViews(deps, scope.cap);
  const live = (await liveWithViews(deps, scope.cap, views)).slice(0, 10);
  if (!live.length) return message({ content: 'No listed Spaces are live right now.' });
  const embeds = live.map((s) => ({
    title: clip(s.view.name || 'Untitled Space', 256),
    url: spacePage(origin, s.id),
    color: BRAND_COLOR,
    fields: [
      { name: 'Creator', value: clip(s.view.author_name || 'Unknown', 256), inline: true },
      { name: 'Players', value: `${s.players}/${s.max_players}`, inline: true },
      { name: 'Rating', value: RATING_WORDS[s.view.rating] || 'Not rated', inline: true },
    ],
  }));
  return message({ content: `Live now on Eustress: ${live.length}`, embeds, buttons: live.map((s) => link(`Play ${s.view.name || 'Space'}`, playPage(origin, s.id))) });
}

export const NOT_LINKED_PROFILE = 'This person has not linked a Eustress account.';

async function profileCommand(env, deps, scope, values, origin) {
  const target = String(values.user || '');
  const account = SNOWFLAKE.test(target) ? await linkedAccountFor(env, target) : null;
  if (!account) return message({ content: NOT_LINKED_PROFILE });
  const shown = await publicProfileOf(env, account.id, account.user);
  const listed = (await listedViews(deps, scope.cap)).filter((v) => v.author_id === account.id).length;
  const username = account.user.username || '';
  const url = `${origin}/profile/${encodeURIComponent(username)}`;
  const avatar = typeof shown.avatar_url === 'string' && shown.avatar_url.startsWith('/assets/') ? `${origin}${shown.avatar_url}` : null;
  const embed = {
    title: clip(shown.display_name || username || 'Eustress member', 256),
    url,
    ...(shown.bio ? { description: clip(shown.bio, 300) } : {}),
    color: BRAND_COLOR,
    fields: [
      { name: 'Username', value: clip(username || 'Unknown', 256), inline: true },
      { name: 'Listed Spaces', value: String(listed), inline: true },
    ],
    ...(avatar ? { thumbnail: { url: avatar } } : {}),
  };
  return message({ embeds: [embed], buttons: [link('View profile', url)] });
}

export const NOT_LINKED_ME = 'Your Discord account is not linked to Eustress yet. Link it in Settings to see your balances here.';

async function meCommand(env, scope, origin) {
  const account = scope.account;
  if (!account) return message({ content: NOT_LINKED_ME, buttons: [link('Open Settings', `${origin}/settings`)], ephemeral: true });
  let bliss = 'Unavailable';
  try {
    await ledgerBalanceMinor(env, account.id);
    bliss = formatBliss((await ledgerDeriveMinor(env, account.id)).balance_minor);
  } catch { /* shown as unavailable */ }
  let tickets = 'Unavailable';
  try { tickets = String(Math.trunc(Number(await walletBalance(env, account.id)) || 0)); } catch { /* shown as unavailable */ }
  let unread = 'Unavailable';
  try {
    const list = await env.SOCIAL.list({ prefix: notifPrefix(account.id), limit: 1000 });
    let n = 0;
    for (const k of list.keys.slice(0, 200)) {
      const raw = await env.SOCIAL.get(k.name);
      try { if (raw && !JSON.parse(raw).read) n += 1; } catch { /* skipped */ }
    }
    unread = String(n);
  } catch { /* shown as unavailable */ }
  const embed = {
    title: clip(account.user.username || 'Your account', 256),
    color: BRAND_COLOR,
    fields: [
      { name: 'Bliss', value: bliss, inline: true },
      { name: 'Tickets', value: tickets, inline: true },
      { name: 'Unread notifications', value: unread, inline: true },
    ],
  };
  return message({ embeds: [embed], buttons: [link('Open Eustress', `${origin}/dashboard`)], ephemeral: true });
}

// ── /follow ───────────────────────────────────────────────────────────────────

const followKey = (kind, target, guild, channel) => `discordfollow:${kind}:${target}:${guild}:${channel}`;
const guildIndexKey = (guild, channel, kind, target) => `discordfollowg:${guild}:${channel}:${kind}:${target}`;

/// Why /follow cannot run here, as an immediate ephemeral reply, or null. Checked on the server: the app installed to
/// the guild, a guild channel, and Manage Server (or Administrator) in `member.permissions`.
export function followRefusal(interaction) {
  const say = (content) => ({ type: 4, data: message({ content, ephemeral: true }) });
  if (interaction?.context !== 0 || !SNOWFLAKE.test(String(interaction?.guild_id || '')) || !SNOWFLAKE.test(String(interaction?.channel?.id || interaction?.channel_id || ''))) {
    return say('Use /follow in a server channel.');
  }
  const owners = interaction.authorizing_integration_owners;
  if (!owners || typeof owners !== 'object' || !Object.hasOwn(owners, '0')) {
    return say('A server admin must add Eustress to this server with "Add to server" before this channel can follow anything.');
  }
  let perms = 0n;
  try { perms = BigInt(String(interaction.member?.permissions ?? '0')); } catch { perms = 0n; }
  if ((perms & MANAGE_GUILD) === 0n && (perms & ADMINISTRATOR) === 0n) return say('You need the Manage Server permission to change what this server follows.');
  return null;
}

const channelOf = (interaction) => String(interaction?.channel?.id || interaction?.channel_id || '');

async function guildFollows(env, guild, channel = null) {
  const prefix = channel ? `discordfollowg:${guild}:${channel}:` : `discordfollowg:${guild}:`;
  const list = await env.SOCIAL.list({ prefix, limit: 1000 });
  const out = [];
  for (const k of list.keys) {
    const [, , ch, kind, target] = k.name.split(':');
    const raw = await env.SOCIAL.get(followKey(kind, target, guild, ch));
    if (!raw) continue;
    try { out.push({ ...JSON.parse(raw), key: followKey(kind, target, guild, ch), index: k.name }); } catch { /* skipped */ }
  }
  return out;
}

async function saveFollow(env, interaction, kind, target, name) {
  const guild = String(interaction.guild_id);
  const channel = channelOf(interaction);
  const existing = await guildFollows(env, guild);
  if (!existing.some((f) => f.kind === kind && f.target === target && f.channel_id === channel) && existing.length >= FOLLOWS_PER_GUILD) {
    return `This server already has ${FOLLOWS_PER_GUILD} follows. Remove one first.`;
  }
  const record = {
    kind, target, name: clip(name, 100), guild_id: guild, channel_id: channel,
    age_restricted: interaction.channel?.nsfw === true,
    created_by: String(discordUserOf(interaction)?.id || ''),
    created_at: new Date().toISOString(),
  };
  await env.SOCIAL.put(followKey(kind, target, guild, channel), JSON.stringify(record));
  await env.SOCIAL.put(guildIndexKey(guild, channel, kind, target), '1');
  return null;
}

async function resolveCreator(env, username) {
  const name = String(username || '').trim();
  if (!name || name.length > 64) return null;
  for (const candidate of new Set([name, name.toLowerCase()])) {
    const id = await env.USERS.get(`username:${candidate}`);
    if (!id || !TARGET_ID.test(id)) continue;
    const raw = await env.USERS.get(`user:${id}`);
    if (!raw) continue;
    try {
      const user = JSON.parse(raw);
      if (!user.banned) return { id, username: user.username || candidate };
    } catch { /* skipped */ }
  }
  return null;
}

async function followCommand(env, deps, scope, interaction, sub, values) {
  const guild = String(interaction.guild_id);
  const channel = channelOf(interaction);
  const say = (content) => message({ content, ephemeral: true });
  if (sub === 'space') {
    const cap = lowerCap(scope.cap, channelPostCap(interaction.channel?.nsfw === true));
    const found = matching(await listedViews(deps, cap), values.space)[0];
    if (!found || !TARGET_ID.test(found.id)) return say('That Space is not listed, or its rating is above what this channel can show.');
    const refused = await saveFollow(env, interaction, 'space', found.id, found.name || 'Untitled Space');
    return say(refused || `This channel now follows ${clip(found.name, 100)}. Posts come when it is updated or goes live.`);
  }
  if (sub === 'creator') {
    const creator = await resolveCreator(env, values.username);
    if (!creator) return say('No Eustress account has that username.');
    const refused = await saveFollow(env, interaction, 'creator', creator.id, creator.username);
    return say(refused || `This channel now follows ${clip(creator.username, 64)}. Posts come when they list, update or host a Space.`);
  }
  if (sub === 'list') {
    const follows = await guildFollows(env, guild);
    if (!follows.length) return say('This server does not follow anything yet.');
    const lines = follows.map((f) => `<#${f.channel_id}> ${f.kind}: ${f.name}${f.age_restricted ? ' (age-restricted channel)' : ''}${f.disabled_at ? ' (paused: Eustress cannot post there)' : ''}`);
    return say(lines.join('\n'));
  }
  if (sub === 'remove') {
    const wanted = String(values.name || '').trim();
    const follows = await guildFollows(env, guild, channel);
    const hit = follows.find((f) => `${f.kind}:${f.target}` === wanted) || follows.find((f) => norm(f.name) === norm(wanted));
    if (!hit) return say('This channel has no follow by that name.');
    await env.SOCIAL.delete(hit.key);
    await env.SOCIAL.delete(hit.index);
    return say(`This channel no longer follows ${clip(hit.name, 100)}.`);
  }
  return say('Choose space, creator, list or remove.');
}

/// The finished message for one command. Exported for the tests; the endpoint sends it as the deferred reply's edit.
export async function commandMessage(interaction, env, deps = {}) {
  const origin = siteOrigin(env);
  const scope = await interactionScope(env, interaction);
  const { sub, values } = optionsOf(interaction?.data);
  switch (interaction?.data?.name) {
    case 'space': return spaceCommand(env, deps, scope, values, origin);
    case 'live': return liveCommand(env, deps, scope, origin);
    case 'profile': return profileCommand(env, deps, scope, values, origin);
    case 'me': return meCommand(env, scope, origin);
    case 'follow': {
      const refusal = followRefusal(interaction);
      if (refusal) return refusal.data;
      return followCommand(env, deps, scope, interaction, sub, values);
    }
    default: return message({ content: 'That command is not known.', ephemeral: true });
  }
}

/// Autocomplete choices (at most 25) for one interaction.
export async function autocompleteChoices(interaction, env, deps = {}, origin = 'https://api.eustress.dev') {
  const name = interaction?.data?.name;
  const { sub, values, focused } = optionsOf(interaction?.data);
  const typed = String(values[focused] ?? '');
  const scope = await interactionScope(env, interaction);
  const spaceChoices = async (cap) => matching(await cachedListedViews(deps, cap, origin), typed).slice(0, 25).map((v) => ({
    name: clip(`${v.name || 'Untitled Space'}${v.author_name ? ` by ${v.author_name}` : ''}`, 100),
    value: v.id,
  }));
  if (name === 'space' && focused === 'query') return spaceChoices(scope.cap);
  if (name === 'follow' && !followRefusal(interaction)) {
    if (sub === 'space' && focused === 'space') return spaceChoices(lowerCap(scope.cap, channelPostCap(interaction.channel?.nsfw === true)));
    if (sub === 'remove' && focused === 'name') {
      const follows = await guildFollows(env, String(interaction.guild_id), channelOf(interaction));
      return follows.filter((f) => !typed || norm(f.name).includes(norm(typed))).slice(0, 25)
        .map((f) => ({ name: clip(`${f.kind}: ${f.name}`, 100), value: `${f.kind}:${f.target}` }));
    }
  }
  return [];
}

// -----------------------------------------------------------------------------
// The endpoint
// -----------------------------------------------------------------------------

/// Edit the deferred reply. Never throws.
async function editOriginal(env, interaction, body, fetchImpl) {
  const appId = String(interaction?.application_id || env?.DISCORD_CLIENT_ID || '');
  const token = String(interaction?.token || '');
  if (!SNOWFLAKE.test(appId) || !INTERACTION_TOKEN.test(token)) return false;
  const { flags: _flags, ...edit } = body;
  try {
    const res = await fetchImpl(`${DISCORD_API}/webhooks/${appId}/${token}/messages/@original`, {
      method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(edit),
    });
    if (!res.ok) console.error(`discord reply edit answered ${res.status}`);
    return !!res.ok;
  } catch (e) {
    console.error('discord reply edit unreachable:', e?.message || e);
    return false;
  }
}

/// POST /api/discord/interactions. `deps`: listViews(cap), liveSessions(cap) (index.js), and for tests fetch, now, cache.
export async function handleDiscordInteraction(request, env, ctx, deps = {}) {
  const answer = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' } });
  if (request.method !== 'POST') return answer({ error: 'Method not allowed' }, 405);
  if (!HEX64.test(env?.DISCORD_PUBLIC_KEY || '')) return answer({ error: 'Discord is not set up on this server.' }, 503);
  const signature = request.headers.get('X-Signature-Ed25519');
  const timestamp = request.headers.get('X-Signature-Timestamp');
  if (!signature || !timestamp) return answer({ error: 'invalid request signature' }, 401);
  if (Number(request.headers.get('Content-Length') || 0) > MAX_BODY_BYTES) return answer({ error: 'too large' }, 413);
  const body = await request.text();
  if (body.length > MAX_BODY_BYTES) return answer({ error: 'too large' }, 413);
  const now = deps.now ? deps.now() : Date.now();
  if (!(await verifyDiscordSignature(env.DISCORD_PUBLIC_KEY, signature, timestamp, body, now))) return answer({ error: 'invalid request signature' }, 401);

  let interaction;
  try { interaction = JSON.parse(body); } catch { return answer({ error: 'bad request' }, 400); }
  if (!interaction || typeof interaction !== 'object') return answer({ error: 'bad request' }, 400);
  const fetchImpl = deps.fetch || fetch;

  if (interaction.type === 1) return answer({ type: 1 });

  if (interaction.type === 4) {
    let choices = [];
    try { choices = await autocompleteChoices(interaction, env, deps, new URL(request.url).origin); }
    catch (e) { console.error('discord autocomplete failed:', e?.message || e); }
    return answer({ type: 8, data: { choices } });
  }

  if (interaction.type === 2) {
    const name = interaction.data?.name;
    if (!COMMAND_NAMES.has(name)) return answer({ type: 4, data: message({ content: 'That command is not known.', ephemeral: true }) });
    if (name === 'follow') {
      const refusal = followRefusal(interaction);
      if (refusal) return answer(refusal);
    }
    const ephemeral = name === 'me' || name === 'follow';
    const work = (async () => {
      let reply;
      try { reply = await commandMessage(interaction, env, deps); }
      catch (e) {
        console.error(`discord /${name} failed:`, e?.message || e);
        reply = message({ content: 'Something went wrong. Try again in a minute.', ephemeral });
      }
      await editOriginal(env, interaction, reply, fetchImpl);
    })();
    if (ctx && typeof ctx.waitUntil === 'function') ctx.waitUntil(work); else await work;
    return answer({ type: 5, ...(ephemeral ? { data: { flags: EPHEMERAL } } : {}) });
  }

  return answer({ error: 'unsupported interaction' }, 400);
}

// -----------------------------------------------------------------------------
// Registration (five-minute tick)
// -----------------------------------------------------------------------------

/// PUT the commands and the Linked Roles metadata when either differs from what was last registered (a SHA-256 kept in
/// SOCIAL). The hash is stored only after Discord answers 2xx. Without DISCORD_BOT_TOKEN or DISCORD_CLIENT_ID, one log
/// line and nothing else. Returns { commands, metadata }: 'skipped', 'unchanged', 'registered' or 'failed'.
export async function registerDiscordDefinitions(env, fetchImpl = fetch) {
  const appId = String(env?.DISCORD_CLIENT_ID || '');
  if (!env?.DISCORD_BOT_TOKEN || !SNOWFLAKE.test(appId) || !env?.SOCIAL) {
    console.log('discord registration skipped: DISCORD_BOT_TOKEN or DISCORD_CLIENT_ID is not set');
    return { commands: 'skipped', metadata: 'skipped' };
  }
  const out = {};
  for (const [what, url, defs] of [
    ['commands', `${DISCORD_API}/applications/${appId}/commands`, COMMANDS],
    ['metadata', `${DISCORD_API}/applications/${appId}/role-connections/metadata`, ROLE_METADATA],
  ]) {
    const text = JSON.stringify(defs);
    const hash = await sha256Hex(`${appId}\n${text}`);
    const kvKey = `discordreg:${what}`;
    if ((await env.SOCIAL.get(kvKey)) === hash) { out[what] = 'unchanged'; continue; }
    try {
      const res = await fetchImpl(url, { method: 'PUT', headers: { Authorization: `Bot ${env.DISCORD_BOT_TOKEN}`, 'Content-Type': 'application/json' }, body: text });
      if (res.ok) { await env.SOCIAL.put(kvKey, hash); out[what] = 'registered'; }
      else { console.error(`discord ${what} registration answered ${res.status}`); out[what] = 'failed'; }
    } catch (e) {
      console.error(`discord ${what} registration unreachable:`, e?.message || e);
      out[what] = 'failed';
    }
  }
  return out;
}

// -----------------------------------------------------------------------------
// Follow posts and Linked Roles updates (five-minute tick)
// -----------------------------------------------------------------------------

const POST_HEADLINE = { new: 'New on Eustress', updated: 'Updated on Eustress', live: 'Live now on Eustress' };

async function followsFor(env, kind, target) {
  if (!TARGET_ID.test(String(target || ''))) return [];
  const list = await env.SOCIAL.list({ prefix: `discordfollow:${kind}:${target}:`, limit: 1000 });
  const out = [];
  for (const k of list.keys) {
    const raw = await env.SOCIAL.get(k.name);
    if (!raw) continue;
    try { out.push({ key: k.name, record: JSON.parse(raw) }); } catch { /* skipped */ }
  }
  return out;
}

/// The sweep: see the header. Never throws. `deps`: listViews, liveSessions (index.js). `opts`: now, fetch.
export async function sweepDiscordFollows(env, deps, opts = {}) {
  const out = { posted: 0, disabled: 0, budget_spent: false, roles: {}, cursor: null };
  if (!env?.SOCIAL || !deps?.listViews) return out;
  const fetchImpl = opts.fetch || fetch;
  const startMs = opts.now ?? Date.now();
  const origin = siteOrigin(env);

  // Every listed listing (for the creators' counts); posts reach mature_17 at most, and a known rating only.
  const all = await deps.listViews('adult_21');
  const views = Array.isArray(all) ? all.filter((v) => v && typeof v.id === 'string') : [];
  const postable = views.filter((v) => withinCap(v.rating, POST_CAP_RESTRICTED));

  // Events: listed or updated since the cursor, then live sessions.
  const events = [];
  const cursor = await env.SOCIAL.get('discordsweep:cursor');
  const stamp = (v) => String(v.updated_at || v.published_at || '');
  if (cursor) {
    for (const v of postable.filter((x) => stamp(x) > cursor).sort((a, b) => (stamp(a) < stamp(b) ? -1 : 1))) {
      events.push({ id: `upd:${v.id}:${stamp(v)}`, view: v, kind: null, stamp: stamp(v) });
    }
  }
  try {
    for (const s of await liveWithViews(deps, POST_CAP_RESTRICTED, postable)) {
      events.push({ id: `live:${s.id}:${s.since}`, view: s.view, kind: 'live', players: s.players });
    }
  } catch (e) {
    console.error('discord sweep: live sessions unavailable:', e?.message || e);
  }

  let lastDone = cursor;
  const canPost = !!env.DISCORD_BOT_TOKEN;
  outer: for (const event of events) {
    const v = event.view;
    if (!event.kind) {
      const marker = await env.SOCIAL.get(`discordlisted:${v.id}`);
      event.kind = !marker && (v.version || 1) === 1 ? 'new' : 'updated';
    }
    const follows = canPost ? [...await followsFor(env, 'space', v.id), ...await followsFor(env, 'creator', v.author_id)] : [];
    for (const { key, record } of follows) {
      if (record.disabled_at || !SNOWFLAKE.test(String(record.channel_id || ''))) continue;
      if (!withinCap(v.rating, channelPostCap(record.age_restricted === true))) continue;
      // Once per channel and event: a channel that follows both the Space and its creator gets one post, not two.
      const posted = `discordposted:${await sha256Hex(`${record.guild_id}:${record.channel_id}|${event.id}`)}`;
      if (await env.SOCIAL.get(posted)) continue;
      if (out.posted >= POSTS_PER_RUN) { out.budget_spent = true; break outer; }
      const card = spaceCard(v, origin, { players: event.players || 0 });
      let res;
      try {
        res = await fetchImpl(`${DISCORD_API}/channels/${record.channel_id}/messages`, {
          method: 'POST',
          headers: { Authorization: `Bot ${env.DISCORD_BOT_TOKEN}`, 'Content-Type': 'application/json' },
          body: JSON.stringify(message({ content: POST_HEADLINE[event.kind], embeds: [card.embed], buttons: card.buttons })),
        });
      } catch (e) {
        console.error('discord follow post unreachable:', e?.message || e);
        continue;
      }
      if (res.ok) {
        out.posted += 1;
        await env.SOCIAL.put(posted, '1', { expirationTtl: POSTED_TTL_S });
      } else if (res.status === 403 || res.status === 404) {
        out.disabled += 1;
        await env.SOCIAL.put(key, JSON.stringify({ ...record, disabled_at: new Date(startMs).toISOString(), disabled_status: res.status }));
      } else if (res.status === 429) {
        out.budget_spent = true;
        break outer;
      } else {
        console.error(`discord follow post answered ${res.status}`);
      }
    }
    if (event.kind === 'new' || event.kind === 'updated') {
      await env.SOCIAL.put(`discordlisted:${v.id}`, '1');
      lastDone = event.stamp;
    }
  }

  // The cursor: the run's start less the overlap, unless the budget ran out (then the last update fully handled).
  const overlap = new Date(startMs - CURSOR_OVERLAP_MS).toISOString();
  let next = out.budget_spent ? lastDone || cursor || overlap : overlap;
  if (cursor && next < cursor) next = cursor;
  await env.SOCIAL.put('discordsweep:cursor', next);
  out.cursor = next;

  // Linked Roles: creators whose listed count changed since the last push.
  try {
    const counts = {};
    for (const v of views) if (typeof v.author_id === 'string') counts[v.author_id] = (counts[v.author_id] || 0) + 1;
    let previous = {};
    try { previous = JSON.parse((await env.SOCIAL.get('discordsweep:counts')) || '{}') || {}; } catch { previous = {}; }
    const kept = { ...previous };
    let pushes = 0;
    for (const id of new Set([...Object.keys(counts), ...Object.keys(previous)])) {
      const now = counts[id] || 0;
      if ((previous[id] || 0) === now) continue;
      if (env.USERS && (await env.USERS.get(`discordtoken:${id}`))) {
        if (pushes >= ROLE_PUSHES_PER_RUN) continue;
        pushes += 1;
        const result = await repushRoleConnection(env, id, now, fetchImpl).catch(() => 'failed');
        out.roles[result] = (out.roles[result] || 0) + 1;
        if (result === 'failed') continue;
      }
      if (now) kept[id] = now; else delete kept[id];
    }
    await env.SOCIAL.put('discordsweep:counts', JSON.stringify(kept));
  } catch (e) {
    console.error('discord sweep: linked roles failed:', e?.message || e);
  }
  return out;
}

/// The five-minute tick's Discord work: registration, then the sweep. Never throws.
export async function discordTick(env, deps, opts = {}) {
  const result = {};
  try { result.registration = await registerDiscordDefinitions(env, opts.fetch || fetch); }
  catch (e) { console.error('discord registration failed:', e?.message || e); }
  try { result.sweep = await sweepDiscordFollows(env, deps, opts); }
  catch (e) { console.error('discord follow sweep failed:', e?.message || e); }
  return result;
}
