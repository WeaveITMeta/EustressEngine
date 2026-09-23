//! `ParticleEmitter` in Play.
//!
//! Particles are simulated here on the CPU and drawn by the instanced
//! particle renderer ([`crate::particles::render`]) as small lit spheres, one
//! draw for every particle in the session. An emitter under a part emits from
//! random points in the part's box; one under an Attachment emits from the
//! attachment. `Emit(n)` bursts and `Rate` streams both work, and each
//! particle follows the emitter's `Color` and `Size` sequences over its life.
//!
//! The spheres are opaque, so `Transparency` shrinks a particle instead of
//! fading it (fully transparent = gone), and `LightEmission` brightens it.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::color::Mix;
use bevy::prelude::*;

use eustress_common::datamodel::{is_base_part, DataModel, DmValue, InstanceId};
use eustress_common::realism::particle_sim::{ParticleCloud, ParticleInstance};

use super::{DataModelSpawned, PlayDataModel};

/// Particles alive at once, across every emitter.
const MAX_PARTICLES: usize = 6000;
/// Particles one emitter may add in a frame (a huge `Rate` or `Emit(n)`).
const MAX_BURST: u32 = 600;

/// What a particle looks like over its life, shared by one emission.
struct Style {
    color: Vec<(f32, LinearRgba)>,
    size: Vec<(f32, f32)>,
    transparency: Vec<(f32, f32)>,
    acceleration: Vec3,
    drag: f32,
    glow: f32,
}

struct Particle {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    life: f32,
    style: Arc<Style>,
}

/// The session's particles and the entity that draws them.
#[derive(Resource)]
pub struct PlayParticles {
    particles: Vec<Particle>,
    /// Fractional particles owed to each streaming emitter.
    owed: HashMap<InstanceId, f32>,
    emitters: Vec<InstanceId>,
    /// `DataModel::structure_version` the emitter list was built at.
    emitters_at: u64,
    cloud: Option<Entity>,
    revision: u64,
    rng: u64,
}

impl Default for PlayParticles {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            owed: HashMap::new(),
            emitters: Vec::new(),
            emitters_at: u64::MAX,
            cloud: None,
            revision: 0,
            rng: 0x2545_F491_4F6C_DD1D,
        }
    }
}

impl PlayParticles {
    fn next(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// Where an emitter emits from: a pose and the box around it.
struct Source {
    transform: Transform,
    extent: Vec3,
}

fn source_of(g: &DataModel, emitter: InstanceId) -> Option<Source> {
    let parent = g.parent(emitter)?;
    let inst = g.get(parent)?;
    if is_base_part(&inst.class_name) {
        let size = g.get_prop(parent, "Size").and_then(|v| v.as_vector3()).map(|v| v.to_vec3()).unwrap_or(Vec3::ONE);
        return Some(Source { transform: inst.cframe()?.to_transform(), extent: size });
    }
    if inst.class_name == "Attachment" {
        let part = g.get(g.parent(parent)?)?;
        let local = g.get_prop(parent, "CFrame").and_then(|v| v.as_cframe()).unwrap_or_default().to_transform();
        return Some(Source { transform: part.cframe()?.to_transform().mul_transform(local), extent: Vec3::ZERO });
    }
    None
}

fn number(g: &DataModel, id: InstanceId, prop: &str, default: f32) -> f32 {
    g.get_prop(id, prop).and_then(|v| v.as_number()).map_or(default, |n| n as f32)
}

fn number_range(g: &DataModel, id: InstanceId, prop: &str, default: (f32, f32)) -> (f32, f32) {
    match g.get_prop(id, prop) {
        Some(DmValue::NumberRange(r)) => (r.min as f32, r.max as f32),
        Some(DmValue::Number(n)) => (n as f32, n as f32),
        _ => default,
    }
}

fn number_sequence(g: &DataModel, id: InstanceId, prop: &str, default: f32) -> Vec<(f32, f32)> {
    match g.get_prop(id, prop) {
        Some(DmValue::NumberSequence(keys)) if !keys.is_empty() => {
            keys.iter().map(|(t, v)| (*t as f32, *v as f32)).collect()
        }
        Some(DmValue::Number(n)) => vec![(0.0, n as f32)],
        _ => vec![(0.0, default)],
    }
}

fn linear(c: &eustress_common::scripting::Color3) -> LinearRgba {
    Color::srgb(c.r as f32, c.g as f32, c.b as f32).to_linear()
}

fn color_sequence(g: &DataModel, id: InstanceId) -> Vec<(f32, LinearRgba)> {
    match g.get_prop(id, "Color") {
        Some(DmValue::ColorSequence(keys)) if !keys.is_empty() => {
            keys.iter().map(|(t, c)| (*t as f32, linear(c))).collect()
        }
        Some(DmValue::Color3(c)) => vec![(0.0, linear(&c))],
        _ => vec![(0.0, LinearRgba::WHITE)],
    }
}

/// Keypoint interpolation at `t` in 0..1.
fn sample<T: Copy>(keys: &[(f32, T)], t: f32, lerp: impl Fn(T, T, f32) -> T) -> T {
    let first = keys[0];
    if t <= first.0 || keys.len() == 1 {
        return first.1;
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.0 {
            let span = (b.0 - a.0).max(1.0e-6);
            return lerp(a.1, b.1, (t - a.0) / span);
        }
    }
    keys[keys.len() - 1].1
}

/// The emission axis in the emitter's local space, and two axes across it
/// for `SpreadAngle`.
fn emission_axes(direction: &str) -> (Vec3, Vec3, Vec3) {
    match direction {
        "Bottom" => (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        "Front" => (Vec3::NEG_Z, Vec3::X, Vec3::Y),
        "Back" => (Vec3::Z, Vec3::X, Vec3::Y),
        "Right" => (Vec3::X, Vec3::Y, Vec3::Z),
        "Left" => (Vec3::NEG_X, Vec3::Y, Vec3::Z),
        _ => (Vec3::Y, Vec3::X, Vec3::Z),
    }
}

fn emit(fx: &mut PlayParticles, g: &DataModel, emitter: InstanceId, count: u32) {
    let room = MAX_PARTICLES.saturating_sub(fx.particles.len());
    let count = (count.min(MAX_BURST) as usize).min(room);
    if count == 0 {
        return;
    }
    let Some(source) = source_of(g, emitter) else { return };
    let style = Arc::new(Style {
        color: color_sequence(g, emitter),
        size: number_sequence(g, emitter, "Size", 0.3),
        transparency: number_sequence(g, emitter, "Transparency", 0.0),
        acceleration: g.get_prop(emitter, "Acceleration").and_then(|v| v.as_vector3()).map_or(Vec3::ZERO, |v| v.to_vec3()),
        drag: number(g, emitter, "Drag", 0.0).max(0.0),
        glow: number(g, emitter, "LightEmission", 0.0).clamp(0.0, 1.0),
    });
    let (life_lo, life_hi) = number_range(g, emitter, "Lifetime", (1.0, 2.0));
    let (speed_lo, speed_hi) = number_range(g, emitter, "Speed", (3.0, 5.0));
    let spread = match g.get_prop(emitter, "SpreadAngle") {
        Some(DmValue::Vector2(v)) => Vec2::new(v.x as f32, v.y as f32),
        _ => Vec2::ZERO,
    };
    let direction = g
        .get_prop(emitter, "EmissionDirection")
        .and_then(|v| v.as_enum_name().map(str::to_string))
        .unwrap_or_else(|| "Top".to_string());
    let (axis, across_a, across_b) = emission_axes(&direction);
    let rotation = source.transform.rotation;

    for _ in 0..count {
        let local = Vec3::new(fx.range(-0.5, 0.5), fx.range(-0.5, 0.5), fx.range(-0.5, 0.5)) * source.extent;
        let tilt_a = fx.range(-spread.x, spread.x).to_radians();
        let tilt_b = fx.range(-spread.y, spread.y).to_radians();
        let dir = Quat::from_axis_angle(across_a, tilt_a) * Quat::from_axis_angle(across_b, tilt_b) * axis;
        let speed = fx.range(speed_lo, speed_hi);
        let life = fx.range(life_lo, life_hi).max(0.01);
        fx.particles.push(Particle {
            position: source.transform.translation + rotation * local,
            velocity: rotation * dir * speed,
            age: 0.0,
            life,
            style: style.clone(),
        });
    }
}

/// One frame: emit (bursts from `Emit`, streams from `Rate`), move, age,
/// and hand the renderer this frame's spheres.
pub fn drive_particles(
    mut commands: Commands,
    dm: Option<Res<PlayDataModel>>,
    fx: Option<ResMut<PlayParticles>>,
    time: Res<Time>,
    mut clouds: Query<&mut ParticleCloud>,
) {
    // Stop removes the resource and despawns the cloud (DataModelSpawned).
    let Some(dm) = dm else { return };
    let Some(mut fx) = fx else {
        commands.insert_resource(PlayParticles::default());
        dm.dm.lock().particle_emits.clear();
        return;
    };
    let dt = time.delta_secs().min(0.1);

    {
        let mut g = dm.dm.lock();
        if fx.emitters_at != g.structure_version {
            fx.emitters = g.ids_of_class("ParticleEmitter");
            fx.emitters_at = g.structure_version;
            let live: std::collections::HashSet<InstanceId> = fx.emitters.iter().copied().collect();
            fx.owed.retain(|id, _| live.contains(id));
        }
        let bursts = std::mem::take(&mut g.particle_emits);
        for (emitter, n) in bursts {
            if g.exists(emitter) {
                emit(&mut fx, &g, emitter, n);
            }
        }
        let emitters = fx.emitters.clone();
        for emitter in emitters {
            let enabled = g.get_prop(emitter, "Enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            let rate = number(&g, emitter, "Rate", 0.0);
            let in_world = g.in_tree(emitter)
                && g.service_of(emitter).and_then(|s| g.class_of(s)) == Some("Workspace");
            if !enabled || rate <= 0.0 || !in_world {
                fx.owed.remove(&emitter);
                continue;
            }
            let owed = fx.owed.entry(emitter).or_insert(0.0);
            *owed += rate * dt;
            let n = owed.floor();
            *owed -= n;
            if n >= 1.0 {
                emit(&mut fx, &g, emitter, n as u32);
            }
        }
    }

    // Move and age. A particle lives until its life runs out or it has
    // faded to nothing.
    let mut instances: Vec<ParticleInstance> = Vec::with_capacity(fx.particles.len());
    fx.particles.retain_mut(|p| {
        p.age += dt;
        if p.age >= p.life {
            return false;
        }
        let s = &p.style;
        p.velocity += s.acceleration * dt;
        if s.drag > 0.0 {
            p.velocity *= 0.5f32.powf(s.drag * dt);
        }
        p.position += p.velocity * dt;
        let t = p.age / p.life;
        let fade = 1.0 - sample(&s.transparency, t, |a, b, k| a + (b - a) * k).clamp(0.0, 1.0);
        let radius = 0.5 * sample(&s.size, t, |a, b, k| a + (b - a) * k).max(0.0) * fade;
        if radius <= 1.0e-4 {
            return true;
        }
        let c = sample(&s.color, t, |a, b, k| a.mix(&b, k)) * (1.0 + 2.0 * s.glow);
        instances.push(ParticleInstance {
            position: p.position.to_array(),
            radius,
            color: [c.red, c.green, c.blue, 1.0],
        });
        true
    });

    fx.revision = fx.revision.wrapping_add(1);
    let revision = fx.revision;
    let cloud = ParticleCloud {
        instances: Arc::new(instances),
        revision,
        domain_size: Vec3::ONE,
        display_scale: 1.0,
        color_range: (0.0, 1.0),
        show_domain: false,
    };
    match fx.cloud.and_then(|e| clouds.get_mut(e).ok()) {
        Some(mut existing) => *existing = cloud,
        None => {
            if cloud.instances.is_empty() {
                return;
            }
            let e = commands
                .spawn((
                    Name::new("Play particles"),
                    Transform::IDENTITY,
                    Visibility::Visible,
                    cloud,
                    DataModelSpawned,
                ))
                .id();
            fx.cloud = Some(e);
        }
    }
}
