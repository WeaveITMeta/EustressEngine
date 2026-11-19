// =============================================================================
// Which routes ask the account's object, and which refuse a frozen account
// =============================================================================
//
// Every request that changes something (POST, PUT, PATCH, DELETE) is classified
// here before it is dispatched, and the class decides how its session is checked:
//
//   public   no session of its own: sign-in, sign-up, a provider's webhook, a
//            one-time link, an anonymous beacon. The route proves itself.
//   session  the cheap check the route makes itself (the KV mirror of the floor,
//            up to about a minute behind). For what is easy to undo or harmless.
//   fresh    the account's object is asked: the session must be newer than the
//            floor right now (a sign-out, a ban or a key replacement ends it at
//            once, everywhere), and an object that cannot be reached means no.
//   value    fresh, and refused while the account's value is frozen after a key
//            replacement: what moves value out of the account or changes where
//            it goes.
//
// A route this table does not name is `fresh`: the strict class is the default,
// so a route added later cannot silently take the cheap path. And
// tests/route_policy.test.mjs fails when a path in the source is not named here
// (`explicit: false`), so adding a route means deciding its class.
//
// Reads (GET, HEAD) are not gated. Design: docs/architecture/
// IDENTITY_KEY_ROTATION_AND_RECOVERY.md, "What asks the Object".
// =============================================================================

export const PUBLIC = 'public';
export const SESSION = 'session';
export const FRESH = 'fresh';
export const VALUE = 'value';

const MUTATING = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);
export const isMutating = (method) => MUTATING.has(String(method || '').toUpperCase());

const ID = '[a-f0-9-]+';

/// `[pattern, class, why]`. A string ending in `*` is a prefix; any other string
/// is the whole path; a RegExp is tested against the whole path. The first rule
/// that matches decides, so a specific rule goes before the family it belongs to.
///
/// Only the STRICT classes (fresh, value) may use a prefix: a route added under
/// one is strict as well. A public or session class names its routes one by one,
/// so a route added under the same prefix is not named, is fresh, and fails
/// tests/route_policy.test.mjs until someone decides what it is.
export const RULES = [
  // ── Public: no account, or the request proves itself ─────────────────────
  ['/api/auth/register', PUBLIC, 'sign-up: no account yet'],
  ['/api/auth/challenge', PUBLIC, 'sign-in: no session yet'],
  ['/api/auth/verify-challenge', PUBLIC, 'sign-in: the signature is the proof'],
  ['/api/passkeys/login/begin', PUBLIC, 'passkey sign-in: no session yet; limited per address'],
  ['/api/passkeys/login', PUBLIC, 'passkey sign-in: the passkey\'s signature is the proof'],
  ['/api/stripe/webhook', PUBLIC, 'signed by Stripe'],
  ['/api/discord/interactions', PUBLIC, 'the Discord app interactions endpoint: no session; every request must carry a valid Ed25519 signature under DISCORD_PUBLIC_KEY, and an unsigned or wrongly signed one is answered 401 before anything is read (src/discord_interactions.mjs)'],
  ['/api/slack/install', PUBLIC, 'the Slack app install start (a GET; src/slack_bot.mjs): no session, anyone may add the app to a workspace they manage; it redirects to Slack with a random one-time state kept hashed for ten minutes; limited per address'],
  ['/api/slack/oauth_redirect', PUBLIC, 'Slack returns the install here (a GET; src/slack_bot.mjs): no session; the one-time state this Worker issued is the gate, spent before the code is exchanged, and the bot token is kept sealed'],
  ['/api/slack/commands', PUBLIC, 'the Slack app slash command (src/slack_bot.mjs): no session; every request must carry a valid Slack signature (HMAC-SHA256 under SLACK_SIGNING_SECRET, within five minutes), and an unsigned or wrongly signed one is answered 401 before anything is parsed'],
  ['/api/slack/events', PUBLIC, 'the Slack app Events API endpoint (src/slack_bot.mjs): no session; every request must carry a valid Slack signature under SLACK_SIGNING_SECRET, checked before anything is parsed'],
  ['/api/slack/interactivity', PUBLIC, 'the Slack app interactivity endpoint (src/slack_bot.mjs): no session; every request must carry a valid Slack signature under SLACK_SIGNING_SECRET; it only acknowledges'],
  ['/api/guest/allowance', PUBLIC, 'a guest session, no account'],
  ['/api/moderation/emulate', PUBLIC, 'the judge emulator for the Eustress MCP: pure rules over a listing and an answer the caller wrote, nothing stored, no model called; limited per address (src/moderation.mjs emulateReview)'],
  ['/api/guest/beat', PUBLIC, 'a guest session, no account'],
  ['/api/guest/end', PUBLIC, 'a guest session, no account'],
  ['/api/guest/play', PUBLIC, 'a guest session, no account'],
  ['/api/guest/ticket', PUBLIC, 'a guest session, no account: the lease ticket in the body is the proof'],
  ['/api/guest/live', PUBLIC, 'read-only: the guest-only sessions live now'],
  ['/api/kyc/jurisdiction', PUBLIC, 'verification at registration: no account yet'],
  ['/api/kyc/upload', PUBLIC, 'verification at registration: no account yet'],
  ['/api/kyc/submit', PUBLIC, 'verification at registration: no account yet'],
  ['/api/kyc/handoff', PUBLIC, 'verification at registration: no account yet'],
  ['/api/kyc/start', PUBLIC, 'starts a verification session: no account yet; its secrets are returned once; limited per address'],
  [/^\/api\/kyc\/(handoff|session|status)\/[^/]+$/, PUBLIC, 'verification at registration: read by an unguessable id'],
  ['/api/account/email/confirm', PUBLIC, 'a one-time link is the proof'],
  ['/api/takedown/requests', PUBLIC, 'a TAKE IT DOWN request needs no account (a session is recorded when present); limited per address and per contact email (src/takedown.mjs)'],
  [/^\/api\/cases\/[^/]+$/, PUBLIC, 'read-only: the minimal status of a case, by its unguessable reference'],
  [new RegExp(`^/api/simulations/${ID}/(archive|unarchive)$`), SESSION, 'an author unlists or relists their own listing; each undoes the other'],
  ['/api/account/time-zone', SESSION, 'a display preference for the account\'s emails; harmless to change'],
  ['/api/account/adult-content', SESSION, 'a viewing preference the account sets for itself: turning it on needs a verified adult, turning it off is always allowed, and each undoes the other'],
  ['/api/account/email/cancel', PUBLIC, 'a one-time link, or a session the handler checks fresh itself'],
  ['/api/account/key-restore', PUBLIC, 'a one-time link in the notice of an administrator\'s replacement is the proof'],
  ['/api/analytics/hit', PUBLIC, 'an anonymous, cookieless beacon'],
  ['/api/telemetry/usage', PUBLIC, 'anonymous aggregates'],
  ['/api/telemetry/comment', PUBLIC, 'anonymous feedback'],
  ['/api/telemetry/summary', PUBLIC, 'read-only'],
  ['/api/node/heartbeat', PUBLIC, 'an anonymous node beacon: the handler has no account'],
  ['/api/node/stats', PUBLIC, 'read-only'],
  ['/api/identity/verify', PUBLIC, 'a host proves itself with its own credential'],
  [new RegExp(`^/api/simulations/${ID}/play-pass$`), PUBLIC, 'the Player redeems a one-time play ticket: the ticket in the body is the proof, and it is spent once (src/play_pass.mjs)'],
  ['/api/notifications/unsubscribe', PUBLIC, 'a one-click link carries its own token'],
  ['/api/community/stats', PUBLIC, 'read-only'],
  ['/api/community/search', PUBLIC, 'read-only'],
  ['/api/community/leaderboard', PUBLIC, 'read-only'],
  [/^\/api\/community\/users\/[^/]+$/, PUBLIC, 'read-only'],
  ['/api/gallery', PUBLIC, 'read-only'],
  ['/api/gallery/featured', PUBLIC, 'read-only'],
  ['/api/simulations/tags', PUBLIC, 'read-only: the tags most used on listed listings'],
  ['/api/simulations/live', PUBLIC, 'read-only: the live sessions of listed listings'],
  ['/api/makers', PUBLIC, 'read-only: the people with a listed listing'],
  [new RegExp(`^/api/simulations/${ID}/previews$`), PUBLIC, 'read-only: the review captures of an approved listing'],
  [new RegExp(`^/api/simulations/${ID}/preview/[0-7]$`), PUBLIC, 'read-only: one review capture of an approved listing'],
  ['/api/marketplace', PUBLIC, 'a stub'],
  ['/api/projects', PUBLIC, 'a stub'],
  ['/api/projects/recent', PUBLIC, 'a stub'],
  [/^\/api\/simulation\/[^/]+(\/latest)?\/manifest$/, PUBLIC, 'read-only manifests'],
  ['/api/ledger/summary', PUBLIC, 'public ledger reads'],
  ['/api/ledger/leaderboard', PUBLIC, 'public ledger reads'],
  [/^\/api\/ledger\/(distribution|history)\/[^/]+$/, PUBLIC, 'public ledger reads'],
  ['/api/tickets/packages', PUBLIC, 'read-only'],
  ['/api/sponsors', PUBLIC, 'read-only: the sponsor tiers and the sponsors who asked to be listed'],
  ['/api/delta/packs', PUBLIC, 'read-only'],
  ['/api/delta/catalog', PUBLIC, 'read-only'],
  ['/api/payouts/rate', PUBLIC, 'read-only'],
  ['/api/economics/actual', PUBLIC, 'read-only: the public statement of the Economics page'],
  [/^\/api\/commerce\/catalog\/[a-f0-9-]{8,64}$/, PUBLIC, 'the public catalog read'],
  [/^\/api\/commerce\/receipts\/[a-f0-9-]{8,64}\/pur_[A-Za-z0-9]{24}$/, PUBLIC, 'a receipt is its own capability'],

  // ── Value: moves value out of the account, or changes where it goes ─────
  ['/api/stripe/connect/onboard', VALUE, 'creates or changes the payout destination (and refused outright while the payout pin lasts)'],
  ['/api/ledger/spend', VALUE, 'spends Bliss'],
  ['/api/commerce/purchases', VALUE, 'buys another account\'s product'],
  [/^\/api\/commerce\/purchases\/pur_[A-Za-z0-9]{24}\/refund$/, VALUE, 'a refund returns a seller\'s value'],

  // ── Fresh: acts as the account, or cannot be taken back ─────────────────
  [new RegExp('^/api/admin/release-notify/[a-z0-9-]+/send$'), FRESH, 'emails every subscriber of a product, once each: cannot be taken back'],
  [/^\/api\/admin\/takedowns(\/[a-f0-9-]+(\/(confirm|invalid|close))?)?$/, FRESH, 'an administrator reviews a TAKE IT DOWN request: confirms it, marks it invalid (which restores the content), or closes it'],
  ['/api/admin/grok-model', FRESH, 'read: the Grok model in use, its history and the auto-update state, for administrators (src/grok_model.mjs)'],
  ['/api/admin/grok-model/pin', FRESH, 'an administrator pins the Grok model every xAI call sends, which turns auto-update off; the canary must pass first'],
  ['/api/admin/grok-model/unpin', FRESH, 'an administrator turns Grok model auto-update back on'],
  ['/api/staff/review-latency', FRESH, 'read: listing-review latency aggregates, administrators only (src/review_latency.mjs)'],
  ['/api/admin/*', FRESH, 'the admin surface'],
  ['/api/payouts/*', FRESH, 'payouts'],
  ['/api/accounting/*', FRESH, 'the admin books'],
  ['/api/identity/rotate', FRESH, 'replaces the identity key'],
  ['/api/identity/rotate/*', FRESH, 'replaces the identity key'],
  ['/api/auth/discord/begin', FRESH, 'starts linking a Discord account to this account (src/discord_link.mjs)'],
  ['/api/auth/discord/finish', FRESH, 'links a Discord account to this account: a credential the account owns, revoked by a key replacement'],
  ['/api/auth/slack/begin', FRESH, 'starts linking a Slack account to this account (src/slack_link.mjs)'],
  ['/api/auth/slack/finish', FRESH, 'links a Slack account to this account: a credential the account owns, revoked by a key replacement'],
  ['/api/passkeys/*', FRESH, 'adds or removes a passkey: a key change, which also needs an identity-file session and the identity key\'s signature'],
  ['/api/account/profile', FRESH, 'the display name, bio and picture every visitor reads: a session that was signed out or replaced must not change them'],
  ['/api/account/email', FRESH, 'the account email'],
  ['/api/account/email/*', FRESH, 'the account email'],
  [new RegExp(`^/api/account/spaces/${ID}/(delete-request|delete-confirm)$`), FRESH, 'the owner deletes a published Space: the typed name and a code emailed to the verified address; ninety days later it cannot be taken back. The handler also refuses it during the pause after a key replacement (src/space_delete.mjs)'],
  [new RegExp(`^/api/account/spaces/${ID}/(restore-request|restore-confirm)$`), FRESH, 'the owner restores a deleted Space within its ninety days, with the same emailed code (src/space_delete.mjs)'],
  ['/api/keys', FRESH, 'creates an API key'],
  ['/api/keys/*', FRESH, 'revokes or rotates an API key'],
  ['/api/commerce/webhook_endpoints', FRESH, 'creates a webhook endpoint'],
  ['/api/commerce/webhook_endpoints/*', FRESH, 'changes or deletes a webhook endpoint'],
  ['/api/commerce/products', FRESH, 'lists a product for sale'],
  ['/api/commerce/products/*', FRESH, 'changes a product on sale'],
  [/^\/api\/commerce\/purchases\/pur_[A-Za-z0-9]{24}\/fulfill$/, FRESH, 'marks a purchase granted'],
  ['/api/delta/charge', FRESH, 'spends Delta on Eustress\'s own tools: the same account\'s balance, nothing transferable. A key replacement does not pause it (McKale, 2026-10-06: the audit trail can revert the damage), so this is fresh and not value'],
  ['/api/simulations/publish', FRESH, 'publishes'],
  ['/api/kyc/link', FRESH, 'binds a completed identity verification to the signed-in account: one verification, one account, and it cannot be taken back (src/kyc_identity.mjs)'],
  [new RegExp(`^/api/simulations/${ID}/(space|spaces/[^/]+|thumbnail|website-manifest|changelogs|page|page/review|page/readme|page/image/[^/]+)$`), FRESH, 'publishing, and the record of it (the sim page and its README: src/page.mjs)'],
  [new RegExp(`^/api/simulations/${ID}/space/multipart/(create|part|complete)$`), FRESH, 'publishing'],
  [new RegExp(`^/api/simulations/${ID}/world/.+$`), FRESH, 'publishing a world'],
  [new RegExp(`^/api/simulations/${ID}/changelogs/.+$`), FRESH, 'one publish record: read only, the author or an admin'],
  [new RegExp(`^/api/simulations/${ID}/(dossier|submit|appeal|moderation)$`), FRESH, 'moderation steps of a listing'],
  [new RegExp(`^/api/simulations/${ID}/tags$`), FRESH, 'the author changes their listing\'s tags (docs/networking/LISTING_TAGS.md): listing text a review reads, so a changed set sends the listing back to review, as a commit\'s does'],
  ['/api/agents/agreement', PUBLIC, 'read-only: the agent agreement text, its hash, and whether agent sign-up is open (src/agents.mjs)'],
  ['/api/agents', SESSION, 'read: the signed-in sponsor\'s own agents (src/agents.mjs)'],
  ['/api/agents/register', PUBLIC, 'an AI agent registers itself (src/agents.mjs): a one-time invite from a verified sponsor and the agent\'s own Ed25519 signature are the proof; off unless AGENT_SIGNUP is on and the agreement is final'],
  ['/api/agents/invites', FRESH, 'a verified adult sponsor accepts the agent agreement and makes a one-time agent invite (src/agents.mjs)'],
  [new RegExp(`^/api/agents/${ID}/(suspend|unsuspend|revoke)$`), FRESH, 'a sponsor suspends, resumes or revokes their own agent; revoking ends its sessions (src/agents.mjs)'],
  [new RegExp(`^/api/simulations/${ID}/listing$`), FRESH, 'the author edits their listing\'s title, description and visibility from the website (docs/networking/LISTING_EDIT.md): a changed title or description is text a review reads and sends the listing back to review, as a commit\'s does'],
  [new RegExp(`^/api/simulations/${ID}/rating-questionnaire$`), FRESH, 'the author answers the rating questionnaire a review reads (Guardian Code v2.1 Part V); the author\'s session, checked fresh like the other moderation steps'],
  [new RegExp(`^/api/simulations/${ID}/captures/[0-7]$`), FRESH, 'moderation steps of a listing'],
  [new RegExp(`^/api/simulations/${ID}/live(/admit)?$`), FRESH, 'hosting a listing'],
  [new RegExp(`^/api/simulations/${ID}/allocate$`), FRESH, 'a player asks the fleet for a seat in a hosted session (src/fleet.mjs): a signed-in account checked fresh, or a guest with a live lease and no session'],
  [/^\/api\/fleet\/hosts\/[A-Za-z0-9_-]{1,64}\/heartbeat$/, PUBLIC, 'a rented machine\'s host agent: no account; its ehost_ credential is the proof, checked before anything is read (src/fleet.mjs)'],
  ['/api/guest/chat/check', FRESH, 'the host of a guest-only session asks whether to deliver one guest chat line: the author\'s session, checked fresh like hosting'],
  [new RegExp(`^/api/simulations/${ID}/server-credential$`), FRESH, 'mints or revokes the credential a dedicated server uses to mark this listing live; it acts as the author'],

  // ── Session: the cheap check is enough, named one route at a time ───────
  ['/api/auth/logout', SESSION, 'ends the caller\'s own sessions'],
  ['/api/account/cases', SESSION, 'read: the account\'s own cases'],
  [new RegExp(`^/api/account/spaces/${ID}/deletion$`), SESSION, 'read: where a deletion of the owner\'s own listing stands, and what one would do (src/space_delete.mjs)'],
  [/^\/api\/account\/cases\/[^/]+\/review$/, SESSION, 'a party of a takedown asks a person to review it; it changes nothing by itself'],
  ['/api/auth/me', SESSION, 'read'],
  ['/api/auth/discord/unlink', SESSION, 'removes the account\'s own Discord link; it only takes something away (src/discord_link.mjs)'],
  ['/api/auth/slack/unlink', SESSION, 'removes the account\'s own Slack link; it only takes something away (src/slack_link.mjs)'],
  ['/api/passkeys', SESSION, 'read: the account\'s passkeys'],
  ['/api/avatar', SESSION, 'the account\'s avatar'],
  ['/api/identity/ticket', SESSION, 'proves the account to a host'],
  ['/api/identity/email-backup', SESSION, 'mails the account\'s own file to its own address'],
  ['/api/workshop/context', SESSION, 'preferences'],
  ['/api/workshop/context/memory', SESSION, 'preferences'],
  ['/api/social/follow', SESSION, 'follows, favourites, plays'],
  ['/api/social/unfollow', SESSION, 'follows, favourites, plays'],
  ['/api/social/favorite', SESSION, 'follows, favourites, plays'],
  ['/api/social/unfavorite', SESSION, 'follows, favourites, plays'],
  ['/api/social/play', SESSION, 'follows, favourites, plays'],
  ['/api/notifications', SESSION, 'the account\'s feed'],
  ['/api/notifications/read', SESSION, 'the account\'s feed'],
  ['/api/notifications/prefs', SESSION, 'the account\'s mail preferences'],
  [new RegExp('^/api/release-notify/[a-z0-9-]+$'), SESSION, 'a signed-in account asks to be emailed once when a product is released, or stops asking: a mail preference, easy to undo'],
  ['/api/cosign', SESSION, 'a witness co-signature'],
  ['/api/screening/check', SESSION, 'the account\'s own screening'],
  ['/api/screening/status', SESSION, 'read'],
  ['/api/inventory', SESSION, 'read'],
  ['/api/purchases', SESSION, 'read'],
  ['/api/holdings', SESSION, 'read: the units the caller holds in the holder ledger'],
  ['/api/offering', SESSION, 'read: the offering now testing or open'],
  ['/api/offering/interest', SESSION, 'a non-binding indication of interest; it commits nobody and is easy to withdraw'],
  ['/api/subscriptions', SESSION, 'prepares a subscription; it reserves nothing and commits nobody until it is signed'],
  [new RegExp(`^/api/subscriptions/${ID}/cancel$`), FRESH, 'the subscriber withdraws their own subscription; once funds arrived that turns them into a refund, so the session must be good right now'],
  [new RegExp(`^/api/subscriptions/${ID}/sign$`), FRESH, 'the person signs a subscription for units: the session must be good right now'],
  ['/api/purchases/summary', SESSION, 'read'],
  ['/api/ledger/me', SESSION, 'read'],
  ['/api/stripe/checkout', SESSION, 'opens a Stripe page; the card is the caller\'s'],
  ['/api/stripe/connect/status', SESSION, 'read'],
  ['/api/tickets/balance', SESSION, 'read'],
  ['/api/tickets/checkout', SESSION, 'buys Tickets; the card is the caller\'s'],
  ['/api/sponsor/checkout', SESSION, 'opens a Stripe page; the card is the caller\'s, and it works signed out'],
  ['/api/sponsor/listing', SESSION, 'shows or hides the caller\'s own name on the sponsor list; easy to undo'],
  ['/api/tickets/spend', SESSION, 'answers 410 Gone'],
  ['/api/tickets/history', SESSION, 'read'],
  ['/api/delta/checkout', SESSION, 'opens a Stripe page'],
  ['/api/delta/balance', SESSION, 'read'],
  ['/api/subscriptions/checkout', SESSION, 'opens a Stripe page'],
  [new RegExp(`^/api/simulations/${ID}/play$`), SESSION, 'counts a play'],
  [new RegExp(`^/api/simulations/${ID}/play-ticket$`), SESSION, 'proves the account to its own Player for one listing, as /api/identity/ticket proves it to a host (src/play_pass.mjs)'],
  [new RegExp(`^/api/simulations/${ID}/download$`), SESSION, 'read'],
  [new RegExp(`^/api/simulations/${ID}/cover$`), SESSION, 'an author sets or removes their own listing\'s cover image, and the cover is read here; reversible, and reviewed like a thumbnail change'],
  [new RegExp(`^/api/simulations/${ID}$`), SESSION, 'read'],
  ['/api/simulations', SESSION, 'read'],
  ['/api/simulations/review-quote', SESSION, 'read: what a review of a publish of a given size would cost (src/review_costs.mjs)'],
  ['/api/commerce/account', SESSION, 'read'],
  ['/api/commerce/me/balance', SESSION, 'read'],
  ['/api/commerce/me/pending', SESSION, 'read'],
  ['/api/commerce/me/entitlements', SESSION, 'read'],
  ['/api/commerce/events', SESSION, 'read'],
  ['/api/commerce/events/stream', SESSION, 'read'],
  ['/api/commerce/listen/secret', SESSION, 'read'],
  [/^\/api\/commerce\/events\/evt_[A-Za-z0-9]{24}(\/resend)?$/, SESSION, 'a read, and resending a webhook event the account already has (no value moves)'],
  ['/api/commerce/test_helpers/trigger', SESSION, 'test mode only: fires a test event, no value moves'],
  [/^\/api\/commerce\/players\/[A-Za-z0-9_-]{1,128}$/, SESSION, 'read'],
  [/^\/api\/commerce\/purchases\/pur_[A-Za-z0-9]{24}$/, SESSION, 'read'],
];

/// The class of a route. `explicit` is false when no rule names it and the
/// default (`fresh`) applies.
export function routePolicy(method, path) {
  void method;
  const p = String(path || '');
  for (const [pattern, klass, why] of RULES) {
    if (pattern instanceof RegExp ? pattern.test(p) : pattern.endsWith('*') ? p.startsWith(pattern.slice(0, -1)) : p === pattern) {
      return { class: klass, explicit: true, why, pattern: String(pattern) };
    }
  }
  return { class: FRESH, explicit: false, why: 'not in the table: strict by default', pattern: null };
}
