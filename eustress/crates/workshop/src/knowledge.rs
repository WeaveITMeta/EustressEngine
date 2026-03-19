//! # knowledge
//!
//! Embedded tool knowledge base — what each tool is, what it is used for, and how to use it.
//! This knowledge is injected into the AI system prompt when generating build guides,
//! giving the AI accurate, tool-specific instructions rather than generic descriptions.
//!
//! ## Table of Contents
//!
//! | Section            | Purpose                                                         |
//! |--------------------|-----------------------------------------------------------------|
//! | `ToolKnowledge`    | A single tool's embedded knowledge entry (name, use, technique) |
//! | `KnowledgeBase`    | The full embedded knowledge database for all common tool types  |
//! | `build_prompt`     | Builds the AI context string from registry + knowledge base     |
//!
//! ## Design
//!
//! The embedded knowledge covers general tool families (e.g. "drill/driver").
//! When a specific registered tool (e.g. "Milwaukee M18 Drill") has its own
//! `how_to_use` and `safety_notes` fields, those OVERRIDE the embedded knowledge
//! in the AI prompt — the registered tool's spec is always more specific and authoritative.

use serde::{Deserialize, Serialize};

// ============================================================================
// 1. ToolKnowledge — a single knowledge entry
// ============================================================================

/// Embedded knowledge for a class of tools — injected into the AI prompt
/// when the registered tool does not have its own `how_to_use` content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolKnowledge {
    /// Tool category keyword — matched against `RegisteredTool.name` and `tags` (case-insensitive)
    pub keyword: String,
    /// One-sentence definition of what this tool is
    pub what_it_is: String,
    /// What tasks this tool is used for
    pub what_it_does: String,
    /// Step-by-step technique for using this tool
    pub how_to_use: String,
    /// Safety reminders specific to this tool type
    pub safety: Vec<String>,
    /// Common mistakes to avoid
    pub common_mistakes: Vec<String>,
}

impl ToolKnowledge {
    /// Format as an AI context block
    pub fn as_context_block(&self) -> String {
        format!(
            "TOOL CLASS: {}\nWHAT IT IS: {}\nUSED FOR: {}\nHOW TO USE: {}\nSAFETY: {}\nAVOID: {}",
            self.keyword,
            self.what_it_is,
            self.what_it_does,
            self.how_to_use,
            self.safety.join("; "),
            self.common_mistakes.join("; "),
        )
    }
}

// ============================================================================
// 2. KnowledgeBase — embedded database of common tool knowledge
// ============================================================================

/// Returns the full embedded knowledge base for common workshop tool types.
/// This is a compile-time constant — no file I/O required.
pub fn embedded_knowledge_base() -> Vec<ToolKnowledge> {
    vec![
        ToolKnowledge {
            keyword: "drill".into(),
            what_it_is: "A rotary power tool that drives a rotating drill bit or screwdriver bit.".into(),
            what_it_does: "Creates holes in wood, metal, masonry, or plastic; drives and removes screws and fasteners.".into(),
            how_to_use: "Select the correct bit for the material. Set the clutch/torque collar to the appropriate setting — lower numbers for screws, higher for drilling. Hold the drill perpendicular to the surface. Apply steady forward pressure while squeezing the trigger smoothly. For pilot holes, use a bit slightly smaller than the fastener diameter.".into(),
            safety: vec![
                "Always wear safety glasses — drill chips can fly at high velocity.".into(),
                "Secure the workpiece with clamps before drilling; never hold it by hand alone.".into(),
                "Keep the bit sharp — dull bits require more force and can slip.".into(),
                "Unplug or remove the battery before changing bits.".into(),
            ],
            common_mistakes: vec![
                "Drilling without a pilot hole in hardwood causes splitting.".into(),
                "Applying too much side pressure can snap the bit.".into(),
                "Running at full speed into metal without cutting fluid causes bit overheating.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "torque wrench".into(),
            what_it_is: "A calibrated wrench that applies a precisely measured rotational force (torque) to fasteners.".into(),
            what_it_does: "Tightens bolts and nuts to a specific torque specification, preventing under-tightening (loose joints) or over-tightening (stripped threads, broken bolts).".into(),
            how_to_use: "Set the torque value by adjusting the handle scale to the manufacturer's specification. Position the socket squarely on the fastener. Turn the wrench in a smooth, steady arc. Stop immediately when you hear or feel the click (click-type) or see the needle reach the set value (beam-type). Never use a torque wrench to loosen fasteners.".into(),
            safety: vec![
                "Never exceed the wrench's maximum torque rating.".into(),
                "Store torque wrenches at minimum setting to relieve spring tension.".into(),
                "Re-calibrate annually or after any drop.".into(),
            ],
            common_mistakes: vec![
                "Jerking the wrench instead of applying smooth continuous force gives inaccurate readings.".into(),
                "Not zeroing a click-type wrench after use fatigues the spring.".into(),
                "Using an extension without accounting for the torque multiplication factor.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "circular saw".into(),
            what_it_is: "A handheld or table-mounted power saw with a rotating toothed blade for making straight cuts.".into(),
            what_it_does: "Cuts wood, sheet metal, plastic, and composite materials in straight lines — cross cuts, rip cuts, and bevel cuts.".into(),
            how_to_use: "Set the blade depth to approximately 6mm deeper than the material thickness. Mark the cut line clearly. Set the blade guard in place. Start the saw before contacting the material. Guide the baseplate along the cut line using a straightedge guide for accuracy. Let the blade do the cutting — do not force it.".into(),
            safety: vec![
                "Always wear safety glasses and hearing protection.".into(),
                "Keep both hands on the saw — use a push stick or guide for narrow cuts.".into(),
                "Never reach under the workpiece while the blade is spinning.".into(),
                "Wait for the blade to stop completely before setting the saw down.".into(),
                "Use the blade guard at all times; never tie it back.".into(),
            ],
            common_mistakes: vec![
                "Setting blade depth too deep increases kickback risk.".into(),
                "Cutting unsupported material causes the kerf to close and bind the blade.".into(),
                "Forcing a dull blade overloads the motor and creates dangerous kickback.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "caliper".into(),
            what_it_is: "A precision measuring instrument for determining the dimensions of an object to within 0.01mm or better.".into(),
            what_it_does: "Measures outer dimensions, inner dimensions, depths, and step heights of parts and materials.".into(),
            how_to_use: "Zero the caliper with jaws closed. For outer measurements, open the jaws wider than the object, close them gently against the object, and read the display. For inner measurements, use the upper jaws. For depth, extend the depth probe into the hole. Apply light consistent pressure — never force the jaws.".into(),
            safety: vec![
                "Handle precision instruments with care — dropping a caliper affects calibration.".into(),
                "Keep measuring surfaces clean and free from burrs or chips.".into(),
            ],
            common_mistakes: vec![
                "Taking measurements at an angle instead of perpendicular introduces error.".into(),
                "Not zeroing before measurement when the caliper has drift.".into(),
                "Gripping the jaws too tightly distorts the measurement.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "angle grinder".into(),
            what_it_is: "A handheld power tool with a rotating abrasive disc used for cutting, grinding, and polishing.".into(),
            what_it_does: "Cuts metal, stone, and tile; removes welds, rust, and paint; sharpens blades; and polishes surfaces depending on the disc type fitted.".into(),
            how_to_use: "Select the correct disc for the task. Fit the disc with the label facing outward and tighten with the spanner wrench. Hold the grinder with two hands. Keep the guard positioned between you and the disc. Move the grinder in smooth, controlled passes. Keep the disc moving — do not dwell in one spot.".into(),
            safety: vec![
                "Face shield AND safety glasses are mandatory — discs can shatter.".into(),
                "Gloves protect against sparks but can be caught by the disc — use cut-resistant gloves.".into(),
                "Never use a cutting disc for grinding or vice versa.".into(),
                "Never remove the guard — it is critical protection against disc failure.".into(),
                "Ensure all bystanders are clear before starting.".into(),
            ],
            common_mistakes: vec![
                "Using a worn or cracked disc — discs must be inspected before every use.".into(),
                "Cutting with the side of a cut-off disc — it is not rated for lateral load.".into(),
                "Starting the grinder while the disc is in contact with the workpiece.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "soldering iron".into(),
            what_it_is: "A handheld heating tool that melts solder to create electrical connections on circuit boards and wiring.".into(),
            what_it_does: "Joins electronic components to PCBs, repairs wire connections, and removes components via desoldering.".into(),
            how_to_use: "Set temperature to 300–370°C for standard leaded solder, 350–400°C for lead-free. Tin the tip before starting. Heat the joint (pad and lead), not the solder. Apply solder to the joint — it should flow and wick in 1–3 seconds. Remove the iron and allow the joint to solidify without movement. Clean the tip on a damp sponge or brass wire cleaner frequently.".into(),
            safety: vec![
                "Never touch the tip — it reaches 400°C and causes severe burns instantly.".into(),
                "Work in a ventilated area — solder flux fumes are harmful.".into(),
                "Use a proper iron holder; never rest a hot iron on a workbench.".into(),
                "Wash hands after handling solder — lead solder is toxic.".into(),
            ],
            common_mistakes: vec![
                "Cold solder joints occur when the joint moves before the solder solidifies.".into(),
                "Applying solder to the iron tip instead of the joint creates weak, uneven joints.".into(),
                "Overheating pads lifts them off the PCB — heat for no more than 3–5 seconds.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "mig welder".into(),
            what_it_is: "A Metal Inert Gas (MIG) arc welder that uses a continuously fed wire electrode and shielding gas to fuse metal.".into(),
            what_it_does: "Joins steel, stainless steel, and aluminium structural members, plates, and tubing in fabrication, repair, and construction.".into(),
            how_to_use: "Set wire speed and voltage per the material thickness chart on the welder. Ensure shielding gas is flowing (75/25 Ar/CO2 for steel). Position the torch at 10–15 degrees push angle, 6–10mm from the workpiece. Hold steady and move at a consistent speed — aim for a 3:1 length-to-width bead ratio. Let the weld puddle lead the arc.".into(),
            safety: vec![
                "Full face auto-darkening welding helmet is mandatory — UV/IR radiation causes arc eye.".into(),
                "Leather gloves and a welding jacket protect against spatter burns.".into(),
                "Never weld in a confined space without forced ventilation — fumes are toxic.".into(),
                "Ground clamp must be attached close to the weld zone.".into(),
                "Keep flammables well clear of the welding area.".into(),
            ],
            common_mistakes: vec![
                "Insufficient shielding gas flow causes porosity in the weld.".into(),
                "Moving too fast produces a narrow, weak bead; too slow causes burn-through.".into(),
                "Welding over rust or mill scale creates weak, contaminated joints — clean first.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "cnc router".into(),
            what_it_is: "A computer-controlled cutting machine that moves a rotating tool along X, Y, and Z axes to cut complex shapes in flat material.".into(),
            what_it_does: "Cuts, profiles, pockets, drills, and engraves wood, plastics, composites, aluminium, and foam with high precision.".into(),
            how_to_use: "Set the origin (home position) at the material corner or center. Load the toolpath G-code file. Verify the spindle speed and feed rate match the bit and material specifications. Clamp the workpiece securely — vacuum tables, screws, or tabs. Run a dry simulation (Z raised) before the first cut. Monitor the first pass and adjust if chatter or deflection occurs.".into(),
            safety: vec![
                "Never reach into the cutting envelope while the spindle is running.".into(),
                "Wear eye protection — fine chips travel far.".into(),
                "Ensure dust collection is active — wood dust is combustible.".into(),
                "Never leave the machine unattended during a cut.".into(),
            ],
            common_mistakes: vec![
                "Incorrect workholding causes the workpiece to move mid-cut, ruining it and potentially damaging the bit.".into(),
                "Running too fast a feed rate causes bit deflection and poor finish.".into(),
                "Not accounting for climb vs conventional milling direction affects surface finish and safety.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "3d printer".into(),
            what_it_is: "An additive manufacturing machine that builds three-dimensional objects layer by layer from a digital model.".into(),
            what_it_does: "Produces prototypes, jigs, fixtures, brackets, housings, and custom parts from thermoplastic filament or resin.".into(),
            how_to_use: "Level the print bed before each session. Slice the model with appropriate support structures, layer height, and infill for the part's purpose. Preheat the nozzle and bed. Start the print and monitor the first layer adhesion — it is the most critical phase. Adjust Z offset if the first layer is too thick or too thin.".into(),
            safety: vec![
                "Print in a ventilated area — melting filament produces ultrafine particles and VOCs.".into(),
                "Never leave a printer unattended for hours-long prints — fire risk from heater failures.".into(),
                "Use thermal runaway protection — always enabled in firmware.".into(),
            ],
            common_mistakes: vec![
                "Printing too fast reduces layer adhesion and causes stringing.".into(),
                "Not drying moisture-absorbent filament (PETG, Nylon, TPU) produces brittle, bubbly prints.".into(),
                "Insufficient support structures cause overhanging features to sag or fail.".into(),
            ],
        },
        ToolKnowledge {
            keyword: "impact driver".into(),
            what_it_is: "A high-torque rotary power tool that uses concussive force pulses to drive fasteners without the wrist-twisting kickback of a standard drill.".into(),
            what_it_does: "Drives long screws, lags, and bolts into wood, metal, and concrete with much higher torque than a standard drill/driver.".into(),
            how_to_use: "Use impact-rated bits — standard bits shatter under the impact forces. Set the speed/mode selector for the fastener size. Position the bit squarely on the fastener head. Squeeze the trigger — the impact mechanism activates automatically at high torque. Do not over-tighten into soft materials.".into(),
            safety: vec![
                "Only use bits rated for impact use — standard bits shatter unpredictably.".into(),
                "Eye protection is essential — bit fragments from non-impact bits can cause serious injury.".into(),
            ],
            common_mistakes: vec![
                "Using standard driver bits causes them to shatter or cam out under impact.".into(),
                "Over-driving screws in softwood strips the head or causes splitting.".into(),
            ],
        },
    ]
}

// ============================================================================
// 3. KnowledgeBase — lookup and prompt builder
// ============================================================================

/// Wrapper around the embedded knowledge entries providing lookup by keyword
pub struct KnowledgeBase {
    entries: Vec<ToolKnowledge>,
}

impl KnowledgeBase {
    /// Create a KnowledgeBase from the embedded entries
    pub fn embedded() -> Self {
        Self {
            entries: embedded_knowledge_base(),
        }
    }

    /// Find the best-matching knowledge entry for a tool name or tag list
    pub fn find_for_tool(&self, name: &str, tags: &[String]) -> Option<&ToolKnowledge> {
        let name_lower = name.to_lowercase();
        let tags_lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();

        // Exact keyword match against name first
        for entry in &self.entries {
            if name_lower.contains(&entry.keyword) {
                return Some(entry);
            }
        }

        // Then try tags
        for entry in &self.entries {
            for tag in &tags_lower {
                if tag.contains(&entry.keyword) || entry.keyword.contains(tag.as_str()) {
                    return Some(entry);
                }
            }
        }

        None
    }

    /// Build the full AI knowledge context block for all entries in the base.
    /// Used when building guide prompts to give the AI comprehensive tool knowledge.
    pub fn build_context_block(&self) -> String {
        let mut lines = vec!["=== TOOL KNOWLEDGE BASE ===".to_string()];
        for entry in &self.entries {
            lines.push(String::new());
            lines.push(entry.as_context_block());
        }
        lines.join("\n")
    }
}

// ============================================================================
// 4. Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_base_finds_drill_by_name() {
        let kb = KnowledgeBase::embedded();
        let result = kb.find_for_tool("Milwaukee M18 Drill", &[]);
        assert!(result.is_some());
        assert_eq!(result.unwrap().keyword, "drill");
    }

    #[test]
    fn knowledge_base_finds_entry_by_tag() {
        let kb = KnowledgeBase::embedded();
        let tags = vec!["cnc".to_string(), "router".to_string()];
        let result = kb.find_for_tool("ShopBot Desktop", &tags);
        assert!(result.is_some());
        assert_eq!(result.unwrap().keyword, "cnc router");
    }

    #[test]
    fn all_entries_have_non_empty_safety_notes() {
        let kb = KnowledgeBase::embedded();
        for entry in &kb.entries {
            assert!(
                !entry.safety.is_empty(),
                "Knowledge entry '{}' has no safety notes",
                entry.keyword
            );
        }
    }
}
