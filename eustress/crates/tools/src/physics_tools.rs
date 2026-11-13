//! Physics tools — query material properties and run physics calculations.
//!
//! Exposes the Realism system's 77+ physics equations to the AI agent.
//! The agent can look up material PBR parameters, compute thermodynamic
//! properties, calculate electrochemical values, and predict collision outcomes.

use crate::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Query Material Properties
// ---------------------------------------------------------------------------

pub struct QueryMaterialTool;

impl ToolHandler for QueryMaterialTool {
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_material",
            description: "Look up the rendering AND mechanical properties of a material preset. Returns roughness/metallic/reflectance for appearance, plus young_modulus, yield_strength, ultimate_strength, fracture_toughness (K_IC), hardness and density for physics. Available presets: Plastic, SmoothPlastic, Wood, WoodPlanks, Metal, CorrodedMetal, DiamondPlate, Foil, Grass, Concrete, Brick, Granite, Marble, Slate, Sand, Fabric, Glass, Neon, Ice. \
                          DESTRUCTIBILITY: a Part dents and cracks only when its `deformation` property is true. It then takes these mechanical constants from its `material` name automatically, so setting an accurate material is normally all that is needed and no numbers have to be supplied. Whether an impact cracks a part rather than denting it is decided by fracture energy G_c = fracture_toughness^2 / young_modulus x smallest cross-section area: Concrete (K_IC 1e6) shatters easily, Metal (5e7) essentially never does. Dent DEPTH is set by yield_strength. Override these with an explicit [material] block only for something the presets do not cover — the crack threshold goes as K_IC SQUARED, so a guessed value silently flips shatters into never-cracks.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "material": { "type": "string", "description": "Material preset name" }
                },
                "required": ["material"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Manufacturing, WorkshopMode::Fabrication],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let material_name = input
            .get("material")
            .and_then(|v| v.as_str())
            .unwrap_or("Plastic");

        // Pure-data lookup so the shared crate stays Bevy-free. These
        // PBR values mirror `eustress_common::classes::Material::pbr_params`
        // as of 2026-04 — when the engine's table changes, update
        // the table below in lock-step. The engine's own PBR path is
        // still the canonical source for runtime rendering; this copy
        // exists only for tool introspection.
        //
        // The lookup is case-insensitive (`gold` finds `Gold`). A name with no
        // appearance entry used to fall through to the Plastic row with
        // `success: true`, so `gold` in the wrong case reported `metallic: 0`
        // and was indistinguishable from a real match. A name with neither an
        // appearance entry nor a mechanical preset is now a failure that lists
        // the known presets; one with only a mechanical preset (`Steel`,
        // `Rubber`) reports the mechanical data and no appearance numbers.
        let pbr = pbr_entry(material_name);
        let mech_known = eustress_common::realism::materials::properties::MaterialProperties
            ::from_name(material_name)
            .is_some();
        if pbr.is_none() && !mech_known {
            return ToolResult {
                tool_name: "query_material".to_string(),
                tool_use_id: String::new(),
                success: false,
                content: format!(
                    "Unknown material preset '{}'. Known presets: {}",
                    material_name,
                    KNOWN_MATERIALS.join(", "),
                ),
                structured_data: Some(serde_json::json!({
                    "material": material_name,
                    "known_presets": KNOWN_MATERIALS,
                })),
                stream_topic: None,
            };
        }
        let (roughness, metallic, reflectance, description) = match pbr {
            Some((r, m, f, d)) => (Some(r), Some(m), Some(f), d),
            None => (None, None, None, "no appearance entry for this name"),
        };
        // Round to 3 decimals before JSON emission. The table stores
        // f32 (cheap, close to the rendering path's precision), but
        // serde_json promotes f32 → f64 literally, so `0.3_f32`
        // serialized as f64 became `0.30000001192092896` in the tool
        // response. Rounding collapses the ULP noise into the
        // human-readable value while preserving enough resolution for
        // downstream math.
        let round3 = |v: f32| ((v as f64) * 1000.0).round() / 1000.0;
        let r = roughness.map(round3);
        let m = metallic.map(round3);
        let f = reflectance.map(round3);

        // Mechanical constants come from the SAME lookup the engine uses to
        // give a deformable part its physics, so what an agent is told here is
        // exactly what it will get at runtime. Reporting appearance alone —
        // which is all this returned before — left the agent no way to reach
        // the numbers that decide crack-vs-dent except by inventing them.
        let mech = eustress_common::realism::materials::properties::MaterialProperties
            ::from_name(material_name);

        let mech_json = mech.as_ref().map(|p| {
            serde_json::json!({
                "preset": p.name,
                "young_modulus_pa": p.young_modulus,
                "poisson_ratio": p.poisson_ratio,
                "yield_strength_pa": p.yield_strength,
                "ultimate_strength_pa": p.ultimate_strength,
                "fracture_toughness_pa_sqrt_m": p.fracture_toughness,
                "hardness": p.hardness,
                "density_kg_m3": p.density,
            })
        });

        let mech_text = mech
            .as_ref()
            .map(|p| {
                format!(
                    " | mechanical: E={:.3e}Pa, yield={:.3e}Pa, K_IC={:.3e}Pa·√m, density={:.0}kg/m³ \
                     (used automatically when deformation = true)",
                    p.young_modulus, p.yield_strength, p.fracture_toughness, p.density
                )
            })
            .unwrap_or_else(|| {
                " | no mechanical preset for this name — a deformable part using it needs an \
                  explicit [material] block"
                    .to_string()
            });

        ToolResult {
            tool_name: "query_material".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: match (roughness, metallic, reflectance) {
                (Some(ro), Some(me), Some(re)) => format!(
                    "{}: roughness={:.2}, metallic={:.2}, reflectance={:.2} — {}{}",
                    material_name, ro, me, re, description, mech_text
                ),
                _ => format!("{}: {}{}", material_name, description, mech_text),
            },
            structured_data: Some(serde_json::json!({
                "material": material_name,
                "roughness": r,
                "metallic": m,
                "reflectance": f,
                "description": description,
                "mechanical": mech_json,
                "destructible_requires": "Part property `deformation = true`",
            })),
            stream_topic: None,
        }
    }
}

/// PBR material table — lookup mirroring
/// `eustress_common::classes::Material::pbr_params`. Kept inline here
/// so the shared `eustress-tools` crate stays Bevy-free. Case-insensitive.
/// `None` for a name with no appearance entry; the caller decides how to
/// report that.
fn pbr_entry(name: &str) -> Option<(f32, f32, f32, &'static str)> {
    Some(match name.to_ascii_lowercase().as_str() {
        "plastic"        => (0.80, 0.00, 0.50, "Standard ABS-like plastic, matte finish"),
        "smoothplastic"  => (0.40, 0.00, 0.50, "Polished plastic, slight gloss"),
        "wood"           => (0.80, 0.00, 0.30, "Natural wood grain, warm tone"),
        "woodplanks"     => (0.85, 0.00, 0.30, "Plank-patterned wood, rustic"),
        "metal"          => (0.30, 1.00, 0.70, "Brushed steel, high reflectance"),
        "corrodedmetal"  => (0.70, 0.80, 0.50, "Oxidized metal, rough pitted surface"),
        "diamondplate"   => (0.50, 1.00, 0.60, "Textured anti-slip metal plate"),
        "foil"           => (0.10, 1.00, 0.90, "Mirror-polished metallic foil"),
        "grass"          => (1.00, 0.00, 0.20, "Natural grass, high roughness"),
        "concrete"       => (0.95, 0.00, 0.30, "Poured concrete, very rough"),
        "brick"          => (0.90, 0.00, 0.30, "Clay brick, rough textured"),
        "granite"        => (0.40, 0.00, 0.40, "Polished granite stone"),
        "marble"         => (0.20, 0.00, 0.50, "Smooth marble, slight veining"),
        "slate"          => (0.70, 0.00, 0.30, "Layered slate rock"),
        "sand"           => (1.00, 0.00, 0.20, "Loose sand, maximum roughness"),
        "fabric"         => (1.00, 0.00, 0.20, "Woven textile, diffuse scatter"),
        "glass"          => (0.05, 0.00, 0.80, "Transparent glass, specular transmission, IOR 1.5"),
        "neon"           => (0.30, 0.00, 0.50, "Self-illuminating, emissive glow"),
        "ice"            => (0.10, 0.00, 0.60, "Translucent ice, very smooth"),
        "gold"           => (0.15, 1.00, 0.95, "Pure gold, highly reflective, dense 19300 kg/m³"),
        "silver"         => (0.10, 1.00, 0.97, "Polished silver, highest reflectance of any metal"),
        "bronze"         => (0.35, 0.90, 0.70, "Copper-tin alloy, warm patina, medium roughness"),
        _                => return None,
    })
}

const KNOWN_MATERIALS: &[&str] = &[
    "Plastic", "SmoothPlastic", "Wood", "WoodPlanks", "Metal", "CorrodedMetal",
    "DiamondPlate", "Foil", "Grass", "Concrete", "Brick", "Granite", "Marble",
    "Slate", "Sand", "Fabric", "Glass", "Neon", "Ice", "Gold", "Silver", "Bronze",
];

// ---------------------------------------------------------------------------
// Calculate Physics
// ---------------------------------------------------------------------------

pub struct CalculatePhysicsTool;

impl ToolHandler for CalculatePhysicsTool {
    /// Read-only: queries only, writes nothing.
    fn read_only(&self) -> bool {
        true
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "calculate_physics",
            description: "Run a physics equation from the Realism system. Available equations: ideal_gas_pressure (P=nRT/V), kinetic_energy (0.5*m*v^2), gravitational_force (G*m1*m2/r^2), heat_transfer_conduction (k*A*dT/L), nernst_potential (E0 - RT/nF * ln(Q)), escape_velocity (sqrt(2*G*M/r)), spring_force (-k*x), drag_force (0.5*Cd*rho*A*v^2), buoyancy_force (rho*g*V). Pass named parameters matching the equation.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "equation": { "type": "string", "description": "Equation name from the list above" },
                    "params": { "type": "object", "description": "Named parameters for the equation (e.g. {\"mass\": 10.0, \"velocity\": 5.0} for kinetic_energy)" }
                },
                "required": ["equation", "params"]
            }),
            modes: &[WorkshopMode::General, WorkshopMode::Simulation, WorkshopMode::Manufacturing],
            requires_approval: false,
            stream_topics: &[],
        }
    }

    fn execute(&self, input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        let equation = input.get("equation").and_then(|v| v.as_str()).unwrap_or("");
        let params = input.get("params").cloned().unwrap_or(serde_json::json!({}));

        let get_f64 = |key: &str| -> f64 {
            params.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0)
        };

        let (result, formula) = match equation {
            "kinetic_energy" => {
                let m = get_f64("mass");
                let v = get_f64("velocity");
                (0.5 * m * v * v, format!("KE = 0.5 * {:.2} * {:.2}^2", m, v))
            }
            "gravitational_force" => {
                let m1 = get_f64("mass1");
                let m2 = get_f64("mass2");
                let r = get_f64("distance").max(0.001);
                let g = 6.674e-11;
                (g * m1 * m2 / (r * r), format!("F = G * {:.2} * {:.2} / {:.2}^2", m1, m2, r))
            }
            "ideal_gas_pressure" => {
                let n = get_f64("moles");
                let t = get_f64("temperature_k");
                let v = get_f64("volume_m3").max(0.001);
                let r = 8.314;
                (n * r * t / v, format!("P = {:.2} * R * {:.2} / {:.4}", n, t, v))
            }
            "heat_transfer_conduction" => {
                let k = get_f64("conductivity");
                let a = get_f64("area_m2");
                let dt = get_f64("temperature_diff_k");
                let l = get_f64("thickness_m").max(0.001);
                (k * a * dt / l, format!("Q = {:.2} * {:.4} * {:.2} / {:.4}", k, a, dt, l))
            }
            "spring_force" => {
                let k = get_f64("spring_constant");
                let x = get_f64("displacement");
                (-k * x, format!("F = -{:.2} * {:.4}", k, x))
            }
            "drag_force" => {
                let cd = get_f64("drag_coefficient");
                let rho = get_f64("fluid_density");
                let a = get_f64("cross_section_area");
                let v = get_f64("velocity");
                (0.5 * cd * rho * a * v * v, format!("Fd = 0.5 * {:.2} * {:.2} * {:.4} * {:.2}^2", cd, rho, a, v))
            }
            "buoyancy_force" => {
                let rho = get_f64("fluid_density");
                let v = get_f64("displaced_volume");
                let g = eustress_common::units::STANDARD_GRAVITY;
                (rho * g * v, format!("Fb = {:.2} * g * {:.6}", rho, v))
            }
            "escape_velocity" => {
                let m = get_f64("body_mass");
                let r = get_f64("radius").max(0.001);
                let g = 6.674e-11;
                ((2.0 * g * m / r).sqrt(), format!("v_esc = sqrt(2 * G * {:.2e} / {:.2})", m, r))
            }
            "nernst_potential" => {
                let e0 = get_f64("standard_potential");
                let t = get_f64("temperature_k").max(1.0);
                let n = get_f64("electron_count").max(1.0);
                let q = get_f64("reaction_quotient").max(1e-30);
                let r = 8.314;
                let f = 96485.0;
                (e0 - (r * t / (n * f)) * q.ln(), format!("E = {:.3} - (RT/{}F) * ln({:.4})", e0, n, q))
            }
            _ => {
                return ToolResult {
                    tool_name: "calculate_physics".to_string(),
                    tool_use_id: String::new(),
                    success: false,
                    content: format!("Unknown equation '{}'. Available: kinetic_energy, gravitational_force, ideal_gas_pressure, heat_transfer_conduction, spring_force, drag_force, buoyancy_force, escape_velocity, nernst_potential", equation),
                    structured_data: None,
                    stream_topic: None,
                };
            }
        };

        ToolResult {
            tool_name: "calculate_physics".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("{}\n{} = {:.6}", formula, equation, result),
            structured_data: Some(serde_json::json!({
                "equation": equation,
                "result": result,
                "formula": formula,
                "params": params,
            })),
            stream_topic: None,
        }
    }
}

#[cfg(test)]
mod pbr_tests {
    use super::*;

    #[test]
    fn pbr_entry_ignores_case() {
        let pascal = pbr_entry("Gold").expect("Gold");
        assert_eq!(pbr_entry("gold"), Some(pascal));
        assert_eq!(pbr_entry("GOLD"), Some(pascal));
        assert_eq!(pascal.1, 1.00);
    }

    #[test]
    fn pbr_entry_is_none_for_an_unknown_name() {
        assert_eq!(pbr_entry("tungsten"), None);
    }

    #[test]
    fn every_known_material_resolves() {
        for name in KNOWN_MATERIALS {
            assert!(pbr_entry(name).is_some(), "{name} is listed but has no row");
        }
    }
}
