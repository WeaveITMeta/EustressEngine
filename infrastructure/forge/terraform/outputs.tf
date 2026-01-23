# Eustress Forge - Terraform Outputs
# Exported values for integration with Forge Orchestrator

# ============================================================================
# VPC
# ============================================================================

output "vpc_id" {
  description = "VPC ID"
  value       = module.vpc.vpc_id
}

output "private_subnet_ids" {
  description = "Private subnet IDs"
  value       = module.vpc.private_subnets
}

output "public_subnet_ids" {
  description = "Public subnet IDs"
  value       = module.vpc.public_subnets
}

# ============================================================================
# Nomad Cluster
# ============================================================================

output "nomad_server_ips" {
  description = "Private IPs of Nomad server nodes"
  value       = module.nomad_servers.private_ips
}

output "nomad_server_asg_name" {
  description = "Nomad server ASG name"
  value       = module.nomad_servers.asg_name
}

output "nomad_client_asg_name" {
  description = "Nomad client ASG name"
  value       = module.nomad_clients.asg_name
}

output "nomad_http_endpoint" {
  description = "Nomad HTTP API endpoint (internal)"
  value       = "http://${module.nomad_servers.private_ips[0]}:4646"
}

# ============================================================================
# Security Groups
# ============================================================================

output "nomad_server_sg_id" {
  description = "Nomad server security group ID"
  value       = module.nomad_server_sg.security_group_id
}

output "nomad_client_sg_id" {
  description = "Nomad client security group ID"
  value       = module.nomad_client_sg.security_group_id
}

output "consul_sg_id" {
  description = "Consul security group ID"
  value       = module.consul_sg.security_group_id
}

# ============================================================================
# Storage
# ============================================================================

output "forge_data_bucket" {
  description = "S3 bucket for game data"
  value       = aws_s3_bucket.forge_data.bucket
}

output "forge_data_bucket_arn" {
  description = "S3 bucket ARN"
  value       = aws_s3_bucket.forge_data.arn
}

# ============================================================================
# Forge Orchestrator Config
# ============================================================================

output "forge_config" {
  description = "Configuration for Forge Orchestrator (JSON)"
  value = jsonencode({
    nomad = {
      address = "http://${module.nomad_servers.private_ips[0]}:4646"
      servers = module.nomad_servers.private_ips
    }
    consul = {
      address = var.consul_server_addresses
    }
    aws = {
      region     = var.aws_region
      datacenter = var.datacenter
    }
    scaling = {
      client_asg_name = module.nomad_clients.asg_name
      min_clients     = var.nomad_client_min_count
      max_clients     = var.nomad_client_max_count
    }
    storage = {
      bucket = aws_s3_bucket.forge_data.bucket
    }
  })
  sensitive = false
}
