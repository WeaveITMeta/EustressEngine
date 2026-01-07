# Eustress Networking Documentation

Comprehensive networking architecture and implementation details for Eustress Engine's multiplayer capabilities.

## Overview

Eustress uses a hybrid networking approach combining:
- **Lightyear** for client-server replication
- **bevy_quinnet** for P2P communication
- **QUIC protocol** for low-latency transport
- **CRDTs** for persistent distributed worlds

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Networking Stack                               │
├─────────────────────────────────────────────────────────────────────────┤
│  Application Layer                                                       │
│  ├── Game Logic: Entity state, events, commands                         │
│  ├── Physics Sync: Rigid body, collision detection                      │
│  └── Voice Chat: Real-time audio communication                           │
├─────────────────────────────────────────────────────────────────────────┤
│  Protocol Layer                                                          │
│  ├── Lightyear: Client-server replication (TCP/QUIC)                   │
│  ├── bevy_quinnet: P2P mesh networking (QUIC)                          │
│  └── Custom: Optimized binary protocol                                 │
├─────────────────────────────────────────────────────────────────────────┤
│  Transport Layer                                                         │
│  ├── QUIC: Modern transport with TLS 1.3                               │
│  ├── WebRTC: Browser P2P support                                        │
│  └── WebSocket: Fallback for restrictive networks                       │
├─────────────────────────────────────────────────────────────────────────┤
│  Infrastructure Layer                                                    │
│  ├── Eustress Forge: Server orchestration                               │
│  ├── Consul: Service discovery                                          │
│  └── CDN: Asset distribution                                            │
└─────────────────────────────────────────────────────────────────────────┘
```

## Key Components

### 1. Client-Server Replication (Lightyear)

**Purpose**: Authoritative server with client prediction

**Features**:
- Entity component synchronization
- Input prediction and rollback
- Client-side interpolation
- Bandwidth optimization

**Configuration**:
```rust
// lightyear config for 60Hz tick rate
let config = ClientConfig {
    tick_rate: 60,
    input_delay: 2,
    packet_loss_simulation: 0.01,
    ..Default::default()
};
```

### 2. P2P Mesh Networking (bevy_quinnet)

**Purpose**: Direct client-to-client communication

**Features**:
- NAT traversal
- Mesh topology
- Automatic failover
- Low latency voice/chat

**Use Cases**:
- Voice chat (WebRTC data channels)
- File sharing (asset streaming)
- Local multiplayer (LAN parties)

### 3. QUIC Transport

**Benefits over TCP**:
- 0-RTT connection establishment
- Built-in encryption (TLS 1.3)
- Head-of-line blocking prevention
- Connection migration

**Performance**:
- 30% lower latency than TCP
- 2x throughput on congested networks
- Better packet loss recovery

## Network Topologies

### Dedicated Server (Default)

```
Clients ←→ Forge Orchestrator ←→ Game Servers
```

**Pros**:
- Authoritative gameplay
- Anti-cheat protection
- Persistent worlds
- Easy matchmaking

**Cons**:
- Higher latency
- Server costs
- Single point of failure

### P2P Mesh (Optional)

```
Clients ←→ Mesh Network (P2P)
```

**Pros**:
- Lowest latency
- No server costs
- Resilient to failures
- Privacy-focused

**Cons**:
- Complex synchronization
- Security challenges
- Limited scalability
- NAT issues

### Hybrid (Recommended)

```
Clients ←→ Game Server (authoritative) + P2P Mesh (voice/chat)
```

**Best of both worlds**:
- Authoritative game logic
- Low-latency communication
- Scalable architecture
- Fault tolerance

## Performance Metrics

### Latency Targets

| Operation | Target | Acceptable |
|-----------|--------|------------|
| Input → Server | <50ms | <100ms |
| Server → Client | <50ms | <100ms |
| P2P Voice | <30ms | <60ms |
| Asset Streaming | N/A | <500ms first byte |

### Bandwidth Requirements

| Player Count | Upload | Download | Total |
|--------------|--------|----------|-------|
| 1-10 | 128 Kbps | 512 Kbps | 640 Kbps |
| 10-50 | 256 Kbps | 1 Mbps | 1.25 Mbps |
| 50-100 | 512 Kbps | 2 Mbps | 2.5 Mbps |
| 100+ | 1 Mbps | 4 Mbps | 5 Mbps |

## Security

### Encryption

- **TLS 1.3** for all connections
- **End-to-end encryption** for P2P
- **Certificate pinning** for server verification
- **Perfect forward secrecy**

### Anti-Cheat

- **Server-side validation** of all inputs
- **Client fingerprinting** for bot detection
- **Behavioral analysis** for cheating patterns
- **Rate limiting** for DDoS protection

### Privacy

- **Data minimization** - only collect necessary data
- **Local processing** - keep sensitive data client-side
- **Anonymous analytics** - no personal identifiers
- **GDPR compliance** - right to deletion

## Implementation Guides

### [Client Setup](./client-setup.md)
- Network configuration
- Connection management
- Error handling

### [Server Deployment](./server-deployment.md)
- Forge orchestration
- Scaling policies
- Monitoring

### [P2P Integration](./p2p-integration.md)
- Mesh networking
- NAT traversal
- WebRTC support

### [Performance Tuning](./performance-tuning.md)
- Bandwidth optimization
- Latency reduction
- Compression techniques

## Troubleshooting

### Common Issues

1. **High Latency**
   - Check server region
   - Verify network quality
   - Reduce tick rate if needed

2. **Connection Drops**
   - Implement reconnection logic
   - Check NAT traversal
   - Verify firewall settings

3. **Sync Issues**
   - Validate entity IDs
   - Check component serialization
   - Review replication rules

### Debug Tools

- **Network profiler**: Real-time metrics
- **Packet inspector**: Protocol analysis
- **Connection tester**: Diagnostic tools
- **Performance dashboard**: Grafana metrics

## API Reference

### Network Events

```rust
// Connection events
#[derive(Event)]
pub struct ConnectedEvent { pub client_id: ClientId }

#[derive(Event)]
pub struct DisconnectedEvent { pub client_id: ClientId, pub reason: String }

// Data events
#[derive(Event)]
pub struct MessageEvent { pub from: ClientId, pub data: Vec<u8> }

#[derive(Event)]
pub struct EntityUpdateEvent { pub entity: Entity, pub components: Vec<Component> }
```

### Network Components

```rust
// Networked entity
#[derive(Component, Serialize, Deserialize)]
pub struct Networked {
    pub owner: ClientId,
    pub replication: ReplicationConfig,
}

// Transform sync
#[derive(Component, Serialize, Deserialize)]
pub struct NetworkTransform {
    pub position: Vec3,
    pub rotation: Quat,
    pub timestamp: u64,
}
```

## Future Roadmap

- **WebTransport**: Next-gen web networking
- **QUIC v2**: Protocol improvements
- **Edge computing**: Regional edge servers
- **5G optimization**: Mobile networking
- **IPv6 support**: Future-proofing