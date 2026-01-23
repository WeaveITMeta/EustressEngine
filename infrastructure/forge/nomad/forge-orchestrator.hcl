# Eustress Forge - Orchestrator System Job
# The Rust control plane that manages game server lifecycle

job "forge-orchestrator" {
  datacenters = ["dc1"]
  type        = "system"  # Runs on all server nodes

  # Only run on Nomad server nodes
  constraint {
    attribute = "${node.class}"
    operator  = "set_contains_any"
    value     = "nomad-server,control-plane"
  }

  group "orchestrator" {
    count = 1

    network {
      mode = "host"
      
      port "http" {
        static = 8080
        to     = 8080
      }
      
      port "grpc" {
        static = 50050
        to     = 50050
      }
      
      port "metrics" {
        static = 9100
        to     = 9100
      }
    }

    restart {
      attempts = 10
      interval = "5m"
      delay    = "10s"
      mode     = "delay"
    }

    # Leader election via Consul
    service {
      name = "forge-orchestrator"
      port = "http"
      
      tags = [
        "eustress",
        "forge",
        "orchestrator",
        "traefik.enable=true",
        "traefik.http.routers.forge.rule=PathPrefix(`/api/v1`)",
      ]

      check {
        type     = "http"
        port     = "http"
        path     = "/health"
        interval = "10s"
        timeout  = "3s"
      }

      # Leader election
      check {
        type     = "http"
        port     = "http"
        path     = "/health/leader"
        interval = "5s"
        timeout  = "2s"
      }
    }

    service {
      name = "forge-orchestrator-grpc"
      port = "grpc"
      
      tags = ["grpc", "internal"]

      check {
        type     = "grpc"
        port     = "grpc"
        interval = "10s"
        timeout  = "3s"
      }
    }

    task "orchestrator" {
      driver = "raw_exec"

      artifact {
        source      = "s3::https://s3.amazonaws.com/${NOMAD_DC}-eustress-forge-data/binaries/forge-orchestrator-${NOMAD_META_version:-latest}"
        destination = "local/bin"
        mode        = "file"
      }

      template {
        data = <<EOF
# Forge Orchestrator Configuration
[orchestrator]
node_id = "{{ env "NOMAD_ALLOC_ID" }}"
datacenter = "{{ env "NOMAD_DC" }}"

[api]
http_port = {{ env "NOMAD_PORT_http" }}
grpc_port = {{ env "NOMAD_PORT_grpc" }}
metrics_port = {{ env "NOMAD_PORT_metrics" }}
bind_address = "0.0.0.0"

[nomad]
address = "http://127.0.0.1:4646"
# ACL token from Vault (uncomment for production)
# token = "{{ with secret "nomad/creds/forge-orchestrator" }}{{ .Data.secret_id }}{{ end }}"

[consul]
address = "127.0.0.1:8500"
service_name = "forge-orchestrator"

[scaling]
# Auto-scaling configuration
min_game_servers = 0
max_game_servers = 10000
scale_up_threshold = 0.7    # 70% capacity triggers scale up
scale_down_threshold = 0.3  # 30% capacity triggers scale down
scale_cooldown_secs = 60

[session]
# Session management
max_players_per_server = 100
session_timeout_secs = 3600
matchmaking_enabled = true

[health]
# Health monitoring
check_interval_secs = 10
unhealthy_threshold = 3
drain_timeout_secs = 60

[metrics]
prometheus_enabled = true
push_gateway = ""
EOF
        destination = "local/config.toml"
      }

      config {
        command = "local/bin/forge-orchestrator"
        args = [
          "--config", "local/config.toml",
        ]
      }

      resources {
        cpu    = 1000  # 1 CPU core
        memory = 2048  # 2 GB RAM
      }

      env {
        RUST_LOG       = "info,forge_orchestrator=debug"
        RUST_BACKTRACE = "1"
      }

      logs {
        max_files     = 10
        max_file_size = 100
      }

      kill_timeout = "30s"
    }
  }

  update {
    max_parallel     = 1
    health_check     = "checks"
    min_healthy_time = "30s"
    healthy_deadline = "5m"
    stagger          = "30s"
    auto_revert      = true
  }
}
