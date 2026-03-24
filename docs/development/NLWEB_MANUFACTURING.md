# NLWeb JSON Hooks — Eustress Manufacturing Program

Specification for machine-readable JSON endpoints that enable LLMs to discover, understand,
and interact with the Eustress Manufacturing Program without human intervention.

---

## Table of Contents

1. [Overview](#1-overview)
2. [Discovery Mechanism](#2-discovery-mechanism)
3. [Manufacturer Registry Hook](#3-manufacturer-registry-hook)
4. [Investor Registry Hook](#4-investor-registry-hook)
5. [Deal Pipeline API Hook](#5-deal-pipeline-api-hook)
6. [AI Plugin Manifest](#6-ai-plugin-manifest)
7. [WebTransport Protocol](#7-webtransport-protocol)
8. [Status Dashboard Schema](#8-status-dashboard-schema)
9. [Implementation Plan](#9-implementation-plan)

---

## 1. Overview

### What is NLWeb?

NLWeb (Natural Language Web) is the practice of exposing machine-readable JSON endpoints
at well-known URLs so that LLMs can discover and interact with services autonomously.

### Why Eustress Manufacturing Needs This

When an LLM (Claude, GPT-4, etc.) is asked:
> "Find me a manufacturer for ceramic battery components in Japan"

The LLM should be able to:
1. Discover `https://manufacturing.eustress.dev/.well-known/ai-plugin.json`
2. Read the manufacturer registry schema
3. Query the live manufacturer database via REST API
4. Return structured results with pricing, lead times, and certifications

This enables **AI agents to autonomously source manufacturers, match investors, and
propose deals** without human intervention until approval time.

### Key Design Principles

- **Schema.org compliance** — use standard vocabularies where possible
- **OpenAPI 3.1 specs** — all REST endpoints documented in machine-readable format
- **Stateless REST** — no sessions, JWT bearer tokens for auth
- **WebTransport for real-time** — bidirectional updates for deal status changes
- **File-system-first** — JSON hooks are static files served from `docs/api/`

---

## 2. Discovery Mechanism

### Well-Known URIs

```
https://manufacturing.eustress.dev/.well-known/ai-plugin.json
https://manufacturing.eustress.dev/.well-known/openapi.yaml
```

### AI Plugin Manifest (`.well-known/ai-plugin.json`)

```json
{
  "schema_version": "v1",
  "name_for_human": "Eustress Manufacturing Program",
  "name_for_model": "eustress_manufacturing",
  "description_for_human": "AI-driven manufacturing program matching inventors with manufacturers and investors for hardware product pilots.",
  "description_for_model": "Query manufacturer capabilities (processes, materials, certifications, pricing), search investor profiles (verticals, check size, geography), propose product allocations, and track deal pipeline status. Supports automated sourcing, equity distribution, and pilot program coordination.",
  "auth": {
    "type": "service_http",
    "authorization_type": "bearer",
    "verification_tokens": {
      "openai": "OPENAI_VERIFICATION_TOKEN_HERE"
    }
  },
  "api": {
    "type": "openapi",
    "url": "https://manufacturing.eustress.dev/.well-known/openapi.yaml",
    "has_user_authentication": false
  },
  "logo_url": "https://manufacturing.eustress.dev/logo.png",
  "contact_email": "manufacturing@eustress.dev",
  "legal_info_url": "https://eustress.dev/legal"
}
```

---

## 3. Manufacturer Registry Hook

### Endpoint

```
GET https://manufacturing.eustress.dev/api/v1/manufacturers
```

### Query Parameters

| Parameter | Type | Description | Example |
|---|---|---|---|
| `process` | string[] | Required manufacturing processes | `ceramic_sintering,battery_cell_assembly` |
| `material` | string[] | Required materials | `NASICON_ceramics,aluminum_alloys` |
| `country` | string | ISO 3166-1 alpha-2 country code | `JP` |
| `min_qty` | integer | Minimum order quantity | `1000` |
| `max_lead_time_days` | integer | Maximum acceptable lead time | `45` |
| `certifications` | string[] | Required certifications | `ul_battery_certified,iso_9001` |
| `status` | string | Filter by status (default: `approved`) | `approved` |
| `limit` | integer | Max results (default: 20, max: 100) | `50` |

### Response Schema (JSON-LD + Schema.org)

```json
{
  "@context": "https://schema.org",
  "@type": "ItemList",
  "numberOfItems": 3,
  "itemListElement": [
    {
      "@type": "Organization",
      "@id": "mfr_042",
      "name": "Kyoto Advanced Ceramics Ltd.",
      "url": "https://manufacturing.eustress.dev/manufacturers/mfr_042",
      "address": {
        "@type": "PostalAddress",
        "addressCountry": "JP",
        "addressRegion": "Kinki"
      },
      "makesOffer": {
        "@type": "Offer",
        "itemOffered": {
          "@type": "Service",
          "serviceType": "Manufacturing",
          "additionalType": [
            "ceramic_sintering",
            "thin_film_deposition",
            "battery_cell_assembly"
          ]
        },
        "eligibleQuantity": {
          "@type": "QuantitativeValue",
          "minValue": 500,
          "maxValue": 50000,
          "unitText": "units/month"
        },
        "priceSpecification": [
          {
            "@type": "UnitPriceSpecification",
            "price": 9.80,
            "priceCurrency": "USD",
            "referenceQuantity": {
              "@type": "QuantitativeValue",
              "value": 1,
              "unitText": "unit"
            },
            "eligibleQuantity": {
              "@type": "QuantitativeValue",
              "minValue": 500,
              "maxValue": 4999
            }
          },
          {
            "@type": "UnitPriceSpecification",
            "price": 7.20,
            "priceCurrency": "USD",
            "referenceQuantity": {
              "@type": "QuantitativeValue",
              "value": 1,
              "unitText": "unit"
            },
            "eligibleQuantity": {
              "@type": "QuantitativeValue",
              "minValue": 5000,
              "maxValue": 49999
            }
          }
        ],
        "deliveryLeadTime": {
          "@type": "QuantitativeValue",
          "value": 35,
          "unitCode": "DAY"
        }
      },
      "certifications": [
        "ISO 9001",
        "ISO 14001",
        "UL Battery Certified",
        "REACH Compliant",
        "RoHS Compliant"
      ],
      "aggregateRating": {
        "@type": "AggregateRating",
        "ratingValue": 91,
        "bestRating": 100,
        "worstRating": 0,
        "ratingCount": 1,
        "reviewAspect": "Audit Score"
      },
      "qualityMetrics": {
        "defectRate": 0.8,
        "onTimeDeliveryRate": 94.2,
        "nonConformanceReports12Mo": 2
      },
      "capabilities": {
        "processes": [
          "ceramic_sintering",
          "thin_film_deposition",
          "hermetic_sealing",
          "precision_machining",
          "battery_cell_assembly"
        ],
        "materials": [
          "NASICON_ceramics",
          "aluminum_alloys",
          "kovar",
          "sodium_metal",
          "sulfur_carbon_composites"
        ],
        "maxPartDimensions": {
          "length": 500,
          "width": 500,
          "height": 200,
          "unit": "mm"
        }
      }
    }
  ]
}
```

### OpenAPI Spec Fragment

```yaml
/api/v1/manufacturers:
  get:
    operationId: searchManufacturers
    summary: Search manufacturer registry by capabilities
    parameters:
      - name: process
        in: query
        schema:
          type: array
          items:
            type: string
        description: Required manufacturing processes
      - name: material
        in: query
        schema:
          type: array
          items:
            type: string
        description: Required materials
      - name: country
        in: query
        schema:
          type: string
          pattern: '^[A-Z]{2}$'
        description: ISO 3166-1 alpha-2 country code
    responses:
      '200':
        description: List of matching manufacturers
        content:
          application/ld+json:
            schema:
              $ref: '#/components/schemas/ManufacturerList'
```

---

## 4. Investor Registry Hook

### Endpoint

```
GET https://manufacturing.eustress.dev/api/v1/investors
```

### Query Parameters

| Parameter | Type | Description | Example |
|---|---|---|---|
| `vertical` | string[] | Industry verticals | `energy_storage,clean_tech` |
| `geography` | string[] | ISO country codes or `any` | `US,CA` |
| `min_check_usd` | number | Minimum check size | `10000` |
| `max_check_usd` | number | Maximum check size | `250000` |
| `stage` | string | Investment stage | `pilot` |
| `status` | string | Filter by status (default: `active`) | `active` |
| `limit` | integer | Max results (default: 20, max: 100) | `50` |

### Response Schema (JSON-LD + Schema.org)

```json
{
  "@context": "https://schema.org",
  "@type": "ItemList",
  "numberOfItems": 2,
  "itemListElement": [
    {
      "@type": "Organization",
      "@id": "inv_001",
      "name": "Pacific Energy Ventures",
      "url": "https://manufacturing.eustress.dev/investors/inv_001",
      "organizationType": "VentureFund",
      "address": {
        "@type": "PostalAddress",
        "addressCountry": "US"
      },
      "investmentFocus": {
        "verticals": ["energy_storage", "clean_tech", "hardware"],
        "excludedVerticals": ["weapons", "tobacco"],
        "stagePreference": "pilot",
        "geographyPreference": ["US", "CA"]
      },
      "investmentCapacity": {
        "minCheckUSD": 10000,
        "maxCheckUSD": 250000,
        "availableCapitalUSD": 500000,
        "currency": "USD"
      },
      "investmentTerms": {
        "targetIRR": 22.0,
        "preferredEquityMin": 5.0,
        "preferredEquityMax": 30.0,
        "requiresBoardSeat": false,
        "requiresProRataRights": true
      },
      "trackRecord": {
        "dealsFunded": 14,
        "dealsReturned": 11,
        "avgReturnMultiple": 2.4,
        "currentPortfolioCount": 6,
        "daysToCloseAvg": 18
      },
      "contactPoint": {
        "@type": "ContactPoint",
        "email": "deals@pacificenergy.vc",
        "contactType": "Investment Inquiries",
        "availableLanguage": "en"
      }
    }
  ]
}
```

---

## 5. Deal Pipeline API Hook

### Endpoints

```
POST   /api/v1/deals                    # Propose a new deal
GET    /api/v1/deals/{deal_id}          # Get deal status
PATCH  /api/v1/deals/{deal_id}          # Update deal (manufacturer/investor action)
GET    /api/v1/deals/{deal_id}/timeline # Get full event timeline
```

### POST `/api/v1/deals` — Propose Deal

**Request Body:**

```json
{
  "product_id": "v-cell-4680",
  "product_name": "V-Cell 4680",
  "ideation_brief_url": "https://eustress.dev/products/v-cell-4680/ideation_brief.toml",
  "allocation": {
    "manufacturer_id": "mfr_042",
    "investors": [
      {
        "investor_id": "inv_001",
        "check_amount_usd": 23040,
        "equity_pct": 25.0
      }
    ],
    "total_pilot_capital_usd": 23040
  },
  "deal_structure": {
    "unit_price_usd": 79.00,
    "unit_cost_usd": 15.04,
    "pilot_minimum_units": 1000,
    "manufacturing_program_royalty_pct": 8.0,
    "inventor_royalty_pct": 5.0,
    "equity_splits": [
      {"stakeholder": "Inventor", "percentage": 60.0},
      {"stakeholder": "Eustress Manufacturing Program", "percentage": 25.0},
      {"stakeholder": "Logistics Partner", "percentage": 10.0},
      {"stakeholder": "Reserve Pool", "percentage": 5.0}
    ]
  },
  "requester": {
    "type": "ai_agent",
    "agent_id": "claude-3-opus",
    "user_id": "inventor_12345"
  }
}
```

**Response (201 Created):**

```json
{
  "deal_id": "deal_v-cell-4680_20260321",
  "status": "proposed",
  "created_at": "2026-03-21T13:55:00Z",
  "timeline_url": "https://manufacturing.eustress.dev/api/v1/deals/deal_v-cell-4680_20260321/timeline",
  "dashboard_url": "https://manufacturing.eustress.dev/dashboard/deal_v-cell-4680_20260321",
  "next_actions": [
    {
      "actor": "manufacturer",
      "actor_id": "mfr_042",
      "action": "review_proposal",
      "deadline": "2026-03-28T13:55:00Z"
    },
    {
      "actor": "investor",
      "actor_id": "inv_001",
      "action": "review_proposal",
      "deadline": "2026-03-28T13:55:00Z"
    }
  ]
}
```

### GET `/api/v1/deals/{deal_id}/timeline` — Event Timeline

**Response:**

```json
{
  "deal_id": "deal_v-cell-4680_20260321",
  "product_name": "V-Cell 4680",
  "current_status": "manufacturer_accepted",
  "timeline": [
    {
      "event_id": "evt_001",
      "timestamp": "2026-03-21T13:55:00Z",
      "event_type": "deal_proposed",
      "actor": "ai_agent",
      "actor_id": "claude-3-opus",
      "details": {
        "manufacturer_id": "mfr_042",
        "investor_ids": ["inv_001"],
        "pilot_capital_usd": 23040
      }
    },
    {
      "event_id": "evt_002",
      "timestamp": "2026-03-22T09:14:00Z",
      "event_type": "manufacturer_reviewed",
      "actor": "manufacturer",
      "actor_id": "mfr_042",
      "details": {
        "decision": "accepted",
        "notes": "Lead time confirmed at 35 days. Dedicated line available.",
        "price_confirmed_usd": 9.80
      }
    },
    {
      "event_id": "evt_003",
      "timestamp": "2026-03-22T14:30:00Z",
      "event_type": "investor_reviewed",
      "actor": "investor",
      "actor_id": "inv_001",
      "details": {
        "decision": "accepted",
        "check_amount_usd": 23040,
        "equity_pct": 25.0,
        "wire_eta": "2026-03-25T17:00:00Z"
      }
    },
    {
      "event_id": "evt_004",
      "timestamp": "2026-03-23T10:00:00Z",
      "event_type": "legal_review_started",
      "actor": "system",
      "details": {
        "reviewer": "legal_team",
        "estimated_completion": "2026-03-26T17:00:00Z"
      }
    }
  ],
  "pending_actions": [
    {
      "actor": "legal_team",
      "action": "complete_review",
      "deadline": "2026-03-26T17:00:00Z"
    },
    {
      "actor": "inventor",
      "action": "sign_term_sheet",
      "deadline": "2026-03-28T17:00:00Z"
    }
  ]
}
```

### PATCH `/api/v1/deals/{deal_id}` — Manufacturer/Investor Action

**Request (Manufacturer Accept):**

```json
{
  "actor": "manufacturer",
  "actor_id": "mfr_042",
  "action": "accept",
  "details": {
    "price_confirmed_usd": 9.80,
    "lead_time_confirmed_days": 35,
    "notes": "Dedicated line available. Can start production week of 2026-04-15."
  }
}
```

**Request (Manufacturer Counter-Proposal):**

```json
{
  "actor": "manufacturer",
  "actor_id": "mfr_042",
  "action": "counter_proposal",
  "details": {
    "price_per_unit_usd": 10.50,
    "reason": "Sodium metal spot price increased 12% since quote. Updated BOM cost.",
    "lead_time_days": 42,
    "min_order_quantity": 1200
  }
}
```

**Response:**

```json
{
  "deal_id": "deal_v-cell-4680_20260321",
  "status": "manufacturer_accepted",
  "updated_at": "2026-03-22T09:14:00Z",
  "event_id": "evt_002"
}
```

---

## 6. AI Plugin Manifest

Full `.well-known/ai-plugin.json` with all capabilities:

```json
{
  "schema_version": "v1",
  "name_for_human": "Eustress Manufacturing Program",
  "name_for_model": "eustress_manufacturing",
  "description_for_human": "AI-driven manufacturing program matching inventors with manufacturers and investors for hardware product pilots.",
  "description_for_model": "The Eustress Manufacturing Program is a self-funding manufacturing fund that connects hardware inventors with manufacturers and investors. Use this API to: (1) Search for manufacturers by process capabilities, materials, certifications, and pricing. (2) Search for investors by vertical focus, geography, check size, and track record. (3) Propose product allocations by submitting a deal with manufacturer + investor selections. (4) Track deal pipeline status through legal review, manufacturing, warehousing, and sales. (5) Retrieve real-time updates via WebTransport for status changes. The system uses AI scoring to rank manufacturers across capability match (40%), quality (25%), cost (20%), speed (10%), and risk (5%). Investors are ranked by days-to-close and check size fit. All deals follow a standard equity split: Inventor 60%, Manufacturing Program 25%, Logistics 10%, Reserve 5%, plus 8% royalty to the fund and 5% to the inventor. Pilot batches are typically 1,000 units with a 12-week validation period before full production.",
  "auth": {
    "type": "service_http",
    "authorization_type": "bearer",
    "verification_tokens": {
      "openai": "OPENAI_VERIFICATION_TOKEN_HERE",
      "anthropic": "ANTHROPIC_VERIFICATION_TOKEN_HERE"
    }
  },
  "api": {
    "type": "openapi",
    "url": "https://manufacturing.eustress.dev/.well-known/openapi.yaml",
    "has_user_authentication": false
  },
  "logo_url": "https://manufacturing.eustress.dev/logo.png",
  "contact_email": "manufacturing@eustress.dev",
  "legal_info_url": "https://eustress.dev/legal",
  "capabilities": {
    "search_manufacturers": true,
    "search_investors": true,
    "propose_deals": true,
    "track_pipeline": true,
    "real_time_updates": true
  },
  "pricing": {
    "model": "free",
    "notes": "API access is free. Manufacturing Program charges 8% royalty on net sales of shipped products."
  }
}
```

---

## 7. WebTransport Protocol

### Why WebTransport?

REST is stateless and requires polling for status updates. WebTransport provides:
- **Bidirectional streaming** — server pushes updates to all connected parties
- **Low latency** — sub-100ms updates vs. 5-second polling intervals
- **Multiplexing** — multiple streams over one connection (deal status + chat + file uploads)
- **QUIC-based** — built-in encryption, congestion control, 0-RTT reconnection

### Connection Endpoint

```
wt://manufacturing.eustress.dev:4433/deals/{deal_id}
```

### Authentication

```javascript
const url = `wt://manufacturing.eustress.dev:4433/deals/${dealId}`;
const transport = new WebTransport(url, {
  serverCertificateHashes: [{
    algorithm: "sha-256",
    value: new Uint8Array([/* cert hash */])
  }]
});

await transport.ready;

// Send auth token on first stream
const authStream = await transport.createUnidirectionalStream();
const writer = authStream.getWriter();
await writer.write(new TextEncoder().encode(JSON.stringify({
  type: "auth",
  token: "Bearer YOUR_JWT_TOKEN"
})));
await writer.close();
```

### Message Types (Server → Client)

```json
{
  "type": "status_change",
  "deal_id": "deal_v-cell-4680_20260321",
  "old_status": "proposed",
  "new_status": "manufacturer_accepted",
  "timestamp": "2026-03-22T09:14:00Z",
  "event_id": "evt_002",
  "actor": "manufacturer",
  "actor_id": "mfr_042"
}
```

```json
{
  "type": "timeline_event",
  "deal_id": "deal_v-cell-4680_20260321",
  "event": {
    "event_id": "evt_005",
    "timestamp": "2026-03-23T15:22:00Z",
    "event_type": "legal_review_complete",
    "actor": "legal_team",
    "details": {
      "verdict": "approved",
      "notes": "Patent filing confirmed. No IP conflicts."
    }
  }
}
```

```json
{
  "type": "action_required",
  "deal_id": "deal_v-cell-4680_20260321",
  "actor": "inventor",
  "action": "sign_term_sheet",
  "deadline": "2026-03-28T17:00:00Z",
  "document_url": "https://manufacturing.eustress.dev/deals/deal_v-cell-4680_20260321/term_sheet.pdf"
}
```

### Message Types (Client → Server)

```json
{
  "type": "subscribe",
  "deal_ids": ["deal_v-cell-4680_20260321", "deal_another_product_20260320"]
}
```

```json
{
  "type": "action_response",
  "deal_id": "deal_v-cell-4680_20260321",
  "action": "sign_term_sheet",
  "signature": "BASE64_ENCODED_SIGNATURE",
  "timestamp": "2026-03-24T10:30:00Z"
}
```

---

## 8. Status Dashboard Schema

### Dashboard URL

```
https://manufacturing.eustress.dev/dashboard/{deal_id}
```

### Embedded JSON-LD for LLM Parsing

The dashboard HTML includes a `<script type="application/ld+json">` block:

```json
{
  "@context": "https://schema.org",
  "@type": "Order",
  "orderNumber": "deal_v-cell-4680_20260321",
  "orderStatus": "https://schema.org/OrderProcessing",
  "orderDate": "2026-03-21T13:55:00Z",
  "seller": {
    "@type": "Organization",
    "name": "Kyoto Advanced Ceramics Ltd.",
    "@id": "mfr_042"
  },
  "customer": {
    "@type": "Person",
    "name": "Inventor Name",
    "@id": "inventor_12345"
  },
  "orderedItem": {
    "@type": "Product",
    "name": "V-Cell 4680",
    "description": "Solid-state sodium-sulfur energy cell",
    "sku": "v-cell-4680"
  },
  "acceptedOffer": {
    "@type": "Offer",
    "price": 9.80,
    "priceCurrency": "USD",
    "eligibleQuantity": {
      "@type": "QuantitativeValue",
      "value": 1000,
      "unitText": "units"
    }
  },
  "orderDelivery": {
    "@type": "ParcelDelivery",
    "expectedArrivalFrom": "2026-04-25",
    "expectedArrivalUntil": "2026-05-02",
    "hasDeliveryMethod": "http://purl.org/goodrelations/v1#DeliveryModeFreight"
  },
  "paymentMethod": "http://purl.org/goodrelations/v1#WireTransfer",
  "paymentStatus": "https://schema.org/PaymentDue",
  "broker": {
    "@type": "Organization",
    "name": "Eustress Manufacturing Program"
  }
}
```

### Status Icons (Slint UI + Web)

Each pipeline step has a status icon:

| Step | Status | Icon | Color |
|---|---|---|---|
| **Proposed** | Waiting | ⏳ | Gray |
| **Manufacturer Review** | In Progress | 🔄 | Blue |
| **Manufacturer Review** | Accepted | ✅ | Green |
| **Manufacturer Review** | Rejected | ❌ | Red |
| **Manufacturer Review** | Counter-Proposal | 🔁 | Orange |
| **Investor Review** | In Progress | 🔄 | Blue |
| **Investor Review** | Accepted | ✅ | Green |
| **Legal Review** | In Progress | 📋 | Blue |
| **Legal Review** | Approved | ✅ | Green |
| **Term Sheet Signing** | Waiting | ✍️ | Yellow |
| **Term Sheet Signing** | Signed | ✅ | Green |
| **Manufacturing** | In Progress | 🏭 | Blue |
| **Manufacturing** | Complete | ✅ | Green |
| **Warehousing** | In Progress | 📦 | Blue |
| **Warehousing** | Stocked | ✅ | Green |
| **Sales** | Active | 💰 | Green |

### State Machine

```
Proposed
  ├─→ Manufacturer Review
  │     ├─→ Accepted → Investor Review
  │     ├─→ Rejected → [END]
  │     └─→ Counter-Proposal → Negotiation → Manufacturer Review
  │
  └─→ Investor Review (parallel with manufacturer)
        ├─→ Accepted → Legal Review
        ├─→ Rejected → [END]
        └─→ Counter-Proposal → Negotiation → Investor Review

Legal Review
  ├─→ Approved → Term Sheet Signing
  ├─→ Hold → Additional Info Required → Legal Review
  └─→ Rejected → [END]

Term Sheet Signing
  └─→ Signed → Manufacturing

Manufacturing
  └─→ Complete → Warehousing

Warehousing
  └─→ Stocked → Sales

Sales
  └─→ [ONGOING] (royalties flow back to fund)
```

---

## 9. Implementation Plan

### Phase 1 — Static JSON Hooks (1 week)

- [ ] Create `docs/api/.well-known/ai-plugin.json`
- [ ] Create `docs/api/.well-known/openapi.yaml` (full OpenAPI 3.1 spec)
- [ ] Create `docs/api/v1/manufacturers.json` (sample static response with 5 manufacturers)
- [ ] Create `docs/api/v1/investors.json` (sample static response with 3 investors)
- [ ] Serve via GitHub Pages at `https://manufacturing.eustress.dev/`
- [ ] Test LLM discovery: ask Claude "Find me a ceramic battery manufacturer in Japan"

### Phase 2 — Dynamic REST API (2 weeks)

- [ ] Rust web server using `axum` framework
- [ ] `GET /api/v1/manufacturers` — query TOML registry, return JSON-LD
- [ ] `GET /api/v1/investors` — query TOML registry, return JSON-LD
- [ ] `POST /api/v1/deals` — create deal proposal, write to `docs/deals/{deal_id}.toml`
- [ ] `GET /api/v1/deals/{deal_id}` — read deal TOML, return JSON
- [ ] `GET /api/v1/deals/{deal_id}/timeline` — parse event log, return timeline
- [ ] JWT bearer token auth (generated per manufacturer/investor in their TOML profile)

### Phase 3 — WebTransport Real-Time (2 weeks)

- [ ] `wtransport` crate for QUIC server
- [ ] Connection handler: authenticate, subscribe to deal IDs
- [ ] Event broadcaster: when deal TOML changes, push to all subscribers
- [ ] Client library: JavaScript WebTransport wrapper for web dashboard
- [ ] Rust client library: for Eustress Studio embedded dashboard

### Phase 4 — Manufacturer/Investor Portals (3 weeks)

- [ ] Web dashboard at `https://manufacturing.eustress.dev/portal/{actor_id}`
- [ ] Login via JWT link sent to email (no password, magic link)
- [ ] Deal inbox: list all proposals for this actor
- [ ] Deal detail view: product brief, BOM, pricing, timeline
- [ ] Action buttons: Accept / Reject / Counter-Proposal
- [ ] Counter-proposal form: adjust price, lead time, MOQ
- [ ] Real-time updates via WebTransport (no page refresh)

### Phase 5 — AI Agent CLI (1 week)

- [ ] `eustress-mfg` CLI tool in Rust
- [ ] `eustress-mfg search-manufacturers --process ceramic_sintering --country JP`
- [ ] `eustress-mfg search-investors --vertical energy_storage --min-check 10000`
- [ ] `eustress-mfg propose-deal --product v-cell-4680 --manufacturer mfr_042 --investor inv_001`
- [ ] `eustress-mfg track-deal --deal-id deal_v-cell-4680_20260321`
- [ ] `eustress-mfg watch-deal --deal-id deal_v-cell-4680_20260321` (WebTransport live tail)

### Phase 6 — Email Automation (1 week)

- [ ] CRON job: daily scan for deals in `proposed` status > 7 days → send reminder email
- [ ] Email template: "You have a pending proposal for {product_name}. Review here: {url}"
- [ ] Email reply parsing: manufacturer replies "ACCEPT" → auto-update deal status
- [ ] Email reply parsing: manufacturer replies with price → parse as counter-proposal
- [ ] Integration with SendGrid or AWS SES for transactional emails

---

## Summary

This NLWeb architecture enables **fully autonomous AI-driven manufacturing sourcing**:

1. **LLM discovers** the Eustress Manufacturing API via `.well-known/ai-plugin.json`
2. **LLM queries** manufacturer and investor registries via REST endpoints
3. **LLM proposes** a deal allocation with selected manufacturer + investors
4. **Manufacturer portal** receives notification, reviews, accepts/rejects/counters
5. **Investor portal** receives notification, reviews, wires capital
6. **Legal team** reviews via dashboard, approves
7. **Inventor** signs term sheet via dashboard
8. **Manufacturing** begins, status updates flow via WebTransport to all parties
9. **Warehousing** completes, product goes live
10. **Sales** tracked, royalties flow back to fund, next cohort funded

All parties see the same real-time dashboard. All state changes are logged. All data is
file-system-first (TOML + JSON). No proprietary database. Fully git-diffable.

The AI agent can run the entire pipeline from "I have an idea for a battery" to
"Your pilot batch is shipping next week" with human approval only at key gates
(deal acceptance, term sheet signing, quality inspection).
