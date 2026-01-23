# Eustress Forge Infrastructure

**Rust-native multiplayer orchestration platform built on HashiCorp Nomad.**

Eustress Forge replaces the deprecated Kubernetes/Agones architecture with a more efficient, lower-overhead solution achieving **80-90% cost savings** at scale.

## Why Forge?

| Metric | K8s/Agones (Old) | Eustress Forge (New) |
|--------|------------------|----------------------|
| Cluster overhead | 3-7% waste | <0.5% waste |
| Scaling latency | Seconds-minutes | Milliseconds |
| Cost at scale | Baseline | 70-90% savings |
| SDK complexity | CRD/Controllers | Pure Rust |

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Eustress Forge                                   │
├─────────────────────────────────────────────────────────────────────────┤
│  Control Plane (Rust)                                                    │
│  ├── ForgeController: Orchestrates game server lifecycle                 │
│  ├── SessionManager: Tracks active sessions and player routing           │
│  ├── ScalingEngine: Auto-scales based on demand metrics                  │
│  └── HealthMonitor: Monitors server health and triggers failover         │
├─────────────────────────────────────────────────────────────────────────┤
│  Data Plane (Nomad)                                                      │
│  ├── GameServer jobs: Actual game instances (QUIC/UDP)                   │
│  ├── PhysicsServer jobs: Dedicated physics simulation (Avian 3D)         │
│  └── AIServer jobs: NPC behavior and MoE sharding                        │
├─────────────────────────────────────────────────────────────────────────┤
│  Service Mesh (Consul)                                                   │
│  ├── Service discovery for game servers                                  │
│  ├── Health checking and load balancing                                  │
│  └── Configuration management                                            │
├─────────────────────────────────────────────────────────────────────────┤
│  Infrastructure (Terraform + AWS)                                        │
│  ├── VPC with multi-AZ private/public subnets                            │
│  ├── Nomad servers (Reserved Instances for reliability)                  │
│  ├── Nomad clients (90%+ Spot for cost savings)                          │
│  └── Auto Scaling with predictive scaling                                │
└─────────────────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
infrastructure/forge/
├── terraform/                    # AWS infrastructure
│   ├── main.tf                  # VPC, ASGs, IAM, S3
│   ├── variables.tf             # Configuration variables
│   ├── outputs.tf               # Exported values for Forge SDK
│   ├── terraform.tfvars.example # Example configuration
│   └── modules/
│       └── nomad-cluster/       # Reusable Nomad ASG module
│           ├── main.tf
│           ├── variables.tf
│           └── templates/
│               └── user_data.sh.tpl
├── nomad/                        # Job specifications
│   ├── forge-orchestrator.hcl  # Control plane (system job)
│   ├── gameserver.hcl          # Game server template (parameterized)
│   ├── physics.hcl             # Physics simulation template
│   └── ai.hcl                  # AI/NPC inference template
├── consul/                       # Service mesh config (TODO)
└── scripts/
    └── deploy.sh                # One-click deployment
```

## Quick Start

### Prerequisites

- **AWS Account** with IAM credentials for Terraform
- **Terraform >= 1.5** installed
- **Packer** (optional) for building custom AMIs
- **Nomad CLI** (optional) for job management

### 1. Build AMI (Recommended)

Build a custom AMI with Nomad, Consul, and Eustress binaries pre-installed:

```bash
# Use HashiCorp's Packer templates or build custom
packer build packer/nomad-consul.pkr.hcl
```

### 2. Configure Terraform

```bash
cd infrastructure/forge/terraform

# Copy example configuration
cp terraform.tfvars.example terraform.tfvars

# Edit with your values:
# - nomad_ami_id: Your custom AMI or HashiCorp's
# - ssh_key_name: Your EC2 key pair
# - environment: development/staging/production
```

### 3. Deploy Infrastructure

```bash
# Initialize Terraform
terraform init

# Preview changes
terraform plan

# Deploy (takes ~10-15 minutes)
terraform apply
```

### 4. Deploy Nomad Jobs

```bash
# Get Nomad address from outputs
export NOMAD_ADDR=$(terraform output -raw nomad_http_endpoint)

# Deploy Forge Orchestrator
nomad job run ../nomad/forge-orchestrator.hcl

# Register job templates
nomad job run ../nomad/gameserver.hcl
nomad job run ../nomad/physics.hcl
nomad job run ../nomad/ai.hcl
```

### 5. Spawn Game Servers

Use the Forge SDK or dispatch jobs directly:

```bash
# Dispatch a game server instance
nomad job dispatch eustress-gameserver \
  -meta experience_id=my-game \
  -meta server_id=server-001 \
  -meta max_players=100
```

## Cost Optimization Strategy

### Compute Allocation

| Component | Instance Type | Pricing | Why |
|-----------|---------------|---------|-----|
| Nomad Servers | m6i.large | Reserved/Savings Plan | Stable control plane |
| Game Servers | c7i.2xlarge | 90%+ Spot | Massive savings, graceful drain |
| Physics Workers | c7i.4xlarge | Spot | CPU-intensive, interruptible |
| AI Workers | g5.xlarge | Spot | GPU inference, batch-friendly |

### Spot Instance Strategy

- **Mixed instance types** for availability (c7i, c6i, c6a, c5, m6i)
- **Capacity-optimized allocation** for lowest interruption rates
- **Graceful drain on interruption** (2-minute warning handled)
- **On-demand base capacity** (2 instances) for guaranteed availability

### Estimated Monthly Costs (AWS us-east-1)

| Scale | Spot Clients | On-Demand Base | Servers | Total | Per-Player |
|-------|--------------|----------------|---------|-------|------------|
| Dev (100 CCU) | 3 × $0.08/hr | 2 × $0.34/hr | 3 × $0.10/hr | ~$150/mo | $1.50 |
| Small (1K CCU) | 10 × $0.08/hr | 2 × $0.34/hr | 3 × $0.10/hr | ~$300/mo | $0.30 |
| Medium (10K CCU) | 50 × $0.08/hr | 2 × $0.34/hr | 5 × $0.10/hr | ~$800/mo | $0.08 |
| Large (100K CCU) | 200 × $0.08/hr | 5 × $0.34/hr | 5 × $0.10/hr | ~$2,500/mo | $0.025 |
| Enterprise (1M CCU) | 1000 × $0.08/hr | 10 × $0.34/hr | 7 × $0.10/hr | ~$10,000/mo | $0.01 |

*CCU = Concurrent Users. Assumes 100 players per game server instance.*

## Networking

### Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 4433 | UDP/QUIC | Game traffic (TLS 1.3) |
| 4646-4648 | TCP | Nomad HTTP/RPC/Serf |
| 8300-8302 | TCP/UDP | Consul RPC/Serf |
| 8500 | TCP | Consul HTTP API |
| 8080 | TCP | Forge Orchestrator API |
| 50050 | TCP | Forge Orchestrator gRPC |
| 9100 | TCP | Prometheus metrics |

### Security

- **Private subnets** for all Nomad nodes
- **NAT Gateway** for outbound internet access
- **Security groups** with least-privilege rules
- **TLS everywhere** (QUIC, Nomad ACL, Consul encryption)
- **VPC Flow Logs** for security monitoring

## Monitoring

### Prometheus Metrics

All components expose Prometheus metrics:

```yaml
# prometheus.yml scrape configs
- job_name: 'nomad'
  consul_sd_configs:
    - server: 'consul:8500'
      services: ['nomad-client', 'nomad']

- job_name: 'forge'
  consul_sd_configs:
    - server: 'consul:8500'
      services: ['forge-orchestrator', 'eustress-gameserver']
```

### Key Metrics

- `nomad_client_allocations_running` - Active game servers
- `forge_sessions_active` - Player sessions
- `forge_scaling_events_total` - Auto-scaling activity
- `eustress_tick_duration_seconds` - Game loop performance

## Multi-Region Deployment

For global low-latency (<20ms), deploy Nomad clusters in multiple regions:

```hcl
# terraform/main.tf
module "forge_us_east" {
  source = "./modules/forge-region"
  region = "us-east-1"
}

module "forge_eu_west" {
  source = "./modules/forge-region"
  region = "eu-west-1"
}

module "forge_ap_northeast" {
  source = "./modules/forge-region"
  region = "ap-northeast-1"
}
```

Use **Consul federation** for cross-region service discovery and **Global Accelerator** for anycast routing.

## Phased Rollout

### Phase 1: MVP (2026 Q1)
- Single-region AWS deployment
- Spot-heavy client nodes
- Basic auto-scaling

### Phase 2: Production (2026 Q2)
- Multi-region with Consul federation
- Advanced metrics and alerting
- Disaster recovery procedures

### Phase 3: Hybrid (2026 Q3+)
- On-premises/colocation for core capacity
- Cloud burst for viral spikes
- 90%+ cost optimization

## Troubleshooting

### Nomad Cluster Not Forming

```bash
# Check Consul members
consul members

# Check Nomad server logs
journalctl -u nomad -f

# Verify security groups allow Serf traffic (4648 TCP/UDP)
```

### Game Servers Not Starting

```bash
# Check job status
nomad job status eustress-gameserver

# Check allocation logs
nomad alloc logs <alloc-id>

# Verify artifact download (S3 permissions)
```

### Spot Interruptions

Spot interruptions are handled gracefully:

1. EC2 sends 2-minute warning
2. `spot-interruption-handler.sh` detects it
3. Nomad node is drained
4. Players are migrated to other servers
5. New capacity is provisioned

## Integration with Eustress Engine

The Forge SDK (`eustress-forge` crate) provides a Rust API:

```rust
use eustress_forge::prelude::*;

#[tokio::main]
async fn main() -> Result<(), ForgeError> {
    let forge = ForgeController::new(ForgeConfig::from_env()?).await?;
    
    // Spawn a game server
    let server = forge.spawn_server(ServerSpec {
        experience_id: "my-experience".into(),
        region: Region::UsEast,
        max_players: 100,
    }).await?;
    
    // Route player to server
    forge.route_player(player_id, server.id()).await?;
    
    Ok(())
}
```

## References

- [HashiCorp Nomad on AWS](https://developer.hashicorp.com/nomad/tutorials/enterprise/production-deployment-guide-vm-with-consul)
- [Terraform AWS Modules](https://registry.terraform.io/namespaces/terraform-aws-modules)
- [Eustress Forge SDK](../../eustress/crates/forge/)
- [Eustress Networking](../../eustress/crates/common/eustress-networking/)