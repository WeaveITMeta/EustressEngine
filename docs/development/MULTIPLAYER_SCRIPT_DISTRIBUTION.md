# Multiplayer Script Distribution Architecture

## Problem Statement

**Current Issue**: Soul scripts compile to `.rune` bytecode in each client's local cache (`~/.eustress/cache/soul/`). In multiplayer:
- Each client would need to compile scripts independently
- No guarantee of identical bytecode across clients
- Server has no control over what scripts clients run
- Potential for desync and exploits

**Required Solution**: Server-authoritative script distribution where:
1. Server compiles `.soul` → `.rune` once
2. Server distributes compiled `.rune` to all clients
3. Clients execute identical bytecode
4. Scripts respect `RunContext` (Server/Client/Both)

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        SERVER (Authoritative)                    │
├─────────────────────────────────────────────────────────────────┤
│  1. Scan SoulService/ for .soul files                           │
│  2. Compile .soul → .rune via Claude API                        │
│  3. Store in server cache: .eustress/server/soul/               │
│  4. Hash each .rune for integrity verification                  │
│  5. Distribute to clients via ScriptReplication protocol        │
└─────────────────────────────────────────────────────────────────┘
                              ↓ QUIC/TLS
                    ┌─────────────────────┐
                    │ ScriptReplication   │
                    │ - ScriptManifest    │
                    │ - ScriptChunk       │
                    │ - ScriptVerify      │
                    └─────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                      CLIENTS (Receivers)                         │
├─────────────────────────────────────────────────────────────────┤
│  1. Receive ScriptManifest (list of scripts + hashes)           │
│  2. Request missing/outdated scripts                            │
│  3. Download .rune chunks via ScriptChunk messages              │
│  4. Verify hash matches manifest                                │
│  5. Store in client cache: .eustress/client/soul/               │
│  6. Execute only scripts with RunContext::Client or Both        │
└─────────────────────────────────────────────────────────────────┘
```

## Service-Based Script Distribution

### Script Services and RunContext

Following Roblox's model, scripts have different execution contexts:

| Service | RunContext | Compiled By | Distributed To | Executed By |
|---------|-----------|-------------|----------------|-------------|
| `ServerScriptService` | Server | Server | None | Server only |
| `ReplicatedStorage` | Both | Server | All clients | Server + Clients |
| `Workspace` | Both | Server | All clients | Server + Clients |
| `StarterGui` | Client | Server | All clients | Clients only |
| `StarterPlayer` | Client | Server | All clients | Clients only |
| `ReplicatedFirst` | Client | Server | All clients | Clients (priority) |

### Key Principles

1. **Server compiles everything**: Even client-only scripts compiled on server for consistency
2. **Selective distribution**: Server-only scripts never sent to clients
3. **Hash verification**: Clients verify `.rune` integrity before execution
4. **Lazy loading**: Scripts downloaded on-demand, not all at once

## Implementation Plan

### Phase 1: Server-Side Compilation Cache

**Location**: `.eustress/server/soul/` (instead of per-user cache)

**Structure**:
```
.eustress/server/soul/
├── manifest.json              # All scripts + hashes
├── ServerScriptService/
│   ├── GameManager.rune       # Server-only
│   └── Leaderboard.rune
├── ReplicatedStorage/
│   ├── Utilities.rune         # Shared
│   └── RemoteFunctions.rune
├── Workspace/
│   ├── DoorScript.rune        # World logic
│   └── TrampolineScript.rune
└── StarterGui/
    └── HUDScript.rune         # Client UI
```

**Manifest Format** (`manifest.json`):
```json
{
  "version": 1,
  "server_tick": 12345,
  "scripts": [
    {
      "path": "ServerScriptService/GameManager.soul",
      "rune_hash": "sha256:abc123...",
      "size_bytes": 4096,
      "run_context": "Server",
      "service": "ServerScriptService",
      "replicate": false
    },
    {
      "path": "ReplicatedStorage/Utilities.soul",
      "rune_hash": "sha256:def456...",
      "size_bytes": 2048,
      "run_context": "Both",
      "service": "ReplicatedStorage",
      "replicate": true
    }
  ]
}
```

### Phase 2: Network Protocol

**New Messages** (add to `eustress-networking/src/protocol.rs`):

```rust
/// Script replication messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScriptReplicationMessage {
    /// Server → Client: Full manifest of available scripts
    ScriptManifest {
        version: u32,
        scripts: Vec<ScriptManifestEntry>,
    },
    
    /// Client → Server: Request specific scripts
    ScriptRequest {
        script_paths: Vec<String>,
    },
    
    /// Server → Client: Script bytecode chunk
    ScriptChunk {
        script_path: String,
        chunk_index: u32,
        total_chunks: u32,
        data: Vec<u8>,
    },
    
    /// Client → Server: Verify script loaded
    ScriptVerify {
        script_path: String,
        hash: String,
        success: bool,
    },
    
    /// Server → Client: Execute script now
    ExecuteScript {
        script_path: String,
        run_context: RunContext,
    },
}
```

### Phase 3: Server Systems

**1. Compile Scripts on Server Startup**

```rust
/// System: Compile all Soul scripts to server cache
fn compile_soul_scripts_for_server(
    mut pipeline: ResMut<SoulBuildPipeline>,
    registry: Res<SpaceFileRegistry>,
) {
    let server_cache = PathBuf::from(".eustress/server/soul");
    std::fs::create_dir_all(&server_cache).ok();
    
    // Find all .soul files
    for (path, entity, metadata) in registry.iter() {
        if metadata.file_type != FileType::Soul {
            continue;
        }
        
        // Determine service and RunContext
        let service = ScriptService::from_str(&metadata.service)
            .unwrap_or(ScriptService::Workspace);
        let run_context = resolve_run_context(service, false);
        
        // Compile to .rune
        let rune_path = server_cache
            .join(&metadata.service)
            .join(format!("{}.rune", metadata.name));
        
        // Queue build
        pipeline.queue_build(BuildRequest {
            entity,
            source: std::fs::read_to_string(path).unwrap(),
            name: metadata.name.clone(),
            force: true,
            scene_context: vec![],
        });
        
        // Store metadata for manifest
        // ... (hash, size, service, run_context)
    }
}
```

**2. Generate and Send Manifest**

```rust
/// System: Send script manifest to newly connected clients
fn send_script_manifest_to_clients(
    mut connected_events: EventReader<ClientConnected>,
    manifest: Res<ScriptManifest>,
    server: Res<ServerState>,
) {
    for event in connected_events.read() {
        // Filter manifest: exclude server-only scripts
        let client_scripts: Vec<_> = manifest.scripts.iter()
            .filter(|s| s.replicate)
            .cloned()
            .collect();
        
        // Send manifest
        send_message(
            event.client_id,
            ScriptReplicationMessage::ScriptManifest {
                version: manifest.version,
                scripts: client_scripts,
            }
        );
    }
}
```

**3. Handle Script Requests**

```rust
/// System: Send requested scripts to clients
fn handle_script_requests(
    mut requests: EventReader<ScriptRequest>,
    server_cache: Res<ServerScriptCache>,
) {
    for request in requests.read() {
        for script_path in &request.script_paths {
            let rune_path = server_cache.get_rune_path(script_path);
            let rune_data = std::fs::read(&rune_path).unwrap();
            
            // Chunk large scripts (max 64KB per chunk)
            const CHUNK_SIZE: usize = 65536;
            let total_chunks = (rune_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
            
            for (i, chunk) in rune_data.chunks(CHUNK_SIZE).enumerate() {
                send_message(
                    request.client_id,
                    ScriptReplicationMessage::ScriptChunk {
                        script_path: script_path.clone(),
                        chunk_index: i as u32,
                        total_chunks: total_chunks as u32,
                        data: chunk.to_vec(),
                    }
                );
            }
        }
    }
}
```

### Phase 4: Client Systems

**1. Receive and Verify Manifest**

```rust
/// System: Handle script manifest from server
fn handle_script_manifest(
    mut manifests: EventReader<ScriptManifest>,
    local_cache: Res<ClientScriptCache>,
    mut commands: Commands,
) {
    for manifest in manifests.read() {
        // Compare with local cache
        let mut missing_scripts = Vec::new();
        
        for script in &manifest.scripts {
            let local_path = local_cache.get_path(&script.path);
            
            // Check if we have it and hash matches
            if !local_path.exists() {
                missing_scripts.push(script.path.clone());
            } else {
                let local_hash = hash_file(&local_path);
                if local_hash != script.rune_hash {
                    missing_scripts.push(script.path.clone());
                }
            }
        }
        
        // Request missing scripts
        if !missing_scripts.is_empty() {
            send_message(ScriptReplicationMessage::ScriptRequest {
                script_paths: missing_scripts,
            });
        }
    }
}
```

**2. Receive and Assemble Chunks**

```rust
/// System: Receive script chunks and assemble
fn handle_script_chunks(
    mut chunks: EventReader<ScriptChunk>,
    mut assembler: ResMut<ScriptChunkAssembler>,
    local_cache: ResMut<ClientScriptCache>,
) {
    for chunk in chunks.read() {
        assembler.add_chunk(
            &chunk.script_path,
            chunk.chunk_index,
            chunk.total_chunks,
            &chunk.data,
        );
        
        // Check if complete
        if assembler.is_complete(&chunk.script_path) {
            let complete_data = assembler.take(&chunk.script_path);
            
            // Verify hash
            let hash = hash_bytes(&complete_data);
            let expected_hash = local_cache.get_expected_hash(&chunk.script_path);
            
            if hash == expected_hash {
                // Save to client cache
                let cache_path = local_cache.get_path(&chunk.script_path);
                std::fs::write(&cache_path, &complete_data).ok();
                
                // Notify server
                send_message(ScriptReplicationMessage::ScriptVerify {
                    script_path: chunk.script_path.clone(),
                    hash,
                    success: true,
                });
            } else {
                error!("Script hash mismatch: {}", chunk.script_path);
            }
        }
    }
}
```

**3. Execute Scripts**

```rust
/// System: Execute scripts when server commands
fn execute_replicated_scripts(
    mut execute_events: EventReader<ExecuteScript>,
    local_cache: Res<ClientScriptCache>,
    mut rune_engine: ResMut<RuneScriptEngine>,
) {
    for event in execute_events.read() {
        // Check RunContext
        if event.run_context == RunContext::Server {
            warn!("Client received server-only script: {}", event.script_path);
            continue;
        }
        
        // Load from cache
        let cache_path = local_cache.get_path(&event.script_path);
        let rune_bytecode = std::fs::read(&cache_path).ok();
        
        if let Some(bytecode) = rune_bytecode {
            // Execute via Rune runtime
            rune_engine.execute_bytecode(&bytecode);
        }
    }
}
```

## Security Considerations

### 1. Hash Verification
- **SHA-256** hashes for all `.rune` files
- Clients reject scripts with mismatched hashes
- Prevents tampering during transmission

### 2. Server Authority
- Only server can compile scripts
- Clients cannot inject custom scripts
- Server controls execution timing

### 3. RunContext Enforcement
- Server never sends `RunContext::Server` scripts to clients
- Clients reject server-only scripts if received
- Prevents information leakage

### 4. Sandboxing
- Rune scripts run in isolated VM
- No direct filesystem access
- Limited to approved API surface

## Performance Optimizations

### 1. Lazy Loading
- Don't send all scripts on connect
- Send manifest first, download on-demand
- Prioritize `ReplicatedFirst` scripts

### 2. Compression
- Compress `.rune` bytecode with zstd
- Typical 60-70% size reduction
- Decompress on client before execution

### 3. Caching
- Client cache persists across sessions
- Only re-download changed scripts
- Version tracking via manifest

### 4. Chunking
- Large scripts split into 64KB chunks
- Allows parallel download
- Resume on disconnect

## Migration Path

### Step 1: Server Cache (This PR)
- Move compilation to `.eustress/server/soul/`
- Generate manifest on server startup
- No client changes yet (backward compatible)

### Step 2: Network Protocol
- Add `ScriptReplicationMessage` to protocol
- Implement server-side distribution systems
- Clients still use local cache (fallback)

### Step 3: Client Integration
- Implement client-side download systems
- Switch to server-provided scripts
- Remove local compilation

### Step 4: Optimization
- Add compression
- Implement lazy loading
- Add progress UI for script downloads

## Example: Full Flow

### Server Startup
```
1. Scan SoulService/ → find "DoorScript.soul"
2. Compile via Claude → "DoorScript.rune" (2.4 KB)
3. Hash: sha256:abc123...
4. Store: .eustress/server/soul/Workspace/DoorScript.rune
5. Add to manifest.json
```

### Client Connect
```
1. Server → Client: ScriptManifest (10 scripts, 24 KB total)
2. Client checks cache: has 8/10, missing 2
3. Client → Server: ScriptRequest ["DoorScript", "HUDScript"]
4. Server → Client: ScriptChunk (DoorScript, chunk 0/1)
5. Client assembles, verifies hash, saves to cache
6. Client → Server: ScriptVerify (success)
7. Server → Client: ExecuteScript (DoorScript, RunContext::Both)
8. Client executes DoorScript.rune
```

## Related Files

- `eustress-networking/src/protocol.rs` - Add ScriptReplicationMessage
- `engine/src/soul/build_pipeline.rs` - Server compilation
- `engine/src/space/file_loader.rs` - Detect .soul files
- `engine/src/play_server/mod.rs` - Server distribution systems
- `client/src/script_loader.rs` - Client download systems (new)

## Future Enhancements

### Hot Reload
- Server recompiles changed `.soul` files
- Increments manifest version
- Clients auto-download updates
- Scripts hot-reload without reconnect

### Script Packages
- Bundle related scripts (e.g., "DoorSystem" = 5 scripts)
- Download as single unit
- Reduces round-trips

### CDN Distribution
- Large games host `.rune` on CDN
- Server provides CDN URLs in manifest
- Clients download from CDN, verify hash
- Reduces server bandwidth
