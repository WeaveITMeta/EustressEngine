# Eustress Forge - Physics Server Job Specification
# Dedicated physics simulation workers for high-fidelity simulations

job "eustress-physics" {
  datacenters = ["dc1"]
  type        = "service"

  # Parameterized for dynamic spawning
  parameterized {
    payload       = "optional"
    meta_required = ["world_id"]
    meta_optional = ["tick_rate", "region"]
  }

  # Prefer compute-optimized nodes
  constraint {
    attribute = "${node.class}"
    value     = "game-server"
  }

  # Spread across AZs
  spread {
    attribute = "${attr.platform.aws.placement.availability-zone}"
    weight    = 100
  }

  group "physics" {
    count = 1

    network {
      mode = "host"
      
      port "grpc" {
        to = 50051
      }
      
      port "metrics" {
        to = 9090
      }
    }

    restart {
      attempts = 3
      interval = "5m"
      delay    = "10s"
      mode     = "fail"
    }

    service {
      name = "eustress-physics"
      port = "grpc"
      
      tags = [
        "eustress",
        "physics",
        "world-${NOMAD_META_world_id}",
      ]

      meta {
        world_id  = "${NOMAD_META_world_id}"
        tick_rate = "${NOMAD_META_tick_rate}"
      }

      check {
        type     = "grpc"
        port     = "grpc"
        interval = "10s"
        timeout  = "3s"
      }
    }

    task "physics-worker" {
      driver = "raw_exec"

      artifact {
        source      = "s3::https://s3.amazonaws.com/${NOMAD_DC}-eustress-forge-data/binaries/eustress-physics-${NOMAD_META_version:-latest}"
        destination = "local/bin"
        mode        = "file"
      }

      template {
        data = <<EOF
# Eustress Physics Server Configuration
[physics]
world_id = "{{ env "NOMAD_META_world_id" }}"
tick_rate = {{ env "NOMAD_META_tick_rate" | default "120" }}
substeps = 2

[network]
grpc_port = {{ env "NOMAD_PORT_grpc" }}
bind_address = "0.0.0.0"

[avian]
# Avian 3D physics settings
gravity = [0.0, -9.80665, 0.0]
solver_iterations = 4
substeps = 2

[consul]
address = "127.0.0.1:8500"
service_name = "eustress-physics"
EOF
        destination = "local/config.toml"
      }

      config {
        command = "local/bin/eustress-physics"
        args = [
          "--config", "local/config.toml",
        ]
      }

      resources {
        cpu    = 4000  # 4 CPU cores for physics
        memory = 8192  # 8 GB RAM
      }

      env {
        RUST_LOG       = "info,eustress_physics=debug,avian3d=info"
        RUST_BACKTRACE = "1"
      }

      logs {
        max_files     = 5
        max_file_size = 50
      }

      kill_timeout = "15s"
    }
  }

  update {
    max_parallel      = 1
    health_check      = "checks"
    min_healthy_time  = "20s"
    healthy_deadline  = "3m"
    progress_deadline = "5m"
    auto_revert       = true
  }
}
