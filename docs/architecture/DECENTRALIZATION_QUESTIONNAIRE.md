# Eustress Decentralization — Design Questionnaire

Goal: no central storage for published content (Cloudflare KV included), Bliss as a real
decentralized currency on its own node network, an on-chain ownership registry for every
Universe / Space / Asset, and an AI guardian in the moderation loop.

Baseline this is built on (today's truth): Bliss is a live proof-of-contribution economy on
one Cloudflare Worker (api.eustress.dev). The engine tracker, 90-second heartbeat, and daily
UTC emission (tail emission: 5%/year of 100 million BLS, halving every 4 years, 0.5% floor)
work end to end — but the "witness" is one server signing its own hashes: no chain, no
consensus, no peer storage. The engine already ships Light/Full node UI whose Full-mode copy
promises "stores blockchain data and produces blocks." This project makes that sentence true.
The `.eustress` directory (header + world.fjalldb + assets/ + schema/) is the natural unit of
published content.

Answer inline, any format — "1D, 2 yes, 3: pause CAD" works. ★ = recommended default.

---

## Section 0 — What "decentralized" must mean

**1.** Which properties must be TRUE at version 1? Rank them:
- **A.** Censorship-resistance — no single party (including Eustress/you) can block, rewrite, or mint.
- **B.** Survivability — every published Universe/Space/Asset outlives Eustress-the-company (external nodes, open protocol, export escape hatch).
- **C.** Self-custody — creators hold keys; ownership provable without our servers.
- **D.** ★ All three, phased, in order B → C → A. Survivability and self-custody ship even with founder-run validators; censorship-resistance is the long pole because the AI moderation gate deliberately retains a veto.

**2.** Is progressive decentralization acceptable — genesis with Eustress-run validators plus a published hand-over schedule — or must independent validators exist at launch? ★ Progressive with a public schedule. Anything else is theater for a solo founder.

**3.** What does this displace? Terrain worldgen, Mission-01 engine gaps, CAD phases, and the Data Platform are all active. Name what pauses. This is the only question with no recommendation — it's entirely yours.

## Section 1 — Chain substrate

**4.** Build vs adopt:
- **A.** ★ Own app-specific chain in Rust — an existing Byzantine-fault-tolerant consensus library (e.g. Informal Systems' `malachite`, a Rust CometBFT-lineage engine) plus our own application state (Bliss balances, ownership registry, storage attestations, moderation verdicts). Sovereign, matches "the engine ships a node," reuses `bliss-core`/`bliss-crypto`. Biggest engineering lift.
- **B.** Substrate / Polkadot SDK — Rust, batteries included, but a heavy framework with ecosystem gravity.
- **C.** Cosmos SDK appchain — mature, but Go: off-stack for an engine-embedded node.
- **D.** Token + registry contracts on an existing chain (Solana, Base, …) plus a separate storage network — fastest credibility, but Bliss becomes a tenant and "nodes in the engine" shrinks to storage duty only.

**5.** What secures consensus:
- **A.** ★ Genesis proof-of-authority (Eustress validators) → bonded proof-of-stake, where validators bond BLS plus artifact-anchored contribution reputation, on the schedule from Q2.
- **B.** Bonded proof-of-stake from day one — note BLS is earned through contribution scoring that the hardening pass made expensive but not impossible to game; stake quality inherits that.
- **C.** Storage-power consensus (Filecoin-style proof-of-storage) — novel cryptography; months of work on its own.

**6.** Chain payload discipline: the chain stores only small facts — transactions, ownership records, content hashes, head pointers, moderation verdicts, storage proofs — never content bytes. Blobs live on the storage network. Agree? ★ Yes. This kills "store places IN the blockchain" literally but preserves it in effect: the hash on chain IS the place's identity, and the place is unreachable by any central party's permission.

## Section 2 — Storage network

**7.** Blob layer:
- **A.** ★ `iroh` — Rust, QUIC, BLAKE3-verified content-addressed blob sync; production-grade; embeds cleanly in both the engine and a headless node.
- **B.** libp2p + bitswap (IPFS componentry) — bigger ecosystem, more glue code.
- **C.** Custom protocol — full control; reinvents hole-punching, relays, and verified streaming.
- **D.** Rent it (Arweave/Filecoin as backing store) — outsources durability, breaks "our nodes solve storage," adds an external token dependency.

**8.** Published artifact format:
- **A.** ★ Deterministic snapshot export of `.eustress` — canonical serialization → chunked → BLAKE3 merkle root recorded on chain. (Raw fjall directory bytes are not deterministic; an export format is required no matter what.)
- **B.** Causal op-log as the replication primitive (snapshot + delta chain) — pairs beautifully with the live op-log work, more moving parts. Reasonable path: A now, B later as a bandwidth optimization.

**9.** Durability policy: target replication factor per blob (suggest 8–12 nodes; erasure coding later). Proof-of-storage = random BLAKE3-chunk challenges answered inside the existing 90-second heartbeat, attested on chain. Accept? ★ Yes. Also set the demotion threshold: how many independent storage nodes, sustained how long, before Cloudflare KV stops being authoritative (suggest ≥20 nodes for 30 days).

**10.** Hot path: peer fetch loses to a CDN at first. Is a non-authoritative Cloudflare cache in front acceptable during transition — chain + peers remain the source of truth, the cache can vanish with zero data loss? ★ Yes, labeled honestly as a cache. Retrieval rewards later make peers competitive.

**11.** Mutability: a Space update = owner-signed head record on chain (monotonic version → new snapshot root); history is a hash-linked DAG. Two sub-questions: (a) is version history public by default for public Spaces? ★ Yes. (b) Private Spaces publish encrypted blobs — the chain sees only a ciphertext hash, which means the AI guardian cannot scan them until they're made public/discoverable. Confirm that boundary: moderation applies at the publication/discovery line, not inside private content. ★ Confirm.

## Section 3 — Bliss, the real deal

**12.** Existing balances: genesis-import the current KV ledger (snapshot at a cutover date, published for audit) so nobody's earned BLS resets. ★ Yes.

**13.** Emission stays proof-of-contribution: the live tail-emission + self-healing-treasury math ports into the chain runtime as THE mint rule. Block producers earn through an uptime/block-production contribution bucket — not block rewards — preserving "no mining" and the BLS-earned-only law. ★ Yes.

**14.** How storers get paid (the core economic design decision):
- **A.** ★ Storage-as-contribution: pinned-bytes × proven-uptime becomes a new contribution bucket paid from the existing emission; publishing costs a flat anti-spam BLS fee. Cheapest to ship — reuses the live economics. Bonus: an on-chain publish is exactly the server-verifiable artifact that the step-4 "artifact attestation" hardening was waiting for — this closes that hole for Development/Creation too.
- **B.** Arweave-style endowment: pay once per byte; an endowment drips to storers forever. Strongest permanence story; hard actuarial math.
- **C.** Filecoin-style term deals: renewable storage contracts. "Your Universe expired" is a terrible ownership story.

**15.** Transferability (the legal fork):
- **A.** BLS stays non-transferable between users — earn → hold → the existing Stripe Connect USD drip. Lowest legal risk, weakest "real currency" claim.
- **B.** ★ On-chain peer-to-peer BLS transfers; no fiat on/off-ramp beyond the existing Stripe payout edge; no exchange listings. Real currency mechanics with contained exposure — but answer Q16 before locking.
- **C.** Freely tradeable, exchange-listed, self-custodied — full Howey / FinCEN money-services-business / OFAC analysis, licensed counsel, possibly entity restructuring.

**16.** Legal posture: the Worker already pays out USD via Stripe Connect — Bliss is money-adjacent TODAY. Will you engage crypto-competent counsel before enabling Q15-B or C, and is there budget for it? (The Cloudflare KYC jurisdiction ontology you built suggests you saw this coming — was it for this?)

**17.** Tickets (the bought-only token): on chain too, or a Stripe-side ledger entry indefinitely? ★ Stripe-side. Putting the purchasable token on chain multiplies the legal surface for near-zero decentralization gain, and the earned/bought firewall is already a locked design law.

## Section 4 — Identity & ownership

**18.** Key model:
- **A.** Pure self-custody — lose the key, lose everything. Maximal and brutal.
- **B.** ★ Self-custody keypair (`bliss-crypto` already ships `KeyPair`) stored in the OS keychain, plus optional witness-assisted recovery (email/social) with an opt-out for purists.

**19.** Ownership registry: Universe, Space, and Asset each get an on-chain record — owner pubkey, content root, license, provenance. Transfers at v1?
- **A.** No transfers; registry only.
- **B.** ★ Gift/transfer allowed; no sale mechanism yet.
- **C.** Full BLS marketplace at launch — drags Q15/Q16 in immediately.

**20.** Provenance: the importer knows source asset IDs, so Roblox-imported content gets flagged — you can register that you imported it, never that you authored it. On-chain provenance enum `original | imported | derived`? ★ Yes. An ownership chain that lets people claim imported Roblox content as their own is dead on arrival.

## Section 5 — The AI guardian

**21.** The contradiction to resolve first: a single AI gate that every publish must pass IS a central chokepoint — the exact thing this project removes elsewhere. Placement:
- **A.** ★ Pre-listing gate: content pins to the network freely, but public listing/discovery requires a recorded moderation verdict. Separately, legal must-remove categories (child sexual abuse material, terror content, DMCA/court orders) go on an enforceable removal list every compliant node honors — that lane is law, not policy, and exists in every option.
- **B.** Pre-pin gate: nothing replicates until the AI passes it — strongest filter, slowest publish, deepest centralization.
- **C.** Post-hoc sweeps and delisting only — weakest guarantee.

**22.** Who runs the judge:
- **A.** Founder-run Grok 4.5 via the xAI API — simplest, but every creator's content ships to xAI's servers; one API outage or xAI policy change halts publishing network-wide; per-publish inference cost scales with the network.
- **B.** Moderator nodes running a pinned open-weights model with a versioned policy prompt, 2-of-3 quorum, each verdict recorded on chain with model hash + policy hash — decentralized, auditable, reproducible.
- **C.** ★ A now, B on the Q2 schedule — ship with Grok, but design the verdict record from day one to carry (model, policy-version, rationale) so swapping in a quorum later changes the judge, not the schema.

**23.** The policy document: "satanic content" must be operationalized into a written, versioned constitution — the criteria the model is prompted with and humans appeal against. It needs adjudicable lines: where do horror themes, occult-fantasy fiction, and religious-critical content fall? Who authors and amends it? ★ Founder-authored v1, hash of each version on chain, every verdict cites the version it applied. This document is yours to write — the questionnaire's job is to put it on your desk.

**24.** Appeals: human review (you) with on-chain overturn records at v1, plus false-positive tracking feeding policy revisions? ★ Yes. A guardian with no appeal path is censorship with extra steps.

**25.** Scope: moderation at publish/update time only, or also live runtime content (in-Space chat, user builds inside running Spaces)? ★ Publish-time only for v1. Runtime moderation is a separate, much larger system.

## Section 6 — Nodes & operations

**26.** Genesis storage nodes — name them. Your machines plus who? Alpha partners, UA Tech Parks / Chamber contacts, public opt-in through the existing Full Node toggle (whose UI copy already promises exactly this duty)? The network is theater until people you don't control hold replicas.

**27.** Node software: a standalone headless `eustress-node` binary — chain validator + storage + (later) moderator duty — folding into the existing headless-runtime plan and sharing the lib crate, while the engine embeds the light node. ★ Yes. Also set the Full-node hardware floor honestly: current UI discloses ~2 GB RAM; storage duty adds a disk pledge (minimum, e.g. 100 GB?).

**28.** MVP gate — proposed definition of done: 3 nodes across ≥2 physical machines; publish Super Station (118,974 entities) through the moderation gate; cold-load it on a clean machine with Cloudflare storage OFF; execute 1 on-chain BLS transfer; kill 1 node mid-fetch with zero data loss. Amend or accept — and set a timebox after which scope gets cut (suggest 6 weeks to this gate).

## Section 7 — Engine integration

**29.** Local-first stance: the local `.eustress` (WorldDb/fjall) remains the working store; the chain is the PUBLISH target — like a git remote, not a mounted drive. "Simulation storage" then means published simulation state lives on the network while live editing stays local. ★ Confirm this reading.

**30.** The web platform: the engine's web build staying on Cloudflare Pages is distribution of the ENGINE, separate from content decentralization. Acceptable indefinitely, or does engine distribution eventually go content-addressed too? ★ Acceptable indefinitely — conflating the two stalls the part that matters.

---

## The three answers that decide everything

- **Q1/Q2** — what decentralized must mean and whether progressive is honest enough.
- **Q15/Q16** — the moment BLS becomes transferable, this stops being purely an engineering project.
- **Q21/Q22** — the AI guardian is the one place you are choosing to KEEP a center; deciding its shape decides how honest the "not centralized" claim is.
