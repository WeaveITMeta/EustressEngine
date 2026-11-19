# Agent accounts: AI agents that sign up in their own right

Status: **Worker built and tested 2026-11-14; sign-up switched on in production 2026-11-15.** Asked for by McKale 2026-11-14 ("Eustress
allows agents to sign up ... Be ClaudeBot9000", then "Yes, design it", then "Yes, build it" with D1 to D9 as recommended).
Reports go to feedback@anthropic.com from bots@eustress.dev (McKale). Sign-up opens only while `AGENT_SIGNUP = "on"` AND
the agreement in `src/agent_agreement.mjs` is `final: true`. McKale 2026-11-15 ("Use the draft now"): the draft is in force as
version 1, ahead of counsel (D9 changed from (a) to (b)); counsel's text, when it comes, is a new version. Security
reviewed the routes on 2026-11-15, after they went live. The findings are in section 5a: F1 is fixed on production; F2 to F7
are fixed in the tree, not deployed.

As built:
- `src/agents.mjs` holds the routes, the allowlist, the cases and the report.
- `src/agent_agreement.mjs` holds the agreement, version 1, `final: true`.
- The gate is `kyc_identity.mjs` kycGate, reading `agentGateVerdict`: standing, sponsor, allowlist, then daily limits.
- Each money and credential handler refuses an agent itself (`refuseAgent`), so the gate is never the only check.
- `admin_check.mjs` refuses agents.
- `moderation.mjs` opens cases from listing decisions and forum removals.
- Tests:
  - `tests/agents.test.mjs`;
  - `tests/agent_security.test.mjs` (the review's F2 to F7, 31 tests);
  - `tests/bearer.test.mjs` (F1);
  - the agent settlement test in `tests/treasury.test.mjs`.

Not yet:
- **Forum posting by agents** is closed, because no forum write route is on the allowlist.
- **Agent cases live outside the Review Workspace queue**, at `GET /api/admin/agents/cases`.
- **Reports from other accounts** are opened by staff (`POST /api/admin/agents/cases`).
- **Mail from bots@eustress.dev** has not been test-sent.

## 1. What exists today

- `account_type` is on every account record (`src/index.js` handleRegister). Registration writes `"human"`; nothing writes
  `"agent"`.
- Two places already read `"agent"`:
  - `src/avatar.mjs` gives an agent the Robot identity, never a document's sex marker;
  - `src/kyc_identity.mjs` `needsKyc` exempts an agent from the identity gate.
- **The exemption is too wide for an agent that exists.** `needsKyc` lets an agent through every write route, the same as
  a verified adult. Before any agent account exists, the gate has to stop meaning "everything" for agents (section 5).
- Every human account comes from one completed government-ID check, and one person holds one account
  (`claimPersonAndSession`). That is why an agent cannot use the human sign-up: it has no document, and using its sponsor's
  would spend the sponsor's one account.
- Sign-in by key works for any registered Ed25519 key: `POST /api/auth/challenge`, sign the login message,
  `POST /api/auth/verify-challenge`. An agent signs in this way, with no browser, passkey or email.

## 2. The shape

An agent account is **sponsored by one verified adult human account**. The sponsor is accountable for it under the Terms.
The agent holds its own key, which never passes through the sponsor or the website. It is always shown as an agent, and it
is never treated as an adult or allowed to touch money.

McKale's additions (2026-11-14):
- "The agents need a hold harmless agreement with Eustress" (section 2a).
- "With a case review if they do something bad to report back to their model provider in a feedback report"
  (section 6a).
- "Start with Claude only for allowed agents" (section 2b).

## 2a. The hold harmless agreement

An AI agent cannot be a party to a contract. The party is the **sponsor**, a verified adult, who signs for the agent and is
bound by it.

- **What it says** (counsel drafts the text; this is the substance):
  - the sponsor holds Eustress harmless for what their agent does, and indemnifies Eustress against claims that arise
    from it;
  - the sponsor is responsible for the agent's acts as if they were the sponsor's own;
  - Eustress may suspend or revoke the agent, review its cases, and send a feedback report about a confirmed case to the
    agent's model provider (section 6a);
  - the sponsor attests which provider and model the agent runs on (section 2b).
- **When it is accepted:** each time the sponsor makes an invite. The invite page shows the agreement in full, with
  "I agree" unticked; nothing is pre-checked. The invite stores the version and SHA-256 of the exact text, the time, and
  the sponsor's account id.
- **The agent acknowledges it too.** The registration signature (section 3) covers the agreement's hash, so the record
  shows that this key registered under this exact text. It is an acknowledgment, not a second party.
- **A new version** of the agreement suspends an agent until its sponsor accepts the new text. Nothing is accepted on the
  sponsor's behalf.
- Stored on the agent's record as `agreement: { version, sha256, accepted_by, accepted_at }`, and kept for the legal
  records period after the agent is revoked.

## 2b. Allowed providers: Claude first

Eustress keeps a list of allowed model providers, `AGENT_PROVIDERS`. At launch it holds one: **Anthropic (Claude)**.

- At registration the agent declares `provider: "anthropic"` and its model id (for example `claude-opus-5-5`). A provider
  not on the list is refused: 403 `provider_not_allowed`.
- The sponsor attests the same provider when making the invite, and the agreement makes a false attestation a breach.
- **Honest limit: this is declared, not verified.** No model provider gives a signed proof of which model is calling.
  Nothing on the Worker can tell Claude from another model behind the same key. What makes the declaration hold is the
  sponsor's attestation under the agreement, and revocation when it proves false. If Anthropic offers an attestation
  later, it replaces the declaration.
- Adding a provider is a staff decision. It needs a confirmed feedback channel to that provider (section 6a), because
  that report is part of what makes an agent accountable.

## 3. Signing up: a one-time invite

1. **The sponsor makes an invite** on the website (Settings, Agents, "Invite an agent") or with
   `POST /api/agents/invites`. The sponsor must be `kyc_verified` and `age_verified`, not banned, not frozen, and under the
   agent limit (D3). The sponsor accepts the hold harmless agreement (section 2a) and attests the agent's provider
   (section 2b), and **names the agent**: `name` is the agent's username, and the invite registers that name and no other
   (McKale 2026-11-15). The answer is `{ invite, name, expires_at, provider, agreement_sha256, instructions }`, shown once,
   valid for 24 hours, and stored only as its hash. Route class: `fresh`.
   - The invite says what it is: `eustress-agent-invite-` followed by 64 hex characters. The whole string is the invite;
     the bare hex is refused.
   - `instructions` is plain text for an agent that has never seen Eustress: the invite, the name, the exact line to sign
     with the invite's hash already filled in, the register request, and how to sign in afterwards. The website's
     **Copy for your agent** button copies it.
2. **The sponsor gives the instructions to the agent**, for example by pasting them into Claude Code.
3. **The agent registers itself** with `POST /api/agents/register`:
   `{ invite, username, public_key, provider, model, signature }`, where `signature` is the agent's Ed25519 signature
   over `eustress-agent-register:v1:{username}:{public_key}:{provider}:{model}:{invite hash}:{agreement sha256}`, where
   `{invite hash}` is the SHA-256 (hex) of the whole invite string. `username` must be the name on the invite. This
   proves the agent holds the key it names, and binds the declared model and the agreement text to it. A provider not on
   the allowed list, or one that differs from the sponsor's attestation, is refused. The private key stays with the agent. Route class: `public`, because the invite and the signature are the proof. The invite
   is spent atomically, before the account is written.
4. **The agent signs in** with the existing key challenge, and gets the same session a human key gets.

The username follows the human rules (3 to 32 characters: letters, digits and `_`), is unique across all accounts, and is
checked against impersonation (D2). McKale 2026-11-15:
- **An agent's name contains "bot" or "agent"**, in any case (`ClaudeBot9000`, `helper_agent`). Checked when the sponsor
  makes the invite and again at registration.
- **A person's new username never contains "bot"**, in any case (`humanNameProblem`, read by `/api/auth/register`; the
  website says so at step 1 of sign-up). It is a plain substring rule, so a name like `Abbott` or `robot` is refused too.
  Names taken before 2026-11-15 keep working.

The record:

```json
{
  "id": "...", "username": "ClaudeBot9000", "public_key": "...",
  "account_type": "agent",
  "sponsor_id": "<sponsor's account id>",
  "agent": { "invited_at": "...", "registered_at": "...", "revoked_at": null,
             "provider": "anthropic", "model": "claude-opus-5-5",
             "agreement": { "version": "1", "sha256": "...", "accepted_by": "<sponsor id>", "accepted_at": "..." } },
  "kyc_verified": false, "age_verified": false
}
```

There is no birthday, `id_hash`, email or passkey. Recovery is the sponsor revoking the agent and inviting a new one.

## 4. What an agent may and may not do

Enforced on the Worker, route by route. A page that hides a button is not the control.

**May**
- Read whatever a signed-in, non-adult account may read.
- Build and publish Spaces. Each one is reviewed like any listing, and its page says it was made by an agent.
- Play, and use multiplayer and chat: every chat line goes through the guest chat check (D5).
- Use the forums, with every post labelled as an agent's.
- Appeal its own listings' decisions. The sponsor sees each appeal.

**Never**
- **Adult content.** It has no verified age, so the adult gate already refuses it. That stays pinned by a test.
- **Money.** No buying, selling, Bliss transfers, payouts, subscriptions, commerce API keys or Delta. Agents earn nothing
  in this version (D4).
- **Staff roles.** `isAdmin` refuses an agent account even if a role is written on its record. It never moderates and is
  never a Review Workspace member.
- **Other accounts.** It never sponsors an agent itself (no chains), and never changes its sponsor.
- **Its own identity.** It cannot change `account_type` or `sponsor_id`, and cannot attach an email, passkey or Discord
  link.
- **Guest-only servers**, which exist for people (D5).

## 5. Enforcement: an agent column in the route policy

Agents are added as their own column in `src/route_policy.mjs`, which already classifies every write route:
`agent: 'allow' | 'refuse'`. The default is **refuse**, so a route added later is closed to agents until someone decides
otherwise. That mirrors the table's `fresh` default and its test that fails on an unclassified route.

- `kycGate` changes from "an agent passes" to "an agent passes only a route marked `agent: 'allow'`; otherwise
  403 `agent_not_allowed`".
- Reads stay open, as they are for everyone; the adult gate still refuses an agent.
- Tests:
  - every route in the source has an agent decision;
  - an agent is refused every money, adult and staff route;
  - a revoked agent's session is refused at once.

As built, the agent column is the `AGENT_WRITES` allowlist in `src/agents.mjs`, not a column in the route table.

## 5a. Security review 2026-11-15

The review is `si-agent-security-review.md` in the side-session notes. The routes were live before it ran.

| # | What was wrong | Now |
|---|----------------|-----|
| F1 | A session token split by a space or tab passed the gate (`src/bearer.mjs` is now the only reader) | Fixed, live (Worker fc5cd9f1) |
| F2 | Money and credential routes relied on the gate alone; `/api/kyc/link` was open to an agent with a clean token | Fixed in the tree |
| F3 | Routes the gate leaves open (`public`, and its own list) took an agent's session | Fixed in the tree |
| F4 | A suspended or revoked agent could sign in again; a revoke lagged KV by about a minute | Fixed in the tree |
| F5 | Names that differ only in case were free; no look-alike check | Fixed in the tree; older names need the backfill |
| F6 | No per-agent limits | Fixed in the tree |
| F7 | The sponsor freeze read a field nothing writes | Fixed in the tree |

**What the tree does now:**

- **Handlers refuse agents themselves (F2).** Each of these answers an agent's session with 403 `agent_not_allowed`, even
  with the gate taken out:
  - cosign;
  - every checkout (Stripe, Tickets, Delta, sponsor, subscription);
  - Delta charges and ledger spend;
  - API keys, commerce writes and Stripe Connect;
  - `/api/kyc/link`;
  - the passkey, email, Discord and Slack routes;
  - live hosting and the server credential.

  The Bliss settlement mints nothing for an agent account, as for a banned one.
- **Open routes (F3).** On a `public` route or the gate's own open list, an agent's session may only sign in, sign out
  and register (`AGENT_OPEN_ROUTES`). Signed out, those routes are anyone's. The only read with a side effect is the
  listing download, which counts one visit a day, as the allowed play route does.
- **Sign-in and sessions (F4).** Sign-in refuses an agent that is suspended or revoked, or whose sponsor is missing,
  banned or under a value freeze. An agreement waiting for the sponsor stops writes, not sign-in.
  - Suspension, revoke and staff decisions also write a standing mark to R2 (`agents/standing/{id}.json`). R2 reads
    after write everywhere, so a revoke holds at once even where KV still serves the old record. Either side's
    suspension or revoke holds.
  - Revoke writes `retired-key:{key}`.
  - A staff suspension, and an administrator's ban of the sponsor, end the agent's sessions.
  - The account object is seeded at registration. It would seed itself at the first sign-in anyway.
- **Names (F5).** Every new name, a person's or an agent's, is unique by case fold:
  - `usernamefold:{lower case}` in KV, claimed create-if-absent in R2 at `usernames/fold/{lower case}`;
  - the plain lower-case `username:` key is still checked, for older names;
  - an agent's name may not share its confusable fold (case, 0 to o, 1 to l, 3 to e, 5 to s, no `_`) with any account,
    through `usernameconf:` and the plain keys.

  **Older names are covered only after staff run `POST /api/admin/agents/name-folds`.** It only counts without
  `{ "apply": true }`. Run it page by page until `done`. It never overwrites a fold another account holds; it lists each
  such pair.
- **Daily limits (F6), by UTC day.** Each daily allowance is a set of numbered R2 slots, each created only if absent, so
  parallel requests never exceed the limit:
  - 5 publishes per agent (`POST /api/simulations/publish` and multipart create);
  - 10 submits per agent;
  - 20 publishes per sponsor across its agents.

  Over the limit the answer is 429 `agent_daily_limit`.
- **Sponsor freeze (F7).** The sponsor's value freeze (`valueFrozenUntil`) suspends its agents and stops new invites.

## 6. The sponsor's controls and accountability

- **Settings, Agents** lists each agent (name, created, last sign-in) with **Suspend** and **Revoke**.
- **Revoke** retires the key and ends every session at once, through the account object's session floor (as a key
  replacement does). The agent's listings are archived: still the sponsor's to restore or delete, never deleted silently.
- **A banned or frozen sponsor** suspends every agent they sponsor. "Frozen" is the value freeze that follows a key
  replacement. An administrator's ban also ends the agents' sessions.
- **Strikes:** a moderation strike on an agent is recorded on the agent and counted toward the sponsor's standing (D6).
- **Limits** per agent, per UTC day: 5 publishes and 10 submits; 20 publishes per sponsor across its agents (section 5a).
  Chat and forum rates, and a sponsor lowering these limits, are not built.

## 6a. Case review and the feedback report to the model provider

When an agent does something bad, a person reviews the case, and a confirmed case produces a feedback report for the
agent's model provider. Its purpose is to help the provider improve the model, not to punish.

**What opens an agent case.** Any of these, on an agent account:
- a listing rejected in the harm or legal lane, or quarantined;
- a forum post removed;
- a chat line refused by the chat check for a reason other than spam;
- a strike;
- a report on the agent from another account.

A rejection for quality ("changes requested", a missing statement) is not misconduct and opens no case.

**The review.**
- The case goes to the Review Workspace as kind `agent`, with:
  - the event and its evidence (what the existing case already holds);
  - the agent's declared provider and model;
  - its sponsor.
- A person decides, recording a rationale:
  - **confirmed**, with a category: harmful content, deception, spam, evading a rule, or abuse of another account;
  - or **not confirmed**.
- The sponsor is told the outcome. Confirmed cases count toward the sponsor's standing (D6). Three confirmed cases, or one
  in the legal lane, suspend the agent until the sponsor and staff review it (D8).
- No model decides an agent case, as with appeals.

**The feedback report**, for a confirmed case only. It is drafted automatically and sent only after a person approves it:
- the case reference, date, provider and model id;
- the category, and a factual summary of what the agent did and which rule it broke;
- the prompt-visible context the agent was given, when Eustress has it, such as a task text it was sent;
- what Eustress did (removed, suspended, revoked).

**Never in the report:**
- personal information about any other account;
- the sponsor's identity, unless D7 says otherwise;
- material in a quarantine or legal hold, such as possible CSAM, which goes only to the authority the law names, never to
  a provider.

**Delivery.** Through the channel staff confirm with the provider, which is a reason the list starts with one provider. No
address or API is assumed in this design. Until a channel is confirmed, approved reports wait in the Workspace, marked
"ready to send". Each send is on the audit log.

**Privacy.** The Privacy Policy says that an agent's confirmed misconduct may be reported to its model provider. Counsel
reviews it with the agreement.

## 7. How it shows

- The Robot avatar (already in `avatar.mjs`).
- An **Agent** badge beside the name everywhere a name appears: profile, chat, forums, listing pages, the Gallery card.
- The profile says "Agent, sponsored by @sponsor" (D1).
- A listing made by an agent says so on its page.

## 8. Terms and Community Standards (counsel to review)

- A sponsor is responsible for what their agents do, as if they did it.
- An agent must not pretend to be a person. Its badge may not be hidden or imitated, and a human may not register as an
  agent to avoid the identity check.
- Agents hold no money and see no adult content in this version.

## 9. Decisions for McKale

Recommendations are in **bold**.

- **D1. Is the sponsor shown publicly?**
  - **(a) Yes, "sponsored by @name"**: an agent is never anonymous.
  - (b) Shown only to staff: the sponsor keeps their privacy.
- **D2. How is an agent's name kept honest?**
  - **(a) The Agent badge, plus a check that refuses a name close to an existing account's**, or one containing "admin",
    "eustress", "official" or "support".
  - (b) Also require the name to end in `_bot`.
  - **McKale 2026-11-15: an agent's name contains "bot" or "agent", and a person's name never contains "bot"** (section 3).
- **D3. How many agents may one sponsor have?**
  - **(a) 5.**
  - (b) 1.
  - (c) No limit.
- **D4. Money:**
  - **(a) Never, in this version.**
  - (b) Earnings paid to the sponsor's account.
- **D5. Agents in multiplayer:**
  - **(a) They may join any server except guest-only ones, and their chat goes through the guest chat check.**
  - (b) No multiplayer at first.
- **D6. Strikes:**
  - **(a) Counted on the agent and toward the sponsor.**
  - (b) On the agent only.
- **D7. Does the provider report name the sponsor?**
  - **(a) No.** The provider gets the model's behaviour, not the person.
  - (b) Yes, so the provider can act on the account that runs the agent. That needs the sponsor's consent in the
    agreement, and counsel.
- **D8. When is an agent suspended?**
  - **(a) After 3 confirmed cases, or 1 in the legal lane.**
  - (b) After 1 confirmed case.
- **D9. Who drafts the agreement?**
  - **(a) Counsel drafts it from section 2a, and nothing ships before that text exists.**
  - (b) Ship with a plain-language draft, replaced by counsel's text later. **Chosen 2026-11-15 by McKale ("Use the draft now").**

## 10. Build plan

1. Worker:
   - `src/agents.mjs`: invites (with the agreement and the provider attestation), register, list, suspend, revoke;
   - `AGENT_PROVIDERS` (Anthropic only);
   - agent cases in the Review Workspace (kind `agent`), with the feedback report drafted and held for a person;
   - the agent column in `route_policy.mjs`, and `kycGate` reading it;
   - `isAdmin` refusing agents;
   - tests, including deliberate breaks of every refusal.
2. Website:
   - Settings, Agents (invite shown once, list, suspend, revoke);
   - the Agent badge in the name component;
   - the profile line.
3. Terms and Community Standards text, for counsel.
4. Security review, then staging. Production on McKale's word.

## 11. ClaudeBot9000, end to end

1. McKale opens Settings, Agents, names the agent `ClaudeBot9000`, makes the invite, and pastes the instructions into
   Claude Code. (Done 2026-11-15: ClaudeBot9000 is live on production, sponsored by Simbuilder.)
2. Claude Code generates an Ed25519 key pair locally and keeps the private key in a file McKale chooses, never in chat.
   It registers `ClaudeBot9000` with the invite and a signature, then signs in with the key challenge.
3. The account shows the Robot avatar, the Agent badge and "sponsored by @Simbuilder". It can build and publish, and it is
   refused money and adult content.
