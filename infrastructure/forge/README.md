# Eustress Forge Infrastructure

Terraform + Nomad deployment configuration for Eustress Forge multiplayer orchestration platform.

## Overview

This directory contains the infrastructure-as-code definitions for deploying Eustress Forge on HashiCorp Nomad with Terraform.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                          Infrastructure Layer                              │
├─────────────────────────────────────────────────────────────────────────┤
│  Terraform (IaC)                                                        │
│  ├── Hetzner Cloud provider                                              │
│  ├── Nomad cluster setup                                                 │
│  ├── Consul service mesh                                                  │
│  └── Network security groups                                             │
├─────────────────────────────────────────────────────────────────────────┤
│  Nomad Cluster (Orchestration)                                           │
│  ├── Control plane nodes                                                  │
│  ├── Game server workers                                                  │
│  ├── Physics simulation workers                                           │
│  └── AI inference workers                                                 │
├─────────────────────────────────────────────────────────────────────────┤
│  Consul (Service Mesh)                                                    │
│  ├── Service discovery                                                    │
│  ├── Health checking                                                       │
│  ├── Load balancing                                                       │
│  └── Configuration management                                             │
└─────────────────────────────────────────────────────────────────────────┘
```

## Directory Structure

```
infrastructure/forge/
├── terraform/              # Terraform configurations
│   ├── main.tf            # Main infrastructure
│   ├── variables.tf      # Input variables
│   ├── outputs.tf        # Output values
│   ├── nomad.tf          # Nomad cluster setup
│   ├── consul.tf         # Consul setup
│   └── security.tf       # Security groups
├── nomad/                 # Nomad job specifications
│   ├── gameserver.hcl    # Game server jobs
│   ├── physics.hcl      # Physics simulation jobs
│   └── ai.hcl           # AI inference jobs
├── consul/               # Consul configuration
│   ├── config.json      # Consul server config
│   └── services/        # Service definitions
└── scripts/              # Deployment scripts
    ├── deploy.sh        # Main deployment script
    ├── update.sh        # Rolling update script
    └── destroy.sh       # Cleanup script
```

## Quick Start

1. **Configure Terraform variables**:
   ```bash
   cp terraform/terraform.tfvars.example terraform/terraform.tfvars
   # Edit terraform/terraform.tfvars with your values
   ```

2. **Deploy infrastructure**:
   ```bash
   ./scripts/deploy.sh
   ```

3. **Deploy services**:
   ```bash
   ./scripts/update.sh
   ```

## Cost Estimates

| Scale | Monthly Cost | Servers | Players |
|-------|--------------|---------|---------|
| Small (1K users) | $19.50/mo | 3 nodes | 1,000 |
| Medium (10K users) | $74/mo | 7 nodes | 10,000 |
| Large (100K users) | $284/mo | 24 nodes | 100,000 |
| Enterprise (1M users) | $1,136/mo | 96 nodes | 1,000,000 |

## Requirements

- Terraform >= 1.5
- Nomad >= 1.7
- Consul >= 1.16
- Hetzner Cloud account
- SSH key for access

## Security

- All nodes use private networking
- SSH access restricted to bastion hosts
- Nomad ACL tokens enabled
- Consul encryption enabled
- Regular security updates

## Monitoring

- Prometheus metrics collection
- Grafana dashboards
- Alertmanager notifications
- Log aggregation with Loki

## Backup & Recovery

- Automated etcd backups
- Database snapshots
- Configuration versioning
- Disaster recovery procedures