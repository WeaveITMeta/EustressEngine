#!/bin/bash
# Eustress Forge - Nomad Node Bootstrap Script
# This script configures Nomad and Consul on EC2 instances

set -euo pipefail

# ============================================================================
# Variables from Terraform
# ============================================================================

IS_SERVER="${is_server}"
DATACENTER="${datacenter}"
CONSUL_SERVERS="${consul_servers}"
NOMAD_SERVERS="${nomad_servers}"
NODE_CLASS="${node_class}"
META_TAGS='${meta_tags}'

# ============================================================================
# System Setup
# ============================================================================

echo "=== Eustress Forge Node Bootstrap ==="
echo "Is Server: $IS_SERVER"
echo "Datacenter: $DATACENTER"

# Wait for cloud-init to complete
cloud-init status --wait || true

# Get instance metadata
INSTANCE_ID=$(curl -s http://169.254.169.254/latest/meta-data/instance-id)
PRIVATE_IP=$(curl -s http://169.254.169.254/latest/meta-data/local-ipv4)
AVAILABILITY_ZONE=$(curl -s http://169.254.169.254/latest/meta-data/placement/availability-zone)

echo "Instance ID: $INSTANCE_ID"
echo "Private IP: $PRIVATE_IP"
echo "AZ: $AVAILABILITY_ZONE"

# ============================================================================
# Consul Configuration
# ============================================================================

cat > /etc/consul.d/consul.hcl <<EOF
datacenter = "$DATACENTER"
data_dir = "/opt/consul/data"
log_level = "INFO"
node_name = "$INSTANCE_ID"
bind_addr = "$PRIVATE_IP"
client_addr = "0.0.0.0"

# Service mesh
connect {
  enabled = true
}

# Performance tuning
performance {
  raft_multiplier = 1
}

# Retry join via AWS tag discovery
retry_join = ["provider=aws tag_key=ConsulAutoJoin tag_value=true"]

# TLS (uncomment for production)
# verify_incoming = true
# verify_outgoing = true
# verify_server_hostname = true
# ca_file = "/etc/consul.d/certs/ca.pem"
# cert_file = "/etc/consul.d/certs/server.pem"
# key_file = "/etc/consul.d/certs/server-key.pem"
EOF

if [ "$IS_SERVER" = "true" ]; then
  cat >> /etc/consul.d/consul.hcl <<EOF

# Server-specific config
server = true
bootstrap_expect = 3
ui_config {
  enabled = true
}
EOF
fi

# Start Consul
systemctl enable consul
systemctl start consul

# Wait for Consul to be ready
echo "Waiting for Consul..."
until consul members &>/dev/null; do
  sleep 2
done
echo "Consul is ready"

# ============================================================================
# Nomad Configuration
# ============================================================================

cat > /etc/nomad.d/nomad.hcl <<EOF
datacenter = "$DATACENTER"
data_dir = "/opt/nomad/data"
log_level = "INFO"
name = "$INSTANCE_ID"
bind_addr = "0.0.0.0"

advertise {
  http = "$PRIVATE_IP"
  rpc  = "$PRIVATE_IP"
  serf = "$PRIVATE_IP"
}

# Consul integration
consul {
  address = "127.0.0.1:8500"
  auto_advertise = true
  server_auto_join = true
  client_auto_join = true
}

# Telemetry for Prometheus
telemetry {
  prometheus_metrics = true
  publish_allocation_metrics = true
  publish_node_metrics = true
}
EOF

if [ "$IS_SERVER" = "true" ]; then
  cat >> /etc/nomad.d/nomad.hcl <<EOF

# Server configuration
server {
  enabled = true
  bootstrap_expect = 3
  
  # Raft performance
  raft_protocol = 3
  
  # Enable job garbage collection
  job_gc_interval = "5m"
  job_gc_threshold = "4h"
  
  # Heartbeat grace period for spot interruptions
  heartbeat_grace = "30s"
}
EOF
else
  # Client configuration
  cat >> /etc/nomad.d/nomad.hcl <<EOF

# Client configuration
client {
  enabled = true
  node_class = "$NODE_CLASS"
  
  # Meta tags for job constraints
  meta {
    availability_zone = "$AVAILABILITY_ZONE"
    instance_id = "$INSTANCE_ID"
  }
  
  # Resource fingerprinting
  cpu_total_compute = 0  # Auto-detect
  
  # Host volumes for game data
  host_volume "game-data" {
    path = "/opt/eustress/data"
    read_only = false
  }
  
  # Network configuration for QUIC
  network_interface = "eth0"
  
  # Drain on shutdown for graceful termination
  drain_on_shutdown {
    deadline = "1m"
    force = false
    ignore_system_jobs = false
  }
}

# Plugin configuration
plugin "docker" {
  config {
    allow_privileged = false
    volumes {
      enabled = true
    }
    gc {
      image = true
      image_delay = "3m"
      container = true
    }
  }
}

# Raw exec for native Rust binaries
plugin "raw_exec" {
  config {
    enabled = true
  }
}
EOF
fi

# Create game data directory
mkdir -p /opt/eustress/data
chown -R nomad:nomad /opt/eustress

# Start Nomad
systemctl enable nomad
systemctl start nomad

# Wait for Nomad to be ready
echo "Waiting for Nomad..."
until nomad node status &>/dev/null; do
  sleep 2
done
echo "Nomad is ready"

# ============================================================================
# Spot Interruption Handler (for clients)
# ============================================================================

if [ "$IS_SERVER" = "false" ]; then
  cat > /usr/local/bin/spot-interruption-handler.sh <<'HANDLER'
#!/bin/bash
# Handle EC2 spot interruption by draining the Nomad node

METADATA_TOKEN=$(curl -X PUT "http://169.254.169.254/latest/api/token" -H "X-aws-ec2-metadata-token-ttl-seconds: 21600")

while true; do
  TERMINATION_TIME=$(curl -s -H "X-aws-ec2-metadata-token: $METADATA_TOKEN" \
    http://169.254.169.254/latest/meta-data/spot/termination-time 2>/dev/null || echo "")
  
  if [ -n "$TERMINATION_TIME" ] && [ "$TERMINATION_TIME" != "404 - Not Found" ]; then
    echo "Spot interruption detected at $TERMINATION_TIME"
    
    # Drain the node gracefully
    NODE_ID=$(nomad node status -self -t '{{.ID}}')
    nomad node drain -enable -deadline 1m -yes "$NODE_ID"
    
    # Notify Forge Orchestrator (if configured)
    # curl -X POST http://forge-orchestrator:8080/api/v1/spot-interruption -d "{\"node_id\": \"$NODE_ID\"}"
    
    exit 0
  fi
  
  sleep 5
done
HANDLER

  chmod +x /usr/local/bin/spot-interruption-handler.sh

  cat > /etc/systemd/system/spot-interruption-handler.service <<EOF
[Unit]
Description=EC2 Spot Interruption Handler
After=nomad.service

[Service]
Type=simple
ExecStart=/usr/local/bin/spot-interruption-handler.sh
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

  systemctl enable spot-interruption-handler
  systemctl start spot-interruption-handler
fi

echo "=== Eustress Forge Node Bootstrap Complete ==="
