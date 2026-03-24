//! # Brief Normalizer — freeform text → ideation_brief.toml via Claude
//!
//! Takes the accumulated conversation context from the Workshop chat and asks
//! Claude to extract a structured IdeationBrief. The result is written to
//! docs/Products/{ProductName}/ideation_brief.toml.
//!
//! ## Table of Contents
//!
//! 1. System prompt template for normalization
//! 2. normalize_brief() — async Claude call that returns IdeationBrief
//! 3. write_brief_to_disk() — serialize to TOML and write to product directory
//! 4. Bevy system for dispatching normalization as an async task

use std::path::{Path, PathBuf};
use bevy::log::{info, warn, error};

use super::IdeationBrief;

// ============================================================================
// 1. System prompt for normalization
// ============================================================================

/// System prompt that instructs Claude to extract structured product data
/// from a freeform conversation. Returns TOML that deserializes into IdeationBrief.
pub const NORMALIZER_SYSTEM_PROMPT: &str = r#"You are a product ideation normalizer for the Eustress Engine.

Your task: Given a conversation between a user and the Workshop system about a product idea, extract a structured product brief in TOML format.

Output ONLY valid TOML that matches this schema exactly. No markdown fences, no explanation, just raw TOML:

[product]
name = "Product Name"
description = "One-line description"
category = "conventional"       # "conventional" | "exotic_propulsion"
tier = "foundation"             # "foundation" | "platform" | "horizon"
version = "V1"

[product.dimensions]
width = 0.100                   # meters
height = 0.300                  # meters
depth = 0.012                   # meters
form_factor = "prismatic"       # "prismatic" | "cylindrical" | "disc" | "custom"

[[innovations]]
name = "Innovation Name"
description = "What it does and why it matters"
tier = "VERIFIED"               # "VERIFIED" | "PROJECTED" | "ASPIRATIONAL"

[[target_specs]]
metric = "energy_density"
target = 900.0
unit = "Wh/kg"
benchmark = 250.0
benchmark_label = "Li-Ion (NMC 811)"

[[bill_of_materials]]
component = "Housing"
material = "Al 6061-T6"
dimensions = [0.300, 0.100, 0.012]  # meters [L, W, H]
role = "structural_enclosure"

[deal_structure]
title = "Product Name — V1 Manufacturing Deal"
manufacturing_program_royalty_pct = 8.0   # % of net sales to Manufacturing Program fund
inventor_royalty_pct = 5.0                # % of net sales retained by inventor
unit_price_usd = 49.00                    # suggested retail price
unit_cost_usd = 12.50                     # estimated BOM + assembly + logistics
pilot_minimum_units = 1000                # units required before full production unlocks
pilot_geography = "US"                    # target geography for pilot
term_validity_months = 6                  # how long this term sheet is valid

[[deal_structure.equity_splits]]
stakeholder = "Inventor"
role = "IP owner and product creator"
percentage = 60.0
# vesting_cliff_months — omit for immediate vesting

[[deal_structure.equity_splits]]
stakeholder = "Eustress Manufacturing Program"
role = "Manufacturing fund, infrastructure, distribution"
percentage = 25.0

[[deal_structure.equity_splits]]
stakeholder = "Logistics Partner"
role = "3PL, warehousing, fulfillment operations"
percentage = 10.0
vesting_cliff_months = 12
vesting_period_months = 24

[[deal_structure.equity_splits]]
stakeholder = "Reserve Pool"
role = "Future co-investors, advisors, strategic partners"
percentage = 5.0

[ideation_metadata]
source = "workshop_panel"

Rules:
- Extract ALL innovations, specs, and BOM entries mentioned in the conversation
- If the user didn't specify exact dimensions, estimate reasonable defaults
- If the user didn't specify a category, infer from the product type
- tier for innovations: VERIFIED = proven tech, PROJECTED = lab-demonstrated, ASPIRATIONAL = theoretical
- Every BOM entry needs a role (what it does in the assembly)
- Be precise with units and values — no rounding unless necessary
- If information is ambiguous, pick the most likely interpretation
- For deal_structure: estimate unit_price_usd and unit_cost_usd from BOM entries (unit cost = BOM total + 35% assembly + 15% logistics + 10% returns)
- Adjust equity_splits percentages if the user mentioned co-investors, partners, or unusual IP arrangements
- equity_splits percentages MUST sum to exactly 100.0
"#;

// ============================================================================
// 2. normalize_brief() — parse Claude's TOML response into IdeationBrief
// ============================================================================

/// Parse a TOML string (from Claude) into an IdeationBrief
pub fn parse_brief_from_toml(toml_str: &str) -> Result<IdeationBrief, String> {
    // Strip markdown fences if Claude wraps the output despite instructions
    let cleaned = toml_str
        .trim()
        .strip_prefix("```toml").unwrap_or(toml_str.trim())
        .strip_prefix("```").unwrap_or(toml_str.trim())
        .strip_suffix("```").unwrap_or(toml_str.trim())
        .trim();
    
    toml::from_str::<IdeationBrief>(cleaned)
        .map_err(|e| format!("Failed to parse ideation brief TOML: {}", e))
}

/// Build the normalization prompt from conversation context
pub fn build_normalize_prompt(conversation_context: &str) -> String {
    format!(
        "Here is the conversation about a product idea:\n\n{}\n\n\
         Extract the structured product brief as TOML. Output ONLY the TOML, nothing else.",
        conversation_context
    )
}

// ============================================================================
// 3. write_brief_to_disk() — serialize to TOML and write to product directory
// ============================================================================

/// Determine the product output directory within a Space's Workspace service.
///
/// Returns `space_root/Workspace/{safe_name}/` — the product directory is a child
/// of Workspace, containing part files, documentation, and the ideation brief.
/// No `docs/Products/` directory is used in Eustress.
pub fn product_output_dir(space_root: &Path, product_name: &str) -> PathBuf {
    let safe_name = product_name
        .replace(' ', "_")
        .replace('/', "_")
        .replace('\\', "_")
        .replace(':', "_");
    space_root.join("Workspace").join(safe_name)
}

/// Write an IdeationBrief to ideation_brief.toml in the product directory
pub fn write_brief_to_disk(
    output_dir: &Path,
    brief: &IdeationBrief,
) -> Result<PathBuf, String> {
    // Ensure directory exists
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create product directory {}: {}", output_dir.display(), e))?;
    
    let brief_path = output_dir.join("ideation_brief.toml");
    
    let toml_str = toml::to_string_pretty(brief)
        .map_err(|e| format!("Failed to serialize ideation brief: {}", e))?;
    
    // Add a header comment
    let content = format!(
        "# Ideation Brief — structured output of System 0\n\
         # Generated by Eustress Workshop Panel\n\
         # Product: {}\n\
         # Session: {}\n\n{}",
        brief.product.name,
        brief.ideation_metadata.session_id,
        toml_str
    );
    
    std::fs::write(&brief_path, content)
        .map_err(|e| format!("Failed to write {}: {}", brief_path.display(), e))?;
    
    info!("Workshop: Wrote ideation_brief.toml to {:?}", brief_path);
    
    Ok(brief_path)
}

// ============================================================================
// 4. Validation helpers
// ============================================================================

/// Validate that an IdeationBrief has the minimum required fields
pub fn validate_brief(brief: &IdeationBrief) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    
    if brief.product.name.is_empty() {
        errors.push("Product name is required".to_string());
    }
    if brief.product.description.is_empty() {
        errors.push("Product description is required".to_string());
    }
    if brief.innovations.is_empty() {
        errors.push("At least one innovation is required".to_string());
    }
    if brief.bill_of_materials.is_empty() {
        errors.push("At least one BOM entry is required".to_string());
    }
    
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_brief_from_toml() {
        let toml_str = r#"
[product]
name = "V-Cell 4680"
description = "Solid-state sodium-sulfur energy cell"
category = "conventional"
tier = "foundation"
version = "V1"

[product.dimensions]
width = 0.100
height = 0.300
depth = 0.012
form_factor = "prismatic"

[[innovations]]
name = "Sc-NASICON Solid Electrolyte"
description = "Scandium-doped NASICON"
tier = "VERIFIED"

[[target_specs]]
metric = "energy_density"
target = 900.0
unit = "Wh/kg"
benchmark = 250.0
benchmark_label = "Li-Ion"

[[bill_of_materials]]
component = "Housing"
material = "Al 6061-T6"
dimensions = [0.300, 0.100, 0.012]
role = "structural_enclosure"

[ideation_metadata]
source = "workshop_panel"
"#;
        
        let brief = parse_brief_from_toml(toml_str).expect("Should parse valid TOML");
        assert_eq!(brief.product.name, "V-Cell 4680");
        assert_eq!(brief.innovations.len(), 1);
        assert_eq!(brief.target_specs.len(), 1);
        assert_eq!(brief.bill_of_materials.len(), 1);
    }
    
    #[test]
    fn test_parse_brief_strips_markdown_fences() {
        let toml_str = r#"```toml
[product]
name = "Test"
description = "Test product"

[ideation_metadata]
source = "workshop_panel"
```"#;
        
        let brief = parse_brief_from_toml(toml_str).expect("Should strip fences and parse");
        assert_eq!(brief.product.name, "Test");
    }
    
    #[test]
    fn test_validate_brief_empty_name() {
        let brief = IdeationBrief::default();
        let errors = validate_brief(&brief).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("name")));
    }
}
