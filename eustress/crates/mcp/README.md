# Eustress MCP (Model Control Protocol) Server

AI-controllable 3D world server that enables AI models to perform precise CRUD operations on entities via HTTP/WebSocket APIs.

## Overview

The Eustress MCP Server turns EustressEngine into a **fully scriptable, AI-controllable 3D world server**. AI models can directly create, update, delete, and query entities in live spaces without guessing or hallucinating spatial actions.

Combined with the `AI = true` property opt-in, this creates the **ultimate consented, real-time training loop**.

## Architecture (EustressStream-backed)

```
AI Model (Claude, GPT, Grok, etc.)
        ↓ (MCP calls over HTTP/WebSocket)
Eustress MCP Server (hosted on Eustress Forge)
        ↓ stream.producer("mcp.entity.create").send(&entity)
EustressStream topic fan-out (<1 µs, zero-copy)
        ↓                   ↓                    ↓
McpRouter subscriber   ChangeQueue subscriber  Properties panel
(AI consent check)     (spawn in Bevy ECS)     (refresh UI)
        ↓
"mcp.exports" topic
        ↓                   ↓                    ↓
Webhook subscriber     Console subscriber      File subscriber
(async HTTP POST)      (tracing::info)         (async file write)
        ↓
Back to AI Training Pipeline
```

All inter-component communication uses named **EustressStream topics** instead of
point-to-point channels. This gives fan-out (N subscribers), replay (ring buffer),
persistence (optional segments), and multi-transport (in-process / SHM / TCP / QUIC).

## Quick Start

### Running the Server

```rust
use eustress_mcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), McpError> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Load configuration
    let config = McpConfig::from_env()?;

    // Build and run server
    let server = McpServer::new(config);
    server.run().await?;

    Ok(())
}
```

### With Export Targets

Export targets register as independent EustressStream subscribers on `"mcp.exports"`.
Each target is fully isolated — a slow webhook never blocks the console logger.

```rust
use eustress_mcp::prelude::*;

#[tokio::main]
async fn main() -> Result<(), McpError> {
    let config = McpConfig::from_env()?;

    let server = McpServerBuilder::new(config)
        .with_console("debug")  // Log exports to console
        .with_webhook("ai_training", "https://api.example.com/eep", Some("api_key"))
        .with_file("backup", std::path::Path::new("./exports"))
        .build();

    server.run().await?;
    Ok(())
}
```

### Sharing a Stream with the Engine

Pass the engine's `ChangeQueue` stream to unify MCP with the ECS:

```rust
use eustress_mcp::prelude::*;

// In your Bevy startup system:
let change_queue: &ChangeQueue = /* from Bevy resource */;
let server = McpServer::with_stream(config, change_queue.stream.clone());
```

## API Endpoints

### Health & Info

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp/health` | GET | Health check |
| `/mcp/capabilities` | GET | Server capabilities |

### Entity CRUD

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp/create` | POST | Create entity |
| `/mcp/update` | POST | Update entity |
| `/mcp/delete` | POST | Delete entity |
| `/mcp/query` | POST | Query entities |
| `/mcp/space/:space_id` | GET | Get space info |

### Batch Operations

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/mcp/batch/create` | POST | Batch create entities |
| `/mcp/batch/delete` | POST | Batch delete entities |

## Request Examples

### Create Entity

```json
POST /mcp/create
{
  "space_id": "main",
  "class": "Part",
  "name": "Sacred Pillar",
  "position": [10, 0, 20],
  "rotation": [0, 45, 0],
  "scale": [1, 5, 1],
  "properties": {
    "Color": [0.8, 0.6, 0.2],
    "Material": "Marble"
  },
  "tags": ["temple", "sacred"],
  "ai": true
}
```

### Update Entity

```json
POST /mcp/update
{
  "space_id": "main",
  "entity_id": "12345",
  "properties": {
    "Color": [1.0, 0.0, 0.0]
  },
  "transform": {
    "position": [15, 0, 25]
  },
  "ai": true
}
```

### Query Entities

```json
POST /mcp/query
{
  "space_id": "main",
  "classes": ["Part", "Model"],
  "tags": ["sacred"],
  "ai_only": true,
  "limit": 100
}
```

### Batch Create

```json
POST /mcp/batch/create
{
  "space_id": "main",
  "entities": [
    {
      "class": "Part",
      "name": "Pillar 1",
      "position": [0, 0, 0],
      "ai": true
    },
    {
      "class": "Part",
      "name": "Pillar 2",
      "position": [10, 0, 0],
      "ai": true
    }
  ]
}
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MCP_HOST` | `0.0.0.0` | Server bind address |
| `MCP_PORT` | `8090` | Server port |
| `FORGE_URL` | `http://localhost:8080` | Forge server URL |
| `FORGE_TOKEN` | - | Forge authentication token |

### TOML Configuration

```toml
host = "0.0.0.0"
port = 8090
protocol_version = "eep_v1"

[cors]
allowed_origins = ["*"]
allow_credentials = false
max_age_secs = 3600

[rate_limit]
requests_per_minute = 60
burst_size = 10
enabled = true

[websocket]
max_message_size = 1048576
ping_interval_secs = 30
timeout_secs = 60

[forge]
url = "http://localhost:8080"
```

## AI Training Opt-In

The `ai` flag on entities controls whether they are included in training data exports:

- **Default**: `false` (not exported)
- **When `true`**: Entity data is automatically routed to configured export targets

This ensures **consented, high-quality training data** from real 3D creations.

## Export Targets

Built-in export targets (each registers as an independent EustressStream subscriber):

- **Webhook**: Async HTTP POST (spawns tokio task, never blocks dispatch)
- **Console**: Synchronous `tracing::info` (instant, zero overhead)
- **File**: Async JSON file write (spawns tokio task)
- **Embedvec**: Vector embedding + ontology indexing (optional `embedvec` feature)

### Well-Known Stream Topics

| Topic | Payload | Publisher |
|-------|---------|-----------|
| `mcp.entity.create` | JSON `EntityData` | HTTP handler |
| `mcp.entity.update` | JSON `UpdateEntityRequest` | HTTP handler |
| `mcp.entity.delete` | JSON `DeleteEntityRequest` | HTTP handler |
| `mcp.exports` | JSON `EepExportRecord` | McpRouter (AI consent gated) |
| `parameter.exports` | JSON `ExportRecord` | ParameterRouter |
| `parameter.changed` | JSON `ParameterChangedSerialized` | Bridge observer |

### Custom Export Targets

Register a closure as a stream subscriber on `"mcp.exports"`:

```rust
use eustress_mcp::prelude::*;

let stream: EustressStream = server.stream().clone();
stream.subscribe_owned("mcp.exports", move |msg| {
    let record: EepExportRecord = serde_json::from_slice(&msg.data).unwrap();
    // Your custom logic here — synchronous callback, <1 µs dispatch
    // For async work, spawn a tokio task:
    tokio::spawn(async move { /* ... */ });
});
```

## Protocol Version

This server implements **EEP v1.0** (Eustress Export Protocol).

See [EEP_SPECIFICATION.md](../../docs/EEP_SPECIFICATION.md) for the full protocol specification.

## License

MIT OR Apache-2.0
