# Government Capture Pipeline: Solicitation to First Draft

Companion to `GOVERNMENT_MODE.md`. That document covers the **buy side**: a city
issuing a solicitation, leveling bids, and awarding. This one covers the **sell
side**: a small contractor, grant writer, or nonprofit screening federal
opportunities and producing a structured first response.

Two public federal APIs are the whole input surface:

| Source | Endpoint | Auth | Notes |
|---|---|---|---|
| SAM.gov Get Opportunities v2 | `https://api.sam.gov/opportunities/v2/search` | free `api_key` query param | `postedFrom`/`postedTo` in `MM/dd/yyyy`, one-year max window, `limit` up to 1000, `offset` paging, `ncode` (NAICS), `typeOfSetAside`, `ptype`. Detail text lives behind `resourceLinks`, not in the search payload. |
| Grants.gov Search2 | `POST https://api.grants.gov/v1/api/search2` | none | JSON body: `keyword`, `oppNum`, `eligibilities`, `agencies`, `oppStatuses`, `aln`, `fundingCategories`, `rows`, `startRecordNum`. Detail via `POST /v1/api/fetchOpportunity`. A full daily XML extract also exists as a bulk fallback. |

Both are documented, public, and carry no scraping or ToS risk. Verify the exact
parameter names against current documentation before wiring: federal API shapes
drift between versions.

## What is built

`crates/engine/src/capture/` (11 modules) plus the `publicsector` mode. Twenty
ribbon tools dispatch for real; the rest of the surface stays declared
intentions.

| Module | Does |
|---|---|
| `model.rs` | `Opportunity`, `CapabilityStatement`, `SetAside`, date and money parsing |
| `sources.rs` | SAM.gov and Grants.gov request building and response parsing |
| `poll.rs` | The Connector poll runtime, on a background thread with backoff |
| `store.rs` | `capability.toml`, raw page reload, Connector authoring |
| `screen.rs` | The deterministic gates, every rejection carrying its reason |
| `score.rs` | Per-criterion fit, the no-bid recommender, the calibration harness |
| `requirements.rs` | Every "shall" and "must", with clause id and bound actor |
| `compliance.rs` | The requirement matrix and the gap report |
| `outline.rs` | FAR and NOFO response templates, seeded from the matrix |
| `audit.rs` | One folder per notice, with a Merkle manifest and verification |
| `layout.rs` | The 3D deal room, written as instances the file watcher spawns |
| `ribbon.rs` | One dispatch entry point for all twenty live tools |

Everything except `poll.rs` and `layout.rs` is pure: no Bevy, no clock, no
network. The decision path is testable offline with no account and no API key.

### Reused rather than rebuilt

- **`Connector` instance class**, written to `<Space>/DataService/<name>/_instance.toml`,
  the same front door the Data menu already used. `poll.rs` is the consumer the
  comment at `slint_ui.rs:17151` said was the next increment.
- **`eustress_data::source::http`** for the transport seam, including
  `redact_query` so a SAM API key never reaches a log.
- **`eustress_data::provenance`** for content-addressed records and the Merkle
  manifest behind `audit.rs`.
- **`Part` + `BillboardGui` + `[attributes]`**, the pattern the Tucson Universe
  proved, so the deal room needs no new spawn path.

### Still open

1. **No `Provenance` ECS component.** The manifest covers the audit folder;
   nothing attaches provenance to a live instance yet.
2. **No attachment fetcher.** SAM's search payload carries a LINK to the
   description, not the text, so requirement extraction runs only where a
   `requirements.txt` has been placed in the notice's folder. The tools say so
   rather than reporting zero requirements.
3. **No generation backend.** Drafts are seeded deterministically from the
   firm's own capability text. A local model would improve the prose; nothing
   depends on it.
4. **No document output.** Drafts are markdown; no docx or pdf crate is in the
   workspace.
5. **No incremental sync cursor.** Each poll refetches its window and dedupes
   on notice id rather than fetching deltas.

## Placement

Government mode's charter states every discipline is a public **analysis seat**
that "signs, files, certifies, and authorizes nothing." A capture desk is a
commercial seat pointed the other way: it exists to win money from the
institution the other twelve disciplines model. The same reasoning that kept
campaign tooling out of Government mode applies here.

Settled: **a new `publicsector` mode**. Not a Government discipline, and not a
Business discipline either.

The workflow decomposes into five seats that map to five real job titles, which
is the tell that it is mode-shaped rather than discipline-shaped:

| Discipline | Glyph | Owns |
|---|---|---|
| `capture` Capture | `binoculars` | Pipeline, screening, fit scoring, bid and no-bid |
| `proposal` Proposal | `scroll` | Compliance matrix, outline, drafting, review gates |
| `grants` Grants | `certificate` | NOFO parsing, logic model, budget narrative, indirect rate |
| `contracts` Contracts | `stamp` | Post-award: CLINs, mods, invoicing, CPARS, closeout |
| `compliance` Compliance | `shield` | SAM entity and UEI, reps and certs, FAR flowdowns, size standards |

Ribbon tabs `["home", "data", "mindspace", "test"]`. MindSpace carries the 3D
deal room; Test carries win-rate calibration runs.

An eighth Business discipline was rejected on one concrete point: ribbon tabs
are mode-level (`ModeManifest.tabs`; `SubmodeMeta` has no tab field), so giving
the discipline a MindSpace tab means adding `mindspace` to Business's own
`[ribbon] tabs`, which changes all seven shipped Business disciplines. A new
mode is purely additive instead.

Cost is small, because `handshake.svg` already ships as a tool icon:

- `modes/publicsector.toml`
- one line in `load_builtin_modes` (`studio_modes.rs:481`)
- one id in the `builtins_parse` ordered vec (`studio_modes.rs:728`)
- `"handshake"` into `KNOWN_ICON_IDS` (`studio_modes.rs:35`)
- one arm in `mode-icon()` (`ribbon.slint:661`), reusing the existing SVG
- a `publicsector_mode_shape` test

### Charter

Three rules, two carried over from Government and one adapted:

1. **The engine drafts; a human files.** Nothing here submits a bid, signs a
   certification, or transmits to SAM.gov or Grants.gov. This mirrors
   Government's "signs, files, certifies, and authorizes nothing."
2. **Every discipline carries a falsifier**, meaning at least one tool able to
   return a result unfavourable to its own operator. In Capture that is the
   no-bid recommender; in Proposal, the compliance-gap report; in Contracts,
   the cost growth analyzer.
3. **Own data is fair game; other parties are aggregate only.** The firm's own
   UEI, personnel, and past performance are inputs. Competitors appear through
   public award records only, never as profiled individuals.

---

## Task list

Stable ids. Append freely.

**Landed:** A1, A2, A3, A4, A7, B4 (partial: manifest, not an ECS component),
C1, C2, C3, C4, D1, D2, D3, D7, E1 through E8, F1 through F4, F6, G2, G3, H2,
H4.

**Open:** A5 (incremental cursor), A6 (attachment fetcher), B1, B2, B3, B5
(the domain lives in `capture::model`, not yet as `ClassName` variants with
Properties panels), C5, D4, D5, D6, F5, F7, G1, G4, G5, H1, H3.

### Track A: Ingest

| # | Task | Files |
|---|---|---|
| A1 | **Connector poll runtime.** Bevy system that reads `enabled = true` Connectors, builds a `SourceConfig` via `materialize::config_from_attributes`, fetches through `UreqTransport`, and writes a sibling Dataset via `plan_for_connector`. Respect `poll_seconds`. Runs off the main thread or on a frame budget. | new `crates/engine/src/data/connector_poll.rs`, `app_core.rs` |
| A2 | **SAM.gov source profile.** `source_type = "sam_opportunities"`. Date-window defaults, NAICS and set-aside params, `offset` pagination loop, 429 backoff. | `crates/data/src/source/`, new profile module |
| A3 | **Grants.gov source profile.** `source_type = "grants_search2"`. POST body construction, `startRecordNum` paging, `fetchOpportunity` detail follow-up. | same |
| A4 | **API key indirection.** SAM requires a key; it must never land in a `_instance.toml` on disk. Add an `api_key_env` attribute resolved at fetch time. Confirm `redact_query` covers it in every log path. | `source/rest.rs`, `source/materialize.rs` |
| A5 | **Incremental sync and dedupe.** Persist a cursor in Connector attributes, fetch deltas only, dedupe on notice id. Re-running must not duplicate rows. | A1 |
| A6 | **Attachment fetch.** SAM `resourceLinks` point at the SOW/PWS/RFP documents where the real requirement text lives. Fetch, content-hash, cache under the Space, negative-cache 404s. Reuse the importer media-fetch pattern. | new fetcher module |
| A7 | **Rate-limit handling.** SAM enforces documented daily caps that vary by account role. Detect, back off, and surface remaining quota rather than failing silently. | A2 |

### Track B: Domain nouns

| # | Task | Files |
|---|---|---|
| B1 | **`Opportunity` class.** One solicitation. Add to `ClassName`, the `insert_classes` bucket, the `serialization/binary.rs` ClassId mapping, and Properties. Note `Connector` currently falls back to `ClassId::Folder` in binary.rs; do not repeat that. | `crates/common/src/classes.rs`, `ui/insert_classes.rs`, `serialization/binary.rs` |
| B2 | **`CapabilityStatement` class.** The firm profile that matching runs against: NAICS list, UEI, set-aside certifications, past performance, key personnel, geography. | same |
| B3 | **`ResponseDraft` class.** Generated draft plus its section tree, linked to the Opportunity and to the source clauses. | same |
| B4 | **`Provenance` ECS component.** Wraps `RecordId` plus source URL, fetch timestamp, and `Manifest` root. Attached to every Opportunity, attachment, score, and draft. | new `crates/engine/src/provenance.rs` |
| B5 | **Properties panel coverage.** Each new class needs an editor surface, or it is invisible to the user. | `ui/parameters.rs` |

### Track C: Screening and fit

| # | Task | Files |
|---|---|---|
| C1 | **Deterministic prefilter.** NAICS match, set-aside eligibility, PSC, place of performance, response deadline versus now, ceiling versus capacity. Pure Rust, unit tested, no model. This removes most of the 500 before anything expensive runs. | new `crates/engine/src/capture/screen.rs` |
| C2 | **Fit scoring with per-criterion subscores.** One opaque number is unusable and unauditable. Every criterion reports its own score and the rule that fired. | `capture/score.rs` |
| C3 | **No-bid recommender (the falsifier).** Government mode requires every discipline to carry a tool able to return a result unfavourable to its operator. Here that is an explicit "do not bid" output with reasons. | `capture/score.rs` |
| C4 | **Calibration harness.** Score past solicitations with known outcomes and report hit rate. Without this the ranking is unfalsifiable, which the mode charter forbids. | `capture/calibration.rs` plus tests |
| C5 | **Semantic similarity (optional).** `embedvec` has real HNSW but its default embedder is hash-only. Either wire a real embedder or ship C1 through C4 alone. | `crates/embedvec` |

### Track D: Drafting

| # | Task | Files |
|---|---|---|
| D1 | **Requirement extraction.** Parse the attachment set for numbered requirements: every "shall" and "must" becomes a row with its clause id and page. Deterministic text processing. | `capture/requirements.rs` |
| D2 | **Compliance matrix generator.** Requirement rows to response sections, with coverage gaps flagged. This is the single highest-value artifact in the workflow and needs no model at all. | `capture/compliance.rs` |
| D3 | **Response outline builder.** FAR Sections L and M for contracts; the NOFO required-narrative structure for grants. Produces the skeleton D4 fills. | `capture/outline.rs` |
| D4 | **Draft generation backend.** Cheapest local path is an HTTP-served local model (Ollama or llama.cpp) reached through the existing `UreqTransport`, rather than making the candle `local` feature build. Decide before writing. | `crates/spatial-llm` |
| D5 | **Citation binding.** Every generated paragraph carries the clause id it answers. A draft with no citations fails the audit requirement. | D4 |
| D6 | **Output format.** No docx or pdf crate exists. Either add one or emit markdown and convert downstream. | new dep or export path |
| D7 | **Never auto-submit.** The engine drafts; a human files. Same posture as the existing twelve disciplines. Enforce it in code, not just in docs. | design constraint |

### Track E: Mode surface

| # | Task | Files |
|---|---|---|
| E1 | ~~Settle placement.~~ **Settled: new `publicsector` mode.** See Placement above. | done |
| E2 | **Write `modes/publicsector.toml`.** Five disciplines, five tabs each, one id prefix per discipline (`pcap:`, `pprp:`, `pgrt:`, `pcon:`, `pcmp:`). Any discipline declaring `[[submode_tabs]]` makes the mode-level `[[tabs]]` unreachable, so repeat a shared tab into all five buckets or it vanishes. | `crates/engine/modes/publicsector.toml` |
| E3 | **Register the mode.** One `load_builtin_modes` entry plus one id in the `builtins_parse` ordered vec, which asserts the exact set rather than a count. Source order is dropdown order. | `studio_modes.rs:481`, `studio_modes.rs:728` |
| E4 | **Allowlist the mode icon.** `"handshake"` into `KNOWN_ICON_IDS` and a matching arm in `mode-icon()`. The SVG already exists; an unlisted id silently falls back to a gear. | `studio_modes.rs:35`, `ui/slint/ribbon.slint:661` |
| E5 | **`publicsector_mode_shape` test.** Five ungated disciplines, tab subset containing `mindspace` and `data`, each discipline leading with its own tab. Model it on `government_mode_shape`. | `studio_modes.rs` |
| E6 | **Regenerate metadata and icons.** Run **both** `scripts/gen_tool_metadata.py` and `scripts/gen_tool_icons.py`. `builtins_fully_populated` fails the build on a missing entry, a missing composed icon, or an empty section. | `scripts/` |
| E7 | **Dispatch arms** for the tools that are real. Everything else stays a declared intention, consistent with the existing 561-inert posture. Do not describe undispatched buttons as working. | `ui/slint_ui.rs` |
| E8 | **Cross-link Government.** `government/procurement` models the buyer; `publicsector/capture` models the seller. Note the pairing in both manifest headers so neither is mistaken for the other. | `government.toml`, `publicsector.toml` |

### Track F: 3D deal room

| # | Task | Files |
|---|---|---|
| F1 | **Dataset to 3D layout, engine-native.** A Rust system or an MCP-driven build, not a one-off generator script. | new `crates/engine/src/capture/layout.rs` |
| F2 | **Settle the spatial encoding.** Proposal: X is time to deadline, Y is fit score so height equals rank, Z clusters by agency or NAICS; node size is ceiling value, color is set-aside eligibility. Walking toward the origin becomes walking toward the closing bids. | F1 |
| F3 | **Node interaction.** `BillboardGui` label, click to select, Properties shows the Opportunity. The camera-relative billboard drag pattern already exists. | F1 |
| F4 | **Stage lanes.** Physical zones for Screening, Fit, Drafting, Submitted. Dragging a node between zones writes the state change to disk and to the op log. | F1, G1 |
| F5 | **Portals per agency or quarter.** Reuse `portal.rs` attributes. Respect the `open_space` camera respawn: hold the pose on a wall-clock timer, never a frame count. | `crates/engine/src/portal.rs` |
| F6 | **Budget the billboard load.** 500 solicitations is 500 billboards. Honor the atlas slot ceiling and the spatial grid, or the room stutters. | F1 |
| F7 | **Confirm the multi-user story** before promising a shared room. Verify what the session layer supports today. | investigation |

### Track G: File-system audit

| # | Task | Files |
|---|---|---|
| G1 | **Op-log every fetch, score, and draft.** Reuse `worlddb::mutations`. | `crates/worlddb/src/mutations.rs` |
| G2 | **On-disk layout readable without the engine.** `Spaces/<Space>/Capture/<NoticeId>/` holding `_instance.toml`, `source.json` (raw API response), `attachments/`, `score.toml`, `draft.md`, `manifest.json`. An auditor with `cat` and nothing else must be able to reconstruct the decision. | A1, B4 |
| G3 | **Merkle manifest per opportunity** via `provenance::Manifest`. Any edit to any record changes the root. | `crates/data/src/provenance.rs` |
| G4 | **Optional `bliss` anchoring** for a dated "this draft existed" claim. | `crates/bliss` |
| G5 | **MCP audit surface** so an agent can walk the same trail: extend `query_audit_log` and `oplog_tail` coverage to capture events. | `crates/mcp-server/src/bridge_tools.rs` |

### Track H: Correctness guards

| # | Task | Files |
|---|---|---|
| H1 | **Timezone-correct deadline math.** Federal deadlines land at a specific local time. An off-by-one loses the bid. Test it explicitly. | `capture/screen.rs` tests |
| H2 | **Freshness indicator.** A stale cache showing an expired opportunity as open is the failure that ends trust in the tool. Surface fetch age everywhere a deadline is shown. | UI |
| H3 | **Amendment tracking.** Solicitations get amended and deadlines move. Detect a changed notice and mark the affected draft stale. | A5 |
| H4 | **Attachment parse failure is visible, not silent.** A PDF that fails to extract must show as unread, never as an empty requirement set. | D1 |

---

## Build order

1. **A1** unblocks everything. Until an enabled Connector actually polls, the
   rest has no input.
2. **A2 plus B1 plus G2** gives the first provable result: real SAM.gov
   opportunities on disk, auditable with `cat`.
3. **C1** next, because deterministic screening delivers most of the value at a
   fraction of the effort, and it is testable.
4. **D2** before **D4**. The compliance matrix needs no model and is the artifact
   users would pay for on its own.
5. **F1 through F3** once there is real data worth standing inside.
6. **E2 through E8** last, so the mode only ever declares what exists.
