# Multi-Claude Orchestration — One Tree, One Compile, Shared Memory

**Status:** Living design. Synthesized from two independent agent-workflow analyses — a broad orchestration design with an adversarial forecast, and a focused shared-build coordination design — which converged on the same spine. Where they diverged, this doc records the resolution.
**Owner:** Agent orchestration.  **Last revised:** 2026-06-01.
**Target:** N Claude agents working the same repository share **one** working tree and **one** build-output (`target/`) directory, never starve each other's build, always learn of each other's edits, and route every compiler error to the agent that caused it — with **no daemon** and **no per-agent disk bloat**.

---

## 0. Read this first — the one idea the obvious fix gets wrong

The obvious fix for "N agents keep blocking each other's builds" is to give each agent its own build-output directory (e.g. a per-agent `CARGO_TARGET_DIR`). **That is the wrong fix, and it makes things worse.** It was floated, rejected on review, and the adversarial forecast confirmed why:

> **The bottleneck is not compile *time*. It is *concurrent build processes contending one `target/`*.**

A per-agent target directory does not remove that contention — it relocates it to disk and recompiles the identical large monolith N times. Worse, pointing N **divergent** trees (e.g. git worktrees) at one shared build-output directory is the pathological case: the build cache is keyed on *the content of the tree it compiles*, so two divergent trees produce two fingerprints that **mutually invalidate each other's artifacts** (→ endless recompile) *and* **contend the one target lock** (→ `Blocking waiting for file lock on build directory`) **simultaneously**. This was caught live: one such worktree's build directory held only `.rustc_info.json` + `CACHEDIR.TAG` — a fresh empty target about to recompile the whole project from scratch. An observed ~110-minute lock starvation and ~8 concurrent compiler processes are the direct, predictable consequence of N trees each able to launch its own monolith build.

The correct architecture is the inverse:

> **One shared working tree, one shared `target/`, and an orchestration layer that guarantees exactly one build process touches that target at a time** — coalescing every agent's request onto that single build, and using a dedicated subagent to find the errors and route each to its owner.

The subagent's job is to **find errors, not bloat disk by compiling N times.** That sentence is the whole design in one line.

---

## 0.5 Design invariants (non-negotiable)

Four invariants gate every mechanism below.

### O1 — One shared compile, always
One tree, one `target/`, no per-agent build-output override anywhere. Every build inherits the shared incremental cache (incremental builds + a bounded parallelism cap, set once in the workspace's build config). The same code compiles **once** and is reused. Git worktrees are retained *only* as an explicit "fork off and opt OUT of the shared compile" escape hatch (a risky isolated refactor), which knowingly pays its own full build cost.

### O2 — Disk is the source of truth; no daemon
Every agent runs its own stdio MCP server process (single-threaded, per-client), so an in-memory semaphore in one process cannot coordinate the others. Authoritative state lives **on disk** under a shared state directory, exactly like the application's existing port-discovery file (a small on-disk file + PID-staleness discovery) and a prior scheduled-tasks lock file (`{sessionId, pid, acquiredAt}`). **A daemon is rejected for the exact reason an observed multi-hour zombie build happened:** the agent harness orphans background handles (a stop request loses the PID while the process lives), so any long-lived owner-of-truth is the thing that orphans and then lies. A directory of small JSON files survives every agent dying and is inspectable with `cat`.

### O3 — Ask before killing; be honest about ownership
No agent kills another agent's process (build, app, server) without an ownership check and — for anything not provably stale — explicit human assent. Every process records `{pid, sessionId, startedAt}`. Liveness/stale-reclaim copies the port-file PID rule; anything still live and not stale is **someone else's**, and the honest move is to say so, not to reap it.

### O4 — No error ever silently vanishes
A shared build sees everyone's merged edits. Every diagnostic must be attributed to an owner or, failing that, surfaced to **all** active agents as unattributed. "Is my file in the error set?" must be answerable deterministically, never by eyeballing a merged error dump.

---

## 1. Three layers, one on-disk state machine

The two designs are not competing systems — they are layers of the same disk-backed state machine. A third layer (context memory) extends the same substrate.

| Layer | State dir | Role | Status |
|---|---|---|---|
| **A · Substrate** | `.orch/` | Agent registry (OS-process-table-as-truth), file-scope claims / CODEOWNERS, ask-before-kill ownership, PM task designation, semantic-gate merge | designed |
| **B · Shared build** | `.claude/build/` | Coordinator (single-build semaphore + coalescer), **BuildScout** subagent (one build → JSON parse → attribute → route), staleness keys (`edit_seq`/`tree_hash`), subscription delivery | designed — **kills the #1 painpoint** |
| **C · Context-Ring Memory** | `.claude/rings/` | Context rings held in parallel + a ledger; a larger model converges two agents' contexts into one | concept — Layer-1 buildable today, Layer-2 needs model-serving support |

All three share the same spine: **on-disk JSON as truth, no daemon, lock-*directory* (atomic `mkdir`), PID-liveness reclaim copied from the application's port-file pattern.** Layer B is the load-bearing layer and the rest of this doc weights it accordingly.

---

## 2. Layer A — the substrate (`.orch/`)

The substrate answers "who else is here, and what are they touching?" — the awareness an isolated agent lacks today.

- **Agent registry** — each agent registers `{sessionId, pid, startedAt, cwd, lane}` on startup and refreshes a heartbeat. **The OS process table is the tiebreaker for truth:** a registry entry whose PID is dead is stale regardless of what the file says (inspecting the process table gives the command line + parent PID — the same diagnostic that distinguished a finishing build from a zombie during the multi-hour incident). Startup ritual: read the registry, reconcile against the process table, *then* act.
- **File-scope claims / CODEOWNERS** — an agent declares the files/crates it owns for the duration of a task. The recommended operating mode is **claims-enforced**: hold a claim to edit a file. This is what makes Layer B's error attribution crisp (see §3.5) and is the concrete form of "stay in your lane."
- **Ask-before-kill ownership** (O3) — the process-ownership table plus the assent protocol. An agent may reclaim only what it can prove is stale by PID; everything else is reported, not reaped.
- **PM task designation** — one agent may be designated coordinator for a multi-agent push (assigns lanes, owns the merge order). Optional; the substrate works leaderless.
- **Semantic-gate merge** — convergence of N agents' work is gated on a semantic check (build-green + owned-tests-green for the touched crates), not just a textual merge.

The substrate is leaderless-by-default and daemon-free: it is a directory of small JSON files under `.orch/`, reconciled against the live process table on every read.

---

## 3. Layer B — the shared-build coordinator (the meat)

This layer is the direct answer to the stated #1 painpoint: *"the agent builds after every sequential edit instead of batching."* The fix is a Coordinator that coalesces N requests onto one build, plus a BuildScout subagent that runs that one build and routes the errors.

### 3.1 Canonical state tree

Standardized on `.claude/build/`:

```
.claude/build/
  coordinator.json          # THE source of truth: state machine + monotonic edit_seq + edit-log + build registry + queue
  coordinator.lock/         # lock-DIRECTORY (atomic mkdir == acquired); holds owner.json {pid, sessionId, acquiredAt}
  ownership.json            # file -> [agentId] routing table
  claims/<agentId>.json     # live file-claims per agent (the attribution disambiguator)
  edits/<agentId>.json      # each agent's last-declared edit fingerprint {lastFlushedSeq, treeHash, files[]}
  builds/<buildId>/
    request.json            # INPUT contract handed to BuildScout (frozen ownership snapshot)
    raw-build.jsonl         # raw build --message-format=json stream (append-only; audit)
    scout.lock              # subagent liveness {pid, heartbeat_at, currently_compiling}
    verdict.json            # global pass/fail + per-crate table + counts + timing
    routing.json            # attribution manifest (who got which diagnostics + unattributed bucket)
    agent/<agentId>.json    # THE per-agent slice (one Read replaces the manual grep)
  inbox/<agentId>.jsonl     # zero-dependency push fallback: one line per delivered result
```

`coordinator.lock/` is a **lock-directory** because `mkdir` is atomic on common filesystems, whereas an exclusive-create on a single file is racy on some of them. It guards only the **millisecond** read-decide-write critical section on `coordinator.json` — **never** the duration of a build. Stale-lock reclaim copies the port-file rule: if `owner.json.pid` is dead, or `acquiredAt` exceeds `STALE_LOCK_CEILING = 60 min` (above the ~49 min worst-case loaded build so a live long build is never stolen, well under the ~110-min pathology), any agent may break it. The state dir resolves from the **workspace root**, never a user-documents folder — the same cloud-sync-avoiding rule the application's port file already follows (a real bug class when the workspace lives under a synced folder).

### 3.2 The flow (agent edits → result delivered)

```
[Agent A edits a file]
   │  build.declare_edit(A, [path])              ← bumps edit_seq, records fingerprint + claim
   ▼
[Agent A requests a build]
   │  build.request(A, ["build","-p","<primary-crate>"])
   ▼
┌──────────── COORDINATOR (under coordinator.lock/, ~ms) ────────────┐
│  read coordinator.json ; decide via §3.3 policy:                   │
│    • idle/done            → START: new buildId, stamp cut_at_seq   │
│    • running + covers A    → JOIN: append to queue (no new build)  │
│    • running, A newer & in-scope → KILL-vs-WAIT → RESTART          │
│    • running, unrelated crate → QUEUE (serialize; never 2nd build) │
│  write coordinator.json ; release lock                             │
└────────────────────────────────────────────────────────────────────┘
   │  (only on START/RESTART)
   ▼
[Coordinator spawns ONE BuildScout subagent with buildId]
   ▼
┌──────────────────── BUILDSCOUT (one build, terminal) ──────────────┐
│  read request.json ; write scout.lock{pid, heartbeat}             │
│  run ONCE: cargo build -p <primary-crate>                          │
│            --message-format=json-diagnostic-rendered-ansi          │
│            --locked --keep-going             (shared target/)      │
│  stream stdout → raw-build.jsonl ; parse compiler-message records  │
│  attribute each error → agent (§3.5)                               │
│  compute clean-but-blocked from build-metadata dep graph           │
│  write verdict.json, routing.json, agent/<each>.json (atomic)      │
│  set coordinator.json state=done + result.success ; exit           │
└────────────────────────────────────────────────────────────────────┘
   │  file writes under the state dir fire the existing MCP watcher
   ▼
[Waiting agents notified]
   │  MCP: resources/updated → resources/read build://latest?agent=A
   │  fallback: tail .claude/build/inbox/A.jsonl
   ▼
[Agent A reads agent/A.json] → {freshness, my_errors[], rendered}, one Read, no grep
```

The decision section is short and serialized; the build is long and runs outside any lock. Concurrent `build.request`s hit the locked section one at a time: the first STARTs, the rest JOIN the same `buildId` (if covered) or QUEUE. Net effect: **one build that covers all current edits, never a second concurrent build against the target.**

### 3.3 When to (re)start — the policy

**Two staleness keys:**

- **`edit_seq`** — one monotonic `u64` in `coordinator.json`, bumped on every `build.declare_edit`. Total order; the cheap hot-path key. *"Build cut at seq 140; my edit became seq 152 → I'm not covered."*
- **`tree_hash`** — content hash over the build-relevant source set `(path, mtime_ns, len)` for source files, manifests, the lockfile, build scripts, and the build config. NOT a whole-tree hash (too slow on a large monolith). The integrity net for edits that bypassed `declare_edit` (a stray tool edit, an external `git pull`).

A result is **FRESH for an agent** iff `result.cut_at_seq >= agent.lastFlushedSeq` **AND** `result.tree_hash == current_tree_hash()`. A newer `edit_seq` but an **equal** `tree_hash` (edits were no-ops/reverts) is still fresh → **no needless restart**.

Fingerprints are **crate-scoped.** A build's `scope` (from a build-metadata query that resolves every package id → manifest path) lists the crates it recompiles. An edit confined to an unrelated crate does not stale a build scoped to the primary crate.

**Decision rule (runs under `coordinator.lock/`):**

```
on build.request(req):
  acquire coordinator.lock/        (break if owner pid dead OR acquiredAt > STALE_LOCK_CEILING=60min)
  rec = read coordinator.json (or {state: idle}) ; defer release
  myScope = crates_touched(req)    # via build-metadata
  myTree  = current_tree_hash() ; mySeq = current_edit_seq()

  (a) state in {idle, done}                         → START(cut_at_seq=mySeq, scope=myScope)
  overlap = intersect(rec.scope, myScope)
  (e) overlap empty                                  → QUEUE   # serialize; never a 2nd build
  (b) rec.cut_at_hash==myTree OR rec covers overlap  → JOIN    # coversYou=true, no new build
  (c) NEWER in-scope edit landed after cut → stale:
        if within EDIT_QUIESCENCE window  → DEBOUNCE: flag stale, schedule ONE restart, JOIN(coversYou=false)
        elif remaining <= KILL_FLOOR(90s) → LET-FINISH, queue restart, deliver STALE_OLDER_EDIT
        else                              → KILL rec.builderPid + RESTART
```

**Kill-vs-wait reasoning:** if my edit is *in the build's scope*, waiting yields a result that *excludes* my edit — I'd rebuild after anyway, so stale-wait cost = `remaining + full_rebuild`, never cheaper than restarting now. Therefore **in-scope newer edit ⇒ KILL+RESTART**, except a build within `KILL_FLOOR (90s)` of done is allowed to finish (discarding near-complete codegen to rebuild a tiny delta is the worse trade); its result is delivered honestly as `STALE_OLDER_EDIT`.

**Anti-thrash gate (this is what prevents reintroducing the #1 pain):** the `EDIT_QUIESCENCE = 8 s` debounce means edits arriving in a flurry **extend the timer and coalesce into ONE restart**, not N kills. Killing on every keystroke-level edit *is* "building after every edit is ruinous." The coordinator only *flags* `stale` + bumps `edit_seq` per edit; the successor launches once, when edits settle.

**Tunables (all in `coordinator.json`, auditable):** `EDIT_QUIESCENCE=8s`, `KILL_FLOOR=90s`, `STALE_LOCK_CEILING=60min`, `SCOUT_HEARTBEAT_DEAD=90s`, `BUILD_WALLCLOCK_CAP=60min` (hard self-kill — no multi-hour zombie), `MIN_REMAIN` (eta divide-by-zero guard).

### 3.4 The BuildScout subagent

Spawned once per build (the agent-SDK subagent mechanism). Self-contained prompt — the spawned session has no memory of the requester:

```
cwd: <workspace root>
1. Read .claude/build/builds/<buildId>/request.json → buildId, command, scope, ownership.
2. Write scout.lock {pid, heartbeat_at}. If coordinator.json points at a NEWER buildId → verdict.status="superseded", exit (don't burn a 10-20m compile).
3. Run ONCE (shared target/, NO per-agent build-output override):
   cargo build -p <primary-crate> --message-format=json-diagnostic-rendered-ansi --locked --keep-going
   tee stdout → raw-build.jsonl ; heartbeat scout.lock every 10s with currently-compiling crate.
4. Parse reason==compiler-message ; dedup on (code,file,line,col,message) ; group by spans[].file_name + package_id.
5. Attribute each error per §3.5 against the FROZEN ownership snapshot ; compute clean-but-blocked from build-metadata dep graph.
6. Write verdict.json, routing.json, agent/<each>.json ATOMICALLY (tmp+rename). Set coordinator.json state=done, result.success. Exit.
Return value: path to routing.json (consumed by the parent, not narrated).
```

The build flag set is load-bearing:
- `--message-format=json-diagnostic-rendered-ansi` — structured `spans[]` for attribution **plus** the verbatim `rendered` text for the owning agent.
- `--locked` — N agents on one tree must not let a stray build rewrite the lockfile mid-flight (a second contention source).
- `--keep-going` — compile every independent crate even after one errors, so one agent's broken crate does **not** blank the error list for everyone (the build-tool half of "broken crate blocks all").
- **A single-crate `-p <primary-crate>` build, not a whole-workspace build, and a real `build`, not `check`** — the canonical build is the project's chosen single-crate build alias at its default feature tier; never `check` (the user-facing build must be a real compile + link), and avoid feature flags known to break the platform's linker. One bounded job pool for the whole machine, because the coordinator guarantees one build.

**Build it as a standalone binary first** so it's testable against a canned `raw-build.jsonl` from a real failing build + a hand-written `ownership.json`, with the 10–20 min compile *out* of the test loop.

### 3.5 Error → agent attribution (O4)

The hard question: *one shared build sees everyone's merged edits — whose error is it?* Per diagnostic, first match wins:

```
P = primary_file (is_primary span) ; S = all span files ; dirty = files changed since last green
1. CLAIM WINS.    owners = claims_for_file(P) ∩ {agents with edits since last green}
                  |owners|==1 → that agent (reason="owns-primary-span")
                  |owners| >1 → ALL of them (verdict shared-file-conflict)
2. RECENT-EDIT.   P in dirty → last_editor_of(P)            (reason="primary-span-last-editor")
3. BLAST-RADIUS.  P unowned, a CHANGED file in S → editor of argmax_seq(changed∩S) (reason="downstream-break")
4. CRATE OWNER.   no span file owned → crate-owner          (reason="owns-crate")
5. UNATTRIBUTED.  nobody → routing.unattributed + flagged to ALL active agents (reason="no-owner-in-map")
```

**Why file-claims are load-bearing, not decoration:** the shared tree means the compiler physically cannot tell two agents' edits in the same file apart — it sees only the merged file. The *only* disambiguator is metadata the build lacks: who claimed/last-edited which file. Clean claim ⇒ exact attribution (Rule 1). Same file, no claim, two editors ⇒ the system **refuses to guess**: marks the diagnostic `CONTESTED`, delivers it to **both** with the edit-log slice showing both edits, surfaces it as a coordination problem. Hence the recommended **claims-enforced** mode (§2), which collapses the contested case to near-never.

**The "0 errors in my files but my crate failed" case** (a co-agent's broken crate blocking everyone) is solved by two mechanisms together: `--keep-going` compiles every still-compilable crate so each agent's own errors surface; then BuildScout computes blocking from the build-metadata intra-workspace dep graph — any crate with 0 errors that **never emitted an artifact** and transitively depends on a failed crate is marked `blocked`, and its owner gets verdict `clean-but-blocked` with `blockingMe = [owners of the crates that blocked them]`. The innocent agent is told *which crate* and *who owns it* — it waits or pitches in rather than hunting a phantom error.

### 3.6 Result delivery

Reuses the existing MCP subscription pipeline. Add a Build URI kind to the server's URI module and resolvers in its resources module:

```
build://<buildId>            → full result doc (markdown-rendered)
build://latest?agent=<id>    → latest result PRE-FILTERED to that agent's errors + freshness
build://verdict              → global verdict.json
```

The MCP server's file-watcher / subscription manager already fires `notifications/resources/updated` on file changes; resource subscription is already advertised and wired. Extend the path→URI mapping so a write to `.claude/build/builds/<id>/agent/<agentId>.json` maps to `build://latest?agent=<agentId>`, and add the Build arm to `read_resource`. A waiting agent subscribes, blocks (no lock held, no polling), and reads on the `updated` push. **Zero-dependency fallback (no MCP):** `.claude/build/inbox/<agentId>.jsonl` — the coordinator appends one line per result; an agent tails its own inbox. This is the disciplined replacement for ad-hoc result-file polling.

**Per-agent slice** (`agent/<agentId>.json`) — the anti-grep payload, one Read:
```jsonc
{ "buildId":"b-…", "treeHash":"sha256:9f3a…", "cutAtSeq":140, "for":"agent-A",
  "verdict":"your-files-broke-the-build", "freshness":"FRESH", "compileSucceeded":false,
  "myErrors":[ {"code":"E0599","file":"<path>","line":42,
                "message":"no method named `foo`","rendered":"error[E0599]…","reason":"owns-primary-span"} ],
  "blockingMe":[], "global":{"errors":3,"warnings":11,"cratesFailed":1},
  "summary":"3 errors in files you own. The build is red because of your edits." }
```
The `verdict` enum is the headline an agent acts on without reading further: `clean` · `your-files-broke-the-build` · `clean-but-blocked` (your crate never compiled because someone else's failed — not your fault) · `shared-file-conflict` (an error in a multi-owner file — coordinate).

### 3.7 The MCP tool surface

Thin, stateless verbs over `coordinator.json`, registered in the MCP server's tool registry (the same registration pattern as the server's other tools). Because state is on disk, all N server processes act on the one file.

- `build.declare_edit {agentId, paths[]}` → `{editSeq, treeHash, scopeCrates[]}` — bump + record fingerprint + claim. (Wire to a post-tool-use hook on edits.)
- `build.request {agentId, command?, waitMs}` → `{decision: started|joined|restarted|queued, buildId, coversYou, etaHint, subscribeUri}` — the §3.3 scheduler entrypoint.
- `build.status {buildId, agentId, blockMs}` → `{state, freshness, coversYou, elapsedMs, supersededBy}`.
- `build.result {buildId, agentId}` → the per-agent slice (PRE-FILTERED).
- `build.cancel {buildId, agentId, reason}` → cooperative; record-owner or quorum can force-kill (orphan reaper, O3).

Each opens `coordinator.lock/` only for the ms-long mutate/read. **All long waits happen outside the lock** via subscription or polling — never spinning on the lock. A zero-dependency shell front-end (`bc-flush`/`bc-status`/`bc-result`/`bc-wait`) operates the same files under the same lock so MCP-less agents participate.

### 3.8 Why this keeps one shared compile

The ~110-min lock starvation dies **by construction**, with no per-agent target dirs and no N-times recompile, because of a single guarantee: **there is never a second concurrent build against the shared `target/`.**

- The locked decision section admits exactly one START. Every other concurrent request JOINs (covered → no new build), QUEUEs (unrelated → serialized), or extends the debounce (newer edits → one coalesced restart). The build tool's exclusive target lock is therefore never contended by a peer.
- One shared `target/`, no override → the same code compiles once and is reused.
- The established "kill in-flight on source change" practice is **centralized into the coordinator** and done **once** per stale event (gated by the 8 s debounce), instead of N agents killing each other into a herd.
- The multi-hour zombie is foreclosed three ways: `scout.lock{pid, heartbeat}` PID-liveness reclaim; a heartbeat staler than `SCOUT_HEARTBEAT_DEAD=90s` is reaped; a hard `BUILD_WALLCLOCK_CAP=60min` self-kill bounds any wedged compile. Because truth is files, an orphan is recoverable from disk — the exact gap the lost-handle failure exposed.

---

## 4. Layer C — Context-Ring Memory

The motivating intuition: *hold context rings in parallel, dynamically, instead of a series operation that compacts every time it fills; swap a cache against the token window; a bigger model summarizes two agents' contexts into one; a ledger of rings dynamically invoked.* This *"solves the history problem and the converging-agents problem."*

**Right diagnosis, one wire correction.** The model's internal attention (key/value) cache cannot be the awareness/sharing channel:

> The attention cache is **opaque per-sequence tensors, ephemeral, and walled off per request.** It is the wrong payload for cross-agent awareness or for "list what another agent is holding." Prompt caching is a short-TTL *prefix* cache, not an addressable, shareable memory.

The *architecture* is sound; it maps onto known systems and splits cleanly into a layer we can build and a layer that needs model-serving support:

- **Layer-1 (buildable today):** a **semantic agent-state digest** — a compact, structured summary of what each agent is holding (active task, claimed files, open questions, last conclusions), published as an **MCP resource** and **subscribed** to by peers. This is "context rings as a ledger, dynamically invoked," realized on the *exact same substrate* as Layer B (`.orch/` + the subscription pipeline). A larger model summarizing two agents' digests into one shared ring is a straightforward agent call over these digests. This is the MemGPT/Letta "LLM OS" pattern (paged memory + a ledger of what's resident) done with files + MCP.
- **Layer-2 (needs model-serving support):** the literal "hold N rings in parallel and swap them against the token window" is **paged-attention serving** — PagedAttention-style key/value paging + attention-sink eviction (à la StreamingLLM: evict the middle, keep sinks + recent). That lives in the model-serving layer, not in user space, and is the substance of the awareness RFC.

The honest split: build Layer-1 now; propose Layer-2 to the model provider.

---

## 5. Asks → the mechanism that answers each

Direct mapping from the original requirements to the design:

| Ask | Mechanism |
|---|---|
| init / checks before action | Coordinator + agent-registry read on startup, reconciled vs the process table (§2) |
| a tool to queue a summary into a running agent | `inbox/<agentId>.jsonl` + `resources/updated` push (§3.6); agent-state digest (§4) |
| list active agents | `.orch/` registry, validated against the live process table (§2) |
| stay in lane | file-claims / **claims-enforced** edit mode / CODEOWNERS (§2, §3.5) |
| ask before killing + honesty about who started what | process-ownership table + assent protocol; `scout.lock{pid, sessionId}` (O3) |
| background-task state management | on-disk task registry that **survives orphaning** — the multi-hour zombie fix (O2) |
| **#1: batch builds instead of building after every edit** | the **coalescer + 8 s `EDIT_QUIESCENCE` debounce**: requests JOIN/QUEUE one `buildId`, flurries collapse to ONE restart, BuildScout is the "find errors, not bloat disk" subagent (§3.3, §3.4) |
| edit-dependency-graph | build-metadata crate-scope + blast-radius attribution (§3.3, §3.5) |

---

## 6. The bigger lever — decomposition > orchestration

The adversarial forecast's headline, recorded here so it is not lost:

> **Orchestration *manages* contention on a 10–49 min monolith build. It does not *reduce* it. The single biggest real lever is decomposing the monolithic primary crate so an edit recompiles a leaf, not the world.**

The monolithic primary crate is why a build is 10–49 minutes; that is the thing being contended. Splitting it (its major subsystems into separately-compilable crates with thin interfaces) shortens every build for every agent and shrinks the blast radius of every edit — which *also* makes Layer B's crate-scoped staleness and attribution far more precise. The two are complementary and ordered: **decompose to shrink the build; orchestrate to make the remaining contention survivable.** Do both. Decomposition is the larger win.

A second forecast note: **a busy-alive lease.** An agent waiting 20 min on a build must heartbeat its wait, or the harness reaps the handle while the PID lives (the orphan gap again). The subscription wait (§3.6) must hold a lease, not block silently.

---

## 7. Roadmap (smallest-viable-first; headline pain dies in Phase 1)

**Phase 1 — Serializing semaphore + coalescer (kills the lock starvation).**
`coordinator.json` + `coordinator.lock/` (lock-dir, PID-reclaim, atomic tmp+rename), monotonic `edit_seq` + `tree_hash()`, and `build.request`'s START/JOIN/QUEUE core (cases a, b, e) — **without** kill-restart and **without** a subagent yet (every build just "lets finish"). Register `build.declare_edit`/`request`/`status` in the MCP server's tool registry as pure file-op entries (no app dependency). Ship the `bc-*` shell front-end too.
- *Unlocks:* serialization alone eliminates lock-thrash and zombie-pileup — one build at a time even if nothing is ever killed. The covers-check removes the most common waste (N agents asking for the same build → they JOIN).
- *Validate:* spawn 3+ concurrent `build.request`s; confirm exactly one build process runs, the others report `joined`/`queued`, `coordinator.json` shows one `buildId` + populated `queue`. `Blocking waiting for file lock` must not appear.

**Phase 2 — BuildScout subagent + JSON attribution (kills the manual grep).**
Wire the `idle→running` winner to spawn BuildScout. It runs the one build with the §3.4 flags, parses, runs §3.5 attribution + the clean-but-blocked dep-graph pass against the frozen ownership snapshot, writes `verdict.json` + `routing.json` + `agent/<id>.json`. **Standalone binary first**, tested against a canned failing-build jsonl + a hand-written ownership map.
- *Unlocks:* each agent reads one file instead of eyeballing the merged error list; `--keep-going` + clean-but-blocked means a co-agent's broken crate no longer blanks everyone's report.
- *Validate:* feed a saved failing-build jsonl + a 3-agent ownership map; assert each `agent/<id>.json` holds only that agent's errors and the correct `verdict` (including a deliberately-induced `clean-but-blocked` and a `shared-file-conflict`).

**Phase 3 — Subscription delivery + the kill-restart cost model (refinements).**
Add the Build URI kind, extend the path→URI mapping + the watcher to push `build://latest?agent=<id>` on result write (the inbox fallback already shipped in Phase 1). Then layer in the `EDIT_QUIESCENCE` debounce + the §3.3 KILL-vs-WAIT cost model (case c) + the `freshness` verdict.
- *Unlocks:* push-on-done (no busy-poll); the in-flight binary always matches current source without per-agent kill herds.
- *Validate:* subscribe agent A, land an in-scope edit mid-build, confirm exactly ONE coalesced restart fires after the 8 s window, the superseded result is delivered `STALE_OLDER_EDIT`, and A gets a `resources/updated` push the instant the fresh `agent/A.json` lands.

**Phase 4 (parallel track) — monolith decomposition (§6).** The larger lever. Independent of Phases 1–3 and compounding with them.

**Layer-A substrate** and **Layer-C context rings** layer on once Phase 1's state-dir + subscription conventions exist.

Phases 1–2 are implementable today from a standard MCP tool-registry surface, the agent-SDK subagent spawn, a build-metadata query + `--message-format=json`, and the on-disk lock + PID-staleness conventions a typical app already has — **no new daemon, no per-agent target dirs.**

---

## 8. Integration points (where this attaches, described by role)

No file paths — these are the abstract touchpoints in any MCP-server + build-tool codebase:

- **MCP tool registry** — register the `build.*` tools (the same registration pattern as the server's other tools).
- **MCP request dispatch + resource subscription** — already advertises `resources.subscribe`; add the `build.*` dispatch arms.
- **MCP URI + resource resolvers** — add the Build URI kind, the `build://*` resolvers, and the path→URI mapping for the per-agent slice.
- **MCP file-watcher / subscription manager** — watch the `.claude/build/` state dir and emit `notifications/resources/updated`.
- **The application's existing port-discovery file** — the on-disk + PID-staleness + cloud-sync-avoiding root-resolution pattern the lock-dir reclaim copies.
- **The workspace build config** — the single shared target's settings (incremental, bounded job pool, the canonical single-crate build alias); never a per-agent build-output override.
- **NEW: a standalone BuildScout binary** — the error-finder subagent.
- **NEW: on-disk state trees** — the build-coordinator dir (`.claude/build/`), the substrate dir (`.orch/`), the context-rings dir (`.claude/rings/`).
- **Build-graph tooling** — the build-metadata query (package id → manifest path; intra-workspace dep graph) for crate-scope and clean-but-blocked attribution.
- **Anti-pattern this design collapses** — per-worktree target directories (each able to kick off its own monolith build → the contention this whole design removes).

---

## 9. What is designed vs. real

Everything in this document is **design**, synthesized and adversarially reviewed across two agent-workflow analyses — **none of it is implemented yet.** It is buildable from a standard MCP-server + build-tool surface (no new daemon, no per-agent target dirs); Phase 1 alone retires the headline pain. The two provider-facing artifacts this design implies — a background-task-state-management bug report (the harness orphaning, O2) and an MCP cross-agent-awareness / context-ring-serving RFC (§4 Layer-2) — are noted but not yet written.
