# Eustress Forge - Nomad Cluster Module Variables

variable "name" {
  description = "Name prefix for resources"
  type        = string
}

variable "instance_type" {
  description = "EC2 instance type"
  type        = string
}

variable "ami_id" {
  description = "AMI ID for instances"
  type        = string
}

variable "key_name" {
  description = "SSH key pair name"
  type        = string
}

variable "iam_instance_profile" {
  description = "IAM instance profile name"
  type        = string
}

variable "subnet_ids" {
  description = "Subnet IDs for ASG"
  type        = list(string)
}

variable "security_group_ids" {
  description = "Security group IDs"
  type        = list(string)
}

variable "desired_capacity" {
  description = "Desired number of instances"
  type        = number
}

variable "min_size" {
  description = "Minimum number of instances"
  type        = number
}

variable "max_size" {
  description = "Maximum number of instances"
  type        = number
}

variable "is_server" {
  description = "Whether this is a Nomad server (true) or client (false)"
  type        = bool
  default     = false
}

variable "datacenter" {
  description = "Nomad/Consul datacenter name"
  type        = string
  default     = "dc1"
}

variable "consul_servers" {
  description = "List of Consul server addresses"
  type        = list(string)
  default     = []
}

variable "nomad_servers" {
  description = "List of Nomad server addresses (for clients)"
  type        = list(string)
  default     = []
}

variable "node_class" {
  description = "Nomad client node class"
  type        = string
  default     = "game-server"
}

variable "meta_tags" {
  description = "Nomad client meta tags"
  type        = map(string)
  default     = {}
}

variable "root_volume_size" {
  description = "Root EBS volume size (GB)"
  type        = number
  default     = 50
}

variable "data_volume_size" {
  description = "Data EBS volume size (GB), 0 to disable"
  type        = number
  default     = 100
}

variable "use_spot" {
  description = "Use spot instances"
  type        = bool
  default     = false
}

variable "spot_max_price" {
  description = "Maximum spot price"
  type        = string
  default     = ""
}

variable "mixed_instances_policy" {
  description = "Mixed instances policy for spot"
  type = object({
    instances_distribution = object({
      on_demand_base_capacity                  = number
      on_demand_percentage_above_base_capacity = number
      spot_allocation_strategy                 = string
    })
    override = list(object({
      instance_type     = string
      weighted_capacity = optional(number, 1)
    }))
  })
  default = {
    instances_distribution = {
      on_demand_base_capacity                  = 0
      on_demand_percentage_above_base_capacity = 0
      spot_allocation_strategy                 = "capacity-optimized"
    }
    override = []
  }
}

variable "tags" {
  description = "Tags to apply to resources"
  type        = map(string)
  default     = {}
}
