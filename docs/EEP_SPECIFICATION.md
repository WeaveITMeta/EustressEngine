# Eustress Export Protocol (EEP) v1.0 Specification

**Version**: 1.0  
**Date**: December 30, 2025  
**Status**: Draft

## Overview

The Eustress Export Protocol (EEP) is a standardized protocol for exporting consented spatial instances from EustressEngine to MCP Servers hosted on AI Models. It enables AI models to receive structured, hierarchical, multimodal training data from live 3D environments.

## Core Principles

| Principle | Description |
|-----------|-------------|
| **Consented** | Opt-in only via `AI = true` flag |
| **Hierarchical** | Preserves full parent/child structure |
| **Multimodal** | Includes geometry, properties, tags, attributes, parameters |
| **Real-time capable** | Supports live streaming and batch export |
| **Vendor-neutral** | Works with any AI model/provider |

## Architecture

```
EustressEngine (3D Scene)
        ↓
Entity with Instance Parameter (AI enabled = true)
        ↓
Parameter Router Module
        ↓
Exports to External Data Source (via Global config)
  → AI Model MCP Server
  → Postgres table
  → Firebase collection
  → JSON/CSV file
        ↓
Eustress Forge Server calls API through MCP
```

## Protocol Messages

### 1. Export Record

The primary data structure for exporting entity data.

```json
{
  "protocol_version": "eep_v1",
  "export_id": "uuid-v4",
  "timestamp": "2025-12-30T12:00:00Z",
  "space": {
    "id": "space-uuid",
    "name": "My Space",
    "settings": {}
  },
  "entity": {
    "id": "entity-uuid",
    "name": "Sacred Pillar",
    "class": "Part",
    "transform": {
      "position": [10.0, 0.0, 20.0],
      "rotation": [0.0, 0.0, 0.0, 1.0],
      "scale": [1.0, 5.0, 1.0]
    },
    "properties": {
      "Color": [0.8, 0.6, 0.2],
      "Material": "Marble"
    },
    "tags": ["temple", "sacred"],
    "attributes": {},
    "parameters": {
      "ai_training": {
        "enabled": true,
        "category": "architecture"
      }
    },
    "child_count": 0
  },
  "hierarchy": [
    {
      "id": "root-uuid",
      "name": "Workspace",
      "class": "Workspace",
      "depth": 0
    },
    {
      "id": "model-uuid",
      "name": "Temple",
      "class": "Model",
      "depth": 1
    },
    {
      "id": "entity-uuid",
      "name": "Sacred Pillar",
      "class": "Part",
      "depth": 2
    }
  ],
  "creator": {
    "source_type": "user",
    "id": "user-uuid",
    "name": "Creator Name"
  },
  "consent": {
    "ai_training": true,
    "consented_at": "2025-12-30T11:00:00Z",
    "consented_by": "user-uuid"
  }
}
```

### 2. Field Definitions

#### Export Record Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `protocol_version` | string | Yes | Protocol version (e.g., "eep_v1") |
| `export_id` | string | Yes | Unique export identifier (UUID v4) |
| `timestamp` | string | Yes | ISO 8601 timestamp |
| `space` | object | Yes | Space information |
| `entity` | object | Yes | Entity data |
| `hierarchy` | array | Yes | Hierarchy path from root to entity |
| `creator` | object | Yes | Creator information |
| `consent` | object | Yes | Consent verification |

#### Entity Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | Yes | Entity unique identifier |
| `name` | string | Yes | Entity display name |
| `class` | string | Yes | Entity class type |
| `transform` | object | Yes | Position, rotation, scale |
| `properties` | object | No | Class-specific properties |
| `tags` | array | No | String tags |
| `attributes` | object | No | Custom key-value attributes |
| `parameters` | object | No | Instance parameters by domain |
| `child_count` | integer | No | Number of child entities |

#### Transform Fields

| Field | Type | Description |
|-------|------|-------------|
| `position` | [f32; 3] | World position [x, y, z] |
| `rotation` | [f32; 4] | Quaternion [x, y, z, w] |
| `scale` | [f32; 3] | Scale [x, y, z] |

#### Creator Types

| Type | Description |
|------|-------------|
| `user` | Human user |
| `ai_model` | AI model via MCP |
| `system` | System-generated |
| `script` | Script-generated |

## MCP Server Endpoints

### Required Endpoints

AI Model MCP Servers must implement these endpoints to receive EEP data:

#### POST /eep/ingest

Receive a single export record.

**Request**:
```json
{
  "protocol_version": "eep_v1",
  "export_id": "...",
  ...
}
```

**Response**:
```json
{
  "success": true,
  "export_id": "...",
  "message": "Record ingested"
}
```

#### POST /eep/batch

Receive multiple export records.

**Request**:
```json
{
  "records": [
    { "protocol_version": "eep_v1", ... },
    { "protocol_version": "eep_v1", ... }
  ]
}
```

**Response**:
```json
{
  "success": true,
  "total": 10,
  "succeeded": 10,
  "failed": 0
}
```

#### GET /eep/capabilities

Return server capabilities.

**Response**:
```json
{
  "protocol_versions": ["eep_v1"],
  "capabilities": {
    "batch_ingest": true,
    "streaming": true,
    "max_batch_size": 1000,
    "supported_classes": ["Part", "Model", "Humanoid"]
  }
}
```

### Optional Endpoints

#### WebSocket /eep/stream

Real-time streaming of export records.

**Subscribe Message**:
```json
{
  "type": "subscribe",
  "data": {
    "space_id": "...",
    "classes": ["Part"],
    "ai_only": true
  }
}
```

**Record Message**:
```json
{
  "type": "record",
  "data": {
    "protocol_version": "eep_v1",
    ...
  }
}
```

## Authentication

### API Key Authentication

MCP Servers should authenticate using API keys in the `Authorization` header:

```
Authorization: Bearer <api_key>
```

### Per-Model Keys

Each AI model/provider should have a unique API key with configurable:
- Rate limits
- Allowed capabilities
- Enabled/disabled status

## Parameters Architecture

### 3-Tier Hierarchy

| Level | Scope | Purpose | Storage |
|-------|-------|---------|---------|
| **Global** | System-wide | Data types, connection templates | Forge Server |
| **Domain** | Logical group | Key-value schema per use case | Forge Server + local cache |
| **Instance** | Per-entity | Specific value applied to entity | Entity component |

### Built-in Domains

| Domain | Purpose | Keys |
|--------|---------|------|
| `ai_training` | AI training opt-in | `enabled`, `category`, `priority` |
| `spatial_metrics` | Analytics | `views`, `interactions`, `time_spent` |
| `user_preferences` | User settings | `favorite`, `hidden`, `custom_color` |

### Domain Schema Example

```json
{
  "id": "ai_training",
  "name": "AI Training",
  "description": "Controls AI training data inclusion",
  "keys": {
    "enabled": {
      "name": "enabled",
      "value_type": "Bool",
      "default": "false",
      "required": true,
      "description": "Whether entity is included in AI training"
    },
    "category": {
      "name": "category",
      "value_type": "String",
      "required": false,
      "description": "Training category (e.g., architecture, nature)"
    }
  },
  "export_targets": ["ai_model_mcp"],
  "requires_ai_consent": true,
  "version": 1
}
```

## Export Targets

### Target Types

| Type | Description |
|------|-------------|
| `McpServer` | AI Model MCP endpoint |
| `Postgres` | PostgreSQL database |
| `Firebase` | Firebase Firestore/Realtime DB |
| `JsonFile` | JSON file export |
| `CsvFile` | CSV file export |
| `Webhook` | Custom HTTP webhook |
| `CloudStorage` | S3/GCS/Azure Blob |

### Target Configuration

```json
{
  "id": "ai_training_mcp",
  "target_type": "McpServer",
  "connection": "https://api.ai-model.com/eep",
  "auth": {
    "auth_type": "ApiKey",
    "credentials": {
      "api_key": "..."
    }
  },
  "schema": "eep_v1",
  "enabled": true
}
```

## Consent Model

### Opt-In Only

- Entities are **not exported by default**
- Users must explicitly set `AI = true` on entities
- Consent is recorded with timestamp and user ID

### Consent Record

```json
{
  "ai_training": true,
  "consented_at": "2025-12-30T11:00:00Z",
  "consented_by": "user-uuid"
}
```

### Consent Revocation

When `AI = false` is set:
1. Entity is removed from future exports
2. Deletion event is sent to MCP servers
3. MCP servers should remove entity from training data

## Error Handling

### Error Response Format

```json
{
  "success": false,
  "error": {
    "code": "INVALID_RECORD",
    "message": "Missing required field: entity.id"
  }
}
```

### Error Codes

| Code | Description |
|------|-------------|
| `INVALID_RECORD` | Malformed export record |
| `INVALID_VERSION` | Unsupported protocol version |
| `AUTH_FAILED` | Authentication failure |
| `RATE_LIMITED` | Rate limit exceeded |
| `INTERNAL_ERROR` | Server error |

## Versioning

### Protocol Version Format

`eep_v{major}`

- **Major**: Breaking changes
- Servers should support multiple versions during transitions

### Version Negotiation

1. Client sends `protocol_version` in request
2. Server validates version is supported
3. Server responds with same version or error

## Security Considerations

1. **TLS Required**: All connections must use HTTPS/WSS
2. **API Key Rotation**: Keys should be rotatable without downtime
3. **Rate Limiting**: Prevent abuse with per-key limits
4. **Input Validation**: Validate all incoming data
5. **Audit Logging**: Log all export operations

## Implementation Checklist

### For Eustress (Exporter)

- [ ] Implement Parameter Router
- [ ] Add AI flag to entity properties
- [ ] Create export target plugins
- [ ] Implement consent tracking
- [ ] Add rate limiting

### For AI Model MCP Server (Receiver)

- [ ] Implement `/eep/ingest` endpoint
- [ ] Implement `/eep/capabilities` endpoint
- [ ] Add API key authentication
- [ ] Store records in training pipeline
- [ ] Handle consent revocation

## References

- [Eustress MCP Server](../eustress/crates/mcp/README.md)
- [Parameters Module](../eustress/crates/common/src/parameters.rs)
- [Forge Infrastructure](../infrastructure/forge/README.md)

## Changelog

### v1.0 (2025-12-30)

- Initial specification
- Core export record format
- MCP Server endpoints
- 3-tier parameters architecture
- Consent model
