# Eustress Forge - AI Inference Job Specification
# NPC behavior, pathfinding, and MoE AI sharding

job "eustress-ai" {
  datacenters = ["dc1"]
  type        = "service"

  # Parameterized for dynamic spawning
  parameterized {
    payload       = "optional"
    meta_required = ["world_id"]
    meta_optional = ["model_id", "shard_id"]
  }

  # Prefer GPU nodes if available, fallback to CPU
  constraint {
    attribute = "${node.class}"
    operator  = "set_contains_any"
    value     = "ai-worker,game-server"
  }

  group "ai-inference" {
    count = 1

    network {
      mode = "host"
      
      port "grpc" {
        to = 50052
      }
      
      port "metrics" {
        to = 9091
      }
    }

    restart {
      attempts = 3
      interval = "5m"
      delay    = "15s"
      mode     = "fail"
    }

    service {
      name = "eustress-ai"
      port = "grpc"
      
      tags = [
        "eustress",
        "ai",
        "world-${NOMAD_META_world_id}",
        "shard-${NOMAD_META_shard_id}",
      ]

      meta {
        world_id = "${NOMAD_META_world_id}"
        model_id = "${NOMAD_META_model_id}"
        shard_id = "${NOMAD_META_shard_id}"
      }

      check {
        type     = "grpc"
        port     = "grpc"
        interval = "15s"
        timeout  = "5s"
      }
    }

    task "ai-worker" {
      driver = "raw_exec"

      artifact {
        source      = "s3::https://s3.amazonaws.com/${NOMAD_DC}-eustress-forge-data/binaries/eustress-ai-${NOMAD_META_version:-latest}"
        destination = "local/bin"
        mode        = "file"
      }

      # Download AI model if specified
      dynamic "artifact" {
        for_each = length(regexall(".", "${NOMAD_META_model_id}")) > 0 ? [1] : []
        content {
          source      = "s3::https://s3.amazonaws.com/${NOMAD_DC}-eustress-forge-data/models/${NOMAD_META_model_id}"
          destination = "local/models"
        }
      }

      template {
        data = <<EOF
# Eustress AI Server Configuration
[ai]
world_id = "{{ env "NOMAD_META_world_id" }}"
shard_id = "{{ env "NOMAD_META_shard_id" | default "0" }}"
model_path = "local/models"

[network]
grpc_port = {{ env "NOMAD_PORT_grpc" }}
bind_address = "0.0.0.0"

[behavior]
# NPC behavior tree settings
max_npcs_per_shard = 1000
update_rate_hz = 30
pathfinding_threads = 4

[moe]
# Mixture of Experts sharding
enabled = true
total_shards = 8
shard_id = {{ env "NOMAD_META_shard_id" | default "0" }}

[consul]
address = "127.0.0.1:8500"
service_name = "eustress-ai"
EOF
        destination = "local/config.toml"
      }

      config {
        command = "local/bin/eustress-ai"
        args = [
          "--config", "local/config.toml",
        ]
      }

      resources {
        cpu    = 2000  # 2 CPU cores
        memory = 4096  # 4 GB RAM
        
        # GPU device if available (uncomment for GPU nodes)
        # device "nvidia/gpu" {
        #   count = 1
        # }
      }

      env {
        RUST_LOG       = "info,eustress_ai=debug"
        RUST_BACKTRACE = "1"
        # CUDA settings for GPU inference
        # CUDA_VISIBLE_DEVICES = "0"
      }

      logs {
        max_files     = 5
        max_file_size = 50
      }

      kill_timeout = "20s"
    }
  }

  update {
    max_parallel      = 2
    health_check      = "checks"
    min_healthy_time  = "30s"
    healthy_deadline  = "5m"
    progress_deadline = "10m"
    auto_revert       = true
  }
}
