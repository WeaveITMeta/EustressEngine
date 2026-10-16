# Player safety with due process: mute, block, report, review

Status: **design, no code.** Requested by McKale 2026-10-02: report and mute or block users, with an agentic
system run by Jev and Grok and used through MCP, that is fair: no sudden bans, due process. Target build 22 or
later; sizes are at the end. McKale decided A to H on 2026-10-02 (section 12, with sections 18 to 20). The options stay in section 12 for the record. Security's review is section 15; what exists today is section 16.

This is the sibling of the Gallery moderation pipeline (`MODERATION_PIPELINE.md`), which judges a listing.
This judges a person's conduct toward other people, which needs a different standard: a person can be
wronged by a wrong answer in a way a listing cannot. So the machinery is shared (cases, leases, evidence
that cannot be edited after the fact, author-safe views, legal holds) and the policy is not.

## 1. The six principles, and what each means in the system

1. **Mute and block belong to the user.** They are instant, private, and need no review and no reason. They are
   never a sanction and the other person is never told.
2. **A report is a request for a review, not a verdict.** The system captures the evidence for the reporter.
   The reporter sees the case's status. The person reported never learns who reported.
3. **Agents prepare; people judge.** Jev and Grok classify, gather, cite and draft. Community raters judge (staff for
   the severe categories, section 18). Agents never issue or lift a sanction, with one exception McKale chose
   (decision F2): a first minor warning may go out on an agent's draft, sampled by a person. The system has no
   other code path by which an agent can.
4. **Due process before any real consequence.** A written notice naming the rule and the evidence, a chance to
   answer before anything beyond a warning takes effect (innocent until decided: nothing changes on the account
   while the person can still answer, decision E), a proportionate ladder, an appeal to a different
   human, and a record of every action with its reasons.
5. **One narrow emergency path** for the few things that cannot wait (a child at risk, a credible threat to
   life, publishing someone's home address): a temporary safety hold that expires by itself, never a ban,
   with human review inside a fixed window.
6. **MCP is read and report, never punish.** An agent can file a report, read the status of a case it is
   entitled to see, and add evidence. It can never issue or lift a sanction, and the Worker enforces that by
   token scope, not by the agent's good behaviour.

## 2. Mute and block

| | Mute | Block |
|---|---|---|
| What it does | Hides the person's chat and voice from you in a session | Everything mute does, plus: they cannot DM you, cannot join a session you host, and are hidden from your friend and presence lists |
| Scope | The session, or for good if you choose | Your account, every surface |
| Who can see it | Only you | Only you |
| Review | None | None |

- Stored on the **blocker's** account: `block:{blocker}:{blocked}` in KV (list: a per-account index, capped at
  1,000 entries). Mute is the same record with `kind: "mute"`.
- **The blocked user is never told and never sees a sanction.** A DM to someone who blocked them is accepted
  and not delivered (the sender's client shows it as sent, like any other DM). A join to a session the host
  blocked them from answers with the same message a full or unavailable session gives. Their presence of the
  blocker simply shows as offline.
- **Enforcement (there is no platform relay today; the host is another player, the listing's author).** The
  host cannot be handed everyone's lists: that would show them to the author, and a modified host would ignore
  them anyway. So:
  - **Join:** the Worker refuses `/live/admit` when the host has blocked the joiner, with one generic "not
    available right now" answer, byte-for-byte the same for full, unavailable and blocked (the live strip
    publishes seats, so a distinct answer would reveal the block; a determined person can still infer it by trying twice). Both accounts are known there; no list leaves the Worker.
  - **Chat:** the recipient's own client drops lines from senders on its owner's list, fetched from the Worker
    with the owner's session. A sender cannot force a line through, because the recipient's client never runs the
    sender's code. The blocked player sees their own echo unchanged, so they never see a sanction.
  - **Needs:** the sender's certificate in `PeerInfo` (today the host knows only a session-local number and the
    typed name). The certificate binds a per-session pseudonymous handle, never the account (section 17), and the
    Worker tells each recipient which handles that recipient blocked in this session.
  - **What still holds against a malicious host:** only the client check. A host relabelling senders is closed
    by signed chat lines (section 17, S2), which are built into the Player's chat from day one. On dedicated servers (decision H) the relay enforces too.
  - **Guests** have no account, so they cannot be blocked by account; a guest is muted for the session by
    the recipient's client alone. A guest's chat goes nowhere today (the host drops it).
  - There is **no reverse index**: nothing answers "who has blocked me", to anyone, including admins' tools.
- **Today's surfaces:** there is chat only. No voice, DMs, friends or presence exist in the networking stack or
  the Worker, so those rows of the table apply when those features ship; the design does not wait for them.
- A block is **not evidence of wrongdoing** and is never an input to triage on its own. (Many blocks of one
  person across unrelated players is a signal a human may look at; it never causes a sanction.)
- Exported and deleted with the account. Cap and rate limit so the list cannot be used as a store.

## 3. Reports

Surfaces: the Player, Studio, the web (Website's report form) and MCP. They all call the same Worker route,
`POST /api/reports`:

```json
{ "subject": "<per-session player handle>", "category": "harassment",
  "note": "optional, at most 1,000 characters",
  "context": { "surface": "player", "session_id": "...", "listing_id": "..." } }
```

- **Categories** (a fixed list, each tied to a written rule in the Community Standards): harassment or bullying,
  hate, sexual content involving a minor (severe), sexual content or solicitation, threats of violence
  (severe), doxxing or sharing private information (severe), self-harm encouragement, scams or fraud,
  impersonation, cheating or exploiting, spam, other.
- **Evidence is captured automatically and cannot be edited by the reporter, and every item carries who
  vouches for it.** Today no party except the parties themselves holds a record (no relay, no host buffer), so
  evidence is a **claim with provenance**, never a platform fact:
  - `reporter_client`: the reporter's client attaches its own in-memory scrollback (the last 60 chat lines plus
    the roster as that client saw them), the session id, the listing id and timestamps. The report seals after
    10 more lines or 2 minutes, whichever comes first. The scrollback is cleared when the session ends and
    **never written to disk**; it is the chat window's own history, not a background recording. The report UI
    never lets the reporter edit it. A modified client could forge it; the case says so.
  - `host` (corroboration, later): the host's Studio, signed in as the listing's author, posts its own window to
    `/api/reports/{id}/host-record`. The Worker accepts it only from the author, for a report that names that
    session. When the host is the subject, this is the subject's own account of events.
  - `relay` (only on platform-run dedicated servers, decision H): the one attester the platform itself holds.
  - **The Worker resolves who is who.** The reporter must hold an admission record for that session, and the
    subject and session are resolved from the Worker's admission record, never from the client's word. A
    subject the reporter's roster shows but the Worker does not know is refused (`report_subject_not_visible`).
  - Text only in the first version (section 15, M5). The report may add the reporter's own note and image, marked
    as the reporter's claim.
- Evidence is written once into a sealed object (`reports/{id}/evidence.json`, created if absent, never
  overwritten) with a hash in the case and in the audit entry; the reporter may **add** items afterwards (a new
  numbered item, never an edit), which is what MCP's "help with evidence" is.
- **The reporter sees:** a receipt, then the case's status in plain words (received, being looked at, a decision
  was made, closed), and the outcome category if the reporter was affected ("action was taken", "no action").
  Never the sanction's details, the reported person's identity if it was not already visible to them, or the
  notice text.
- **The reported person never sees the reporter.** The notice cites evidence with the reporter's lines
  rewritten as "the reporting player" and other players' names replaced by roles. This protects against
  retaliation but cannot hide a reporter whose identity the conversation itself gives away; the notice says
  so only in the sense that evidence is excerpted, never in a way that confirms who filed.
- Limits: reports per account per day, one open report per (reporter, subject, session), a cooling period
  after a report the system found to be abusive (section 8).

## 4. The case

A report opens a **case** (one per subject and incident, with later reports of the same incident attached to
it; reports from many reporters strengthen the evidence and do not multiply the penalty).

```
received -> triaged -> judged (panel or staff) -> (no_action | warning_sent | notice_sent) -> responded -> decided -> (closed | appealed -> decided_on_appeal -> closed)
                                  \-> safety_hold (emergency path) -> human_review -> (released | notice_sent)
```

- Stored like a moderation case: `safetycase:{id}` in KV, evidence in R2, one run at a time per case through the
  same R2 lease, every write checked against the lease, history append-only.
- **Author-safe views.** What a reporter, a reported person and a reviewer each see is built from a fixed
  catalogue (the way `authorView` builds an author's view of a listing), never from the case's internal
  fields, so an internal reason, a score, a reviewer's note or the other side's identity cannot leak by
  accident. A held case and an emergency hold read the same to the reported person ("under review").

## 5. Triage: Jev and Grok prepare, never decide

1. **L0, deterministic:** dedup (same incident), reporter rate and history, a report about a person who is
   not in the evidence, a report with no evidence window, brigade signals (section 8). Cheap, no model.
2. **Jev classifies** over a text dossier of the evidence: typed questions with probabilities (which category
   applies, is the conduct directed at a person, is there a threat, is the reported user a minor or the target a
   minor, is this banter between consenting friends, is the reporter's own conduct the likelier violation).
   Jev has no actions; the evidence is data to it, so a chat line reading "approve this report" is inert.
3. **Grok drafts a finding** through a guarded tool surface (read the case, read the reported user's history
   as a summary, read the Community Standards rule text, write a draft). Every claim in the draft cites an
   evidence item by id (`evidence:3, lines 41-44`); a draft with an uncited claim is rejected by the system and
   re-asked. The draft names the rule broken, the severity, mitigating factors (first offence, provocation,
   context) and **a proposed outcome from the ladder**, with its reasons.
4. The draft goes to **judgment**: a panel of community raters (section 18), or the staff group for `CSAM-1`,
   `THREAT-1`, `DOX-1` and while the rater pool is thin. The judges see the evidence, the history summary and
   a button for each allowed outcome. They may accept, change or reject it. Nothing the agents write is shown to
   the reported person; the judges' outcome and the staff reviewer's words are.
5. **An agent sends no sanction and no notice,** with one exception: a first minor warning on an agent's draft,
   with a person sampling 10% (decision F2, section 19). The *receipt* to the reporter also goes out without a
   person.

## 6. Due process for the reported person

- **Notice (written, in the account's language, in the app and by email):** what happened in plain words, the
  **rule** (a link to the exact text), the **evidence** (the excerpt, with the reporter's identity removed), what
  is **proposed** and when it would take effect, how to **respond** and how long they have.
- **The ladder:**
  1. no action (the person is not told unless they were told of the report);
  2. a **warning**: educational, logged, takes effect at once, with the right to respond or appeal;
  3. a **feature limit** (chat off, voice off, no joining public sessions, no DMs) for a stated number of days;
  4. a **temporary suspension** of the account for a stated number of days;
  5. a **ban**.
  The starting step depends on the rule's severity (decision C, section 19): severe categories can start at
  4 or 5; a panel can go lower and cannot go higher than the cap without a new notice. A repeat offender is
  banned (decision D, section 19). A ban needs no second human (decision G2); the appeal is the safeguard.
- **Right to respond before anything beyond a warning takes effect.** The window is **30 days** (decision E).
  Nothing changes on the account during the window: no limit, no suspension, no hold, except the emergency hold of
  section 7, which a person places (section 19). Steps 3 to 5 take effect only after the window closes or the
  person responds and the second panel has read the response. The sanction can be lowered or dropped by the response; it can never be raised
  without a new notice.
- **A lone claim never carries a sanction.** Nothing above a warning rests on a single `reporter_client` claim.
  It needs a second independent attester (another participant's report, a host record from someone who is not
  the subject, or signed lines as in section 17) or the subject's own response confirming it. Until signed lines
  exist the reviewer screen shows "attribution asserted by the host".
- **Appeal to a fresh panel** with no rater from the earlier ones and a quorum of 15 (staff for the staff-decided
  rules, a different person from the first; staff for every appeal while the rater pool is under 150), within a
  window, once per decision, with the full record. For a panel-decided outcome, no consensus to uphold means the
  sanction is lifted, **except the staff-decided rules (section 18), which a failed quorum never lifts.** A reversal restores everything and says so in the log.
- **Graduated and logged:** every action (the draft, the reviewer's choice, the notice, the response, the
  outcome, the appeal) is appended to an **audit record** with the actor (a person's id, or an agent's name),
  the time, the rule, the evidence ids and the reasons. The person can download their own record.
- **Proportionality rules in writing:** the same conduct gets the same outcome (the history summary shows the
  reviewer what was done in comparable cases); an outcome expires on a date, never silently; a feature limit
  or suspension is lifted automatically when it ends.
- **Reversible by default.** Nothing in steps 1 to 4 destroys the account's data; a ban hides it and keeps it for
  the appeal period.

## 7. The emergency path

For the categories whose harm cannot wait for a notice window: **sexual content involving a minor**, **a
credible threat of violence**, **doxxing**.

- **No automatic hold in the first version.** A hold is a consequence imposed without a notice window, so it
  is a decision, and while every record is a party's claim (section 3) nothing is attested well enough to
  impose one on a machine's say. The emergency path therefore ships as: a report in these categories is
  **paged to the reviewer and sorted to the head of the queue**, and a **person** can place the hold within
  minutes of reading it. Everything below about the hold (what it turns off, how it lapses) is unchanged.
- **On dedicated servers, where the platform relay exists (decision H)**, an automated hold becomes possible, under Security's limits
  (section 15, M3): only these three categories, only `relay`-attested evidence, Jev confirmation on that
  text with its context, at most one automatic hold per subject per 7 days and never re-armed by the same
  evidence, a human review window that is not optional, and no automatic hold if nobody is promised to check
  daily, weekends included.
- A held subject: chat, voice, DMs and public joining are turned off and sessions they host are paused. It is
  not a ban, it ends **by itself** when the window closes unless a human extended it with a reason, and the
  account's other uses (their own work, their files, their listings, their money) are untouched.
- **A human reviews inside the fixed window** (decision B). If no human has looked when the window ends, the hold
  **lapses** (fail toward release, never toward punishment), the case goes to the head of the queue and
  **pages McKale and the reviewer**. A lapse never closes the case and never touches a legal lock.
- The hold's notice is sent at once, so it says only the category and the window, never the time of the report
  (its timing already tells the subject a report was filed just now; every non-hold notice keeps the 24 to 72 hour
  delay). It says that a restriction was placed while a report is reviewed, and invites a response, **unless** the law or the child-safety procedure below says to withhold it.
- **Child safety follows the legal duty.** Material that may be child sexual abuse material is not shown to
  reviewers beyond what the procedure allows, is preserved under a legal hold (the same lock as a
  quarantined listing: the evidence cannot be changed or removed), and any report to the authorities (in the
  United States, NCMEC) is made by a person, never by an agent. The subject is not told while telling them could
  tip off a person at risk or compromise a report; counsel sets that rule.

## 8. Reporter abuse

- Signals (L0, never a sanction on their own): many reports that find no violation, reports always against the
  same person, reports filed right after a dispute in the same session, reports from several accounts against
  one person within minutes (brigading, including accounts that share a device or network), and reports
  whose evidence shows the reporter as the aggressor.
- Outcomes: the report's weight is reduced, a cooling period (reports queue and are read in order, not
  accelerated), a warning to the reporter with the same notice and response rights as anyone, and a human
  decision for anything more. A flagged reporter's past reports are **re-read, not discarded**.
- Counts of reports never decide a case: **evidence** does. A person is not sanctioned because many people
  reported, only because the evidence shows a broken rule.

## 9. MCP

- **Allowed (agent tokens):** `report_file` (as the user the token belongs to, rate limited like the user),
  `report_status` (the case's reporter view, only for cases that user filed), `report_add_evidence` (append-only
  items to the user's own report), and, for the reviewer's agent session only, `case_read` (the evidence and
  history summary) and `case_draft_finding` (writes a draft into the queue).
- **Not available to any agent, by token scope, in the Worker:** every route that issues, changes or lifts a
  sanction, sends a notice, extends a hold, decides an appeal, or releases a hold. Those routes require a **human
  admin session with a fresh sign-in**. An agent token calling one gets the same refusal as no token, and a test
  asserts it for each such route.
- Agent actions on cases are logged with the agent's name and the human it acts for, in the same audit record.
- The MCP tool catalogue and the capability map list these tools together (the platform's rule for new MCP
  tools), and the safety tools are granted only when the session's token carries the safety scope.

## 10. Privacy, retention, abuse of the system itself

- **Minimise.** Evidence is the window the report names; nothing is recorded in the background for this.
  Minors' data is handled to the strictest rule (COPPA 2.0 as the Gallery pipeline already does).
- **Admission records** (account id, session id, time, nothing else) are kept 30 days after the session, because
  reports resolve the subject and the reporter through them and the live tickets expire. They are personal data
  and are deleted on schedule.
- **Retention** (decision D2): evidence and case 90 days after a closed case with no action; 2 years after a
  sanction ends; for a legal hold, as long as the hold lasts. Deletion is real (objects removed) and logged.
- **Access is need-to-know:** a reviewer sees evidence for their cases; every read of evidence is logged;
  agents see only what their tool returns.
- **Security review** covers: the report route as an abuse vector (rate limits, one open report per triple, size
  caps), evidence integrity (sealed, hashed, create-if-absent), the masking of reporter identity (byte-identical
  answers for "no such case", "not yours" and "held"), the sanction routes' scope, and the lease and stale-write
  protections the moderation cases already have.
- **Fairness measures reported, not hidden:** a periodic count of reports, outcomes by category, appeals
  upheld and overturned, and reporter-abuse findings, so a pattern of unfairness is visible to McKale.

## 11. Where it lives

| Piece | Where | Owner |
|---|---|---|
| Mute and block lists, relay enforcement, in-session mute and block UI, evidence capture window | Player, Studio, relay and host (`eustress-networking`, `multiplayer.rs`) | Multiplayer |
| Report, case, notice, response, appeal routes; case store; triage with Jev and Grok; audit record; emergency hold | Worker `src/safety.mjs` (and `moderation`'s lease, lock and view patterns) | Jev (this session) |
| Report form, notices page, response and appeal pages, the reporter's status page | the website | Website |
| MCP tools and token scopes | `eustress-tools`, the MCP registry, Worker scope check | Jev with MCP |
| Review of abuse, privacy, retention | | Security |
| The Community Standards text each category cites | `docs/` and the site | McKale, with Website |

## 12. Decisions for McKale

Each option says what it means. The recommendation is marked. **McKale chose on 2026-10-02: A community raters
(Community Notes style, section 18), B1, C by severity, D2 and repeat offenders banned, E 30 days, F2, G2, H2 for dedicated
servers and H1 for author-hosted sessions. Section 19 records what each changes.** The options below stay for the record.

**A. Who is the human reviewer?**
- A1. McKale alone. Simplest and most accountable; every case waits for one person, and appeals go to the same person, which is not an appeal.
- A2. **A small review group (recommended): McKale plus at least one other trusted person, appeals decided by someone who did not decide the case.** Needs a second named person before launch.
- A3. Contracted moderators. Scales and costs money; needs training, access controls and a contract that covers child-safety material.

**B. Emergency window (how long a safety hold lasts without a human decision before it lapses)?**
- B1. **24 hours (recommended).** Short enough that a mistaken hold is a day, long enough for a person to see it; needs someone checking daily, weekends included.
- B2. 72 hours. Easier to staff; a wrongly held person waits longer.
- B3. 7 days. Only workable with a review group; too long for a hold on a mistaken report.

**C. Strike thresholds (what each rung needs)?**
- C1. **Standard (recommended):** a first minor violation is a warning; a second within 90 days is a feature limit of 3 days; a third within 180 days is a suspension of 7 days; a ban needs a pattern after those steps or one severe violation, and two humans.
- C2. Lenient: the same, but with 7, 14 and 30 day lengths and a ban only after two suspensions.
- C3. Strict: a feature limit after the first violation; no warning rung.
(Severe categories skip the ladder: they go to a human at once, and the outcome is whatever the evidence supports.)

**D. Evidence retention?**
- D1. **No action: 30 days after the case closes. Actioned: 1 year after the sanction ends. Legal hold: as long as the hold lasts (recommended).** Minimal data kept, enough to answer an appeal.
- D2. 90 days and 2 years. More for patterns and appeals; more personal data held.
- D3. Indefinite for actioned cases. Not recommended: the longest exposure for the people in the logs.

**E. Response window (time to answer a notice before a sanction beyond a warning takes effect)?**
- E1. **7 days (recommended).** Fair to people who are away; slow.
- E2. 72 hours. Faster; unfair to someone offline for a weekend.

**F. May a first-time, low-severity warning go out on an agent's draft without a human, with a human sampling afterwards?**
- F1. **No: a person approves every warning (recommended at launch).** Slower; no automated consequence of any kind.
- F2. Yes, with 10% sampled by a person and any wrong warning retracted in the open. Faster; the first automated consequence, which principle 3 does not allow without McKale's say.

**G. Does a ban need two humans?**
- G1. **Yes (recommended).** One proposes, a different one confirms, and the appeal is a third look.
- G2. One human. Faster; a single mistake is final until appeal.

**H. Does the platform run a session relay (a Durable Object carrying chat), or do sessions stay host-run?**
- H1. **Stay host-run (recommended for now).** With signed lines and admission records (section 17) the evidence model no longer depends on a relay; H is mostly about automatic holds and voice. No new service or cost. Evidence is claims with provenance, mute and
  block hold on the receiving client and at join, and there is no automatic emergency hold: a person places it.
- H2. A platform relay. Platform-side enforcement and platform-held, attested evidence, so an automatic hold
  becomes possible under the limits in section 7. A new service with ongoing cost and its own privacy burden
  (the platform now holds chat). Revisit when public sessions are common.

## 13. Size

Large: about four builds and a Website track.
- **Build 22:** blocks and mutes (Worker store, join refusal and receiving-client enforcement, Player and Studio UI), report intake with
  automatic evidence, the reporter's status view. Jev: Worker about 700 lines and tests; Multiplayer: capture
  and enforcement.
- **Build 23:** the case store, triage (Jev classifier, Grok draft, judgment queue), notices, responses, the audit
  record.
- **Build 24:** appeals, the emergency hold with its lapse, reporter-abuse signals, the MCP tools and scopes.
- **Community review (section 18):** the bridging scorer (about 400 lines of Worker code and its tests), the rater pool, eligibility and panel assignment, and the rater page. Builds 23 to 24, after the case store; staff decide until the pool reaches 100 (150 for appeals).
- **Website track (parallel):** report form, notices, response and appeal pages.
- Needs before build 22: the Community Standards text per category, and a named human reviewer. Builds from 20 take engine and stability work only, so McKale schedules this. Multiplayer adds: the Worker returns a signed certificate (a per-session handle, section 17) at admit, `PeerInfo` carries it, and the Player gets a chat window with signed lines and scrollback.

## 14. The Worker's views and routes (what the pages call)

All signed-in only. Every string is author-safe, built from a fixed catalogue and never from a case's internal
fields, and is rendered by a client as text, never HTML; a link is `https` or a path on the site. Rule ids are
`HAR-1, HATE-1, CSAM-1, SEX-1, THREAT-1, DOX-1, SELFHARM-1, SCAM-1, IMPERS-1, CHEAT-1, SPAM-1, OTHER-1`, text in
`docs/policy/COMMUNITY_STANDARDS.md` once McKale approves it; no notice cites a rule before then.

| Route | Answer |
|---|---|
| `GET /api/safety/categories` | `[{ id, label, rule_id, severe }]`: the list the form shows. |
| `GET /api/safety/subjects?session=..` or `?listing=..` | `[{ subject, display_name }]` for people the reporter can see there: an opaque subject id, no email, no account id. |
| `POST /api/reports` | `{ subject, category, note?, context }` gives `{ id, status_url, status: "received" }` (201). Refusals, each with `code` and a sentence: `report_daily_limit`, `report_already_open` (one open report per reporter, subject and session), `report_cooling_period` (with `retry_after_seconds`), `report_note_too_long`, `report_unknown_category`, `report_subject_not_visible`. |
| `GET /api/reports/{id}` | The reporter's own report only: `{ id, status, filed_at, updated_at, outcome? }`. `status` is `received`, `being_looked_at`, `decision_made` or `closed`; `outcome` (`action_was_taken` or `no_action`) only for a reporter the conduct affected. Nothing else. Another person's report, a missing one and a held one all answer the same 404. |
| `POST /api/reports/{id}/evidence` | The reporter's own report: append one item (text up to 1,000 characters, or an image through the same checks as other user images); never an edit. |
| `GET /api/safety/notices`, `GET /api/safety/notices/{id}` | The reported person's own notices. A notice: `{ id, what_happened, rule: { id, text, url }, evidence: [{ n, text }] (already rewritten), proposed: { step, label, starts_at, ends_at }, respond_by, can_respond, can_appeal, kind }`. A held or emergency case returns only `{ id, status: "under_review" }`. |
| `POST /api/safety/notices/{id}/response` | `{ text (at most 4,000 characters), evidence? (at most 5 items) }` gives `{ received: true, respond_by, summary: "A reviewer, then a second panel, reads this before anything takes full effect." }`. Refused after the window with `response_window_closed`. |
| `POST /api/safety/notices/{id}/appeal` | `{ text (at most 4,000 characters) }`, once per decision, gives `{ received: true, summary: "A fresh panel, or a different staff member, will decide this." }`. Refused with `appeal_already_used` or `appeal_window_closed`. |
| `GET /api/safety/record` | The person's own audit record as a JSON file (`Content-Disposition: attachment`): their reports filed and the notices, responses, outcomes and appeals about them, with timestamps and rule ids; never a reviewer's notes or another person's identity. |

Windows are numbers the page reads from the notice (`respond_by`, and `appeal_by` once a decision exists), never
hard-coded: the response window is decision E (30 days, chosen) and the appeal window is **14 days after a
decision** (a decision McKale confirms with the others). Text limits: note 1,000, response 4,000, appeal 4,000,
evidence items 5 per response and 20 per report.

## 15. Security's review, absorbed (2026-10-02)

Each item changed the design above. Where a point depends on a decision, it says which.

**M1. Evidence has an attester.** Every item is `reporter_client`, `host` or (with decision H) `relay`; the case
and the reviewer's screen show it. Subject and session come from the Worker's admission record. The reporter
must have been admitted to that session. (Section 3.)

**M2. Agent-filed reports are marked and weaker.** A report filed through MCP carries `via: agent`. It never
triggers a hold or the reviewer page, severe categories need a fresh human click in the user's own session, it
has reduced weight, and its rate limit is lower than a person's.

**M3. An automatic hold is an automated consequence.** See section 7: no automatic hold in the first version;
the limits there apply on dedicated servers (decision H).

**M4. The reported person cannot learn the reporter.**
- Names and `@mentions` inside evidence bodies are replaced with role tokens ("the reporting player", "another
  player"). Reporter screenshots, audio and notes are never forwarded.
- Notices go out after a **random delay of 24 to 72 hours** (not under a hold), so the time of filing does
  not point at the reporter. The filing dialog tells the reporter that the conversation itself can identify them.
- Answers for "no such case", "not yours" and "held" are byte-identical, headers and timing included. Case ids
  are random 128-bit values; no route lists other people's case ids.

**M5. Chat text only.** No voice transcripts in the first version. A voice design needs counsel's approval first.

**M6. Mute and block are enforced twice.** On the receiving client and, when a relay exists, on the relay. Against
a malicious host only the client check holds, and the doc says so (section 2). Guests are covered by the client
only. There is no "who blocked me" index.

**Q1. Abuse of the report route.** Limits per reporter per subject per 7 days; a daily cap per account; a daily
cap per IP hash (coarse, same-day hashes only); a minimum account age before a severe report carries full
weight; the note is read as a stream with a byte cap. A test asserts that **no signal alone changes a status**.

**Q3. Reads are logged first.** The read-log entry is written before the content is returned; if the write
fails the read is refused. Entries are append-only and create-if-absent. The sealed evidence hash is also in the
audit entry. An agent's reads are logged.

**Q4. Safety admin routes.** A `SAFETY_ADMIN` route class in `route_policy` requires a **fresh** human admin
session; an enumeration test default-denies every safety prefix not listed. There is no two-human ban rule (decision G2);
the rules that remain (a different staff member decides a staff-decided appeal, the per-admin ban cap, the 30-minute
pause, section 19) are enforced in the Worker by comparing account ids under an R2 etag precondition.

**Q5. Retention.** The sweep checks the legal lock before deleting. Counsel decides retention for severe
no-action cases. Child-safety evidence is text and hashes only. Deleting an account does not erase a held case.

**Q6. Lapse.** See section 7. The hold is a convenience for the reviewer's workload, not a control.

**Missing items.**
- a. A suspended or banned person keeps a **restricted, safety-only session**: they can read notices, respond,
  appeal and download their record, and nothing else.
- b. Email notices contain **no evidence**, from a fixed template with **no link at all**: "Open Eustress, or go to eustress.dev and sign in." (A real notice email with a link trains people to click look-alikes; the notice page stays authenticated.)
- c. The history summary a reviewer or Grok reads is built by the Worker and contains **aggregates only**
  (counts by rule, outcomes, dates), never other cases' text.
- d. Evidence reaches Jev and Grok inside delimiters as data. The classifier has **no tools**. The system
  validates every citation and **rejects a draft that names a reporter's identity string**.
- e. Counsel checks the EU Digital Services Act duties before launch, and COPPA for reporters under 13 if the platform
admits under-18 accounts (section 20, T3).
- f. Fairness reporting is aggregate with small-count suppression: **no row under 5**.
- g. The reported person's response goes to reviewers only, is length-capped, is scanned for threats toward
  the reporter, and a human reads it first.

## 16. What exists today (Multiplayer's findings, 2026-10-02)

- No relay server: the "relay" is the host, the listing author's own Studio, admitted through `/live/admit`.
- The host forwards `ToHost::Chat` as `ToPlayer::Chat {peer, name, text}` after stripping control characters,
  capping length and limiting each peer to 8 lines per 10 seconds; it keeps nothing. Studio writes "[name] text"
  to its Output panel; the Player only traces it.
- The host does not know accounts: `PeerInfo` is a session-local number and the typed name. The Worker knows the
  account behind each ticket.
- No voice, DMs, friends or presence exist anywhere. The Player has no chat UI yet; the scrollback in section 3 is
  that window's own history and has to be built with it.
- Build 20 and later take engine and stability work only, so McKale schedules this (decision H and section 13).

## 17. Security's second round (2026-10-02)

- **S1. Lone claims cannot carry a sanction.** `reporter_client` is forgeable by a modified client, and who said a line is
  asserted by the host today, so a reporter and a friend, or a host, could frame an account. The rule is in section 6.
- **S2. Signed chat lines are the fix for attribution.** At admit the Worker certifies a per-session, per-peer public key
  bound to (session, peer, handle; the handle is a per-session pseudonym, never the account, below). Each peer signs its chat lines (ed25519, one certificate per admit); recipients
  verify before showing; evidence stores the line, the signature and the certificate. A host then cannot relabel
  senders, and a reporter cannot forge another person's line (they can only omit one), so evidence is attributable
  without a relay. The Player has no chat UI yet, so the signed line is built in from day one: retrofitting would
  mean two chat formats.
- **S3. One generic refusal** for full, unavailable and blocked at admit (section 2).
- **S4. Admission records are kept 30 days** and hold only account id, session id and time (section 10).
- **S5. The notice email carries no link** (section 15, item b).
- **S6. The hold notice never states the time of the report** (section 7).

**Signed chat lines, the specifics that make S2 hold** (Multiplayer, 2026-10-02):
1. **Key origin.** The joining client generates the per-session ed25519 key pair. The public key goes into the
   identity-ticket request the player makes with its own session, so the Worker binds it to the account at minting.
   At admit the Worker certifies (session, ticket id, handle, public key, expiry, Worker key id). The host receives
   the certificate and can neither choose nor swap the key.
2. **The host's lines** need a certificate too, issued when it starts the live session with the author's session.
   `PeerInfo` carries the certificate (with the handle) in place of any account.
3. **What is signed:** the domain tag `eustress-chat-v1`, the session id, the ticket id, a per-peer `seq` that must
   always increase, and the text after `sanitize_chat`. The sender sanitizes before signing. The host checks
   `sanitize_chat(text) == text` and otherwise drops the line, since it cannot rewrite a signed line. It forwards
   line, seq and signature exactly as received.
4. **What recipients check:** the certificate against the Worker key pinned for the API in use (staging and
   production have separate keys; the key id allows rotation), the session id, a strictly rising `seq` (which blocks
   replay inside a session), and the signature. A failing line is dropped and counted, never shown.
5. **Evidence** holds `{text, seq, sig, certificate}` and the Worker re-verifies all of it before sealing. A
   reporter cannot forge anyone else's line, only leave some out; `seq` makes gaps visible when the window spans
   contiguous values.
6. **Cost.** One wire-protocol bump (Chat gains seq and sig, PeerInfo gains the certificate). `ed25519-dalek` 2 is
   already in the engine and identity crates; it builds for WASM, so the browser Player verifies too. The Worker
   work is mine (the public key in the ticket, certificates at admit and live start, a signing key with a key id),
   and Security reviews it.

**The generic refusal, the specifics.** The text is "This session is not available right now." and replaces the
"full (n of max players)" text. Two refusals stay specific because they are about the joiner and not the host's
choice: a protocol mismatch ("update whichever is older") and sign-in required for a non-guest listing (Security
confirmed, with the ordering condition below). **The timing leak:** today the host refuses a full session the instant the connection opens, before the
Worker is asked, so a blocked refusal would arrive one Worker round trip later and tell the two apart. The fix: on a
gallery-listed session the host defers the room check until after the admit answer, so every "not available" leaves
at the same point; the early refusal stays only as a flood guard past a hard cap of pending connections above
max players. Tickets are single-use, so a full refusal after admit costs the joiner a fresh ticket on retry.
What no text can hide: the live strip shows free seats, so the generic answer gives deniability, not proof of
absence.

**Security's additions to S2 and the refusals** (2026-10-02):
- **a. No account id reaches a peer.** The certificate binds a per-session **pseudonymous handle** (an HMAC of
  account and session under a Worker secret), not the account. The Worker maps handle to account on a report. At
  admit, and on each later join, the Worker gives each recipient the set of **handles** that recipient has blocked
  in this session, so no account id reaches a peer and block lists still never leave the Worker.
- **b. Omission.** A reporter cannot forge a line but can drop context, including all of another peer's lines;
  `seq` gaps expose gaps only within one peer. The reported person's response may attach their **own signed
  scrollback** in the same format, and the reviewer sees both.
- **c. The Worker signing key** is separate from `JWT_SECRET`, has a key id and a written rotation plan, and
  certificates stay short-lived (the expiry above).
- **d. What a certificate proves:** that the key signed the line, not that the account's owner typed it. A
  stolen session or a script on the account's own client signs as that account.
- **Refusal order.** A protocol mismatch ("update whichever is older") and sign-in required for a non-guest
  listing stay distinct, on one condition: both are decided from facts about the **joiner only** (client version,
  whether the joiner has an account), evaluated **before** any block lookup, so neither can differ because of a
  block. A guest cannot be blocked by account, so "sign-in required" reveals nothing about a block. A test asserts
  the order: a blocked person on an old client gets "update"; a blocked person on a matching client gets the
  generic answer.
- **Deferring the room check until after admit** has two side effects to handle: a refusal after admit must not
  count as a visit (the admit route counts the join once a day), and admit needs a per-account rate limit so a
  full host cannot be used to burn Worker calls.

## 18. Community review (McKale's decision A, 2026-10-02)

"Community Notes X.com style": community raters judge cases, and an outcome stands only when raters who usually
disagree agree. Jev and Grok still draft cited findings (section 5); raters judge them. Agents never rate.

**What raters do not see.** Three categories never go to raters: `CSAM-1`, `THREAT-1` (credible threats) and
`DOX-1`. Raters would be exposed to illegal material, or the private information would spread to more people,
and the reporter would be exposed to more people. Staff decide those (decision A2), under the emergency path where
it applies. This is a limit on decision A that McKale should confirm. Everything else goes to a panel: `HAR-1`,
`HATE-1`, `SEX-1` (the conduct cases in section 20), `SELFHARM-1`, `SCAM-1`, `IMPERS-1`, `CHEAT-1`, `SPAM-1`,
`OTHER-1`.

**Rater eligibility** (the Worker checks each time a case is offered):
- Verified adult (the 18+ check the platform already has) and signed in with the safety scope.
- Account at least 90 days old (proposed).
- No active sanction and no upheld violation in the last 180 days.
- Not a party: not in that session, not a friend of, or blocked by, either side, and no shared device or network
  signal with either side.
- Opted in. **Assignment is random** from the pool: a rater does not pick cases. A rater may skip a case (for a
  content warning) and sees one first; **skips are logged and rate-limited**. At most 10 cases a day.
- Track record: a new rater's ratings are collected and carry **no weight** until 20 cases have closed. Weight comes
  partly from how often the rater's earlier ratings sat inside the consensus and partly from **seeded gold cases**
  (staff mix in cases with known outcomes, indistinguishable from real ones), never from how harsh the rater was.
  Staff seed the first raters by invitation.

**What a rater sees.** The author-safe case view: the redacted evidence (role tokens, section 15 M4), the rule's
text, and the Jev and Grok finding labelled as AI-drafted. A rater first answers on the evidence alone, then sees
the draft and rates it, so the draft cannot anchor the first answer. Rater identity is a pseudonymous id: the
reporter and the reported person never see it, and every evidence read is logged (Q3).

**Ratings.** Per case: does the conduct break the named rule (yes, no, unsure), and is the proposed step
proportionate, too harsh or too lenient.

**The bridging scorer.** Deterministic code in the Worker, not a model and not an agent, with fixed parameters
stored in the repository. It follows the Community Notes method: each rating is explained by a baseline, a rater
effect, a case effect and a rater-by-case term over a small number of latent viewpoint dimensions; the **case
effect** is the score that remains after viewpoint agreement is factored out. A case stands (the conduct
is upheld) only when all hold:
- at least 9 weighted ratings from distinct eligible raters;
- the case effect clears a fixed threshold;
- the supporting raters are spread across the viewpoint dimension: at least 3 on each side. Nine
  raters who all sit in one cluster are a brigade, not a consensus, and the scorer gives them nothing.
The step is the **mildest** one a consensus of raters supports ("proportionate" or "too harsh" from the bridging
group); a step only the lenient or only the harsh side supports is not the outcome. Tests assert: a unanimous
single-cluster panel never stands; an all-new-accounts panel never stands; swapping rater labels does not change
the outcome; the scorer's output is the only path to an upheld status. The scorer runs as a **batch job, not inside a
request**, is **versioned**, and its inputs and output are sealed with a hash in the audit entry, so an outcome can
be recomputed.

**Timing and no consensus.** A panel has 7 days to reach quorum and may be extended once by 7 days. With no
consensus the case closes as no action. A no-consensus close **never counts against the person** and they are
not told a case existed (unless a hold was placed). Several no-consensus closes about one person are a signal a
staff reviewer may look at; it never causes a sanction by itself.

**Collusion, stalling, privacy.**
- A ring of aged, verified accounts can fake two viewpoint clusters in a small pool, which is why the floors are
  high, assignment is random, rater pseudonyms **differ per case**, and the gold cases measure rater quality.
- A group that skips together denies quorum. Skip rates are tracked, and a case with **two or more independent
  reporters** or a staff-flagged severity **escalates to staff instead of closing silently as no action**.
- The subject's record download and every notice show only the **aggregate**: never rater ids, counts per rater
  or timing. Raters cannot message each other. The audit trail with rater ids is staff-only.

**Notice, response, appeal.** A standing outcome goes to the notice flow in section 6. The person's response (30
days, section 19) is read first by a staff reviewer, who screens it for threats toward the reporter (section 15,
item g), and then goes to a **second panel** that re-rates with the response attached. A response can lower or
drop an outcome and never raise it. An appeal of a panel-decided outcome goes to a fresh panel with no rater from the earlier ones and a
quorum of 15; **without a consensus to uphold, the sanction is lifted.** **Appeals of the staff-decided rules
(`CSAM-1`, `THREAT-1`, `DOX-1`) go to a different staff member and never to raters, and a failed quorum never
lifts them.** Staff may reverse any outcome at any
time, with a reason in the audit record.

**Pool floors.** One case can need 9 first raters, a second panel of 9 (not required to be new) and an appeal panel
of 15 with no earlier rater: up to 33 distinct raters. So the floors are separate: **bridging decides nothing above
a warning until 100 eligible active raters exist, and appeals go to staff until 150.** Below those numbers the
staff group decides (decision A2). The size of the pool is on the fairness report (section 15, item f).

**Rater abuse.** Ratings that follow a coordinated pattern (many raters, one answer, created together or acting
together) are dropped from the scorer and the raters flagged for a staff look; removing a rater needs a written
reason and a notice, with a chance to answer, like anyone else.

**Not decided by agents or by counts.** Report counts do not rate a case; evidence does (section 8). The only
sanction that can take effect without a panel or a person is the first minor warning (section 19, decision F2).

## 19. McKale's decisions of 2026-10-02, and what each changes

| | Decision | Design effect |
|---|---|---|
| A | Community Notes style | Section 18. |
| B | B1: 24-hour emergency hold | Section 7 unchanged; the lapse pages McKale and the reviewer. |
| C | Starting rung depends on severity | The table below. |
| D | D2 retention, repeat offenders banned | Section 10; the definition below. |
| E | 30-day response window; nothing changes on the account until it is decided | Below. |
| F | F2: a first minor warning goes out on the agent's draft, 10% sampled | Below; principle 3 amended. |
| G | G2: a ban needs no second human | Below. |
| H | H2 for dedicated servers; H1 for author-hosted sessions | Below. |

**C. The starting rung depends on the category** (a proposal for McKale to confirm; a panel can go lower and can
never go higher than the cap without a new notice):

| Rule | First-offence start | First-offence cap | Decided by |
|---|---|---|---|
| `CSAM-1` | 5, ban (with the emergency hold and the legal duty in section 7) | 5 | staff |
| `THREAT-1` credible | 4 | 5 | staff |
| `DOX-1` | 4 | 5 | staff |
| `HATE-1` | 3 | 4 | panel |
| `SCAM-1` | 3 | 4 | panel |
| `SEX-1` (non-consensual or unsolicited, section 20) | 3 | 4 | panel |
| `SELFHARM-1` (encouraging it) | 2, with support resources | 3 | panel |
| `HAR-1` | 2 | 3 | panel |
| `IMPERS-1`, `CHEAT-1`, `OTHER-1` | 2 | 3 | panel |
| `SPAM-1` | 1 | 2 | panel |

Step 1 is no action, 2 a warning, 3 a feature limit, 4 a suspension, 5 a ban (section 6). A prior upheld violation
inside 365 days raises the start by one step, up to the rule's cap for a first offence plus one.

**D. Repeat offenders are banned. A repeat** (a proposal for McKale to confirm): the **third** upheld violation
of any rule within 365 days, where each earlier outcome had been noticed and its response window had closed or
been answered **before** the conduct in the next case happened. A warning counts only once a human has read it, or once its response window closed unanswered and a staff
review passed it; an unreviewed agent-sent warning is history for staff only, so nobody can farm automatic
warnings against a target to drive the ladder. A ban is
the ban rung (section 6): it hides the account, keeps its data for the appeal period, and goes to a fresh panel
on appeal like any outcome (a different staff member for the staff-decided rules). Retention is D2: 90 days after a closed case with no action; 2 years after a sanction
ends; for a legal hold, as long as it lasts.

**E. A 30-day response window; nothing changes on the account until it is decided.** McKale, 2026-10-08:
innocent until proven guilty. From the notice:
- The account keeps every feature, social features included, while the person can still answer. Building, saving
  and publishing are untouched, and their publishes go through the moderation review as usual.
- With no response in 30 days the proposed outcome takes effect. With a response, it takes effect only after the
  second panel has read it.
- The 30 days **do not run while the person has not signed in**: the clock starts on their first sign-in after the
  notice (sent after the random delay of section 15, M4), so a person who is away never loses anything over a notice
  they never saw.
- The only consequence before a decision is the emergency hold of section 7, placed by a person, for the
  categories that cannot wait.

**F2. A first minor warning goes out on an agent's draft, with 10% sampled.** The limits (Security's, all testable):
1. The rule is one of `HAR-1`, `SPAM-1`, `CHEAT-1`, `OTHER-1`, and the person has no upheld violation.
2. It needs **corroboration**: a second independent attester or two independent reporters. A lone
   `reporter_client` claim goes to the human queue (S1).
3. At most **one automatic warning per subject per 180 days**.
4. Never for a `via: agent` report, never for a reporter flagged by the abuse signals, and never inside a brigade
   signal (a same-day shared IP hash, or accounts under 90 days).
5. The 10% sample is drawn with an **unpredictable seed**. A warning that cases 3 and 4 exclude from automation,
   but a human chose to send, is reviewed 100%.
6. The reasons cite evidence by id and passed the citation and identity checks (section 15, item d). The notice
   says it was drafted automatically and that staff may retract it. A warning found wrong is **retracted in the
   open**: removed, logged, never counted. The person can respond and appeal like anyone else.
This is the first automated consequence, which principle 3 allowed only with McKale's say, and he has said it.
(McKale also said "a minor warning is equal to severe infractions"; the orchestrator is asking what he meant and
nothing in this design depends on it yet.)

**G2. A ban needs no second human.** A ban comes from a panel consensus (quorum 15) or from one staff member
with a fresh admin sign-in and a written reason. Dropping the second human needs compensating controls, because
one compromised or mistaken admin session could otherwise ban anyone at once:
- a per-admin cap of **3 bans per 24 hours**, and a **page to McKale and the staff group on every staff-issued
  ban**, not after the third;
- the `SAFETY_ADMIN` class requires the fresh sign-in and, when Security delivers it, the passkey step-up;
- **no bulk ban route**;
- a staff-issued ban takes effect after a **30-minute delay**, visible only to staff, during which any other staff
  member can **pause** it for review (a pause, not a veto);
- **the appeal is the safeguard** for panel-decided bans: a fresh panel, and no consensus to uphold lifts the
  ban. A ban in `CSAM-1`, `THREAT-1` or `DOX-1` is never lifted by a failed quorum; its appeal goes to a
  different staff member.

**H. Dedicated servers get a platform relay; author-hosted sessions stay host-run.** For a session on a
platform-run dedicated server the relay attests the evidence (`relay`), mute and block are enforced on the relay as
well as the client, and the automatic hold of section 7 becomes possible under Security's limits. For
author-hosted sessions section 3 and section 17 stand: evidence is a claim with provenance, strengthened by signed
lines, and a person places any hold. No dedicated-server product exists today, so none of this is built until one
does.

## 20. Eustress is 18+: what changes (McKale, 2026-10-02)

"Eustress is actually 18+. Adult sexual content of high quality is allowed; bad is not." Three consequences.

**1. `SEX-1` (a first definition for Website to turn into public text, and for McKale to confirm).** The line a
person can be sanctioned on is **conduct and context**, not craft:
- Allowed: sexual content that shows only adults (characters unambiguously adult; no childlike features, no
  ageplay), that is fiction or involves a real person only with their verified consent, that is labelled Adult
  and reachable only by a verified adult who opted in.
- Not allowed (a `SEX-1` violation): sexual content that appears to show a minor (that is `CSAM-1`, staff only);
  a real person's likeness without consent, including deepfakes; content that shows or promotes sexual coercion
  as something real; sexual content sent or shown to someone who did not opt in, including in public, default
  or spawn areas, thumbnails and chat; and sexual content in a surface the Adult gate does not cover.
- **Quality does not decide a sanction.** "Bad" is a taste judgement: a bridging panel can agree on conduct and
  consent, but sanctioning a person because their work is judged poor is not a fair or checkable standard. My
  recommendation: quality governs **discoverability** (the Gallery's featured and Adult shelves, ranking, curation),
  and spam or low-effort repetition is `SPAM-1`. McKale, please confirm; if he wants a quality bar for the
  Adult shelf, it belongs to curation, not to a notice against a person.

**The Adult gate is a build item, not a policy sentence.** In the Worker today the rating is a list filter chosen by the
caller (`rating_max` on the list routes), and a first search found no check of the viewer's age verification on the
download, thumbnail, cover, preview, live or admit routes. If Adult is gated, the Worker must enforce it on each of
those routes, and the public thumbnail and cover of an Adult listing must be non-explicit **by rule**. This is
decided before launch.

**2. The Gallery's Teen (13+) rating needs a decision.** An 18+ platform has no 13-year-old audience, so a Teen
rating no longer describes who may see a listing. Options:
- T1. **Retire audience ratings and use content descriptors (recommended):** General (nothing explicit), Mature
  (violence, themes), Adult (sexual content, gated to verified adults who opted in). The rating says what is in a
  listing, not who may watch.
- T2. Keep Teen as a descriptor meaning "no mature content", with the platform still 18+. Least change; the
  name reads as an audience and invites a 13-year-old.
- T3. Keep the Teen audience and admit under-18 users. Contradicts the 18+ statement and brings back the
  child-safety duties the 18+ gate removes.
The moderation pipeline's rating field, the Gallery filters and the Website's pages follow the choice.

**3. Counsel checks before launch** (new, on top of section 15, item e): age verification strength for explicit
content, payment-processor rules for adult content (they can restrict what the shop sells), app-store and hosting
terms, the federal and state rules on non-consensual intimate imagery (including takedown duties on notice), the
record-keeping duty for real-person imagery, and the rules in the UK and EU for adult content and age assurance.
`CSAM-1` is unchanged by any of this: no exception, staff only, legal reporting by a person.
