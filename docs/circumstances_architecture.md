# Eustress Circumstances — Architecture

> **Module Location:** `eustress/crates/engine/src/circumstances/`  
> **Registered in:** `lib.rs` as `pub mod circumstances;`  
> **Shared Core:** Reuses `scenarios/` engine (Monte Carlo, Bayesian, branching, hierarchy)  
> **Status:** Design in progress

---

## Table of Contents

1. [Overview](#overview)
2. [Scenarios vs Circumstances](#scenarios-vs-circumstances)
3. [Core Domains](#core-domains)
4. [Shared Engine Architecture](#shared-engine-architecture)
5. [Supply Chain Domain](#supply-chain-domain)
    - 5.1 [Supply Chain Data Model](#supply-chain-data-model)
    - 5.2 [Demand Forecasting](#demand-forecasting)
    - 5.3 [Disruption Prediction & Resilience](#disruption-prediction--resilience)
    - 5.4 [Supplier Risk Scoring](#supplier-risk-scoring)
    - 5.5 [Inventory Optimization](#inventory-optimization)
    - 5.6 [Product Provenance & Anti-Counterfeit](#product-provenance--anti-counterfeit)
    - 5.7 [Recall Tracing](#recall-tracing)
    - 5.8 [Route & Cost Optimization](#route--cost-optimization)
    - 5.9 [Compliance & Audit Trail](#compliance--audit-trail)
6. [Logistics Domain](#logistics-domain)
    - 6.1 [Fleet Management & Routing](#fleet-management--routing)
    - 6.2 [Last-Mile Delivery Optimization](#last-mile-delivery-optimization)
    - 6.3 [Warehouse Operations](#warehouse-operations)
    - 6.4 [Cross-Docking & Hub Optimization](#cross-docking--hub-optimization)
7. [Business Intelligence Domain](#business-intelligence-domain)
    - 7.1 [Market Scenario Planning](#market-scenario-planning)
    - 7.2 [Pricing Strategy Optimization](#pricing-strategy-optimization)
    - 7.3 [Customer Lifetime Value Modeling](#customer-lifetime-value-modeling)
    - 7.4 [Competitive Intelligence](#competitive-intelligence)
8. [Manufacturing Domain](#manufacturing-domain)
    - 8.1 [Production Line Optimization](#production-line-optimization)
    - 8.2 [Quality Prediction & Defect Prevention](#quality-prediction--defect-prevention)
    - 8.3 [Predictive Maintenance](#predictive-maintenance)
    - 8.4 [Bill of Materials Optimization](#bill-of-materials-optimization)
9. [Healthcare & Pharma Domain](#healthcare--pharma-domain)
    - 9.1 [Drug Supply Chain Integrity](#drug-supply-chain-integrity)
    - 9.2 [Clinical Trial Scenario Modeling](#clinical-trial-scenario-modeling)
    - 9.3 [Hospital Resource Optimization](#hospital-resource-optimization)
10. [Agriculture & Food Domain](#agriculture--food-domain)
    - 10.1 [Farm-to-Table Traceability](#farm-to-table-traceability)
    - 10.2 [Crop Yield Prediction](#crop-yield-prediction)
    - 10.3 [Food Safety & Recall](#food-safety--recall)
11. [Eustress Network Effect](#eustress-network-effect)
12. [Data Sources & Adapters](#data-sources--adapters)
13. [Visualization Modes](#visualization-modes)
14. [Dependencies](#dependencies)
15. [Implementation Checklist](#implementation-checklist)

---

## Overview

> **Think of Eustress Scenarios as something the FBI would use.**  
> **Think of Eustress Circumstances as something Costco would use.**  
> Same engine. Different questions. One platform.

**Eustress Circumstances** is the positive-use, forward-looking counterpart to Eustress Scenarios. While Scenarios asks "What happened?" (investigative, backward-looking, evidence-driven), Circumstances asks **"What will happen?"** and **"What should we do?"** (operational, forward-looking, decision-driven).

Costco needs to know: Will this shipment of Kirkland batteries arrive on time? Should we reorder now or wait? Is our supplier in Shenzhen at risk of shutdown? Which warehouse should fulfill this region's demand? What happens to our margins if we drop the price $0.50? If a product recall hits, which stores have the affected lot?

The FBI needs to know: Who bought this specific item? Where did the suspect go after the store? Which evidence supports which hypothesis?

**Same probabilistic engine. Same Eustress Parameters. Same data model.** The difference is vocabulary, data sources, and optimization objectives.

Both share Monte Carlo simulations, Bayesian updates, composable micro/macro hierarchy, and 4D visualization.

**Key insight:** When multiple parties in a supply chain (manufacturers, distributors, retailers, logistics providers) all use Eustress, the data fragmentation problem disappears. Each node in the chain contributes its Eustress Parameters to a shared probabilistic model, creating end-to-end visibility that no single-vendor solution can achieve today. And when law enforcement needs cooperation, the data is already structured and queryable — no weeks of subpoenas and spreadsheet matching.

---

## Scenarios vs Circumstances

| Aspect | Scenarios | Circumstances |
|--------|-----------|---------------|
| **Orientation** | Backward-looking (what happened?) | Forward-looking (what will happen?) |
| **Primary user** | Investigators, analysts, law enforcement | Supply chain managers, logistics ops, business strategists |
| **Core question** | "Who did it? How? Why?" | "What's the optimal path? What risks exist?" |
| **Branch semantics** | Hypotheses about past events | Possible future outcomes / decision alternatives |
| **Evidence** | Crime scene data, witness statements, forensics | Sensor data, POS feeds, weather, market signals |
| **Bayesian updates** | New evidence changes hypothesis probabilities | New data changes forecast/risk probabilities |
| **Optimization goal** | Maximize confidence in correct hypothesis | Minimize cost / risk / time, maximize throughput / resilience |
| **Audit trail purpose** | Court-admissible documentation | Regulatory compliance, ISO certification |
| **Collaboration** | Multi-agency analyst roles | Multi-company supply chain roles |
| **Shared engine** | Monte Carlo, branching, hierarchy, 4D viz | Same — reused directly |

---

## Core Domains

Circumstances applies to any domain where probabilistic decision-making over branching futures adds value:

1. **Supply Chain** — End-to-end visibility, demand forecasting, disruption resilience
2. **Logistics** — Fleet routing, warehouse ops, last-mile optimization
3. **Business Intelligence** — Market planning, pricing, competitive analysis
4. **Manufacturing** — Production optimization, quality prediction, predictive maintenance
5. **Healthcare & Pharma** — Drug supply integrity, clinical trials, hospital resources
6. **Agriculture & Food** — Farm-to-table traceability, yield prediction, food safety

---

## Shared Engine Architecture

Circumstances reuses the Scenarios engine with domain-specific type aliases:

```rust
// circumstances/mod.rs — thin wrapper over scenarios engine

// Scenarios terminology → Circumstances terminology
type Circumstance = Scenario;           // A decision context
type Forecast = BranchNode;             // A possible future outcome
type Signal = Evidence;                 // Data point informing the forecast
type SignalLink = EvidenceLink;         // Signal attached to a forecast branch
type DecisionPoint = BranchNode;        // Where the user chooses an action
type Outcome = OutcomeData;             // Result of a decision path

// Reused directly
type CircumstanceParameter = ScenarioParameter;
type CircumstanceEntity = ScenarioEntity;
type CircumstanceScale = ScenarioScale;  // Micro (single shipment) / Macro (global supply chain)
```

The engine module (`scenarios/engine.rs`) runs identically — Monte Carlo simulations over branching trees with Bayesian updates. Only the UI labels, visualization defaults, and domain-specific adapters differ.

---

## Supply Chain Domain

### Supply Chain Data Model

```rust
// === Supply Chain Entities ===

enum SupplyChainRole {
    RawMaterialSupplier,
    ComponentManufacturer,
    Assembler,
    Distributor,
    Wholesaler,
    Retailer,
    LogisticsProvider,
    Customer,
    Regulator,
}

struct SupplyChainNode {
    id: Uuid,
    name: String,
    role: SupplyChainRole,
    location: GeoPoint,
    capabilities: Vec<String>,          // What this node produces/handles
    capacity: CapacityInfo,
    risk_score: f64,                    // 0.0–1.0, computed from multiple factors
    eustress_connected: bool,           // Is this node running Eustress? (network effect)
    upstream: Vec<Uuid>,                // Suppliers
    downstream: Vec<Uuid>,             // Customers
}

struct CapacityInfo {
    max_throughput: f64,                // Units per time period
    current_utilization: f64,           // 0.0–1.0
    lead_time: Duration,               // Average order-to-delivery
    lead_time_variance: f64,           // Standard deviation
    buffer_stock: f64,                  // Safety stock in units
}

// === Product Tracking (shared with Scenarios ItemEntity) ===

struct Product {
    id: Uuid,
    sku: String,
    name: String,
    category: String,
    lot_number: Option<String>,         // Batch/lot for recalls
    serial_number: Option<String>,      // Individual unit tracking
    bom: Vec<BomEntry>,                 // Bill of Materials
    provenance: ProvenanceChain,        // Full chain of custody
}

struct BomEntry {
    component_id: Uuid,                 // Product ID of component
    quantity: f64,
    supplier_id: Uuid,
    lead_time: Duration,
    unit_cost: f64,
    alternatives: Vec<Uuid>,            // Alternative suppliers/components
}

// === Shipment Tracking ===

struct Shipment {
    id: Uuid,
    origin: Uuid,                       // SupplyChainNode
    destination: Uuid,
    products: Vec<(Uuid, f64)>,         // (Product ID, quantity)
    carrier: Option<Uuid>,              // LogisticsProvider
    mode: TransportMode,
    status: ShipmentStatus,
    route: Vec<RouteWaypoint>,
    eta: DateTime<Utc>,
    eta_confidence: f64,                // How confident is the ETA
    cost: f64,
    signals: Vec<Uuid>,                 // Signal IDs affecting this shipment
}

enum TransportMode {
    Ocean,
    Air,
    Rail,
    Truck,
    Intermodal,                         // Multiple modes
    Pipeline,
    LastMile,                           // Final delivery
}

enum ShipmentStatus {
    Planned,
    InTransit,
    AtPort,
    InCustoms,
    Delayed { reason: String, new_eta: Option<DateTime<Utc>> },
    Delivered,
    Lost,
    Damaged,
}

struct RouteWaypoint {
    location: GeoPoint,
    name: String,                       // Port, warehouse, border crossing
    scheduled_arrival: DateTime<Utc>,
    actual_arrival: Option<DateTime<Utc>>,
    dwell_time: Option<Duration>,       // Time spent at this point
}
```

### Demand Forecasting

**Problem:** Each retailer forecasts independently, creating the bullwhip effect — small demand fluctuations at retail amplify into massive swings upstream.

**Solution:** Composable micro/macro Circumstances where POS data from retailers feeds directly into manufacturer forecasts via shared Eustress Parameters.

```rust
struct DemandForecast {
    product_id: Uuid,
    node_id: Uuid,                      // Which supply chain node
    time_horizon: Duration,             // How far ahead
    granularity: Duration,              // Daily, weekly, monthly buckets
    
    // Bayesian forecast
    prior: DemandDistribution,          // Historical baseline
    signals: Vec<DemandSignal>,         // Real-time updates
    posterior: DemandDistribution,       // Updated forecast
    
    // Branching outcomes
    branches: Vec<DemandBranch>,
}

struct DemandDistribution {
    mean: f64,
    std_dev: f64,
    percentiles: HashMap<u8, f64>,      // p10, p25, p50, p75, p90
}

enum DemandSignal {
    PosData { store_id: Uuid, units_sold: f64, period: Duration },
    SeasonalPattern { factor: f64 },
    PromotionPlanned { start: DateTime<Utc>, expected_lift: f64 },
    CompetitorAction { description: String, impact_estimate: f64 },
    WeatherForecast { region: String, impact: f64 },
    EconomicIndicator { name: String, value: f64, trend: f64 },
    SocialMediaTrend { sentiment: f64, volume: f64 },
}

struct DemandBranch {
    label: String,                      // "Base case", "Promotion spike", "Recession dip"
    probability: f64,
    demand_adjustment: f64,             // Multiplier on base forecast
    recommended_action: String,         // "Increase safety stock by 20%"
}
```

**Network effect:** When Walmart, Target, and Amazon all run Eustress, the manufacturer sees aggregated demand signals across all channels in real-time — not quarterly reports with 3-month lag.

### Disruption Prediction & Resilience

**Problem:** Supply chain disruptions (COVID, Suez Canal, port strikes, natural disasters) are reactive — companies find out when it's too late.

**Solution:** Live signal feeds (weather, geopolitical, port congestion, carrier data) drive probabilistic disruption branches with pre-computed contingency plans.

```rust
struct DisruptionCircumstance {
    id: Uuid,
    disruption_type: DisruptionType,
    affected_nodes: Vec<Uuid>,
    affected_shipments: Vec<Uuid>,
    probability: f64,
    severity: DisruptionSeverity,
    time_window: (DateTime<Utc>, DateTime<Utc>),
    
    // Pre-computed contingencies
    contingency_branches: Vec<ContingencyBranch>,
}

enum DisruptionType {
    NaturalDisaster { disaster_type: String, region: String },
    PortCongestion { port: String, delay_days: f64 },
    GeopoliticalEvent { description: String, countries: Vec<String> },
    SupplierFailure { supplier_id: Uuid, reason: String },
    TransportStrike { carrier_type: TransportMode, region: String },
    RegulatoryChange { regulation: String, effective_date: DateTime<Utc> },
    PandemicRestriction { region: String, severity: f64 },
    CyberAttack { target: String, impact: String },
    RawMaterialShortage { material: String, global_supply_pct: f64 },
}

enum DisruptionSeverity {
    Minor,      // <1 week delay, <5% cost increase
    Moderate,   // 1-4 week delay, 5-20% cost increase
    Major,      // 1-3 month delay, 20-50% cost increase
    Critical,   // >3 month delay, >50% cost increase or complete stoppage
}

struct ContingencyBranch {
    label: String,                      // "Reroute via air freight"
    cost_delta: f64,                    // Additional cost
    time_delta: Duration,               // Additional time
    probability_of_success: f64,
    required_actions: Vec<String>,
    auto_trigger: bool,                 // Auto-execute if disruption confirmed?
}
```

**Signal sources:**
- **Weather APIs** — NOAA, OpenWeatherMap for storm/flood prediction
- **Port congestion** — MarineTraffic, port authority APIs
- **Geopolitical** — GDELT, news sentiment analysis
- **Carrier tracking** — Real-time GPS from logistics providers running Eustress
- **Supplier health** — Financial data, production output signals

### Supplier Risk Scoring

**Problem:** Annual supplier audits are snapshots. A supplier can go from healthy to bankrupt between audits.

**Solution:** Continuous multi-signal risk scoring with Bayesian updates.

```rust
struct SupplierRiskProfile {
    supplier_id: Uuid,
    
    // Risk dimensions (each 0.0–1.0, higher = more risk)
    financial_risk: f64,                // Credit rating, payment patterns, public filings
    operational_risk: f64,              // On-time delivery rate, quality defect rate
    geopolitical_risk: f64,             // Country risk, sanctions, trade war exposure
    concentration_risk: f64,            // How dependent are we on this supplier?
    compliance_risk: f64,               // Regulatory violations, certifications expiring
    cyber_risk: f64,                    // IT security posture, breach history
    esg_risk: f64,                      // Environmental, social, governance factors
    
    // Composite
    overall_risk: f64,                  // Weighted aggregate
    trend: RiskTrend,                   // Improving, stable, deteriorating
    
    // Signals driving the score
    active_signals: Vec<Uuid>,
    last_updated: DateTime<Utc>,
    
    // Alternatives
    alternative_suppliers: Vec<(Uuid, f64)>, // (supplier_id, switching_cost)
}

enum RiskTrend {
    Improving { rate: f64 },
    Stable,
    Deteriorating { rate: f64 },
    Critical { alert: String },
}
```

### Inventory Optimization

**Problem:** Each node in the supply chain optimizes inventory locally, creating oscillations (bullwhip effect). Too much stock = capital waste. Too little = stockouts.

**Solution:** Multi-echelon inventory optimization using the composable hierarchy — store-level micros feed into regional macros feed into global.

```rust
struct InventoryCircumstance {
    product_id: Uuid,
    node_id: Uuid,
    
    // Current state
    on_hand: f64,
    in_transit: f64,
    on_order: f64,
    committed: f64,                     // Reserved for known orders
    available: f64,                     // on_hand + in_transit - committed
    
    // Optimization targets
    service_level_target: f64,          // e.g., 0.95 = 95% fill rate
    holding_cost_per_unit: f64,
    stockout_cost_per_unit: f64,
    
    // Monte Carlo output
    reorder_point: f64,                 // When to reorder
    order_quantity: f64,                // How much to order
    safety_stock: f64,                  // Buffer for uncertainty
    expected_total_cost: f64,
    
    // Branching what-ifs
    branches: Vec<InventoryBranch>,
}

struct InventoryBranch {
    label: String,                      // "Demand spike", "Supplier delay", "Normal"
    probability: f64,
    days_of_supply: f64,                // How many days current stock lasts
    stockout_risk: f64,                 // Probability of running out
    recommended_action: String,
}
```

### Product Provenance & Anti-Counterfeit

**Problem:** Counterfeit goods cost the global economy $500B+/year. No end-to-end chain of custody from manufacturer to consumer.

**Solution:** Reuse the `ProvenanceChain` from Scenarios — every handoff in the supply chain is logged with the same tamper-evident hash chain.

```rust
// Reuses ProvenanceChain from scenarios/types.rs
// Each supply chain node running Eustress adds entries:

// Manufacturer: ProvenanceEntry { action: Manufactured, custodian: "Factory X", ... }
// Distributor:  ProvenanceEntry { action: Received, custodian: "Dist Y", ... }
// Distributor:  ProvenanceEntry { action: Stored, custodian: "Warehouse Z", ... }
// Retailer:     ProvenanceEntry { action: Received, custodian: "Store #4521", ... }
// Consumer:     ProvenanceEntry { action: Sold, custodian: "POS Terminal", ... }

// Anti-counterfeit: If a product appears at a retailer without a valid
// ProvenanceChain from the manufacturer, it's flagged as potentially counterfeit.
// Gap detection in the chain = same logic as timeline gap analysis (307).
```

**Network effect:** The more nodes in the chain running Eustress, the harder it is to inject counterfeits. A product with a complete, hash-verified provenance chain from factory to shelf is provably authentic.

### Recall Tracing

**Problem:** "Which stores received batch X?" currently takes weeks of phone calls and spreadsheet matching.

**Solution:** Item tracking (SKU/lot/serial) + provenance chain = instant recall scope.

```rust
struct RecallCircumstance {
    id: Uuid,
    product_id: Uuid,
    lot_numbers: Vec<String>,           // Affected batches
    serial_range: Option<(String, String)>, // Affected serial number range
    reason: String,
    severity: RecallSeverity,
    
    // Instant trace results
    affected_nodes: Vec<AffectedNode>,
    total_units_affected: f64,
    units_sold_to_consumers: f64,
    units_still_in_channel: f64,
    units_location_unknown: f64,        // Gap in provenance chain
    
    // Action branches
    branches: Vec<RecallBranch>,
}

enum RecallSeverity {
    ClassI,     // Serious health hazard or death
    ClassII,    // Temporary or reversible health effects
    ClassIII,   // Not likely to cause adverse health effects
    Voluntary,  // Company-initiated, no regulatory mandate
}

struct AffectedNode {
    node_id: Uuid,
    units_received: f64,
    units_sold: f64,
    units_in_stock: f64,
    consumer_contact_possible: bool,    // Can we reach the end consumer?
}

struct RecallBranch {
    label: String,                      // "Full recall", "Targeted recall", "Consumer advisory"
    cost_estimate: f64,
    time_to_complete: Duration,
    consumer_reach_pct: f64,            // What % of affected consumers can we notify
    regulatory_compliance: bool,
}
```

### Route & Cost Optimization

**Problem:** Shipping route decisions are made with incomplete information — cost vs speed vs risk tradeoffs aren't modeled probabilistically.

**Solution:** Monte Carlo over routing alternatives with probabilistic costs, delays, and risks.

```rust
struct RouteCircumstance {
    shipment_id: Uuid,
    origin: GeoPoint,
    destination: GeoPoint,
    
    // Route alternatives
    routes: Vec<RouteOption>,
    
    // Optimization objective
    objective: RouteObjective,
}

struct RouteOption {
    id: Uuid,
    waypoints: Vec<RouteWaypoint>,
    mode: TransportMode,
    carrier: Option<Uuid>,
    
    // Probabilistic estimates (from Monte Carlo)
    cost: Distribution,                 // Mean, std_dev, percentiles
    transit_time: Distribution,
    delay_risk: f64,                    // P(delay > threshold)
    damage_risk: f64,                   // P(cargo damage)
    carbon_footprint: f64,              // kg CO2
    
    // Disruption exposure
    disruption_exposure: Vec<(DisruptionType, f64)>, // (type, probability)
}

enum RouteObjective {
    MinCost,
    MinTime,
    MinRisk,
    MinCarbon,
    Balanced { cost_weight: f64, time_weight: f64, risk_weight: f64, carbon_weight: f64 },
}

struct Distribution {
    mean: f64,
    std_dev: f64,
    p10: f64,
    p50: f64,
    p90: f64,
}
```

### Compliance & Audit Trail

Reuses the immutable audit trail from Scenarios (task 405) with supply-chain-specific action types:

```rust
enum SupplyChainAuditAction {
    // Product lifecycle
    ProductManufactured { product_id: Uuid, lot: String },
    ProductShipped { shipment_id: Uuid },
    ProductReceived { node_id: Uuid, product_id: Uuid },
    ProductSold { transaction_id: Uuid },
    
    // Quality
    QualityInspection { product_id: Uuid, result: String, inspector: String },
    DefectReported { product_id: Uuid, defect: String },
    
    // Compliance
    CertificationVerified { node_id: Uuid, cert_type: String, valid_until: DateTime<Utc> },
    RegulatoryFilingSubmitted { filing_type: String },
    RecallInitiated { recall_id: Uuid },
    
    // Decisions
    SupplierSelected { supplier_id: Uuid, reason: String },
    RouteChanged { shipment_id: Uuid, old_route: Uuid, new_route: Uuid, reason: String },
    InventoryReordered { product_id: Uuid, quantity: f64, supplier_id: Uuid },
}
```

Supports: ISO 9001, ISO 28000 (supply chain security), FDA 21 CFR Part 11 (pharma), FSMA (food safety), EU MDR (medical devices).

---

## Logistics Domain

### Fleet Management & Routing

```rust
struct FleetCircumstance {
    fleet: Vec<Vehicle>,
    orders: Vec<DeliveryOrder>,
    constraints: FleetConstraints,
    
    // Monte Carlo optimization output
    optimal_assignments: Vec<VehicleAssignment>,
    total_cost: Distribution,
    total_distance: Distribution,
    on_time_probability: f64,
}

struct Vehicle {
    id: Uuid,
    vehicle_type: String,               // "Box truck", "Semi", "Van", "Drone"
    capacity_weight: f64,
    capacity_volume: f64,
    current_location: GeoPoint,
    available_from: DateTime<Utc>,
    cost_per_km: f64,
    driver_hours_remaining: f64,        // HOS compliance
}

struct DeliveryOrder {
    id: Uuid,
    pickup: GeoPoint,
    delivery: GeoPoint,
    weight: f64,
    volume: f64,
    time_window: (DateTime<Utc>, DateTime<Utc>),
    priority: OrderPriority,
}

enum OrderPriority {
    Standard,
    Express,
    SameDay,
    Critical,                           // Medical, emergency
}
```

### Last-Mile Delivery Optimization

```rust
struct LastMileCircumstance {
    hub: GeoPoint,
    deliveries: Vec<DeliveryOrder>,
    
    // Environmental signals
    traffic_data: Vec<TrafficSignal>,
    weather: WeatherSignal,
    
    // Branching routes
    route_options: Vec<LastMileRoute>,
}

struct TrafficSignal {
    road_segment: (GeoPoint, GeoPoint),
    congestion_level: f64,              // 0.0 (free flow) – 1.0 (gridlock)
    predicted_clear_time: Option<DateTime<Utc>>,
}

struct LastMileRoute {
    stops: Vec<DeliveryOrder>,          // Ordered sequence
    estimated_time: Distribution,
    estimated_cost: Distribution,
    on_time_deliveries: f64,            // Expected % on-time
    driver_satisfaction: f64,           // Route complexity score
}
```

### Warehouse Operations

```rust
struct WarehouseCircumstance {
    warehouse_id: Uuid,
    
    // Current state
    occupancy: f64,                     // 0.0–1.0
    inbound_scheduled: Vec<Shipment>,
    outbound_scheduled: Vec<Shipment>,
    labor_available: usize,
    
    // Optimization branches
    pick_strategies: Vec<PickStrategy>,
    slotting_recommendations: Vec<SlottingChange>,
    
    // Predictions
    peak_occupancy_forecast: Distribution,
    labor_requirement_forecast: Distribution,
}

enum PickStrategy {
    WavePicking { wave_size: usize },
    ZonePicking { zones: Vec<String> },
    BatchPicking { batch_size: usize },
    SingleOrder,
}

struct SlottingChange {
    product_id: Uuid,
    current_location: String,           // "Aisle 3, Rack B, Shelf 2"
    recommended_location: String,
    reason: String,                     // "High velocity item, move to golden zone"
    expected_pick_time_reduction: f64,  // Seconds saved per pick
}
```

### Cross-Docking & Hub Optimization

```rust
struct HubCircumstance {
    hub_id: Uuid,
    
    // Inbound
    arriving_shipments: Vec<(Shipment, DateTime<Utc>)>,
    
    // Outbound
    departing_routes: Vec<(Uuid, DateTime<Utc>)>,
    
    // Optimization
    cross_dock_assignments: Vec<CrossDockAssignment>,
    dock_utilization: Distribution,
    turnaround_time: Distribution,
}

struct CrossDockAssignment {
    inbound_shipment: Uuid,
    outbound_route: Uuid,
    products: Vec<(Uuid, f64)>,
    dock_door: String,
    handling_time: Duration,
}
```

---

## Business Intelligence Domain

### Market Scenario Planning

```rust
struct MarketCircumstance {
    market: String,                     // "US Consumer Electronics Q2 2026"
    
    // Macro signals
    economic_indicators: Vec<EconomicSignal>,
    competitor_actions: Vec<CompetitorSignal>,
    regulatory_changes: Vec<RegulatorySignal>,
    
    // Branching futures
    scenarios: Vec<MarketScenario>,
}

struct MarketScenario {
    label: String,                      // "Bull case", "Bear case", "Base case"
    probability: f64,
    market_size: Distribution,
    growth_rate: Distribution,
    our_market_share: Distribution,
    revenue_impact: Distribution,
    recommended_strategy: String,
}
```

### Pricing Strategy Optimization

```rust
struct PricingCircumstance {
    product_id: Uuid,
    current_price: f64,
    
    // Demand elasticity model
    price_points: Vec<PricePoint>,
    competitor_prices: Vec<(String, f64)>,
    
    // Monte Carlo output
    optimal_price: f64,
    expected_revenue: Distribution,
    expected_volume: Distribution,
    margin_impact: Distribution,
}

struct PricePoint {
    price: f64,
    expected_demand: Distribution,
    expected_revenue: Distribution,
    competitor_response_probability: f64, // Will competitors match?
}
```

### Customer Lifetime Value Modeling

```rust
struct CLVCircumstance {
    segment: String,                    // Customer segment
    
    // Cohort data
    acquisition_cost: f64,
    avg_order_value: Distribution,
    purchase_frequency: Distribution,   // Orders per year
    retention_rate: Distribution,       // Year-over-year
    
    // Branching outcomes
    ltv_scenarios: Vec<CLVScenario>,
}

struct CLVScenario {
    label: String,                      // "Loyal", "At-risk", "Churned"
    probability: f64,
    lifetime_value: Distribution,
    recommended_action: String,         // "Loyalty program", "Win-back campaign"
    action_cost: f64,
    expected_roi: f64,
}
```

### Competitive Intelligence

```rust
struct CompetitiveCircumstance {
    competitor: String,
    
    // Observed signals
    pricing_changes: Vec<PricingSignal>,
    product_launches: Vec<ProductSignal>,
    market_share_trend: f64,
    hiring_signals: Vec<String>,        // Job postings indicating strategy
    patent_filings: Vec<String>,
    
    // Inferred strategy branches
    strategy_hypotheses: Vec<StrategyBranch>,
}

struct StrategyBranch {
    label: String,                      // "Price war", "Premium pivot", "Market exit"
    probability: f64,
    our_impact: Distribution,           // Revenue impact on us
    recommended_response: String,
    response_cost: f64,
    response_effectiveness: f64,
}
```

---

## Manufacturing Domain

### Production Line Optimization

```rust
struct ProductionCircumstance {
    line_id: Uuid,
    product_id: Uuid,
    
    // Current state
    throughput: f64,                    // Units per hour
    defect_rate: f64,
    downtime_pct: f64,
    
    // Optimization branches
    configuration_options: Vec<LineConfiguration>,
}

struct LineConfiguration {
    label: String,
    speed: f64,                         // Units per hour
    expected_defect_rate: Distribution,
    expected_downtime: Distribution,
    changeover_time: Duration,
    cost_per_unit: Distribution,
}
```

### Quality Prediction & Defect Prevention

```rust
struct QualityCircumstance {
    product_id: Uuid,
    batch_id: String,
    
    // Process signals
    temperature_readings: Vec<(DateTime<Utc>, f64)>,
    pressure_readings: Vec<(DateTime<Utc>, f64)>,
    humidity_readings: Vec<(DateTime<Utc>, f64)>,
    raw_material_quality: Vec<(String, f64)>,
    
    // Prediction
    defect_probability: f64,
    defect_type_distribution: HashMap<String, f64>,
    recommended_action: String,         // "Adjust temperature", "Hold batch for inspection"
    
    // Cost branches
    branches: Vec<QualityBranch>,
}

struct QualityBranch {
    label: String,                      // "Ship as-is", "Rework", "Scrap"
    probability_of_defect: f64,
    cost: f64,
    customer_impact: String,
    recall_risk: f64,
}
```

### Predictive Maintenance

```rust
struct MaintenanceCircumstance {
    equipment_id: Uuid,
    equipment_type: String,
    
    // Sensor signals
    vibration: Vec<(DateTime<Utc>, f64)>,
    temperature: Vec<(DateTime<Utc>, f64)>,
    power_consumption: Vec<(DateTime<Utc>, f64)>,
    operating_hours: f64,
    
    // Prediction
    failure_probability: Distribution,  // Over next N days
    remaining_useful_life: Distribution,
    
    // Decision branches
    branches: Vec<MaintenanceBranch>,
}

struct MaintenanceBranch {
    label: String,                      // "Schedule maintenance now", "Run to failure", "Monitor"
    cost: f64,
    downtime: Duration,
    failure_risk_if_deferred: f64,
    unplanned_downtime_cost: f64,       // Cost if it fails unexpectedly
}
```

### Bill of Materials Optimization

```rust
struct BomCircumstance {
    product_id: Uuid,
    current_bom: Vec<BomEntry>,
    
    // Alternative configurations
    alternatives: Vec<BomAlternative>,
}

struct BomAlternative {
    label: String,
    entries: Vec<BomEntry>,
    total_cost: Distribution,
    lead_time: Distribution,
    quality_impact: f64,                // -1.0 (worse) to +1.0 (better)
    supplier_risk: f64,
    availability: f64,                  // 0.0–1.0
}
```

---

## Healthcare & Pharma Domain

### Drug Supply Chain Integrity

```rust
struct DrugProvenanceCircumstance {
    ndc: String,                        // National Drug Code
    lot_number: String,
    
    // DSCSA compliance (Drug Supply Chain Security Act)
    provenance: ProvenanceChain,
    verification_status: VerificationStatus,
    
    // Counterfeit risk
    counterfeit_risk: f64,
    chain_gaps: Vec<TimelineGap>,       // Reuse from Scenarios
    temperature_excursions: Vec<TemperatureExcursion>,
}

enum VerificationStatus {
    Verified,                           // Full chain verified
    PartiallyVerified { missing: Vec<String> },
    Suspicious { reasons: Vec<String> },
    Counterfeit { evidence: Vec<String> },
}

struct TemperatureExcursion {
    timestamp: DateTime<Utc>,
    duration: Duration,
    max_temperature: f64,
    threshold: f64,
    impact: String,                     // "Potency reduced", "Product compromised"
}
```

### Clinical Trial Scenario Modeling

```rust
struct TrialCircumstance {
    trial_id: String,
    phase: TrialPhase,
    
    // Enrollment signals
    enrollment_rate: Distribution,
    dropout_rate: Distribution,
    site_performance: Vec<(String, f64)>,
    
    // Outcome branches
    efficacy_scenarios: Vec<EfficacyBranch>,
    timeline_scenarios: Vec<TimelineBranch>,
}

enum TrialPhase { I, II, III, IV }

struct EfficacyBranch {
    label: String,                      // "Primary endpoint met", "Partial response", "No effect"
    probability: f64,
    regulatory_path: String,            // "Standard approval", "Accelerated", "Rejected"
    revenue_impact: Distribution,
    time_to_market: Distribution,
}
```

### Hospital Resource Optimization

```rust
struct HospitalCircumstance {
    department: String,
    
    // Current state
    bed_occupancy: f64,
    staff_available: HashMap<String, usize>,  // Role → count
    equipment_available: HashMap<String, usize>,
    
    // Demand forecast
    admission_forecast: Distribution,
    surgery_schedule: Vec<ScheduledProcedure>,
    er_volume_forecast: Distribution,
    
    // Optimization branches
    staffing_options: Vec<StaffingBranch>,
    discharge_planning: Vec<DischargeBranch>,
}
```

---

## Agriculture & Food Domain

### Farm-to-Table Traceability

```rust
struct FoodProvenanceCircumstance {
    product: String,                    // "Romaine Lettuce"
    lot: String,
    
    // FSMA compliance (Food Safety Modernization Act)
    provenance: ProvenanceChain,
    
    // Growing conditions
    farm: FarmInfo,
    harvest_date: DateTime<Utc>,
    cold_chain: Vec<TemperatureReading>,
    
    // Risk assessment
    contamination_risk: f64,
    shelf_life_remaining: Duration,
    recall_scope: Option<RecallCircumstance>,
}

struct FarmInfo {
    name: String,
    location: GeoPoint,
    certifications: Vec<String>,        // "USDA Organic", "GAP Certified"
    water_source: String,
    adjacent_operations: Vec<String>,   // Contamination risk from neighbors
}
```

### Crop Yield Prediction

```rust
struct YieldCircumstance {
    crop: String,
    field: GeoPoint,
    planted_date: DateTime<Utc>,
    
    // Signals
    soil_moisture: Vec<(DateTime<Utc>, f64)>,
    weather_forecast: Vec<WeatherSignal>,
    satellite_ndvi: Vec<(DateTime<Utc>, f64)>,  // Vegetation index
    pest_reports: Vec<PestSignal>,
    
    // Forecast
    yield_forecast: Distribution,       // Tons per hectare
    harvest_window: (DateTime<Utc>, DateTime<Utc>),
    
    // Risk branches
    branches: Vec<YieldBranch>,
}

struct YieldBranch {
    label: String,                      // "Normal", "Drought impact", "Pest outbreak"
    probability: f64,
    yield_impact: f64,                  // Multiplier
    recommended_action: String,
    action_cost: f64,
}
```

### Food Safety & Recall

Reuses `RecallCircumstance` from Supply Chain with food-specific extensions:

```rust
struct FoodRecallExtension {
    pathogen: Option<String>,           // "E. coli O157:H7", "Salmonella", "Listeria"
    illness_reports: usize,
    hospitalizations: usize,
    cdc_investigation: bool,
    fda_classification: RecallSeverity,
    distribution_states: Vec<String>,   // Which states received the product
}
```

---

## Eustress Network Effect

The transformative insight: **Eustress Circumstances becomes exponentially more valuable as more supply chain participants adopt it.**

```
Adoption Level → Value
─────────────────────────────────────────────────
Single company    → Local optimization only
                    (still valuable, like SAP/Oracle)

Supplier + Buyer  → Shared demand signals, reduced bullwhip
                    (like EDI but probabilistic)

Full chain        → End-to-end visibility, instant recalls,
(mfg→dist→retail)   anti-counterfeit provenance, disruption
                    prediction across all nodes

Industry-wide     → Market-level intelligence, cross-chain
                    optimization, regulatory compliance as
                    a shared service

Cross-domain      → FBI + Retailers + Manufacturers =
(Scenarios +        unified platform for both investigation
 Circumstances)     AND supply chain optimization
```

**The same Eustress Parameters that help a retailer optimize inventory also help the FBI trace a purchase.** The data model is identical — `ItemEntity`, `TransactionEvidence`, `ProvenanceChain`. The difference is the query:

- **Retailer asks:** "What's the optimal reorder point for SKU X given current demand signals?"
- **FBI asks:** "Who bought SKU X at Store Y between Jan 15-31?"
- **Manufacturer asks:** "Which stores have lot #ABC that needs recall?"

Same data. Same platform. Different Circumstance vs Scenario queries.

---

## Data Sources & Adapters

### Supply Chain Specific

| Source | Format | Adapter | Domain |
|--------|--------|---------|--------|
| **EDI (Electronic Data Interchange)** | X12/EDIFACT | `EdiAdapter` | All supply chain |
| **ERP Systems** | SAP RFC/BAPI, Oracle REST | `ErpAdapter` | Manufacturing, inventory |
| **WMS (Warehouse Management)** | REST API | `WmsAdapter` | Warehouse ops |
| **TMS (Transport Management)** | REST API | `TmsAdapter` | Logistics, routing |
| **IoT Sensors** | MQTT/AMQP streams | `IotAdapter` | Manufacturing, cold chain |
| **GPS/Telematics** | Real-time stream | `TelematicsAdapter` | Fleet, shipment tracking |
| **Weather** | NOAA/OpenWeatherMap API | `WeatherAdapter` | Disruption, agriculture |
| **Port/Vessel** | MarineTraffic/AIS | `MaritimeAdapter` | Ocean freight |
| **Market Data** | Bloomberg/Reuters API | `MarketDataAdapter` | Business intelligence |
| **POS Data** | Retailer CSV/API | `RetailPosAdapter` (shared with Scenarios 407) | Demand forecasting |
| **Satellite Imagery** | Sentinel/Landsat API | `SatelliteAdapter` | Agriculture |
| **FDA/USDA** | Public API | `RegulatoryAdapter` | Food, pharma compliance |

### Shared with Scenarios

| Adapter | Shared Task |
|---------|-------------|
| `RetailPosAdapter` | 407 |
| `FinancialAdapter` | 404 |
| `NlpExtractor` | 401 |
| `AuditTrail` | 405 |
| `ProvenanceChain` | 303 |

---

## Visualization Modes

Circumstances reuses all 4 Scenarios visualization phases with domain-specific defaults:

| Viz Phase | Scenarios Use | Circumstances Use |
|-----------|--------------|-------------------|
| **Decision Tree (3D)** | Hypothesis branches | Decision alternatives (route A vs B, supplier X vs Y) |
| **Geospatial Heatmap** | Crime scene mapping | Supply chain network map, disruption zones, fleet positions |
| **Timeline Ribbon** | Event reconstruction | Shipment tracking, production schedules, demand forecasts |
| **Slint Dashboard** | Case summary | KPI dashboard, inventory levels, risk scores, cost projections |

Additional Circumstances-specific visualizations:

- **Sankey Diagram** — Material/product flow through the supply chain
- **Network Graph** — Supplier-buyer relationships (reuses link analysis from 402)
- **Gantt Chart** — Production schedules, shipment timelines
- **Control Chart** — Quality metrics with statistical process control limits

---

## Dependencies

All shared with Scenarios. Additional Circumstances-specific:

| Crate | Use | Domain |
|-------|-----|--------|
| `rumqttc` | MQTT client for IoT sensor streams | Manufacturing, cold chain |
| `geo` | Geospatial calculations (distance, routing) | Logistics |
| `chrono-tz` | Timezone-aware scheduling | Global supply chain |

---

## Implementation Checklist

### Phase 7: Circumstances Core
- [ ] **501** Circumstances type aliases and domain vocabulary (Circumstance, Forecast, Signal, DecisionPoint)
- [ ] **502** Supply chain data model (SupplyChainNode, Product, Shipment, BomEntry)
- [ ] **503** Demand forecasting engine (DemandForecast, DemandSignal, Bayesian updates from POS)
- [ ] **504** Disruption prediction (DisruptionCircumstance, signal feeds, contingency branches)
- [ ] **505** Supplier risk scoring (multi-signal continuous risk, trend detection, alternative suppliers)
- [ ] **506** Inventory optimization (multi-echelon, composable micro/macro, safety stock calculation)
- [ ] **507** Product provenance & anti-counterfeit (ProvenanceChain reuse, gap detection, hash verification)
- [ ] **508** Recall tracing (instant lot/serial trace through provenance chain, affected node mapping)
- [ ] **509** Route & cost optimization (Monte Carlo over routing alternatives, multi-objective)
- [ ] **510** Supply chain audit trail (reuse 405 with supply-chain-specific AuditAction variants)

### Phase 8: Logistics
- [ ] **511** Fleet management & vehicle routing (VRP solver, HOS compliance, capacity constraints)
- [ ] **512** Last-mile delivery optimization (traffic signals, time windows, route sequencing)
- [ ] **513** Warehouse operations (pick strategy optimization, slotting, labor forecasting)
- [ ] **514** Cross-docking & hub optimization (inbound/outbound matching, dock scheduling)

### Phase 9: Business Intelligence
- [ ] **515** Market scenario planning (economic signals, competitor actions, branching futures)
- [ ] **516** Pricing strategy optimization (demand elasticity, competitor response modeling)
- [ ] **517** Customer lifetime value modeling (cohort analysis, retention prediction, action ROI)
- [ ] **518** Competitive intelligence (signal aggregation, strategy inference, response planning)

### Phase 10: Manufacturing
- [ ] **519** Production line optimization (throughput/defect/downtime tradeoffs)
- [ ] **520** Quality prediction & defect prevention (sensor signals → defect probability)
- [ ] **521** Predictive maintenance (vibration/temp/power → remaining useful life)
- [ ] **522** Bill of materials optimization (alternative components, cost/risk/availability tradeoffs)

### Phase 11: Healthcare & Pharma
- [ ] **523** Drug supply chain integrity (DSCSA compliance, temperature excursion tracking)
- [ ] **524** Clinical trial scenario modeling (enrollment, efficacy branches, regulatory paths)
- [ ] **525** Hospital resource optimization (bed/staff/equipment forecasting)

### Phase 12: Agriculture & Food
- [ ] **526** Farm-to-table traceability (FSMA compliance, cold chain monitoring)
- [ ] **527** Crop yield prediction (weather, soil, satellite NDVI, pest signals)
- [ ] **528** Food safety & recall (pathogen tracking, distribution scope, CDC integration)

### Phase 13: Data Source Adapters
- [ ] **529** EDI adapter (X12/EDIFACT parsing)
- [ ] **530** ERP adapter (SAP/Oracle integration)
- [ ] **531** IoT/MQTT adapter (sensor streams)
- [ ] **532** GPS/Telematics adapter (fleet tracking)
- [ ] **533** Maritime/AIS adapter (vessel tracking)
- [ ] **534** Weather adapter (NOAA/OpenWeatherMap)
- [ ] **535** Satellite imagery adapter (Sentinel/Landsat NDVI)
- [ ] **536** Regulatory adapter (FDA/USDA public APIs)

### Phase 14: Circumstances Visualization
- [ ] **537** Sankey diagram (material/product flow through supply chain)
- [ ] **538** Network graph (supplier-buyer relationships, reuse link analysis 402)
- [ ] **539** Gantt chart (production schedules, shipment timelines)
- [ ] **540** Control chart (quality metrics, SPC limits)
