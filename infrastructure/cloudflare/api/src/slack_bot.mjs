// =============================================================================
// The Eustress Slack app: slash command, events, install, follow posts
// =============================================================================
//
// The Slack twin of the Discord app (src/discord_interactions.mjs). Every route
// here is public in src/route_policy.mjs: none takes a session. What gates each
// one is named beside it.
//
//   GET  /api/slack/install          "Add to Slack". Redirects to Slack's OAuth v2
//                                    authorize page, bot scopes `commands,chat:write,users:read`
//                                    only, with a random one-time state kept hashed in
//                                    CHALLENGES for ten minutes. No account binding:
//                                    anyone may add the app to a workspace they manage.
//                                    Limited per address.
//   GET  /api/slack/oauth_redirect   Slack sends the browser back here. The state must
//                                    be one this Worker issued and is spent before the
//                                    code is exchanged (oauth.v2.access). The bot token
//                                    is kept SEALED per installation; then the browser
//                                    goes to {site}/slack?installed=1, or
//                                    ?install_error=<code>.
//   POST /api/slack/commands         `/eustress <subcommand>`, or `/eustress-<subcommand>`
//                                    (space, live, profile, me, follow, help), which reads
//                                    the same as `/eustress <subcommand>`.
//   POST /api/slack/events           Events API: url_verification, app_uninstalled,
//                                    tokens_revoked, app_home_opened.
//   POST /api/slack/interactivity    Link buttons still send an interaction: acknowledged.
//
// The three POST routes read the raw body once (64 KB at most) and verify Slack's
// request signature before anything is parsed: HMAC-SHA256 under
// SLACK_SIGNING_SECRET over `v0:{X-Slack-Request-Timestamp}:{body}`, compared in
// constant time (WebCrypto verify), the timestamp within five minutes of the
// Worker's clock. A missing header, a stale timestamp or a mismatch is 401; no
// signing secret is 503. Then `api_app_id` must be SLACK_APP_ID ([vars]).
//
// Commands (`/eustress help` lists them):
//
//   space <query>      the best match as a card (creator, rating, live players, an
//                      image when the listing has a public one, Open and Play link
//                      buttons), and up to four more matches as a list. In channel.
//   live               up to ten listed Spaces live now, each with a Play button. In channel.
//   profile @someone   the public profile of a Slack member whose Connect Slack link
//                      names this workspace and member; otherwise "has not linked".
//   me                 only the person who asks sees it: Bliss, Tickets, unread
//                      notifications; or a link to Settings when not linked.
//   follow ...         `space <query>`, `creator <username>`, `list`, `remove <name>`:
//                      a channel follows a Space or a creator. The app must be installed
//                      to the workspace, and only the person who installed it or a
//                      workspace admin or owner (users.info) may change follows.
//   help               the list (also any unknown subcommand).
//
// Ratings. Every reply starts at `ages_10` (SIGNED_OUT_CAP). A higher cap applies only
// to a Slack member who is linked to an Eustress account whose verified age is 18 or
// over (the Discord app's own test), AND who asks in their own DM with the app: the
// command's channel_name is `directmessage`, its channel is a D... id, and that id is
// the one Slack named as this member's app DM in a signed app_home_opened event. Any
// doubt (a DM between two people, a group DM, a channel, no app_home_opened seen yet)
// is `ages_10`. The higher cap is that account's own viewer cap
// (src/adult_content.mjs), never more. A listing whose rating is missing or unknown is
// never shown, and an adult listing's image is never put in Slack.
//
// Follow posts (sweepSlackFollows, five-minute tick): listings listed or updated since
// the cursor, and listed listings that went live, are posted with chat.postMessage to
// each channel that follows the listing or its creator, once per channel and event,
// at `ages_10` in every channel. Slack answering channel_not_found, not_in_channel or
// is_archived pauses the follow (`follow list` says so, with the hint /invite @Eustress Engine);
// invalid_auth, account_inactive or token_revoked removes the installation.
//
// Storage:
//   USERS   slackinstall:{teamId | enterpriseId}   the installation; its bot token sealed
//                                                  (AES-GCM, HKDF over SLACK_CLIENT_SECRET,
//                                                  info eustress-slack-bot-token-v1, the
//                                                  key name as associated data)
//   CHALLENGES slackinstallstate:{sha256(state)}   an install in flight (ten minutes)
//   SOCIAL  slackfollow:{kind}:{target}:{install}:{team}:{channel}   the follow record
//           slackfollowg:{install}:{team}:{channel}:{kind}:{target}  the installation's index
//           slackposted:{sha256(team:channel|event)}                 a post made (30 days)
//           slacklisted:{simId}                                      posted as new once
//           slacksweep:cursor                                        the sweep's place
//           slackappdm:{team}:{user}                                 the member's DM with the app
//
// Config: SLACK_APP_ID ([vars]), SLACK_SIGNING_SECRET, SLACK_CLIENT_ID and
// SLACK_CLIENT_SECRET (secrets; see wrangler.toml).
// =============================================================================

import { siteOrigin } from './cors.mjs';
import { BANDS as RATINGS } from './rating_questionnaire.mjs';
import { notifPrefix } from './moderation.mjs';
import { SIGNED_OUT_CAP, viewerFor, viewerCap } from './adult_content.mjs';
import { ledgerBalanceMinor, ledgerDeriveMinor, formatBliss } from './bliss.mjs';
import { walletBalance } from './commerce.mjs';
import { publicProfileOf } from './account_profile.mjs';
import { accountState } from './account_auth.mjs';
import { currentSlackLink } from './slack_link.mjs';
import {
  withinCap, isVerifiedAdult, listedViews, liveWithViews, matching, resolveCreator, safeImage, RATING_WORDS,
} from './discord_interactions.mjs';

export const SLACK_API = 'https://slack.com/api';
export const SLACK_OAUTH_AUTHORIZE = 'https://slack.com/oauth/v2/authorize';
export const SLACK_OAUTH_ACCESS = `${SLACK_API}/oauth.v2.access`;
/// What an install asks for: run the slash command, and post messages. Nothing that reads.
export const SLACK_BOT_SCOPE = 'commands,chat:write,users:read';
export const INSTALL_PATH = '/api/slack/install';
export const OAUTH_REDIRECT_PATH = '/api/slack/oauth_redirect';
const COMMANDS_PATH = '/api/slack/commands';
const EVENTS_PATH = '/api/slack/events';
const INTERACTIVITY_PATH = '/api/slack/interactivity';

/// A request whose timestamp is further than this from the Worker's clock is refused.
export const SIGNATURE_WINDOW_S = 300;
/// Slack's slash commands, events and interaction payloads are a few kilobytes.
export const MAX_BODY_BYTES = 64 * 1024;
export const INSTALL_STATE_TTL_S = 600;
export const FOLLOWS_PER_TEAM = 25;
export const POSTS_PER_RUN = 50;
/// Every follow post, in every channel, is at most this rating.
export const POST_CAP = SIGNED_OUT_CAP;
const POSTED_TTL_S = 30 * 86400;
const CURSOR_OVERLAP_MS = 120_000;
const CURSOR_KEY = 'slacksweep:cursor';

const TOKEN_KEY_INFO = 'eustress-slack-bot-token-v1';

const TEAM_RE = /^T[A-Z0-9]{6,20}$/;
const ENTERPRISE_RE = /^E[A-Z0-9]{6,20}$/;
/// An installation is kept per team (T...) or, for an org-wide install, per enterprise (E...).
export const INSTALL_ID_RE = /^[TE][A-Z0-9]{6,20}$/;
const USER_RE = /^[UW][A-Z0-9]{6,20}$/;
const BOT_USER_RE = /^[UBW][A-Z0-9]{6,20}$/;
const CHANNEL_RE = /^[A-Z][A-Z0-9]{6,20}$/;
const DM_RE = /^D[A-Z0-9]{6,20}$/;
const STATE_RE = /^[0-9a-f]{64}$/;
const CODE_RE = /^[A-Za-z0-9._~-]{8,512}$/;
const BOT_TOKEN_RE = /^xoxb-[A-Za-z0-9-]{10,500}$/;
const TARGET_ID = /^[a-f0-9-]{1,64}$/;
/// Slack's OAuth errors that mean the code itself is no good.
const CODE_ERRORS = new Set(['invalid_code', 'code_already_used', 'code_expired', 'invalid_grant', 'bad_redirect_uri']);
/// A bot token Slack no longer honours: the installation is gone.
export const AUTH_ERRORS = new Set(['invalid_auth', 'account_inactive', 'token_revoked']);
/// A channel the app cannot post to: the follow is paused.
export const CHANNEL_ERRORS = new Set(['channel_not_found', 'not_in_channel', 'is_archived']);

const enc = new TextEncoder();

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', enc.encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}
const randomHex = (n) => [...crypto.getRandomValues(new Uint8Array(n))].map((b) => b.toString(16).padStart(2, '0')).join('');
const hexBytes = (hex) => Uint8Array.from(hex.match(/../g).map((h) => parseInt(h, 16)));
const ratingIdx = (r) => RATINGS.indexOf(r);

export function slackBotConfigured(env) {
  return !!(env?.SLACK_CLIENT_ID && env?.SLACK_CLIENT_SECRET && env?.SLACK_APP_ID);
}

// -----------------------------------------------------------------------------
// Request signing
// -----------------------------------------------------------------------------

/// Whether `signature` (`v0=<hex>`) is HMAC-SHA256 under `secret` of `v0:{timestamp}:{body}`, and `timestamp` (Unix
/// seconds) is within SIGNATURE_WINDOW_S of `nowMs`. `body` is the raw bytes (or a string). The comparison is WebCrypto's
/// HMAC verify, which is constant time. Any malformed part, or a failure inside WebCrypto, is false.
export async function verifySlackSignature(secret, signature, timestamp, body, nowMs = Date.now()) {
  try {
    if (typeof secret !== 'string' || !secret) return false;
    if (!/^v0=[0-9a-f]{64}$/.test(signature || '') || !/^[0-9]{1,12}$/.test(timestamp || '')) return false;
    if (Math.abs(nowMs / 1000 - Number(timestamp)) > SIGNATURE_WINDOW_S) return false;
    const bytes = typeof body === 'string' ? enc.encode(body) : new Uint8Array(body);
    const head = enc.encode(`v0:${timestamp}:`);
    const signed = new Uint8Array(head.length + bytes.length);
    signed.set(head, 0);
    signed.set(bytes, head.length);
    const key = await crypto.subtle.importKey('raw', enc.encode(secret), { name: 'HMAC', hash: 'SHA-256' }, false, ['verify']);
    return await crypto.subtle.verify('HMAC', key, hexBytes(signature.slice(3)), signed);
  } catch {
    return false;
  }
}

const answer = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' } });
const ack = () => new Response('', { status: 200, headers: { 'Cache-Control': 'no-store' } });

/// The raw body of a signed Slack request as text, or `{ response }` to answer at once. Nothing is parsed before the
/// signature is checked.
async function verifiedBody(request, env, nowMs) {
  if (request.method !== 'POST') return { response: answer({ error: 'Method not allowed' }, 405) };
  if (!env?.SLACK_SIGNING_SECRET || !env?.SLACK_APP_ID) return { response: answer({ error: 'Slack is not set up on this server.' }, 503) };
  const signature = request.headers.get('X-Slack-Signature');
  const timestamp = request.headers.get('X-Slack-Request-Timestamp');
  if (!signature || !timestamp) return { response: answer({ error: 'invalid request signature' }, 401) };
  if (Number(request.headers.get('Content-Length') || 0) > MAX_BODY_BYTES) return { response: answer({ error: 'too large' }, 413) };
  const bytes = new Uint8Array(await request.arrayBuffer());
  if (bytes.byteLength > MAX_BODY_BYTES) return { response: answer({ error: 'too large' }, 413) };
  if (!(await verifySlackSignature(env.SLACK_SIGNING_SECRET, signature, timestamp, bytes, nowMs))) {
    return { response: answer({ error: 'invalid request signature' }, 401) };
  }
  return { text: new TextDecoder().decode(bytes) };
}

// -----------------------------------------------------------------------------
// The installation: the bot token, sealed
// -----------------------------------------------------------------------------

const installKey = (installId) => `slackinstall:${installId}`;
const b64 = (bytes) => btoa(String.fromCharCode(...new Uint8Array(bytes)));
const unb64 = (text) => Uint8Array.from(atob(text), (c) => c.charCodeAt(0));

/// AES-GCM key: HKDF-SHA256 over SLACK_CLIENT_SECRET, info `eustress-slack-bot-token-v1`. Null without the secret.
async function sealKey(env) {
  const secret = env?.SLACK_CLIENT_SECRET;
  if (typeof secret !== 'string' || !secret) return null;
  const base = await crypto.subtle.importKey('raw', enc.encode(secret), 'HKDF', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(0), info: enc.encode(TOKEN_KEY_INFO) },
    base, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt'],
  );
}

/// `{ v: 1, iv, ct }` for a bot token, bound to its installation (the key name is the associated data). A fresh IV each time.
export async function sealSlackBotToken(env, installId, token) {
  const key = await sealKey(env);
  if (!key) throw new Error('SLACK_CLIENT_SECRET is not set');
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: enc.encode(installKey(installId)) }, key, enc.encode(token));
  return { v: 1, iv: b64(iv), ct: b64(ct) };
}

/// The token a sealed record holds, or null when it does not open (another key, another installation, tampered).
export async function openSlackBotToken(env, installId, sealed) {
  try {
    const key = await sealKey(env);
    if (!key || sealed?.v !== 1) return null;
    const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: unb64(sealed.iv), additionalData: enc.encode(installKey(installId)) }, key, unb64(sealed.ct));
    return new TextDecoder().decode(plain);
  } catch {
    return null;
  }
}

/// `{ id, record, token }` for one installation id, or null (none, unreadable, or a token that does not open).
async function readInstallation(env, installId) {
  if (!env?.USERS || !INSTALL_ID_RE.test(String(installId || ''))) return null;
  try {
    const raw = await env.USERS.get(installKey(installId));
    if (!raw) return null;
    const record = JSON.parse(raw);
    const token = await openSlackBotToken(env, installId, record?.token);
    return token ? { id: installId, record, token } : null;
  } catch {
    return null;
  }
}

/// The installation that serves a workspace: its own, or its organisation's org-wide one.
export async function installationFor(env, teamId, enterpriseId = null) {
  if (TEAM_RE.test(String(teamId || ''))) {
    const own = await readInstallation(env, teamId);
    if (own) return own;
  }
  if (ENTERPRISE_RE.test(String(enterpriseId || ''))) {
    const org = await readInstallation(env, enterpriseId);
    if (org && org.record?.is_enterprise_install === true) return org;
  }
  return null;
}

/// The app uninstalled, its token revoked, or Slack refusing the token: the installation, every follow it carries, and
/// (for a workspace install) the members' app-DM records go.
export async function removeInstallation(env, installId) {
  if (!INSTALL_ID_RE.test(String(installId || ''))) return;
  if (env?.USERS) await env.USERS.delete(installKey(installId));
  if (!env?.SOCIAL) return;
  const index = await env.SOCIAL.list({ prefix: `slackfollowg:${installId}:`, limit: 1000 });
  for (const k of index.keys) {
    const [, , team, channel, kind, target] = k.name.split(':');
    await env.SOCIAL.delete(followKey(kind, target, installId, team, channel));
    await env.SOCIAL.delete(k.name);
  }
  if (TEAM_RE.test(installId)) {
    const dms = await env.SOCIAL.list({ prefix: `slackappdm:${installId}:`, limit: 1000 });
    for (const k of dms.keys) await env.SOCIAL.delete(k.name);
  }
}

/// One Slack Web API call with the bot token. `{ ok, error, status, ...reply }`; never throws, never logs the token.
async function slackApi(fetchImpl, method, token, body, { form = false } = {}) {
  try {
    const res = await fetchImpl(`${SLACK_API}/${method}`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': form ? 'application/x-www-form-urlencoded' : 'application/json; charset=utf-8' },
      body: form ? new URLSearchParams(body).toString() : JSON.stringify(body),
    });
    if (res.status === 429) return { ok: false, error: 'ratelimited', status: 429 };
    const reply = await res.json().catch(() => ({}));
    const ok = res.ok && reply?.ok === true;
    return { ...reply, ok, error: ok ? null : (typeof reply?.error === 'string' ? reply.error : `http_${res.status}`), status: res.status };
  } catch (e) {
    console.error(`slack ${method} unreachable:`, e?.message || e);
    return { ok: false, error: 'unreachable', status: 0 };
  }
}

// -----------------------------------------------------------------------------
// Who asks, and the cap that follows
// -----------------------------------------------------------------------------

const linkClaimKey = (teamId, userId) => `slack:${teamId}:${userId}`;

/// The Eustress account a Slack member is linked to RIGHT NOW: `{ id, user }`, or null. The claim `slack:{team}:{user}`
/// must name an account whose current link (currentSlackLink, honouring the account object's `slack_unlinked_at`) is
/// this same team and member. A banned account, or an account object that cannot be read, is no link: fails closed.
export async function linkedAccountForSlack(env, teamId, userId) {
  if (!env?.USERS || !/^[A-Z0-9]{6,20}$/.test(String(teamId || '')) || !/^[A-Z0-9]{6,20}$/.test(String(userId || ''))) return null;
  try {
    const owner = await env.USERS.get(linkClaimKey(teamId, userId));
    if (!owner) return null;
    const raw = await env.USERS.get(`user:${owner}`);
    if (!raw) return null;
    const user = JSON.parse(raw);
    if (!user || user.banned) return null;
    let unlinkedAt = null;
    if (env.ACCOUNT_AUTH) {
      const held = await accountState(env, owner);
      if (!held?.ok) return null;
      unlinkedAt = held.slack_unlinked_at || null;
    }
    const link = await currentSlackLink(env, { ...user, id: owner }, unlinkedAt);
    return link && link.team_id === String(teamId) && link.user_id === String(userId) ? { id: owner, user: { ...user, id: owner } } : null;
  } catch {
    return null;
  }
}

const appDmKey = (teamId, userId) => `slackappdm:${teamId}:${userId}`;

/// Whether the command was used in the member's own DM with the app. See the header: every part must hold.
export async function isAppDm(env, cmd) {
  try {
    if (cmd?.channel_name !== 'directmessage' || !DM_RE.test(String(cmd?.channel_id || ''))) return false;
    if (!TEAM_RE.test(String(cmd.team_id || '')) || !USER_RE.test(String(cmd.user_id || '')) || !env?.SOCIAL) return false;
    return (await env.SOCIAL.get(appDmKey(cmd.team_id, cmd.user_id))) === cmd.channel_id;
  } catch {
    return false;
  }
}

/// `{ cap, account }` for one command: `ages_10` unless a linked, verified adult asks in their DM with the app, and then
/// that account's own viewer cap.
export async function commandScope(env, cmd) {
  const account = await linkedAccountForSlack(env, cmd?.team_id, cmd?.user_id);
  let cap = SIGNED_OUT_CAP;
  if (account && (await isAppDm(env, cmd)) && (await isVerifiedAdult(env, account))) {
    const own = viewerCap(await viewerFor(env, account.id));
    if (RATINGS.includes(own) && ratingIdx(own) > ratingIdx(cap)) cap = own;
  }
  return { cap, account };
}

// -----------------------------------------------------------------------------
// Messages (Block Kit)
// -----------------------------------------------------------------------------

/// Text a member or a creator wrote, made inert in mrkdwn: no mention, no link, no `<!channel>`.
export const esc = (text) => String(text ?? '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const clip = (text, max) => {
  const s = String(text ?? '').trim();
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
};
const spacePage = (origin, id) => `${origin}/simulation/${encodeURIComponent(id)}`;
const playPage = (origin, id) => `${origin}/play/${encodeURIComponent(id)}`;
const button = (text, url, actionId, primary = false) => ({ type: 'button', text: { type: 'plain_text', text: clip(text, 75), emoji: false }, url, action_id: actionId, ...(primary ? { style: 'primary' } : {}) });

/// A reply as Slack takes it: `ephemeral` or `in_channel`, no link previews.
function reply(text, { blocks, inChannel = false } = {}) {
  return {
    response_type: inChannel ? 'in_channel' : 'ephemeral',
    text: clip(text, 3000),
    ...(blocks ? { blocks: blocks.slice(0, 50) } : {}),
    unfurl_links: false,
    unfurl_media: false,
  };
}

/// One listing as Block Kit: a section with its facts (and its image when it may have one) and the Open and Play buttons.
export function spaceBlocks(view, origin, { players = 0, headline = null, n = 0 } = {}) {
  const name = view.name || 'Untitled Space';
  const lines = [`*<${spacePage(origin, view.id)}|${esc(clip(name, 150))}>*`];
  if (view.description) lines.push(esc(clip(view.description, 300)));
  const fields = [
    { type: 'mrkdwn', text: `*Creator*\n${esc(clip(view.author_name || 'Unknown', 100))}` },
    { type: 'mrkdwn', text: `*Rating*\n${RATING_WORDS[view.rating] || 'Not rated'}` },
  ];
  if (Number.isSafeInteger(players) && players > 0) fields.push({ type: 'mrkdwn', text: `*Live now*\n${players} playing` });
  const image = safeImage(view);
  const blocks = [];
  if (headline) blocks.push({ type: 'context', elements: [{ type: 'mrkdwn', text: esc(headline) }] });
  blocks.push({
    type: 'section',
    text: { type: 'mrkdwn', text: lines.join('\n') },
    fields,
    ...(image ? { accessory: { type: 'image', image_url: image, alt_text: clip(name, 100) } } : {}),
  });
  blocks.push({ type: 'actions', elements: [button('Open', spacePage(origin, view.id), `open_${n}`), button('Play', playPage(origin, view.id), `play_${n}`, true)] });
  return blocks;
}

export const HELP_TEXT = [
  '*Eustress* is a simulation and data platform. Commands:',
  '• `/eustress space <name>`: find a listed Space and share its card here',
  '• `/eustress live`: the Spaces with people in them right now',
  '• `/eustress profile @someone`: the Eustress profile of someone who linked their Slack account',
  '• `/eustress me`: your Bliss, Tickets and notifications (only you see it)',
  '• `/eustress follow space <name>`, `follow creator <username>`, `follow list`, `follow remove <name>`: post a Space\'s or a creator\'s updates in this channel (the person who added Eustress, or a workspace admin)',
  'Each also has its own command: `/eustress-space`, `/eustress-live`, `/eustress-profile`, `/eustress-me`, `/eustress-follow`.',
].join('\n');

function helpReply(origin) {
  return reply(`${HELP_TEXT}\nLink your Slack account in <${origin}/settings|Settings>.`);
}

export const NOT_LINKED_PROFILE = 'This person has not linked a Eustress account.';
export const NOT_LINKED_ME = 'Your Slack account is not linked to Eustress yet. Link it in Settings to see your balances here:';
export const NO_MATCH = 'No listed Space matches that.';
export const INVITE_HINT = 'If the app is not in this channel yet, invite it with /invite @Eustress Engine.';

// -----------------------------------------------------------------------------
// The commands
// -----------------------------------------------------------------------------

/// The text to read for one command. `/eustress` passes its text through; `/eustress-<subcommand>` reads as
/// `/eustress <subcommand> <text>`. Any other command name passes its text through, as `/eustress` does.
export function commandText(command, text) {
  const t = String(text || '');
  const m = String(command || '').toLowerCase().match(/^\/eustress-(space|live|profile|me|follow|help)$/);
  return m ? `${m[1]} ${t}`.trim() : t;
}

/// `{ sub, rest }` from the command's text.
function parseText(text) {
  const t = String(text || '').trim();
  const m = t.match(/^(\S+)\s*([\s\S]*)$/);
  return m ? { sub: m[1].toLowerCase(), rest: m[2].trim() } : { sub: '', rest: '' };
}
const SUBCOMMANDS = new Set(['space', 'live', 'profile', 'me', 'follow']);

async function spaceCommand(deps, scope, query, origin) {
  if (!query) return reply('Type a Space name: /eustress space <name>');
  const views = await listedViews(deps, scope.cap);
  const found = matching(views, clip(query, 100));
  if (!found.length) return reply(NO_MATCH);
  const [top, ...more] = found;
  const live = await liveWithViews(deps, scope.cap, [top]).catch(() => []);
  const blocks = spaceBlocks(top, origin, { players: live[0]?.players || 0 });
  const others = more.slice(0, 4);
  if (others.length) {
    blocks.push({
      type: 'section',
      text: { type: 'mrkdwn', text: ['*More matches*', ...others.map((v) => `• <${spacePage(origin, v.id)}|${esc(clip(v.name || 'Untitled Space', 100))}> by ${esc(clip(v.author_name || 'Unknown', 64))}`)].join('\n') },
    });
  }
  return reply(`${top.name || 'Untitled Space'} on Eustress`, { blocks, inChannel: true });
}

async function liveCommand(deps, scope, origin) {
  const views = await listedViews(deps, scope.cap);
  const live = (await liveWithViews(deps, scope.cap, views)).slice(0, 10);
  if (!live.length) return reply('No listed Spaces are live right now.');
  const blocks = [{ type: 'section', text: { type: 'mrkdwn', text: `*Live now on Eustress: ${live.length}*` } }];
  live.forEach((s, i) => {
    blocks.push({
      type: 'section',
      text: { type: 'mrkdwn', text: `*<${spacePage(origin, s.id)}|${esc(clip(s.view.name || 'Untitled Space', 150))}>*\nby ${esc(clip(s.view.author_name || 'Unknown', 64))} | ${Number(s.players) || 0}/${Number(s.max_players) || 0} playing | ${RATING_WORDS[s.view.rating] || 'Not rated'}` },
      accessory: button('Play', playPage(origin, s.id), `play_${i}`, true),
    });
  });
  return reply(`Live now on Eustress: ${live.length}`, { blocks, inChannel: true });
}

async function profileCommand(env, deps, scope, cmd, rest, origin) {
  const m = rest.match(/^<@([UW][A-Z0-9]{6,20})(?:\|[^>]*)?>$/);
  if (!m) return reply('Mention someone: /eustress profile @name');
  const account = await linkedAccountForSlack(env, cmd.team_id, m[1]);
  if (!account) return reply(NOT_LINKED_PROFILE);
  const shown = await publicProfileOf(env, account.id, account.user);
  const listed = (await listedViews(deps, scope.cap)).filter((v) => v.author_id === account.id).length;
  const username = account.user.username || '';
  const url = `${origin}/profile/${encodeURIComponent(username)}`;
  const avatar = typeof shown.avatar_url === 'string' && shown.avatar_url.startsWith('/assets/') ? `${origin}${shown.avatar_url}` : null;
  const title = shown.display_name || username || 'Eustress member';
  const lines = [`*<${url}|${esc(clip(title, 150))}>*`];
  if (shown.bio) lines.push(esc(clip(shown.bio, 300)));
  const blocks = [
    {
      type: 'section',
      text: { type: 'mrkdwn', text: lines.join('\n') },
      fields: [
        { type: 'mrkdwn', text: `*Username*\n${esc(clip(username || 'Unknown', 64))}` },
        { type: 'mrkdwn', text: `*Listed Spaces*\n${listed}` },
      ],
      ...(avatar ? { accessory: { type: 'image', image_url: avatar, alt_text: clip(title, 100) } } : {}),
    },
    { type: 'actions', elements: [button('View profile', url, 'view_profile')] },
  ];
  return reply(`${title} on Eustress`, { blocks, inChannel: true });
}

async function meCommand(env, scope, origin) {
  const account = scope.account;
  if (!account) return reply(`${NOT_LINKED_ME} <${origin}/settings|${origin}/settings>`);
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
  const name = account.user.username || 'Your account';
  const blocks = [
    {
      type: 'section',
      text: { type: 'mrkdwn', text: `*${esc(clip(name, 100))}*` },
      fields: [
        { type: 'mrkdwn', text: `*Bliss*\n${esc(bliss)}` },
        { type: 'mrkdwn', text: `*Tickets*\n${esc(tickets)}` },
        { type: 'mrkdwn', text: `*Unread notifications*\n${esc(unread)}` },
      ],
    },
    { type: 'actions', elements: [button('Open Eustress', `${origin}/dashboard`, 'open_dashboard')] },
  ];
  return reply(`Bliss ${bliss}, Tickets ${tickets}, unread notifications ${unread}`, { blocks });
}

// ── follow ────────────────────────────────────────────────────────────────────

const followKey = (kind, target, install, team, channel) => `slackfollow:${kind}:${target}:${install}:${team}:${channel}`;
const indexKey = (install, team, channel, kind, target) => `slackfollowg:${install}:${team}:${channel}:${kind}:${target}`;
const norm = (text) => String(text || '').normalize('NFKC').trim().toLowerCase();

async function teamFollows(env, installId, team, channel = null) {
  const prefix = channel ? `slackfollowg:${installId}:${team}:${channel}:` : `slackfollowg:${installId}:${team}:`;
  const list = await env.SOCIAL.list({ prefix, limit: 1000 });
  const out = [];
  for (const k of list.keys) {
    const [, , tm, ch, kind, target] = k.name.split(':');
    const key = followKey(kind, target, installId, tm, ch);
    const raw = await env.SOCIAL.get(key);
    if (!raw) continue;
    try { out.push({ ...JSON.parse(raw), key, index: k.name }); } catch { /* skipped */ }
  }
  return out;
}

/// Whether this member may change follows: the person who installed the app, or a workspace admin or owner as
/// users.info reports them with the bot token. Any failure is no.
async function mayChangeFollows(env, install, cmd, fetchImpl) {
  if (install.record?.installer_user_id && install.record.installer_user_id === cmd.user_id) return true;
  const info = await slackApi(fetchImpl, 'users.info', install.token, { user: cmd.user_id }, { form: true });
  if (info.ok && info.user?.id === cmd.user_id && (info.user.is_admin === true || info.user.is_owner === true || info.user.is_primary_owner === true)) return true;
  if (AUTH_ERRORS.has(info.error)) await removeInstallation(env, install.id);
  return false;
}

/// Why `follow` cannot run here, or null with the installation it runs under.
async function followGate(env, cmd, fetchImpl) {
  if (cmd.channel_name === 'directmessage' || /^mpdm-/.test(String(cmd.channel_name || '')) || DM_RE.test(cmd.channel_id)) {
    return { refusal: reply('Use /eustress follow in a channel.') };
  }
  const install = await installationFor(env, cmd.team_id, cmd.enterprise_id);
  if (!install) return { refusal: reply('A workspace admin must add Eustress to this workspace (Add to Slack) before a channel can follow anything.') };
  if (!(await mayChangeFollows(env, install, cmd, fetchImpl))) {
    return { refusal: reply('Only the person who added Eustress to this workspace, or a workspace admin, can change what it follows.') };
  }
  return { install };
}

async function saveFollow(env, install, cmd, kind, target, name) {
  const existing = await teamFollows(env, install.id, cmd.team_id);
  if (!existing.some((f) => f.kind === kind && f.target === target && f.channel_id === cmd.channel_id) && existing.length >= FOLLOWS_PER_TEAM) {
    return `This workspace already has ${FOLLOWS_PER_TEAM} follows. Remove one first.`;
  }
  const record = {
    kind, target, name: clip(name, 100), install_id: install.id, team_id: cmd.team_id, channel_id: cmd.channel_id,
    created_by: cmd.user_id, created_at: new Date().toISOString(),
  };
  await env.SOCIAL.put(followKey(kind, target, install.id, cmd.team_id, cmd.channel_id), JSON.stringify(record));
  await env.SOCIAL.put(indexKey(install.id, cmd.team_id, cmd.channel_id, kind, target), '1');
  return null;
}

async function followCommand(env, deps, cmd, rest, fetchImpl) {
  const { sub, rest: arg } = parseText(rest);
  if (!['space', 'creator', 'list', 'remove'].includes(sub)) {
    return reply('Use /eustress follow space <name>, follow creator <username>, follow list, or follow remove <name>.');
  }
  const gate = await followGate(env, cmd, fetchImpl);
  if (gate.refusal) return gate.refusal;
  const { install } = gate;
  if (sub === 'space') {
    if (!arg) return reply('Type a Space name: /eustress follow space <name>');
    // Channel posts are at most POST_CAP, so a Space above it cannot be followed.
    const found = matching(await listedViews(deps, POST_CAP), clip(arg, 100))[0];
    if (!found || !TARGET_ID.test(found.id)) return reply('That Space is not listed, or its rating is above what a Slack channel can show.');
    const refused = await saveFollow(env, install, cmd, 'space', found.id, found.name || 'Untitled Space');
    return reply(refused || `This channel now follows ${esc(clip(found.name || 'Untitled Space', 100))}. Posts come when it is updated or goes live. ${INVITE_HINT}`);
  }
  if (sub === 'creator') {
    const creator = await resolveCreator(env, arg);
    if (!creator) return reply('No Eustress account has that username.');
    const refused = await saveFollow(env, install, cmd, 'creator', creator.id, creator.username);
    return reply(refused || `This channel now follows ${esc(clip(creator.username, 64))}. Posts come when they list, update or host a Space. ${INVITE_HINT}`);
  }
  if (sub === 'list') {
    const follows = await teamFollows(env, install.id, cmd.team_id);
    if (!follows.length) return reply('This workspace does not follow anything yet.');
    const lines = follows.map((f) => {
      const paused = !f.disabled_at ? '' : f.disabled_reason === 'not_in_channel'
        ? ' (paused: the app is not in that channel. Invite it with /invite @Eustress Engine, then follow again)'
        : ' (paused: Eustress cannot post there)';
      return `• <#${f.channel_id}> ${f.kind}: ${esc(f.name)}${paused}`;
    });
    return reply(lines.join('\n'));
  }
  // remove
  const follows = await teamFollows(env, install.id, cmd.team_id, cmd.channel_id);
  const hit = follows.find((f) => `${f.kind}:${f.target}` === arg) || follows.find((f) => norm(f.name) === norm(arg));
  if (!hit) return reply('This channel has no follow by that name.');
  await env.SOCIAL.delete(hit.key);
  await env.SOCIAL.delete(hit.index);
  return reply(`This channel no longer follows ${esc(clip(hit.name, 100))}.`);
}

/// The reply to one slash command. Exported for the tests; the endpoint posts it to the command's response_url.
export async function commandReply(cmd, env, deps = {}) {
  const origin = siteOrigin(env);
  const fetchImpl = deps.fetch || fetch;
  const { sub, rest } = parseText(cmd?.text);
  if (!SUBCOMMANDS.has(sub)) return helpReply(origin);
  if (sub === 'follow') return followCommand(env, deps, cmd, rest, fetchImpl);
  const scope = await commandScope(env, cmd);
  switch (sub) {
    case 'space': return spaceCommand(deps, scope, rest, origin);
    case 'live': return liveCommand(deps, scope, origin);
    case 'profile': return profileCommand(env, deps, scope, cmd, rest, origin);
    case 'me': return meCommand(env, scope, origin);
    default: return helpReply(origin);
  }
}

/// Where a reply may be posted: Slack's own response URLs only.
export function validResponseUrl(value) {
  try {
    const u = new URL(String(value || ''));
    return u.protocol === 'https:' && u.hostname === 'hooks.slack.com' && u.port === '' && !u.username && !u.password && u.pathname.startsWith('/commands/');
  } catch {
    return false;
  }
}

async function postResponse(fetchImpl, responseUrl, message) {
  try {
    const res = await fetchImpl(responseUrl, { method: 'POST', headers: { 'Content-Type': 'application/json; charset=utf-8' }, body: JSON.stringify(message) });
    if (!res.ok) console.error(`slack response_url answered ${res.status}`);
    return !!res.ok;
  } catch (e) {
    console.error('slack response_url unreachable:', e?.message || e);
    return false;
  }
}

async function handleCommand(request, env, ctx, deps) {
  const verified = await verifiedBody(request, env, deps.now ? deps.now() : Date.now());
  if (verified.response) return verified.response;
  const form = new URLSearchParams(verified.text);
  if (form.get('api_app_id') !== env.SLACK_APP_ID) return answer({ error: 'This request is for another Slack app.' }, 403);
  const cmd = {
    team_id: form.get('team_id') || '', enterprise_id: form.get('enterprise_id') || null, user_id: form.get('user_id') || '',
    channel_id: form.get('channel_id') || '', channel_name: form.get('channel_name') || '', text: commandText(form.get('command'), form.get('text')),
    response_url: form.get('response_url') || '',
  };
  const origin = siteOrigin(env);
  if (!TEAM_RE.test(cmd.team_id) || !USER_RE.test(cmd.user_id) || !CHANNEL_RE.test(cmd.channel_id)) {
    return answer(reply('That request could not be read. Try again.'));
  }
  const { sub } = parseText(cmd.text);
  if (!SUBCOMMANDS.has(sub)) return answer(helpReply(origin));
  if (!validResponseUrl(cmd.response_url)) return answer(reply('That request could not be read. Try again.'));
  const fetchImpl = deps.fetch || fetch;
  const work = (async () => {
    let message;
    try { message = await commandReply(cmd, env, deps); }
    catch (e) {
      console.error(`slack /eustress ${sub} failed:`, e?.message || e);
      message = reply('Something went wrong. Try again in a minute.');
    }
    await postResponse(fetchImpl, cmd.response_url, message);
  })();
  if (ctx && typeof ctx.waitUntil === 'function') ctx.waitUntil(work); else await work;
  return ack();
}

// -----------------------------------------------------------------------------
// Events and interactivity
// -----------------------------------------------------------------------------

export function homeView(origin) {
  return {
    type: 'home',
    blocks: [
      { type: 'header', text: { type: 'plain_text', text: 'Eustress', emoji: false } },
      { type: 'section', text: { type: 'mrkdwn', text: 'Eustress is a simulation and data platform. Creators build Spaces in Eustress Studio and publish them to the Gallery; people play them in the Eustress Player or right in the browser.' } },
      { type: 'section', text: { type: 'mrkdwn', text: HELP_TEXT } },
      { type: 'context', elements: [{ type: 'mrkdwn', text: 'Channels show Spaces rated for ages 10 and up. A linked account that is 18 or older sees more only here, in its own DM with the app.' }] },
      { type: 'context', elements: [{ type: 'mrkdwn', text: 'For follow posts in a channel, invite the app there first with /invite @Eustress Engine.' }] },
      { type: 'actions', elements: [button('Eustress on Slack', `${origin}/slack`, 'open_slack_page'), button('Link your account', `${origin}/settings`, 'open_settings', true)] },
    ],
  };
}

async function publishHome(env, teamId, enterpriseId, userId, fetchImpl) {
  const install = await installationFor(env, teamId, enterpriseId);
  if (!install) return false;
  const r = await slackApi(fetchImpl, 'views.publish', install.token, { user_id: userId, view: homeView(siteOrigin(env)) });
  if (!r.ok) {
    console.error(`slack views.publish answered ${r.error}`);
    if (AUTH_ERRORS.has(r.error)) await removeInstallation(env, install.id);
  }
  return r.ok;
}

async function handleEvent(request, env, ctx, deps) {
  const verified = await verifiedBody(request, env, deps.now ? deps.now() : Date.now());
  if (verified.response) return verified.response;
  let body;
  try { body = JSON.parse(verified.text); } catch { return answer({ error: 'bad request' }, 400); }
  if (!body || typeof body !== 'object') return answer({ error: 'bad request' }, 400);
  if (body.type === 'url_verification') {
    return typeof body.challenge === 'string' && body.challenge.length <= 500 ? answer({ challenge: body.challenge }) : answer({ error: 'bad request' }, 400);
  }
  if (body.api_app_id !== env.SLACK_APP_ID) return answer({ error: 'This request is for another Slack app.' }, 403);
  if (body.type !== 'event_callback' || !body.event || typeof body.event !== 'object') return ack();
  const event = body.event;
  const team = String(body.team_id || '');
  const enterprise = ENTERPRISE_RE.test(String(body.enterprise_id || '')) ? String(body.enterprise_id) : null;
  const retry = request.headers.get('X-Slack-Retry-Num');
  const fetchImpl = deps.fetch || fetch;

  if (event.type === 'app_uninstalled' || (event.type === 'tokens_revoked' && Array.isArray(event.tokens?.bot) && event.tokens.bot.length)) {
    // Deleting is the same however many times it runs, so a retry is honoured here.
    const installId = body.is_enterprise_install === true && enterprise ? enterprise : team;
    if (INSTALL_ID_RE.test(installId)) await removeInstallation(env, installId);
    return ack();
  }
  if (event.type === 'app_home_opened') {
    const user = String(event.user || '');
    if (!TEAM_RE.test(team) || !USER_RE.test(user)) return ack();
    // Slack names the member's DM with the app here: the one place a higher cap may apply (commandScope).
    if (DM_RE.test(String(event.channel || '')) && env.SOCIAL) await env.SOCIAL.put(appDmKey(team, user), String(event.channel));
    if (event.tab === 'home' && !retry) {
      const work = publishHome(env, team, enterprise, user, fetchImpl).catch((e) => console.error('slack home failed:', e?.message || e));
      if (ctx && typeof ctx.waitUntil === 'function') ctx.waitUntil(work); else await work;
    }
    return ack();
  }
  return ack();
}

async function handleInteractivity(request, env, deps) {
  const verified = await verifiedBody(request, env, deps.now ? deps.now() : Date.now());
  if (verified.response) return verified.response;
  let payload = null;
  try { payload = JSON.parse(new URLSearchParams(verified.text).get('payload') || 'null'); } catch { payload = null; }
  if (!payload || typeof payload !== 'object') return answer({ error: 'bad request' }, 400);
  if (payload.api_app_id !== env.SLACK_APP_ID) return answer({ error: 'This request is for another Slack app.' }, 403);
  // Every button the app sends is a link button: the browser opens the URL, and Slack wants only an acknowledgment.
  return ack();
}

// -----------------------------------------------------------------------------
// Install (OAuth v2)
// -----------------------------------------------------------------------------

const installStateKey = async (state) => `slackinstallstate:${await sha256Hex(state)}`;
const redirect = (location) => new Response(null, { status: 302, headers: { Location: location, 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer' } });
const siteResult = (env, query) => redirect(`${siteOrigin(env)}/slack?${query}`);
const installFailed = (env, code) => siteResult(env, `install_error=${encodeURIComponent(code)}`);

async function handleInstallStart(request, url, env) {
  if (request.method !== 'GET') return answer({ error: 'Method not allowed' }, 405);
  if (!slackBotConfigured(env) || !env.CHALLENGES) return installFailed(env, 'slack_unavailable');
  if (env.AUTH_RATE_LIMITER) {
    const address = request.headers.get('CF-Connecting-IP');
    if (address) {
      const { success } = await env.AUTH_RATE_LIMITER.limit({ key: `slackinstall:${address}` });
      if (!success) return installFailed(env, 'rate_limited');
    }
  }
  const state = randomHex(32);
  const redirectUri = `${url.origin}${OAUTH_REDIRECT_PATH}`;
  await env.CHALLENGES.put(await installStateKey(state), JSON.stringify({ redirect_uri: redirectUri, at: new Date().toISOString() }), { expirationTtl: INSTALL_STATE_TTL_S });
  const authorize = new URL(SLACK_OAUTH_AUTHORIZE);
  authorize.searchParams.set('client_id', env.SLACK_CLIENT_ID);
  authorize.searchParams.set('scope', SLACK_BOT_SCOPE);
  authorize.searchParams.set('redirect_uri', redirectUri);
  authorize.searchParams.set('state', state);
  return redirect(authorize.toString());
}

/// Revoke a token Slack issued that will not be kept. Best effort; never logs it.
async function revokeToken(fetchImpl, token) {
  if (typeof token !== 'string' || !token) return;
  await slackApi(fetchImpl, 'auth.revoke', token, {}, { form: true });
}

async function handleOAuthRedirect(request, url, env, deps) {
  if (request.method !== 'GET') return answer({ error: 'Method not allowed' }, 405);
  if (!slackBotConfigured(env) || !env.CHALLENGES || !env.USERS) return installFailed(env, 'slack_unavailable');
  const state = url.searchParams.get('state') || '';
  if (!STATE_RE.test(state)) return installFailed(env, 'invalid_state');
  const key = await installStateKey(state);
  const heldRaw = await env.CHALLENGES.get(key);
  if (!heldRaw) return installFailed(env, 'state_expired');
  // Spent before anything else: a state is good for one try.
  await env.CHALLENGES.delete(key);
  let held;
  try { held = JSON.parse(heldRaw); } catch { return installFailed(env, 'state_expired'); }
  const denied = url.searchParams.get('error');
  if (denied) return installFailed(env, denied === 'access_denied' ? 'access_denied' : 'slack_failed');
  const code = url.searchParams.get('code') || '';
  if (!CODE_RE.test(code) || typeof held?.redirect_uri !== 'string') return installFailed(env, 'bad_request');

  const fetchImpl = deps.fetch || fetch;
  let grant;
  try {
    const res = await fetchImpl(SLACK_OAUTH_ACCESS, {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
      body: new URLSearchParams({ client_id: env.SLACK_CLIENT_ID, client_secret: env.SLACK_CLIENT_SECRET, code, redirect_uri: held.redirect_uri }).toString(),
    });
    grant = await res.json().catch(() => ({}));
    if (!res.ok || grant?.ok !== true) {
      const error = typeof grant?.error === 'string' ? grant.error : '';
      console.error(`slack install exchange answered ${res.status} ${error}`.trim());
      if (typeof grant?.access_token === 'string') await revokeToken(fetchImpl, grant.access_token);
      return installFailed(env, CODE_ERRORS.has(error) ? 'slack_code_rejected' : 'slack_failed');
    }
  } catch (e) {
    console.error('slack install exchange unreachable:', e?.message || e);
    return installFailed(env, 'slack_failed');
  }
  // A user token is never asked for; one handed back anyway is not kept.
  if (typeof grant.authed_user?.access_token === 'string') await revokeToken(fetchImpl, grant.authed_user.access_token);

  const token = grant.access_token;
  const enterpriseInstall = grant.is_enterprise_install === true;
  const teamId = typeof grant.team?.id === 'string' ? grant.team.id : null;
  const enterpriseId = typeof grant.enterprise?.id === 'string' ? grant.enterprise.id : null;
  const installId = enterpriseInstall ? enterpriseId : teamId;
  const valid = grant.token_type === 'bot' && typeof token === 'string' && BOT_TOKEN_RE.test(token)
    && (grant.app_id === undefined || grant.app_id === env.SLACK_APP_ID)
    && (enterpriseInstall ? ENTERPRISE_RE.test(String(installId || '')) : TEAM_RE.test(String(installId || '')))
    && INSTALL_ID_RE.test(String(installId || ''));
  if (!valid) {
    console.error('slack install answered without a usable bot token or workspace id');
    if (typeof token === 'string') await revokeToken(fetchImpl, token);
    return installFailed(env, 'slack_failed');
  }
  const text = (v, max) => (typeof v === 'string' && v.trim() ? v.trim().slice(0, max) : null);
  const record = {
    v: 1,
    install_id: installId,
    is_enterprise_install: enterpriseInstall,
    team_id: TEAM_RE.test(String(teamId || '')) ? teamId : null,
    team_name: text(grant.team?.name, 100),
    enterprise_id: ENTERPRISE_RE.test(String(enterpriseId || '')) ? enterpriseId : null,
    bot_user_id: BOT_USER_RE.test(String(grant.bot_user_id || '')) ? grant.bot_user_id : null,
    scope: text(grant.scope, 500),
    installer_user_id: USER_RE.test(String(grant.authed_user?.id || '')) ? grant.authed_user.id : null,
    installed_at: new Date().toISOString(),
    token: await sealSlackBotToken(env, installId, token),
  };
  await env.USERS.put(installKey(installId), JSON.stringify(record));
  return siteResult(env, 'installed=1');
}

// -----------------------------------------------------------------------------
// Dispatch
// -----------------------------------------------------------------------------

/// Handles `/api/slack/*`, or answers null for any other path. `deps`: listViews(cap), liveSessions(cap) (index.js
/// discordDeps), and for tests fetch and now.
export async function handleSlackBotRoute(request, url, env, ctx, deps = {}) {
  switch (url.pathname) {
    case INSTALL_PATH: return handleInstallStart(request, url, env);
    case OAUTH_REDIRECT_PATH: return handleOAuthRedirect(request, url, env, deps);
    case COMMANDS_PATH: return handleCommand(request, env, ctx, deps);
    case EVENTS_PATH: return handleEvent(request, env, ctx, deps);
    case INTERACTIVITY_PATH: return handleInteractivity(request, env, deps);
    default: return null;
  }
}

// -----------------------------------------------------------------------------
// Follow posts (five-minute tick)
// -----------------------------------------------------------------------------

const POST_HEADLINE = { new: 'New on Eustress', updated: 'Updated on Eustress', live: 'Live now on Eustress' };

async function followsFor(env, kind, target) {
  if (!TARGET_ID.test(String(target || ''))) return [];
  const list = await env.SOCIAL.list({ prefix: `slackfollow:${kind}:${target}:`, limit: 1000 });
  const out = [];
  for (const k of list.keys) {
    const raw = await env.SOCIAL.get(k.name);
    if (!raw) continue;
    try { out.push({ key: k.name, record: JSON.parse(raw) }); } catch { /* skipped */ }
  }
  return out;
}

/// The sweep: see the header. Never throws out of slackTick. `deps`: listViews, liveSessions. `opts`: now, fetch.
export async function sweepSlackFollows(env, deps, opts = {}) {
  const out = { posted: 0, disabled: 0, removed_installs: 0, budget_spent: false, cursor: null };
  if (!env?.SOCIAL || !env?.USERS || !deps?.listViews) return out;
  const fetchImpl = opts.fetch || fetch;
  const startMs = opts.now ?? Date.now();
  const origin = siteOrigin(env);
  const cursor = await env.SOCIAL.get(CURSOR_KEY);
  const overlap = new Date(startMs - CURSOR_OVERLAP_MS).toISOString();

  // Nothing followed: only move the cursor, so a first follow does not post a backlog.
  const anyFollow = await env.SOCIAL.list({ prefix: 'slackfollowg:', limit: 1 });
  if (!anyFollow.keys.length) {
    const next = cursor && overlap < cursor ? cursor : overlap;
    await env.SOCIAL.put(CURSOR_KEY, next);
    out.cursor = next;
    return out;
  }

  const all = await deps.listViews(POST_CAP);
  const postable = (Array.isArray(all) ? all : []).filter((v) => v && typeof v.id === 'string' && withinCap(v.rating, POST_CAP));
  const events = [];
  const stamp = (v) => String(v.updated_at || v.published_at || '');
  if (cursor) {
    for (const v of postable.filter((x) => stamp(x) > cursor).sort((a, b) => (stamp(a) < stamp(b) ? -1 : 1))) {
      events.push({ id: `upd:${v.id}:${stamp(v)}`, view: v, kind: null, stamp: stamp(v) });
    }
  }
  try {
    for (const s of await liveWithViews(deps, POST_CAP, postable)) {
      events.push({ id: `live:${s.id}:${s.since}`, view: s.view, kind: 'live', players: s.players });
    }
  } catch (e) {
    console.error('slack sweep: live sessions unavailable:', e?.message || e);
  }

  const installs = new Map();
  const installation = async (id) => {
    if (!installs.has(id)) installs.set(id, await readInstallation(env, id));
    return installs.get(id);
  };

  let lastDone = cursor;
  outer: for (const event of events) {
    const v = event.view;
    if (!event.kind) {
      const marker = await env.SOCIAL.get(`slacklisted:${v.id}`);
      event.kind = !marker && (v.version || 1) === 1 ? 'new' : 'updated';
    }
    const follows = [...await followsFor(env, 'space', v.id), ...await followsFor(env, 'creator', v.author_id)];
    for (const { key, record } of follows) {
      if (record.disabled_at || !CHANNEL_RE.test(String(record.channel_id || '')) || !TEAM_RE.test(String(record.team_id || ''))) continue;
      if (!withinCap(v.rating, POST_CAP)) continue;
      // Once per channel and event: a channel that follows both the Space and its creator gets one post.
      const posted = `slackposted:${await sha256Hex(`${record.team_id}:${record.channel_id}|${event.id}`)}`;
      if (await env.SOCIAL.get(posted)) continue;
      const install = await installation(record.install_id);
      if (!install) {
        // The installation is gone: so is the follow.
        await env.SOCIAL.delete(key);
        await env.SOCIAL.delete(indexKey(record.install_id, record.team_id, record.channel_id, record.kind, record.target));
        continue;
      }
      if (out.posted >= POSTS_PER_RUN) { out.budget_spent = true; break outer; }
      const headline = POST_HEADLINE[event.kind];
      const r = await slackApi(fetchImpl, 'chat.postMessage', install.token, {
        channel: record.channel_id,
        text: `${headline}: ${clip(v.name || 'Untitled Space', 150)}`,
        blocks: spaceBlocks(v, origin, { players: event.players || 0, headline }),
        unfurl_links: false,
        unfurl_media: false,
      });
      if (r.ok) {
        out.posted += 1;
        await env.SOCIAL.put(posted, '1', { expirationTtl: POSTED_TTL_S });
      } else if (r.error === 'ratelimited') {
        out.budget_spent = true;
        break outer;
      } else if (CHANNEL_ERRORS.has(r.error)) {
        out.disabled += 1;
        await env.SOCIAL.put(key, JSON.stringify({ ...record, disabled_at: new Date(startMs).toISOString(), disabled_reason: r.error }));
      } else if (AUTH_ERRORS.has(r.error)) {
        out.removed_installs += 1;
        await removeInstallation(env, install.id);
        installs.set(install.id, null);
      } else {
        console.error(`slack follow post answered ${r.error}`);
      }
    }
    if (event.kind === 'new' || event.kind === 'updated') {
      await env.SOCIAL.put(`slacklisted:${v.id}`, '1');
      lastDone = event.stamp;
    }
  }

  let next = out.budget_spent ? lastDone || cursor || overlap : overlap;
  if (cursor && next < cursor) next = cursor;
  await env.SOCIAL.put(CURSOR_KEY, next);
  out.cursor = next;
  return out;
}

/// The five-minute tick's Slack work. Skipped without SLACK_CLIENT_SECRET (no token could open). Never throws.
export async function slackTick(env, deps, opts = {}) {
  try {
    if (!env?.SLACK_CLIENT_SECRET) return { skipped: true };
    return { sweep: await sweepSlackFollows(env, deps, opts) };
  } catch (e) {
    console.error('slack follow sweep failed:', e?.message || e);
    return { failed: true };
  }
}
