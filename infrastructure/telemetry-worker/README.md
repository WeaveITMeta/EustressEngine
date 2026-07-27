# Usage-telemetry ingest Worker

Turns anonymous per-session tool-click aggregates into a demand-ranked wiring
backlog. Pairs with `crates/engine/src/usage_telemetry.rs` (the client).

**Status: written, not deployed.** Deploying needs a Cloudflare account and an
explicit `wrangler deploy` — an outward-facing action that belongs to the
operator, not to a build step. The engine works fine without it: telemetry is
local-only until this exists, which is exactly the intended launch posture.

## Why aggregates, not events

The client buffers clicks locally as JSONL and would upload **one summary per
session** — counts keyed by tool id. No per-click timing crosses the network,
so nothing here can be replayed into a behavioural timeline. Combined with a
random install UUID that is never joined to an Eustress/Bliss account, the
worst-case leak is "some anonymous install clicked Bail Calculator 3 times".

## Deploying (operator)

```bash
cd infrastructure/telemetry-worker && npx wrangler deploy
```

Then point the client at the resulting URL. The client-side uploader is the
remaining piece — `usage_telemetry.rs` currently writes local JSONL only and
has no network code at all.

## The query that matters

This is the whole point — it ranks the ~1,400 unwired "dream" buttons by real
demand, so wiring order stops being a guess:

```sql
SELECT
  blob1                        AS tool,
  blob4                        AS state,          -- 'wired' | 'dream'
  SUM(double1)                 AS clicks,
  COUNT(DISTINCT index1)       AS installs
FROM eustress_usage
WHERE timestamp > NOW() - INTERVAL '7' DAY
  AND blob4 = 'dream'
GROUP BY tool, state
ORDER BY installs DESC, clicks DESC
LIMIT 50;
```

`installs DESC` leads deliberately: fifty people trying a tool once is a far
stronger signal than one person clicking it fifty times.

## Local inspection without any of this

Every event is already on disk in readable form:

```
%LOCALAPPDATA%/Eustress/telemetry/usage-YYYYMMDD.jsonl
```

```bash
# top dream tools this machine has voted for
cat usage-*.jsonl | jq -r 'select(.wired==false) | .tool' | sort | uniq -c | sort -rn | head -20
```
