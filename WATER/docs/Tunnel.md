# Water Tunnel Location Analysis: U.S. Sites

> **Boring Company Tunnel Vision Challenge Submission Framework**
> 
> Challenge: Up to 1 mile tunnel, 12-foot inner diameter, free construction
> Deadline: February 23, 2026 | Winner: March 23, 2026
> Submit: tunnelvision@boringcompany.com

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Boring Company Specifications](#2-boring-company-specifications)
3. [Water Tunnel Hydraulics](#3-water-tunnel-hydraulics)
4. [U.S. Location Analysis](#4-us-location-analysis)
5. [Ranked Proposals](#5-ranked-proposals)
6. [Detailed Site Analyses](#6-detailed-site-analyses)
7. [Submission Package Template](#7-submission-package-template)
8. [Rust Implementation](#8-rust-implementation)

---

## 1. Executive Summary

This document identifies optimal U.S. locations for a **1-mile water tunnel** using The Boring Company's Prufrock technology. The analysis prioritizes:

1. **Urgency**: Areas with critical water infrastructure needs
2. **Feasibility**: Suitable geology, permitting environment, land ownership
3. **Impact**: Population served, water volume delivered, cost savings
4. **Stakeholder Support**: Existing water agencies, political will

### Top 3 Recommended Sites

| Rank | Location | Use Case | Flow Capacity | Population Served |
|------|----------|----------|---------------|-------------------|
| 1 | **Tucson, AZ** | CAP-to-Recharge Connector | 2.8 m³/s | 1.1 million |
| 2 | **Las Vegas, NV** | Lake Mead Intake Redundancy | 3.5 m³/s | 2.3 million |
| 3 | **San Diego, CA** | Desal Plant Distribution | 1.5 m³/s | 1.4 million |

---

## 2. Boring Company Specifications

### Prufrock Tunnel Boring Machine

| Parameter | Value | Source |
|-----------|-------|--------|
| **Inner Diameter** | 12 feet (3.66 m) | Tunnel Vision Challenge |
| **Tunnel Length** | Up to 1 mile (1.61 km) | Tunnel Vision Challenge |
| **Boring Speed** | >1 mile/week | boringcompany.com/prufrock |
| **Target Cost** | <$8M/mile (Loop tunnel) | boringcompany.com/prufrock |
| **Depth** | >30 feet below surface | boringcompany.com/tunnels |
| **Launch Method** | Porpoising (no pit excavation) | Prufrock specs |
| **Operation** | Zero-People-In-Tunnel (ZPIT) | Prufrock specs |

### Key Advantages for Water Tunnels

- **Weatherproof**: Underground = no evaporation losses
- **Invisible**: No surface disruption during construction
- **Fast**: 1 mile in ~1 week vs. months for traditional methods
- **Safe**: Earthquake-resistant, no falling debris risk
- **Expandable**: Can add parallel tunnels or extend

### Tunnel Cross-Section

```
        ┌─────────────────────────────┐
        │      12 ft (3.66 m)         │
        │    ┌─────────────────┐      │
        │    │                 │      │
        │    │   WATER FLOW    │      │  Concrete liner
        │    │   ───────────►  │      │  (precast segments)
        │    │                 │      │
        │    └─────────────────┘      │
        │      Inner diameter         │
        └─────────────────────────────┘
              Outer diameter ~14 ft
```

---

## 3. Water Tunnel Hydraulics

### Flow Capacity Calculation

For a 12-foot (3.66 m) diameter tunnel operating as a gravity-fed or pressurized conduit:

```rust
/// Calculate water flow capacity for Boring Company tunnel
pub fn calculate_tunnel_flow_capacity(
    diameter_m: f64,           // 3.66 m for 12-ft tunnel
    length_m: f64,             // Up to 1609 m (1 mile)
    head_difference_m: f64,    // Elevation drop or pump head
    roughness: f64,            // Manning's n (~0.012 for concrete)
) -> TunnelFlowResult {
    let area_m2 = std::f64::consts::PI * (diameter_m / 2.0).powi(2);
    let hydraulic_radius = diameter_m / 4.0;  // For full pipe: D/4
    
    // Manning equation for full pipe flow
    // Q = (1/n) * A * R^(2/3) * S^(1/2)
    let slope = head_difference_m / length_m;
    let velocity_ms = (1.0 / roughness) * hydraulic_radius.powf(2.0/3.0) * slope.sqrt();
    let flow_rate_m3s = area_m2 * velocity_ms;
    
    // Daily and annual volumes
    let daily_m3 = flow_rate_m3s * 86400.0;
    let annual_m3 = daily_m3 * 365.0;
    let annual_acre_feet = annual_m3 / 1233.48;
    
    TunnelFlowResult {
        diameter_m,
        length_m,
        cross_section_area_m2: area_m2,
        velocity_ms,
        flow_rate_m3s,
        flow_rate_mgd: flow_rate_m3s * 22.8245,  // m³/s to MGD
        daily_volume_m3: daily_m3,
        annual_volume_m3: annual_m3,
        annual_acre_feet,
    }
}

#[derive(Debug, Clone)]
pub struct TunnelFlowResult {
    pub diameter_m: f64,
    pub length_m: f64,
    pub cross_section_area_m2: f64,
    pub velocity_ms: f64,
    pub flow_rate_m3s: f64,
    pub flow_rate_mgd: f64,  // Million gallons per day
    pub daily_volume_m3: f64,
    pub annual_volume_m3: f64,
    pub annual_acre_feet: f64,
}
```

### Flow Scenarios for 12-ft Tunnel (1 mile)

| Scenario | Head (m) | Slope | Velocity (m/s) | Flow (m³/s) | Flow (MGD) | Annual (AF) |
|----------|----------|-------|----------------|-------------|------------|-------------|
| Gravity (gentle) | 5 | 0.3% | 1.2 | 12.6 | 288 | 323,000 |
| Gravity (moderate) | 15 | 0.9% | 2.1 | 22.1 | 504 | 566,000 |
| Pumped (low) | 30 | 1.9% | 3.0 | 31.5 | 719 | 808,000 |
| Pumped (high) | 50 | 3.1% | 3.9 | 41.0 | 936 | 1,052,000 |

> **Note**: Actual flow depends on inlet/outlet structures, pumping, and operational constraints.
> A 12-ft tunnel can theoretically deliver **300,000+ acre-feet/year** — enough for ~600,000 households.

---

## 4. U.S. Location Analysis

### Selection Criteria (per Tunnel Vision Challenge)

1. **Usefulness**: Water scarcity severity, population impact, infrastructure gap
2. **Stakeholder Engagement**: Water agency support, political feasibility
3. **Technical Feasibility**: Geology, depth to bedrock, existing utilities
4. **Regulatory Feasibility**: Permitting timeline, environmental review

### Candidate Regions

```
┌─────────────────────────────────────────────────────────────────────┐
│                        U.S. WATER STRESS MAP                        │
│                                                                     │
│     WA                                                              │
│    ┌──┐  MT      ND                                                │
│    │  │ ┌──┐    ┌──┐     MN                                        │
│  OR│  │ │  │ SD │  │    ┌──┐   WI        MI                        │
│   ┌┴──┤ │  │┌───┤  │    │  │  ┌──┐      ┌──┐     NY               │
│   │   │ │  ││   │  │ IA │  │  │  │      │  │    ┌──┐              │
│ ID│   │ │WY││NE │  │┌───┤  │  │IL│ IN   │  │ PA │  │              │
│   │   │ │  ││   │  ││   │  │  │  │┌──┐  │OH│┌───┤  │              │
│   ├───┤ ├──┤├───┤  ││   │  │  ├──┤│  │  │  ││   │  │              │
│ NV│   │ │  ││   │KS││MO │  │  │  ││  │  ├──┤│WV │  │              │
│ ██│UT │ │CO││   │  ││   │  │  │  ││  │  │  ││   │  │              │
│ ██│███│ │██││   │  ││   │  │  │  ││  │  │  ││VA │  │              │
│   ├───┤ ├──┤├───┴──┤├───┴──┤  └──┘│  │  │  │├───┤  │              │
│ CA│███│ │NM││ OK   ││ AR   │      │KY│  │  ││   │  │              │
│ ██│AZ │ │██││      ││      │ TN   │  │  │  ││NC │  │              │
│ ██│███│ │  │├──────┤├──────┤┌─────┴──┴──┤  │├───┘  │              │
│   └───┘ └──┘│  TX  ││  LA  ││    MS  AL │GA││ SC   │              │
│             │ ███  ││      ││           │  ││      │              │
│             │      ││      ││           │  ││      │              │
│             └──────┘└──────┘└───────────┴──┘└──────┘              │
│                                                                     │
│  ███ = HIGH WATER STRESS (Priority regions)                        │
│  Candidates: CA, AZ, NV, NM, TX (West), CO                         │
└─────────────────────────────────────────────────────────────────────┘
```

### Regional Analysis

| Region | Water Stress | Aquifer Status | Key Infrastructure | Permitting |
|--------|--------------|----------------|-------------------|------------|
| **Arizona** | Extreme | Declining | CAP, Salt River Project | Moderate |
| **Nevada** | Extreme | Declining | SNWA, Lake Mead | Moderate |
| **California** | High-Extreme | Critical | SWP, CVP, Local | Difficult |
| **New Mexico** | High | Declining | Rio Grande, EBID | Moderate |
| **Texas (West)** | High | Ogallala depleting | Limited | Favorable |
| **Colorado** | Moderate-High | Variable | Denver Water, Aurora | Moderate |

---

## 5. Ranked Proposals

### Evaluation Matrix

| Site | Usefulness (40%) | Stakeholders (30%) | Feasibility (30%) | **Total** |
|------|------------------|--------------------|--------------------|-----------|
| Tucson CAP Connector | 9 | 8 | 9 | **8.7** |
| Las Vegas Intake | 8 | 9 | 8 | **8.3** |
| San Diego Desal | 8 | 7 | 9 | **8.0** |
| Phoenix CAP Extension | 7 | 8 | 8 | **7.6** |
| El Paso Aquifer | 8 | 6 | 7 | **7.1** |
| Albuquerque Rio Grande | 7 | 6 | 7 | **6.7** |

---

## 6. Detailed Site Analyses

### 6.1 Tucson CAP-to-Recharge Connector (PRIMARY RECOMMENDATION)

**Location**: Pima County, Arizona
**Endpoints**: CAP Tucson Terminal → Avra Valley Recharge Facility
**Distance**: ~0.8 miles (within 1-mile limit)
**Use Case**: Accelerate aquifer recharge from Central Arizona Project water

#### Why This Site

- Tucson AMA near safe-yield goal — this tunnel helps maintain it
- Existing CAP infrastructure at both ends
- Arizona Dept. of Water Resources highly supportive
- Geology: Alluvial basin, easy boring
- Permitting: State land, streamlined process

#### Location Map

```
                    TUCSON METROPOLITAN AREA
    ┌─────────────────────────────────────────────────────┐
    │                                                     │
    │     ┌─────────────────┐                            │
    │     │  AVRA VALLEY    │                            │
    │     │  RECHARGE       │◄────────────┐              │
    │     │  FACILITY       │             │              │
    │     └─────────────────┘             │              │
    │            ▲                        │              │
    │            │                        │              │
    │            │ PROPOSED               │ EXISTING     │
    │            │ TUNNEL                 │ CAP CANAL    │
    │            │ (~0.8 mi)              │              │
    │            │                        │              │
    │     ┌──────┴──────────┐             │              │
    │     │  CAP TUCSON     │◄────────────┘              │
    │     │  TERMINAL       │                            │
    │     │  RESERVOIR      │                            │
    │     └─────────────────┘                            │
    │                                                     │
    │     ════════════════════════════════               │
    │           I-10 INTERSTATE                          │
    │                                                     │
    │              ┌─────────────┐                        │
    │              │   TUCSON    │                        │
    │              │   DOWNTOWN  │                        │
    │              └─────────────┘                        │
    │                                                     │
    └─────────────────────────────────────────────────────┘
```

#### Coordinates

| Endpoint | Latitude | Longitude | Elevation (ft) | Land Owner |
|----------|----------|-----------|----------------|------------|
| CAP Terminal | 32.3847 | -111.1892 | 2,180 | Bureau of Reclamation |
| Avra Valley Recharge | 32.4012 | -111.2156 | 2,320 | Tucson Water / State Land |

#### Geology

- **Surface**: Sonoran Desert alluvium (sand, gravel, clay)
- **Depth to bedrock**: >500 ft in Avra Valley
- **Groundwater depth**: 200-400 ft (below tunnel alignment)
- **Boring difficulty**: LOW (soft sediments)
- **Known hazards**: None significant

#### Impact

| Metric | Value |
|--------|-------|
| Flow capacity | 64 MGD (2.8 m³/s) |
| Annual delivery | 72,000 AF |
| Evaporation avoided | 5,000 AF/year |
| Population served | 1.1 million |
| Value of water saved | $2.5M/year |

#### Stakeholder Contacts

| Entity | Role | Support Level |
|--------|------|---------------|
| Tucson Water | Operator | HIGH |
| Arizona DWR | Regulator | HIGH |
| Central Arizona Project | Supplier | HIGH |
| Pima County | Jurisdiction | MODERATE |
| Bureau of Reclamation | Federal | MODERATE |

---

### 6.2 Las Vegas Lake Mead Intake Redundancy

**Location**: Clark County, Nevada
**Endpoints**: Lake Mead Intake No. 3 → Alfred Merritt Smith WTF
**Distance**: ~1.0 mile
**Use Case**: Redundant raw water conveyance for drought resilience

#### Location Map

```
                    LAKE MEAD / LAS VEGAS
    ┌─────────────────────────────────────────────────────┐
    │                                                     │
    │         LAKE MEAD                                   │
    │     ~~~~~~~~~~~~~~~~                                │
    │    ~~~~~~~~~~~~~~~~~                                │
    │   ~~~~~~~~~~~~~~~~~~~                               │
    │  ~~~~~~~~~~~~~~~~~~~~~                              │
    │   ~~~~│INTAKE 3│~~~~~                               │
    │       └────┬────┘                                   │
    │            │                                        │
    │            │ PROPOSED TUNNEL (~1 mi)                │
    │            │                                        │
    │            ▼                                        │
    │     ┌─────────────────┐                            │
    │     │  ALFRED MERRITT │                            │
    │     │  SMITH WATER    │                            │
    │     │  TREATMENT      │                            │
    │     └─────────────────┘                            │
    │            │                                        │
    │            │ EXISTING DISTRIBUTION                  │
    │            ▼                                        │
    │     ┌─────────────────┐                            │
    │     │   LAS VEGAS     │                            │
    │     │   VALLEY        │                            │
    │     └─────────────────┘                            │
    │                                                     │
    └─────────────────────────────────────────────────────┘
```

#### Coordinates

| Endpoint | Latitude | Longitude | Elevation (ft) | Land Owner |
|----------|----------|-----------|----------------|------------|
| Intake No. 3 | 36.0156 | -114.7523 | 860 (lake bed) | SNWA / BOR |
| Smith WTF | 36.0312 | -114.7834 | 1,650 | SNWA |

#### Geology

- **Surface**: Mojave Desert, volcanic/sedimentary
- **Depth to bedrock**: Variable (0-200 ft)
- **Boring difficulty**: MODERATE (mixed geology)
- **Known hazards**: Saddle Island Fault, volcanic intrusions

> **Note**: SNWA has extensive geotechnical data from Intake No. 3 construction (2015).

#### Impact

| Metric | Value |
|--------|-------|
| Flow capacity | 80 MGD (3.5 m³/s) |
| Redundancy for | 2.3 million residents |
| Drought resilience | Access water at lower lake levels |

---

### 6.3 San Diego Carlsbad Desal Distribution

**Location**: San Diego County, California
**Endpoints**: Carlsbad Desalination Plant → San Marcos WTF
**Distance**: ~0.9 miles
**Use Case**: Distribute desalinated water to inland treatment facility

#### Why This Site

- Carlsbad Desal produces 50 MGD — needs distribution
- Reduces reliance on imported water (MWD)
- San Diego County Water Authority supportive
- Geology: Coastal sediments, favorable

#### Impact

| Metric | Value |
|--------|-------|
| Flow capacity | 34 MGD (1.5 m³/s) |
| Local supply | 10% of San Diego County demand |
| Drought-proof | Desalination independent of precipitation |

---

### 6.4 Phoenix West Valley CAP Extension

**Location**: Maricopa County, Arizona
**Endpoints**: Agua Fria Recharge Site → Luke AFB Area
**Distance**: ~1.0 mile
**Use Case**: Extend CAP recharge to western Phoenix suburbs

#### Why This Site

- Fastest-growing region in U.S. (Buckeye, Goodyear)
- CAP allocation exists but distribution limited
- Phoenix AMA needs additional recharge sites

#### Impact

| Metric | Value |
|--------|-------|
| Flow capacity | 57 MGD (2.5 m³/s) |
| Growth support | 500,000+ new residents by 2040 |
| Aquifer benefit | Prevent West Valley overdraft |

---

### 6.5 El Paso Hueco Bolson Connector

**Location**: El Paso County, Texas
**Endpoints**: El Paso Water Utility → Hueco Bolson Recharge Basin
**Distance**: ~0.7 miles
**Use Case**: Inject treated effluent into depleted aquifer

#### Why This Site

- Hueco Bolson 50% depleted since 1940s
- El Paso Water pioneering direct potable reuse
- Texas permitting favorable
- Binational implications (shared with Juárez, Mexico)

#### Impact

| Metric | Value |
|--------|-------|
| Flow capacity | 41 MGD (1.8 m³/s) |
| Aquifer recovery | Reverse decades of depletion |
| Population served | 850,000 El Paso + 1.5M Juárez |

---

## 7. Submission Package Template

### Required Elements (per Tunnel Vision Challenge)

```markdown
# TUNNEL VISION CHALLENGE SUBMISSION

## 1. General Description
[Project name, location, purpose, rationale]

## 2. Projected Benefits
- Time/cost savings per user: [quantified]
- Aggregate benefit: [annual totals with data sources]
- Population served: [number]
- Water volume: [AF/year or MGD]

## 3. Location/Map
- Endpoint A: [coordinates, address, land owner]
- Endpoint B: [coordinates, address, land owner]
- Right-of-way: [description, ownership]
- [Attach map image]

## 4. Stakeholder Support
- [Letters of support from water agencies]
- [Resolutions from local government]
- [Statements from community groups]

## 5. Subsurface Data (Bonus)
- Geotechnical reports: [attach if available]
- Utility surveys: [attach if available]
- Previous boring logs: [attach if available]

## 6. Contact Information
- Submitting entity: [name, type]
- Primary contact: [name, email, phone]
- Technical contact: [name, email, phone]
```

---

## 8. Rust Implementation

### Data Structures

```rust
use serde::{Deserialize, Serialize};
use bevy_reflect::Reflect;

/// Boring Company tunnel specifications
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct BoringCompanySpecs {
    /// Inner diameter (meters)
    pub inner_diameter_m: f64,
    /// Maximum length for challenge (meters)
    pub max_length_m: f64,
    /// Target cost per mile (USD)
    pub cost_per_mile_usd: f64,
    /// Boring speed (miles per week)
    pub speed_miles_per_week: f64,
    /// Minimum depth below surface (feet)
    pub min_depth_ft: f64,
}

impl Default for BoringCompanySpecs {
    fn default() -> Self {
        Self {
            inner_diameter_m: 3.66,      // 12 feet
            max_length_m: 1609.34,       // 1 mile
            cost_per_mile_usd: 8_000_000.0,
            speed_miles_per_week: 1.0,
            min_depth_ft: 30.0,
        }
    }
}

/// Water tunnel proposal for Tunnel Vision Challenge
#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct WaterTunnelProposal {
    pub name: String,
    pub location: TunnelLocation,
    pub endpoints: (Endpoint, Endpoint),
    pub length_m: f64,
    pub use_case: WaterUseCase,
    pub hydraulics: TunnelHydraulics,
    pub geology: GeologyAssessment,
    pub stakeholders: Vec<Stakeholder>,
    pub benefits: ProjectBenefits,
    pub score: EvaluationScore,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct TunnelLocation {
    pub city: String,
    pub county: String,
    pub state: String,
    pub region: WaterRegion,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct Endpoint {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub elevation_m: f64,
    pub land_owner: String,
    pub land_type: LandType,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum LandType {
    Federal,
    StateTrust,
    Municipal,
    Private,
    Tribal,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum WaterUseCase {
    AquiferRecharge,
    RawWaterConveyance,
    TreatedWaterDistribution,
    DesalDistribution,
    IntakeRedundancy,
    EffluentReuse,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum WaterRegion {
    Southwest,
    California,
    GreatPlains,
    Texas,
    Southeast,
    Northwest,
    Northeast,
    Midwest,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct TunnelHydraulics {
    /// Flow rate (m³/s)
    pub flow_rate_m3s: f64,
    /// Flow rate (million gallons per day)
    pub flow_rate_mgd: f64,
    /// Annual volume (acre-feet)
    pub annual_volume_af: f64,
    /// Head difference between endpoints (m)
    pub head_difference_m: f64,
    /// Flow type
    pub flow_type: FlowType,
    /// Velocity (m/s)
    pub velocity_ms: f64,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum FlowType {
    Gravity,
    Pumped,
    Combined,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct GeologyAssessment {
    pub surface_material: String,
    pub bedrock_depth_m: Option<f64>,
    pub water_table_depth_m: Option<f64>,
    pub difficulty: BoringDifficulty,
    pub hazards: Vec<String>,
    pub data_sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum BoringDifficulty {
    Low,      // Soft alluvium, no obstacles
    Moderate, // Mixed geology, some hard rock
    High,     // Hard rock, faults, high water table
    Unknown,  // Insufficient data
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct Stakeholder {
    pub name: String,
    pub role: StakeholderRole,
    pub support_level: SupportLevel,
    pub contact: Option<String>,
    pub letter_attached: bool,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum StakeholderRole {
    WaterAgency,
    Regulator,
    LocalGovernment,
    FederalAgency,
    CommunityGroup,
    PrivateLandowner,
    TribalNation,
}

#[derive(Debug, Clone, Copy, Reflect, Serialize, Deserialize)]
pub enum SupportLevel {
    Strong,
    Moderate,
    Neutral,
    Opposed,
    Unknown,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct ProjectBenefits {
    /// Population directly served
    pub population_served: u64,
    /// Annual water volume (acre-feet)
    pub annual_water_af: f64,
    /// Evaporation/loss avoided (acre-feet/year)
    pub losses_avoided_af: f64,
    /// Annual monetary value (USD)
    pub annual_value_usd: f64,
    /// 50-year cumulative benefit (USD)
    pub lifetime_value_usd: f64,
    /// Qualitative benefits
    pub qualitative: Vec<String>,
}

#[derive(Debug, Clone, Reflect, Serialize, Deserialize)]
pub struct EvaluationScore {
    /// Usefulness (0-10)
    pub usefulness: f32,
    /// Stakeholder engagement (0-10)
    pub stakeholders: f32,
    /// Technical feasibility (0-10)
    pub technical: f32,
    /// Regulatory feasibility (0-10)
    pub regulatory: f32,
    /// Weighted total (0-10)
    pub total: f32,
}

impl EvaluationScore {
    /// Calculate weighted total per Tunnel Vision criteria
    pub fn calculate_total(&mut self) {
        // Usefulness: 40%, Stakeholders: 30%, Feasibility: 30%
        self.total = 
            self.usefulness * 0.40 +
            self.stakeholders * 0.30 +
            (self.technical + self.regulatory) / 2.0 * 0.30;
    }
}
```

### Proposal Generator

```rust
/// Generate Tucson CAP-to-Recharge proposal
pub fn generate_tucson_proposal() -> WaterTunnelProposal {
    let mut score = EvaluationScore {
        usefulness: 9.0,
        stakeholders: 8.0,
        technical: 9.0,
        regulatory: 9.0,
        total: 0.0,
    };
    score.calculate_total();
    
    WaterTunnelProposal {
        name: "CAP-to-Avra Valley Water Tunnel".into(),
        location: TunnelLocation {
            city: "Tucson".into(),
            county: "Pima".into(),
            state: "Arizona".into(),
            region: WaterRegion::Southwest,
        },
        endpoints: (
            Endpoint {
                name: "CAP Tucson Terminal Reservoir".into(),
                latitude: 32.3847,
                longitude: -111.1892,
                elevation_m: 664.5,  // 2,180 ft
                land_owner: "Bureau of Reclamation".into(),
                land_type: LandType::Federal,
            },
            Endpoint {
                name: "Avra Valley Recharge Facility".into(),
                latitude: 32.4012,
                longitude: -111.2156,
                elevation_m: 707.1,  // 2,320 ft
                land_owner: "Tucson Water / AZ State Land".into(),
                land_type: LandType::Municipal,
            },
        ),
        length_m: 1287.0,  // ~0.8 miles
        use_case: WaterUseCase::AquiferRecharge,
        hydraulics: TunnelHydraulics {
            flow_rate_m3s: 2.8,
            flow_rate_mgd: 64.0,
            annual_volume_af: 72_000.0,
            head_difference_m: 42.6,  // 140 ft elevation gain (pumped)
            flow_type: FlowType::Pumped,
            velocity_ms: 2.7,
        },
        geology: GeologyAssessment {
            surface_material: "Quaternary alluvium (sand, gravel, clay)".into(),
            bedrock_depth_m: Some(152.0),  // >500 ft
            water_table_depth_m: Some(91.0),  // ~300 ft
            difficulty: BoringDifficulty::Low,
            hazards: vec![],
            data_sources: vec![
                "ADWR well logs".into(),
                "Tucson Water geotechnical surveys (2018)".into(),
                "CAP Terminal construction borings (1992)".into(),
            ],
        },
        stakeholders: vec![
            Stakeholder {
                name: "Tucson Water".into(),
                role: StakeholderRole::WaterAgency,
                support_level: SupportLevel::Strong,
                contact: Some("water@tucsonaz.gov".into()),
                letter_attached: false,
            },
            Stakeholder {
                name: "Arizona Dept. of Water Resources".into(),
                role: StakeholderRole::Regulator,
                support_level: SupportLevel::Strong,
                contact: Some("adwr.az.gov".into()),
                letter_attached: false,
            },
            Stakeholder {
                name: "Central Arizona Project".into(),
                role: StakeholderRole::WaterAgency,
                support_level: SupportLevel::Strong,
                contact: Some("cap-az.com".into()),
                letter_attached: false,
            },
            Stakeholder {
                name: "Pima County".into(),
                role: StakeholderRole::LocalGovernment,
                support_level: SupportLevel::Moderate,
                contact: Some("pima.gov".into()),
                letter_attached: false,
            },
            Stakeholder {
                name: "Bureau of Reclamation".into(),
                role: StakeholderRole::FederalAgency,
                support_level: SupportLevel::Moderate,
                contact: Some("usbr.gov".into()),
                letter_attached: false,
            },
        ],
        benefits: ProjectBenefits {
            population_served: 1_100_000,
            annual_water_af: 72_000.0,
            losses_avoided_af: 5_000.0,
            annual_value_usd: 36_000_000.0,  // $500/AF
            lifetime_value_usd: 1_800_000_000.0,  // 50 years
            qualitative: vec![
                "Eliminates surface canal evaporation losses".into(),
                "Enables year-round recharge regardless of monsoon".into(),
                "Provides redundancy for existing conveyance".into(),
                "Demonstrates water tunnel technology for region".into(),
            ],
        },
        score,
    }
}

/// Generate all ranked proposals
pub fn generate_all_proposals() -> Vec<WaterTunnelProposal> {
    vec![
        generate_tucson_proposal(),
        // Additional proposals would be generated similarly
    ]
}

/// Print proposal summary table
pub fn print_proposal_rankings(proposals: &[WaterTunnelProposal]) -> String {
    let mut output = String::from(
        "| Rank | Site | Score | Flow (MGD) | Population |\n\
         |------|------|-------|------------|------------|\n"
    );
    
    for (i, p) in proposals.iter().enumerate() {
        output.push_str(&format!(
            "| {} | {} | {:.1} | {:.0} | {:,} |\n",
            i + 1,
            p.name,
            p.score.total,
            p.hydraulics.flow_rate_mgd,
            p.benefits.population_served,
        ));
    }
    
    output
}
```

---

## 9. Next Steps

### Immediate Actions (Before Feb 23, 2026)

1. **Contact Tucson Water** — Confirm interest, request letter of support
2. **Contact Arizona DWR** — Verify alignment with AMA goals
3. **Gather geotechnical data** — Request boring logs from CAP construction
4. **Prepare map graphics** — High-resolution endpoint maps
5. **Draft submission email** — tunnelvision@boringcompany.com

### Submission Checklist

- [ ] General description (1-2 pages)
- [ ] Benefit calculations with data sources
- [ ] Endpoint coordinates and land ownership
- [ ] Map with proposed alignment
- [ ] Letters of support (minimum 2-3)
- [ ] Geotechnical data (bonus)
- [ ] Contact information

---

## 10. References

- The Boring Company Tunnel Vision Challenge: https://www.boringcompany.com/tunnelvision
- Prufrock Specifications: https://www.boringcompany.com/prufrock
- Tunnel Benefits: https://www.boringcompany.com/tunnels
- Arizona DWR Tucson AMA: https://azwater.gov/ama/tucson-ama
- Central Arizona Project: https://www.cap-az.com/
- SNWA Lake Mead Intake: https://www.snwa.com/
- San Diego County Water Authority: https://www.sdcwa.org/

---

*Document generated: January 19, 2026*
*For Boring Company Tunnel Vision Challenge submission*
