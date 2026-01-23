# Eustress Forge - Game Server Job Specification
# Nomad job for running Eustress game server instances

job "eustress-gameserver" {
  datacenters = ["dc1"]
  type        = "service"

  # Parameterized for dynamic spawning
  parameterized {
    payload       = "optional"
    meta_required = ["experience_id", "server_id"]
    meta_optional = ["max_players", "region"]
  }

  # Spread across availability zones
  spread {
    attribute = "${node.datacenter}"
    weight    = 100
  }

  # Prefer nodes with game-server class
  constraint {
    attribute = "${node.class}"
    value     = "game-server"
  }

  group "gameserver" {
    count = 1

    # Network configuration for QUIC
    network {
      mode = "host"
      
      port "quic" {
        to = 4433
      }
      
      port "http" {
        to = 8080
      }
    }

    # Restart policy for resilience
    restart {
      attempts = 3
      interval = "5m"
      delay    = "15s"
      mode     = "fail"
    }

    # Reschedule on node failure
    reschedule {
      attempts       = 5
      interval       = "1h"
      delay          = "5s"
      delay_function = "exponential"
      max_delay      = "1m"
      unlimited      = false
    }

    # Graceful shutdown
    shutdown_delay = "10s"

    # Service registration for discovery
    service {
      name = "eustress-gameserver"
      port = "quic"
      
      tags = [
        "eustress",
        "gameserver",
        "experience-${NOMAD_META_experience_id}",
      ]

      meta {
        experience_id = "${NOMAD_META_experience_id}"
        server_id     = "${NOMAD_META_server_id}"
        max_players   = "${NOMAD_META_max_players}"
      }

      check {
        type     = "http"
        port     = "http"
        path     = "/health"
        interval = "10s"
        timeout  = "3s"
      }
    }

    # HTTP API service
    service {
      name = "eustress-gameserver-api"
      port = "http"
      
      tags = ["api", "internal"]
    }

    task "server" {
      driver = "raw_exec"

      # Artifact: Download game server binary
      artifact {
        source      = "s3::https://s3.amazonaws.com/${NOMAD_DC}-eustress-forge-data/binaries/eustress-server-${NOMAD_META_version:-latest}"
        destination = "local/bin"
        mode        = "file"
      }

      # Configuration template
      template {
        data = <<EOF
# Eustress Game Server Configuration
[server]
experience_id = "{{ env "NOMAD_META_experience_id" }}"
server_id = "{{ env "NOMAD_META_server_id" }}"
max_players = {{ env "NOMAD_META_max_players" | default "100" }}

[network]
quic_port = {{ env "NOMAD_PORT_quic" }}
http_port = {{ env "NOMAD_PORT_http" }}
bind_address = "0.0.0.0"

[physics]
tick_rate = 120
physics_rate = 120

[consul]
address = "127.0.0.1:8500"
service_name = "eustress-gameserver"

[metrics]
prometheus_port = {{ env "NOMAD_PORT_http" }}
enabled = true
EOF
        destination = "local/config.toml"
      }

      config {
        command = "local/bin/eustress-server"
        args = [
          "--config", "local/config.toml",
        ]
      }

      # Resource allocation
      resources {
        cpu    = 2000  # 2 CPU cores
        memory = 4096  # 4 GB RAM
      }

      # Environment variables
      env {
        RUST_LOG           = "info,eustress=debug"
        RUST_BACKTRACE     = "1"
        EUSTRESS_DATACENTER = "${NOMAD_DC}"
        EUSTRESS_ALLOC_ID   = "${NOMAD_ALLOC_ID}"
      }

      # Logs
      logs {
        max_files     = 5
        max_file_size = 50
      }

      # Kill timeout for graceful shutdown
      kill_timeout = "30s"
    }
  }

  # Update strategy for rolling deployments
  update {
    max_parallel      = 1
    health_check      = "checks"
    min_healthy_time  = "30s"
    healthy_deadline  = "5m"
    progress_deadline = "10m"
    auto_revert       = true
    canary            = 0
  }
}
