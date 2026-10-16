//! # Ocean
//!
//! The engine side of the Terrain object's sea. It owns the spectral
//! simulation of [`eustress_ocean`], steps it on the clock the session runs
//! on, writes its fields into textures for the renderer, answers questions
//! about the surface on the CPU, and floats Avian rigid bodies on it.
//!
//! ## Who writes what
//!
//! | Resource         | Written by                               | Read by                                |
//! |------------------|------------------------------------------|----------------------------------------|
//! | [`OceanControl`] | the Terrain side                         | the ocean, from the next frame's start |
//! | [`OceanClock`]   | the engine (Edit, Play, Pause)           | the ocean's steps                      |
//! | [`OceanSurface`] | the ocean                                | the renderer                           |
//! | [`OceanQueries`] | the ocean (the seabed: the Terrain side) | anything that asks about the water     |
//!
//! ## Schedule
//!
//! | Schedule    | Set ([`OceanSystems`]) | Work                      | When                                                  |
//! |-------------|------------------------|---------------------------|-------------------------------------------------------|
//! | PreUpdate   | `Control`              | apply [`OceanControl`]    | it changed                                            |
//! | Update      | `Step`                 | step on real time         | clock View, ocean on                                  |
//! | FixedUpdate | `Step`                 | step on the physics clock | clock Physics, ocean on                               |
//! | FixedUpdate | `Buoyancy`, after Step | push bodies (`physics`)   | clock Physics, ocean on, buoyancy on, physics running |
//! | PostUpdate  | `Upload`               | write the textures        | the fields changed since the last write               |
//!
//! View moves the surface on real time, for looking at in Edit. Physics
//! evaluates it at the elapsed time of `Time<Physics>` in FixedUpdate, before
//! Avian steps in FixedPostUpdate: every peer and every replay evaluates the
//! same sea at the same step, and the buoyancy forces, which Avian clears
//! after each step, are applied afresh every tick. Frozen holds it still. A
//! jump of the clock (a switch of clocks, a restart, a long stall) primes the
//! rate of rise with an update a 60th of a second earlier, so the first rate
//! after the jump is a real one.
//!
//! ## Textures
//!
//! Per cascade, largest tile first: a displacement texture (Dx, height, Dz,
//! 0) in metres and a derivatives texture (dh/dx, dh/dz, J, 0), Rgba16Float,
//! N x N for the detail's lattice side N, sampled with Repeat addressing and
//! Linear filtering. Texel (i, j), in row j, holds the field at world
//! (i L / N, j L / N) for the cascade's tile size L, repeating with period L:
//! u runs along world X and v along world Z. GPU filtering puts texel
//! centres at (i + 1/2) / N, so the uv that matches the CPU queries at world
//! (x, z) is (x, z) / L + 1 / (2N). The textures exist while the ocean is on
//! (holding still water while its sea is flat) and are made again, with new
//! handles, when the detail changes; an app without image assets (headless)
//! gets none.
//!
//! To agree with the CPU queries, a renderer sums the cascades (heights,
//! slopes, and J - 1), scales displacement and slopes by
//! [`eustress_ocean::shore_fade`] of the water depth over
//! [`OceanSurface::shore_fade_depth`], takes the normal along
//! (-dh/dx, 1, -dh/dz), and foams by [`eustress_ocean::foam_coverage`] of
//! 1 + fade (J - 1) against [`OceanSurface::foam_threshold`], with no foam at
//! all at a threshold of -1 or below.
//!
//! ## The ground under the water
//!
//! The shore fade needs the depth of the water, sea level minus the height
//! of the ground. The ocean does not know the terrain, so the Terrain side
//! hands [`OceanQueries::set_seabed`] a [`Seabed`]: any
//! `Fn(x, z) -> Option<f32>` giving the ground's world height. Where it
//! gives None, or while no seabed is set, the water is deep and the waves
//! run at full height. Buoyancy reads the same queries, so a hull on a beach
//! feels the waves that are drawn there.
//!
//! ## Buoyancy
//!
//! With the `physics` feature, [`buoyancy`] pushes Avian's dynamic bodies
//! with the forces of [`eustress_ocean::buoyancy`].

#[cfg(feature = "physics")]
pub mod buoyancy;

use std::fmt;
use std::sync::Arc;

use bevy::asset::{AssetId, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use eustress_ocean::{
    foam_coverage, foam_threshold, normal_from_slopes, Ocean, OceanSettings, WaterColumn, FOAM_OFF_THRESHOLD,
    MIN_SHORE_FADE_DEPTH, SEAWATER_DENSITY,
};
use half::f16;
use half::slice::HalfFloatSliceExt;

/// Longest step between updates (s) that still counts as the clock running
/// on; a longer one, or one backwards, is a jump.
const MAX_CONTINUOUS_STEP: f64 = 0.5;
/// How long before a jump's new time (s) the update that primes the rate of
/// rise runs.
const PRIME_STEP: f64 = 1.0 / 60.0;

// ============================================================================
// Resources
// ============================================================================

/// What the Terrain object asks of the sea. The Terrain side fills it; the
/// ocean applies a change at the start of the next frame
/// ([`OceanSystems::Control`]).
#[derive(Resource, Clone, Debug)]
pub struct OceanControl {
    /// Whether there is a sea at all.
    pub enabled: bool,
    /// Wind, swell, spread, choppiness, depth, gravity, detail and seed of
    /// the waves (sanitised when applied).
    pub settings: OceanSettings,
    /// World height of the rest surface (m).
    pub sea_level: f32,
    /// Whether dynamic rigid bodies float.
    pub buoyancy: bool,
    /// Density of the water (kg/m^3).
    pub water_density: f32,
    /// Foam setting in [0, 1]: 0 none, 1 the most.
    pub foam: f32,
}

impl Default for OceanControl {
    /// Off; the crate's default sea at height 0; buoyancy on in seawater of
    /// 1025 kg/m^3; full foam.
    fn default() -> Self {
        Self {
            enabled: false,
            settings: OceanSettings::default(),
            sea_level: 0.0,
            buoyancy: true,
            water_density: SEAWATER_DENSITY,
            foam: 1.0,
        }
    }
}

/// The clock the surface follows. The engine sets it.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OceanClock {
    /// Edit: the surface moves on real time, for viewing; nothing floats.
    #[default]
    View,
    /// Play: the surface follows the physics clock in FixedUpdate, the same
    /// at every step on every peer, and dynamic bodies float.
    Physics,
    /// Paused: the surface holds still.
    Frozen,
}

/// What the renderer draws the sea from. The ocean writes it, only when
/// something in it changes; the texture contents change without touching
/// it. See the module documentation for the texture layout.
#[derive(Resource, Clone, Debug, Default)]
pub struct OceanSurface {
    /// Whether there is a moving sea to draw: the ocean is on and its sea is
    /// not flat.
    pub active: bool,
    /// Per cascade, largest tile first: (Dx, height, Dz, 0) in metres,
    /// Rgba16Float. Empty while the ocean is off or without image assets.
    pub displacement: Vec<Handle<Image>>,
    /// Per cascade, largest tile first: (dh/dx, dh/dz, J, 0), Rgba16Float.
    pub derivatives: Vec<Handle<Image>>,
    /// Tile edge of each cascade (m), largest first; 0 while the ocean is
    /// off.
    pub tile_sizes: [f32; 3],
    /// World height of the rest surface (m).
    pub sea_level: f32,
    /// Depth of water (m) over which the waves fade out toward a shore:
    /// displacement and slopes scale by
    /// [`eustress_ocean::shore_fade`]`(depth, shore_fade_depth)`.
    pub shore_fade_depth: f32,
    /// Foam forms where the summed (and faded) J falls below this; -1 or
    /// below means no foam (see [`eustress_ocean::foam_coverage`]).
    pub foam_threshold: f32,
}

/// The ground under the water, for the shore fade: the Terrain side
/// supplies it to [`OceanQueries::set_seabed`]. Any
/// `Fn(f32, f32) -> Option<f32> + Send + Sync + 'static` is one.
pub trait Seabed: Send + Sync + 'static {
    /// World height (m) of the ground at world (x, z), or None where no
    /// ground is known (open sea, treated as deep).
    fn ground_height(&self, x: f32, z: f32) -> Option<f32>;
}

impl<F> Seabed for F
where
    F: Fn(f32, f32) -> Option<f32> + Send + Sync + 'static,
{
    fn ground_height(&self, x: f32, z: f32) -> Option<f32> {
        self(x, z)
    }
}

/// The simulation and the questions about it: the height, normal, foam
/// and rate of rise of the water at a world point, with the shore fade
/// applied. While the ocean is off the answers are those of still water at
/// the last sea level (check [`OceanQueries::is_enabled`]).
#[derive(Resource)]
pub struct OceanQueries {
    /// The sea, while the ocean is on.
    ocean: Option<Ocean>,
    sea_level: f32,
    shore_fade_depth: f32,
    foam_threshold: f32,
    seabed: Option<Arc<dyn Seabed>>,
    /// Bumped whenever the fields change: an update of a sea with waves, a
    /// rebuild, the ocean turned on or off.
    generation: u64,
    /// Time of the latest update (s); None before the first and after any
    /// change that reset the fields.
    last_time: Option<f64>,
}

impl Default for OceanQueries {
    /// Off, at sea level 0, full foam, no seabed.
    fn default() -> Self {
        Self {
            ocean: None,
            sea_level: 0.0,
            shore_fade_depth: MIN_SHORE_FADE_DEPTH,
            foam_threshold: foam_threshold(1.0),
            seabed: None,
            generation: 0,
            last_time: None,
        }
    }
}

impl fmt::Debug for OceanQueries {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OceanQueries")
            .field("settings", &self.ocean.as_ref().map(Ocean::settings))
            .field("sea_level", &self.sea_level)
            .field("shore_fade_depth", &self.shore_fade_depth)
            .field("foam_threshold", &self.foam_threshold)
            .field("seabed", &self.seabed.is_some())
            .field("generation", &self.generation)
            .field("last_time", &self.last_time)
            .finish()
    }
}

impl OceanQueries {
    /// Applies what the Terrain object asks for. Returns true when the
    /// fields changed meaning (the ocean turned on or off, the amplitudes
    /// rebuilt, the detail or the choppiness changed), after which the rate
    /// of rise is primed afresh. [`OceanPlugin`] calls this when
    /// [`OceanControl`] changes; call it yourself only without the plugin.
    pub fn configure(&mut self, control: &OceanControl) -> bool {
        self.sea_level = if control.sea_level.is_finite() { control.sea_level } else { 0.0 };
        self.foam_threshold = foam_threshold(control.foam);
        let changed = if !control.enabled {
            self.ocean.take().is_some()
        } else if let Some(ocean) = self.ocean.as_mut() {
            let before = *ocean.settings();
            ocean.set_settings(control.settings);
            *ocean.settings() != before
        } else {
            self.ocean = Some(Ocean::new(control.settings));
            true
        };
        self.shore_fade_depth = self.ocean.as_ref().map_or(MIN_SHORE_FADE_DEPTH, Ocean::shore_fade_depth);
        if changed {
            self.last_time = None;
            self.generation = self.generation.wrapping_add(1);
        }
        changed
    }

    /// Evaluates the sea at `time_s` (s) on whichever clock drives it. The
    /// same time again does nothing; a step back, or forward by more than
    /// half a second, first runs an update a 60th of a second earlier so
    /// the rate of rise right after the jump is real. Does nothing while the
    /// ocean is off or for a time that is not finite.
    pub fn advance(&mut self, time_s: f64) {
        let Some(ocean) = self.ocean.as_mut() else {
            return;
        };
        if !time_s.is_finite() {
            return;
        }
        match self.last_time {
            Some(last) if time_s == last => return,
            Some(last) if time_s > last && time_s - last <= MAX_CONTINUOUS_STEP => ocean.update(time_s),
            _ => {
                ocean.update(time_s - PRIME_STEP);
                ocean.update(time_s);
            }
        }
        self.last_time = Some(time_s);
        if !ocean.is_flat() {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Whether the ocean is on.
    pub fn is_enabled(&self) -> bool {
        self.ocean.is_some()
    }

    /// Whether the ocean is on and its sea has waves.
    pub fn is_active(&self) -> bool {
        self.ocean.as_ref().is_some_and(|ocean| !ocean.is_flat())
    }

    /// The simulation, while the ocean is on: settings, cascades and the
    /// unfaded samplers.
    pub fn ocean(&self) -> Option<&Ocean> {
        self.ocean.as_ref()
    }

    /// World height of the rest surface (m).
    pub fn sea_level(&self) -> f32 {
        self.sea_level
    }

    /// Depth of water (m) over which the waves fade out toward a shore.
    pub fn shore_fade_depth(&self) -> f32 {
        self.shore_fade_depth
    }

    /// The foam threshold on J of the foam setting.
    pub fn foam_threshold(&self) -> f32 {
        self.foam_threshold
    }

    /// Significant wave height of the sea (m); 0 while the ocean is off.
    pub fn significant_wave_height(&self) -> f32 {
        self.ocean.as_ref().map_or(0.0, Ocean::significant_wave_height)
    }

    /// Counts changes of the fields; equal values mean equal fields.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Sets, or with None clears, the ground under the water that the shore
    /// fade measures depth against. Without one the water is deep everywhere.
    pub fn set_seabed(&mut self, seabed: Option<Arc<dyn Seabed>>) {
        self.seabed = seabed;
    }

    /// Whether a seabed is set.
    pub fn has_seabed(&self) -> bool {
        self.seabed.is_some()
    }

    /// Depth of the water at world (x, z) (m): sea level minus the ground's
    /// height, negative over land; infinite where no ground is known.
    pub fn depth(&self, x: f32, z: f32) -> f32 {
        match self.seabed.as_ref().and_then(|seabed| seabed.ground_height(x, z)) {
            Some(ground) if !ground.is_nan() => self.sea_level - ground,
            _ => f32::INFINITY,
        }
    }

    /// How much of the waves survives at world (x, z): 1 in deep water, 0
    /// on the shoreline and over land.
    pub fn shore_fade(&self, x: f32, z: f32) -> f32 {
        eustress_ocean::shore_fade(self.depth(x, z), self.shore_fade_depth)
    }

    /// The sea with waves and the fade at (x, z), where any wave survives.
    fn waves_at(&self, x: f32, z: f32) -> Option<(&Ocean, f32)> {
        let ocean = self.ocean.as_ref().filter(|ocean| !ocean.is_flat())?;
        let fade = self.shore_fade(x, z);
        (fade > 0.0).then_some((ocean, fade))
    }

    /// World height of the water surface at world (x, z) (m): sea level plus
    /// the faded height of the waves.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        match self.waves_at(x, z) {
            Some((ocean, fade)) => self.sea_level + fade * ocean.height_at(x, z),
            None => self.sea_level,
        }
    }

    /// Unit normal of the water surface at world (x, z): along
    /// (-dh/dx, 1, -dh/dz) of the faded slopes.
    pub fn normal(&self, x: f32, z: f32) -> Vec3 {
        match self.waves_at(x, z) {
            Some((ocean, fade)) => {
                let [slope_x, slope_z, _] = ocean.derivatives_at(x, z);
                Vec3::from_array(normal_from_slopes(fade * slope_x, fade * slope_z))
            }
            None => Vec3::Y,
        }
    }

    /// Foam coverage in [0, 1] at world (x, z): [`foam_coverage`] of the
    /// faded Jacobian 1 + fade (J - 1) against the foam threshold.
    pub fn foam(&self, x: f32, z: f32) -> f32 {
        if !(self.foam_threshold > FOAM_OFF_THRESHOLD) {
            return 0.0;
        }
        match self.waves_at(x, z) {
            Some((ocean, fade)) => foam_coverage(1.0 + fade * (ocean.jacobian_at(x, z) - 1.0), self.foam_threshold),
            None => 0.0,
        }
    }

    /// Rate of rise of the water surface at world (x, z) (m/s), faded.
    pub fn vertical_velocity(&self, x: f32, z: f32) -> f32 {
        match self.waves_at(x, z) {
            Some((ocean, fade)) => fade * ocean.vertical_velocity_at(x, z),
            None => 0.0,
        }
    }

    /// [`OceanQueries::height`] and [`OceanQueries::vertical_velocity`]
    /// together, at the cost of one fewer inversion: the water that
    /// buoyancy needs at each sample.
    pub fn water_column(&self, x: f32, z: f32) -> WaterColumn {
        match self.waves_at(x, z) {
            Some((ocean, fade)) => {
                let (height, rise) = ocean.height_and_vertical_velocity_at(x, z);
                WaterColumn { surface: self.sea_level + fade * height, vertical_velocity: fade * rise }
            }
            None => WaterColumn::still(self.sea_level),
        }
    }
}

// ============================================================================
// Plugin
// ============================================================================

/// The ocean's system sets, for ordering against it (see the module
/// documentation for where each runs).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OceanSystems {
    /// PreUpdate: [`OceanControl`] applied.
    Control,
    /// Update (clock View) and FixedUpdate (clock Physics): the surface
    /// stepped.
    Step,
    /// FixedUpdate, after `Step`: buoyancy for the coming physics step.
    Buoyancy,
    /// PostUpdate: the fields written into the textures.
    Upload,
}

/// The ocean: its resources, its steps, buoyancy (with the `physics`
/// feature) and its textures.
pub struct OceanPlugin;

impl Plugin for OceanPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OceanControl>()
            .init_resource::<OceanClock>()
            .init_resource::<OceanSurface>()
            .init_resource::<OceanQueries>()
            .init_resource::<OceanTextures>()
            .add_systems(PreUpdate, apply_control.in_set(OceanSystems::Control))
            .add_systems(Update, step_view.in_set(OceanSystems::Step))
            .add_systems(FixedUpdate, step_physics.in_set(OceanSystems::Step))
            .add_systems(PostUpdate, upload_textures.in_set(OceanSystems::Upload));
        #[cfg(feature = "physics")]
        app.configure_sets(FixedUpdate, OceanSystems::Buoyancy.after(OceanSystems::Step))
            .add_systems(FixedUpdate, buoyancy::apply_buoyancy.in_set(OceanSystems::Buoyancy));
    }
}

/// The textures' lattice side and what was last written into them.
#[derive(Resource, Default)]
struct OceanTextures {
    /// Lattice side of the textures in [`OceanSurface`]; None without any.
    resolution: Option<usize>,
    /// Generation of the fields last written.
    written: Option<u64>,
    /// The textures last written: displacements, then derivatives.
    written_to: Vec<AssetId<Image>>,
    /// Half floats on their way into a texture.
    scratch: Vec<f16>,
}

/// Applies a changed [`OceanControl`]: settings, sea level and foam, and the
/// textures, made when the ocean turns on or its detail changes and dropped
/// when it turns off. Writes [`OceanSurface`] only when it differs.
fn apply_control(
    control: Res<OceanControl>,
    mut queries: ResMut<OceanQueries>,
    mut surface: ResMut<OceanSurface>,
    mut textures: ResMut<OceanTextures>,
    images: Option<ResMut<Assets<Image>>>,
) {
    if !control.is_changed() {
        return;
    }
    queries.configure(&control);
    let mut next = OceanSurface {
        active: queries.is_active(),
        displacement: Vec::new(),
        derivatives: Vec::new(),
        tile_sizes: queries.ocean().map_or([0.0; 3], Ocean::tile_sizes),
        sea_level: queries.sea_level(),
        shore_fade_depth: queries.shore_fade_depth(),
        foam_threshold: queries.foam_threshold(),
    };
    let textures = &mut *textures;
    match (queries.ocean(), images) {
        (Some(ocean), Some(mut images)) => {
            let resolution = ocean.settings().detail.resolution();
            let cascades = ocean.cascades();
            if textures.resolution == Some(resolution)
                && surface.displacement.len() == cascades.len()
                && surface.derivatives.len() == cascades.len()
            {
                next.displacement = surface.displacement.clone();
                next.derivatives = surface.derivatives.clone();
            } else {
                for cascade in cascades {
                    let displacement = field_image(resolution, cascade.displacement(), "ocean displacement", &mut textures.scratch);
                    let derivatives = field_image(resolution, cascade.derivatives(), "ocean derivatives", &mut textures.scratch);
                    next.displacement.push(images.add(displacement));
                    next.derivatives.push(images.add(derivatives));
                }
                textures.resolution = Some(resolution);
                textures.written = Some(queries.generation());
                textures.written_to = next.displacement.iter().chain(&next.derivatives).map(Handle::id).collect();
            }
        }
        _ => {
            textures.resolution = None;
            textures.written = None;
            textures.written_to.clear();
        }
    }
    if !same_surface(&surface, &next) {
        *surface = next;
    }
}

/// Steps the surface on real time while the clock is View.
fn step_view(clock: Res<OceanClock>, time: Option<Res<Time<Real>>>, mut queries: ResMut<OceanQueries>) {
    if *clock != OceanClock::View || !queries.is_enabled() {
        return;
    }
    if let Some(time) = time {
        queries.advance(time.elapsed_secs_f64());
    }
}

/// Steps the surface on the physics clock while the clock is Physics, in
/// FixedUpdate: the sea at the start of the coming physics step.
#[cfg(feature = "physics")]
fn step_physics(
    clock: Res<OceanClock>,
    time: Option<Res<Time<avian3d::prelude::Physics>>>,
    mut queries: ResMut<OceanQueries>,
) {
    if *clock != OceanClock::Physics || !queries.is_enabled() {
        return;
    }
    if let Some(time) = time {
        queries.advance(time.elapsed_secs_f64());
    }
}

/// Without Avian there is no physics clock: the fixed clock stands in.
#[cfg(not(feature = "physics"))]
fn step_physics(clock: Res<OceanClock>, time: Option<Res<Time<Fixed>>>, mut queries: ResMut<OceanQueries>) {
    if *clock != OceanClock::Physics || !queries.is_enabled() {
        return;
    }
    if let Some(time) = time {
        queries.advance(time.elapsed_secs_f64());
    }
}

/// Writes the fields into the textures when they changed since the last
/// write, once a frame however many steps ran.
fn upload_textures(
    queries: Res<OceanQueries>,
    surface: Res<OceanSurface>,
    mut textures: ResMut<OceanTextures>,
    images: Option<ResMut<Assets<Image>>>,
) {
    let (Some(ocean), Some(mut images)) = (queries.ocean(), images) else {
        return;
    };
    let cascades = ocean.cascades();
    if surface.displacement.len() != cascades.len() || surface.derivatives.len() != cascades.len() {
        return;
    }
    let targets = || surface.displacement.iter().chain(&surface.derivatives).map(Handle::id);
    if textures.written == Some(queries.generation()) && textures.written_to.iter().copied().eq(targets()) {
        return;
    }
    let textures = &mut *textures;
    for (cascade, (displacement, derivatives)) in cascades.iter().zip(surface.displacement.iter().zip(&surface.derivatives)) {
        write_field(&mut images, displacement, cascade.displacement(), &mut textures.scratch);
        write_field(&mut images, derivatives, cascade.derivatives(), &mut textures.scratch);
    }
    textures.written = Some(queries.generation());
    textures.written_to = targets().collect();
}

/// Whether two surfaces hold the same values.
fn same_surface(a: &OceanSurface, b: &OceanSurface) -> bool {
    a.active == b.active
        && a.displacement == b.displacement
        && a.derivatives == b.derivatives
        && a.tile_sizes == b.tile_sizes
        && a.sea_level == b.sea_level
        && a.shore_fade_depth == b.shore_fade_depth
        && a.foam_threshold == b.foam_threshold
}

/// An N x N Rgba16Float texture of one field: Repeat addressing, Linear
/// filtering, written from the CPU every update and sampled by shaders, kept
/// in the main world for the next write.
fn field_image(resolution: usize, texels: &[[f32; 4]], label: &'static str, scratch: &mut Vec<f16>) -> Image {
    let side = resolution as u32;
    let mut image = Image::new(
        Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        TextureDimension::D2,
        texel_bytes(texels, scratch).to_vec(),
        TextureFormat::Rgba16Float,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.label = Some(label);
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some(label.to_string()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// Writes a field into its texture, in place when the size matches.
fn write_field(images: &mut Assets<Image>, handle: &Handle<Image>, texels: &[[f32; 4]], scratch: &mut Vec<f16>) {
    let Some(image) = images.get_mut(handle.id()) else {
        return;
    };
    let image = image.into_inner();
    let bytes = texel_bytes(texels, scratch);
    match image.data.as_mut() {
        Some(data) if data.len() == bytes.len() => data.copy_from_slice(bytes),
        _ => image.data = Some(bytes.to_vec()),
    }
}

/// The texels as Rgba16Float bytes, through a reused buffer of half floats
/// (native byte order, which is little-endian on every platform wgpu
/// supports).
fn texel_bytes<'a>(texels: &[[f32; 4]], scratch: &'a mut Vec<f16>) -> &'a [u8] {
    let values = texels.as_flattened();
    scratch.clear();
    scratch.resize(values.len(), f16::ZERO);
    scratch.convert_from_f32_slice(values);
    bytemuck::cast_slice(scratch.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(sea_level: f32) -> OceanControl {
        OceanControl { enabled: true, sea_level, settings: OceanSettings { seed: 4, ..Default::default() }, ..Default::default() }
    }

    /// Points spread over a few hundred metres (a golden-ratio sequence).
    fn spread(count: u32) -> impl Iterator<Item = (f32, f32)> {
        (1..=count).map(|i| {
            let x = -250.0 + 500.0 * (i as f64 * 0.618_033_988_75).fract() as f32;
            let z = -250.0 + 500.0 * (i as f64 * 0.754_877_666_25).fract() as f32;
            (x, z)
        })
    }

    #[test]
    fn defaults_are_off_seawater_full_foam_and_the_view_clock() {
        let control = OceanControl::default();
        assert!(!control.enabled && control.buoyancy);
        assert_eq!(control.settings, OceanSettings::default());
        assert_eq!((control.sea_level, control.water_density, control.foam), (0.0, 1025.0, 1.0));
        assert_eq!(OceanClock::default(), OceanClock::View);
        let surface = OceanSurface::default();
        assert!(!surface.active && surface.displacement.is_empty() && surface.derivatives.is_empty());
    }

    /// Off, the queries answer still water at the sea level; on, a moving
    /// sea around it, the column query agreeing bit for bit with the
    /// separate ones; a flat sea is still water again.
    #[test]
    fn queries_follow_the_control() {
        let mut queries = OceanQueries::default();
        assert!(!queries.is_enabled() && !queries.is_active());
        assert_eq!((queries.height(3.0, 4.0), queries.normal(3.0, 4.0), queries.foam(3.0, 4.0)), (0.0, Vec3::Y, 0.0));
        assert_eq!(queries.vertical_velocity(3.0, 4.0), 0.0);

        let control = on(5.0);
        assert!(queries.configure(&control), "turning on changes the fields");
        assert!(!queries.configure(&control), "the same control again changes nothing");
        assert!(queries.is_enabled() && queries.is_active());
        assert_eq!(queries.shore_fade_depth(), queries.ocean().unwrap().shore_fade_depth());
        let generation = queries.generation();
        queries.advance(10.0);
        queries.advance(10.05);
        assert!(queries.generation() > generation);
        let (mut moved, mut rising) = (false, false);
        for (x, z) in spread(64) {
            let height = queries.height(x, z);
            assert!(height.is_finite() && (height - 5.0).abs() < 10.0, "({x}, {z}): {height}");
            moved |= (height - 5.0).abs() > 0.05;
            let rise = queries.vertical_velocity(x, z);
            rising |= rise.abs() > 1e-3;
            let column = queries.water_column(x, z);
            assert_eq!(column.surface.to_bits(), height.to_bits());
            assert_eq!(column.vertical_velocity.to_bits(), rise.to_bits());
            let normal = queries.normal(x, z);
            assert!((normal.length() - 1.0).abs() < 1e-5 && normal.y > 0.0);
            assert!((0.0..=1.0).contains(&queries.foam(x, z)));
        }
        assert!(moved && rising, "the sea should move about its level");

        // The same time again does nothing; a clock jump primes the rate.
        let generation = queries.generation();
        queries.advance(10.05);
        assert_eq!(queries.generation(), generation);
        queries.advance(3.0);
        assert!(spread(64).any(|(x, z)| queries.vertical_velocity(x, z).abs() > 1e-3), "a rate right after a jump back");

        let flat = OceanControl { settings: OceanSettings { wind_speed: 0.0, swell: 0.0, ..control.settings }, ..control.clone() };
        assert!(queries.configure(&flat));
        queries.advance(11.0);
        assert!(queries.is_enabled() && !queries.is_active());
        for (x, z) in spread(16) {
            assert_eq!(queries.height(x, z), 5.0);
            assert_eq!(queries.normal(x, z), Vec3::Y);
            assert_eq!(queries.water_column(x, z), WaterColumn::still(5.0));
        }
        assert!(queries.configure(&OceanControl { enabled: false, ..control }), "turning off changes the fields");
        assert!(!queries.is_enabled());
    }

    /// Over land the water rests flat at sea level and calm; in deep water
    /// (or where the seabed knows no ground) the waves run at full height;
    /// in between they are scaled by the fade.
    #[test]
    fn the_shore_fade_flattens_the_waves_toward_land() {
        let mut queries = OceanQueries::default();
        queries.configure(&on(2.0));
        queries.advance(20.0);
        queries.advance(20.05);
        let fade_depth = queries.shore_fade_depth();
        // Land east of x = 100, a shelf of half the fade depth between 0 and
        // 100, open sea to the west.
        queries.set_seabed(Some(Arc::new(move |x: f32, _z: f32| {
            if x > 100.0 {
                Some(3.0)
            } else if x > 0.0 {
                Some(2.0 - 0.5 * fade_depth)
            } else {
                None
            }
        })));
        assert!(queries.has_seabed());
        let ocean_height = |q: &OceanQueries, x: f32, z: f32| q.ocean().unwrap().height_at(x, z);
        for z in [-40.0f32, 0.0, 33.0] {
            let land = 150.0;
            assert_eq!(queries.depth(land, z), -1.0);
            assert_eq!(queries.height(land, z), 2.0, "dry land at z {z}");
            assert_eq!(queries.normal(land, z), Vec3::Y);
            assert_eq!((queries.foam(land, z), queries.vertical_velocity(land, z)), (0.0, 0.0));
            assert_eq!(queries.water_column(land, z), WaterColumn::still(2.0));

            let open = -150.0;
            assert_eq!(queries.depth(open, z), f32::INFINITY);
            assert_eq!(queries.height(open, z), 2.0 + ocean_height(&queries, open, z), "open sea at z {z}");

            let shelf = 50.0;
            assert!((queries.shore_fade(shelf, z) - 0.5).abs() < 1e-6);
            let expect = 2.0 + 0.5 * ocean_height(&queries, shelf, z);
            assert!((queries.height(shelf, z) - expect).abs() < 1e-6, "shelf at z {z}");
        }
        // A NaN ground is no ground: deep water.
        queries.set_seabed(Some(Arc::new(|_x: f32, _z: f32| Some(f32::NAN))));
        assert_eq!(queries.depth(1.0, 1.0), f32::INFINITY);
        queries.set_seabed(None);
        assert_eq!(queries.shore_fade(150.0, 0.0), 1.0);
    }

    /// The textures are N x N Rgba16Float, filterable and repeating, made to
    /// be written from the CPU and sampled by shaders, holding the fields as
    /// half floats.
    #[test]
    fn field_images_are_repeating_filterable_half_floats() {
        let texels: Vec<[f32; 4]> = (0..16).map(|i| [i as f32 * 0.25, -2.0, 0.5, 1.0]).collect();
        let mut scratch = Vec::new();
        let image = field_image(4, &texels, "test field", &mut scratch);
        let d = &image.texture_descriptor;
        assert_eq!(d.format, TextureFormat::Rgba16Float);
        assert_eq!(d.dimension, TextureDimension::D2);
        assert_eq!((d.size.width, d.size.height, d.size.depth_or_array_layers), (4, 4, 1));
        assert_eq!(d.usage, TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST);
        assert_eq!(image.asset_usage, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD);
        let ImageSampler::Descriptor(sampler) = &image.sampler else {
            panic!("the textures need their own sampler");
        };
        assert_eq!(
            (sampler.address_mode_u, sampler.address_mode_v, sampler.address_mode_w),
            (ImageAddressMode::Repeat, ImageAddressMode::Repeat, ImageAddressMode::Repeat)
        );
        assert_eq!((sampler.mag_filter, sampler.min_filter), (ImageFilterMode::Linear, ImageFilterMode::Linear));
        let data = image.data.as_ref().expect("the fields are in the image");
        assert_eq!(data.len(), 16 * 4 * 2);
        // Texel 5, channel by channel: 1.25, -2, 0.5, 1 are exact in half
        // precision.
        let channel = |texel: usize, c: usize| {
            let at = (texel * 4 + c) * 2;
            f16::from_bits(u16::from_le_bytes([data[at], data[at + 1]])).to_f32()
        };
        assert_eq!([channel(5, 0), channel(5, 1), channel(5, 2), channel(5, 3)], [1.25, -2.0, 0.5, 1.0]);
        assert_eq!(f16::from_f32(1.0).to_bits(), 0x3c00);
        assert_eq!(&data[6..8], &[0x00, 0x3c], "1.0 of texel 0's last channel, little-endian");
    }

    /// Writing a field converts every value to the nearest half float,
    /// within half a unit in the last place (2^-11 relative).
    #[test]
    fn texel_bytes_carry_the_fields_to_half_precision() {
        let texels: Vec<[f32; 4]> = (0..64).map(|i| {
            let t = i as f32 * 0.37;
            [3.7 * t.sin(), 1.9 * t.cos(), -0.8 * t, 1.0 - 0.3 * (2.0 * t).cos()]
        }).collect();
        let mut scratch = Vec::new();
        let bytes = texel_bytes(&texels, &mut scratch).to_vec();
        assert_eq!(bytes.len(), 64 * 8);
        for (i, value) in texels.as_flattened().iter().enumerate() {
            let back = f16::from_bits(u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]])).to_f32();
            assert!((back - value).abs() <= value.abs() * 4.9e-4 + 3e-8, "value {i}: {value} came back {back}");
        }
    }
}
