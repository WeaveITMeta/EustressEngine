# Targeted marketing opt-in: the "Marketing Emails" switch

Status: **Phase 1 written 2026-11-15, not deployed.** Sign-up has a **Marketing Emails (Optional)** box, and the
**Marketing Emails** switch on `/settings` saves the account's choice. Worker: `src/marketing_consent.mjs`, the register
route and `/api/notifications/prefs`; tests `tests/marketing_consent.test.mjs`. Website: `pages/login.rs`,
`pages/settings.rs`, `api/auth.rs`. Phases 2 and 3 are not built. **Weekly Digest** on `/settings` is still a plain
button with nothing behind it, drawn **on**.

Deadline that shapes the order: **Black Friday is Friday 2026-11-27** (Thanksgiving is Thursday the 26th, Cyber Monday
is the 30th). Phase 1 and Phase 2 below are what a Black Friday send needs.

## The shape

```
 /settings switch ──PUT──▶ Worker: notify-prefs:{id}.marketing_email + index key mkt-optin:{id}
                                │
                 daily cron ────┤ builds the list from mkt-optin:* (opted in, verified email, not suppressed)
                                ▼
            GetCSV.io S3 bucket: eustress/marketing-optin/YYYY-MM-DD.csv  (+ latest.csv)
                                │
     NOVA (Rune scripts on schedule) · Claude Duet · Grok Bot read the day's list
                                │
          draft HTML per lead ──▶ GetCSV campaign rubric loop until it passes ──▶ send (GetCSV / SES)
                                │
 opt-out (switch, or the email's one-click link) ──▶ removed from the next list AND suppressed at once
```

An account is on the list from the first daily build after it turns the switch on, and every day after, until it turns
the switch off or unsubscribes from an email. Each day's file is a full snapshot, never a delta, so a reader that
misses a day loses nothing and an opted-out account cannot linger in an old append.

## Phase 1: consent at sign-up and in Settings (built, not deployed)

### Where the box may start ticked

The rule is GetCSV's own jurisdiction table (`getcsv-desktop/crates/marketing/src/jurisdiction.rs`), and the country is
the one Cloudflare reports for the request (`request.cf.country`, else `CF-IPCountry`):

| Visitor's country | Box at sign-up | Why |
|---|---|---|
| United States | **ticked**, "(Optional)" | CAN-SPAM is opt-out: no prior consent is needed, only a working unsubscribe and honest headers |
| Canada | empty | CASL's implied consent covers a published business address, not a consumer sign-up |
| EU and EEA, UK, Switzerland | empty | GDPR, UK PECR, nDSG: a pre-ticked box is not consent |
| Every other country in the table (AU, NZ, JP, KR, BR, MX, IN, and the rest) | empty | consent required |
| Unknown (`XX`), a Tor exit (`T1`), or no country | empty | treated as needing consent |

Widening the ticked list (`OPT_OUT_COUNTRIES` in `src/marketing_consent.mjs`) is a legal decision. The person may
always untick the box, and nothing else on the page depends on it.

**The Worker decides, not the page.** `GET /api/kyc/jurisdiction` (which the sign-up page already calls for its ID
list) now answers `marketing: { prechecked, text, version }`, and the page starts the box from `prechecked`. At
`POST /api/auth/register` the Worker works out `prechecked` again from the request's country and never reads it from
the body. So a "yes" from a country that needs consent is always recorded as the person's own tick. A sign-up that
sends no `marketing_email` (Studio, an older page) records nothing and stays off. Recording is best effort: a failure
to store the choice never fails the sign-up.

### What is kept (USERS)

| Key | What |
|---|---|
| `notify-prefs:{id}` | `.marketing_email` and `.marketing`: `{ on, basis, country, version, source, at }`, beside `.moderation_email` |
| `mkt-optin:{id}` | present while it is on: the index the daily list is built from |
| `mkt-consent:{id}:{at}` | one record per change, never overwritten: the evidence of who agreed to what, where and when |

`basis` is `prechecked` (US, box left ticked), `express` (the person ticked an empty box, or turned the Settings switch
on), `declined`, or `unsubscribed` (the one-click link, Phase 2). `source` is `signup`, `settings` or
`unsubscribe_link`. `version` is `MARKETING_CONSENT_VERSION`; bump it whenever the sentence changes:

> Marketing Emails (optional): Eustress news, release notes, your stats and holiday promotions. Unsubscribe anytime.

### Settings

`GET /api/notifications/prefs` answers `marketing_email` (off unless chosen). `PUT` takes `marketing_email`,
`moderation_email`, or both; each is optional, each must be true or false. Turning marketing on there is `express`
wherever the person is; turning it off always works and removes `mkt-optin:{id}` at once. The switch is wired like
the review-email switch above it: shown at once, put back with a line if the save fails.

No verified email is needed to tick the box: the address is often unconfirmed at sign-up. Instead the daily list
(Phase 2) takes only accounts whose email is verified, so confirming the address is the second step of a double
opt-in.

## Phase 2: the daily list to GetCSV.io

**Cron.** The Worker's `scheduled()` already ticks hourly (`wrangler.toml`, `"0 * * * *"`). At 09:00 UTC (2:00 AM
Arizona), build the list:

1. List `mkt-optin:*`. For each account: skip if banned, deleted, email not verified, or on the suppression list.
2. Write one CSV row per account.
3. `PUT` it to the GetCSV bucket as `eustress/marketing-optin/YYYY-MM-DD.csv` and overwrite `latest.csv`.

**Columns** (only what the consent sentence covers; no KYC data, no age, no identity documents, ever):

| Column | Source |
|---|---|
| `contact_id` | opaque per-list id (HMAC of the account id); never the account id itself |
| `email` | `readAccountEmail` |
| `username` | public username, for the greeting |
| `opted_in_at` | `marketing.at` |
| `country` | `marketing.country`, the country at the time of the choice |
| `consent` | `granted` for `express`, `not-required` for `prechecked` (GetCSV's `ConsentStatus`) |
| `plan` | Free / Pro, for offer fit |
| `last_active_on` | date of the last Studio or website session |
| `spaces_published`, `bls_earned_30d`, `plays_30d` | the "your stats" lines |
| `unsubscribe_url` | the one-click link below, unique per account |

**Credentials.** A dedicated IAM user whose only permission is `s3:PutObject` on
`arn:aws:s3:::<getcsv bucket>/eustress/*`. Its keys are Worker secrets (`GETCSV_S3_ACCESS_KEY_ID`,
`GETCSV_S3_SECRET_ACCESS_KEY`, `GETCSV_S3_BUCKET`, `GETCSV_S3_REGION`), never in the repo or a response. SigV4 signed
in the Worker (`aws4fetch` or `crypto.subtle`). Bucket side: SSE on, public access blocked, a lifecycle rule that
expires `eustress/marketing-optin/` objects after 30 days, so no old snapshot outlives an opt-out by more than that.

**Opt-out reaches the sender at once, not tomorrow.** A list that is up to 24 hours old must not mail someone who
opted out an hour ago. On every opt-out, the Worker also writes `eustress/suppressions/<iso time>-<contact_id>.json`
to the bucket, and the send step checks suppressions immediately before each send. The rubric requires opt-outs
"honored at once"; CAN-SPAM's 10 business days is the legal floor, not the target.

**One-click unsubscribe.** `GET`/`POST /api/marketing/unsubscribe?c=<contact_id>&t=<token>`, the same RFC 8058 shape
as the listing-review unsubscribe already in `moderation.mjs` (a GET shows a button, the POST does the work), with an
HMAC token under its own purpose label so a review-email token cannot unsubscribe marketing or the reverse. It turns
the switch off, deletes `mkt-optin:{id}`, logs `source: unsubscribe_link`, and writes the suppression object. Every
campaign carries `List-Unsubscribe` and `List-Unsubscribe-Post: List-Unsubscribe=One-Click` headers with it.

## Phase 3: campaigns from the list (NOVA, Claude Duet, Grok Bot)

The consumers read `latest.csv` on their own schedules (webhooks, cron jobs, NOVA Rune scripts) and act on it. Two
streams, and only two:

- **Updates and stats:** release notes, new features, the reader's own stats. At most one a week, and only when there
  is a release or a number worth telling.
- **Promotions:** on the holiday schedule below, and nowhere else.

**Rubric loop.** Grok Bot drafts per lead and scores each draft against the GetCSV Email Campaign Rubric
(`https://getcsv.io/downloads/getcsv-campaign-rubric.pdf`), redrafting until it passes all three gates: **zero hard
fails**, **soft score 90 or more**, **benchmark gate 70 or more**. A draft that cannot pass after a set number of tries
(suggest 5) is dropped for that lead, never sent anyway.

The rubric is written for cold outreach; for an opted-in list, read these items this way:

| Rubric item | For an opted-in Eustress member |
|---|---|
| Verified business name (12) | their Eustress username |
| Where you found them (6) | "You turned on Eustress updates in your settings." |
| A specific line about them (6) | one real stat from their row (a published Space, BLS earned, plays) |
| Every claim checked on their own website | every claim checked against their row; nothing invented |
| First touch never pretends an earlier email | still applies |
| Postal address in the footer | still applies (CAN-SPAM); the address is an open item below |

**Approval.** The rubric's Section 05 says nothing sends until the owner approves the exact rendered message. Auto-send
after the loop passes conflicts with that, so the rule here is the one already set for Eustress campaigns: **McKale
approves each campaign once, before it is scheduled**, from its brief and one rendered sample at desktop and 375 px;
after that, per-lead variants that pass the rubric send without a second approval. A campaign that changes its offer,
audience or brief needs a new approval. Each send stops on any complaint, as Section 05 says.

**Research and custom campaigns (later).** Tailoring campaigns to what each member wants needs data they gave for that
purpose. Add a preference center (topics: releases, tutorials, stats, promotions, research invites) and an optional
short survey, each with its own consent sentence and version. Nothing from Studio telemetry, KYC, chat or moderation
records feeds a campaign.

## Holiday promotion schedule: essentials only

Times are Arizona. One promotional email per date, to the opted-in list only. The offer for each is McKale's call
(Pro, Tickets, Delta).

| Date | Send | Why |
|---|---|---|
| Fri 2026-11-27, 6:00 AM | Black Friday | the main sale day |
| Mon 2026-11-30, 6:00 AM | Cyber Monday | the online-sale day; last day of the Black Friday offer |
| Tue 2026-12-15, 6:00 AM | Holiday gift | gift Pro or Tickets in time for the holidays |
| Sat 2027-01-02, 8:00 AM | New Year | the "start building" month; off New Year's Day itself, when nobody reads email |

Nothing else promotional in this window. A member who gets all four plus the weekly update stays within one email a
week on average.

## Privacy policy section (on the page in the live tree 2026-11-15, not deployed; counsel to review)

Written into `eustress/crates/web/src/pages/privacy.rs` on McKale's word (2026-11-15), with the bracketed proposals
below taken as written and "Last updated" set to November 15, 2026. It goes live with the site deploy. Counsel may still
change any bracketed term. The page promises an unsubscribe link in every marketing email, an immediate stop at
GetCSV.io and 30-day list deletion: all three must exist before the first marketing email is sent (Phase 2).

**Section 3, Information Sharing: one more line in the list**

> Our email service provider, GetCSV.io, and the AI providers it uses to write emails, only for marketing emails you
> chose to receive (see Marketing Emails below)

**Section 3: a new subsection after the list**

> ### Marketing Emails
>
> Marketing emails are optional. You choose at sign-up with the Marketing Emails box, and you can change your mind at
> any time in Settings or with the unsubscribe link at the bottom of every marketing email.
>
> Where the law lets the box start ticked (today, the United States), it starts ticked and you can untick it. Everywhere
> else it starts empty, and only your own tick turns it on. We decide which applies from the country Cloudflare
> reports for your connection when you sign up.
>
> If you choose to receive them, we send Eustress news, release notes, a summary of your own activity (such as Spaces
> you published, plays and BLS earned) and holiday promotions: [at most one email a week, plus holiday offers].
>
> To write and send these emails, we share your email address, username, plan, country and those activity figures with
> GetCSV.io, our email service provider. GetCSV.io stores them on Amazon Web Services and uses AI models from [xAI and
> Anthropic] to write a version of each email for you. They use this information only to send Eustress emails on our
> behalf. We share nothing from your identity verification, your chats or your support requests.
>
> We keep a record of each choice you make (what you chose, the words you were shown, the country and the time) as
> proof of your consent, [for as long as your account exists and for 3 years after you turn marketing emails off].
> When you turn them off, we take you off the next list at once and tell GetCSV.io to stop emailing you straight away.
> The lists GetCSV.io holds are deleted after [30 days].
>
> Where the law requires your consent for marketing emails, that consent is our legal basis, and you can withdraw it
> at any time. Elsewhere, our basis is our legitimate interest in telling account holders about Eustress, and you can
> object at any time in the same ways.
>
> Emails about your account, such as sign-in, security notices and review results, are not marketing and do not
> depend on this choice.

**Questions for counsel**

1. The bracketed cadence, retention periods and AI provider names.
2. Whether GetCSV.io's relationship to Eustress (same founder) should be stated.
3. Whether a data processing agreement between Eustress and GetCSV.io must be signed before the first export (GDPR
   Article 28 requires one for EU members).
4. Whether the US default should also check the country on the member's verified ID, so a VPN cannot put an EU
   resident on the ticked default.

## Open items (McKale)

1. **Postal address** for the footer (CAN-SPAM): which business address.
2. **Sending identity:** the `From:` address and domain, with SPF and DKIM passing (rubric Section 04). The earlier
   decision was Eustress's own SES setup, built from GetCSV's code.
3. **The bucket:** its name, region, and whether the IAM user above is created by McKale (it is an AWS account
   change, so it is his).
4. **Privacy policy:** it must name GetCSV.io and AWS as processors of the email address for marketing, before the
   first list is exported. Counsel reads the sentence.
5. **Approval rule:** confirm the once-per-campaign approval above, or say auto-send with no approval.
6. **Black Friday offer** and whether the 2026-11-27 send is in scope, given Phases 1 and 2 must ship and deploy first.
7. **Countries GetCSV may mail.** GetCSV's send policy defaults to the US only until a legal review (`SendPolicy` in
   `jurisdiction.rs`). Non-US members who ticked the box are recorded as `granted`, but GetCSV will not mail them
   until their countries are enabled on the Eustress account there. Which ones, and when, is a counsel question.
8. **Location is where the visitor connects from.** A VPN or travel can place someone in the US who lives in the EU.
   The record keeps the country seen, and counsel decides whether the KYC document's country should also be checked
   before a ticked-by-default box is allowed.

## Build order

1. Done, not deployed: the sign-up box, the consent record and index, the Settings switch. Worker and website deploys
   need McKale's word.
2. Worker: the one-click unsubscribe. Decide Weekly Digest.
3. Worker: daily export and the suppression writes, once the bucket and IAM user exist.
4. Consumers: NOVA, Claude Duet and Grok Bot read `latest.csv`; the rubric loop; the approval step.
5. Holiday sends on the schedule above.
