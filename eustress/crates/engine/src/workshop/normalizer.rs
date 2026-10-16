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
use bevy::log::info;

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

/// The content of a model reply that may be wrapped in a markdown code fence.
///
/// The prompt says to output raw TOML, and a model sometimes fences it anyway,
/// sometimes after a sentence of its own. The fence is the first line that begins
/// with three backticks (its language tag, `toml`, is not content), and it closes at
/// the first line that begins with three backticks after it (so a second block, or a
/// sentence with backticks of its own, after the first is not swept in), or at the end
/// when the reply was cut off. A reply with no fence comes back trimmed and otherwise as it was,
/// including backticks inside a value.
///
/// The chain this replaces stripped the prefix, the language tag and the suffix with
/// `unwrap_or(toml_str.trim())` after each step, which falls back to the ORIGINAL text
/// rather than the previous step's result: after the language tag was removed, the next
/// step found no bare fence and put the whole fenced reply back.
fn strip_markdown_fences(reply: &str) -> &str {
    let text = reply.trim();
    let mut after = 0;
    let mut opened = false;
    for line in text.split_inclusive('\n') {
        after += line.len();
        if line.trim_start().starts_with("```") {
            opened = true;
            break;
        }
    }
    if !opened {
        return text;
    }
    let body = &text[after..];
    let close = body
        .match_indices("```")
        .map(|(at, _)| at)
        .filter(|&at| at == 0 || body.as_bytes()[at - 1] == b'\n')
        .next();
    let body = close.map_or(body, |at| &body[..at]).trim();
    // A fence closed on the same line as the last of the content.
    body.strip_suffix("```").unwrap_or(body).trim()
}

/// A reply that is ONE fenced block and nothing else, without its fence; any other text
/// comes back untouched.
///
/// For text a model was told to write raw but wrapped in a fence anyway: a Blender script,
/// a part file, a markdown document. Stricter than [`strip_markdown_fences`] on purpose. A
/// markdown document with a code block in the middle, or one that opens with a code block
/// and goes on, is a document and not a wrapped one, and cutting it down to its first block
/// would destroy it. So a reply is unwrapped only when its first line opens a fence and the
/// line that closes that fence is the last line.
///
/// Fences follow CommonMark: a closing fence is only backticks, at least as many as the
/// opening one, so a "```python" line inside a four-backtick wrapper is content. A reply
/// cut off before its closing fence loses only its opening line.
pub(crate) fn unwrap_fenced_document(reply: &str) -> &str {
    let text = reply.trim();
    let Some(first) = text.split_inclusive('\n').next() else { return reply };
    let opening = first.trim();
    let ticks = opening.chars().take_while(|&c| c == '`').count();
    // Three backticks, then at most a language tag: a backtick later on the line means
    // this is inline code, not a fence.
    if ticks < 3 || opening[ticks..].contains('`') {
        return reply;
    }
    let body = &text[first.len()..];
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        let closing = line.trim();
        if closing.len() >= ticks && closing.chars().all(|c| c == '`') {
            // Anything after the closing fence means the reply was not one wrapped block.
            return if body[at + line.len()..].trim().is_empty() { body[..at].trim() } else { reply };
        }
        at += line.len();
    }
    // Never closed: a reply cut off. The opening line was not content.
    body.trim()
}

/// Parse a TOML string (from Claude) into an IdeationBrief
pub fn parse_brief_from_toml(toml_str: &str) -> Result<IdeationBrief, String> {
    // Strip markdown fences if Claude wraps the output despite instructions
    let cleaned = strip_markdown_fences(toml_str);

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
    fn a_fenced_reply_comes_back_as_its_content() {
        let toml = "[product]\nname = \"Test\"\n";
        assert_eq!(strip_markdown_fences(&format!("```toml\n{toml}```")), toml.trim());
        assert_eq!(strip_markdown_fences(&format!("```toml\n{toml}\n```")), toml.trim());
        assert_eq!(strip_markdown_fences(&format!("```\n{toml}\n```")), toml.trim(), "no language tag");
        assert_eq!(strip_markdown_fences(&format!("```TOML\n{toml}```")), toml.trim(), "any case");
        assert_eq!(strip_markdown_fences(&format!("  \n```toml\n{toml}```\n\n")), toml.trim(), "outer whitespace");
        assert_eq!(strip_markdown_fences(&format!("```toml\r\n{toml}\r\n```\r\n")), toml.trim(), "Windows line ends");
    }

    #[test]
    fn a_sentence_before_or_after_the_fence_is_dropped() {
        let toml = "[product]\nname = \"Test\"";
        assert_eq!(strip_markdown_fences(&format!("Here is the brief:\n```toml\n{toml}\n```")), toml);
        assert_eq!(strip_markdown_fences(&format!("```toml\n{toml}\n```\nLet me know if you want changes.")), toml);
    }

    #[test]
    fn the_first_block_wins_and_prose_with_backticks_after_it_is_not_swept_in() {
        let toml = "[product]\nname = \"Test\"";
        // A sentence after the closing fence that has backticks of its own mid-line.
        let tail = "```toml\n".to_string() + toml + "\n```\nUse ```toml blocks like this one.";
        assert_eq!(strip_markdown_fences(&tail), toml);
        // A second fenced block after the first: the first is the answer.
        let two = format!("```toml\n{toml}\n```\nAn alternative:\n```toml\n[product]\nname = \"Other\"\n```");
        assert_eq!(strip_markdown_fences(&two), toml);
    }

    #[test]
    fn backticks_inside_a_fenced_value_do_not_close_the_fence() {
        // Only a line that BEGINS with three backticks closes it.
        let toml = "[product]\ndescription = \"use ```code``` here\"";
        assert_eq!(strip_markdown_fences(&format!("```toml\n{toml}\n```")), toml);
    }

    #[test]
    fn a_reply_cut_off_before_its_closing_fence_still_yields_the_content() {
        let toml = "[product]\nname = \"Test\"";
        assert_eq!(strip_markdown_fences(&format!("```toml\n{toml}")), toml);
    }

    #[test]
    fn a_reply_with_no_fence_is_left_alone_even_with_backticks_in_a_value() {
        let toml = "[product]\nname = \"Test\"\ndescription = \"use ```code``` here\"";
        assert_eq!(strip_markdown_fences(&format!("  {toml}\n")), toml);
        assert_eq!(strip_markdown_fences(""), "");
    }

    #[test]
    fn the_old_chain_returned_the_fenced_text() {
        // The behaviour that failed test_parse_brief_strips_markdown_fences, kept as a
        // statement of why the helper exists.
        let reply = "```toml\n[product]\nname = \"Test\"\n```";
        let old = reply
            .trim()
            .strip_prefix("```toml").unwrap_or(reply.trim())
            .strip_prefix("```").unwrap_or(reply.trim())
            .strip_suffix("```").unwrap_or(reply.trim())
            .trim();
        assert!(old.starts_with("```toml"), "the old chain put the fence back");
        assert!(!strip_markdown_fences(reply).contains("```"));
    }

    #[test]
    fn a_document_that_is_one_fenced_block_is_unwrapped() {
        let doc = "# Patent\n\nClaim 1: a cell.";
        for wrapped in [
            format!("```markdown\n{doc}\n```"),
            format!("```\n{doc}\n```"),
            format!("```TOML\n{doc}\n```"),
            format!("  \n```markdown\r\n{doc}\r\n```\r\n\r\n"),
            format!("```markdown\n{doc}\n```   "),
        ] {
            assert_eq!(unwrap_fenced_document(&wrapped), doc, "{wrapped:?}");
        }
    }

    #[test]
    fn a_document_with_a_code_block_inside_is_left_whole() {
        // The case that makes this stricter than strip_markdown_fences: these are documents,
        // not wrapped ones, and cutting them to a block would destroy them.
        for doc in [
            "# Patent\n\nFigure 1:\n```text\nfig\n```\n\nClaim 2.",
            "```text\nfig\n```\n\nThen the claims follow here.",
            "# Title\n\n```sh\nmake\n```",
            "```sh\nmake\n```\n\nand later\n\n```sh\nmake test\n```",
            "plain text with `inline code` and no fence",
            "```code``` on one line",
            "`one backtick` start",
            "",
        ] {
            assert_eq!(unwrap_fenced_document(doc), doc, "{doc:?}");
        }
    }

    #[test]
    fn a_longer_wrapper_keeps_the_blocks_inside_it() {
        let inner = "# Doc\n\n```python\nprint(1)\n```\n\nEnd.";
        assert_eq!(unwrap_fenced_document(&format!("````markdown\n{inner}\n````")), inner);
        // A three-backtick wrapper around three-backtick blocks cannot be told apart from
        // a document followed by prose, so it is left alone.
        let ambiguous = format!("```markdown\n{inner}\n```");
        assert_eq!(unwrap_fenced_document(&ambiguous), ambiguous);
    }

    #[test]
    fn a_wrapped_reply_cut_off_before_its_closing_fence_loses_only_the_opening_line() {
        assert_eq!(unwrap_fenced_document("```python\nprint(1)\nprint(2)"), "print(1)\nprint(2)");
        assert_eq!(unwrap_fenced_document("```python"), "");
    }

    #[test]
    fn test_validate_brief_empty_name() {
        let brief = IdeationBrief::default();
        let errors = validate_brief(&brief).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("name")));
    }
}
