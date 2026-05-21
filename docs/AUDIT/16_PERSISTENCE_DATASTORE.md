# 16 — Persistence & DataStore

> Fjall WorldDb store, `DataStoreService`, `OrderedDataStore`, `ReplicatedStorage`, save data
> serialisation, atomic transactions, versioning. The **how state persists to disk** layer
> across backend ⇄ engine ⇄ client tiers. *Backend SQLite remains for content tables; the
> `eustress-worlddb` crate is the pluggable DataStore backend (2026-05-16, see MASTER C17).*

## Pass changelog

- **P3 (2026-05-14):** New doc; 10 features.
- **P4 (2026-05-14):** State correction from secondary critique: `DataStoreService` is **more than stubs** — struct + `get_datastore()` exists; `OrderedDataStore` has full BTreeMap-backed range-query logic; `DataStoreBackend` trait is pluggable (SQLite + Redis shaped). True state for Features 2/3/4 is **🟡 ~50%**, not 🔴 0%. The actual gap is **script API bindings** (Rune / Luau), not the underlying structs.
- **Updated 2026-05-16: storage pivot.** A real **`eustress-worlddb`** crate now exists, backing (or being wired to back) `DataStoreService` / `OrderedDataStore` via the pluggable backend on the **Fjall log-structured-merge-tree key-value database** (not just SQLite). It exposes `ds_get` / `ds_set` / `ds_remove` / `ds_update` / `ds_range` / `ds_set_sorted` plus Roblox-parity `DataStore` / `OrderedDataStore` / `DataStorePages` types. "Studio scene save via file-system-first" is superseded: live state lives in the Fjall WorldDb store; TOML is legacy/seed + human-editable schema. See MASTER C17.

---

## Concept summary

Persistence cuts across three tiers: **backend** (SQLite at `api.eustress.dev`, content tables, project listings, telemetry sink), **in-engine** (`OrderedDataStore` API mirroring Roblox's, used by scripts to persist game state — now backed by the Fjall WorldDb store via the `eustress-worlddb` pluggable backend), and **client-side** (`ReplicatedStorage` snapshot for hot-reload compatibility, per-user save data in `~/.config/eustress/saves/`). Each tier needs ACID semantics, versioning, and migration.

This is distinct from [04_ASSET_PIPELINE] (which persists static content) and [10_TELEMETRY] (which is event-stream, not key-value). DataStore is the Roblox-parity public scripting API for **player state, game progress, leaderboard scores, configuration**. *Updated 2026-05-16:* the `eustress-worlddb` crate is real — `DataStoreService` / `OrderedDataStore` are backed (or being wired) by the Fjall log-structured-merge-tree database via a pluggable backend (`ds_get`/`ds_set`/`ds_remove`/`ds_update`/`ds_range`/`ds_set_sorted`, plus Roblox-parity `DataStore`/`OrderedDataStore`/`DataStorePages` types). The remaining gap is the **Rune / Luau script API bindings**, not the underlying store.

---

## Implementation snapshot

**Crates / files:**
- [eustress-backend](../../eustress/crates/backend/) — Axum + sqlx + SQLite at `api.eustress.dev` (content tables)
- **`eustress-worlddb`** *(2026-05-16)* — Fjall log-structured-merge-tree key-value store behind the `WorldDb` trait (stock Fjall 2.11.2, no fork yet); `ds_get`/`ds_set`/`ds_remove`/`ds_update`/`ds_range`/`ds_set_sorted` + Roblox-parity `DataStore`/`OrderedDataStore`/`DataStorePages` types; pluggable DataStore backend
- `common::services::DataStoreService` — struct + `get_datastore()` exist; backend pluggable (SQLite / Fjall WorldDb / Redis-shaped)
- `OrderedDataStore` + `DataStorePages` — BTreeMap-backed range-query logic exists; Fjall `ds_range`/`ds_set_sorted` backing
- `ReplicatedStorage` — instance class; client-side snapshot path unverified
- [engine/src/serialization/](../../eustress/crates/engine/src/) — scene save/load (for editor undo/redo)
- [common/src/change_queue.rs](../../eustress/crates/common/src/) — change tracking (more about Telemetry than persistence per se)

**Working:**
- Backend SQLite for content (simulations, projects, marketplace, community)
- Editor undo/redo with in-memory waypoint history
- **Fjall WorldDb store is the authoritative live ECS state** *(2026-05-16)* — a faithful importer mirrors a Space's TOML tree into the `tree` partition on first open; TOML retained as legacy/seed + human-editable schema *(was "Studio scene save via TOML, file-system-first")*
- `eustress-worlddb` DataStore primitives (`ds_*`) + Roblox-parity types

**Stubbed / missing:**
- `DataStoreService::GetDataStore` **Rune / Luau script API bindings** (the underlying store now exists)
- `OrderedDataStore::SetAsync` / `GetAsync` versioning surfaced to scripts
- Per-user save game persistence ([01_CLIENT] gap 13)
- Atomic transactions across engine + backend
- Migration framework for schema evolution
- GC / TTL of expired keys

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Backend SQLite schema (content tables) | ✅ |
| 2 | `DataStoreService` script API | 🟡 *(2026-05-16: Fjall WorldDb backend + `ds_*` real; Rune/Luau bindings still the gap)* |
| 3 | `OrderedDataStore` (sorted KV) | 🟡 *(2026-05-16: `ds_range`/`ds_set_sorted` real in `eustress-worlddb`)* |
| 4 | `DataStorePages` cursor pagination | 🟡 *(2026-05-16: type exists in `eustress-worlddb`)* |
| 5 | `ReplicatedStorage` client-side snapshot | 🟡 |
| 6 | Per-user save data (`~/.config/eustress/saves/`) | 🔴 |
| 7 | Atomic transactions (multi-key write) | 🔴 |
| 8 | Schema versioning + migration | 🔴 |
| 9 | TTL / GC of expired keys | 🔴 |
| 10 | Studio scene save / load round-trip | ✅ *(2026-05-16: now Fjall WorldDb store; TOML legacy/seed + human-editable schema)* |

---

## Detailed per-feature cards (top 6)

### Feature 1 — Backend SQLite schema

**State:** ✅ · **Effort:** Done · **Risk:** Med (scale) · **Touches:** [06_WEBSITE], [09_ECONOMY], [16]
**Sub-features:** content tables (simulations, gallery, marketplace, community) · sqlx + SQLite · prepared statements · row-level constraints

**Concept.** Axum routes back onto SQLite via `sqlx`. Schemas live in `src/db/`. Content-only — no user-key-value APIs yet.

**Forecasted feedback (R)**
- R1.1 SQLite single-writer locks become a bottleneck at scale; plan PG migration path.
- R1.2 No explicit schema migrations tooling — `sqlx migrate` integration suggested.
- R1.3 Connection pool size default — verify under load.
- R1.4 No read replica strategy.

**Implications (I)**
- *Operational:* SQLite holds at 100s of writes/sec; beyond that, switch to PG.
- *Architectural:* moving to PG = sqlx-friendly; no schema rewrite needed.
- *Cross-system:* [09_ECONOMY] Bliss balance ledger will be the first heavy write workload.

**Risks (X)** — X1.1 Single-file SQLite corrupt = backend offline.

**Mitigations (M)** — M1.1 LiteStream replicate to S3; daily snapshot.

---

### Feature 2 / 3 / 4 — DataStoreService / OrderedDataStore / DataStorePages

**State:** 🟡 *(store real; script bindings the gap)* · **Effort:** L · **Risk:** Med · **Touches:** [02_STUDIO], [03_MULTIPLAYER], [16], C17
**Sub-features:** `GetDataStore(name)` script API · `GetAsync` / `SetAsync` / `UpdateAsync` / `RemoveAsync` · ordered variant for leaderboards · cursor-based pagination · per-script-key namespacing · *(2026-05-16)* `eustress-worlddb` `ds_get`/`ds_set`/`ds_remove`/`ds_update`/`ds_range`/`ds_set_sorted` on Fjall

**Concept.** *Updated 2026-05-16: storage pivot.* Roblox-parity scripting API. A script says `local store = DataStoreService:GetDataStore("PlayerData")` then reads / writes typed values keyed by user ID. Server-side replicates writes to a backing store — now the **Fjall WorldDb store via the `eustress-worlddb` pluggable backend** (`ds_*` primitives + Roblox-parity `DataStore`/`OrderedDataStore`/`DataStorePages` types are real). The remaining gap is the **Rune / Luau script API bindings** that expose these primitives to game scripts.

**Forecasted feedback (R)**
- R2.1 Today scripts can't persist anything; every restart is fresh. Big gap.
- R2.2 Conflict resolution: `UpdateAsync(key, fn)` is the Roblox CAS pattern; needs server-side compare-and-swap.
- R2.3 Quota: per-script, per-user, per-store — caps to prevent abuse.
- R2.4 Rate limits: write/sec, read/sec per store.
- R2.5 Encryption-at-rest for sensitive game state.
- R2.6 Studio "Play" mode should hit a local-only backing (not production) — gate with environment flag.

**Implications (I)**
- *Architectural:* this is the public game-state persistence contract — define the on-disk format once and version it.
- *Cross-system:* [03_MULTIPLAYER] needs server-side enforcement (only authorised user can write their key).
- *Operational:* every published game uses DataStore — must be production-grade or every game breaks.
- *Strategic:* core Roblox parity gap.

**Risks (X)**
- X2.1 Naive impl writes to backend on every `SetAsync` → backend overwhelmed.
- X2.2 Without quota, a runaway script fills the store and bricks projects.

**Mitigations (M)**
- M2.1 Per-key write-coalescing on server side (batch within 100 ms window).
- M2.2 Hard quota per store + per-user; surface usage in Studio Settings.

---

### Feature 5 — ReplicatedStorage client-side snapshot

**State:** 🟡 · **Effort:** M · **Risk:** Low · **Touches:** [01_CLIENT], [03_MULTIPLAYER], [16]
**Sub-features:** server-pushed initial snapshot · client-side cache · hot-reload compatibility · per-client view filtering

**Concept.** When a client joins, server sends the current `ReplicatedStorage` contents (templates, configs, shared scripts). Client caches; subsequent changes replicate as deltas. Hot-reload on Studio side propagates.

**Forecasted feedback (R)**
- R5.1 Initial snapshot can be MBs; need progress UI.
- R5.2 What's in ReplicatedStorage vs. ServerStorage vs. ServerScriptService is a Roblox-parity content design question.
- R5.3 Per-client view filtering (don't ship admin scripts to all clients).

**Implications (I)** — *Architectural:* clean partition between client-visible and server-only state.

**Risks (X)** — X5.1 Leaking server-only data via ReplicatedStorage.

**Mitigations (M)** — M5.1 Lint at publish time: nothing under ServerStorage replicates.

---

### Feature 6 — Per-user save data

**State:** 🔴 · **Effort:** M · **Risk:** Low · **Touches:** [01_CLIENT], [16]
**Sub-features:** `~/.config/eustress/saves/{project_id}/` directory · TOML or binary save format · cloud-sync via Premium subscription · auto-save on quit

**Concept.** The Client (and embedded server in solo) writes per-project save data on quit / checkpoint. Free tier = local; Premium = cloud-sync to user's KYC account.

**Forecasted feedback (R)**
- R6.1 Solo session crash = lost save unless auto-save tick exists.
- R6.2 Cloud sync conflict (laptop + desktop) — last-writer-wins or merge?
- R6.3 Save format binary vs. TOML — binary is smaller, TOML is debuggable.

**Implications (I)** — *Cross-system:* Premium feature ([09_ECONOMY]); ties to identity.

**Risks (X)** — X6.1 Save data corruption on crash mid-write.

**Mitigations (M)** — M6.1 Atomic write to temp + rename; double-buffered save slots.

---

### Feature 8 — Schema versioning + migration

**State:** 🔴 · **Effort:** L · **Risk:** High · **Touches:** [04_ASSETS], [10_TELEMETRY], [16]
**Sub-features:** schema version in stored value · migration registry (v1→v2 fn) · validation gate at read · log + telemetry of migrations

**Concept.** Every persisted value (DataStore, save data, content row) tags its schema version. Engine startup runs pending migrations. Reads from old schema auto-upgrade.

**Forecasted feedback (R)**
- R8.1 Today nothing has a schema version; first engine update breaks every save.
- R8.2 Migration must be idempotent (re-runnable safely).
- R8.3 What happens if a published game's DataStore schema differs from the engine the player is running?

**Implications (I)** — *Strategic:* if not designed before launch, every future engine release breaks user data.

**Risks (X)** — X8.1 Bad migration corrupts data.

**Mitigations (M)** — M8.1 Migrations are pure functions; tested in CI before release.

---

## Wiring / import gaps (top 8)

1. `DataStoreService` script API + server-side store
2. `OrderedDataStore` sorted-set backing
3. `DataStorePages` cursor pagination
4. Per-user save data writer (`~/.config/eustress/saves/`)
5. Atomic multi-key transaction primitive
6. Schema versioning framework
7. TTL / GC of expired keys
8. Cloud-sync of saves (Premium feature gate)

---

## Cross-system dependencies

- **[01_CLIENT]** save data writer.
- **[03_MULTIPLAYER]** server validates DataStore writes (no spoofing user IDs).
- **[09_ECONOMY]** Bliss balance ledger reuses the same persistence patterns.
- **[10_TELEMETRY]** migration events tee.
- **[12_INFRASTRUCTURE]** SQLite → PG migration path.

---

## Open questions

- Q16.1 SQLite vs. PG migration trigger (writes/sec threshold).
- Q16.2 Save data format: TOML or binary postcard?
- Q16.3 Premium cloud-save quota (GB).
- Q16.4 Conflict resolution policy for cross-device save sync.
- Q16.5 DataStore quota defaults — per user / per store / per script.
- Q16.6 Encryption at rest for DataStore — opt-in or default?
