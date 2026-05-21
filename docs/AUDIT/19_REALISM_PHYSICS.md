# 19 — Realism & Physics Laws

> Fundamental physics laws (thermodynamics, mechanics, electromagnetism),
> Symbolica symbolic solver, GPU SPH fluids, materials science (stress / strain /
> fracture), particle ECS, quantum statistics. The **science-simulation layer** that
> sits alongside Avian's rigid-body physics, not inside it.

## Pass changelog

- **P3 (2026-05-14):** New doc; 12 features.
- **P4 (2026-05-14):** State correction from secondary critique: Symbolica is **partially wired** — `use symbolica::atom::Atom` in `causal.rs` + feature flag in `ARCHITECTURE.md`. Feature 10 state 🔴 → **🟡** (scaffold exists; full solver impl pending). V-Cell `Nernst + Butler-Volmer` are **lumped 0-D models** (no spatial electrochemistry / ion transport) — validation gap vs. real cells now flagged. Particle ECS `ElectrochemicalState` + `ThermodynamicState` are **decoupled** (no thermal-effect-on-reaction-rate coupling). Fracture mechanics `fracture_mesh.rs` exists but **no integration path to Avian** — visualisation only today.

---

## Concept summary

The **Realism & Physics Laws** subsystem is a complete *science* simulator. It is distinct from [11_SIMULATION_DEBUGGER] (which is the tooling — clocks, watchpoints, debugger UI) and from Avian (rigid-body physics). Realism implements **fundamental physics equations** (ideal gas, F=ma, conservation laws, Nernst, Butler-Volmer), runs them symbolically via Symbolica, and applies the results to particle ECS state.

The architecture has four layers:
- **Kernel Laws**: pure Rust functions implementing equations from peer-reviewed literature.
- **Symbolica solver**: symbolic equation manipulation for solving / simplifying / differentiating laws at runtime.
- **Particle ECS**: per-particle temperature, pressure, phase, composition.
- **GPU compute**: WGPU compute shaders for SPH (Smoothed Particle Hydrodynamics), thermal diffusion.

This subsystem is what makes Eustress an *engineering* engine — battery cells, fluid flow, thermal stress, fracture mechanics — not just a game engine. It feeds [11_SIMULATION_DEBUGGER] (watchpoints monitor Realism state) and is the data source for the AI-Repairman loop in [07_AI_PLATFORM] Feature 11.

---

## Implementation snapshot

**Crates / files:**
- [common/src/realism/](../../eustress/crates/common/src/realism/) — laws, symbolic, particles, materials, fluids, gpu
- [common/src/realism/ARCHITECTURE.md](../../eustress/crates/common/src/realism/ARCHITECTURE.md) — existing detailed doc
- [common/src/realism/gpu/buffers.rs](../../eustress/crates/common/src/realism/gpu/) — WGPU compute setup
- [docs/development/KERNEL_LAW_SYSTEM.md](../development/KERNEL_LAW_SYSTEM.md)
- [docs/development/RECURSIVE_FEEDBACK_LOOP.md](../development/RECURSIVE_FEEDBACK_LOOP.md)
- [docs/development/VCELL_CASE_STUDY.md](../architecture/VCELL_CASE_STUDY.md) — battery sim case study

**Working:**
- Kernel law registry (Nernst, Butler-Volmer, ideal gas, F=ma, thermal diffusion)
- Particle ECS (`ElectrochemicalState`, `ThermodynamicState`)
- V-Cell electrochemistry tick system
- Basic material properties (density, Young's modulus, yield stress)

**Stubbed / missing:**
- Symbolica integration (concept only; no crate dependency wired)
- SPH GPU compute (buffers ready, no compute pass)
- Fracture mechanics (Griffith, Paris) — design only
- Quantum statistics (Bose-Einstein, Fermi-Dirac) — out of scope today
- AI prompt grounding via KernelLawRegistry (07 Feature 11)

---

## Feature inventory

| # | Feature | State |
| ---: | --- | :-: |
| 1 | Kernel Law registry (50+ named laws) | ✅ |
| 2 | Particle ECS (temperature, pressure, phase) | ✅ |
| 3 | Thermal conduction + diffusion | 🟡 |
| 4 | Material properties (density, modulus, yield) | ✅ |
| 5 | V-Cell electrochemistry (Nernst, Butler-Volmer) | ✅ |
| 6 | SPH fluid dynamics (GPU compute) | 🟠 |
| 7 | Stress / strain tensor field | 🟠 |
| 8 | Fracture mechanics (Griffith, Paris) | 🔴 |
| 9 | Buoyancy + aerodynamic drag | 🟡 |
| 10 | Symbolica symbolic solver integration | 🔴 |
| 11 | AI prompt grounding via Kernel Laws | 🔴 |
| 12 | Quantum statistics (Bose-Einstein, Fermi-Dirac) | 🔴 |

---

## Detailed per-feature cards (top 6)

### Feature 1 — Kernel Law registry

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [07], [11], [19]
**Sub-features:** named law (`nernst_potential`, `butler_volmer`, `f_eq_ma`) · pure Rust fn · doc-test against textbook values · law metadata (units, references) · registry lookup

**Concept.** Every physics equation is a pure Rust function with named inputs/outputs, doc-tested against published values. The registry is the canonical source for both runtime tick systems and AI prompt grounding ([07_AI_PLATFORM] Feature 11).

**Forecasted feedback (R)**
- R1.1 Unit-safety via `eustress_common::units` enforced on inputs (C1 cross-cut).
- R1.2 Law metadata for citations (which paper, which equation number).
- R1.3 Doc-tests are critical — every law gets tested against textbook values.
- R1.4 Symbolic-friendly fn signatures (single-out, scalar args) for Symbolica integration.

**Implications (I)**
- *Architectural:* the registry IS the language Eustress speaks for science.
- *Cross-system:* AI prompts include "the law says V = -ΔG/nF" verbatim; consumer reasoning grounded.
- *Strategic:* the differentiator vs. every game engine; the entry vs. every CAE tool.

**Risks (X)** — X1.1 Mis-coded equation propagates everywhere.

**Mitigations (M)** — M1.1 Doc-tests + peer review on every new law.

---

### Feature 5 — V-Cell electrochemistry

**State:** ✅ · **Effort:** Done · **Risk:** Low · **Touches:** [11_SIMULATION], [19]
**Sub-features:** Nernst potential · Butler-Volmer kinetics · thermal coupling · degradation per cycle · dendrite-risk heuristic

**Concept.** A battery cell as ECS entity has `ElectrochemicalState` + `ThermodynamicState`. Each sim tick applies Nernst (voltage from concentration), Butler-Volmer (current from overpotential), thermal diffusion (Joule heating), and degradation (capacity fade per cycle).

**Forecasted feedback (R)**
- R5.1 Simplified model — lumped 1-D RC thermal, no spatial gradients.
- R5.2 Dendrite-risk is a heuristic; full morphology model would be a separate Feature.
- R5.3 No validation against real Na-S cell data yet.
- R5.4 Cathode-only chemistries are working; anode coupling is implicit.

**Implications (I)**
- *Strategic:* V-Cell is the showcase for engineering customers.
- *Operational:* tick cost is O(N cells); benchmarks needed at 10k+ cells.

**Risks (X)** — X5.1 Customers run "Eustress sim says voltage X" against real cells and discover divergence.

**Mitigations (M)** — M5.1 Publish per-chemistry calibration sweep; M5.2 disclose model fidelity in customer-facing reports.

---

### Feature 6 — SPH fluid dynamics (GPU compute)

**State:** 🟠 · **Effort:** L · **Risk:** Med · **Touches:** [11_SIMULATION], [13_TERRAIN], [19]
**Sub-features:** SPH particle ECS · WGPU compute kernels (density / pressure / viscosity) · spatial hashing on GPU · Tait equation of state · free-surface tracking

**Concept.** Smoothed Particle Hydrodynamics — water as particles, not mesh. GPU compute does per-particle density + pressure + viscosity. Free surface tracked via density gradient. Rendering as point sprites or marching cubes.

**Forecasted feedback (R)**
- R6.1 GPU buffer setup exists; compute pass + kernel missing.
- R6.2 Spatial hash on GPU is non-trivial (atomic increments).
- R6.3 Rendering choice (sprites vs. surface) affects visual quality.
- R6.4 Multi-fluid (oil + water) needs miscibility flag.

**Implications (I)** — *Cross-system:* [13_TERRAIN] water Feature 6 could share particles for coastal interaction.

**Risks (X)** — X6.1 GPU compute requires a baseline GPU; mobile path absent.

**Mitigations (M)** — M6.1 CPU fallback for small particle counts; M6.2 mobile force-disable.

---

### Feature 8 — Fracture mechanics

**State:** 🔴 · **Effort:** XL · **Risk:** High · **Touches:** [19]
**Sub-features:** Griffith energy-release-rate · Paris-law crack growth · stress-intensity factor · fracture-toughness from material · crack tip element insertion

**Concept.** Per-material fracture toughness drives crack initiation and growth from a stress concentration. Useful for engineering customers (turbine blade, battery casing, mechanical fasteners).

**Forecasted feedback (R)** — R8.1 Crack mesh evolution is non-trivial. R8.2 Element insertion at crack tip requires remesh.

**Implications (I)** — *Strategic:* the unique value-add over game engines.

**Risks (X)** — X8.1 Numerical instability at crack tip.

**Mitigations (M)** — M8.1 Cohesive zone model variant for stability.

---

### Feature 10 — Symbolica integration

**State:** 🔴 · **Effort:** XL · **Risk:** High (vendor dep) · **Touches:** [07_AI], [19]
**Sub-features:** Symbolica crate dependency (or HTTP service) · symbolic simplify · solve · differentiate · constraint-validate generated code

**Concept.** Symbolica is a symbolic-algebra engine. Eustress uses it to (a) simplify equations at runtime for fast tick, (b) solve laws for unknown variables, (c) validate AI-generated equations against known laws.

**Forecasted feedback (R)**
- R10.1 No code today; APEX_ENGINE.md mentions only.
- R10.2 Symbolica licensing terms need vetting.
- R10.3 Performance — symbolic op can be slow; cache results.
- R10.4 Cross-platform support.

**Implications (I)**
- *Cross-system:* [07_AI_PLATFORM] Feature 11 depends on this.
- *Strategic:* enables AI-Repairman to reason about laws, not just numbers.

**Risks (X)** — X10.1 Vendor lock-in if Symbolica licence changes.

**Mitigations (M)** — M10.1 Build adapter trait; Symbolica is one impl, custom solver another.

---

### Feature 11 — AI prompt grounding via Kernel Laws

**State:** 🔴 · **Effort:** L · **Risk:** Med · **Touches:** [07_AI_PLATFORM], [19]
**Sub-features:** law-vocabulary injection into Claude system prompt · output validation (LLM-generated code must reference real laws) · physics-violation rejection · auto-retry with constraint hint

**Concept.** When Workshop generates code, the system prompt includes the relevant kernel laws as context ("a battery's voltage is V = E₀ − (RT/nF)·ln(Q)"). Generated code is validated against the registry: a function that claims to compute Nernst must call the canonical `nernst_potential` fn or compute equivalent.

**Forecasted feedback (R)**
- R11.1 Selection of relevant laws per prompt is itself an embedding problem ([07_AI] embedvec).
- R11.2 Validation rejects garbage but can't catch wrong-but-plausible.
- R11.3 Token cost: 50 laws × 30 tokens = ~1500 tokens system prompt overhead.

**Implications (I)** — *Strategic:* this is what "physics-grounded AI" means in concrete terms.

**Risks (X)** — X11.1 Over-strict validation rejects legitimate user code.

**Mitigations (M)** — M11.1 Warning level (not block) on first iteration.

---

## Wiring / import gaps (top 8)

1. Symbolica crate dependency (or HTTP service wrapper)
2. SPH compute shader pass + spatial-hash kernel
3. Fracture mechanics (Griffith + Paris)
4. AI prompt-grounding bridge (Workshop ↔ KernelLawRegistry)
5. CPU fallback for SPH on mobile / no-GPU systems
6. Stress / strain tensor visualisation overlay
7. Per-material physics-property TOML extension ([04_ASSETS] tie-in)
8. Quantum statistics module (out of P3, P4+ scope)

---

## Cross-system dependencies

- **[04_ASSET_PIPELINE]** per-asset physics-material assignment.
- **[07_AI_PLATFORM]** physics-grounded prompts + AI-Repairman loop.
- **[11_SIMULATION_DEBUGGER]** watchpoints observe Realism state.
- **[13_TERRAIN]** water + thermal coupling on terrain.
- **[14_GEO_COORDINATES]** gravity computation per-position.

---

## Open questions

- Q19.1 Symbolica licence terms — vendor-lock acceptable?
- Q19.2 SPH default particle count budget (10k mobile / 100k desktop?).
- Q19.3 Validation policy: block vs. warn on unphysical generated code?
- Q19.4 Per-material physics props TOML schema location ([04_ASSETS] or [19]?).
- Q19.5 Quantum-stat module — P4 / P5 / never?
- Q19.6 Fracture coupling with Avian rigid bodies (when does a part break apart)?
