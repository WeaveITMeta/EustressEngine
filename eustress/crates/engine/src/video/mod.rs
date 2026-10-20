//! Video playback abstraction.
//!
//! Sits between the file_loader's `Video` spawn arm and a swappable
//! decoder backend. Each playing video gets a [`VideoPlayer`]
//! component pointing at a [`Handle<Image>`] that backs whichever
//! `StandardMaterial.base_color_texture` (3D quad), Slint
//! `base-color-texture` (BillboardGui / SurfaceGui — pending compositor),
//! or `bevy_ui::UiImage` (screen-space UI — same path) the entity is
//! rendered through. The [`pump_video_frames`] system runs each frame,
//! asks every active player's [`VideoFrameSource`] for the next RGBA8
//! frame, and writes it into the player's GPU image so the renderer
//! picks up the new pixels next paint.
//!
//! The decoder is the [`VideoFrameSource`] trait. Today the only impl
//! is [`TestPatternSource`] — an animated gradient + frame counter that
//! exists purely to prove the texture-pump pipeline is wired through to
//! the screen. When `bevy_video` (or whichever decoder we settle on)
//! lands, it slots in as another `VideoFrameSource` impl and the rest
//! of the pipeline (component, system, file_loader hook) doesn't move.
//!
//! ## Where this plugs in
//!
//! - **3D scene `Video` class entities** ← live now (this module +
//!   `space::file_loader`'s Video arm).
//! - **BillboardGui / SurfaceGui** ← needs a Slint compositor that
//!   blits the `VideoPlayer.texture` onto the BillboardCard's staging
//!   buffer pre-render. Sketched in the doc-comment at the bottom of
//!   this file; not implemented yet.
//! - **Screen-space UI (Slint StudioWindow)** ← same compositor question
//!   for Slint elements that want to embed a video. Best path is a new
//!   Slint `VideoFrame` component that the StudioWindow can host,
//!   compositing the texture per-frame from the SoftwareRenderer's
//!   draw callback.

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

/// Plugin — registers the frame-pump system + the deferred-setup
/// system that turns `PendingVideoSetup` markers into live
/// `VideoPlayer` components. Setup runs before pump in the same
/// update so a freshly-imported video starts producing frames the
/// frame after it spawns.
pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            attach_pending_video_players,
            pump_video_frames,
        ).chain());
    }
}

/// Marker placed by `file_loader`'s Video spawn arm. The
/// [`attach_pending_video_players`] system picks it up next frame,
/// allocates a `VideoPlayer` (which mints a fresh `Handle<Image>`),
/// rewrites the entity's StandardMaterial to bind that image as the
/// base color texture, and removes itself.
///
/// Decoupling spawn-time from texture allocation avoids cascading the
/// `Assets<Image>` ResMut requirement up through every caller of
/// `spawn_directory_entry`. The cost is a one-frame delay before video
/// starts playing, which is imperceptible (16ms) and matches the
/// timing every other "spawn-and-then-attach-renderer" path uses
/// (BillboardGui's `spawn_billboard_render_state` works the same way).
#[derive(Component)]
pub struct PendingVideoSetup {
    pub width: u32,
    pub height: u32,
    /// Absolute path on disk (already resolved against Universe root by
    /// the file_loader). When `bevy_video` integration lands, this is
    /// what the decoder gets opened against.
    pub asset_path: std::path::PathBuf,
}

fn attach_pending_video_players(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    pending: Query<(Entity, &PendingVideoSetup, &MeshMaterial3d<StandardMaterial>)>,
) {
    for (entity, setup, material_handle) in &pending {
        // Probe the file first so we can size the GPU texture to match
        // the source resolution. Probe failure (file missing, not an
        // MP4, no H.264 track, etc.) falls back to the test-pattern
        // source at the file_loader's default dimensions, which makes
        // import-and-spawn always show *something* even when playback
        // can't start. The user gets a visible animated grid + a
        // warning log instead of a silent black quad.
        let (w, h, fps, source): (u32, u32, f32, Box<dyn VideoFrameSource + Send + Sync>) =
            match Mp4H264Source::probe(&setup.asset_path) {
                Some((w, h, fps)) => {
                    let src = Mp4H264Source::new(setup.asset_path.clone(), w, h, fps, true);
                    (w, h, fps, Box::new(src) as Box<dyn VideoFrameSource + Send + Sync>)
                }
                None => {
                    warn!(
                        "🎬 video probe failed for {:?} — falling back to test pattern. \
                         Common causes: file isn't an MP4 / no H.264 track / unsupported codec (HEVC, AV1, VP9).",
                        setup.asset_path
                    );
                    let src = TestPatternSource::new(setup.width, setup.height);
                    (setup.width, setup.height, 30.0, Box::new(src))
                }
            };

        let mut player = VideoPlayer::new(&mut images, w, h);
        player.source = Some(source);

        // Bind the freshly allocated image to the entity's existing
        // material. Reusing the material handle (rather than minting
        // a new one) preserves the colour tint / transparency /
        // alpha-mode the file_loader already set on it.
        if let Some(mut mat) = materials.get_mut(&material_handle.0) {
            mat.base_color_texture = Some(player.texture.clone());
            // Switch off the placeholder emissive — the texture now
            // carries colour information of its own.
            mat.emissive = bevy::color::LinearRgba::NONE;
            // Reset base color to white so the texture shows through
            // without being darkened by the placeholder grey.
            mat.base_color = bevy::color::Color::WHITE;
        }

        info!(
            "🎬 attached VideoPlayer to entity {:?} ({}×{} @ {:.1} fps, source: {:?})",
            entity, w, h, fps, setup.asset_path
        );

        commands.entity(entity).insert(player);
        commands.entity(entity).remove::<PendingVideoSetup>();
    }
}

/// Per-entity playback state. The `texture` field is what every
/// downstream renderer (StandardMaterial, BillboardGui composite, UI
/// image) binds to — `pump_video_frames` rewrites the image's
/// pixel buffer in place each frame, so renderers don't need to
/// reattach handles; they just see fresh pixels.
///
/// `source` is `Option` so a player can be paused (set to `None` to
/// stop pumping frames; reattach to resume). Boxed so the trait stays
/// object-safe and decoder backends can hold whatever heavy state
/// they need (FFmpeg context, audio buffers, etc.).
///
/// `started_at` and `last_frame_at` drive the pump's notion of "when
/// to ask for the next frame" — sources can return `None` from
/// `next_frame` to indicate they're not ready yet, and the pump will
/// retry on the next tick.
#[derive(Component)]
pub struct VideoPlayer {
    pub texture: Handle<Image>,
    pub width: u32,
    pub height: u32,
    /// Decoder-dependent source. `None` = paused (no frames pumped).
    pub source: Option<Box<dyn VideoFrameSource + Send + Sync>>,
    /// Wall-clock instant the player was attached. Drives the
    /// test-pattern's animation phase.
    pub started_at: std::time::Instant,
    pub frame_count: u64,
}

impl VideoPlayer {
    /// Construct a paused player with a freshly-allocated GPU image of
    /// the right dimensions. Caller picks the source separately so a
    /// single helper handles both immediate-attach and lazy-attach
    /// flows.
    pub fn new(images: &mut Assets<Image>, width: u32, height: u32) -> Self {
        let image = make_video_texture(width, height);
        Self {
            texture: images.add(image),
            width,
            height,
            source: None,
            started_at: std::time::Instant::now(),
            frame_count: 0,
        }
    }

    /// Attach a frame source. Replaces any existing source.
    pub fn with_source(mut self, source: Box<dyn VideoFrameSource + Send + Sync>) -> Self {
        self.source = Some(source);
        self
    }
}

/// Decoder-backend abstraction. Implementations produce RGBA8 frame
/// data (`width * height * 4` bytes, row-major, top-to-bottom) on
/// demand. The pump calls `next_frame` once per Bevy Update tick;
/// returning `None` means "no new frame yet" and the existing image
/// keeps showing whatever was last written.
///
/// Memory model: implementations should reuse internal buffers and
/// hand back a borrowed slice. The pump copies into the GPU image,
/// so the lifetime of the returned slice only needs to outlive that
/// call.
pub trait VideoFrameSource {
    /// Asked once per frame. Return `Some(rgba8_bytes)` to push a new
    /// frame, or `None` if no fresh frame is available yet.
    ///
    /// `dt` is wall-clock seconds since the previous call — useful for
    /// fixed-rate decoders that need to know when their next frame
    /// "should" arrive.
    fn next_frame(&mut self, dt: f32) -> Option<&[u8]>;

    /// Width in pixels of the frames this source produces. The pump
    /// uses this to validate against `VideoPlayer.width`; mismatches
    /// produce a one-shot warning + skip.
    fn width(&self) -> u32;
    fn height(&self) -> u32;
}

/// Allocate a Bevy `Image` sized for video frames. RGBA8 unorm,
/// `TEXTURE_BINDING | COPY_DST` so the renderer can sample it and the
/// CPU can write into it via `image.data`.
fn make_video_texture(width: u32, height: u32) -> Image {
    let mut image = Image {
        texture_descriptor: TextureDescriptor {
            label: Some("VideoFrame"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            mip_level_count: 1,
            sample_count: 1,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        ..default()
    };
    image.resize(image.texture_descriptor.size);
    image
}

/// System: walk every [`VideoPlayer`], ask its source for the next
/// frame, blit into the GPU image. Touching `images.get_mut(...)`
/// marks the asset Changed, which is what triggers Bevy to re-upload
/// the texture to the GPU next render — without that the visible
/// surface stays on the first frame regardless of CPU-side updates.
fn pump_video_frames(
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    mut players: Query<&mut VideoPlayer>,
) {
    let dt = time.delta_secs();
    for mut player in &mut players {
        // Width / height / handle are Copy/Clone so we capture them
        // before borrowing `source` mutably.
        let width = player.width;
        let height = player.height;
        let texture = player.texture.clone();
        let expected_len = (width * height * 4) as usize;

        // Borrow `source` mutably for the next_frame call; release
        // before touching the image asset / frame counter. Capture
        // dimensions BEFORE borrowing `frame` so we don't end up with
        // an `&self` (`width()`, `height()`) accessor call held alive
        // alongside the `&mut self` borrow `frame` is tied to.
        let Some(bytes_owned) = player.source.as_mut().and_then(|src| {
            let src_w = src.width();
            let src_h = src.height();
            let frame = src.next_frame(dt)?;
            if src_w != width || src_h != height {
                warn!(
                    "🎬 video frame size mismatch (source {}x{}, player {}x{}) — skipping",
                    src_w, src_h, width, height
                );
                return None;
            }
            if frame.len() != expected_len {
                warn!(
                    "🎬 video frame buffer length mismatch (source returned {} bytes, expected {}) — skipping",
                    frame.len(), expected_len
                );
                return None;
            }
            // Copy out so the source borrow ends here. Keeps the
            // Bevy `Mut<VideoPlayer>` free for subsequent field
            // mutation (`frame_count`).
            Some(frame.to_vec())
        }) else {
            continue;
        };

        let Some(mut image) = images.get_mut(&texture) else { continue };
        if let Some(data) = image.data.as_mut() {
            data[..expected_len].copy_from_slice(&bytes_owned[..expected_len]);
        }
        player.frame_count += 1;
    }
}

// ──────────────────────────────────────────────────────────────────────
// TestPatternSource — animated gradient + frame counter, used as the
// default decoder until a real one (`Mp4H264Source` below) is wired
// in. Provides immediate visual proof that the texture pump
// is working: video entities show a clearly-animated surface rather
// than the previous static placeholder.
// ──────────────────────────────────────────────────────────────────────

/// Produces a procedurally animated test pattern (rolling colour
/// gradient + frame counter ticking in the corner). Useful for proving
/// the texture-pump pipeline reaches the GPU before any real decoder
/// is integrated. Once a real decoder lands this gets used only by
/// the "no asset / asset failed to load" fallback path.
pub struct TestPatternSource {
    width: u32,
    height: u32,
    elapsed: f32,
    frame_count: u64,
    /// Reused frame buffer so each `next_frame` doesn't allocate.
    buffer: Vec<u8>,
}

impl TestPatternSource {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            elapsed: 0.0,
            frame_count: 0,
            buffer: vec![0u8; (width * height * 4) as usize],
        }
    }
}

impl VideoFrameSource for TestPatternSource {
    fn next_frame(&mut self, dt: f32) -> Option<&[u8]> {
        self.elapsed += dt;
        self.frame_count += 1;

        let w = self.width;
        let h = self.height;
        let phase = self.elapsed * 0.6;

        // Body fill — animated diagonal gradient with subtle
        // chequerboard so the user can see the pattern actually
        // moving rather than just a flat colour shift.
        for y in 0..h {
            for x in 0..w {
                let xn = x as f32 / w as f32;
                let yn = y as f32 / h as f32;
                let band = (xn * 8.0 + phase).sin() * 0.5 + 0.5;
                let cell = ((x / 16) ^ (y / 16)) & 1;
                let cell_tint = if cell == 0 { 0.0 } else { 0.05 };
                let r = (xn * 200.0 + (phase * 40.0).sin() * 30.0 + cell_tint * 255.0) as u8;
                let g = ((yn * 220.0 + band * 30.0) - cell_tint * 255.0) as u8;
                let b = (180.0 + (phase * 1.7).cos() * 50.0) as u8;
                let i = ((y * w + x) * 4) as usize;
                self.buffer[i]     = r;
                self.buffer[i + 1] = g;
                self.buffer[i + 2] = b;
                self.buffer[i + 3] = 255;
            }
        }

        // Frame counter — top-left 5x5 dot grid. Each lit cell is one
        // bit of the running frame counter; gives an unmistakable
        // "video is playing" signal even when the gradient happens to
        // be near-uniform colour. 25 bits ≈ 33M frames before wrap;
        // plenty for a debug source.
        let counter = self.frame_count;
        for bit in 0..25u64 {
            if (counter >> bit) & 1 == 0 { continue; }
            let cx = (bit % 5) as u32;
            let cy = (bit / 5) as u32;
            let px = 4 + cx * 6;
            let py = 4 + cy * 6;
            for dy in 0..5u32 {
                for dx in 0..5u32 {
                    let x = px + dx;
                    let y = py + dy;
                    if x >= w || y >= h { continue; }
                    let i = ((y * w + x) * 4) as usize;
                    self.buffer[i]     = 255;
                    self.buffer[i + 1] = 255;
                    self.buffer[i + 2] = 255;
                    self.buffer[i + 3] = 255;
                }
            }
        }

        Some(&self.buffer)
    }

    fn width(&self) -> u32  { self.width }
    fn height(&self) -> u32 { self.height }
}

// ──────────────────────────────────────────────────────────────────────
// Mp4H264Source — pure-Rust H.264-in-MP4 decoder.
//
// Wraps `mp4::Mp4Reader` (alfg/mp4-rust) for container parsing and
// `rusty_h264_decoder::Decoder` for H.264 frame decoding. Both are
// pure Rust; nothing native is compiled or linked. The combination
// handles MP4/H.264 files (the dominant "user uploads a clip" format);
// HEVC, AV1, VP9/WebM are out of scope until a heavier decoder stack
// lands.
// ──────────────────────────────────────────────────────────────────────

/// Pictures held back for display reordering once a stream is known to
/// have B-frames. x264's defaults need at most 2 or 3; 4 covers them
/// without a visible delay (at 30 fps it is 133 ms, once, at the start).
const REORDER_DEPTH: usize = 4;

/// A decoded 4:2:0 picture's planes, owned, waiting to be shown.
struct Picture {
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

/// Decoder source backed by an `.mp4` file containing an H.264 video
/// track. Frame dimensions, frame rate, and SPS/PPS configuration are
/// read from the MP4 track header at probe time; per-frame samples
/// are converted from AVCC length-prefixed NALs to Annex-B start-code
/// format and decoded one access unit (one sample) at a time.
///
/// Loops the playback automatically when `looped` is true (which is
/// the `Video.defaults.toml` default), restarting from sample 1 on
/// each loop.
pub struct Mp4H264Source {
    asset_path: std::path::PathBuf,
    width: u32,
    height: u32,
    fps: f32,
    looped: bool,
    /// RGBA8 frame buffer, sized `width * height * 4`. Reused across
    /// frames; the borrow returned from `next_frame` points into this.
    rgba_buf: Vec<u8>,
    /// Lazy-opened decoder state. Held in `Option` so construction
    /// can be cheap (no I/O, no decoder alloc) and the source can be
    /// hot-swapped between paused / playing states without leaking
    /// the underlying file handle.
    inner: Option<Mp4DecoderState>,
    /// Set to true after the first failed open so we don't repeatedly
    /// retry (and spam logs) every frame on a permanently-broken asset.
    init_failed: bool,
    /// Wall-clock seconds since playback started. Drives the
    /// "what frame should be on screen now?" calculation.
    elapsed: f32,
    /// Index of the most recently displayed sample (1-based, per the
    /// `mp4` crate's convention). Advances forward as `elapsed` grows
    /// past each successive frame's display time.
    last_displayed_sample: u32,
}

/// Decoder + reader bundled together so we can drop them as a unit
/// when looping (re-open the file from sample 1).
struct Mp4DecoderState {
    reader: mp4::Mp4Reader<std::io::BufReader<std::fs::File>>,
    /// Behind a `Mutex` only because the decoder holds an `mpsc::Receiver`,
    /// which is `Send` but not `Sync`, and video sources are boxed as
    /// `Send + Sync`. Every call goes through `get_mut`, so it is never
    /// actually locked.
    decoder: std::sync::Mutex<rusty_h264_decoder::Decoder>,
    track_id: u32,
    sample_count: u32,
    /// Reusable scratch buffer for the AVCC → Annex-B NAL conversion.
    /// Cleared per sample, never deallocated.
    annex_b_buf: Vec<u8>,
    /// Decoded pictures not yet shown, with their composition times.
    /// The decoder returns pictures in DECODE order; with B-frames that
    /// is not the order they are shown in, so they wait here and leave
    /// earliest composition time first.
    reorder: Vec<(i64, Picture)>,
    /// How many pictures `reorder` holds back: 0 until a sample carries
    /// a composition offset (the container's sign of B-frames), then
    /// [`REORDER_DEPTH`]. A stream without B-frames is shown the moment
    /// each picture decodes, exactly as before.
    reorder_depth: usize,
}

/// Remove and return the waiting picture with the earliest composition
/// time. The buffer never holds more than `REORDER_DEPTH + 1` pictures,
/// so a linear scan beats keeping a heap ordered.
fn take_earliest(reorder: &mut Vec<(i64, Picture)>) -> Option<Picture> {
    let index = reorder
        .iter()
        .enumerate()
        .min_by_key(|(_, (cts, _))| *cts)?
        .0;
    Some(reorder.swap_remove(index).1)
}

/// Convert a 4:2:0 picture to RGBA8 with BT.601 limited-range
/// coefficients and saturating float-to-byte casts: the same math as the
/// OpenH264 wrapper this replaced, so a clip looks identical after the
/// swap (checked on 90 decoded frames: 2 bytes in the whole run differed,
/// each by 1). Returns `false`, writing nothing, if the planes or the
/// target are not the sizes the dimensions promise.
fn yuv420_to_rgba(picture: &Picture, width: usize, height: usize, out: &mut [u8]) -> bool {
    // Chroma is half size, rounded down for most streams; an odd cropped
    // size can round up instead, so take whichever the plane actually is.
    let (cw, ch) = if picture.u.len() == width.div_ceil(2) * height.div_ceil(2) {
        (width.div_ceil(2), height.div_ceil(2))
    } else {
        (width / 2, height / 2)
    };
    if width == 0
        || height == 0
        || cw == 0
        || ch == 0
        || picture.y.len() < width * height
        || picture.u.len() < cw * ch
        || picture.v.len() < cw * ch
        || out.len() < width * height * 4
    {
        return false;
    }

    const Y_MUL: f32 = 255.0 / 219.0;
    const RV_MUL: f32 = 255.0 / 224.0 * 1.402;
    const GV_MUL: f32 = -255.0 / 224.0 * 1.402 * 0.299 / 0.687;
    const GU_MUL: f32 = -255.0 / 224.0 * 1.772 * 0.114 / 0.587;
    const BU_MUL: f32 = 255.0 / 224.0 * 1.772;

    for row in 0..height {
        let chroma_row = (row / 2).min(ch - 1) * cw;
        for col in 0..width {
            let c = chroma_row + (col / 2).min(cw - 1);
            let y = Y_MUL * (f32::from(picture.y[row * width + col]) - 16.0);
            let u = f32::from(picture.u[c]) - 128.0;
            let v = f32::from(picture.v[c]) - 128.0;
            let o = (row * width + col) * 4;
            out[o] = RV_MUL.mul_add(v, y) as u8;
            out[o + 1] = GV_MUL.mul_add(v, GU_MUL.mul_add(u, y)) as u8;
            out[o + 2] = BU_MUL.mul_add(u, y) as u8;
            out[o + 3] = 255;
        }
    }
    true
}

impl Mp4H264Source {
    /// Open the MP4 just long enough to read the H.264 track header,
    /// then drop the reader. Returns `(width, height, fps)` so the
    /// caller can size the GPU texture before allocating a full
    /// `Mp4H264Source`. `None` means the file can't be parsed, has no
    /// H.264 video track, or has malformed headers.
    pub fn probe(path: &std::path::Path) -> Option<(u32, u32, f32)> {
        let file = std::fs::File::open(path).ok()?;
        let size = file.metadata().ok()?.len();
        let reader = std::io::BufReader::new(file);
        let mp4 = mp4::Mp4Reader::read_header(reader, size).ok()?;
        for (_id, track) in mp4.tracks() {
            // `video_profile()` succeeds only on H.264 tracks (it reads
            // the avcC box). Audio / subtitle / metadata tracks fail
            // here, which is exactly the filter we want.
            if track.video_profile().is_ok() {
                let w = track.width() as u32;
                let h = track.height() as u32;
                let fps = track.frame_rate() as f32;
                return Some((w, h, if fps > 0.0 { fps } else { 30.0 }));
            }
        }
        None
    }

    pub fn new(asset_path: std::path::PathBuf, width: u32, height: u32, fps: f32, looped: bool) -> Self {
        let buf_len = (width * height * 4) as usize;
        Self {
            asset_path,
            width,
            height,
            fps: if fps > 0.0 { fps } else { 30.0 },
            looped,
            rgba_buf: vec![0u8; buf_len],
            inner: None,
            init_failed: false,
            elapsed: 0.0,
            last_displayed_sample: 0,
        }
    }

    /// Open the file + decoder + feed SPS/PPS so the decoder is ready
    /// to accept slice data. Idempotent — does nothing if `inner` is
    /// already populated.
    fn try_open(&mut self) {
        if self.inner.is_some() || self.init_failed { return; }
        match self.open_inner() {
            Ok(state) => {
                self.inner = Some(state);
                self.last_displayed_sample = 0;
            }
            Err(e) => {
                warn!("🎬 Mp4H264Source: failed to open {:?}: {}", self.asset_path, e);
                self.init_failed = true;
            }
        }
    }

    fn open_inner(&self) -> Result<Mp4DecoderState, String> {
        let file = std::fs::File::open(&self.asset_path)
            .map_err(|e| format!("open: {}", e))?;
        let size = file.metadata().map_err(|e| format!("metadata: {}", e))?.len();
        let reader = std::io::BufReader::new(file);
        let mp4 = mp4::Mp4Reader::read_header(reader, size)
            .map_err(|e| format!("read_header: {}", e))?;

        // Find the H.264 video track. `video_profile()` succeeds only
        // on AVC tracks so it's both the test and the data fetch.
        // Copy SPS / PPS out into owned buffers immediately so the
        // borrow on `mp4` is released before we use it again below.
        let mut found: Option<(u32, Vec<u8>, Vec<u8>)> = None;
        for (id, track) in mp4.tracks() {
            if track.video_profile().is_ok() {
                let sps = track.sequence_parameter_set()
                    .map_err(|e| format!("missing SPS: {}", e))?
                    .to_vec();
                let pps = track.picture_parameter_set()
                    .map_err(|e| format!("missing PPS: {}", e))?
                    .to_vec();
                found = Some((*id, sps, pps));
                break;
            }
        }
        let Some((track_id, sps, pps)) = found else {
            return Err("no H.264 video track found".to_string());
        };
        let sample_count = mp4.sample_count(track_id)
            .map_err(|e| format!("sample_count: {}", e))?;

        let mut decoder = rusty_h264_decoder::Decoder::new();

        // Feed SPS and PPS in Annex-B format (start-code prefixed).
        // The decoder is stateful — until it has seen valid SPS/PPS
        // it'll reject every slice, so this is mandatory before the
        // first frame. MP4 keeps them in the avcC box, not in the
        // samples, so they are never seen otherwise.
        let mut header = Vec::with_capacity(sps.len() + pps.len() + 8);
        header.extend_from_slice(&[0, 0, 0, 1]);
        header.extend_from_slice(&sps);
        header.extend_from_slice(&[0, 0, 0, 1]);
        header.extend_from_slice(&pps);
        // Discard whatever the decoder returns here — SPS/PPS NALs
        // never produce a frame.
        let _ = decoder.decode(&header);

        Ok(Mp4DecoderState {
            reader: mp4,
            decoder: std::sync::Mutex::new(decoder),
            track_id,
            sample_count,
            annex_b_buf: Vec::with_capacity(64 * 1024),
            reorder: Vec::with_capacity(REORDER_DEPTH + 1),
            reorder_depth: 0,
        })
    }

    /// Read one MP4 sample, convert AVCC NALs → Annex-B, decode it as one
    /// access unit, and write the next picture due on screen (if any) into
    /// `self.rgba_buf`. Returns `Ok(true)` when a frame landed in the
    /// buffer, `Ok(false)` if the sample produced nothing to show yet (a
    /// parameter-set-only sample, or a picture held back for display
    /// reordering), or `Err` once the file and the reorder buffer are
    /// both exhausted.
    fn decode_next_sample(&mut self) -> Result<bool, ()> {
        let state = self.inner.as_mut().ok_or(())?;
        let next_sample_id = self.last_displayed_sample + 1;
        if next_sample_id > state.sample_count {
            // Out of samples, but pictures held for reordering are still
            // owed to the screen: hand them out one per frame step.
            self.last_displayed_sample = next_sample_id;
            let Some(picture) = take_earliest(&mut state.reorder) else {
                return Err(());
            };
            return Ok(yuv420_to_rgba(&picture, self.width as usize, self.height as usize, &mut self.rgba_buf));
        }

        let sample = match state.reader.read_sample(state.track_id, next_sample_id) {
            Ok(Some(s)) => s,
            Ok(None) | Err(_) => return Err(()),
        };
        self.last_displayed_sample = next_sample_id;

        // AVCC → Annex-B: each NAL in the sample is preceded by a
        // 4-byte big-endian length. We replace each length with the
        // start-code `[0x00, 0x00, 0x00, 0x01]` and concatenate. This
        // assumes the standard MP4 4-byte NAL prefix; in the rare
        // case of 1- or 2-byte prefixes the resulting bitstream will
        // be malformed and the decoder will error out — we'll log
        // and skip the frame rather than crash.
        state.annex_b_buf.clear();
        let bytes = &sample.bytes[..];
        let mut i = 0;
        while i + 4 <= bytes.len() {
            let len = ((bytes[i]   as usize) << 24)
                    | ((bytes[i+1] as usize) << 16)
                    | ((bytes[i+2] as usize) << 8)
                    | ((bytes[i+3] as usize));
            i += 4;
            if i + len > bytes.len() {
                // Malformed sample — bail out, don't overrun.
                return Ok(false);
            }
            state.annex_b_buf.extend_from_slice(&[0, 0, 0, 1]);
            state.annex_b_buf.extend_from_slice(&bytes[i..i + len]);
            i += len;
        }

        // A composition offset on any sample is the container saying the
        // stream has B-frames: decode order is no longer display order, so
        // from here on pictures wait in `reorder` before being shown.
        if sample.rendering_offset != 0 {
            state.reorder_depth = REORDER_DEPTH;
        }

        // One MP4 sample is one access unit, which is what the decoder
        // takes per call; it splits the NALs itself.
        let decoder = state.decoder.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
        match decoder.decode(&state.annex_b_buf) {
            Ok(Some(frame)) => {
                if frame.width as u32 == self.width && frame.height as u32 == self.height {
                    let cts = sample.start_time as i64 + i64::from(sample.rendering_offset);
                    state.reorder.push((cts, Picture { y: frame.y, u: frame.u, v: frame.v }));
                }
                // Otherwise the source changed resolution mid-stream (rare
                // — it forces an IDR + new SPS). Skip until it matches the
                // texture again rather than write past the buffer.
            }
            Ok(None) => {} // SPS / PPS / no coded picture in this unit.
            Err(_) => {}   // Soft-fail a bad access unit; the next IDR recovers.
        }

        if state.reorder.len() > state.reorder_depth {
            if let Some(picture) = take_earliest(&mut state.reorder) {
                return Ok(yuv420_to_rgba(&picture, self.width as usize, self.height as usize, &mut self.rgba_buf));
            }
        }
        Ok(false)
    }

    fn restart(&mut self) {
        // Drop the decoder state and re-open from scratch. Could be
        // optimised by seeking back to sample 1 + flushing the
        // decoder, but a fresh open is robust + uncomplicated and
        // happens at video boundaries (1× per loop) so the cost
        // doesn't matter.
        self.inner = None;
        self.elapsed = 0.0;
        self.last_displayed_sample = 0;
        self.try_open();
    }
}

impl VideoFrameSource for Mp4H264Source {
    fn next_frame(&mut self, dt: f32) -> Option<&[u8]> {
        self.elapsed += dt;
        if self.inner.is_none() {
            self.try_open();
            if self.inner.is_none() { return None; }
        }

        // What sample SHOULD be on screen right now, given playback
        // time and frame rate? Indices are 1-based to match `mp4`'s
        // `read_sample` convention.
        let target_sample = ((self.elapsed * self.fps) as u32).max(1);

        // Already showing the right frame — no new pixels needed.
        if target_sample <= self.last_displayed_sample {
            return None;
        }

        // Catch up. If we're behind by many frames (engine stutter),
        // decode through them until current — the last decoded YUV
        // wins out and lands in `rgba_buf`.
        let mut produced_frame = false;
        while self.last_displayed_sample < target_sample {
            match self.decode_next_sample() {
                Ok(true)  => produced_frame = true,
                Ok(false) => {} // sample produced no frame — keep going
                Err(())   => {
                    // EOF.
                    if self.looped {
                        self.restart();
                        if self.inner.is_none() { return None; }
                    } else {
                        return None;
                    }
                }
            }
        }

        if produced_frame { Some(&self.rgba_buf) } else { None }
    }

    fn width(&self)  -> u32 { self.width }
    fn height(&self) -> u32 { self.height }
}

// ──────────────────────────────────────────────────────────────────────
// Decoder integration sketch — Slint composite path
//
// Stage 2 (decoder integration):
// ───────────────────────────────
// `bevy_video` exposes a `VideoStream` resource that decodes file paths
// and offers a `next_frame() -> Option<&[u8]>` style API. The integration
// is a `BevyVideoSource` struct holding a `VideoStream` + buffer cache;
// implementing `VideoFrameSource::next_frame` is then a pass-through
// after format conversion (YUV→RGBA if needed). Add `bevy_video` as an
// optional dep gated by a `video-decoder` feature flag so users on
// constrained build environments can opt out.
//
// Compatibility unknowns (verify before committing):
//   - bevy_video's last-known-good Bevy version (might trail 0.18; if
//     so, fork or pin to whichever branch tracks 0.18).
//   - Native deps: bevy_video → ffmpeg-next → ffmpeg shared libs. Windows
//     install requires either vcpkg-installed FFmpeg or pkg-config setup.
//   - Hardware accel paths (vaapi/d3d11) — bevy_video may or may not
//     plumb these through; software decode is fine for the MVP.
//
// Stage 3 (Slint composite for BillboardGui / SurfaceGui / UI):
// ─────────────────────────────────────────────────────────────
// `BillboardCard.slint` renders into a software-renderer staging buffer
// that becomes the quad's albedo texture. To embed video inside one of
// these cards, we composite as follows in `update_and_render_billboards`:
//
//   1. Slint renders UI to staging.pixels (already happens).
//   2. For each video frame embedded in this card, blit the
//      `VideoPlayer.texture`'s current pixels into a sub-rect of
//      staging.pixels BEFORE the upload to GPU. Order: video underneath,
//      Slint UI overlaid (with its own alpha).
//   3. Single upload pass to the GPU image (already happens).
//
// Knowing where each video element lives inside the BillboardCard
// requires the Slint component to expose layout info — easiest is a
// new `VideoSlot` Slint struct with x/y/width/height/video-id, and the
// engine reads back those slot rectangles after Slint renders to know
// where to blit.
//
// Same idea for screen-space Slint UI (StudioWindow) — `render_slint_to_texture`
// gets a "blit videos before this upload" step. The hard part is
// teaching Slint how to express "leave a hole here for the engine to
// fill" without breaking the software-render abstraction. A first
// pass: video elements render as fully-transparent rectangles in
// Slint, and the engine composites underneath; alpha-over compositing
// then yields correct layering.
