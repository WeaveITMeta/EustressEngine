#!/bin/bash
# Eustress Forge - Deployment Script
# Deploys the complete Forge infrastructure to AWS

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FORGE_DIR="$(dirname "$SCRIPT_DIR")"
TERRAFORM_DIR="$FORGE_DIR/terraform"
NOMAD_DIR="$FORGE_DIR/nomad"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() { echo -e "${GREEN}[INFO]${NC} $1"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
log_error() { echo -e "${RED}[ERROR]${NC} $1"; }

# ============================================================================
# Pre-flight Checks
# ============================================================================

check_prerequisites() {
    log_info "Checking prerequisites..."
    
    # Check Terraform
    if ! command -v terraform &> /dev/null; then
        log_error "Terraform is not installed. Please install Terraform >= 1.5"
        exit 1
    fi
    
    # Check AWS CLI
    if ! command -v aws &> /dev/null; then
        log_error "AWS CLI is not installed. Please install and configure AWS CLI"
        exit 1
    fi
    
    # Check AWS credentials
    if ! aws sts get-caller-identity &> /dev/null; then
        log_error "AWS credentials not configured. Run 'aws configure'"
        exit 1
    fi
    
    # Check Nomad CLI (optional, for job deployment)
    if ! command -v nomad &> /dev/null; then
        log_warn "Nomad CLI not installed. Job deployment will be skipped."
    fi
    
    log_info "All prerequisites satisfied"
}

# ============================================================================
# Terraform Deployment
# ============================================================================

deploy_infrastructure() {
    log_info "Deploying infrastructure with Terraform..."
    
    cd "$TERRAFORM_DIR"
    
    # Check for tfvars
    if [ ! -f "terraform.tfvars" ]; then
        log_error "terraform.tfvars not found. Copy terraform.tfvars.example and configure."
        exit 1
    fi
    
    # Initialize Terraform
    log_info "Initializing Terraform..."
    terraform init -upgrade
    
    # Validate configuration
    log_info "Validating Terraform configuration..."
    terraform validate
    
    # Plan deployment
    log_info "Planning deployment..."
    terraform plan -out=tfplan
    
    # Confirm deployment
    read -p "Do you want to apply this plan? (yes/no): " confirm
    if [ "$confirm" != "yes" ]; then
        log_warn "Deployment cancelled"
        exit 0
    fi
    
    # Apply
    log_info "Applying Terraform plan..."
    terraform apply tfplan
    
    # Export outputs
    log_info "Exporting Terraform outputs..."
    terraform output -json > "$FORGE_DIR/terraform-outputs.json"
    
    log_info "Infrastructure deployment complete"
}

# ============================================================================
# Nomad Job Deployment
# ============================================================================

deploy_nomad_jobs() {
    if ! command -v nomad &> /dev/null; then
        log_warn "Skipping Nomad job deployment (CLI not installed)"
        return
    fi
    
    log_info "Deploying Nomad jobs..."
    
    # Get Nomad address from Terraform output
    NOMAD_ADDR=$(jq -r '.nomad_http_endpoint.value' "$FORGE_DIR/terraform-outputs.json")
    export NOMAD_ADDR
    
    # Wait for Nomad to be ready
    log_info "Waiting for Nomad cluster..."
    for i in {1..30}; do
        if nomad server members &> /dev/null; then
            break
        fi
        sleep 10
    done
    
    # Deploy Forge Orchestrator (system job)
    log_info "Deploying Forge Orchestrator..."
    nomad job run "$NOMAD_DIR/forge-orchestrator.hcl"
    
    # Register parameterized jobs (templates)
    log_info "Registering game server job template..."
    nomad job run "$NOMAD_DIR/gameserver.hcl"
    
    log_info "Registering physics server job template..."
    nomad job run "$NOMAD_DIR/physics.hcl"
    
    log_info "Registering AI server job template..."
    nomad job run "$NOMAD_DIR/ai.hcl"
    
    log_info "Nomad jobs deployed successfully"
}

# ============================================================================
# Post-Deployment Verification
# ============================================================================

verify_deployment() {
    log_info "Verifying deployment..."
    
    # Check Nomad cluster health
    if command -v nomad &> /dev/null; then
        NOMAD_ADDR=$(jq -r '.nomad_http_endpoint.value' "$FORGE_DIR/terraform-outputs.json")
        export NOMAD_ADDR
        
        log_info "Nomad server members:"
        nomad server members || true
        
        log_info "Nomad node status:"
        nomad node status || true
        
        log_info "Nomad job status:"
        nomad job status || true
    fi
    
    log_info "Deployment verification complete"
}

# ============================================================================
# Main
# ============================================================================

main() {
    log_info "=== Eustress Forge Deployment ==="
    
    check_prerequisites
    deploy_infrastructure
    deploy_nomad_jobs
    verify_deployment
    
    log_info "=== Deployment Complete ==="
    log_info "Forge Orchestrator API: $(jq -r '.nomad_http_endpoint.value' "$FORGE_DIR/terraform-outputs.json" | sed 's/4646/8080/')"
    log_info "Nomad UI: $(jq -r '.nomad_http_endpoint.value' "$FORGE_DIR/terraform-outputs.json")/ui"
}

main "$@"
