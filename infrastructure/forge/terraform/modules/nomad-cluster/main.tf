# Eustress Forge - Nomad Cluster Module
# Reusable module for Nomad server/client ASGs

terraform {
  required_version = ">= 1.5"
}

# ============================================================================
# Launch Template
# ============================================================================

resource "aws_launch_template" "this" {
  name_prefix   = "${var.name}-"
  image_id      = var.ami_id
  instance_type = var.instance_type
  key_name      = var.key_name

  iam_instance_profile {
    name = var.iam_instance_profile
  }

  network_interfaces {
    associate_public_ip_address = false
    security_groups             = var.security_group_ids
  }

  block_device_mappings {
    device_name = "/dev/xvda"
    ebs {
      volume_size           = var.root_volume_size
      volume_type           = "gp3"
      encrypted             = true
      delete_on_termination = true
    }
  }

  # Data volume for Nomad/Consul data
  dynamic "block_device_mappings" {
    for_each = var.data_volume_size > 0 ? [1] : []
    content {
      device_name = "/dev/xvdb"
      ebs {
        volume_size           = var.data_volume_size
        volume_type           = "gp3"
        encrypted             = true
        delete_on_termination = true
      }
    }
  }

  user_data = base64encode(templatefile("${path.module}/templates/user_data.sh.tpl", {
    is_server       = var.is_server
    datacenter      = var.datacenter
    consul_servers  = join(",", var.consul_servers)
    nomad_servers   = join(",", var.nomad_servers)
    node_class      = var.node_class
    meta_tags       = jsonencode(var.meta_tags)
  }))

  tag_specifications {
    resource_type = "instance"
    tags = merge(var.tags, {
      Name = var.name
    })
  }

  tag_specifications {
    resource_type = "volume"
    tags = merge(var.tags, {
      Name = "${var.name}-volume"
    })
  }

  lifecycle {
    create_before_destroy = true
  }
}

# ============================================================================
# Auto Scaling Group
# ============================================================================

resource "aws_autoscaling_group" "this" {
  name                = var.name
  desired_capacity    = var.desired_capacity
  min_size            = var.min_size
  max_size            = var.max_size
  vpc_zone_identifier = var.subnet_ids

  # Health checks
  health_check_type         = "EC2"
  health_check_grace_period = 300

  # Instance refresh for rolling updates
  instance_refresh {
    strategy = "Rolling"
    preferences {
      min_healthy_percentage = 50
    }
  }

  # Termination policies for graceful shutdown
  termination_policies = ["OldestInstance", "Default"]

  # Enable capacity rebalancing for spot
  capacity_rebalance = var.use_spot

  dynamic "mixed_instances_policy" {
    for_each = var.use_spot ? [1] : []
    content {
      instances_distribution {
        on_demand_base_capacity                  = var.mixed_instances_policy.instances_distribution.on_demand_base_capacity
        on_demand_percentage_above_base_capacity = var.mixed_instances_policy.instances_distribution.on_demand_percentage_above_base_capacity
        spot_allocation_strategy                 = var.mixed_instances_policy.instances_distribution.spot_allocation_strategy
      }

      launch_template {
        launch_template_specification {
          launch_template_id = aws_launch_template.this.id
          version            = "$Latest"
        }

        dynamic "override" {
          for_each = var.mixed_instances_policy.override
          content {
            instance_type     = override.value.instance_type
            weighted_capacity = lookup(override.value, "weighted_capacity", 1)
          }
        }
      }
    }
  }

  dynamic "launch_template" {
    for_each = var.use_spot ? [] : [1]
    content {
      id      = aws_launch_template.this.id
      version = "$Latest"
    }
  }

  dynamic "tag" {
    for_each = var.tags
    content {
      key                 = tag.key
      value               = tag.value
      propagate_at_launch = true
    }
  }

  tag {
    key                 = "Name"
    value               = var.name
    propagate_at_launch = true
  }

  lifecycle {
    create_before_destroy = true
    ignore_changes        = [desired_capacity]
  }
}

# ============================================================================
# Outputs
# ============================================================================

output "asg_name" {
  description = "Auto Scaling Group name"
  value       = aws_autoscaling_group.this.name
}

output "asg_arn" {
  description = "Auto Scaling Group ARN"
  value       = aws_autoscaling_group.this.arn
}

output "launch_template_id" {
  description = "Launch template ID"
  value       = aws_launch_template.this.id
}

# Note: Private IPs are populated after instances launch
# Use data source or Consul for discovery
output "private_ips" {
  description = "Placeholder for private IPs (use Consul for discovery)"
  value       = []
}
