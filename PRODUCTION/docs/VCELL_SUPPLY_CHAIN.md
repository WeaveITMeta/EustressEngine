# V-Cell Supply Chain — A Concrete Application

> **The Supply Chain Flow Analysis (`README.md`) applied to a real product: the Voltec V-Cell solid-state battery.**
>
> This is to `README.md` what `WATER/docs/IGBWP.md` is to `WATER/docs/README.md` — the general
> framework grounded in one true, specific case. Like the Indo-Gangetic Basin project, the
> engineering is not the blocker. The blocker is a single scarce input and an immature process.

> **SACRED-SOURCE NOTICE.** The V-Cell is not designed, modified, or reinvented here. Every
> material fact below is *derived from* the canonical Voltec source and cited to it:
> - `E:\Workspace\Voltec\docs\Products\V-Cell\README.md` (bill of materials)
> - `E:\Workspace\Voltec\docs\Products\V-Cell\SOTA_VALIDATION.md` (the honesty-tiered diligence)
> - `E:\Workspace\Voltec\docs\Products\Products.md` (product lineage)
> - `E:\Workspace\Voltec\docs\Products\V-Cell\V1\*.glb.toml` (per-component instances)
>
> Performance and cost figures keep the **source document's own tier** (VERIFIED / PROJECTED /
> ASPIRATIONAL). This document does not upgrade a tier or invent a number. Where the V-Cell's own
> SOTA review corrected a marketing claim toward the truth, this document follows the truth.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [The Bill of Materials as Supply Streams](#2-the-bill-of-materials-as-supply-streams)
3. [Why "Earth-Abundant" Is the Wrong Claim — The Scandium Constraint](#3-why-earth-abundant-is-the-wrong-claim--the-scandium-constraint)
4. [The Second Constraint: VACNT Manufacturing Maturity](#4-the-second-constraint-vacnt-manufacturing-maturity)
5. [The Seven Stages, Applied to the V-Cell](#5-the-seven-stages-applied-to-the-v-cell)
6. [Manufacturing: V-Man and the Yield-Multiplication Reality](#6-manufacturing-v-man-and-the-yield-multiplication-reality)
7. [Inventory Conservation for Reactive & Precious Inputs](#7-inventory-conservation-for-reactive--precious-inputs)
8. [Cost & Pricing — Honestly Tiered](#8-cost--pricing--honestly-tiered)
9. [Supply-Chain Risk Matrix](#9-supply-chain-risk-matrix)
10. [Phased Plan (Aligned to the V-Cell Roadmap)](#10-phased-plan-aligned-to-the-v-cell-roadmap)
11. [Data Integrity](#11-data-integrity)
12. [References](#12-references)

---

## 1. Problem Statement

**Given:** A solid-state sodium-sulfur cell whose *chemistry* is, per its own diligence, sound — built from a 40-layer anode/electrolyte/cathode stack *(source: `VCell_Assembly_40Layer.glb.toml` — 8,100 Ah, 2.23 V OCV, 296×96×6.8 mm)*. It is the foundation of the entire Voltec line: V-Cell → V-Pack → V-Grid, and the buffer inside V-Core and V-Supreme *(source: `Products.md`)*.

**Find:** Whether the chain that *feeds and builds* this cell can sustain it — and where it breaks first.

**The core failure mode, in the `README.md` frame:**

> The V-Cell does not fail at the bench. It fails at the **bottleneck** (§4 of `README.md`) — the
> narrowest cross-section in its supply chain. By conservation of mass, the whole line's output is
> capped by that one stage. For the V-Cell, that stage is **scandium supply**, with **VACNT
> fabrication** close behind. Optimizing anything else delivers zero additional cells.

This mirrors `IGBWP.md` exactly. There, the binding constraint was *"India does not know its own pumping rate"* — an unmeasured, scarce thing (recharge) gated the whole megaproject. Here, the binding constraint is a scarce *thing* (scandium) and an immature *process* (aligned-CNT growth). The cell's physics is the easy part.

---

## 2. The Bill of Materials as Supply Streams

Each V-Cell input is a supply stream with its own source, scarcity, and risk. *(Materials and per-cell mass fractions from `SOTA_VALIDATION.md` §1.1 and `README.md`; the same materials are encoded as real constants in the Eustress realism crate at `realism::constants::vcell_materials`.)*

| Input | Role | ~Mass frac | Scarcity | Supply confidence | Realism-crate constant |
|-------|------|-----------|----------|-------------------|------------------------|
| **Sulfur** (cathode active) | Cathode | ~36% | Abundant (petroleum byproduct) | **High** | `vcell_materials::sulfur_vacnt` |
| **Sodium** (anode) | Anode | ~10% | Abundant (23,600 ppm crust) | **High** | `vcell_materials::sodium` |
| **Sc-NASICON** (electrolyte) | Electrolyte | ~18% | **Scandium-limited** | **Low** (§3) | `vcell_materials::sc_nasicon` |
| **Al hex lattice** (collector) | Current collector | ~12% | Abundant Al, hard *process* | Medium | `vcell_materials::al_hex_lattice` |
| **Housing** (Al 6061-T6) | Structure | ~8% | Abundant | High | `vcell_materials::al_6061_t6` |
| **AlN thermal pad + terminals + inactive** | Thermal / electrical | ~6% | Moderate | Medium | `vcell_materials::aluminum_nitride` |
| **VACNT forest** (cathode host) | Sulfur confinement | (within cathode) | **Process-limited** (§4) | **Low** | — (carbon nanotube) |
| **ALD Al₂O₃ interlayer** (5 nm) | Dendrite barrier | trace | Abundant, precise process | Medium | — |

The honest reading of this table: **two of eight streams are Low confidence, and they are the two that make the cell special** (the high-conductivity electrolyte and the high-utilization cathode). The abundant streams (Na, S, Al) are exactly the ones the marketing leans on — and exactly the ones that were never the risk. This is the truth gap (`README.md` §8) at the bill-of-materials level: the comfortable numbers are real; the load-bearing ones are uncertain.

---

## 3. Why "Earth-Abundant" Is the Wrong Claim — The Scandium Constraint

This section is the V-Cell's *"Why the Rivers Cannot Help"* (cf. `IGBWP.md` §3): the honest reason the obvious framing fails.

**The marketing claim** *(`Products.md`)*: *"Raw Materials: Abundant. No lithium/cobalt dependency."*

**The V-Cell's own internal correction** *(`SOTA_VALIDATION.md` §4.2, Tier: VERIFIED — "it's a real concern")*:

> Scandium is **not** earth-abundant by any reasonable definition.
> - Crustal abundance: **22 ppm** (vs. aluminum 82,000 ppm, sodium 23,600 ppm)
> - Price: **~$3,500/kg** Sc₂O₃ (2025)
> - Global production: **~25 tonnes/year** Sc₂O₃

The supply-chain math, quoted from the source so no number is invented here:

```
Sc per cell:        0.195 g            (x=0.2 doping, ~10 g electrolyte/cell)
Sc cost per cell:   $0.68              (0.195 g × $3,500/kg)
Sc cost per kWh:    $1.68/kWh          (not a dealbreaker on cost alone)

BUT at scale (Year 5, 500,000 cells/day):
  0.195 g × 500,000 × 365 = 35,588 kg/yr = 35.6 tonnes Sc₂O₃/yr
  → EXCEEDS current global production (~25 t/yr).
```

**This is the bottleneck, stated as conservation of mass:** you cannot pour 35.6 tonnes/year through a 25-tonne/year cross-section. The line's throughput is capped at the scandium it can actually get, no matter how good V-Man is or how cheap sodium is. *(Source tier: VERIFIED concern, PROJECTED demand.)*

### Mitigations — named in the source, framed as supply strategy

The `README.md` §5 discipline (every risk gets a countermeasure) applied to the source's own recommendations:

| Mitigation *(from SOTA §4.2)* | Supply-chain effect | Trade-off |
|-------------------------------|---------------------|-----------|
| **Secure scandium from nickel-laterite tailings** (Rio Tinto, Clean TeQ named) | Opens a *new* source rather than competing for the existing 25 t/yr | New supplier qualification risk; ramp time |
| **Reduce doping x=0.2 → x=0.1** | Halves Sc/cell → ~18 t/yr at scale (still tight) | Lower conductivity → lower rate capability |
| **Substitute yttrium (Y³⁺), ~$30/kg, abundant** | Removes the constraint almost entirely | Slightly lower conductivity (parallel R&D path) |
| **Revise the claim language** to *"lithium-free, cobalt-free"* | Honesty: the claim survives audit; trust preserved | Loses the "abundant" headline — worth losing |

The last row is the most important for the user's theme. The source document *chose truth over a better headline.* That is the `README.md` §6 anti-pattern (dishonest reporting) being actively refused at the design stage — and it is exactly why this product is creditable.

---

## 4. The Second Constraint: VACNT Manufacturing Maturity

*(Source: `SOTA_VALIDATION.md` §5.2, Tier: ASPIRATIONAL.)*

Roll-to-roll CVD for *aligned* carbon nanotubes is the least mature process in the chain. It is also the single largest cost component *(§5.3: VACNT ≈ 24% of cell cost in Year 1)*. Demand vs. demonstrated capability:

```
V-Cell needs: 284 cm²/cell × 1,000 cells/day = 28.4 m²/day  (Year 1)
```

| Supplier *(named in source)* | Method | Throughput | CNT quality |
|------------------------------|--------|-----------|-------------|
| Nanocomp | Floating-catalyst CVD | ~100 m²/day | Random orientation |
| Lintec | Drawable CVD | ~10 m²/day | Aligned but short |
| Tortech Nano | Batch CVD | ~50 m²/day | Aligned, 100 μm height |

The source's honest conclusion: 28.4 m²/day is *within* Tortech's capability but requires *continuous* operation, not batch — and the recommendation is to **license/partner for Years 1–2, build internal capability for Year 3+.** In supply-chain terms (`README.md` §5 Production): qualify a second source *before* you need it, and do not vertically integrate an immature process until it is proven.

**Two bottlenecks, one chain.** Scandium is a *materials* constraint (you can't get enough). VACNT is a *process* constraint (you can't make it fast enough yet). Per continuity (`README.md` §4), the binding one at any moment sets throughput; right now scandium binds at scale and VACNT binds at quality/cost. A truthful plan watches both and does not declare victory by widening the one that isn't binding.

---

## 5. The Seven Stages, Applied to the V-Cell

The `README.md` §5 right/wrong inventory, instantiated with real V-Cell facts. ✅ go-right, ⚠️ go-wrong, → countermeasure.

### 1 — Production (raw inputs)
- ✅ Na, S, Al streams are abundant, cheap, low-carbon (S is a petroleum byproduct; Al is recyclable). The cell's *bulk* is genuinely de-risked.
- ⚠️ **Scandium supply exceeds global production at scale** (§3) — the dominant risk in the whole chain. → Ni-laterite tailings source + Y substitution path + doping reduction.
- ⚠️ Sodium is reactive (Na + H₂O → NaOH + H₂) *(SOTA §3.1 caveat)* — a handling/storage hazard upstream. → Inert-atmosphere handling; moisture-indicating packaging (ties to §7).
- ⚠️ Scandium price volatility (thin ~25 t/yr market; one buyer entering moves the price). → Long-term offtake contracts; hedge via the Y-doped parallel path.

### 2 — Manufacturing (transform) — *see §6*
- ✅ Dry-electrode process avoids NMP solvent recovery → lower cost and carbon *(SOTA §6.4)*.
- ✅ V-Man targets ≥99.2% first-pass yield at 7,200 cells/day *(Products.md)* — *if achieved*, manufacturing stops being the bottleneck.
- ⚠️ **Realistic Year-1 yield is 40–50%, not the aspirational 85%** *(SOTA §5.1, step yields multiply)*. → Start with 20-layer stacks, scale layer count as alignment matures.
- ⚠️ Stack-assembly alignment at 40 layers → shorts (HIGH risk, no Na-S precedent). → Fewer layers first; in-line alignment metrology.

### 3 — Warehousing (buffer) — *the anchor stage*
- ✅ Solid-state cells have superior calendar life (20+ yr projected, no off-gassing) *(SOTA §2.4)* → low obsolescence risk in storage.
- ⚠️ **Sodium-anode and Na-containing WIP are moisture-sensitive** — a cold/dry-chain analog: humidity control, not temperature. → Continuous humidity telemetry + alarms (the `TRACK_TRACE.md` pattern).
- ⚠️ **Scandium and VACNT inventory are extremely high-value per gram** — the shrinkage/theft sink (`README.md` §2) matters far more here than for the bulk Na/S. → Tier-3 tracking on the precious streams; reconcile every shift.
- ⚠️ Phantom inventory on the *precious* streams is catastrophic (you plan a production run you cannot feed). → Sensor-verified counts on Sc/VACNT specifically.

### 4 — Shipping (move)
- ✅ Finished cells are non-thermal-runaway *(SOTA §3.1)* → far safer to ship than Li-ion (a genuine logistics advantage: fewer hazmat restrictions for the *finished* cell).
- ⚠️ Sodium metal and reactive intermediates are **UN dangerous-goods** for inbound transport. → Compliant hazmat handling; this is a Regulation overlap (§6 of README).
- ⚠️ Scandium and CNT shipments are high-value theft targets. → Sealed, sensored, reconciled custody (`TRACK_TRACE.md` §7).

### 5 — Inspection (verify) — *the truth stage*
- ✅ The single most important experiment is well-defined: **synthesize Sc-NASICON, measure σ by EIS** *(SOTA §4.1, §5.4)*. Inspection here *is* the program's first gate.
- ⚠️ The core claim (σ = 10⁻² S/cm) is a **10× extrapolation beyond demonstrated doped NASICON** *(SOTA §4.1, the biggest technical risk)*. → Treat 10⁻² as a stretch goal; design the cell viable at the demonstrated 10⁻³ S/cm.
- ⚠️ Dendrite penetration at high current density is undemonstrated beyond ~1 mA/cm² in Na systems; V-Cell runs to 3.1 mA/cm² *(SOTA §4.4)*. → Post-mortem TEM after 1,000 cycles; independent abuse testing at Sandia/Argonne.
- ⚠️ False-negative inspection on a precious-input cell wastes scandium, not just labor. → 100% EIS on electrolyte lots; destructive sampling budgeted as cost of truth.

### 6 — Regulation (permit)
- ✅ **First market = grid-scale storage** *(SOTA §7.4)* — deliberately chosen because it avoids automotive certification → shorter time to revenue. A regulation-aware strategy, not an accident.
- ✅ The safety story (no thermal-runaway pathway) is the *strongest* claim and the easiest to certify on its merits.
- ⚠️ UL 1642 / UL 9540A, UN 38.3, IEC 62660-1 all pending; no physical test data yet *(SOTA §3.1, §2.4, §7.1)*. → Sandia abuse test first (validates the strongest claim), then Argonne energy-density verification.
- ⚠️ The reactive-sodium-in-water caveat could trigger transport/storage rules for damaged cells. → Hermetic laser-weld seal + moisture-indicating packaging *(SOTA §3.1)*.

### 7 — Pricing (settle) — *see §8*
- ✅ Even at the **conservative 650 Wh/kg and $40/kWh**, the cell beats Amprius/Mercedes/LFP on the metrics grid buyers care about *(SOTA §6)* → honest pricing still wins the first market.
- ⚠️ The $25/kWh Year-5 target is ASPIRATIONAL; realistic is **$35–45/kWh** *(SOTA §5.3)*. → Price the grid-storage entry on the credible number, not the stretch goal.
- ⚠️ Scandium price shock flows straight to margin (thin market). → Index price-adjustment clauses; Y-doped fallback caps the downside.

---

## 6. Manufacturing: V-Man and the Yield-Multiplication Reality

V-Man is the autonomous manufacturing cell that builds the V-Cell *(source: `Products.md`)*: ISO-container factory, 4–6 six-axis arms, Rust-native EtherCAT control, **7,200 cells/day at ≥99.2% first-pass yield** target, software-defined recipes (TOML).

The honest counterweight, from the cell's own diligence *(SOTA §5.1)* — yields **multiply**:

```
Per-step yields (Year 1, realistic):
0.90 (Al lattice etch) × 0.80 (VACNT CVD) × 0.92 (S infiltration)
  × 0.88 (NASICON 30µm tape) × 0.95 (Na evap) × 0.75 (40-layer assembly)
  × 0.98 (laser seal) = 0.42  → 42% overall
```

> **The gap between 99.2% (the V-Man nameplate) and 42% (the realistic integrated yield) is the
> brochure-vs-bottleneck gap from `README.md` §4, made concrete.** 99.2% is a *per-station*
> aspiration; 42% is the *product* of seven real stations early in their learning curve. Both can
> be true. The supply-chain discipline is to plan on the product, not the best station — and to
> publish both, not just the comfortable one.

The source's own countermeasure is pure Theory-of-Constraints: **start at 20 layers (not 40)** to lift assembly yield, accept lower per-cell capacity, and scale layer count as the binding step (alignment) matures. Widen the bottleneck, ignore the stations that already pass.

---

## 7. Inventory Conservation for Reactive & Precious Inputs

`README.md` §2 (inventory is mass; the ledger must close) and `TRACK_TRACE.md` (you can't be honest about what you can't see), applied to the V-Cell's specific materials. **Tier the tracking to the stream** — exactly the `TRACK_TRACE.md` §4 tier-matching rule:

| Stream | Why it needs tracking | Tracking tier | The sink to fear |
|--------|----------------------|---------------|------------------|
| **Scandium / Sc-NASICON** | Precious, thin market, binding constraint | **T3 live** + per-shift reconcile | Theft/loss of a gram skews a whole run |
| **VACNT forest** | High-value, process-limited, fragile | **T3 live** + condition log | Damage in storage = wasted scarce output |
| **Sodium metal** | Reactive (H₂ risk), DG-classified | **T2 condition** (humidity) + sealed | Moisture ingress = safety + scrap |
| **Sulfur** | Cheap, abundant | **T0/T1 identity** | Negligible — don't over-instrument |
| **Aluminum (housing/lattice)** | Abundant, recyclable | **T0/T1 identity** | Negligible |

```rust
// The §2 conservation check, pointed at the stream that actually matters.
// A nonzero scandium sink is never "rounding" — it is grams of a 25-t/yr global supply.
let scandium_ledger = InventoryLedger {
    opening_count: opening_sc_grams,
    units_in: received_sc_grams,
    units_out: consumed_sc_grams,     // 0.195 g/cell × cells built
    closing_count_measured: weighed_sc_grams,
    tolerance: 0.5, // grams — tight, because each gram is ~$3.50 and globally scarce
};
assert!(!scandium_ledger.requires_investigation(), "find the missing scandium before the next run");
```

The moral content (`README.md` §6) is sharpest here: in a chain where one input is genuinely scarce, **hoarding, shrinkage, and dishonest counts are not just inefficiency — they consume a global commons** (the world's 25 t/yr of scandium). The tragedy of the commons is not a metaphor for this cell; it is the literal Year-5 supply situation.

---

## 8. Cost & Pricing — Honestly Tiered

Per-cell cost structure, quoted from `SOTA_VALIDATION.md` §5.3 (Tier: ASPIRATIONAL for the targets):

| Cost component | $/kWh (Yr 1) | $/kWh (Yr 5) | % of total |
|----------------|-------------|-------------|-----------|
| VACNT forest | 25.00 | 6.00 | 24% |
| Sc-NASICON electrolyte | 15.00 | 5.00 | 20% |
| QC / formation / yield loss | 9.40 | 4.77 | 19% |
| Equipment depreciation | 10.00 | 3.00 | 12% |
| Al hex lattice | 8.00 | 2.50 | 10% |
| Housing + terminals | 5.00 | 2.00 | 8% |
| Manufacturing labor | 12.00 | 1.50 | 6% |
| Sodium + sulfur | 0.60 | 0.23 | ~1% |
| **Total** | **$85.00** | **$25.00** | 100% |

The `README.md` §10 honesty test applied: the **true landed cost** is dominated by the two Low-confidence streams (VACNT 24% + Sc-NASICON 20% = 44%) plus yield loss (19%). The cheap, abundant, headline-friendly inputs (Na + S) are ~1%. **Pricing on the abundant inputs would be a lie; the cost lives in the scarce ones and the immature process.**

- **Credible pricing position** *(SOTA §1.1, §5.3, §7.4)*: enter grid storage at ~$85/kWh (Year 1), 650 Wh/kg, 5,000 cycles — which already beats LFP on 20-year cost of ownership. Do **not** price on the $25/kWh / 900 Wh/kg / 10,000-cycle stretch goals.
- **The honest sentence to a customer** *(SOTA §1.1 verbatim recommendation)*: *"V-Cell targets 900 Wh/kg; component data supports 600–700 Wh/kg today; the path to 900 requires three breakthroughs, each demonstrated independently but not yet together."*

---

## 9. Supply-Chain Risk Matrix

Derived from `SOTA_VALIDATION.md` §7.3 / §8, re-cut through the supply-chain lens (impact × probability; probabilities are the source's):

| # | Supply-chain risk | Impact | Prob. | Stage | Countermeasure |
|---|-------------------|--------|-------|-------|----------------|
| 1 | Sc-NASICON σ < 10⁻² S/cm | Critical | 60% | Inspection/Production | Design viable at 10⁻³; EIS gate first |
| 2 | Scandium supply < demand at scale | High | 50% | Production | Ni-laterite source; Y substitution; lower doping |
| 3 | VACNT roll-to-roll CVD immaturity | High | 45% | Manufacturing | License Yrs 1–2; internal Yr 3+ |
| 4 | Na dendrite at high current density | Critical | 40% | Inspection | ALD interlayer; 100-cycle gate |
| 5 | 40-layer assembly yield (shorts) | Medium | 40% | Manufacturing | Start at 20 layers |
| 6 | Cost target $25/kWh unmet | Medium | 55% | Pricing | Price on $35–45/kWh realistic |
| 7 | Scandium price shock (thin market) | Medium | — | Pricing/Production | Offtake contracts; Y-doped hedge |

Two of the top three are pure supply-chain risks (scandium supply, VACNT process) — confirming §1: for this product, the chain, not the chemistry, is the binding problem.

---

## 10. Phased Plan (Aligned to the V-Cell Roadmap)

Mapped onto `SOTA_VALIDATION.md` §9, with the supply-chain workstream made explicit alongside the technical one:

```
PHASE 1 — PROOF OF CONCEPT (Months 1–6)
  Technical: synthesize Sc-NASICON, measure σ by EIS  ← THE gate
  Supply:    qualify Ni-laterite scandium source; start Y-doped parallel path
             license a VACNT supplier (don't build CVD yet)
  Gate:      σ < 10⁻³ S/cm → pivot electrolyte

PHASE 2 — CELL-LEVEL PROTOTYPE (Months 6–12)
  Technical: single bi-cell; target 500+ Wh/kg; 100 cycles
  Supply:    T3 tracking live on Sc + VACNT; reconcile ledger every shift
  Gate:      <400 Wh/kg → reassess mass budget

PHASE 3 — MULTI-LAYER PROTOTYPE (Months 12–18)
  Technical: 10-layer stack; full safety suite; Sandia/Argonne
  Supply:    lock scandium offtake; second VACNT source qualified
  Regulation: begin UL 9540A / UN 38.3 for grid-storage entry

PHASE 4 — PILOT PRODUCTION (Months 18–24)
  Technical: 20-layer, 200 Wh; V-Man dry line commissioned
  Supply:    first true 40–50% integrated yield measured & published
             scandium demand vs. global supply re-checked at pilot volume

PHASE 5 — SCALE-UP (Months 24–36)
  Technical: 40-layer full V-Cell; V-Man at 100 cells/day → 7,200/day target
  Supply:    THE scaling decision — is scandium throughput real at volume?
             If not, ship the Y-doped variant. Do not scale into a 25-t/yr wall.
```

The Phase-5 line is the whole point: **scaling is gated by the supply bottleneck, not the bench result.** A cell that works perfectly but cannot get its scandium is, at volume, a cell that does not ship.

---

## 11. Data Integrity

This document inherits the V-Cell's own three-tier honesty framework *(`SOTA_VALIDATION.md` Preface)* and the `README.md` §8 ethic:

- **VERIFIED** here means *the source verified it* (e.g., scandium scarcity is a real, cited concern). It does not mean a physical V-Cell was tested — none has been built *(SOTA §5.4: pre-prototype, simulation only)*.
- **PROJECTED / ASPIRATIONAL** numbers (energy density, σ, cost, yield) keep the source's tier and are **not** upgraded here.
- Scandium price (~$3,500/kg) and global production (~25 t/yr) are the **source document's stated 2025 figures**; treat as `Unverified` against today's market until re-quoted — the same discipline `IGBWP.md` applies to aquifer depletion rates.
- No new performance claim about the V-Cell is created in this document. Where the source corrected a marketing claim toward truth (§3, "earth-abundant"), this document follows the correction.

> The reason this can be a *commendable* document and not a cynical one: the source material already
> chose truth over a better headline. This analysis simply carries that choice into the supply chain,
> where the scarce gram of scandium is, in the end, a shared global thing — and honesty about it is
> the same honesty the WATER project demanded about a shared aquifer.

---

## 12. References

**Canonical Voltec source (SACRED — derived from, never modified):**
- `E:\Workspace\Voltec\docs\Products\V-Cell\README.md` — bill of materials, component instances
- `E:\Workspace\Voltec\docs\Products\V-Cell\SOTA_VALIDATION.md` — honesty-tiered diligence; scandium (§4.2), VACNT (§5.2), yield (§5.1), cost (§5.3), risk (§7.3/§8), roadmap (§9)
- `E:\Workspace\Voltec\docs\Products\Products.md` — V-Cell → V-Pack → V-Grid lineage; V-Man; V-Core/V-Supreme buffer role
- `E:\Workspace\Voltec\docs\Products\V-Cell\V1\VCell_Assembly_40Layer.glb.toml` — 40-layer stack, 8,100 Ah, 2.23 V

**Eustress realism crate (verified, real):**
- `eustress/crates/common/src/realism/constants.rs` → `vcell_materials::{sodium, sc_nasicon, sulfur_vacnt, al_hex_lattice, aluminum_nitride, al_6061_t6}`, `na_s` electrochemistry
- `eustress/crates/common/src/realism/laws/conservation.rs` → `mass_conservation_check` (the inventory ledger of §7)

**Framework documents (this folder):**
- [`README.md`](README.md) — Supply Chain Flow Analysis (the general framework this applies)
- [`TRACK_TRACE.md`](TRACK_TRACE.md) — the measurement layer that makes §7's tracking real

---

*Document created: May 29, 2026*
*Folder: PRODUCTION — concrete application of the supply chain flow analysis to the Voltec V-Cell*
*Derived from the sacred Voltec source; tiers and figures preserved from `SOTA_VALIDATION.md`*
*The chemistry is the easy part. The scandium is the bottleneck.*
