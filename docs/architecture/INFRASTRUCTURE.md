# Eustress Infrastructure Architecture

> ⚠️ **DEPRECATED**: This document describes the **OLD** Kubernetes/k3s MoE architecture.
> 
> **This approach has been superseded by [Eustress Forge](./EUSTRESS_FORGE.md)** - a Rust-native 
> multiplayer orchestration platform built on HashiCorp Nomad with a custom Rust control plane.
> 
> **Reasons for deprecation:**
> - K8s overhead (3-7% cluster waste) vs Nomad (<0.5%)
> - Slower scaling latency (seconds-minutes) vs Forge (milliseconds)
> - Higher cost at scale vs 80-90% savings with Forge
> - Complex CRD/controller abstractions vs pure Rust SDK
>
> **Migration**: See `infrastructure/forge/` for the new Nomad + Terraform deployment.
> 
> ---
> 
> # [DEAD CODE] Legacy K8s MoE Architecture

## Overview (DEPRECATED)

~~Eustress uses a **Mixture of Experts (MoE)** architecture on **Kubernetes (k3s)** hosted on **Hetzner Cloud**.~~ This approach maximizes server utilization by routing different workload types to specialized "expert" pods rather than running monolithic game servers.

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    Kubernetes Cluster                       │
│  ┌─────────────────────────────────────────────────────┐    │
│  │                   Hetzner Node                      │    │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐    │    │
│  │  │ Game    │ │ Physics │ │ AI/NPC  │ │ Asset   │    │    │
│  │  │ Logic   │ │ Expert  │ │ Expert  │ │ Serving │    │    │
│  │  │ Pod     │ │ Pod     │ │ Pod     │ │ Pod     │    │    │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘    │    │
│  └─────────────────────────────────────────────────────┘    │
│                           ▲                                 │
│                           │                                 │
│              ┌────────────┴────────────┐                    │
│              │    Ingress / Gateway    │                    │
│              │    (Request Router)     │                    │
│              └─────────────────────────┘                    │
└─────────────────────────────────────────────────────────────┘
```

---

## Mixture of Experts (MoE)

### Concept

Instead of homogeneous game servers where each instance runs all workloads, MoE splits responsibilities across specialized experts:

| Expert | Purpose | Resource Profile | K8s Requests |
|--------|---------|------------------|--------------|
| **GameLogic** | Player state, events, scripting | CPU-bound | 500m CPU, 512Mi |
| **Physics** | Collision, rigid body, Avian | CPU-heavy, SIMD | 1000m CPU, 256Mi |
| **AINPC** | Pathfinding, behavior trees, ML | CPU/GPU | 500m CPU, 1Gi |
| **AssetServing** | Streaming models, textures | I/O-bound | 100m CPU, 2Gi |
| **VoiceChat** | Real-time comms | Network-bound | 200m CPU, 256Mi |
| **Matchmaking** | Player pairing, lobbies | Memory-bound | 200m CPU, 1Gi |
| **Analytics** | Telemetry, metrics | Batch | 100m CPU, 512Mi |
| **Moderation** | Chat filtering, reports | Burst | 200m CPU, 512Mi |

### Benefits

1. **Higher utilization** — CPU-bound experts fill gaps when I/O experts wait
2. **Specialized optimization** — Physics expert uses AVX-512, AI expert can use GPU
3. **Elastic scaling** — Spin up more physics experts during combat-heavy games
4. **Fault isolation** — AI crash doesn't kill game logic
5. **Cost efficiency** — One server does work of 3-4 specialized ones

---

## Kubernetes Integration

### Cluster Setup (Hetzner + k3s)

```yaml
# infrastructure/k8s/cluster.yaml
apiVersion: k3s.cattle.io/v1
kind: Cluster
metadata:
  name: eustress-prod
spec:
  # Hetzner Cloud provider
  cloudProvider: hetzner
  
  # Node pools
  nodePools:
    # Game server nodes (CPU optimized)
    - name: game-servers
      machineType: cpx31  # 4 vCPU, 8GB RAM
      count: 4
      labels:
        workload: game
      taints: []
      
    # AI/ML nodes (GPU optional)
    - name: ai-workers
      machineType: ccx33  # 8 vCPU, 32GB RAM
      count: 2
      labels:
        workload: ai
        
    # Storage/API nodes
    - name: api-servers
      machineType: cpx21  # 3 vCPU, 4GB RAM
      count: 2
      labels:
        workload: api
```

### Expert Deployments

#### Game Logic Expert

```yaml
# infrastructure/k8s/experts/game-logic.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: expert-game-logic
  namespace: eustress-game
spec:
  replicas: 8
  selector:
    matchLabels:
      expert: game-logic
  template:
    metadata:
      labels:
        expert: game-logic
        moe: "true"
    spec:
      nodeSelector:
        workload: game
      containers:
        - name: game-logic
          image: ghcr.io/eustress/expert-game-logic:latest
          resources:
            requests:
              cpu: "500m"
              memory: "512Mi"
            limits:
              cpu: "1000m"
              memory: "1Gi"
          ports:
            - containerPort: 7000
              name: grpc
          env:
            - name: EXPERT_TYPE
              value: "game_logic"
            - name: RUST_LOG
              value: "info"
          livenessProbe:
            grpc:
              port: 7000
            initialDelaySeconds: 5
          readinessProbe:
            grpc:
              port: 7000
            initialDelaySeconds: 3
---
apiVersion: v1
kind: Service
metadata:
  name: expert-game-logic
  namespace: eustress-game
spec:
  selector:
    expert: game-logic
  ports:
    - port: 7000
      targetPort: 7000
      name: grpc
```

#### Physics Expert

```yaml
# infrastructure/k8s/experts/physics.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: expert-physics
  namespace: eustress-game
spec:
  replicas: 4
  selector:
    matchLabels:
      expert: physics
  template:
    metadata:
      labels:
        expert: physics
        moe: "true"
    spec:
      nodeSelector:
        workload: game
      containers:
        - name: physics
          image: ghcr.io/eustress/expert-physics:latest
          resources:
            requests:
              cpu: "1000m"  # Physics needs more CPU
              memory: "256Mi"
            limits:
              cpu: "2000m"
              memory: "512Mi"
          ports:
            - containerPort: 7001
              name: grpc
          env:
            - name: EXPERT_TYPE
              value: "physics"
            - name: PHYSICS_SUBSTEPS
              value: "4"
```

#### AI/NPC Expert

```yaml
# infrastructure/k8s/experts/ai-npc.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: expert-ai-npc
  namespace: eustress-game
spec:
  replicas: 2
  selector:
    matchLabels:
      expert: ai-npc
  template:
    metadata:
      labels:
        expert: ai-npc
        moe: "true"
    spec:
      nodeSelector:
        workload: ai
      containers:
        - name: ai-npc
          image: ghcr.io/eustress/expert-ai-npc:latest
          resources:
            requests:
              cpu: "500m"
              memory: "1Gi"
            limits:
              cpu: "2000m"
              memory: "4Gi"
          ports:
            - containerPort: 7002
              name: grpc
```

### MoE Router (Gateway)

```yaml
# infrastructure/k8s/router/moe-router.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: moe-router
  namespace: eustress-game
spec:
  replicas: 3
  selector:
    matchLabels:
      app: moe-router
  template:
    spec:
      containers:
        - name: router
          image: ghcr.io/eustress/moe-router:latest
          resources:
            requests:
              cpu: "200m"
              memory: "256Mi"
          ports:
            - containerPort: 8080
              name: http
            - containerPort: 8081
              name: grpc
          env:
            - name: EXPERTS_GAME_LOGIC
              value: "expert-game-logic:7000"
            - name: EXPERTS_PHYSICS
              value: "expert-physics:7001"
            - name: EXPERTS_AI_NPC
              value: "expert-ai-npc:7002"
            - name: EXPERTS_ASSET
              value: "expert-asset:7003"
---
apiVersion: v1
kind: Service
metadata:
  name: moe-router
  namespace: eustress-game
spec:
  type: LoadBalancer
  selector:
    app: moe-router
  ports:
    - port: 80
      targetPort: 8080
      name: http
    - port: 443
      targetPort: 8080
      name: https
```

### Horizontal Pod Autoscaler

```yaml
# infrastructure/k8s/autoscaling/hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: expert-game-logic-hpa
  namespace: eustress-game
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: expert-game-logic
  minReplicas: 4
  maxReplicas: 20
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 70
    - type: Pods
      pods:
        metric:
          name: game_sessions_active
        target:
          type: AverageValue
          averageValue: "50"  # 50 sessions per pod
  behavior:
    scaleUp:
      stabilizationWindowSeconds: 30
      policies:
        - type: Pods
          value: 4
          periodSeconds: 60
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
        - type: Pods
          value: 2
          periodSeconds: 120
---
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: expert-physics-hpa
  namespace: eustress-game
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: expert-physics
  minReplicas: 2
  maxReplicas: 10
  metrics:
    - type: Resource
      resource:
        name: cpu
        target:
          type: Utilization
          averageUtilization: 80  # Physics can run hotter
```

---

## Rust Implementation

```rust
// crates/services/src/moe/router.rs

use tonic::{Request, Response, Status};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicU32, AtomicBool};

/// Mixture of Experts router for game server workloads
pub struct MoeRouter {
    experts: Arc<RwLock<HashMap<ExpertType, ExpertPool>>>,
    metrics: MetricsClient,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum ExpertType {
    GameLogic,
    Physics,
    AiNpc,
    AssetServing,
    VoiceChat,
    Matchmaking,
    Analytics,
    Moderation,
}

/// Pool of expert instances for load balancing
pub struct ExpertPool {
    endpoints: Vec<ExpertEndpoint>,
    load_balancer: LoadBalancer,
}

#[derive(Clone)]
pub struct ExpertEndpoint {
    pub address: String,
    pub current_load: Arc<AtomicU32>,
    pub max_capacity: u32,
    pub healthy: Arc<AtomicBool>,
}

impl MoeRouter {
    /// Create router from Kubernetes service discovery
    pub async fn from_k8s(namespace: &str) -> Result<Self, MoeError> {
        let kube_client = kube::Client::try_default().await?;
        
        let mut experts = HashMap::new();
        
        // Discover expert services
        for expert_type in ExpertType::all() {
            let service_name = expert_type.service_name();
            let endpoints = discover_endpoints(&kube_client, namespace, &service_name).await?;
            
            experts.insert(expert_type, ExpertPool {
                endpoints,
                load_balancer: LoadBalancer::round_robin(),
            });
        }
        
        Ok(Self {
            experts: Arc::new(RwLock::new(experts)),
            metrics: MetricsClient::new(),
        })
    }
    
    /// Route request to appropriate expert
    pub async fn route(&self, request: GameRequest) -> Result<GameResponse, MoeError> {
        let expert_type = self.classify(&request);
        
        let experts = self.experts.read().await;
        let pool = experts.get(&expert_type)
            .ok_or(MoeError::NoExpertAvailable(expert_type))?;
        
        // Select best endpoint
        let endpoint = pool.load_balancer.select(&pool.endpoints)?;
        
        // Record metrics
        self.metrics.record_route(expert_type, &endpoint.address);
        
        // Forward request
        let response = endpoint.send(request).await?;
        
        Ok(response)
    }
    
    /// Classify request to determine expert type
    fn classify(&self, request: &GameRequest) -> ExpertType {
        match &request.payload {
            Payload::PlayerMove(_) | Payload::PlayerAction(_) => ExpertType::GameLogic,
            Payload::PhysicsStep(_) | Payload::Collision(_) => ExpertType::Physics,
            Payload::NpcBehavior(_) | Payload::Pathfind(_) => ExpertType::AiNpc,
            Payload::LoadAsset(_) | Payload::StreamChunk(_) => ExpertType::AssetServing,
            Payload::VoicePacket(_) => ExpertType::VoiceChat,
            Payload::FindMatch(_) | Payload::JoinLobby(_) => ExpertType::Matchmaking,
            Payload::LogEvent(_) | Payload::Telemetry(_) => ExpertType::Analytics,
            Payload::ChatMessage(_) | Payload::Report(_) => ExpertType::Moderation,
        }
    }
}

impl ExpertType {
    pub fn all() -> &'static [ExpertType] {
        &[
            ExpertType::GameLogic,
            ExpertType::Physics,
            ExpertType::AiNpc,
            ExpertType::AssetServing,
            ExpertType::VoiceChat,
            ExpertType::Matchmaking,
            ExpertType::Analytics,
            ExpertType::Moderation,
        ]
    }
    
    pub fn service_name(&self) -> String {
        match self {
            ExpertType::GameLogic => "expert-game-logic",
            ExpertType::Physics => "expert-physics",
            ExpertType::AiNpc => "expert-ai-npc",
            ExpertType::AssetServing => "expert-asset",
            ExpertType::VoiceChat => "expert-voice",
            ExpertType::Matchmaking => "expert-matchmaking",
            ExpertType::Analytics => "expert-analytics",
            ExpertType::Moderation => "expert-moderation",
        }.to_string()
    }
    
    /// Kubernetes resource requests
    pub fn k8s_resources(&self) -> K8sResources {
        match self {
            ExpertType::GameLogic => K8sResources {
                cpu_request: "500m",
                cpu_limit: "1000m",
                memory_request: "512Mi",
                memory_limit: "1Gi",
            },
            ExpertType::Physics => K8sResources {
                cpu_request: "1000m",
                cpu_limit: "2000m",
                memory_request: "256Mi",
                memory_limit: "512Mi",
            },
            ExpertType::AiNpc => K8sResources {
                cpu_request: "500m",
                cpu_limit: "2000m",
                memory_request: "1Gi",
                memory_limit: "4Gi",
            },
            ExpertType::AssetServing => K8sResources {
                cpu_request: "100m",
                cpu_limit: "500m",
                memory_request: "2Gi",
                memory_limit: "4Gi",
            },
            _ => K8sResources::default(),
        }
    }
}

/// Kubernetes service discovery
async fn discover_endpoints(
    client: &kube::Client,
    namespace: &str,
    service: &str,
) -> Result<Vec<ExpertEndpoint>, MoeError> {
    use k8s_openapi::api::core::v1::Endpoints;
    use kube::Api;
    
    let endpoints_api: Api<Endpoints> = Api::namespaced(client.clone(), namespace);
    let eps = endpoints_api.get(service).await?;
    
    let mut result = vec![];
    
    if let Some(subsets) = eps.subsets {
        for subset in subsets {
            if let (Some(addresses), Some(ports)) = (subset.addresses, subset.ports) {
                for addr in addresses {
                    if let Some(ip) = addr.ip {
                        for port in &ports {
                            if let Some(port_num) = port.port {
                                result.push(ExpertEndpoint {
                                    address: format!("{}:{}", ip, port_num),
                                    current_load: Arc::new(AtomicU32::new(0)),
                                    max_capacity: 100,
                                    healthy: Arc::new(AtomicBool::new(true)),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    
    Ok(result)
}

#[derive(Debug, Default)]
pub struct K8sResources {
    pub cpu_request: &'static str,
    pub cpu_limit: &'static str,
    pub memory_request: &'static str,
    pub memory_limit: &'static str,
}
```

---

## Roblox vs Eustress Comparison

### Architecture Overview

| Aspect | Roblox | Eustress |
|--------|--------|----------|
| **Architecture** | Private cloud, bare-metal → containerized | Cloud-native MoE on K8s |
| **Scale** | 18,000+ servers, 170,000+ containers | Elastic (Hetzner nodes) |
| **Orchestration** | HashiCorp Nomad | Kubernetes (k3s) |
| **Data Centers** | Self-owned, core + edge | Hetzner Cloud |
| **DAUs** | ~80 million (2024) | Target: 1M+ |
| **Workload Model** | Monolithic game servers | Specialized experts |

### Why Different Approaches?

| Decision | Roblox | Eustress |
|----------|--------|----------|
| **Orchestrator** | Nomad (bare-metal native, simpler) | K8s (cloud-native, HPA) |
| **Infrastructure** | CapEx (own hardware) | OpEx (cloud rental) |
| **Scaling** | Manual + Nomad jobs | Auto-scaling (HPA) |
| **Entry Cost** | $100M+ DC investment | $20/mo minimum |

### Cost Structure

#### Roblox (Estimated from public data)
```
Infrastructure spend: ~$500M+/year
DAUs: 80 million
Hours engaged: 68 billion/year
Cost per DAU: ~$6.25/year = $0.52/month
Cost per hour engaged: ~$0.007/hour
```

#### Eustress (MoE + K8s on Hetzner)
```
At 1M users scale:
Infrastructure: $1,600/mo (servers) + $75,000/mo (storage)
Cost per user: ~$0.08/month
Cost per hour (est. 10 hrs/user/mo): ~$0.008/hour
```

### Server Capacity

| Metric | Roblox | Eustress (MoE) |
|--------|--------|----------------|
| **Players per server** | ~100 per game instance | ~200-500 per node |
| **Server type** | Dedicated game instance | Shared expert pods |
| **Physics** | Per-instance | Dedicated Physics expert |
| **AI/NPC** | Per-instance | Dedicated AINPC expert |
| **Utilization** | ~40-60% (estimated) | ~80%+ (MoE routing) |

### Cost Per CCU

| Scale | Roblox (est.) | Eustress MoE |
|-------|---------------|--------------|
| 1,000 CCU | N/A (min scale too high) | $0.02/CCU/mo |
| 10,000 CCU | ~$0.05/CCU/mo | $0.007/CCU/mo |
| 100,000 CCU | ~$0.03/CCU/mo | $0.003/CCU/mo |
| 1,000,000 CCU | ~$0.02/CCU/mo | $0.002/CCU/mo |

### Key Takeaways

1. **Roblox chose Nomad** — bare-metal native, simpler for owned hardware
2. **Eustress uses K8s** — cloud-native, auto-scaling, no DC investment
3. **MoE advantage** — specialized experts achieve 80%+ utilization vs ~50% monolithic
4. **Cost parity** — Eustress matches Roblox per-CCU cost at 100K+ scale without owning DCs
5. **Trade-off** — Roblox has lower marginal cost at massive scale (CapEx amortized)

---

## Hetzner Pricing

| Node Type | Specs | Monthly | Best For |
|-----------|-------|---------|----------|
| CPX21 | 3 vCPU, 4GB | $4.50 | API, routing |
| CPX31 | 4 vCPU, 8GB | $7.50 | Game experts |
| CPX41 | 8 vCPU, 16GB | $14 | Physics-heavy |
| CCX33 | 8 vCPU, 32GB (dedicated) | $35 | AI/ML workloads |

---

## Recommended Cluster Configurations

### Small (1,000 users)
```
2× CPX31 (game)     = $15/mo
1× CPX21 (api)      = $4.50/mo
k3s (free)          = $0
Total               = $19.50/mo
```

### Medium (10,000 users)
```
4× CPX31 (game)     = $30/mo
2× CPX21 (api)      = $9/mo
1× CCX33 (ai)       = $35/mo
k3s (free)          = $0
Total               = $74/mo
```

### Large (100,000 users)
```
16× CPX31 (game)    = $120/mo
4× CPX21 (api)      = $18/mo
4× CCX33 (ai)       = $140/mo
Hetzner LB          = $6/mo
Total               = $284/mo
```

### Enterprise (1,000,000 users)
```
64× CPX31 (game)    = $480/mo
16× CPX21 (api)     = $72/mo
16× CCX33 (ai)      = $560/mo
4× Hetzner LB       = $24/mo
Total               = $1,136/mo
```

---

## Cost Comparison Summary

| Users | Homogeneous | MoE + K8s | Savings |
|-------|-------------|-----------|---------|
| 1,000 | $50/mo | $19.50/mo | 61% |
| 10,000 | $250/mo | $74/mo | 70% |
| 100,000 | $1,250/mo | $284/mo | 77% |
| 1,000,000 | $5,000/mo | $1,136/mo | 77% |

---

## Related Documentation

- [SUBSCRIPTIONS.md](../monetization/SUBSCRIPTIONS.md) — Cost analysis and profitability
- [S3 Storage](../../eustress/crates/common/src/assets/s3.rs) — Cloud storage implementation
