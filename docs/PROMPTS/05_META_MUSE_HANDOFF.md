# 05 · META MUSE HANDOFF

**Status:** Normative for the founder's web work and for testing Eustress the way a new user meets it.
This file hands that work to Meta Muse, and states what stays with Claude and with the founder.
**Companions:** `LAUNCH_PLAN.md` (Streams 2 and 3), `docs/launch/PUBLIC_ALPHA_CHECKLIST.md`,
`docs/legal/`, `02_QUEUE.md` §10 (executor routing for the 159 program items).

---

## 1. Who does what

**Meta Muse** is an agent running in its own virtual machine. It works mainly in a browser, and it can
also download, install, launch, and uninstall programs there, Eustress included. The founder keeps
credentials, passwords, identity details, documents, and payment methods in Muse's vault; Muse does
the grunt work: filings, account setup, dashboards, posting, and installing Eustress from the public
site onto a machine that has never seen it. It never touches this repository or the founder's own
computer.

| Role | Owns |
|---|---|
| **Meta Muse** | The company's web work (sections 4 and 5), and the install path a new user takes, on its own fresh machine (section 6) |
| **Claude** (this repository) | Drafting what Muse submits (operating agreement template, business descriptions, privacy note, post text); wiring what Muse obtains into code and CI; publishing the installers Muse tests; read-only command-line checks; and the instrumented Studio sessions, which must run on the pinned reference machine (`02_QUEUE.md` §10) |
| **The founder** | The decisions in section 7, one approval per irreversible step, signatures, and anyone outside the company |

The engineering program's 159 items are almost entirely code, so most of Muse's workload sits upstream
of them. `G1.40` measured the money rail: the Worker and its Stripe code are live, the treasury has
never held a dollar, and the missing piece is the legal and banking chain in section 3.

---

## 2. Rules every card inherits

Paste this section at the start of every Muse session, followed by the card.

**Sites.**
1. Use only the official site each card names, or a subdomain of it. Lookalike sites charge for
   filings that are free: an EIN costs nothing, so any site asking payment for one is not the IRS.
2. If a flow sends you to a different domain, stop and report the address before continuing.

**The vault.**
3. Take every credential, identifier, document, and payment method from the founder's vault. Enter a
   secret only into the official form field that asks for it. Never email, message, post, or write a
   secret anywhere else.
4. Enter the legal name, addresses, and ownership details exactly as the vault holds them, character
   for character. The LLC's legal name must match across every filing, or later identity checks fail.

**Programs.**
5. Install only what a card names, downloaded from the site the card names, and only on your own
   virtual machine.
6. Override a security warning (SmartScreen, Smart App Control, a browser certificate warning) only
   where a card authorizes that exact warning, and copy the warning's full text first.

**Checkpoints.**
7. Before each irreversible step (submitting a legal filing, paying, activating live payments,
   accepting an agreement on the company's behalf, publishing), stop on the final review screen,
   screenshot it, and wait for the founder's go. That is one approval per filing; the founder may
   waive it card by card.
8. When a page asks something the vault cannot answer, such as a business classification or an
   optional election, stop and ask. Leave it unanswered rather than guess.

**Records.**
9. Save every confirmation, receipt, stamped filing, certificate, and letter as PDF to the founder's
   records location, immediately. Some can be downloaded only once. Never save them in the Eustress
   repository.
10. End each card with a short report: what was filed or tested, confirmation numbers, amounts paid,
    exact error and warning text, what is pending, and when to check back.

**Scope.**
11. Change only the settings a card names. Production dashboards (Cloudflare, Stripe, GitHub) carry
    live systems.

---

## 3. Order of operations

```
B01 Nevada LLC ──┬──> B03 EIN ──> B04 Mercury ──> B05 Stripe on the LLC
                 ├──> B02 D-U-N-S ──┬──> B06 Apple Developer (organization)
                 │                  ├──> B07 Google Play Console (organization)
                 │                  └──> B08 Windows code signing
                 ├──> B09 Arizona foreign registration (if it applies)
                 ├──> B10 DMCA designated agent
                 └──> B11 Child-safety reporting registrations (before public uploads)

Available now, no entity needed: W01 to W06
Needs a published installer:     R01, then R02 and R03
```

B01 is the keystone. `LAUNCH_PLAN.md` calls it the root unlock, and `LICENSE-COMMERCIAL.md` already
grants rights in the name of "Eustress LLC", an entity that does not exist until B01 is approved.

---

## 4. Business cards

### B01 · Form Eustress LLC in Nevada
- **Site:** Nevada SilverFlume, `esos.nv.gov` (Nevada Secretary of State).
- **Founder decides first:**
  - The **registered agent.** Nevada requires one with a Nevada street address, and you operate from
    Arizona, so this is a paid commercial agent. Muse can compare three and report their prices.
  - The **addresses to list.** Everything on the Initial List becomes public record, so a business
    or agent address may suit better than a home address.
  - The **managers or members**, and confirmation that the name is "Eustress LLC".
- **Muse:** check name availability, then file the Articles of Organization, the Initial List, and the
  State Business License in one session. Stop at the fee summary. Expect about $425 in state fees
  ($75 articles, $150 initial list, $200 business license) plus the agent's own fee; the page is
  authoritative.
- **Save:** the stamped articles, the business license, the receipt, and the Nevada business ID.
- **Claude drafts:** a single-member operating agreement template for your review, to sign after the
  EIN as `LAUNCH_PLAN.md` sequences it. Consider an attorney's read before signing.

### B02 · D-U-N-S number
- **Site:** Apple's free lookup and request tool, `developer.apple.com/enroll/duns-lookup`, which
  files with Dun & Bradstreet.
- **Needs:** B01 approved; the legal name and address exactly as filed.
- **Muse:** look the LLC up; if it is absent, submit the request. Issuance often takes about a week.
- **Unlocks:** B06 and B07 as organizations, and usually B08's validation.

### B03 · EIN
- **Site:** the IRS online EIN application on `irs.gov` only. It is free and open during the weekday
  hours the page states.
- **Needs:** B01 approved; the responsible party's details from the vault.
- **Muse:** apply as a limited liability company. Stop at the final review.
- **Save immediately:** the EIN confirmation letter (CP 575). The site offers it once, at the end of
  the session; a lost copy means asking the IRS for a 147C letter by phone.
- **Unlocks:** B04, B05, and the operating agreement signature.

### B04 · Mercury business account
- **Site:** `mercury.com`.
- **Needs:** B01's articles, B03's EIN letter, and the owner's identity details and documents from
  the vault.
- **Muse:** apply as Eustress LLC, upload the formation document and EIN letter, and stop before the
  final submit.
- **Save:** the approval notice. Account and routing numbers go into the vault only.
- **Claude drafts:** the business description and expected-activity answers.
- **Unlocks:** B05's payouts, and the cash-out leg of Bliss once that design decision is made.

### B05 · Stripe on the LLC
- **Site:** `dashboard.stripe.com`.
- **Needs:** B03 and B04. The Worker at `api.eustress.dev` already reads `STRIPE_SECRET_KEY`,
  `STRIPE_WEBHOOK_SECRET`, and `STRIPE_PRICES`, so a Stripe account exists. Update that account;
  opening a second one splits your history.
- **Muse:** complete business verification as Eustress LLC with the legal name exactly as the IRS
  holds it for the EIN, since Stripe checks the pair. Set Mercury as the payout account. Stop before
  switching on live payments.
- **Claude:** checks read-only whether the Worker holds a live or a test key, and aligns Stripe's
  products with the chargeable-today SKU sheet (`G1.42`). Putting the live key on the Worker is a
  production deploy you approve.

### B06 · Apple Developer Program
- **Site:** `developer.apple.com/programs/enroll`.
- **Founder decides:** **organization** (the seller shows as Eustress LLC; needs B02 and a public
  website on the company's domain) or **individual** (faster; shows your personal name). Either can
  notarize the macOS build.
- **Muse:** enroll with the Apple ID from the vault. Stop at the $99 annual fee.
- **After approval:** create an App Store Connect API key for notarization. The key file goes to the
  vault and from there into GitHub Actions secrets (W01), never through Claude.
- **Claude:** adds notarization to the release workflow once those secrets exist.

### B07 · Google Play Console
- **Site:** `play.google.com/console`.
- **Founder decides:** **organization** (needs B02) or **personal.** New personal accounts must run a
  closed test with a minimum number of testers for two weeks before publishing to production, and
  organization accounts skip that step; confirm on the page.
- **Muse:** create the developer account and complete identity verification. Stop at the $25
  one-time fee.
- **Claude:** the store listing waits for an Android build worth shipping.

### B08 · Windows code signing
- **Founder decides:** the provider. Since June 2023 code-signing keys must live in hardware, so a
  certificate file in CI no longer works; the practical routes are cloud signing services, such as
  Microsoft's Trusted Signing on Azure or a certificate authority's own cloud signing.
- **Muse:** compare three providers on price, validation paperwork, and CI support, and report. After
  your pick, place the order and complete organization validation with B01, B02, and B03 documents.
  Stop at payment.
- **Claude:** adds signing to the release workflow once the credentials sit in GitHub Actions secrets.
  R01 records exactly what an unsigned installer costs you in warnings today.

### B09 · Arizona foreign registration (only if it applies)
- You operate from Tucson. Arizona generally expects an out-of-state LLC transacting business there to
  register as a foreign LLC with the Arizona Corporation Commission. Settle whether that applies with
  an accountant or attorney.
- **If it does:** Muse files at `azcc.gov` after B01 and stops at the fee.

### B10 · DMCA designated agent
- **Site:** `dmca.copyright.gov` (US Copyright Office).
- **Source:** `docs/legal/DMCA.md` records it as "Pending, register before launch". The safe harbor for
  hosting other people's content depends on it.
- **Needs:** B01, since the agent is registered for the legal entity.
- **Muse:** register the designated agent with the contact details from the vault. Stop at the fee.
  Save the directory entry.

### B11 · Child-safety reporting registrations (before user uploads go public)
- **Source:** `docs/legal/CSAM.md` designs for NCMEC CyberTipline reports and PhotoDNA matching. Both
  need the legal entity to register, and approval takes time.
- **When:** as soon as B01 is approved if the publish pipeline is close to live; it is not deployed today.
- **Muse:** register the company as an electronic service provider with NCMEC's CyberTipline and apply
  for Microsoft's PhotoDNA Cloud Service. Stop at each final submit.
- **Claude:** wires both into the moderation pipeline once the credentials exist.

---

## 5. Web operations cards

Available now; none needs the LLC.

| Card | Source | What Muse does | Stop gate |
|---|---|---|---|
| **W01** · GitHub settings | `G0.12`; B06, B08 | Enable private vulnerability reporting (the site's `security.txt` already points to the advisory form). Later, add the signing and notarization secrets as GitHub Actions secrets | Change only the settings named |
| **W02** · Cloudflare | `security.txt`; the 522 on `releases.eustress.dev` | Confirm mail to `security@eustress.dev` reaches you, and set up Email Routing if it does not. Remove the orphaned `releases.eustress.dev` DNS record, which answers 522 because the updater now uses `downloads.eustress.dev` | Remove the record only after your go |
| **W03** · Admin pages at phone width | Launch checklist (SHOULD) | Sign in with the admin credentials from the vault, set the browser to phone width, screenshot `/admin` and `/admin/telemetry`, and note clipped elements and sideways scrolling. Confirm R01's test comment appears in the admin Comments panel | Navigation only; several admin buttons change production data |
| **W04** · Live-site check after each deploy | Launch checklist (BLOCKERS) | Confirm the download page shows the version, SHA-256, and system requirements; the privacy note is live; no page says "open source"; `security.txt` is served. Report what is there | Read only |
| **W05** · Posts and outreach | Launch checklist; `G0.08`; `G0.09` | Post the announcement thread on X as @Simbuilder, the channel posts for `G0.09`, and the outreach messages for `G0.08`, each only with text and recipients you approved | Nothing goes out unapproved |
| **W06** · Crash reporting | Launch checklist (SHOULD decision) | Only if you choose it: create the Sentry project and put the DSN in the vault | Your decision first |

Claude drafts every piece of text W05 posts, and fixes whatever W03 and W04 find.

---

## 6. Install cards

Muse's virtual machine has never seen Eustress, and it reaches the installer the way a stranger does,
through the public site. That makes it the right place to test the path a new user takes. These cards
need a published installer, which Claude produces through the release workflow and the founder
approves before it goes out.

### R01 · Install Eustress the way a new user does
- **Plan source:** `G7.45`, the ten-minute cohesion journey. Also closes these launch-checklist items:
  the clean install on a machine that has never seen the repository (BLOCKER), the cold start on a
  blank profile (BLOCKER), the telemetry notice appearing once with the privacy switch honoured
  (BLOCKER), and the Send Feedback live check (SHOULD).
- **Steps**, each timed and screenshotted:
  1. **Find it.** Starting at `eustress.dev`, reach the download page as a visitor would. Record the
     version and SHA-256 it shows. If it asks you to sign in, use the test account from the vault and
     note that it asked.
  2. **Download.** Record the file's size. If you can, compute its SHA-256 (in Windows PowerShell,
     `Get-FileHash`) and compare it with the page.
  3. **Install.** SmartScreen will likely warn, because the build is unsigned. Copy the warning's full
     text, then choose More info, then Run anyway; this card authorizes that override for this
     installer only. Copy the text of every installer screen.
  4. **First launch.** Record whether a window appears, the seconds until it responds, any error text,
     and the display adapter Windows reports (Settings, System, Display, Advanced display). The Studio
     needs a graphics card. If the machine has none and the Studio does not start, record exactly what
     happened and end the journey here: that is a finding worth having, since some testers will be on
     machines like this.
  5. **Cold start.** Record whether the default Universe and Space open, whether the ribbon and the
     Modes dropdown render, and whether the telemetry notice appears (copy its text). Pick a
     discipline. Place a Part. Press Ctrl+Z and record what happened.
  6. **Author and simulate.** Move, rotate, and scale the Part; undo twice and redo once; save. Press
     Play, wait ten seconds, press Stop, and record whether the scene returned to its state before Play.
  7. **Inspect.** Select the Part and record what the Properties panel shows.
  8. **Agent-drive.** In the Workshop panel, enter the Claude API key from the vault in Settings, then
     ask it to "add three Parts stacked on top of each other". Record what it did.
  9. **Publish.** Open the publish flow and stop at the final confirmation. Screenshot it.
  10. **Relaunch.** Close the Studio, reopen it, and record whether the telemetry notice appears a
      second time and whether the Space still holds your changes.
  11. **Privacy.** Switch off Settings, Notifications, Privacy. Note the size of the newest file in
      `%LOCALAPPDATA%\Eustress\telemetry\`, use the Studio for two minutes, and note it again.
  12. **Feedback.** Use Help, Send Feedback to send the text "Muse install test, please ignore"; this
      card authorizes that one send. W03 confirms it arrived.
  13. **Uninstall** from Windows Settings, Apps. List every Eustress folder that remains under
      `%LOCALAPPDATA%`, `%APPDATA%`, and `Program Files`.
- **Runs:** three, each on a fresh copy of the virtual machine. Record the screen if you can; otherwise
  screenshot every step.
- **Stop gates:** publishing past the confirmation; any payment or licence screen; any security
  override other than the SmartScreen one this card names.
- **Report:** per step, the times, what you saw, and any message text, word for word; an action that
  produced no visible change and no message gets called out as such.
- **Claude:** publishes the installer, turns the three reports into `G7.45`'s evidence, sends the
  recordings to the blinded Critic, and fixes what breaks.

### R02 · Auto-update test
- **Source:** launch checklist (NICE).
- **Needs:** R01's installed version kept on a machine, and the founder's go to publish the next
  version.
- **Muse:** once the next version is published, launch the Studio, copy the update prompt's text,
  accept the update (this card authorizes it), and record the version shown afterwards.

### R03 · Core interactions and save and load on a fresh install
- **Source:** launch-checklist BLOCKERS: the ten core interactions, and the save and load torture test,
  which the checklist says to verify on a fresh install.
- **Needs:** R01 showing the Studio runs on Muse's machine. If it does not, Claude runs this card on the
  workstation instead.
- **Muse:** on a fresh install, check each by what you can see: clicking a Part selects it; the
  selection shows a cyan outline; move, scale, and rotate act on it; Delete removes it; F frames the
  camera on it; undo and redo reverse and restore each of these; the Properties panel updates when the
  selection changes; a ScreenGui renders; copy and paste duplicates. Then import the splat file the
  card names, save, quit, reopen, and record whether everything is where you left it.

---

## 7. Decisions only the founder makes

1. The Nevada registered agent, and the addresses that go on public record (B01).
2. Which entity and email address own each account. The repository's git identity is Weave Solutions;
   the licence names Eustress LLC.
3. Organization or individual for Apple (B06) and for Google Play (B07).
4. The code-signing provider (B08).
5. Whether Arizona registration applies (B09).
6. The go on each release Muse tests (R01, R02), and on each irreversible step unless you waive it.
7. Whether to adopt crash reporting (W06).

## 8. What stays off Muse's machine, and why

| Work | Owner | Why |
|---|---|---|
| The instrumented Studio sessions for `G6.02` to `G6.31`, and the 30-minute soak | Claude, with the computer-use tool against an installed copy on the workstation | Their numbers only mean something on the pinned reference machine `RM-1`, and their instruments write files Claude reads there |
| The reference-product capture `G1.14` | The founder chooses, installs, signs in, and accepts the licence; Claude or the founder captures on `RM-1` | A comparison needs identical hardware on both sides |
| The outside people in `G0.07`, `G0.08`, `G0.10`, and the pilot in `G6.34` | The founder; Muse sends the approved messages | Only a real outside person counts |
