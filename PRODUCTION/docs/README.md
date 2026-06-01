# Supply Chain Flow Analysis

> **Using the Eustress Engine Realism Crate to model goods the way we model water**
>
> A supply chain is a pipe. Goods are the fluid. The same conservation laws that govern
> water moving from ocean to aquifer govern raw material moving from mine to doorstep.
> This document treats the supply chain as a physical flow problem — and treats the
> human beings inside it as the part most likely to break, because they are.

---

## Table of Contents

1. [Problem Statement](#1-problem-statement)
2. [The Supply Chain Is a Conservation Law](#2-the-supply-chain-is-a-conservation-law)
3. [The Seven Stages](#3-the-seven-stages)
4. [Flow Physics: Throughput, Little's Law, and the Bottleneck](#4-flow-physics-throughput-littles-law-and-the-bottleneck)
5. [What Could Go Right / What Could Go Wrong — The Full Inventory](#5-what-could-go-right--what-could-go-wrong--the-full-inventory)
6. [The Human Failure Modes (Sin, Honestly Named)](#6-the-human-failure-modes-sin-honestly-named)
7. [Implementation with the Realism Crate](#7-implementation-with-the-realism-crate)
8. [Data Integrity & Verification Requirements](#8-data-integrity--verification-requirements)
9. [0-1 Strategy Matrix: Vertical & Horizontal](#9-0-1-strategy-matrix-vertical--horizontal)
10. [Pricing: Where the Whole Chain Settles Its Accounts](#10-pricing-where-the-whole-chain-settles-its-accounts)
11. [Project Phases](#11-project-phases)
12. [Public Transparency & Dissemination](#12-public-transparency--dissemination)
13. [References](#13-references)

---

## 1. Problem Statement

**Given:**
- A finished good a person needs (medicine, food, a battery cell, a part).
- A chain of stages — production, manufacturing, warehousing, shipping, inspection, regulation, pricing — each operated by a different party with different incentives.
- No single party can see the whole chain. Each sees its own stage and a rumor of the next.

**Find:**
- The throughput the chain can actually sustain (not the throughput anyone claims).
- The single binding constraint (the bottleneck) that sets that throughput.
- The full list of ways the chain delivers (what could go right) and the full list of ways it fails (what could go wrong) — including the failures that come from people, not parts.
- The minimum instrumentation needed to know the chain's true state rather than its reported state.

**The core failure mode (one sentence):**

> A supply chain fails not when a stage runs out of capacity, but when the people running it
> stop telling each other the truth about capacity — and the math, which does not lie, keeps
> running on numbers that do.

This is the same structure as the WATER project's central finding: *"India does not know its own pumping rate."* A supply chain almost never knows its own true inventory, lead time, or defect rate. The largest source of error is not the physics. It is the gap between **reported state** and **real state** — and that gap is usually opened on purpose.

---

## 2. The Supply Chain Is a Conservation Law

The Eustress realism crate already implements the law that governs every supply chain. It is in `eustress/crates/common/src/realism/laws/conservation.rs`, and it was written for water:

```rust
/// Check mass conservation in a system
/// Returns the difference from initial mass (should be ~0)
pub fn mass_conservation_check(initial_mass: f32, current_masses: &[f32]) -> f32 {
    let current_total: f32 = current_masses.iter().sum();
    current_total - initial_mass
}
```

### Inventory is mass. Shrinkage is a violation that demands an explanation.

In a closed warehouse over a period:

```
units_in  −  units_out  −  units_on_hand_change  =  0     (if mass is conserved)
```

When that equation does **not** balance, the difference is not noise. It is a real, physical sink — and the supply chain version of "mass left the system" has names: **theft, spoilage, miscount, damage, fraud, phantom inventory.** The honest operator treats a nonzero `mass_conservation_check` exactly the way a physicist does: *something left through a hole I have not yet found.* The dishonest operator rounds it to zero and books the profit.

> **This is the whole ethic of this document in one function call.** A discrepancy is a sink
> you owe an explanation for. You do not get to ignore it because it is small, and you do not
> get to invent inventory to close it because that is convenient.

### Throughput is flow rate.

```rust
/// Volume flow rate: Q = Av
pub fn volume_flow_rate(area: f32, velocity: f32) -> f32 {
    area * velocity
}
```

A stage's throughput `Q` (units/day) is its capacity cross-section `A` times the velocity `v` at which work moves through it. You raise throughput by widening the pipe (more capacity) or speeding the flow (less friction) — never by wishing.

### The bottleneck is the continuity equation.

```rust
/// Velocity from continuity: v₂ = (A₁/A₂)v₁ (incompressible)
pub fn velocity_from_continuity(area1: f32, velocity1: f32, area2: f32) -> f32 {
    if area2 <= 0.0 {
        return f32::INFINITY;   // a fully closed stage stalls everything upstream
    }
    (area1 / area2) * velocity1
}
```

When flow hits a narrow section it must speed up — or pile up. In a pipe, narrowing raises velocity and drops pressure (Bernoulli). In a supply chain, the narrowest stage sets the throughput of the **entire** chain, and inventory piles up (WIP, queues, demurrage) immediately upstream of it. This is the Theory of Constraints, and it is just continuity. **Optimizing any stage that is not the bottleneck produces zero additional delivered goods.** That is not an opinion; it is conservation of mass.

### The ledger must close.

The crate's `ConservationTracker` checks mass, energy, and momentum against an initial state within a tolerance. The supply-chain analog is a reconciliation ledger: at every handoff, the books must close to within a stated tolerance, and a failure to close is an alarm, not an embarrassment to be hidden.

---

## 3. The Seven Stages

The chain the user named, modeled as serial flow segments. Each has a capacity (`A`), a velocity/lead-time (`v`), a yield (fraction that survives the stage), and a **truth gap** (how far reported state drifts from real state).

```
 PRODUCTION → MANUFACTURING → WAREHOUSING → SHIPPING → INSPECTION → REGULATION → PRICING
 (raw input)   (transform)      (buffer)     (move)     (verify)     (permit)     (settle)
```

| # | Stage | What it does | The flow variable | The thing it conserves | The lie it tells |
|---|-------|--------------|-------------------|------------------------|------------------|
| 1 | **Production** | Extracts/grows raw input | Yield per unit time | Material mass | "Reserves are larger than they are" |
| 2 | **Manufacturing** | Transforms input → product | Cycle time, first-pass yield | Mass + value added | "Defect rate is lower than it is" |
| 3 | **Warehousing** | Buffers supply against demand | Stock level, turns/year | Inventory (mass) | "On-hand count matches the system" |
| 4 | **Shipping** | Moves goods across distance | Transit time, on-time % | Mass + location | "It shipped today" (it didn't) |
| 5 | **Inspection** | Verifies the goods are real & good | Detection rate, false-negative rate | Truth | "Passed QC" (it was waved through) |
| 6 | **Regulation** | Licenses the right to operate | Approval latency, compliance % | Legitimacy | "We are compliant" (on paper only) |
| 7 | **Pricing** | Settles cost, margin, and risk | Price, elasticity, margin | Money (accounting identity) | "This price reflects cost" (it reflects power) |

Warehousing — the user's anchor stage — is special: it is the **capacitor** of the chain. It stores flow so that upstream and downstream can run at different rates without stalling. A warehouse is the supply chain's safety stock, its shock absorber, and the single place where the truth gap is most easily hidden (a phantom pallet is invisible until someone walks the aisle). That is why the companion document [`TRACK_TRACE.md`](TRACK_TRACE.md) — the metering layer — exists: **you cannot be honest about a buffer you cannot see.**

---

## 4. Flow Physics: Throughput, Little's Law, and the Bottleneck

### Little's Law — the supply chain's continuity equation

```
L = λ × W

  L = average inventory in the system (units)
  λ = average throughput (units / time)
  W = average flow time (time a unit spends in the system)
```

This is exact, assumption-free, and the most important equation in operations. It is the discrete-goods form of `volume_flow_rate(area, velocity)`: inventory is the integral of flow over residence time. Three honest consequences:

1. **You cannot cut lead time `W` without cutting inventory `L` or throughput `λ`.** Pick which.
2. **Piles of inventory are not wealth — they are slow flow time.** High `L` with flat `λ` means `W` is large: goods are sitting, aging, tying up cash.
3. **A bottleneck caps `λ` for the whole chain.** Everything upstream of it inflates `L` (queues); everything downstream of it starves.

### Finding the bottleneck honestly

```rust
/// The bottleneck is the stage with the smallest effective capacity.
/// Optimizing anything else delivers zero extra goods (conservation of mass).
fn find_bottleneck(stages: &[StageFlow]) -> usize {
    stages
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.effective_throughput()
                .partial_cmp(&b.effective_throughput())
                .unwrap()
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}
```

The discipline: measure each stage's *demonstrated* throughput under load, not its *nameplate* throughput from the brochure. The brochure is a lie of optimism; the bottleneck hides behind it.

---

## 5. What Could Go Right / What Could Go Wrong — The Full Inventory

This is the core of the document. For each stage: **what could go right** (the upside we are building toward) and **what could go wrong** (the failure modes, named without flinching), and a **countermeasure** for each major risk — because the goal is to be *advancing and commendable*, not merely to catalogue doom.

> **Reading note:** ✅ = opportunity (go right). ⚠️ = risk (go wrong). → = countermeasure.
> Risks are roughly ordered most-common/most-damaging first.

### Stage 1 — Production (raw input)

**✅ What could go right**
- Stable, diversified sourcing means no single failure starves the chain.
- Yield improvements at the source multiply through every downstream stage (a 2% better ore grade is 2% more of *everything* after it).
- Local/regional sourcing shortens `W`, cuts emissions, and creates jobs near the point of need (the WATER project's "Manufacturing & Employment Pipeline" logic).
- Byproduct recovery turns waste into a second revenue stream (cf. zero-liquid-discharge mineral recovery in WATER).
- Long-term supplier relationships build the trust that makes honest capacity-sharing possible.

**⚠️ What could go wrong**
- ⚠️ **Single-source dependence.** One mine, one farm, one fab. A flood, strike, or sanction zeroes the chain. → Qualify a second source *before* you need it; pay the premium as insurance.
- ⚠️ **Reserve overstatement.** "We have years of supply" when assays were optimistic or political. → Independent assay; treat reserves as `Option<f64> = None` until verified (see §8).
- ⚠️ **Resource depletion / quality decline.** The seam thins, the soil salts, the aquifer drops. Output holds while quality silently falls. → Meter the *quality*, not just the quantity.
- ⚠️ **Geographic concentration risk.** 90% of a critical mineral from one country. → Strategic reserve + recycling stream.
- ⚠️ **Weather / climate shocks** (drought, flood, freeze) hitting agricultural or water-dependent inputs. → Buffer stock sized to the *historical worst case*, not the average.
- ⚠️ **Forced/child labor or environmental crime upstream**, invisible until an audit or a journalist finds it. → Chain-of-custody tracing to the source mine/farm, not just the last vendor.

### Stage 2 — Manufacturing (transform)

**✅ What could go right**
- High first-pass yield means few defects propagate downstream (defects caught late cost 10–100× more — the "1-10-100 rule").
- Flexible/modular lines let you re-tool for demand shifts without scrapping the plant.
- Automation lifts throughput `λ` and removes a class of human error — *and* a class of human discretion to cheat.
- Process knowledge compounds: every defect understood is a defect designed out.
- Distributed manufacturing (many small plants) is more resilient than one megafactory (WATER's "3 plants × 3 GW beats one 9 GW monolith").

**⚠️ What could go wrong**
- ⚠️ **Quality drift.** Tooling wears, a supplier substitutes a cheaper resin, tolerances creep. The line still runs; the product is quietly worse. → SPC (statistical process control) on the *output*, with alarms on trend, not just on spec-violation.
- ⚠️ **Single point of failure in capacity** (one furnace, one cleanroom). → Identify it explicitly as the bottleneck (§4) and protect it.
- ⚠️ **Hidden defect / latent failure** that passes inspection and fails in the customer's hands (Takata airbags, exploding cells). → Destructive sampling + traceable lot genealogy so a recall is surgical, not total.
- ⚠️ **Counterfeit components entering the BOM** (especially electronics). → Authenticated parts, supplier-of-record, incoming inspection (§Inspection).
- ⚠️ **Capacity overstatement to win the order**, then failure to deliver. → Contractual demonstrated-rate test before volume commitment.
- ⚠️ **IP/process theft** collapsing the moat that justified the investment. → Compartmentalized process knowledge; trust but segment.
- ⚠️ **Scaling a process that worked in pilot but not at volume** (the pilot lied). → WATER's Phase 2 discipline: validate models against *real* data before scale-up.

### Stage 3 — Warehousing (buffer) — *the anchor stage*

**✅ What could go right**
- Right-sized safety stock absorbs upstream shocks so downstream never starves — the capacitor doing its job.
- High inventory *turns* (low `W`) means fresh goods, low carrying cost, low obsolescence.
- Accurate real-time location means O(1) pick, not O(n) search.
- Cross-docking and flow-through cut `W` toward zero for fast-movers.
- A warehouse that *knows its own true state* becomes the trusted reference for the whole chain.
- Climate/condition control preserves perishable and sensitive goods (cold chain, ESD, humidity).

**⚠️ What could go wrong**
- ⚠️ **Phantom inventory.** The system says 100; the shelf has 80. Every downstream promise built on the 100 is a lie the chain doesn't know it's telling. *(Studies of retail put inventory-record inaccuracy near or above half of all SKUs.)* → Cycle counting + sensor-verified counts (`TRACK_TRACE.md`); treat the ledger discrepancy as a `mass_conservation_check` alarm.
- ⚠️ **Shrinkage** — theft, miscount, damage, spoilage, fraud. The unexplained sink in the conservation equation. → Reconcile every period; investigate every nonzero residual; do not "adjust to system."
- ⚠️ **Stockout** of a critical SKU (lost sales, line-down, patient without medicine). → Service-level targets set by *criticality*, not by uniform policy.
- ⚠️ **Overstock / obsolescence.** Cash frozen in goods that age out (the high-`L`, low-`λ` trap of Little's Law). → Turn-rate alarms; markdown discipline; don't confuse a full warehouse with a healthy one.
- ⚠️ **Spoilage / cold-chain break.** A freezer fails at 2 a.m. and no one knows until the morning. → Continuous condition telemetry with night-time alarms (the WELL_METER pattern: 15-min readings, 24/7).
- ⚠️ **Misplacement** — the goods exist but are unfindable (functionally a stockout). → Location discipline + scan-on-move.
- ⚠️ **Bullwhip amplification.** A small demand wobble downstream becomes a violent swing in warehouse orders upstream (see §6 — this is a *trust/information* failure, not a storage failure). → Share real point-of-sale demand upstream; stop each tier guessing.
- ⚠️ **Single-warehouse concentration.** One fire, flood, or ransomware event takes out the whole buffer. → Geographic distribution; offline-capable records.
- ⚠️ **WMS/system outage or ransomware** freezing all movement. → Paper-fallback drills; immutable, exportable records.

### Stage 4 — Shipping (move)

**✅ What could go right**
- Reliable, predictable transit time lets every downstream stage plan tightly (low variance is worth more than low mean).
- Multi-modal options (sea/rail/road/air) give re-routing flexibility around disruption.
- Consolidation and backhaul cut cost and emissions per unit.
- Real-time tracking turns "it's somewhere on a ship" into a known position and ETA.
- Resilient routing survives a canal blockage or port strike without stopping the chain.

**⚠️ What could go wrong**
- ⚠️ **Transit-time variance** (worse than long transit — you can plan for slow; you can't plan for *unpredictable*). → Measure and publish the *distribution*, buffer to a high percentile.
- ⚠️ **Chokepoint failure** — a canal, strait, port, or border (Suez 2021, every port during 2021–22). → Pre-identified alternate routes; don't assume the chokepoint stays open.
- ⚠️ **In-transit loss, theft, or damage.** Goods leave, fewer arrive — a `mass_conservation_check` failure across the move. → Sealed, sensored containers; reconcile at every transfer of custody.
- ⚠️ **"It shipped" when it didn't.** The most common lie in the chain — a label printed is booked as a shipment. → Confirm by *scan at gate-out*, not by intent.
- ⚠️ **Demurrage / detention** — goods stuck at a port racking up fees, invisible until the invoice. → Dwell-time alarms.
- ⚠️ **Last-mile failure** (the hardest, most expensive leg). → Density planning; honest delivery-window promises.
- ⚠️ **Cold-chain or shock damage in transit** (pharma, electronics, produce). → Condition loggers that travel *with* the goods.
- ⚠️ **Customs hold / documentation error** stranding a shipment at a border (overlaps Regulation, §6). → Documents complete and correct *before* dispatch.

### Stage 5 — Inspection (verify) — *the truth stage*

**✅ What could go right**
- Catching a defect here is 10–100× cheaper than catching it at the customer.
- Statistically sound sampling gives real confidence at low cost.
- Automated/vision inspection removes inspector fatigue and discretion.
- A trusted inspection record lets downstream skip re-inspection (trust, earned by evidence, removes friction).
- Inspection data fed back upstream *prevents* the next defect (the loop, not just the gate).

**⚠️ What could go wrong**
- ⚠️ **False negatives** — bad goods passed as good (the dangerous error). → Track the false-negative rate explicitly; it is the inspection's true quality metric.
- ⚠️ **Rubber-stamping.** The inspection is recorded as done but wasn't (the WATER "well it down for maintenance but logged as read" failure). → Inspector accountability + audit-the-auditor sampling.
- ⚠️ **Inspecting the wrong thing** — measuring what's easy, not what matters. → Inspect against *failure modes*, not against convenience.
- ⚠️ **Sampling that misses rare-but-fatal defects.** → Risk-weighted sampling; 100% inspection for safety-critical traits.
- ⚠️ **Capture / collusion** — the inspector is paid (or pressured) by the inspected. → Independence; rotate inspectors; separate who-pays from who-judges.
- ⚠️ **Inspection as theater** — present for the audit, absent in daily operation. → Continuous in-line inspection, not episodic show inspection.
- ⚠️ **Destructive-test starvation** — skipping the tests that actually prove reliability because they cost units. → Budget destructive sampling as cost of truth.

### Stage 6 — Regulation (permit)

**✅ What could go right**
- Clear, stable rules let everyone invest with confidence (the rule of law is throughput infrastructure).
- Safety/quality floors protect the end person and keep honest players from being undercut by cheats.
- Standardization (containers, units, certs) is why global trade flows at all.
- Mutual recognition between jurisdictions removes duplicate compliance cost.
- Good regulation *is* the countermeasure to the tragedy of the commons (§6).

**⚠️ What could go wrong**
- ⚠️ **Compliance theater** — compliant on paper, not in reality (the deadliest and most common). → Outcome audits, not document audits.
- ⚠️ **Regulatory capture** — the regulated write the rules to entrench themselves and block entrants. → Transparency; independent review; sunset clauses.
- ⚠️ **Approval latency** as the bottleneck — a safe product stuck for years in review while people go without. → Time-boxed review with default-grant or transparent reason for delay.
- ⚠️ **Jurisdictional conflict** — legal here, illegal there; the chain crosses both. → Map the *binding* constraint (strictest applicable rule) explicitly.
- ⚠️ **Regulatory whiplash** — rules change mid-investment, stranding capital. → Grandfathering; phase-in periods.
- ⚠️ **Corruption / bribery at the permit gate.** → Digitize and timestamp every approval; remove the human discretion that bribery feeds on.
- ⚠️ **Over-regulation that strangles** the small/new player while the incumbent absorbs the cost. → Proportionate, tiered compliance.
- ⚠️ **Under-regulation that lets a bad actor poison the commons** (tainted food, fake medicine, unsafe cells). → Floors that are enforced, not just written.

### Stage 7 — Pricing (settle)

**✅ What could go right**
- A price that reflects true cost + fair margin sustains the whole chain indefinitely.
- Transparent pricing builds the trust that lowers everyone's transaction cost.
- Price signals route scarce goods to highest need/value (the market doing its honest job).
- Long-term contracts trade some upside for the stability that lets suppliers invest.
- Value-based pricing funds the R&D that makes the next generation cheaper for everyone.

**⚠️ What could go wrong**
- ⚠️ **Price that doesn't cover true cost** (often because true cost is hidden in shrinkage, returns, or unpriced risk). → Cost the *whole* chain, including the conservation-law sinks.
- ⚠️ **Price gouging in a shortage** — extracting from desperation (the moral failure the user names). → Pre-committed surge caps; reputation as a long-term asset.
- ⚠️ **Hidden cost-shifting** — margin made by pushing cost onto a weaker party, the environment, or the future. → Honest full-cost accounting; externalities are still costs.
- ⚠️ **Predatory pricing** to kill a competitor, then monopoly pricing after. → Anti-trust; the regulation stage as backstop.
- ⚠️ **Opaque pricing** that hides where the money goes and breeds distrust. → Open the cost structure to partners who've earned it.
- ⚠️ **Demand misread → mispriced** (left money on the table, or priced out the buyer). → Elasticity measured, not assumed.
- ⚠️ **Currency / commodity / tariff swings** wiping out margin between order and settlement. → Hedging; price-adjustment clauses tied to *published* indices.
- ⚠️ **The race to the bottom** — everyone cuts price by cutting quality/wages/safety until the commons is exhausted. → This is §6's tragedy of the commons in pricing clothes; the only fix is an enforced floor + buyers who value truth.

### Cross-cutting: the whole-chain failures

- ⚠️ **The bullwhip effect.** Demand variance amplifies up each tier because each tier hedges against the tier below's *orders* instead of the end customer's *real demand*. It is a trust-and-information failure dressed as a forecasting problem. → Share real downstream demand; the cure is transparency, not better guessing.
- ⚠️ **The truth gap compounds.** Each stage's small optimistic lie multiplies: 95% honest × 7 stages ≈ 70% of the truth survives to the end. → Measure real state at each handoff (§8, `TRACK_TRACE.md`).
- ⚠️ **Local optimization, global loss.** Every stage hitting its own KPI while the chain delivers less (conservation of mass: only the bottleneck's output is real output). → Optimize the chain, reward the chain.
- ✅ **The whole-chain win.** When every stage tells the truth about its real state and shares it, the bottleneck becomes visible, the bullwhip dies, inventory drops, lead time drops, and the end person gets the good — cheaper, faster, and actually real.

---

## 6. The Human Failure Modes (Sin, Honestly Named)

The WATER project's blunt finding bears repeating: *"The engineering is the easy part. The governance problem is the actual blocker."* For a supply chain, the governance problem is human nature. These are not soft factors. They are the dominant failure modes, and pretending otherwise is itself a form of the dishonesty being described.

| Failure | What it looks like in the chain | Where it bites | Countermeasure |
|---------|--------------------------------|----------------|----------------|
| **Tragedy of the commons** | Each party draws down a shared resource (port capacity, a supplier's goodwill, a finite buffer, the planet) faster than it regenerates, because the cost is shared and the gain is private | Production, Warehousing, Regulation | Meter the commons; price the externality; enforce a floor. Make the shared cost visible and personal. |
| **Principal–agent gap** | The person doing the work has different incentives than the owner of the outcome (the warehouse clerk paid for speed, not accuracy; the inspector paid by the inspected) | Inspection, Manufacturing, Shipping | Align reward with the true outcome; separate who-judges from who-benefits; audit the agent. |
| **Envy / hoarding** | A party hoards stock or capacity it doesn't need so a rival can't have it — starving the chain to spite a competitor | Warehousing, Shipping | Transparency removes the information asymmetry envy feeds on; allocation rules based on need. |
| **Greed / gouging** | Extracting maximum short-term price from a captive or desperate counterparty, spending long-term trust for short-term margin | Pricing | The relationship is the asset; reputation compounds; surge caps pre-committed in calm times. |
| **Moral hazard** | A party takes risk because someone else bears the loss (skipping the destructive test, deferring maintenance, shipping the marginal lot) | Inspection, Manufacturing | Make the risk-taker bear the consequence; traceability so the loss finds its source. |
| **Dishonest reporting** | "It shipped." "It passed." "Stock is 100." The optimistic lie that keeps the math running on false numbers | Every stage | Measure real state independently of the party reporting it (§8). The whole point of `TRACK_TRACE`. |
| **Bystander / diffusion** | Everyone sees the discrepancy; no one owns it; it rounds to zero | Warehousing, Inspection | Assign the residual to a named owner. A discrepancy without an owner is a discrepancy that grows. |
| **Cynicism / learned helplessness** | "The chain is always broken, why bother measuring." The most corrosive, because it ends the search for truth | Leadership, culture | Small, visible wins; show that a measured discrepancy *gets fixed*. Advancing, not maniacal. |

### On leadership and hierarchy (the user's point, taken seriously)

A supply chain is a hierarchy because flow requires sequencing, and sequencing requires someone accountable at each node and someone accountable for the whole. The healthy version: hierarchy exists to **accommodate the busyness of the most capable** and to **place accountability where the information is** — not to extract status. The unhealthy version is status-seeking dressed as structure, where rank is for taking credit (greed) rather than for carrying responsibility.

The test is simple and honest: *does this layer of hierarchy make the truth travel faster, or slower?* A good leader builds hierarchy that surfaces bad news quickly (the bottleneck, the shrinkage, the failed lot) and adapts when a key person is overloaded or absent. A bad hierarchy punishes the messenger, so the messages stop, so the math runs on lies, so the chain breaks — and the person at the end of it does not get their medicine. **We judge the system by the end person, not the org chart.**

---

## 7. Implementation with the Realism Crate

Real code, referencing the real modules verified in `eustress/crates/common/src/realism/`. These types are illustrative models in the WATER documentation style — they show how the supply chain maps onto the engine's existing conservation machinery.

```rust
//! Supply-chain flow model built on the realism crate's conservation laws.
//! Maps goods-flow onto the same continuity / mass-conservation primitives
//! used for water in `realism::laws::conservation`.

use bevy::prelude::*;
use crate::realism::laws::conservation::{
    mass_conservation_check, volume_flow_rate, velocity_from_continuity,
};

/// One stage of the chain, modeled as a flow segment.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct StageFlow {
    /// Stage name (Production, Manufacturing, …)
    pub name: String,
    /// Nameplate capacity cross-section (units/day at full speed) — the BROCHURE number.
    pub nameplate_capacity: f32,
    /// Demonstrated velocity factor under real load, 0.0–1.0 — measured, not claimed.
    pub demonstrated_velocity: f32,
    /// First-pass yield: fraction of input that survives this stage (0.0–1.0).
    pub yield_fraction: f32,
    /// Truth gap: |reported_state − real_state| / real_state. 0.0 = honest.
    /// Left as None until independently measured — see §8. We do NOT assume 0.
    pub truth_gap: Option<f32>,
}

impl StageFlow {
    /// Effective throughput = the only number that delivers real goods.
    /// Q = A·v, then derated by yield. The brochure capacity is irrelevant.
    pub fn effective_throughput(&self) -> f32 {
        volume_flow_rate(self.nameplate_capacity, self.demonstrated_velocity)
            * self.yield_fraction
    }

    /// Confidence in this stage's reported numbers.
    /// An unmeasured truth gap is the LOWEST confidence, not the highest —
    /// silence is not evidence of honesty.
    pub fn confidence(&self) -> Confidence {
        match self.truth_gap {
            None => Confidence::Unverified,
            Some(g) if g < 0.02 => Confidence::High,
            Some(g) if g < 0.10 => Confidence::Medium,
            Some(_) => Confidence::Low,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Reflect, Serialize, Deserialize)]
pub enum Confidence { High, Medium, Low, Unverified }

/// Inventory reconciliation for a warehouse over a period.
/// This is `mass_conservation_check` wearing a hard hat.
#[derive(Debug, Clone, Reflect)]
pub struct InventoryLedger {
    pub opening_count: f32,
    pub units_in: f32,
    pub units_out: f32,
    pub closing_count_measured: f32,
    /// Tolerance below which a discrepancy is noise, not an alarm.
    pub tolerance: f32,
}

impl InventoryLedger {
    /// The shrinkage / phantom-inventory sink. Should be ~0.
    /// Reuses the engine's mass-conservation check directly: the expected
    /// closing balance is the "initial mass", the measured count is the
    /// single "current mass" we compare against.
    pub fn unexplained_sink(&self) -> f32 {
        let expected_closing = self.opening_count + self.units_in - self.units_out;
        mass_conservation_check(expected_closing, &[self.closing_count_measured])
    }

    /// An honest operator returns true here and then FINDS THE HOLE.
    /// A dishonest one edits `closing_count_measured` to match the system.
    pub fn requires_investigation(&self) -> bool {
        self.unexplained_sink().abs() > self.tolerance
    }
}

/// The bottleneck speed-up: when flow hits a narrower stage it must accelerate
/// or pile up. Identical to fluid continuity — `velocity_from_continuity`.
pub fn queue_pressure(upstream: &StageFlow, downstream: &StageFlow) -> f32 {
    velocity_from_continuity(
        upstream.nameplate_capacity,
        upstream.demonstrated_velocity,
        downstream.nameplate_capacity,
    )
}
```

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinkage_is_a_conservation_violation() {
        // Opened with 100, received 50, shipped 40 → books say 110.
        // Walked the aisle: 104. Six units left through a hole.
        let ledger = InventoryLedger {
            opening_count: 100.0,
            units_in: 50.0,
            units_out: 40.0,
            closing_count_measured: 104.0,
            tolerance: 1.0,
        };
        assert!((ledger.unexplained_sink() - (-6.0)).abs() < 1e-3);
        assert!(ledger.requires_investigation()); // do NOT round to zero
    }

    #[test]
    fn unmeasured_truth_gap_is_low_confidence_not_high() {
        let stage = StageFlow {
            name: "Manufacturing".into(),
            nameplate_capacity: 1000.0,
            demonstrated_velocity: 0.8,
            yield_fraction: 0.95,
            truth_gap: None, // nobody measured it
        };
        assert_eq!(stage.confidence(), Confidence::Unverified);
        // Effective throughput = 1000 * 0.8 * 0.95 = 760, not the 1000 on the brochure.
        assert!((stage.effective_throughput() - 760.0).abs() < 1e-3);
    }
}
```

---

## 8. Data Integrity & Verification Requirements

> **DATA INTEGRITY NOTICE** *(modeled directly on the WATER project's aquifer-data notice)*
>
> Every throughput, yield, lead time, and inventory count in any real application of this
> framework must be left as `None` / `Unverified` until measured from primary source. Reported
> supply-chain numbers are systematically optimistic for the same reasons aquifer depletion rates
> are systematically uncertain:
> - **Different methods disagree** (system count vs. physical count vs. sensor count).
> - **Incentives distort** (everyone's bonus depends on their stage looking good).
> - **Heterogeneity hides** (the basin average looks fine; the one critical SKU is empty).
> - **Time-variance masks** (the quarter-end number is gamed; the daily truth is worse).
>
> **Before trusting any number, verify against:** an independent physical/sensor count, a
> reconciled ledger that closes (§2), and a demonstrated-rate test under real load (§4).

| Data gap | Typical reported state | Required action | Priority |
|----------|------------------------|-----------------|----------|
| True on-hand inventory | System count (optimistic) | Cycle count + sensor verification (`TRACK_TRACE`) | **P0** |
| True bottleneck throughput | Nameplate capacity (brochure) | Demonstrated-rate test under load | **P0** |
| True first-pass yield | "About 95%" | Lot-traced defect data | **P0** |
| True transit-time distribution | "Usually a week" | Per-shipment timestamped tracking | P1 |
| True inspection false-negative rate | "We catch everything" | Audit-the-auditor sampling | P1 |
| True landed cost (incl. sinks) | Invoice price | Full-chain cost incl. shrinkage/returns | P1 |

**The single highest-value investment, exactly as in WATER, is closing the measurement gap before committing capital.** You cannot size a buffer, find a bottleneck, or set an honest price on numbers you have not verified. This is why the metering layer comes first.

---

## 9. 0-1 Strategy Matrix: Vertical & Horizontal

Binary decision points (0 = not done, 1 = done) across technical depth and stakeholder breadth — the WATER framework applied to a supply chain.

```
┌──────────────────────────────────────────────────────────────────────────┐
│                     0-1 STRATEGY MATRIX — Supply Chain                     │
├──────────────────────────────────────────────────────────────────────────┤
│  VERTICAL (Technical Depth)                                                │
│  V1 FLOW MODEL    : [ ] map stages [ ] Little's Law [ ] find bottleneck    │
│                     [ ] demonstrated-rate tests [ ] yield per stage        │
│  V2 MEASUREMENT   : [ ] inventory truth [ ] transit truth [ ] defect truth │
│                     [ ] ledger closes [ ] truth-gap per stage              │
│  V3 IMPLEMENTATION: [ ] buffer sizing [ ] bottleneck protected            │
│                     [ ] reconcile loop [ ] alarms live [ ] full-cost price │
│                                                                            │
│  HORIZONTAL (Stakeholder Breadth)                                          │
│  H1 PARTNERS      : [ ] suppliers [ ] carriers [ ] 2nd source [ ] trust    │
│  H2 GOVERNANCE    : [ ] regulators [ ] standards [ ] audits [ ] floors     │
│  H3 PEOPLE        : [ ] incentives aligned [ ] messenger safe              │
│                     [ ] commons metered [ ] end-person represented         │
└──────────────────────────────────────────────────────────────────────────┘
```

```rust
/// Combined readiness across technical depth and human/stakeholder breadth.
#[derive(Debug, Clone, Default, Reflect, Serialize, Deserialize)]
pub struct SupplyChainStrategy {
    // Vertical — the math
    pub v1_flow_modeled: bool,
    pub v1_bottleneck_found: bool,
    pub v2_inventory_truth_known: bool,
    pub v2_ledger_closes: bool,
    pub v3_alarms_live: bool,
    pub v3_full_cost_priced: bool,
    // Horizontal — the people (the actual blocker)
    pub h1_second_source_qualified: bool,
    pub h2_floors_enforced: bool,
    pub h3_incentives_aligned: bool,
    pub h3_messenger_safe: bool,
    pub h3_end_person_represented: bool,
}

impl SupplyChainStrategy {
    pub fn next_action(&self) -> &'static str {
        // Measurement before everything — you cannot manage what you cannot see.
        if !self.v2_inventory_truth_known { return "Measure true inventory (TRACK_TRACE) before trusting any plan"; }
        if !self.v1_flow_modeled         { return "Map stages and apply Little's Law"; }
        if !self.v1_bottleneck_found     { return "Find the bottleneck via demonstrated-rate tests"; }
        if !self.v2_ledger_closes        { return "Reconcile inventory ledger; investigate the sink"; }
        if !self.h3_messenger_safe       { return "Make it safe to report bad news — or the data will lie"; }
        if !self.h3_incentives_aligned   { return "Align stage rewards with whole-chain delivery"; }
        if !self.v3_alarms_live          { return "Turn on real-time alarms (stockout, cold-chain, dwell)"; }
        if !self.h1_second_source_qualified { return "Qualify a second source before you need it"; }
        if !self.h2_floors_enforced      { return "Get quality/safety floors actually enforced"; }
        if !self.v3_full_cost_priced     { return "Price the full chain including the conservation-law sinks"; }
        if !self.h3_end_person_represented { return "Put the end person's need into the decision"; }
        "Chain is honest, instrumented, and aligned — maintain it."
    }
}
```

Note the ordering of `next_action`: **measurement and messenger-safety come before optimization.** You cannot optimize a chain whose numbers are lies, and the numbers stay lies until it is safe to tell the truth.

---

## 10. Pricing: Where the Whole Chain Settles Its Accounts

Price is the accounting identity that must, eventually, balance — money is conserved the way mass is. A price is honest when:

```
price  ≥  true_landed_cost  +  fair_return_on_risk_and_capital
```

…and `true_landed_cost` includes the sinks the chain would rather forget:

```rust
/// Full landed cost — including the conservation-law sinks most invoices hide.
pub fn true_landed_cost(c: &CostStack) -> f32 {
    c.input_cost
        + c.transformation_cost
        + c.holding_cost          // capital tied up in inventory (Little's Law: high L = real cost)
        + c.transport_cost
        + c.inspection_cost
        + c.compliance_cost
        + c.shrinkage_cost        // the unexplained sink (§2) — priced, not ignored
        + c.returns_and_defect_cost
        + c.externality_cost      // cost pushed onto others/the future is still a cost
}
```

The moral content of pricing, in the user's frame: a price that hides cost in shrinkage, in a weaker partner, in the environment, or in the future is not a lower price — it is a deferred bill, and someone pays it, usually the end person. **Honest pricing prices the whole chain.** Gouging in a shortage spends trust that took years to build for margin that lasts a quarter; the relationship is the asset, and reputation compounds like interest.

| Pricing approach | Goes right when… | Goes wrong when… |
|------------------|------------------|------------------|
| Cost-plus | Costs are honestly known and stable | Hidden sinks make "cost" fiction |
| Value-based | Value is real and shared with the buyer | It becomes extraction of desperation |
| Market/dynamic | Signals route goods to need | It becomes surge-gouging in a crisis |
| Long-term contract | Both sides want stability to invest | One side is locked in as conditions shift |

---

## 11. Project Phases

```
PHASE 0: MEASUREMENT FOUNDATION (the prerequisite — see §8, TRACK_TRACE)
  ├── Instrument inventory, transit, defects, conditions
  ├── Establish true on-hand, true bottleneck, true yield (± stated accuracy)
  ├── Get the reconciliation ledger to close
  └── Make it safe to report a discrepancy

PHASE 1: FIND THE CONSTRAINT
  ├── Demonstrated-rate test every stage under load
  ├── Identify THE bottleneck (one stage; conservation says only it matters)
  ├── Map the truth gap per stage
  └── Quantify the bullwhip (variance amplification up the tiers)

PHASE 2: PROTECT & WIDEN THE CONSTRAINT
  ├── Never starve the bottleneck; buffer immediately upstream of it
  ├── Qualify a second source for single points of failure
  ├── Turn on live alarms (stockout, cold-chain break, port dwell)
  └── Share real downstream demand upstream — kill the bullwhip

PHASE 3: ALIGN THE PEOPLE
  ├── Reward whole-chain delivery, not local KPIs
  ├── Separate who-judges from who-benefits (inspection independence)
  ├── Meter and price the commons
  └── Represent the end person in the decision

PHASE 4: SUSTAIN
  ├── Continuous reconciliation; every sink owned and investigated
  ├── Full-cost honest pricing
  ├── Public transparency dashboard (§12)
  └── The chain stays honest because honesty is now the cheapest policy
```

---

## 12. Public Transparency & Dissemination

The WATER project's transparency dashboard, adapted. A supply chain earns trust the way the water project does: by publishing its real state, including its failures.

```rust
/// Real-time public-facing supply-chain health dashboard.
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct ChainDashboard {
    // Flow
    pub bottleneck_stage: String,
    pub throughput_units_day: f32,
    pub avg_flow_time_days: f32,       // Little's Law W
    // Truth (published, not hidden)
    pub inventory_record_accuracy_pct: f32,  // phantom-inventory honesty
    pub unexplained_shrinkage_units: f32,    // the sink, in the open
    pub ledger_closes: bool,
    // Service to the end person
    pub on_time_in_full_pct: f32,
    pub stockouts_on_critical_skus: u32,
    // People / commons
    pub second_source_coverage_pct: f32,
    pub inspection_independence_verified: bool,
    pub last_update: String,
}
```

The discipline that makes this commendable rather than cynical: **publish the shrinkage, the stockouts, and the open bottleneck — not just the wins.** A dashboard that only shows green is the compliance-theater failure mode (§5) in a nicer font. The WATER dashboard publishes brine-to-ocean (target 0) precisely so it cannot be hidden; this one publishes the unexplained sink for the same reason.

---

## 13. References

- **Eustress Realism Crate**: `eustress/crates/common/src/realism/`
  - `laws/conservation.rs` — `mass_conservation_check`, `mass_flow_rate`, `volume_flow_rate`, `velocity_from_continuity`, `ConservationTracker` (the laws this document maps goods-flow onto)
  - `constants.rs` — `WATER_DENSITY`, `STANDARD_PRESSURE`, `materials::steel` (bill-of-materials physics)
  - `units.rs` — SI unit conversions
- **Companion documents in this folder**:
  - [`TRACK_TRACE.md`](TRACK_TRACE.md) — the measurement/inspection layer; "you cannot be honest about a chain you cannot see"
- **Sibling project (the model this parallels)**: `WATER/docs/README.md`, `WATER/docs/IGBWP.md`, `WATER/docs/WELL_METER.md`
- **Operations foundations**:
  - Little's Law (Little, 1961) — `L = λW`
  - Theory of Constraints (Goldratt, *The Goal*, 1984) — the bottleneck sets the chain
  - The Bullwhip Effect (Lee, Padmanabhan & Whang, 1997) — variance amplification as an information failure
  - The 1-10-100 rule of quality cost (defect cost by stage caught)
  - Tragedy of the Commons (Hardin, 1968) — the shared-resource failure named in §6

---

*Document created: May 29, 2026*
*Folder: PRODUCTION — supply chain flow analysis, anchored on warehousing*
*Modeled on the Eustress Engine WATER framework; physics grounded in the realism crate's conservation laws*
*Numbers in any real application are `Unverified` until measured from primary source (§8)*
