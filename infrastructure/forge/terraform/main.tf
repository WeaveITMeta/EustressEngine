# Eustress Forge - Main Terraform Configuration
# AWS infrastructure for Nomad-based multiplayer orchestration
#
# This replaces the deprecated Kubernetes/Agones setup with a more efficient,
# Rust-native control plane achieving 80-90% cost savings.

terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  backend "s3" {
    bucket         = "eustress-forge-terraform-state"
    key            = "forge/terraform.tfstate"
    region         = "us-east-1"
    encrypt        = true
    dynamodb_table = "eustress-forge-terraform-locks"
  }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project     = "EustressForge"
      Environment = var.environment
      ManagedBy   = "Terraform"
    }
  }
}

# ============================================================================
# Data Sources
# ============================================================================

data "aws_availability_zones" "available" {
  state = "available"
}

data "aws_caller_identity" "current" {}

# ============================================================================
# VPC Module
# ============================================================================

module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "~> 5.0"

  name = "${var.project_name}-vpc"
  cidr = var.vpc_cidr

  azs             = slice(data.aws_availability_zones.available.names, 0, 3)
  private_subnets = var.private_subnet_cidrs
  public_subnets  = var.public_subnet_cidrs

  enable_nat_gateway     = true
  single_nat_gateway     = var.environment != "production"
  enable_dns_hostnames   = true
  enable_dns_support     = true

  # VPC Flow Logs for security
  enable_flow_log                      = var.enable_flow_logs
  create_flow_log_cloudwatch_log_group = var.enable_flow_logs
  create_flow_log_cloudwatch_iam_role  = var.enable_flow_logs

  tags = {
    Component = "networking"
  }
}

# ============================================================================
# Security Groups
# ============================================================================

module "nomad_server_sg" {
  source  = "terraform-aws-modules/security-group/aws"
  version = "~> 5.0"

  name        = "${var.project_name}-nomad-server-sg"
  description = "Security group for Nomad server nodes"
  vpc_id      = module.vpc.vpc_id

  # Ingress rules
  ingress_with_cidr_blocks = [
    {
      from_port   = 4646
      to_port     = 4648
      protocol    = "tcp"
      description = "Nomad HTTP/RPC/Serf"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
    {
      from_port   = 4648
      to_port     = 4648
      protocol    = "udp"
      description = "Nomad Serf UDP"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
  ]

  # Allow all egress
  egress_rules = ["all-all"]

  tags = {
    Component = "nomad-server"
  }
}

module "nomad_client_sg" {
  source  = "terraform-aws-modules/security-group/aws"
  version = "~> 5.0"

  name        = "${var.project_name}-nomad-client-sg"
  description = "Security group for Nomad client nodes (game servers)"
  vpc_id      = module.vpc.vpc_id

  ingress_with_cidr_blocks = [
    {
      from_port   = 4646
      to_port     = 4647
      protocol    = "tcp"
      description = "Nomad HTTP/RPC"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
    # QUIC/UDP for game traffic
    {
      from_port   = 4433
      to_port     = 4433
      protocol    = "udp"
      description = "QUIC game traffic"
      cidr_blocks = "0.0.0.0/0"
    },
    # Dynamic port range for game servers
    {
      from_port   = 20000
      to_port     = 32000
      protocol    = "udp"
      description = "Dynamic game server ports"
      cidr_blocks = "0.0.0.0/0"
    },
  ]

  egress_rules = ["all-all"]

  tags = {
    Component = "nomad-client"
  }
}

module "consul_sg" {
  source  = "terraform-aws-modules/security-group/aws"
  version = "~> 5.0"

  name        = "${var.project_name}-consul-sg"
  description = "Security group for Consul service mesh"
  vpc_id      = module.vpc.vpc_id

  ingress_with_cidr_blocks = [
    {
      from_port   = 8300
      to_port     = 8302
      protocol    = "tcp"
      description = "Consul RPC/Serf LAN/WAN"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
    {
      from_port   = 8301
      to_port     = 8302
      protocol    = "udp"
      description = "Consul Serf UDP"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
    {
      from_port   = 8500
      to_port     = 8500
      protocol    = "tcp"
      description = "Consul HTTP API"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
    {
      from_port   = 8600
      to_port     = 8600
      protocol    = "tcp"
      description = "Consul DNS"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
    {
      from_port   = 8600
      to_port     = 8600
      protocol    = "udp"
      description = "Consul DNS UDP"
      cidr_blocks = module.vpc.vpc_cidr_block
    },
  ]

  egress_rules = ["all-all"]

  tags = {
    Component = "consul"
  }
}

# ============================================================================
# Nomad Server Cluster (Control Plane)
# ============================================================================

module "nomad_servers" {
  source = "./modules/nomad-cluster"

  name                = "${var.project_name}-nomad-server"
  instance_type       = var.nomad_server_instance_type
  desired_capacity    = var.nomad_server_count
  min_size            = var.nomad_server_count
  max_size            = var.nomad_server_count
  
  subnet_ids          = module.vpc.private_subnets
  security_group_ids  = [
    module.nomad_server_sg.security_group_id,
    module.consul_sg.security_group_id,
  ]
  
  ami_id              = var.nomad_ami_id
  key_name            = var.ssh_key_name
  iam_instance_profile = aws_iam_instance_profile.nomad_server.name
  
  is_server           = true
  datacenter          = var.datacenter
  consul_servers      = var.consul_server_addresses
  
  # Reserved instances for reliability (no spot)
  use_spot            = false

  tags = {
    Component = "nomad-server"
    Role      = "control-plane"
  }
}

# ============================================================================
# Nomad Client Cluster (Game Servers)
# ============================================================================

module "nomad_clients" {
  source = "./modules/nomad-cluster"

  name                = "${var.project_name}-nomad-client"
  instance_type       = var.nomad_client_instance_type
  desired_capacity    = var.nomad_client_min_count
  min_size            = var.nomad_client_min_count
  max_size            = var.nomad_client_max_count
  
  subnet_ids          = module.vpc.private_subnets
  security_group_ids  = [
    module.nomad_client_sg.security_group_id,
    module.consul_sg.security_group_id,
  ]
  
  ami_id              = var.nomad_ami_id
  key_name            = var.ssh_key_name
  iam_instance_profile = aws_iam_instance_profile.nomad_client.name
  
  is_server           = false
  datacenter          = var.datacenter
  consul_servers      = var.consul_server_addresses
  nomad_servers       = module.nomad_servers.private_ips
  
  # Aggressive spot usage for 60-80% savings
  use_spot            = true
  spot_max_price      = var.spot_max_price
  
  # Mixed instance types for spot availability
  mixed_instances_policy = {
    instances_distribution = {
      on_demand_base_capacity                  = var.on_demand_base_capacity
      on_demand_percentage_above_base_capacity = 10
      spot_allocation_strategy                 = "capacity-optimized"
    }
    override = var.spot_instance_types
  }

  tags = {
    Component = "nomad-client"
    Role      = "game-server"
  }
}

# ============================================================================
# IAM Roles
# ============================================================================

resource "aws_iam_role" "nomad_server" {
  name = "${var.project_name}-nomad-server-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })
}

resource "aws_iam_role_policy" "nomad_server" {
  name = "${var.project_name}-nomad-server-policy"
  role = aws_iam_role.nomad_server.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "ec2:DescribeInstances",
          "ec2:DescribeTags",
          "autoscaling:DescribeAutoScalingGroups",
        ]
        Resource = "*"
      },
      {
        Effect = "Allow"
        Action = [
          "s3:GetObject",
          "s3:PutObject",
        ]
        Resource = "${aws_s3_bucket.forge_data.arn}/*"
      }
    ]
  })
}

resource "aws_iam_instance_profile" "nomad_server" {
  name = "${var.project_name}-nomad-server-profile"
  role = aws_iam_role.nomad_server.name
}

resource "aws_iam_role" "nomad_client" {
  name = "${var.project_name}-nomad-client-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })
}

resource "aws_iam_role_policy" "nomad_client" {
  name = "${var.project_name}-nomad-client-policy"
  role = aws_iam_role.nomad_client.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "ec2:DescribeInstances",
          "ec2:DescribeTags",
        ]
        Resource = "*"
      },
      {
        Effect = "Allow"
        Action = [
          "ecr:GetAuthorizationToken",
          "ecr:BatchCheckLayerAvailability",
          "ecr:GetDownloadUrlForLayer",
          "ecr:BatchGetImage",
        ]
        Resource = "*"
      },
      {
        Effect = "Allow"
        Action = [
          "s3:GetObject",
        ]
        Resource = "${aws_s3_bucket.forge_data.arn}/*"
      }
    ]
  })
}

resource "aws_iam_instance_profile" "nomad_client" {
  name = "${var.project_name}-nomad-client-profile"
  role = aws_iam_role.nomad_client.name
}

# ============================================================================
# S3 Bucket for Game Data
# ============================================================================

resource "aws_s3_bucket" "forge_data" {
  bucket = "${var.project_name}-data-${data.aws_caller_identity.current.account_id}"

  tags = {
    Component = "storage"
  }
}

resource "aws_s3_bucket_versioning" "forge_data" {
  bucket = aws_s3_bucket.forge_data.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "forge_data" {
  bucket = aws_s3_bucket.forge_data.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

# ============================================================================
# Auto Scaling for Game Servers
# ============================================================================

resource "aws_autoscaling_policy" "scale_up" {
  name                   = "${var.project_name}-scale-up"
  scaling_adjustment     = var.scale_up_adjustment
  adjustment_type        = "ChangeInCapacity"
  cooldown               = 60
  autoscaling_group_name = module.nomad_clients.asg_name
}

resource "aws_autoscaling_policy" "scale_down" {
  name                   = "${var.project_name}-scale-down"
  scaling_adjustment     = var.scale_down_adjustment
  adjustment_type        = "ChangeInCapacity"
  cooldown               = 300
  autoscaling_group_name = module.nomad_clients.asg_name
}

resource "aws_cloudwatch_metric_alarm" "high_cpu" {
  alarm_name          = "${var.project_name}-high-cpu"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = 60
  statistic           = "Average"
  threshold           = 70

  dimensions = {
    AutoScalingGroupName = module.nomad_clients.asg_name
  }

  alarm_actions = [aws_autoscaling_policy.scale_up.arn]
}

resource "aws_cloudwatch_metric_alarm" "low_cpu" {
  alarm_name          = "${var.project_name}-low-cpu"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 5
  metric_name         = "CPUUtilization"
  namespace           = "AWS/EC2"
  period              = 60
  statistic           = "Average"
  threshold           = 30

  dimensions = {
    AutoScalingGroupName = module.nomad_clients.asg_name
  }

  alarm_actions = [aws_autoscaling_policy.scale_down.arn]
}
