# Idle host preferences

Status: **design only.** This branch does not add a host badge, a preference
list, a demand signal, or an automatic start. No Worker deploy, and no
port, DNS, or firewall change.

## Spec

McKale, 2026-10-20.

Good hosts can choose which games they want to host more of, even when
they are idle. A good host is one with the **best host badge**. The badge
is earned by **uptime and quality**. The preference is what the host
chooses to spend that badge on.

When a preferred game has demand and the host is idle, the Forge server
starts that game on its own, so the listing stays warm. The host does not
have to be sitting in Studio with Play pressed.

The badge and the preference are different things. Uptime and quality earn
the badge. The preference only chooses which listing that badge is used
for. A host with no badge does not get to set a preference that Forge will
act on. A host with a badge and no preference is not started for a game
they did not choose.

## What this tree already has

None of the following is a badge, a preference, or an idle start.

**A listing is live only while a host heartbeats.**
`infrastructure/cloudflare/api/src/live.mjs` stores one `host` record.
`HEARTBEAT_TTL_MS` is 90 seconds. `LiveHost.alarm` deletes that record when
the deadline passes. `POST /api/simulations/{id}/live` is how Studio (and,
on this branch's unfinished server path, a dedicated server) marks the
listing live. That record is "someone is hosting now." It is not an uptime
score, and it is not a quality score. A search of this tree found no host
badge.

**Forge does not start a game because a host is idle.**
`eustress/crates/forge` wraps `forge-orchestration` 0.6.0.
`Forge::submit_job` forwards a job to Nomad only when a Nomad client was
configured. The control plane's `POST /api/v1/jobs` stores the job in the
process. It does not spawn a process. There is no preference table on a
job, and no idle check. `GameServerJob` (`eustress/crates/forge/src/jobs.rs`)
names an experience, a region, and a player cap. It does not name a host's
chosen games.

**One listing, one host record.** The live Durable Object keeps a single
`host` key. Several preferred games on one machine, each warmed while the
host is idle, needs one job per game. That jobs model is the same one host
migration and world shards already point at
(`docs/host-migration-design.md`, `docs/world-shards-design.md`). This
document does not add it.

## How the later branch would behave

Not built. The order is fixed so a preference cannot start a server for a
host who has not earned it.

1. **Measure uptime** from heartbeats the witness already sees, not from
   the host's own claim. A beat that arrives, and a beat that does not,
   are the only uptime facts the live registry has today. A quality fact
   is not in that record. What "quality" is (crashes, tick time, player
   reports) is **not specified**. No threshold for the badge is specified
   either. Both stay open until McKale names them.
2. **Award the badge** when those two are met. The badge is a property of
   the host's account, not of one listing. This tree has no such field.
3. **Let that account store a preference list**: listing ids they are
   willing to warm. Anyone else storing a list does nothing. The list is
   the host's choice. It is not inferred from what they played.
4. **Call the host idle** only when they are not already simulating a
   game. What "idle" means on the machine (no players, spare RAM, the
   dedicated process exited) is **not specified**.
5. **Call a game in demand** only from a signal the witness sees. A player
   opening a listing that is offline is the obvious candidate. No such
   counter exists on `GET /live` today (`{ live: false }` is the whole
   offline body). The demand rule is **not specified**.
6. **Start one Forge job** for that listing on that host, and only then.
   The job is the same dedicated server a person would have started by
   hand. The listing heartbeats and shows Live. When demand drops, or the
   host goes away, the job stops. Stopping rules are **not specified**.

Forge is the thing that decides to start. The game process does not poll
for demand itself. The orchestrator does not simulate the game. It only
submits the job, which is the same split world shards already use.

## What this must not do

- Do not start a game for a host who has no badge.
- Do not start a game the host did not put on the preference list, even
  if that game has demand and the host is idle.
- Do not treat the badge as a payment. Paying hosts is
  `docs/host-bliss-payouts-design.md`. The badge here is only permission
  to have a preference acted on.
- Do not trust the host's client to report its own uptime or quality into
  the co-sign endpoint. That endpoint already treats the body as a claim
  the witness must bound (`handleCosign` in
  `infrastructure/cloudflare/api/src/index.js`). A badge built from a
  self-reported duration would be the claim, not the measurement.
- No port, DNS, or firewall change. Staging only. No deploy from this
  branch.

## Open, on purpose

- The numbers that earn the badge (how much uptime, what quality means).
- The demand signal, and how long a warm listing stays up after demand
  falls.
- Whether one idle host may warm more than one preferred game at once.
- Whether "best host badge" is one tier or a ladder. McKale named the
  best badge as the one that unlocks the preference. Lower tiers are not
  described.
