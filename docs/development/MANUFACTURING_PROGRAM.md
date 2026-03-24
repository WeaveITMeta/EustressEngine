# Eustress Manufacturing Program — Full Architecture

Complete specification covering money flow, investor and manufacturer database,
AI-driven allocation, and the single-source-per-product optimization principle.

---

## Table of Contents

1. [The Flywheel Model](#1-the-flywheel-model)
2. [V-Cell 4680 — Worked Example](#2-v-cell-4680--worked-example)
3. [Money Flow — Full Picture](#3-money-flow--full-picture)
4. [Investor Database](#4-investor-database)
5. [Manufacturer Database](#5-manufacturer-database)
6. [AI Allocation Engine](#6-ai-allocation-engine)
7. [Single-Source-Per-Product Principle](#7-single-source-per-product-principle)
8. [Rust Data Models](#8-rust-data-models)
9. [Implementation Plan](#9-implementation-plan)

---

## 1. The Flywheel Model

The Manufacturing Program is a **self-funding, self-improving manufacturing fund**.

```
Investors seed Round 0
        │
        ▼
 Product cohort pilots (batch 1,000 units each)
        │
        ▼
 Products ship → each unit pays 8% royalty to fund
        │
        ▼
 Fund grows → seeds next cohort of inventors
        │
        ▼
 More products → more royalties → larger fund
        │
        └──────────────────────────────► (repeat, fund never depletes)
```

After Round 0, **no external capital is required**. The fund is self-sustaining because
royalties from live products continuously pay for new pilots.

The AI allocator optimizes which investors fund which products and which manufacturers
build which parts — both decided at the moment a deal is approved, locked for the
pilot, then reassessed at Phase 2.

---

## 2. V-Cell 4680 — Worked Example

The V-Cell 4680 is a solid-state sodium-sulfur energy cell. Here is the complete
deal as the system would generate it.

### Bill of Materials

| Component | Material | Dimensions | Role | Est. Cost |
|---|---|---|---|---|
| Housing | Al 6061-T6 | 46×100×12 mm | Structural enclosure | $1.80 |
| Sc-NASICON solid electrolyte | Scandium-doped NASICON ceramic | 44×98×1.2 mm | Ion transport layer | $4.20 |
| Sodium anode | Metallic Na, stabilized | 44×98×0.8 mm | Anode active material | $0.90 |
| Sulfur-carbon cathode | S/C porous composite | 44×98×2.0 mm | Cathode active material | $1.40 |
| Hermetic seal + terminals | Kovar alloy | — | Electrical/sealing | $1.10 |
| **BOM Subtotal** | | | | **$9.40** |

### Unit Cost Build-Up

```
BOM materials                 $  9.40
+ 35% assembly labor          $  3.29
+ 15% inbound freight         $  1.41
+ 10% returns/warranty        $  0.94
──────────────────────────────────────
Total unit cost               $ 15.04

Suggested retail price        $ 79.00
Gross margin per unit         $ 63.96  (81.0%) This will be a variables locked between 40%-80%
```

### Per-Unit Money Flow

```
Customer pays:                $ 79.00
  − Sales tax / chargebacks   $  5.53  (7% estimate)
Net sales per unit:           $ 73.47

From net sales:
  → Mfg Program royalty  8%  $  5.88  → fund (seeds next inventor)
  → Inventor royalty     5%  $  3.67  → inventor recurring stream
  → Unit cost                $ 15.04  → manufacturer + 3PL
  ────────────────────────────────────
  Net to equity pool:         $ 48.88 per unit
```

### Equity Pool Split (per unit sold)

```
Inventor                60%  → $ 29.33
Eustress Mfg Program    25%  → $ 12.22
Logistics Partner       10%  → $  4.89  (vests month 13)
Reserve Pool             5%  → $  2.44
                             ─────────
                               $ 48.88  ✓
```

### Pilot Batch (1,000 units)

```
Gross revenue:              $ 79,000
Fund contribution:          $  5,880  (reinvested immediately)
Inventor total:             $ 33,000  (royalty $3,670 + equity $29,330)
Manufacturer paid:          $ 15,040
Logistics Partner (vesting) $  4,890  (held in escrow until month 13)
Eustress program share:     $ 12,220  (retained for infrastructure)
Reserve Pool:               $  2,440  (held for future co-investors)
```

### AI Allocation for V-Cell 4680

The AI allocator would assign:

- **Investor**: Energy sector fund (high-density storage vertical match) — funds pilot at
  $15,040 manufacturing cost + $8,000 warehousing = $23,040 total pilot capital
- **Manufacturer**: Precision ceramics + battery assembly shop (Sc-NASICON requires
  ceramic sintering capability — rare, so manufacturer score weights this heavily)
- **3PL**: Regional warehouse near pilot geography (US Pacific Northwest — proximity to
  energy storage early adopters)
- **Single source**: One manufacturer covers all five BOM components as an integrated
  battery assembly. No splitting parts across vendors.

---

## 3. Money Flow — Full Picture

### Sources

| Source | When | Amount | Purpose |
|---|---|---|---|
| **Investor capital** | Before pilot | Covers manufacturing cost of pilot batch | First-time product launch |
| **Customer purchases** | During/after pilot | $79/unit retail | Primary revenue stream |
| **Manufacturing Program fund** | Ongoing | Reinvested royalties | Self-sustaining pilot capital |

### Destinations

| Destination | Amount (per unit) | Timing |
|---|---|---|
| **Manufacturer** | $15.04 | Before delivery (investor-funded at pilot; revenue-funded at scale) |
| **3PL / Logistics Partner** | Included in unit cost ($1.41 freight, $2.50 pick+pack, $5–8 ship) | Per order |
| **Inventor royalty** | $3.67 (5% net) | Monthly, per unit sold |
| **Manufacturing Program fund** | $5.88 (8% net) | Monthly, per unit sold → immediate reinvestment |
| **Inventor equity share** | $29.33 (60% pool) | Quarterly distribution |
| **Eustress equity share** | $12.22 (25% pool) | Quarterly distribution |
| **Logistics Partner equity** | $4.89 (10% pool) | Quarterly, vests month 13 |
| **Reserve Pool** | $2.44 (5% pool) | Held in escrow, deployed by board |

### Fund Lifecycle

```
Round 0 investors inject $X into fund
       │
       ├── Product A pilot: $23,000 of capital deployed
       ├── Product B pilot: $18,000 of capital deployed
       └── Product C pilot: $31,000 of capital deployed

After pilot (all 3 products shipping):
  Product A royalties: $5,880/batch → $5,880 back to fund
  Product B royalties: $3,920/batch → $3,920 back to fund
  Product C royalties: $8,320/batch → $8,320 back to fund
  Monthly fund income: ~$18,120 across a 3-product cohort

Fund can now self-fund 1 new pilot per month without any new investor capital.
At 10 products: ~$60,000/month → 3 new pilots/month.
```

---

## 4. Investor Database (StartEngine)

### What the Database Stores

Each investor record defines their **investment thesis, capacity, and track record** so
the AI allocator can match them to the right product.

### Schema: `Investor`

```toml
[investor]
id = "inv_001"
name = "Pacific Energy Ventures"
type = "venture_fund"                    # "individual" | "venture_fund" | "family_office" | "strategic_corporate"
status = "active"                        # "active" | "inactive" | "blacklisted"

[investor.focus]
verticals = ["energy_storage", "clean_tech", "hardware"]
excluded_verticals = ["weapons", "tobacco"]
stage_preference = "pilot"               # "seed" | "pilot" | "series_a" | "any"
geography_preference = ["US", "Canada"]  # ISO country codes

[investor.capacity]
min_check_usd = 10_000
max_check_usd = 250_000
available_capital_usd = 500_000         # updated when deals close
currency = "USD"

[investor.terms]
target_irr_pct = 22.0                   # minimum acceptable internal rate of return
preferred_equity_pct_min = 5.0          # minimum equity stake they'll accept
preferred_equity_pct_max = 30.0
requires_board_seat = false
requires_pro_rata_rights = true

[investor.track_record]
deals_funded = 14
deals_returned = 11
avg_return_multiple = 2.4               # average 2.4x return on exited deals
current_portfolio_count = 6
days_to_close_avg = 18                  # average days from intro to wire

[investor.contact]
email = "deals@pacificenergy.vc"
preferred_contact = "email"
timezone = "America/Los_Angeles"
```

### Investor Types

| Type | Check Size | Decision Speed | Best For |
|---|---|---|---|
| **Individual angel** | $5k–$50k | Fast (days) | Early pilots, exotic products |
| **Venture fund** | $25k–$500k | Medium (weeks) | Proven category, strong IP |
| **Family office** | $50k–$2M | Medium | Platform-tier products |
| **Strategic corporate** | $100k–$5M | Slow (months) | Products in their supply chain |

---

## 5. Manufacturer Database - This is requirement of MVP

### What the Database Stores

Each manufacturer record defines their **capabilities, certifications, capacity, and pricing**
so the AI allocator can find the single best source for a given product's BOM.

### Schema: `Manufacturer`

```toml
[manufacturer]
id = "mfr_042"
name = "Kyoto Advanced Ceramics Ltd."
status = "approved"                      # "pending_audit" | "approved" | "suspended" | "blacklisted"
country = "JP"
region = "Kinki"
lead_time_days = 35                      # days from PO to delivered to warehouse

[manufacturer.capabilities]
# Process capabilities — AI matches these against BOM material requirements
processes = [
    "ceramic_sintering",
    "thin_film_deposition",
    "hermetic_sealing",
    "precision_machining",
    "battery_cell_assembly",
]
materials = [
    "NASICON_ceramics",
    "aluminum_alloys",
    "kovar",
    "sodium_metal",
    "sulfur_carbon_composites",
]
max_part_dimensions_mm = [500, 500, 200]
min_feature_size_mm = 0.1
surface_finish_ra_max = 0.8              # Ra micrometers

[manufacturer.certifications]
iso_9001 = true
iso_14001 = true
iatf_16949 = false                       # Automotive — not required for V-Cell
ul_certified = true
reach_compliant = true
rohs_compliant = true
ul_battery_certified = true             # Critical for energy storage products

[manufacturer.capacity]
monthly_units_available = 50_000
min_order_quantity = 500
dedicated_line_available = true         # Can reserve a dedicated production line
dedicated_line_setup_weeks = 4

[manufacturer.pricing]
# Pricing tiers by volume — used by AI to compute best source cost
[[manufacturer.pricing.tiers]]
min_qty = 500
max_qty = 4_999
price_per_unit_usd = 9.80               # BOM + assembly only, no logistics

[[manufacturer.pricing.tiers]]
min_qty = 5_000
max_qty = 49_999
price_per_unit_usd = 7.20

[[manufacturer.pricing.tiers]]
min_qty = 50_000
max_qty = 999_999_999
price_per_unit_usd = 5.50

[manufacturer.quality]
historical_defect_rate_pct = 0.8        # updated from past orders
on_time_delivery_rate_pct = 94.2
ncr_count_12mo = 2                      # non-conformance reports in last 12 months
last_audit_date = "2025-11-14"
audit_score = 91                        # out of 100

[manufacturer.logistics]
incoterms_offered = ["EXW", "FOB", "DDP"]
freight_partners = ["DHL", "FedEx", "Kintetsu"]
export_license_held = true
hazmat_certified = true                 # Can ship sodium metal (UN3292)
```

### Manufacturer Scoring Dimensions

The AI allocator scores each manufacturer across five dimensions:

| Dimension | Weight | Signals |
|---|---|---|
| **Capability match** | 40% | Does manufacturer have all required processes and materials? |
| **Quality** | 25% | Defect rate, on-time delivery, audit score, certifications |
| **Cost** | 20% | Price per unit at pilot quantity vs. unit_cost_usd in brief |
| **Speed** | 10% | Lead time vs. pilot launch date |
| **Risk** | 5% | Single country dependency, capacity headroom, NCR count |

We will need really strong benchmarks and tests for products to pass based on categorical assignment per part. This leads to a system where the feasibility of each product is judged by AI after the simulation runs and data is iterated upon until the theoretical maximum for first prototype.

---

## 6. AI Allocation Engine

### What It Does

Given a product's `ideation_brief.toml` and `deal_structure`, the AI allocator:

1. **Matches manufacturers** to the BOM — finds every manufacturer with ALL required
   processes and materials, ranks by composite score
2. **Selects the single best manufacturer** — one source, locked for the pilot
3. **Matches investors** to the deal — finds investors whose thesis, check size, and
   geography match this product, ranks by days-to-close (fastest first)
4. **Selects the minimum number of investors** to cover pilot capital — typically 1–3
5. **Outputs an `AllocationDecision`** as TOML, which goes into the `ideation_brief.toml`
   and becomes part of the `DEAL_STRUCTURE.md`

### Allocation Algorithm

We can automate emails to manufacturers with CRON jobs, repeating jobs to inquire updates and reply to updates and use Amail to loop in the inventor.

```
Input:
  brief: IdeationBrief (product, BOM, deal_structure)

Step 1: Manufacturer Selection
  required_processes = extract_processes(brief.bill_of_materials)
  required_materials = extract_materials(brief.bill_of_materials)
  pilot_qty = brief.deal_structure.pilot_minimum_units

  candidates = DB.manufacturers
    .filter(status == "approved")
    .filter(has_all(capabilities.processes, required_processes))
    .filter(has_all(capabilities.materials, required_materials))
    .filter(min_order_quantity <= pilot_qty)
    .filter(monthly_units_available >= pilot_qty)

  scored = candidates.map(|m| {
    capability_score = jaccard_similarity(m.capabilities, required) × 40
    quality_score    = weighted(defect_rate, on_time, audit_score) × 25
    cost_score       = (1 - m.price(pilot_qty) / deal.unit_cost_usd) × 20
    speed_score      = (1 - m.lead_time / deadline_days) × 10
    risk_score       = risk_assess(m) × 5
    total = sum(scores)
  })

  selected_manufacturer = scored.max_by(total)

Step 2: Investor Selection
  pilot_capital_needed = brief.deal_structure.unit_cost_usd
                         × brief.deal_structure.pilot_minimum_units
                         + warehousing_estimate

  vertical = brief.product.category  // maps to investor vertical tags
  geography = brief.deal_structure.pilot_geography

  candidates = DB.investors
    .filter(status == "active")
    .filter(verticals.contains(vertical))
    .filter(geography_preference.contains(geography) OR geography_preference.contains("any"))
    .filter(available_capital_usd >= min_check_usd)
    .filter(min_check_usd <= pilot_capital_needed)
    .sort_by(days_to_close_avg ASC)   // fastest to close first

  // Select minimum investors to cover capital — prefer single investor
  selected_investors = greedy_cover(candidates, pilot_capital_needed)
    // Take top investor. If check_max < needed, add next, repeat.

Step 3: Output AllocationDecision
  {
    manufacturer: selected_manufacturer.id,
    manufacturer_score: ...,
    investors: [selected_investors],
    total_pilot_capital: ...,
    allocation_confidence: ...,   // 0.0–1.0
    alternatives: top_3_runners_up,
    generated_at: timestamp,
  }
```

### AI Prompting Layer

The allocator uses Claude with a structured prompt:

```
System: You are the Eustress Manufacturing Program AI Allocator.
  Given a product brief and databases of manufacturers and investors,
  select the optimal single manufacturer and minimum investors.

  Allocation rules:
  1. Manufacturer MUST have ALL required processes and materials — no exceptions
  2. Prefer ONE manufacturer (single source) — split only if single source impossible
  3. Prefer ONE investor — add more only if check size insufficient
  4. Fastest-to-close investor wins ties
  5. Never select a suspended, blacklisted, or pending_audit manufacturer
  6. Never select an investor in a blacklisted vertical for this product category

  Output as TOML. Explain your reasoning for each selection in one sentence.
```

### Confidence Scoring

The allocator outputs an `allocation_confidence` score (0.0–1.0):

| Score | Meaning | Action |
|---|---|---|
| ≥ 0.85 | Strong match — auto-approve available | Human review optional |
| 0.65–0.84 | Good match — present to founder for approval | One-click approve |
| 0.40–0.64 | Marginal — flag concerns, show alternatives | Requires human decision |
| < 0.40 | No good match — escalate | Manual sourcing required |

Low confidence triggers an automatic "needs more manufacturers in database" alert.

---

## 7. Single-Source-Per-Product Principle

### The Rule

> Each product has **one manufacturer** for its entire BOM assembly.
> That manufacturer is responsible for all components, sub-assembly, and QC.
> The 3PL is responsible for warehousing and fulfillment.
> No part is split across multiple suppliers at the pilot stage.

### Why This Works

| Concern | How Single-Source Handles It |
|---|---|
| **Quality control** | One QC interface. One defect rate. One audit. |
| **Lead time** | No multi-vendor coordination. One PO, one delivery. |
| **Cost** | Manufacturer bundles components — negotiates own sub-suppliers internally |
| **Accountability** | Any defect is unambiguously that manufacturer's responsibility |
| **Inventory** | One inbound shipment to warehouse per production run |
| **Cognitive load** | Founder manages one relationship, not five |

### When Single-Source Fails

The AI flags a product as "multi-source required" only if:

1. No single manufacturer has all required processes (rare — database grows over time)
2. The product requires a regulated component only one certified supplier globally makes
   (e.g., medical isotopes, specific aerospace alloys)
3. Pilot volume exceeds any single manufacturer's monthly capacity

In these cases the AI splits at the **sub-assembly level**, not the component level:
- Manufacturer A: electronic sub-assembly
- Manufacturer B: mechanical housing + final integration

Still two sources maximum. Never more.

### Manufacturer Database Growth Strategy

The bottleneck is database coverage, not the algorithm. To stay single-source:

1. **Onboard manufacturers before products need them** — the AI flags gaps ("no ceramic
   sintering capability in North America") and the program recruits to fill them
2. **Manufacturers self-register** via a web portal — their capabilities are AI-extracted
   from their spec sheets and verified at first audit
3. **Audit cadence**: annual re-audit + automatic suspension if defect rate > 3% or
   on-time delivery < 85% over 90 days

---

## 8. Rust Data Models

New crate: `eustress/crates/engine/src/manufacturing/`

```rust
// ============================================================================
// manufacturing/mod.rs — Investor + Manufacturer DB + Allocation Decision
// ============================================================================

/// An investor in the Manufacturing Program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Investor {
    pub id: String,
    pub name: String,
    pub investor_type: InvestorType,
    pub status: InvestorStatus,
    pub focus: InvestorFocus,
    pub capacity: InvestorCapacity,
    pub terms: InvestorTerms,
    pub track_record: InvestorTrackRecord,
    pub contact: InvestorContact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InvestorType {
    Individual,
    VentureFund,
    FamilyOffice,
    StrategicCorporate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InvestorStatus {
    Active,
    Inactive,
    Blacklisted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorFocus {
    pub verticals: Vec<String>,
    pub excluded_verticals: Vec<String>,
    pub stage_preference: String,
    pub geography_preference: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorCapacity {
    pub min_check_usd: f64,
    pub max_check_usd: f64,
    pub available_capital_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorTerms {
    pub target_irr_pct: f64,
    pub preferred_equity_pct_min: f64,
    pub preferred_equity_pct_max: f64,
    pub requires_board_seat: bool,
    pub requires_pro_rata_rights: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorTrackRecord {
    pub deals_funded: u32,
    pub deals_returned: u32,
    pub avg_return_multiple: f64,
    pub current_portfolio_count: u32,
    pub days_to_close_avg: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorContact {
    pub email: String,
    pub preferred_contact: String,
    pub timezone: String,
}

// ============================================================================

/// A manufacturer in the Manufacturing Program network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manufacturer {
    pub id: String,
    pub name: String,
    pub status: ManufacturerStatus,
    pub country: String,
    pub region: String,
    pub lead_time_days: u32,
    pub capabilities: ManufacturerCapabilities,
    pub certifications: ManufacturerCertifications,
    pub capacity: ManufacturerCapacity,
    pub pricing: Vec<PricingTier>,
    pub quality: ManufacturerQuality,
    pub logistics: ManufacturerLogistics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ManufacturerStatus {
    PendingAudit,
    Approved,
    Suspended,
    Blacklisted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerCapabilities {
    pub processes: Vec<String>,
    pub materials: Vec<String>,
    pub max_part_dimensions_mm: [f64; 3],
    pub min_feature_size_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerCertifications {
    pub iso_9001: bool,
    pub iso_14001: bool,
    pub ul_certified: bool,
    pub reach_compliant: bool,
    pub rohs_compliant: bool,
    pub ul_battery_certified: bool,
    pub additional: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerCapacity {
    pub monthly_units_available: u32,
    pub min_order_quantity: u32,
    pub dedicated_line_available: bool,
    pub dedicated_line_setup_weeks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingTier {
    pub min_qty: u32,
    pub max_qty: u32,
    pub price_per_unit_usd: f64,
}

impl PricingTier {
    /// Price per unit for a given quantity, using the matching tier
    pub fn price_for_qty(tiers: &[PricingTier], qty: u32) -> Option<f64> {
        tiers.iter()
            .find(|t| qty >= t.min_qty && qty <= t.max_qty)
            .map(|t| t.price_per_unit_usd)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerQuality {
    pub historical_defect_rate_pct: f64,
    pub on_time_delivery_rate_pct: f64,
    pub ncr_count_12mo: u32,
    pub last_audit_date: String,
    pub audit_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerLogistics {
    pub incoterms_offered: Vec<String>,
    pub freight_partners: Vec<String>,
    pub export_license_held: bool,
    pub hazmat_certified: bool,
}

// ============================================================================

/// Output of the AI Allocation Engine for one product
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationDecision {
    pub product_id: String,
    pub manufacturer_id: String,
    pub manufacturer_score: f64,
    pub manufacturer_rationale: String,
    pub investors: Vec<InvestorAllocation>,
    pub total_pilot_capital_usd: f64,
    pub allocation_confidence: f64,
    pub alternatives: Vec<AlternativeManufacturer>,
    pub generated_at: String,
    pub status: AllocationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorAllocation {
    pub investor_id: String,
    pub check_amount_usd: f64,
    pub equity_pct: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeManufacturer {
    pub manufacturer_id: String,
    pub score: f64,
    pub gap: String,    // why this was NOT chosen (e.g. "15% higher cost per unit")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AllocationStatus {
    Proposed,       // AI generated, awaiting human review
    Approved,       // Founder approved
    Rejected,       // Founder rejected — manual override required
    Locked,         // Manufacturer PO issued, investors wired — immutable
    Cancelled,      // Pilot cancelled
}

// ============================================================================

/// The database of investors and manufacturers — persisted as TOML files
/// in docs/Products/{product}/allocation/ and the program-wide registry
#[derive(Debug, Default)]
pub struct ManufacturingProgramRegistry {
    pub investors: Vec<Investor>,
    pub manufacturers: Vec<Manufacturer>,
}

impl ManufacturingProgramRegistry {
    /// Find all manufacturers that have ALL required processes and materials
    pub fn capable_manufacturers(
        &self,
        required_processes: &[String],
        required_materials: &[String],
        pilot_qty: u32,
    ) -> Vec<&Manufacturer> {
        self.manufacturers.iter()
            .filter(|m| m.status == ManufacturerStatus::Approved)
            .filter(|m| {
                required_processes.iter().all(|p| m.capabilities.processes.contains(p))
                && required_materials.iter().all(|mat| m.capabilities.materials.contains(mat))
            })
            .filter(|m| m.capacity.min_order_quantity <= pilot_qty)
            .filter(|m| m.capacity.monthly_units_available >= pilot_qty)
            .collect()
    }

    /// Find all investors that match a product's vertical, geography, and capital needs
    pub fn matching_investors(
        &self,
        vertical: &str,
        geography: &str,
        capital_needed_usd: f64,
    ) -> Vec<&Investor> {
        self.investors.iter()
            .filter(|i| i.status == InvestorStatus::Active)
            .filter(|i| i.focus.verticals.iter().any(|v| v == vertical || v == "any"))
            .filter(|i| {
                i.focus.geography_preference.iter()
                    .any(|g| g == geography || g == "any")
            })
            .filter(|i| i.capacity.available_capital_usd >= i.capacity.min_check_usd)
            .filter(|i| i.capacity.min_check_usd <= capital_needed_usd)
            .collect()
    }
}
```

---

## 9. Implementation Plan

### Phase 1 — Data Models (1 week)

- [ ] Create `eustress/crates/engine/src/manufacturing/mod.rs` with `Investor`,
      `Manufacturer`, `AllocationDecision`, `ManufacturingProgramRegistry` structs
- [ ] Add `allocation: Option<AllocationDecision>` to `IdeationBrief`
- [ ] Add `ArtifactStep::AllocationDecision` (Step 11) to pipeline
- [ ] TOML serialization tests for both database schemas

### Phase 2 — File-System Registry (1 week)

- [ ] Registry stored as TOML files in `docs/manufacturing/investors/` and
      `docs/manufacturing/manufacturers/`
- [ ] `load_registry()` — scans directory, deserializes all TOML files
- [ ] `save_investor()` / `save_manufacturer()` — atomic write (write temp, rename)
- [ ] Studio UI panel: simple list view of investors and manufacturers
- [ ] Seed the registry with 3 sample investors and 5 sample manufacturers as TOML files

### Phase 3 — AI Allocator (2 weeks)

- [ ] `ArtifactStep::AllocationDecision` — generates `ALLOCATION.md` via Claude
- [ ] `ALLOCATION_SYSTEM_PROMPT` — instructs Claude to score, select, and explain
- [ ] Score computation functions: `score_manufacturer()`, `score_investors()`
- [ ] `AllocationDecision` written to `docs/Products/{name}/ALLOCATION.md` and
      embedded in `ideation_brief.toml`
- [ ] Workshop Panel card: shows selected manufacturer + investors with scores,
      one-click approve or request alternatives

### Phase 4 — Studio Management UI (2 weeks)

- [ ] Investor registry tab: add, edit, view, activate/deactivate investors
- [ ] Manufacturer registry tab: add, edit, audit score, suspend/approve
- [ ] Allocation review panel: show `AllocationDecision` per product,
      alternative manufacturers, confidence score
- [ ] Manual override: founder can swap manufacturer or investor, override is logged

### Phase 5 — Automated Monitoring (future)

- [ ] Manufacturer quality tracker: import order fulfillment data, update
      `historical_defect_rate_pct` and `on_time_delivery_rate_pct`
- [ ] Auto-suspend: if defect rate > 3% over 90 days, set status = `Suspended`,
      trigger re-allocation for active products
- [ ] Fund balance tracker: sum all live product royalty streams, project runway
- [ ] Investor pipeline: notify matching investors via email when new product enters
      the allocation queue

---

## Summary

**Yes, it can be done** — and here is the exact design:

1. **Investor database** — TOML files describing thesis, check size, geography, track record
2. **Manufacturer database** — TOML files describing processes, materials, certifications, pricing tiers, quality history
3. **AI Allocation Engine** — scores all candidates, selects one manufacturer + minimum investors, outputs `AllocationDecision` with confidence score
4. **Single-source rule** — one manufacturer per product for the entire BOM assembly; split only if impossible, maximum two sources ever
5. **Fund flywheel** — royalties from live products fund new pilots; investors only needed for Round 0 seeding

The whole system is file-system-first (TOML, git-diffable), no database server required.
The AI allocator runs on Claude (BYOK) at deal approval time — ~$0.04 per allocation.
