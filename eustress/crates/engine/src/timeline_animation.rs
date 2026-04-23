//! # Timeline Animation (Phase 2+)
//!
//! Extends the Timeline panel with keyframed + procedural animation
//! tracks. Same panel, same Stream substrate, same filter UI — each
//! track just has a *type*:
//!
//! | Track type    | What it stores                                  |
//! |---------------|-------------------------------------------------|
//! | Marker        | discrete `TimelineEvent`s (keyframe/watchpoint/ |
//! |               | breakpoint) — the original timeline semantics   |
//! | Keyframed     | ordered `AnimationKeyframe`s targeting a        |
//! |               | specific entity + property path; interpolated   |
//! |               | each frame between adjacent keyframes           |
//! | Procedural    | an expression (`=sin(t * 2) * 0.5 + 0.5`)       |
//! |               | evaluated each frame against `AnimationClock.t` |
//!
//! All three types share the tag system for grouping and the
//! filter modal for visibility. Marker tracks keep their discrete
//! yellow-diamond / orange-dot / red-asterisk rendering; animation
//! tracks render as a smooth curve in the Slint panel (follow-up UI;
//! data model + playback ship here).
//!
//! ## Supported property paths (v0)
//!
//! - `transform.translation.x` / `.y` / `.z`
//! - `transform.rotation.x` / `.y` / `.z` / `.w`  (quaternion component)
//! - `transform.rotation.euler.x/y/z`              (angle in radians)
//! - `transform.scale.x` / `.y` / `.z`
//! - `baseplate.color.r/.g/.b/.a`
//!
//! Arbitrary Reflect-path drives need Bevy Reflect introspection —
//! that follow-up reuses the existing reflected-property infrastructure
//! in `eustress-embedvec`'s ReflectPropertyEmbedder.
//!
//! ## Procedural tracks reuse the numeric_input expression evaluator
//!
//! Same parser (`=sin(t) * 0.5`), same unit + property-ref support.
//! A magic variable `t` resolves to `AnimationClock.t` (current
//! animation time in seconds). Scene-level property refs
//! (`other.x`) work too — enables procedural tracks driven by
//! other entities' state.

use bevy::prelude::*;
use std::collections::HashMap;

// ============================================================================
// Data model
// ============================================================================

/// Global animation clock. Incremented each frame when `playing`;
/// the playhead in the Slint panel reads this. User scrub snaps
/// the value directly. Looping resets at `loop_end`.
#[derive(Resource, Debug, Clone, Copy)]
pub struct AnimationClock {
    pub t: f64,
    pub playing: bool,
    pub loop_start: f64,
    pub loop_end: f64,
    pub playback_rate: f64, // 1.0 = realtime
}

impl Default for AnimationClock {
    fn default() -> Self {
        Self {
            t: 0.0,
            playing: false,
            loop_start: 0.0,
            loop_end: 10.0,
            playback_rate: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpKind {
    /// Hold — value snaps at each keyframe, no interpolation.
    Step,
    /// Straight line between keyframe values.
    Linear,
    /// Smooth cubic ease-in-out between keyframes.
    EaseInOut,
    /// Bezier with the keyframe's `(in_tangent, out_tangent)` fields.
    Bezier,
}

#[derive(Debug, Clone)]
pub struct AnimationKeyframe {
    pub time: f64,
    pub value: f64,
    /// Per-keyframe interp — determines how the segment *following*
    /// this keyframe is interpolated.
    pub interp: InterpKind,
    /// Bezier tangents — (out-tangent of this keyframe, in-tangent
    /// of next). Used only when `interp == Bezier`. Expressed as
    /// (time_delta, value_delta).
    pub out_tangent: (f64, f64),
    pub in_tangent: (f64, f64),
}

/// Which property this track drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyPath {
    TransformTranslationX,
    TransformTranslationY,
    TransformTranslationZ,
    TransformRotationEulerX,
    TransformRotationEulerY,
    TransformRotationEulerZ,
    TransformScaleX,
    TransformScaleY,
    TransformScaleZ,
    BasePartColorR,
    BasePartColorG,
    BasePartColorB,
    BasePartColorA,
}

impl PropertyPath {
    pub fn as_str(self) -> &'static str {
        match self {
            PropertyPath::TransformTranslationX => "transform.translation.x",
            PropertyPath::TransformTranslationY => "transform.translation.y",
            PropertyPath::TransformTranslationZ => "transform.translation.z",
            PropertyPath::TransformRotationEulerX => "transform.rotation.euler.x",
            PropertyPath::TransformRotationEulerY => "transform.rotation.euler.y",
            PropertyPath::TransformRotationEulerZ => "transform.rotation.euler.z",
            PropertyPath::TransformScaleX => "transform.scale.x",
            PropertyPath::TransformScaleY => "transform.scale.y",
            PropertyPath::TransformScaleZ => "transform.scale.z",
            PropertyPath::BasePartColorR => "baseplate.color.r",
            PropertyPath::BasePartColorG => "baseplate.color.g",
            PropertyPath::BasePartColorB => "baseplate.color.b",
            PropertyPath::BasePartColorA => "baseplate.color.a",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "transform.translation.x" => Self::TransformTranslationX,
            "transform.translation.y" => Self::TransformTranslationY,
            "transform.translation.z" => Self::TransformTranslationZ,
            "transform.rotation.euler.x" => Self::TransformRotationEulerX,
            "transform.rotation.euler.y" => Self::TransformRotationEulerY,
            "transform.rotation.euler.z" => Self::TransformRotationEulerZ,
            "transform.scale.x" => Self::TransformScaleX,
            "transform.scale.y" => Self::TransformScaleY,
            "transform.scale.z" => Self::TransformScaleZ,
            "baseplate.color.r" => Self::BasePartColorR,
            "baseplate.color.g" => Self::BasePartColorG,
            "baseplate.color.b" => Self::BasePartColorB,
            "baseplate.color.a" => Self::BasePartColorA,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub enum AnimationTrackKind {
    Keyframed(Vec<AnimationKeyframe>),
    /// Procedural — expression evaluated against `t` + scene refs.
    Procedural(String),
}

#[derive(Component, Debug, Clone)]
pub struct AnimationTrack {
    pub target: Entity,
    pub property: PropertyPath,
    pub kind: AnimationTrackKind,
    /// Tag used for timeline grouping. Defaults to `"animation"`.
    pub tag: String,
    pub enabled: bool,
}

impl AnimationTrack {
    /// Evaluate the track at the given time. Returns the value (in
    /// the property's native units) or None if the track has no
    /// defined value at `t` (e.g. empty keyframe list).
    pub fn eval(&self, t: f64) -> Option<f64> {
        if !self.enabled { return None; }
        match &self.kind {
            AnimationTrackKind::Keyframed(keys) => eval_keyframes(keys, t),
            AnimationTrackKind::Procedural(expr) => {
                // Substitute `t` by editing the expression string — a
                // real implementation would push `t` into the parser's
                // symbol table. For v0 we do a naive string-replace of
                // the standalone identifier `t`.
                let substituted = substitute_t(expr, t);
                crate::numeric_input::parse_expression_public(&substituted)
                    .map(|v| v as f64)
            }
        }
    }
}

// ============================================================================
// Evaluation
// ============================================================================

fn eval_keyframes(keys: &[AnimationKeyframe], t: f64) -> Option<f64> {
    if keys.is_empty() { return None; }
    if t <= keys[0].time { return Some(keys[0].value); }
    if t >= keys.last().unwrap().time {
        return Some(keys.last().unwrap().value);
    }
    // Find the segment containing t.
    for pair in keys.windows(2) {
        let a = &pair[0];
        let b = &pair[1];
        if t >= a.time && t <= b.time {
            let span = (b.time - a.time).max(1e-9);
            let u = (t - a.time) / span;
            return Some(match a.interp {
                InterpKind::Step => a.value,
                InterpKind::Linear => a.value + (b.value - a.value) * u,
                InterpKind::EaseInOut => {
                    let eased = if u < 0.5 {
                        2.0 * u * u
                    } else {
                        1.0 - (-2.0 * u + 2.0).powi(2) * 0.5
                    };
                    a.value + (b.value - a.value) * eased
                }
                InterpKind::Bezier => {
                    // Cubic Bezier with control points derived from
                    // tangents — standard keyframe-curve evaluation.
                    let p0 = a.value;
                    let p3 = b.value;
                    let p1 = p0 + a.out_tangent.1;
                    let p2 = p3 - b.in_tangent.1;
                    let one_u = 1.0 - u;
                    one_u.powi(3) * p0
                        + 3.0 * one_u.powi(2) * u * p1
                        + 3.0 * one_u * u.powi(2) * p2
                        + u.powi(3) * p3
                }
            });
        }
    }
    None
}

fn substitute_t(expr: &str, t: f64) -> String {
    // Replace standalone `t` tokens with the numeric value.
    // Naive: split on non-alphanumeric boundaries, swap matching
    // tokens. Correct for the common cases (`sin(t)`, `t * 2`,
    // `t + other.x`); edge cases with `t` embedded in identifiers
    // (e.g. `step.t_value`) would land with a proper parse.
    let mut out = String::with_capacity(expr.len());
    let mut chars = expr.chars().peekable();
    let mut prev_alnum = false;
    while let Some(c) = chars.next() {
        if c == 't' {
            let next_alnum = chars.peek()
                .map(|n| n.is_ascii_alphanumeric() || *n == '_')
                .unwrap_or(false);
            if !prev_alnum && !next_alnum {
                // Standalone `t` — substitute.
                out.push_str(&format!("({:.6})", t));
                prev_alnum = false;
                continue;
            }
        }
        out.push(c);
        prev_alnum = c.is_ascii_alphanumeric() || c == '_';
    }
    out
}

// ============================================================================
// Playback system
// ============================================================================

pub struct TimelineAnimationPlugin;

impl Plugin for TimelineAnimationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationClock>()
            .add_systems(Update, (
                tick_animation_clock,
                apply_animation_tracks,
            ).chain());
    }
}

fn tick_animation_clock(time: Res<Time>, mut clock: ResMut<AnimationClock>) {
    if !clock.playing { return; }
    clock.t += (time.delta_secs() as f64) * clock.playback_rate;
    if clock.loop_end > clock.loop_start && clock.t >= clock.loop_end {
        let span = clock.loop_end - clock.loop_start;
        clock.t = clock.loop_start + ((clock.t - clock.loop_start) % span);
    }
}

fn apply_animation_tracks(
    clock: Res<AnimationClock>,
    tracks: Query<&AnimationTrack>,
    mut transforms: Query<&mut Transform>,
    mut parts: Query<&mut crate::classes::BasePart>,
) {
    // Group tracks by target entity so we can apply all per-entity
    // properties in one mutate.
    let mut per_entity: HashMap<Entity, Vec<(&AnimationTrack, f64)>> = HashMap::new();
    for track in tracks.iter() {
        if let Some(v) = track.eval(clock.t) {
            per_entity.entry(track.target).or_default().push((track, v));
        }
    }

    for (entity, updates) in per_entity {
        // Transform properties.
        let has_transform_update = updates.iter().any(|(t, _)| matches!(
            t.property,
            PropertyPath::TransformTranslationX | PropertyPath::TransformTranslationY | PropertyPath::TransformTranslationZ
            | PropertyPath::TransformRotationEulerX | PropertyPath::TransformRotationEulerY | PropertyPath::TransformRotationEulerZ
            | PropertyPath::TransformScaleX | PropertyPath::TransformScaleY | PropertyPath::TransformScaleZ
        ));
        if has_transform_update {
            if let Ok(mut t) = transforms.get_mut(entity) {
                let mut euler = t.rotation.to_euler(EulerRot::XYZ);
                let (mut ex, mut ey, mut ez) = (euler.0, euler.1, euler.2);
                for (track, v) in &updates {
                    let v = *v as f32;
                    match track.property {
                        PropertyPath::TransformTranslationX => t.translation.x = v,
                        PropertyPath::TransformTranslationY => t.translation.y = v,
                        PropertyPath::TransformTranslationZ => t.translation.z = v,
                        PropertyPath::TransformRotationEulerX => ex = v,
                        PropertyPath::TransformRotationEulerY => ey = v,
                        PropertyPath::TransformRotationEulerZ => ez = v,
                        PropertyPath::TransformScaleX => t.scale.x = v,
                        PropertyPath::TransformScaleY => t.scale.y = v,
                        PropertyPath::TransformScaleZ => t.scale.z = v,
                        _ => {}
                    }
                }
                t.rotation = Quat::from_euler(EulerRot::XYZ, ex, ey, ez);
                euler = (ex, ey, ez); let _ = euler;
            }
        }

        // BasePart.color channels.
        let has_color_update = updates.iter().any(|(t, _)| matches!(
            t.property,
            PropertyPath::BasePartColorR | PropertyPath::BasePartColorG
            | PropertyPath::BasePartColorB | PropertyPath::BasePartColorA
        ));
        if has_color_update {
            if let Ok(mut bp) = parts.get_mut(entity) {
                let mut c = bp.color.to_srgba();
                for (track, v) in &updates {
                    let v = *v as f32;
                    match track.property {
                        PropertyPath::BasePartColorR => c.red = v,
                        PropertyPath::BasePartColorG => c.green = v,
                        PropertyPath::BasePartColorB => c.blue = v,
                        PropertyPath::BasePartColorA => c.alpha = v,
                        _ => {}
                    }
                }
                bp.color = Color::Srgba(c);
            }
        }
    }
}
