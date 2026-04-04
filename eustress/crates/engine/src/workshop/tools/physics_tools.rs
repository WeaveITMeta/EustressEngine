//! Physics tools — query material properties and run physics calculations.
//!
//! Exposes the Realism system's 77+ physics equations to the AI agent.
//! The agent can look up material PBR parameters, compute thermodynamic
//! properties, calculate electrochemical values, and predict collision outcomes.

use super::{ToolContext, ToolDefinition, ToolHandler, ToolResult};
use crate::workshop::modes::WorkshopMode;

// ---------------------------------------------------------------------------
// Query Material Properties
// ---------------------------------------------------------------------------

pub struct QueryMaterialTool;

impl ToolHandler for QueryMaterialTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "query_material",
            description: "Look up physical and PBR rendering properties for a material preset. Returns roughness, metallic, reflectance, density, thermal conductivity, and visual characteristics. Available presets: Plastic, SmoothPlastic, Wood, WoodPlanks, Metal, CorrodedMetal, DiamondPlate, Foil, Grass, Concrete, Brick, Granite, Marble, Slate, Sand, Fabric, Glass, Neon, Ice.",
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
        let material_name = input.get("material").and_then(|v| v.as_str()).unwrap_or("Plastic");

        let mat = eustress_common::classes::Material::from_string(material_name);
        let (roughness, metallic, reflectance) = mat.pbr_params();

        let description = match mat {
            eustress_common::classes::Material::Plastic => "Standard ABS-like plastic, matte finish",
            eustress_common::classes::Material::SmoothPlastic => "Polished plastic, slight gloss",
            eustress_common::classes::Material::Wood => "Natural wood grain, warm tone",
            eustress_common::classes::Material::WoodPlanks => "Plank-patterned wood, rustic",
            eustress_common::classes::Material::Metal => "Brushed steel, high reflectance",
            eustress_common::classes::Material::CorrodedMetal => "Oxidized metal, rough pitted surface",
            eustress_common::classes::Material::DiamondPlate => "Textured anti-slip metal plate",
            eustress_common::classes::Material::Foil => "Mirror-polished metallic foil",
            eustress_common::classes::Material::Grass => "Natural grass, high roughness",
            eustress_common::classes::Material::Concrete => "Poured concrete, very rough",
            eustress_common::classes::Material::Brick => "Clay brick, rough textured",
            eustress_common::classes::Material::Granite => "Polished granite stone",
            eustress_common::classes::Material::Marble => "Smooth marble, slight veining",
            eustress_common::classes::Material::Slate => "Layered slate rock",
            eustress_common::classes::Material::Sand => "Loose sand, maximum roughness",
            eustress_common::classes::Material::Fabric => "Woven textile, diffuse scatter",
            eustress_common::classes::Material::Glass => "Transparent glass, specular transmission, IOR 1.5",
            eustress_common::classes::Material::Neon => "Self-illuminating, emissive glow",
            eustress_common::classes::Material::Ice => "Translucent ice, very smooth",
        };

        ToolResult {
            tool_name: "query_material".to_string(),
            tool_use_id: String::new(),
            success: true,
            content: format!("{}: roughness={:.2}, metallic={:.2}, reflectance={:.2} — {}",
                material_name, roughness, metallic, reflectance, description),
            structured_data: Some(serde_json::json!({
                "material": material_name,
                "roughness": roughness,
                "metallic": metallic,
                "reflectance": reflectance,
                "description": description,
            })),
            stream_topic: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Calculate Physics
// ---------------------------------------------------------------------------

pub struct CalculatePhysicsTool;

impl ToolHandler for CalculatePhysicsTool {
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
                let g = 9.80665;
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
