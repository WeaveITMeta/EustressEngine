# Staging publish test: from Studio to a joined Player

**Status:** written 2026-10-01 for the run after build 17. Staging only; production stays behind
McKale's go. Prepared by the Website session from the current code and read-only probes of the
staging Worker (version `bf784a97`, 0 listings, 1 user). Nothing here has been run yet.

The chain this proves, in order:

1. Studio signs in to staging with the identity key.
2. Studio publishes a copy of the Chassis Playground.
3. The upload reaches the staging Worker and its storage.
4. Jev's AI review approves or declines it.
5. The decision reaches the publisher, in Studio and on the site.
6. The listing shows in the staging gallery.
7. Studio hosts it, the listing shows LIVE, and a signed-in Player joins and is admitted.

---

## Before you start

| Needed | State today |
|---|---|
| Build 17 of Studio and the Player | Built and promoted to `E:\Workspace\EustressEngine\eustress\target\last-good\` (`eustress-engine.exe` and `eustress-client.exe`). Run them from there, never from `target\debug\`: the next build compiles into `target\debug`, and a program running from it blocks that build's link step ("Access is denied"). |
| The login-v1 Worker on staging, and the website build that signs the new message | Written and Security-approved; the orchestrator deploys the Worker first, then the website. Until both are up, neither Studio nor the site can sign in to staging. |
| Your staging account and its identity file (`.toml`) | The account exists (staging has one user). Have its identity file at hand. |
| The listing's author is the account that hosts it | Yes: you publish and host as the same account. The Worker refuses anyone else. |
| Review keys on staging (`GROK_API_KEY`, `JEV_API_KEY`) | Present (names checked, values not read). The review has never run end to end on staging. |

---

## Step 0: prepare a copy of the Chassis Playground

Publishing a Universe publishes every Space in it. The Chassis Playground sits in the Universe
"Drive Tests" beside "Vehicle Simulator Drive Test 1", so work on a copy with only the Chassis
Playground in it. Close Studio first, because the Universe's database is locked while it is open.

```powershell
Copy-Item -Recurse "C:\Users\miksu\Documents\Eustress\Drive Tests" "C:\Users\miksu\Documents\Eustress\Chassis Staging Test"
Remove-Item -Recurse "C:\Users\miksu\Documents\Eustress\Chassis Staging Test\Spaces\Vehicle Simulator Drive Test 1"
```

The copy has no listing id yet, which is what makes the first publish create a new listing.

## Step 1: start Studio against staging

Find this computer's local network address (the host link must name an address other players
can reach, and the Worker refuses `127.0.0.1` and `localhost`):

```powershell
(Get-NetIPConfiguration | Where-Object { $_.IPv4DefaultGateway -and $_.NetAdapter.Status -eq 'Up' } | Select-Object -First 1).IPv4Address.IPAddress
```

Then start Studio in the same window, putting that address where `<LAN>` is:

```powershell
$env:EUSTRESS_API_URL = 'https://api-staging.eustress.dev'
$env:EUSTRESS_HOST_LAN = '1'
$env:EUSTRESS_HOST_PUBLIC = '<LAN>:7777'
& "E:\Workspace\EustressEngine\eustress\target\last-good\eustress-engine.exe"
```

**You should see:** Studio opens. In its log (`C:\Users\miksu\.eustress_engine\logs\engine-<number>.log`,
newest file) a line says the Eustress API is not production.

## Step 2: sign in with your identity key

Open the Login dialog, choose your staging identity file, and sign in.

**You should see:** "Signed in as" followed by your staging username. If instead you see a message
about the identity ("not registered", "could not sign in", "did not accept this identity's
signature"), stop: the message names the problem (see "If something goes wrong").

Side effect to expect: a file named `auth_token.91d8c5f1e19e9d0b` appears in
`C:\Users\miksu\AppData\Local\EustressEngine\`. That is the login the Player reads.

## Step 3: open the copy and publish it

1. Open the Universe "Chassis Staging Test". Wait until the Space has finished opening.
2. Press Ctrl+P (or Publish Universe in the ribbon). Set the name to "Chassis Playground (staging test)", write a
   one-line description, pick a genre, leave **Public** on, leave **Share source** off. Click Publish.

**You should see:** a toast "Publishing Universe..." and the dialog closing. Studio shows no
further progress or result today (known gap, see the end of this file). Watch the log instead:

```powershell
Get-Content (Get-ChildItem "C:\Users\miksu\.eustress_engine\logs\engine-*.log" | Sort-Object LastWriteTime | Select-Object -Last 1) -Wait -Tail 40
```

The log should show the listing being created, the world uploaded (a few small pieces), the
thumbnail, four screenshots from the AI camera, the review notes, and "Published". A first
publish takes about one to two minutes, most of it the screenshots.

## Step 4: wait for the AI review

Studio asks for the decision for about 18 seconds, then every 5 minutes while it stays open.
The review itself should take a minute or two.

**You should see, in any of three places:**

- Studio: reopen the Publish dialog; the LISTING row reads Listed (approved), In review, or the reason it was declined.
- The site: sign in at https://staging.eustress-cbb.pages.dev/login with the same identity file, open
  https://staging.eustress-cbb.pages.dev/projects; the project shows its review status in words.
- Your email, if the staging mail sender is verified (not confirmed; do not rely on it).

Outcomes:

- **Listed:** go on.
- **In review for more than ten minutes:** the AI sent it to a person. You can approve it yourself at
  https://staging.eustress-cbb.pages.dev/admin if your staging account is an admin (see below). This is the
  review's first run on any deployment, so a hold is not a surprise. A hold can look like a stall: if the judge's
  answer could not be read or named no concrete evidence from the screenshots, the case lands in "held" with the
  code `judge_malformed`. An admin can read the case, with the judge's result or its error code, at
  `GET /api/admin/moderation/case/<listing id>`. In that record:
  - `judge.error` is the code: `judge_malformed`, `judge_no_spatial_evidence`, `judge_bad_enum`, `judge_not_json`,
    `judge_empty`, `judge_unavailable_<http status>` or `judge_unavailable_no_key`. `judge_malformed` means the AI
    answered but its answer failed the strict check (usually no concrete evidence from the screenshots); it is not an outage.
  - If the judge ran, `judge.verdict`, `judge.quality` and `judge.rationale` are there.
  - `decision.reasons` lists the codes that held or rejected the listing.
  - `jev.error` shows if the classifier failed first: the case is then held with `classifier_unavailable` and the judge never runs.
- **Declined or changes requested:** read the reason on the project page. Tell the Website session; a
  Chassis Playground has no reason to be declined, so this is a finding.

## Step 5: see it in the gallery

Open https://staging.eustress-cbb.pages.dev/gallery.

**You should see:** a card for "Chassis Playground (staging test)" with a thumbnail (it may be a squashed
picture of the Studio window, a known weakness). Click it: the listing page opens and shows **OFFLINE**,
because nobody is hosting yet.

## Step 6: host it

In Studio, press F9 (host the session). Studio shows a Hosting notice.

**You should see:**

- In Studio's Output panel: "Join link: eustress-player://join/<LAN>:7777?key=...&pin=...". Keep this line.
- In the engine log within a minute: "the gallery shows this session as live".
- On the listing page (it refreshes every 30 seconds): **LIVE**, with 0 players.

If the log says to set `EUSTRESS_HOST_PUBLIC` or `EUSTRESS_HOST_LAN`, the two variables from Step 1 were
not in Studio's window; close Studio and start it again from the window where you set them.

## Step 7: join with the Player

The simplest way, with no website click, is the Player's command line, with the link from Step 6
in quotes (the `&` characters need them):

```powershell
$env:EUSTRESS_API_URL = 'https://api-staging.eustress.dev'
& "E:\Workspace\EustressEngine\eustress\target\last-good\eustress-client.exe" 'eustress-player://join/<LAN>:7777?key=<key>&pin=<pin>'
```

**You should see:** the Player window opens and the Chassis world appears with your avatar. On the
listing page the player count goes to 1. If the window stays empty, read the newest
`C:\Users\miksu\.eustress_engine\logs\player-<number>.log`; it says why (host unreachable, the link is from an
earlier hosting session, or "the host refused the join" with a reason).

The Player can only be admitted when it is signed in: it reads the login file from Step 2. A Player with no login joins as a guest, and the host refuses guests.

### Joining with the Play button on the site

The Player from build 17 registers the `eustress-player://` link for your Windows user the first time it runs
(the last Player you run owns the link, and a Player that is not pointed at production registers itself with the API
it was started for). So the one-time setup is to run the build-17 Player once with `EUSTRESS_API_URL` set:

```powershell
$env:EUSTRESS_API_URL = 'https://api-staging.eustress.dev'
& "E:\Workspace\EustressEngine\eustress\target\last-good\eustress-client.exe"
```

Close it, then click **Play** on the LIVE listing while signed in to the staging site. The browser asks to open
the Player once. If the Play button opens nothing or opens a Player that talks to production, run the Player again
from the command above: the last one run is the one the link starts. Until you run a different Player, the link
keeps starting the staging one, so run your production Player once afterwards if you want the link back.

**What a person must do for Play to open the Player** (the same four steps are in the Play chooser and on the Space's page, under "Eustress Player not opening?"):

1. Download Eustress Player and run it once. That registers the `eustress-player` link with the browser.
2. Press Play. Chrome or Edge asks once whether to open Eustress Player: allow it.
3. If Play was pressed before the Player had run, reload the page first, then press Play.
4. With more than one Player build, the last one run is the one the link opens.

**What Play does.** Every Play button on the site (the Gallery cards and lead Space, a Space's page, Projects) opens the Play chooser over the page. The first time it asks how to play: Eustress Player (recommended) or in the browser, which says "Coming soon" until it exists; the choice is remembered, and "Change" in the chooser forgets it. `/play/<id>` is an old address that lands on the Space's page (`/simulation/<id>?play=1`) with the chooser already open. The chooser asks the Worker for the link at the click (`POST /api/simulations/{id}/play`).
While a Studio host is live and the visitor may join (signed in, or a guest holding a live lease; for a listing
that is not approved, only its author), the answer is `status: "live"` and `launch.link` is the host's own join
link, `eustress-player://join/<host>:<port>?key=..&pin=..`, exactly as the host sent it and checked by `checkJoinLink`
in `src/live.mjs`; the Player joins and the host admits it. With nobody hosting the answer is `status: "solo"` and
`launch.link` is `eustress-player://play/<id>`: the Player downloads the world and plays it alone. `launch.solo_link`
is always the solo link, so the page can offer "Play on your own" beside a live session. A visitor who may not have
the host's link (signed out, no lease) gets `status: "solo"`, `live: {players, max_players}` and
`link_hidden: true`; the host's link, key and pin are not in that answer. If the browser does not take the link the
page says "Eustress Player didn't open" and shows these steps.

## Step 8: stop

Press F9 again (or Stop Server). **You should see** the listing go OFFLINE within about 90 seconds
(immediately if Studio stopped the session, up to 90 seconds if you close Studio instead).

---

## If something goes wrong

| What you see | What it means |
|---|---|
| Sign-in: "not registered" | The identity file is not an account on staging. Staging has its own accounts. |
| Sign-in: "could not sign in" plus a sentence | The Worker refused the request itself: the sentence says which (old Studio build, wrong address). |
| Sign-in: "did not accept this identity's signature" | The signature did not match this address: Studio and the Worker disagree on the API address. |
| Publish: nothing in the log after the toast | Studio was not signed in properly, or the Space was still opening. Check the log for "Unauthorized" or "still opening". |
| Publish: the log says "No changes" | Something was published from this Universe before; the copy should have no listing id. |
| Review stays "pending" and nothing moves | The upload finished but the review request did not (known gap). Tell the Website session. |
| Listing shows OFFLINE after F9 | The heartbeat did not go out: see the log hint (the two variables, or Studio not signed in as the author), or the listing is not approved yet (an unapproved listing shows LIVE to its author only). |
| Player: blank window | Read the Player log (Step 7). |
| Player: "host refused the join" | The Player was not signed in, or its login file is for another API. |

To make your staging account an admin (only if the AI holds the listing for a person), the
orchestrator or you runs one write on the staging store: key `admin:<your account id>`, value `1`,
in the USERS namespace of `eustress-api-staging`. That is a change to staging data, so it needs your go.

---

## What is known to break today, and who fixes it

Read from the code on 2026-10-01 and spot-checked; the first three are why this test waits for build 17.

| # | Link | Problem | Owner | Build 17? |
|---|---|---|---|---|
| 1 | Sign-in | Studio signs the bare challenge, so a login-v1 Worker answers 400; and a failed sign-in still shows "Signed in" with the public key as the token, so publish then answers 401 with no visible error | Multiplayer (Studio), Website (Worker, written) | Yes, in Multiplayer's plan |
| 2 | Player's login | Identity sign-in never wrote the login file the Player reads; the Player then has no ticket and the host refuses it | Multiplayer | Yes, in the same plan (it saves the token per API) |
| 3 | Worker on staging | The staging Worker predates login v1 and the registration fixes | Orchestrator (deploy) | Needed first |
| 4 | Publish result | Nothing shows the publish result in Studio: the dialog closes at once and no code reads `PublishProgress`; errors go to the log only. The first publish's review is not shown until the dialog is reopened | Publish dialog owner (`publish.slint`, `file_event_handler.rs`, `moderation_dossier.rs`) | Book |
| 5 | Review request | The publish state is saved before the review is requested, so if Studio closes or the dossier fails, a republish says "No changes" and never asks again. The Worker side is fixed in the working tree (not deployed): the sweep now resumes a listing stuck at `pending` with stored content after 15 minutes, on every hourly tick, up to 3 tries; with no dossier it is held for a person, never approved | Studio: Multiplayer (`echk_publish.rs`, writes the state before the request). Worker: Jev (done, `moderation.mjs`, `index.js` cron) | Studio part: book |
| 6 | Player on this machine | The `eustress-player://` link had no handler until an installer existed (none has been released; no `player-v*` tag). Client's build 17 registers it at run time, per user; not yet in the working tree when I checked, so confirm it landed | Client | Yes, in Client's build 17 |
| 7 | Player failures | A bad link or a refused join shows nothing: the window stays empty and the reason is in a log file | Client | Book |
| 8 | Host heartbeat | Needs `EUSTRESS_HOST_LAN=1` and `EUSTRESS_HOST_PUBLIC`; the hint is in the log only; an unpublished Space gives no message; quitting while hosting sends no stop, so the listing stays LIVE up to 90 seconds; admission is switched on before the first heartbeat succeeds, so a refused heartbeat still blocks every join with "could not confirm" | Multiplayer | Book |
| 9 | Listing id and site address | The listing id in `sync.toml` is not tied to an API, so a later production publish of this Universe fails on its first call; "Open on eustress.dev" always opens the production site | Multiplayer | Book |
| 10 | Publish dialog | No guard against a second click before the first publish finishes (it can create a second listing); the thumbnail is a window screenshot with Studio's own chrome and upload errors are swallowed; the size estimate is blank | Publish dialog owner | Book |
| 11 | Guests | Guest tickets are not built, so only signed-in accounts can join a listed session | Website and Multiplayer | After |
| 12 | First live run | The review's judge step (screenshots plus the Guardian policy) has never run end to end on any deployment | Jev, Website | This test is the run |

Ready now on the Website side: the login-v1 Worker and website sign-in (item 3's other half), the review
panel, the gallery card and listing page with the LIVE badge (all in the staging site build of 2026-09-30 22:16, checked by
strings in the build).
