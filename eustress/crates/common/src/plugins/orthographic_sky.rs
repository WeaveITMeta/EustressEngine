//! # Orthographic sky rays
//!
//! Bevy 0.19's sky shaders build each pixel's ray from the camera's position
//! through the pixel's point on the near plane
//! (`view_from_clip * vec4(ndc, 1.0, 1.0)`): the atmosphere in
//! `bevy_pbr::atmosphere::functions::uv_to_ray_direction`, the skybox in
//! `coords_to_ray_direction`. That holds for a perspective projection only.
//! An orthographic projection's rays are parallel, and with its near plane
//! through the camera the near-plane points lie in the view plane itself, so
//! every "ray" pointed sideways out of the screen centre: each orthographic
//! view drew a horizon through the middle of the screen that followed the
//! camera as it panned, with the sun and the atmosphere's shadows smeared into
//! streaks converging on it.
//!
//! Upstream has no hook for this, so [`patch_sky_shaders`] edits the loaded
//! shader source. Each function below gains an early return for an
//! orthographic view (`view.clip_from_view[3].w == 1.0`, the test bevy's own
//! shaders use); a perspective view runs bevy's code unchanged.
//!
//! | function | orthographic behaviour |
//! |---|---|
//! | `uv_to_ray_direction` (atmosphere) | the camera's forward axis, for every pixel |
//! | `ndc_to_camera_dist` (atmosphere) | zero, so geometry gets no aerial haze |
//! | `sample_sun_radiance` (atmosphere) | no sun disk: a parallel view of a half-degree disk is nothing or the whole screen |
//! | `coords_to_ray_direction` (skybox) | the camera's forward axis, through the skybox rotation |
//!
//! Haze needs a viewer position to measure from, and a parallel view has
//! none. Measuring from the camera would haze an orthographic view more the
//! further it is zoomed out, since the editor camera backs away as the view
//! widens, so orthographic views show the scene's true colours against the
//! sky seen straight ahead.
//!
//! The patch finds functions by name. If a bevy upgrade renames one, the
//! shader is left exactly as shipped and a warning names the missing function.

use bevy::prelude::*;
use bevy::shader::{Shader, ShaderImport, Source};
use tracing::{info, warn};

/// Written into every patched function. Its presence means a shader is
/// already patched, which keeps the `Modified` event the patch itself
/// raises from patching again.
const MARKER: &str = "// eustress: orthographic sky";

/// Code inserted at the top of one function's body.
struct EarlyReturn {
    function: &'static str,
    code: &'static str,
}

/// A bevy shader to patch.
struct SkyShader {
    name: &'static str,
    matches: fn(&Shader) -> bool,
    returns: &'static [EarlyReturn],
}

const SKY_SHADERS: &[SkyShader] = &[
    SkyShader {
        name: "bevy_pbr::atmosphere::functions",
        matches: |shader| {
            matches!(&shader.import_path, ShaderImport::Custom(path) if path == "bevy_pbr::atmosphere::functions")
        },
        returns: &[
            EarlyReturn {
                function: "uv_to_ray_direction",
                code: "if view.clip_from_view[3].w == 1.0 {
        return normalize((view.world_from_view * vec4(0.0, 0.0, -1.0, 0.0)).xyz);
    }",
            },
            EarlyReturn {
                function: "ndc_to_camera_dist",
                code: "if view.clip_from_view[3].w == 1.0 {
        return 0.0;
    }",
            },
            EarlyReturn {
                function: "sample_sun_radiance",
                code: "if view.clip_from_view[3].w == 1.0 {
        return vec3(0.0);
    }",
            },
        ],
    },
    SkyShader {
        name: "bevy_core_pipeline skybox.wgsl",
        matches: |shader| shader.path.replace('\\', "/").ends_with("skybox/skybox.wgsl"),
        returns: &[EarlyReturn {
            function: "coords_to_ray_direction",
            code: "if view.clip_from_view[3].w == 1.0 {
        let forward = (view.world_from_view * vec4(0.0, 0.0, -1.0, 0.0)).xyz;
        return normalize((uniforms.transform * vec4(forward, 0.0)).xyz);
    }",
        }],
    },
];

/// Insert each early return at the top of its function. `Err` names the
/// first function not found, and nothing is changed.
fn patch_source(source: &str, returns: &[EarlyReturn]) -> Result<String, &'static str> {
    let mut patched = source.to_owned();
    for early in returns {
        let start = patched
            .find(&format!("fn {}(", early.function))
            .ok_or(early.function)?;
        let body = start + patched[start..].find('{').ok_or(early.function)? + 1;
        patched.insert_str(body, &format!("\n    {MARKER}\n    {}", early.code));
    }
    Ok(patched)
}

/// Patch the sky shaders as they load. The first run also sweeps the shaders
/// already loaded, so a shader that finished loading before this system
/// first ran is still patched.
pub fn patch_sky_shaders(
    mut events: MessageReader<AssetEvent<Shader>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut swept: Local<bool>,
) {
    let mut candidates: Vec<AssetId<Shader>> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    if !*swept {
        *swept = true;
        candidates.extend(shaders.ids());
    }

    for id in candidates {
        let Some(shader) = shaders.get(id) else { continue };
        let Some(target) = SKY_SHADERS.iter().find(|target| (target.matches)(shader)) else {
            continue;
        };
        let Source::Wgsl(source) = &shader.source else { continue };
        if source.contains(MARKER) {
            continue;
        }
        match patch_source(source, target.returns) {
            Ok(patched) => {
                if let Some(mut shader) = shaders.get_mut(id) {
                    shader.source = Source::Wgsl(patched.into());
                }
                info!("Orthographic sky: patched {}", target.name);
            }
            Err(function) => warn!(
                "Orthographic sky: `fn {function}` not found in {}; orthographic views keep bevy's perspective sky rays",
                target.name
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "fn uv_to_ray_direction(uv: vec2<f32>) -> vec3<f32> {
    return vec3(uv, 1.0);
}

fn ndc_to_camera_dist(ndc: vec3<f32>) -> f32 {
    return ndc.z;
}

fn sample_sun_radiance(ray_dir_ws: vec3<f32>) -> vec3<f32> {
    return ray_dir_ws;
}
";

    #[test]
    fn early_returns_open_each_function_body() {
        let patched = patch_source(SOURCE, SKY_SHADERS[0].returns).unwrap();
        for early in SKY_SHADERS[0].returns {
            let head = format!("fn {}(", early.function);
            let start = patched.find(&head).unwrap();
            let body = &patched[start..];
            let brace = body.find('{').unwrap();
            assert!(
                body[brace + 1..].trim_start().starts_with(MARKER),
                "{} does not open with the patch",
                early.function
            );
        }
        // Bevy's own statements survive, after the early returns.
        assert!(patched.contains("return vec3(uv, 1.0);"));
        assert!(patched.contains("return ndc.z;"));
        assert_eq!(patched.matches(MARKER).count(), 3);
    }

    #[test]
    fn missing_function_leaves_the_source_alone() {
        let renamed = SOURCE.replace("fn ndc_to_camera_dist(", "fn ndc_to_view_dist(");
        assert_eq!(
            patch_source(&renamed, SKY_SHADERS[0].returns),
            Err("ndc_to_camera_dist")
        );
    }

    #[test]
    fn inserted_code_balances_its_braces() {
        for early in SKY_SHADERS.iter().flat_map(|shader| shader.returns) {
            assert_eq!(
                early.code.matches('{').count(),
                early.code.matches('}').count(),
                "{}",
                early.function
            );
        }
    }
}
