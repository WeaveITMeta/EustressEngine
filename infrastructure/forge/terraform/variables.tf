# Eustress Forge - Terraform Variables
# Configuration variables for AWS/Nomad deployment

# ============================================================================
# General
# ============================================================================

variable "project_name" {
  description = "Project name used for resource naming"
  type        = string
  default     = "eustress-forge"
}

variable "environment" {
  description = "Environment (development, staging, production)"
  type        = string
  default     = "development"

  validation {
    condition     = contains(["development", "staging", "production"], var.environment)
    error_message = "Environment must be development, staging, or production."
  }
}

variable "aws_region" {
  description = "AWS region for deployment"
  type        = string
  default     = "us-east-1"
}

variable "datacenter" {
  description = "Nomad/Consul datacenter name"
  type        = string
  default     = "dc1"
}

# ============================================================================
# Networking
# ============================================================================

variable "vpc_cidr" {
  description = "CIDR block for VPC"
  type        = string
  default     = "10.0.0.0/16"
}

variable "private_subnet_cidrs" {
  description = "CIDR blocks for private subnets"
  type        = list(string)
  default     = ["10.0.1.0/24", "10.0.2.0/24", "10.0.3.0/24"]
}

variable "public_subnet_cidrs" {
  description = "CIDR blocks for public subnets"
  type        = list(string)
  default     = ["10.0.101.0/24", "10.0.102.0/24", "10.0.103.0/24"]
}

variable "enable_flow_logs" {
  description = "Enable VPC flow logs for security monitoring"
  type        = bool
  default     = true
}

# ============================================================================
# Nomad Server (Control Plane)
# ============================================================================

variable "nomad_server_count" {
  description = "Number of Nomad server nodes (odd number for raft consensus)"
  type        = number
  default     = 3

  validation {
    condition     = var.nomad_server_count % 2 == 1 && var.nomad_server_count >= 3
    error_message = "Nomad server count must be an odd number >= 3."
  }
}

variable "nomad_server_instance_type" {
  description = "EC2 instance type for Nomad servers"
  type        = string
  default     = "m6i.large"
}

variable "nomad_ami_id" {
  description = "AMI ID with Nomad/Consul pre-installed (use Packer to build)"
  type        = string
}

variable "ssh_key_name" {
  description = "SSH key pair name for EC2 access"
  type        = string
}

variable "consul_server_addresses" {
  description = "List of Consul server addresses for service discovery"
  type        = list(string)
  default     = []
}

# ============================================================================
# Nomad Client (Game Servers)
# ============================================================================

variable "nomad_client_min_count" {
  description = "Minimum number of Nomad client nodes"
  type        = number
  default     = 3
}

variable "nomad_client_max_count" {
  description = "Maximum number of Nomad client nodes (for viral scaling)"
  type        = number
  default     = 1000
}

variable "nomad_client_instance_type" {
  description = "Primary EC2 instance type for Nomad clients"
  type        = string
  default     = "c7i.2xlarge"
}

variable "spot_max_price" {
  description = "Maximum spot price (set high for reliability)"
  type        = string
  default     = "0.50"
}

variable "on_demand_base_capacity" {
  description = "Number of on-demand instances as base capacity"
  type        = number
  default     = 2
}

variable "spot_instance_types" {
  description = "Mixed instance types for spot fleet diversity"
  type = list(object({
    instance_type     = string
    weighted_capacity = optional(number, 1)
  }))
  default = [
    { instance_type = "c7i.2xlarge" },
    { instance_type = "c6i.2xlarge" },
    { instance_type = "c6a.2xlarge" },
    { instance_type = "c5.2xlarge" },
    { instance_type = "m6i.2xlarge" },
  ]
}

# ============================================================================
# Auto Scaling
# ============================================================================

variable "scale_up_adjustment" {
  description = "Number of instances to add when scaling up"
  type        = number
  default     = 5
}

variable "scale_down_adjustment" {
  description = "Number of instances to remove when scaling down (negative)"
  type        = number
  default     = -2
}

# ============================================================================
# Cost Optimization
# ============================================================================

variable "enable_spot_interruption_handler" {
  description = "Enable graceful handling of spot interruptions"
  type        = bool
  default     = true
}

variable "spot_allocation_strategy" {
  description = "Spot allocation strategy (capacity-optimized recommended)"
  type        = string
  default     = "capacity-optimized"
}
