//! # Manufacturing Module — Investor + Manufacturer Registry + AI Allocation Engine
//!
//! Manages the Eustress Manufacturing Program's two registries (investors and
//! manufacturers) and the AI-driven allocation engine that assigns the optimal
//! single manufacturer and minimum investor set to each product.
//!
//! ## Table of Contents
//!
//! 1. Investor — profile, focus, capacity, terms, track record
//! 2. Manufacturer — capabilities, certifications, capacity, pricing tiers, quality
//! 3. AllocationDecision — output of the AI allocator per product
//! 4. ManufacturingProgramRegistry — in-memory registry with query helpers
//! 5. Scoring — capability matching and composite score computation
//! 6. ManufacturingPlugin — Bevy plugin that loads registry from TOML files
//!
//! ## Key Design Principles
//!
//! - **File-system-first**: All registry data lives in TOML files under
//!   `docs/manufacturing/investors/` and `docs/manufacturing/manufacturers/`
//! - **Single-source per product**: One manufacturer covers the full BOM assembly.
//!   Split at sub-assembly level only when truly impossible.
//! - **Minimum investors**: Select the fewest investors whose combined check size
//!   covers pilot capital. Prefer one investor.
//! - **AI allocation via Claude (BYOK)**: Scoring is deterministic Rust; the final
//!   narrative rationale and confidence explanation come from Claude at ~$0.04/call.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ============================================================================
// 1. Investor
// ============================================================================

/// An investor registered in the Manufacturing Program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Investor {
    /// Unique identifier (e.g. "inv_001")
    pub id: String,
    /// Display name
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
#[serde(rename_all = "snake_case")]
pub enum InvestorType {
    Individual,
    VentureFund,
    FamilyOffice,
    StrategicCorporate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InvestorStatus {
    Active,
    Inactive,
    Blacklisted,
}

/// What kinds of products and geographies an investor will fund
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorFocus {
    /// Industry verticals (e.g. ["energy_storage", "clean_tech", "hardware"])
    pub verticals: Vec<String>,
    /// Verticals this investor explicitly refuses (compliance filter)
    pub excluded_verticals: Vec<String>,
    /// "seed" | "pilot" | "series_a" | "any"
    pub stage_preference: String,
    /// ISO country codes, or ["any"]
    pub geography_preference: Vec<String>,
}

/// How much capital this investor can deploy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorCapacity {
    /// Smallest check they write (USD)
    pub min_check_usd: f64,
    /// Largest check they write (USD)
    pub max_check_usd: f64,
    /// Currently available and undeployed capital (USD) — updated when deals close
    pub available_capital_usd: f64,
}

/// Legal and governance requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorTerms {
    /// Minimum acceptable internal rate of return
    pub target_irr_pct: f64,
    /// Minimum equity stake they will accept in a deal
    pub preferred_equity_pct_min: f64,
    /// Maximum equity stake they want (avoid over-concentration)
    pub preferred_equity_pct_max: f64,
    /// Whether they require a board seat
    pub requires_board_seat: bool,
    /// Whether they require pro-rata rights in follow-on rounds
    pub requires_pro_rata_rights: bool,
}

/// Historical performance data — used by AI to rank by reliability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorTrackRecord {
    pub deals_funded: u32,
    pub deals_returned: u32,
    /// Average return multiple on exited deals (e.g. 2.4 = 2.4x)
    pub avg_return_multiple: f64,
    pub current_portfolio_count: u32,
    /// Average calendar days from first contact to wire — lower is better
    pub days_to_close_avg: u32,
}

/// Contact preferences for outreach
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorContact {
    pub email: String,
    /// "email" | "phone" | "linkedin"
    pub preferred_contact: String,
    /// IANA timezone string (e.g. "America/Los_Angeles")
    pub timezone: String,
}

// ============================================================================
// 2. Manufacturer
// ============================================================================

/// A manufacturer in the Manufacturing Program network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manufacturer {
    /// Unique identifier (e.g. "mfr_042")
    pub id: String,
    /// Display name
    pub name: String,
    pub status: ManufacturerStatus,
    /// ISO 3166-1 alpha-2 country code
    pub country: String,
    pub region: String,
    /// Calendar days from purchase order to delivered to first warehouse node
    pub lead_time_days: u32,
    pub capabilities: ManufacturerCapabilities,
    pub certifications: ManufacturerCertifications,
    pub capacity: ManufacturerCapacity,
    /// Volume-tiered pricing — must cover the pilot quantity range
    pub pricing: Vec<PricingTier>,
    pub quality: ManufacturerQuality,
    pub logistics: ManufacturerLogistics,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ManufacturerStatus {
    /// Awaiting first audit — cannot be allocated
    PendingAudit,
    /// Audited and cleared — eligible for allocation
    Approved,
    /// Temporarily suspended (quality issue) — cannot be allocated
    Suspended,
    /// Permanently banned — cannot be allocated
    Blacklisted,
}

/// Manufacturing process and material capabilities — core allocation filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerCapabilities {
    /// Process names matched against BOM requirements
    /// e.g. ["ceramic_sintering", "thin_film_deposition", "battery_cell_assembly"]
    pub processes: Vec<String>,
    /// Material names matched against BOM requirements
    /// e.g. ["NASICON_ceramics", "aluminum_alloys", "sodium_metal"]
    pub materials: Vec<String>,
    /// Maximum part dimensions [L, W, H] in millimeters
    pub max_part_dimensions_mm: [f64; 3],
    /// Minimum achievable feature size in millimeters
    pub min_feature_size_mm: f64,
}

/// Compliance certifications — some required for specific product types
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManufacturerCertifications {
    pub iso_9001: bool,
    pub iso_14001: bool,
    pub iatf_16949: bool,
    pub ul_certified: bool,
    pub reach_compliant: bool,
    pub rohs_compliant: bool,
    /// Required for energy storage products (batteries, supercaps)
    pub ul_battery_certified: bool,
    /// Additional certifications not covered above (e.g. "AS9100D", "FDA_CFR_21")
    #[serde(default)]
    pub additional: Vec<String>,
}

/// Production capacity constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerCapacity {
    /// Maximum units producible per calendar month across all products
    pub monthly_units_available: u32,
    /// Smallest order they will accept
    pub min_order_quantity: u32,
    /// Whether they can reserve a dedicated production line for a single product
    pub dedicated_line_available: bool,
    /// Weeks to set up and qualify a dedicated line
    pub dedicated_line_setup_weeks: u32,
}

/// One row in the volume-tiered pricing table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingTier {
    pub min_qty: u32,
    pub max_qty: u32,
    /// BOM + assembly cost per unit at this volume (USD, excludes inbound freight)
    pub price_per_unit_usd: f64,
}

impl PricingTier {
    /// Return the price per unit for a given order quantity, searching provided tiers
    pub fn price_for_qty(tiers: &[PricingTier], qty: u32) -> Option<f64> {
        tiers.iter()
            .find(|t| qty >= t.min_qty && qty <= t.max_qty)
            .map(|t| t.price_per_unit_usd)
    }
}

/// Historical quality metrics — updated from order fulfilment data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerQuality {
    /// Percentage of units with defects (lower is better, >3% triggers auto-review)
    pub historical_defect_rate_pct: f64,
    /// Percentage of deliveries arriving on or before committed date
    pub on_time_delivery_rate_pct: f64,
    /// Non-conformance reports raised in the last 12 months
    pub ncr_count_12mo: u32,
    /// ISO date of most recent facility audit
    pub last_audit_date: String,
    /// Audit score out of 100 (>= 80 required for Approved status)
    pub audit_score: u32,
}

/// Outbound logistics capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManufacturerLogistics {
    /// Incoterms this manufacturer can ship under (e.g. ["EXW", "FOB", "DDP"])
    pub incoterms_offered: Vec<String>,
    /// Freight forwarders or carriers they work with
    pub freight_partners: Vec<String>,
    /// Whether they hold a valid export license for controlled goods
    pub export_license_held: bool,
    /// Whether they are certified to ship hazardous materials (e.g. sodium metal, Li batteries)
    pub hazmat_certified: bool,
}

// ============================================================================
// 3. AllocationDecision
// ============================================================================

/// The AI allocator's output for one product — written to ALLOCATION.md and
/// embedded in ideation_brief.toml after founder approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationDecision {
    /// Product name this decision is for
    pub product_id: String,
    /// ID of the selected manufacturer
    pub manufacturer_id: String,
    /// Composite score 0.0–100.0 (weighted across 5 dimensions)
    pub manufacturer_score: f64,
    /// One-sentence AI rationale for the manufacturer selection
    pub manufacturer_rationale: String,
    /// Price per unit at pilot quantity from this manufacturer (USD)
    pub manufacturer_price_per_unit_usd: f64,
    /// Investors selected to fund the pilot capital
    pub investors: Vec<InvestorAllocation>,
    /// Total capital the selected investors are providing (USD)
    pub total_pilot_capital_usd: f64,
    /// 0.0–1.0 — confidence in this allocation
    /// >= 0.85 = auto-approve eligible; < 0.40 = escalate for manual sourcing
    pub allocation_confidence: f64,
    /// Top 3 runner-up manufacturers not selected, with gap explanation
    pub alternatives: Vec<AlternativeManufacturer>,
    /// ISO 8601 timestamp of when this decision was generated
    pub generated_at: String,
    pub status: AllocationStatus,
}

/// One investor's contribution in the allocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvestorAllocation {
    pub investor_id: String,
    /// Amount this investor is providing for the pilot (USD)
    pub check_amount_usd: f64,
    /// Equity stake they receive for this check (%)
    pub equity_pct: f64,
    /// One-sentence AI rationale for selecting this investor
    pub rationale: String,
}

/// A runner-up manufacturer not selected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlternativeManufacturer {
    pub manufacturer_id: String,
    pub score: f64,
    /// Short description of why this was not chosen (e.g. "15% higher cost per unit")
    pub gap: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AllocationStatus {
    /// AI generated — awaiting founder review
    #[default]
    Proposed,
    /// Founder approved — manufacturer and investors notified
    Approved,
    /// Founder rejected — manual override required
    Rejected,
    /// Manufacturer purchase order issued, investors wired — immutable
    Locked,
    /// Pilot cancelled before lock
    Cancelled,
}

// ============================================================================
// 4. ManufacturingProgramRegistry
// ============================================================================

/// In-memory registry of all investors and manufacturers.
/// Loaded from TOML files at startup; re-loaded on Studio registry panel save.
#[derive(Debug, Default, Resource)]
pub struct ManufacturingProgramRegistry {
    pub investors: Vec<Investor>,
    pub manufacturers: Vec<Manufacturer>,
}

impl ManufacturingProgramRegistry {
    /// Load all `.toml` files from a directory into a Vec<T>
    pub fn load_from_dir<T: serde::de::DeserializeOwned>(dir: &std::path::Path) -> Vec<T> {
        let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
        entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "toml").unwrap_or(false))
            .filter_map(|e| {
                let content = std::fs::read_to_string(e.path()).ok()?;
                toml::from_str::<T>(&content).ok()
            })
            .collect()
    }

    /// Find all Approved manufacturers that have ALL required processes and materials
    /// and can handle the pilot quantity
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

    /// Find all Active investors that match a product's vertical, geography, and can write a meaningful check
    pub fn matching_investors(
        &self,
        vertical: &str,
        geography: &str,
        capital_needed_usd: f64,
    ) -> Vec<&Investor> {
        self.investors.iter()
            .filter(|i| i.status == InvestorStatus::Active)
            .filter(|i| {
                i.focus.verticals.iter().any(|v| v == vertical || v == "any")
                    && !i.focus.excluded_verticals.iter().any(|v| v == vertical)
            })
            .filter(|i| {
                i.focus.geography_preference.iter()
                    .any(|g| g == geography || g == "any")
            })
            .filter(|i| i.capacity.available_capital_usd >= i.capacity.min_check_usd)
            .filter(|i| i.capacity.min_check_usd <= capital_needed_usd)
            .collect()
    }

    /// Select the minimum set of investors (fewest count) whose combined max check
    /// covers `capital_needed`. Returns in order: fastest-to-close first.
    pub fn greedy_investor_cover<'a>(
        investors: &[&'a Investor],
        capital_needed_usd: f64,
    ) -> Vec<&'a Investor> {
        // Sort by days_to_close ascending (fastest first) then max_check descending
        let mut sorted: Vec<&&Investor> = investors.iter()
            .collect();
        sorted.sort_by(|a, b| {
            a.track_record.days_to_close_avg
                .cmp(&b.track_record.days_to_close_avg)
                .then(b.capacity.max_check_usd
                    .partial_cmp(&a.capacity.max_check_usd)
                    .unwrap_or(std::cmp::Ordering::Equal))
        });

        let mut covered = 0.0_f64;
        let mut selected = Vec::new();
        for investor in sorted {
            if covered >= capital_needed_usd {
                break;
            }
            let check = investor.capacity.max_check_usd.min(capital_needed_usd - covered);
            covered += check;
            selected.push(*investor);
        }
        selected
    }
}

// ============================================================================
// 5. Scoring
// ============================================================================

/// Composite manufacturer score — 0.0 to 100.0
///
/// Weights:
///   40% capability match (Jaccard similarity of required vs. available)
///   25% quality (defect rate, on-time delivery, audit score)
///   20% cost (how close manufacturer price is to target unit cost)
///   10% speed (lead time vs. pilot timeline)
///    5% risk (single-country, NCR count, capacity headroom)
pub fn score_manufacturer(
    manufacturer: &Manufacturer,
    required_processes: &[String],
    required_materials: &[String],
    target_unit_cost_usd: f64,
    pilot_qty: u32,
    pilot_deadline_days: u32,
) -> f64 {
    // --- Capability (40%) ---
    let req_count = (required_processes.len() + required_materials.len()) as f64;
    let matched_processes = required_processes.iter()
        .filter(|p| manufacturer.capabilities.processes.contains(p))
        .count() as f64;
    let matched_materials = required_materials.iter()
        .filter(|m| manufacturer.capabilities.materials.contains(m))
        .count() as f64;
    let capability_ratio = if req_count > 0.0 {
        (matched_processes + matched_materials) / req_count
    } else {
        1.0
    };
    let capability_score = capability_ratio * 40.0;

    // --- Quality (25%) ---
    let defect_score = (1.0 - (manufacturer.quality.historical_defect_rate_pct / 10.0).min(1.0)) * 40.0;
    let otd_score = (manufacturer.quality.on_time_delivery_rate_pct / 100.0) * 40.0;
    let audit_score = (manufacturer.quality.audit_score as f64 / 100.0) * 20.0;
    let quality_score = (defect_score + otd_score + audit_score) / 100.0 * 25.0;

    // --- Cost (20%) ---
    let mfr_price = PricingTier::price_for_qty(&manufacturer.pricing, pilot_qty)
        .unwrap_or(target_unit_cost_usd * 2.0);
    let cost_ratio = if target_unit_cost_usd > 0.0 {
        (1.0 - ((mfr_price - target_unit_cost_usd) / target_unit_cost_usd).abs().min(1.0))
    } else {
        0.5
    };
    let cost_score = cost_ratio * 20.0;

    // --- Speed (10%) ---
    let speed_ratio = if pilot_deadline_days > 0 {
        (1.0 - (manufacturer.lead_time_days as f64 / pilot_deadline_days as f64).min(1.0))
    } else {
        0.5
    };
    let speed_score = speed_ratio * 10.0;

    // --- Risk (5%) ---
    let ncr_penalty = (manufacturer.quality.ncr_count_12mo as f64 / 5.0).min(1.0);
    let capacity_headroom = (manufacturer.capacity.monthly_units_available as f64 - pilot_qty as f64)
        / manufacturer.capacity.monthly_units_available as f64;
    let risk_score = ((1.0 - ncr_penalty) * 0.5 + capacity_headroom.clamp(0.0, 1.0) * 0.5) * 5.0;

    capability_score + quality_score + cost_score + speed_score + risk_score
}

// ============================================================================
// 6. ManufacturingPlugin
// ============================================================================

pub struct ManufacturingPlugin {
    /// Path to the investors TOML directory
    pub investors_dir: std::path::PathBuf,
    /// Path to the manufacturers TOML directory
    pub manufacturers_dir: std::path::PathBuf,
}

impl Default for ManufacturingPlugin {
    fn default() -> Self {
        Self {
            investors_dir: std::path::PathBuf::from("docs/manufacturing/investors"),
            manufacturers_dir: std::path::PathBuf::from("docs/manufacturing/manufacturers"),
        }
    }
}

impl Plugin for ManufacturingPlugin {
    fn build(&self, app: &mut App) {
        let mut registry = ManufacturingProgramRegistry::default();
        registry.investors = ManufacturingProgramRegistry::load_from_dir(&self.investors_dir);
        registry.manufacturers = ManufacturingProgramRegistry::load_from_dir(&self.manufacturers_dir);

        let investor_count = registry.investors.len();
        let manufacturer_count = registry.manufacturers.len();

        app.insert_resource(registry);

        tracing::info!(
            "ManufacturingPlugin loaded: {} investors, {} manufacturers",
            investor_count,
            manufacturer_count
        );
    }
}
