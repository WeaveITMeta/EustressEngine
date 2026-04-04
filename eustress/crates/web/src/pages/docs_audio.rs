// =============================================================================
// Eustress Web - Audio System Documentation Page
// =============================================================================
// Comprehensive documentation on the Eustress audio system covering spatial 3D
// audio, music tracks, sound effects, ambient soundscapes, scripting APIs,
// audio zones, and performance optimization.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Table of Contents Data
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct TocSection {
    id: &'static str,
    title: &'static str,
    subsections: Vec<TocSubsection>,
}

#[derive(Clone, Debug, PartialEq)]
struct TocSubsection {
    id: &'static str,
    title: &'static str,
}

fn get_toc() -> Vec<TocSection> {
    vec![
        TocSection {
            id: "overview",
            title: "Overview",
            subsections: vec![
                TocSubsection { id: "overview-architecture", title: "Architecture" },
                TocSubsection { id: "overview-formats", title: "Supported Formats" },
                TocSubsection { id: "overview-pipeline", title: "Audio Pipeline" },
            ],
        },
        TocSection {
            id: "components",
            title: "Sound Components",
            subsections: vec![
                TocSubsection { id: "components-source", title: "AudioSource" },
                TocSubsection { id: "components-emitter", title: "AudioEmitter" },
                TocSubsection { id: "components-listener", title: "AudioListener" },
                TocSubsection { id: "components-settings", title: "Playback Settings" },
            ],
        },
        TocSection {
            id: "spatial",
            title: "Spatial Audio",
            subsections: vec![
                TocSubsection { id: "spatial-positional", title: "Positional Audio" },
                TocSubsection { id: "spatial-attenuation", title: "Distance Attenuation" },
                TocSubsection { id: "spatial-doppler", title: "Doppler Effect" },
                TocSubsection { id: "spatial-hrtf", title: "HRTF" },
            ],
        },
        TocSection {
            id: "music",
            title: "Music System",
            subsections: vec![
                TocSubsection { id: "music-tracks", title: "Background Tracks" },
                TocSubsection { id: "music-crossfade", title: "Crossfading" },
                TocSubsection { id: "music-layers", title: "Layered Music" },
                TocSubsection { id: "music-adaptive", title: "Adaptive Music" },
            ],
        },
        TocSection {
            id: "sfx",
            title: "Sound Effects",
            subsections: vec![
                TocSubsection { id: "sfx-oneshot", title: "One-Shot Sounds" },
                TocSubsection { id: "sfx-pools", title: "Sound Pools" },
                TocSubsection { id: "sfx-ambient", title: "Ambient Loops" },
            ],
        },
        TocSection {
            id: "scripting",
            title: "Scripting API",
            subsections: vec![
                TocSubsection { id: "scripting-rune", title: "Rune API" },
                TocSubsection { id: "scripting-luau", title: "Luau API" },
                TocSubsection { id: "scripting-events", title: "Event Triggers" },
            ],
        },
        TocSection {
            id: "zones",
            title: "Audio Zones",
            subsections: vec![
                TocSubsection { id: "zones-reverb", title: "Reverb Zones" },
                TocSubsection { id: "zones-transitions", title: "Zone Transitions" },
                TocSubsection { id: "zones-presets", title: "Environment Presets" },
            ],
        },
        TocSection {
            id: "performance",
            title: "Performance",
            subsections: vec![
                TocSubsection { id: "performance-streaming", title: "Audio Streaming" },
                TocSubsection { id: "performance-voices", title: "Voice Limiting" },
                TocSubsection { id: "performance-pooling", title: "Channel Pooling" },
            ],
        },
    ]
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Audio System documentation page.
#[component]
pub fn DocsAudioPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            // Background
            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-audio"></div>
            </div>

            <div class="docs-layout">
                // Floating TOC Sidebar
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/audio.svg" alt="Audio" class="toc-icon" />
                        <h2>"Audio System"</h2>
                    </div>
                    <nav class="toc-nav">
                        {get_toc().into_iter().map(|section| {
                            let section_id = section.id.to_string();
                            let is_active = {
                                let section_id = section_id.clone();
                                move || active_section.get() == section_id
                            };
                            view! {
                                <div class="toc-section">
                                    <a
                                        href=format!("#{}", section.id)
                                        class="toc-section-title"
                                        class:active=is_active
                                    >
                                        {section.title}
                                    </a>
                                    <div class="toc-subsections">
                                        {section.subsections.into_iter().map(|sub| {
                                            view! {
                                                <a href=format!("#{}", sub.id) class="toc-subsection">
                                                    {sub.title}
                                                </a>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </div>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </nav>

                    <div class="toc-footer">
                        <a href="/learn" class="toc-back">
                            <img src="/assets/icons/arrow-left.svg" alt="Back" />
                            "Back to Learn"
                        </a>
                    </div>
                </aside>

                // Main Content
                <main class="docs-content">
                    // Hero
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Audio System"</span>
                        </div>
                        <h1 class="docs-title">"Audio System"</h1>
                        <p class="docs-subtitle">
                            "Immersive 3D audio powered by Kira and rodio. Spatial sound, adaptive
                            music, ambient soundscapes, and scriptable audio events. Bring your
                            worlds to life with sound."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "25 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Level" />
                                "Intermediate"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "v0.16.1"
                            </span>
                        </div>
                    </header>

                    // ─────────────────────────────────────────────────────
                    // 1. Overview
                    // ─────────────────────────────────────────────────────
                    <section id="overview" class="docs-section">
                        <h2 class="section-anchor">"1. Overview"</h2>

                        <div id="overview-architecture" class="docs-block">
                            <h3>"Architecture"</h3>
                            <p>
                                "The Eustress audio system is built on two battle-tested Rust audio
                                libraries: "<strong>"Kira"</strong>" for high-level music and sound
                                management, and "<strong>"rodio"</strong>" for low-level audio
                                decoding and output. Together they provide a complete audio pipeline
                                that integrates natively with the Bevy ECS."
                            </p>
                            <p>
                                "Every sound in Eustress flows through a consistent pipeline: asset
                                loading, decoding, spatial processing, effects, mixing, and finally
                                output to the system audio device. The entire pipeline runs on a
                                dedicated audio thread to avoid frame hitches."
                            </p>
                            <div class="feature-grid">
                                <div class="feature-item">
                                    <h4>"Spatial 3D Audio"</h4>
                                    <p>"Positional sounds with distance attenuation, Doppler, and HRTF"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Music Engine"</h4>
                                    <p>"Crossfading tracks, layered stems, adaptive state-driven music"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Sound Effects"</h4>
                                    <p>"One-shot, looping, pooled, and randomized variation sounds"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Ambient Soundscapes"</h4>
                                    <p>"Environmental audio layers, reverb zones, and smooth transitions"</p>
                                </div>
                            </div>
                        </div>

                        <div id="overview-formats" class="docs-block">
                            <h3>"Supported Formats"</h3>
                            <p>
                                "Eustress supports the most common audio formats out of the box.
                                Assets are loaded through the Bevy asset pipeline with automatic
                                format detection."
                            </p>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Format"</th>
                                            <th>"Extension"</th>
                                            <th>"Compression"</th>
                                            <th>"Best For"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"WAV"</td>
                                            <td>".wav"</td>
                                            <td>"None (PCM)"</td>
                                            <td>"Short SFX, low-latency triggers"</td>
                                        </tr>
                                        <tr>
                                            <td>"OGG Vorbis"</td>
                                            <td>".ogg"</td>
                                            <td>"Lossy"</td>
                                            <td>"Music, ambient loops, dialogue"</td>
                                        </tr>
                                        <tr>
                                            <td>"FLAC"</td>
                                            <td>".flac"</td>
                                            <td>"Lossless"</td>
                                            <td>"High-fidelity music, mastered tracks"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <div class="docs-callout info">
                                <strong>"Tip:"</strong>
                                " Use WAV for short sound effects (under 2 seconds) where latency
                                matters. Use OGG for everything else to save memory. FLAC is ideal
                                when you need lossless quality for cinematic audio."
                            </div>
                        </div>

                        <div id="overview-pipeline" class="docs-block">
                            <h3>"Audio Pipeline"</h3>
                            <p>
                                "The audio pipeline processes sound through several stages before
                                it reaches the speakers:"
                            </p>
                            <ol class="docs-list numbered">
                                <li><strong>"Asset Loading"</strong>" — Files loaded via Bevy AssetServer, decoded on background thread"</li>
                                <li><strong>"Source Creation"</strong>" — AudioSource handle created, ready for playback"</li>
                                <li><strong>"Spatial Processing"</strong>" — Position, distance, and HRTF applied"</li>
                                <li><strong>"Effects Chain"</strong>" — Reverb, echo, filter, and zone effects"</li>
                                <li><strong>"Mixing"</strong>" — All active voices mixed with priority and volume"</li>
                                <li><strong>"Output"</strong>" — Final signal sent to system audio device"</li>
                            </ol>
                            <pre class="code-block"><code>{"// Audio pipeline setup in your app
use eustress_audio::{AudioPlugin, AudioConfig};

app.add_plugins(AudioPlugin {
    config: AudioConfig {
        sample_rate: 48000,
        buffer_size: 512,
        max_voices: 64,
        hrtf_enabled: true,
        ..default()
    },
});"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 2. Sound Components
                    // ─────────────────────────────────────────────────────
                    <section id="components" class="docs-section">
                        <h2 class="section-anchor">"2. Sound Components"</h2>

                        <div id="components-source" class="docs-block">
                            <h3>"AudioSource"</h3>
                            <p>
                                "An "<code>"AudioSource"</code>" is a handle to a loaded audio asset.
                                It represents the raw sound data and can be shared across multiple
                                emitters. Sources are loaded through the Bevy asset system and
                                cached automatically."
                            </p>
                            <pre class="code-block"><code>{"// Load an audio source
let sfx: Handle<AudioSource> = asset_server.load(\"sounds/explosion.ogg\");
let music: Handle<AudioSource> = asset_server.load(\"music/theme.ogg\");

// AudioSource is an asset — it can be reused across entities
commands.spawn(AudioSourceBundle {
    source: sfx.clone(),
    settings: PlaybackSettings::ONCE,
});"}</code></pre>
                        </div>

                        <div id="components-emitter" class="docs-block">
                            <h3>"AudioEmitter"</h3>
                            <p>
                                "The "<code>"AudioEmitter"</code>" component attaches a sound to an
                                entity in the world. When combined with a "<code>"Transform"</code>",
                                the sound becomes spatial — its volume, panning, and filtering change
                                based on the listener's position."
                            </p>
                            <pre class="code-block"><code>{"// Attach a sound emitter to an entity
commands.spawn((
    AudioEmitter {
        source: asset_server.load(\"sounds/engine_loop.ogg\"),
        volume: 0.8,
        pitch: 1.0,
        looping: true,
        spatial: true,
    },
    Transform::from_xyz(10.0, 0.0, -5.0),
    Name::new(\"EngineSound\"),
));"}</code></pre>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Property"</th>
                                            <th>"Type"</th>
                                            <th>"Default"</th>
                                            <th>"Description"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"source"</code></td>
                                            <td>"Handle&lt;AudioSource&gt;"</td>
                                            <td>"Required"</td>
                                            <td>"The audio asset to play"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"volume"</code></td>
                                            <td>"f32"</td>
                                            <td>"1.0"</td>
                                            <td>"Volume multiplier (0.0 to 1.0)"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"pitch"</code></td>
                                            <td>"f32"</td>
                                            <td>"1.0"</td>
                                            <td>"Playback speed/pitch multiplier"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"looping"</code></td>
                                            <td>"bool"</td>
                                            <td>"false"</td>
                                            <td>"Whether the sound loops"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"spatial"</code></td>
                                            <td>"bool"</td>
                                            <td>"true"</td>
                                            <td>"Enable 3D spatial processing"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="components-listener" class="docs-block">
                            <h3>"AudioListener"</h3>
                            <p>
                                "The "<code>"AudioListener"</code>" component designates which entity
                                acts as the \"ears\" in the scene. Typically attached to the camera,
                                it determines how spatial sounds are panned, attenuated, and filtered.
                                Only one listener should be active at a time."
                            </p>
                            <pre class="code-block"><code>{"// Attach the listener to the camera
commands.spawn((
    Camera3d::default(),
    AudioListener {
        left_ear_offset: Vec3::new(-0.1, 0.0, 0.0),
        right_ear_offset: Vec3::new(0.1, 0.0, 0.0),
    },
    Transform::from_xyz(0.0, 1.6, 0.0),
));"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Note:"</strong>
                                " The ear offsets control stereo separation for HRTF processing. The
                                default 0.1m offset models average human ear spacing. Adjust for
                                different head sizes or exaggerated stereo effects."
                            </div>
                        </div>

                        <div id="components-settings" class="docs-block">
                            <h3>"Playback Settings"</h3>
                            <p>
                                "Fine-tune playback behavior with "<code>"PlaybackSettings"</code>":"
                            </p>
                            <pre class="code-block"><code>{"// Common playback presets
PlaybackSettings::ONCE       // Play once, then despawn
PlaybackSettings::LOOP       // Loop forever
PlaybackSettings::DESPAWN    // Despawn entity when finished

// Custom settings
PlaybackSettings {
    mode: PlaybackMode::Loop,
    volume: Volume::Relative(0.5),
    speed: 1.2,
    paused: false,
    fade_in: Duration::from_millis(500),
    fade_out: Duration::from_millis(1000),
}"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 3. Spatial Audio
                    // ─────────────────────────────────────────────────────
                    <section id="spatial" class="docs-section">
                        <h2 class="section-anchor">"3. Spatial Audio"</h2>

                        <div id="spatial-positional" class="docs-block">
                            <h3>"Positional Audio"</h3>
                            <p>
                                "Eustress provides full 3D positional audio. Any entity with both
                                an "<code>"AudioEmitter"</code>" and a "<code>"Transform"</code>"
                                automatically participates in spatial processing. The audio engine
                                calculates the relative position between emitter and listener each
                                frame to produce accurate panning and distance effects."
                            </p>
                            <pre class="code-block"><code>{"// Spawn a spatial sound at a world position
fn spawn_footstep(
    commands: &mut Commands,
    position: Vec3,
    sounds: &FootstepSounds,
) {
    commands.spawn((
        AudioEmitter {
            source: sounds.random(),
            volume: 0.6,
            pitch: rand::thread_rng().gen_range(0.9..1.1),
            looping: false,
            spatial: true,
        },
        SpatialAudio {
            min_distance: 1.0,
            max_distance: 30.0,
            falloff: Falloff::Inverse,
        },
        Transform::from_translation(position),
    ));
}"}</code></pre>
                        </div>

                        <div id="spatial-attenuation" class="docs-block">
                            <h3>"Distance Attenuation"</h3>
                            <p>
                                "Distance attenuation controls how quickly a sound fades as the
                                listener moves away from the emitter. Eustress supports three
                                falloff curves:"
                            </p>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Curve"</th>
                                            <th>"Formula"</th>
                                            <th>"Character"</th>
                                            <th>"Use Case"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><strong>"Linear"</strong></td>
                                            <td><code>"1.0 - (d / max_d)"</code></td>
                                            <td>"Steady fade, predictable"</td>
                                            <td>"UI sounds, simple environments"</td>
                                        </tr>
                                        <tr>
                                            <td><strong>"Inverse"</strong></td>
                                            <td><code>"ref_d / d"</code></td>
                                            <td>"Realistic, natural rolloff"</td>
                                            <td>"Most 3D sounds, voices, footsteps"</td>
                                        </tr>
                                        <tr>
                                            <td><strong>"Exponential"</strong></td>
                                            <td><code>"(d / ref_d) ^ -rolloff"</code></td>
                                            <td>"Aggressive fade, tight radius"</td>
                                            <td>"Small objects, subtle ambient"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <pre class="code-block"><code>{"// Configure distance attenuation
SpatialAudio {
    min_distance: 2.0,   // Full volume within this radius
    max_distance: 50.0,  // Silent beyond this radius
    falloff: Falloff::Inverse,
    rolloff_factor: 1.0, // 1.0 = realistic, >1.0 = faster fade
}"}</code></pre>
                        </div>

                        <div id="spatial-doppler" class="docs-block">
                            <h3>"Doppler Effect"</h3>
                            <p>
                                "The Doppler effect shifts the pitch of a sound based on the relative
                                velocity between emitter and listener. A siren approaching sounds
                                higher pitched; receding sounds lower. Eustress computes this
                                automatically from entity velocities."
                            </p>
                            <pre class="code-block"><code>{"// Enable Doppler effect on an emitter
commands.spawn((
    AudioEmitter { source: siren_sound, ..default() },
    SpatialAudio {
        doppler_factor: 1.0,  // 1.0 = realistic, 0.0 = disabled
        ..default()
    },
    Transform::from_xyz(100.0, 0.0, 0.0),
    Velocity::linear(Vec3::new(-30.0, 0.0, 0.0)), // moving toward origin
));"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Tip:"</strong>
                                " Set "<code>"doppler_factor"</code>" to values above 1.0 for an
                                exaggerated effect (useful for racing games), or below 1.0 for a
                                subtler shift."
                            </div>
                        </div>

                        <div id="spatial-hrtf" class="docs-block">
                            <h3>"HRTF"</h3>
                            <p>
                                "Head-Related Transfer Function (HRTF) processing simulates how sound
                                interacts with the shape of the human head and ears. This enables
                                the listener to perceive height and front/back directionality through
                                standard stereo headphones."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Elevation Cues"</strong>" — Distinguish sounds above and below the listener"</li>
                                <li><strong>"Front/Back Separation"</strong>" — Resolve front-back ambiguity in stereo"</li>
                                <li><strong>"Head Shadow"</strong>" — Frequency-dependent attenuation for off-axis sounds"</li>
                                <li><strong>"ITD/ILD"</strong>" — Interaural time and level differences for precise localization"</li>
                            </ul>
                            <pre class="code-block"><code>{"// HRTF is enabled globally in AudioConfig
AudioConfig {
    hrtf_enabled: true,
    hrtf_dataset: HrtfDataset::Default, // Built-in CIPIC dataset
    ..default()
}"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 4. Music System
                    // ─────────────────────────────────────────────────────
                    <section id="music" class="docs-section">
                        <h2 class="section-anchor">"4. Music System"</h2>

                        <div id="music-tracks" class="docs-block">
                            <h3>"Background Tracks"</h3>
                            <p>
                                "The music system manages background tracks separately from sound
                                effects. Music plays through a dedicated bus with its own volume
                                control, and supports playlists with sequential or shuffled
                                playback order."
                            </p>
                            <pre class="code-block"><code>{"// Play a background music track
fn setup_music(mut music: ResMut<MusicPlayer>, assets: Res<AssetServer>) {
    music.play(assets.load(\"music/exploration.ogg\"), MusicSettings {
        volume: 0.7,
        fade_in: Duration::from_secs(3),
        looping: true,
    });
}

// Build a playlist
music.set_playlist(vec![
    assets.load(\"music/dawn.ogg\"),
    assets.load(\"music/journey.ogg\"),
    assets.load(\"music/dusk.ogg\"),
], PlaylistMode::Sequential);"}</code></pre>
                        </div>

                        <div id="music-crossfade" class="docs-block">
                            <h3>"Crossfading"</h3>
                            <p>
                                "Smooth transitions between tracks using configurable crossfade
                                durations. The outgoing track fades out while the incoming track
                                fades in, with optional overlap control."
                            </p>
                            <pre class="code-block"><code>{"// Crossfade to a new track
music.crossfade_to(
    assets.load(\"music/combat.ogg\"),
    CrossfadeSettings {
        duration: Duration::from_secs(2),
        curve: FadeCurve::EaseInOut,
        overlap: 0.5,  // 50% overlap between tracks
    },
);"}</code></pre>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Fade Curve"</th>
                                            <th>"Behavior"</th>
                                            <th>"Best For"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"FadeCurve::Linear"</code></td>
                                            <td>"Constant rate fade"</td>
                                            <td>"Quick transitions"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FadeCurve::EaseIn"</code></td>
                                            <td>"Slow start, fast finish"</td>
                                            <td>"Dramatic exits"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FadeCurve::EaseOut"</code></td>
                                            <td>"Fast start, slow finish"</td>
                                            <td>"Gentle introductions"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"FadeCurve::EaseInOut"</code></td>
                                            <td>"Smooth S-curve"</td>
                                            <td>"Most music transitions"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="music-layers" class="docs-block">
                            <h3>"Layered Music"</h3>
                            <p>
                                "Music layers allow multiple stems (drums, bass, melody, strings) to
                                play simultaneously. Individual layers can be faded in or out based
                                on game state, creating dynamic soundtracks that respond to gameplay."
                            </p>
                            <pre class="code-block"><code>{"// Set up a layered music track
let layers = MusicLayers::new(vec![
    MusicLayer { name: \"base\",    source: load(\"music/combat_base.ogg\"),    volume: 1.0 },
    MusicLayer { name: \"drums\",   source: load(\"music/combat_drums.ogg\"),   volume: 0.0 },
    MusicLayer { name: \"strings\", source: load(\"music/combat_strings.ogg\"), volume: 0.0 },
    MusicLayer { name: \"choir\",   source: load(\"music/combat_choir.ogg\"),   volume: 0.0 },
]);

music.play_layered(layers);

// During gameplay, fade layers in/out
music.set_layer_volume(\"drums\", 1.0, Duration::from_secs(2));
music.set_layer_volume(\"strings\", 0.8, Duration::from_secs(4));"}</code></pre>
                        </div>

                        <div id="music-adaptive" class="docs-block">
                            <h3>"Adaptive Music"</h3>
                            <p>
                                "Adaptive music reacts to game state automatically. Define rules
                                that map game conditions to music behavior. The system handles
                                transitions, layer blending, and tempo changes."
                            </p>
                            <pre class="code-block"><code>{"// Define adaptive music rules
AdaptiveMusicRules {
    rules: vec![
        MusicRule {
            condition: \"health < 0.3\",
            action: MusicAction::FadeInLayer(\"tension\", 1.0),
        },
        MusicRule {
            condition: \"enemies_nearby > 5\",
            action: MusicAction::CrossfadeTo(\"combat_intense\"),
        },
        MusicRule {
            condition: \"in_safe_zone\",
            action: MusicAction::CrossfadeTo(\"peaceful\"),
        },
    ],
    transition_time: Duration::from_secs(3),
}"}</code></pre>
                            <div class="docs-callout success">
                                <strong>"Pro Tip:"</strong>
                                " Compose your music stems at the same BPM and key so that layer
                                transitions always sound musical. Use a shared click track during
                                recording to keep stems perfectly synchronized."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 5. Sound Effects
                    // ─────────────────────────────────────────────────────
                    <section id="sfx" class="docs-section">
                        <h2 class="section-anchor">"5. Sound Effects"</h2>

                        <div id="sfx-oneshot" class="docs-block">
                            <h3>"One-Shot Sounds"</h3>
                            <p>
                                "One-shot sounds fire once and clean up automatically. They are the
                                simplest way to play a sound effect — spawn the entity, and it
                                despawns itself when playback completes."
                            </p>
                            <pre class="code-block"><code>{"// Fire-and-forget one-shot sound
fn play_explosion(commands: &mut Commands, assets: &AssetServer, pos: Vec3) {
    commands.spawn((
        AudioEmitter {
            source: assets.load(\"sfx/explosion.wav\"),
            volume: 1.0,
            pitch: rand::thread_rng().gen_range(0.85..1.15),
            looping: false,
            spatial: true,
        },
        SpatialAudio {
            min_distance: 5.0,
            max_distance: 100.0,
            falloff: Falloff::Inverse,
        },
        Transform::from_translation(pos),
        DespawnOnFinish,  // Auto-cleanup
    ));
}"}</code></pre>
                        </div>

                        <div id="sfx-pools" class="docs-block">
                            <h3>"Sound Pools"</h3>
                            <p>
                                "Sound pools hold a collection of variations for the same logical
                                sound. When triggered, a random or round-robin selection is made,
                                preventing repetitive audio. Essential for footsteps, impacts, and
                                any frequently-repeated sound."
                            </p>
                            <pre class="code-block"><code>{"// Define a sound pool for footsteps
let footsteps = SoundPool::new(vec![
    assets.load(\"sfx/footstep_01.wav\"),
    assets.load(\"sfx/footstep_02.wav\"),
    assets.load(\"sfx/footstep_03.wav\"),
    assets.load(\"sfx/footstep_04.wav\"),
    assets.load(\"sfx/footstep_05.wav\"),
], PoolMode::RandomNoRepeat);

// Play from pool — never repeats the same sound twice in a row
footsteps.play(commands, PlaySettings {
    volume: 0.5,
    pitch_variation: 0.1,  // +/- 10% pitch randomization
    position: Some(player_pos),
});"}</code></pre>
                            <div class="feature-grid">
                                <div class="feature-item">
                                    <h4>"RandomNoRepeat"</h4>
                                    <p>"Random selection, never plays the same sound twice consecutively"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"RoundRobin"</h4>
                                    <p>"Cycles through sounds in order, wrapping back to start"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Weighted"</h4>
                                    <p>"Assign probability weights to each variation"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Shuffle"</h4>
                                    <p>"Shuffled order, all play before any repeats"</p>
                                </div>
                            </div>
                        </div>

                        <div id="sfx-ambient" class="docs-block">
                            <h3>"Ambient Loops"</h3>
                            <p>
                                "Ambient loops create persistent background audio — wind, rain,
                                machinery hum, crowd noise. They loop seamlessly and can be layered
                                to build complex soundscapes."
                            </p>
                            <pre class="code-block"><code>{"// Layer ambient sounds for a forest scene
fn setup_forest_ambience(commands: &mut Commands, assets: &AssetServer) {
    // Base wind layer
    commands.spawn(AudioEmitter {
        source: assets.load(\"ambient/wind_gentle.ogg\"),
        volume: 0.4,
        looping: true,
        spatial: false,  // Non-spatial, always present
        ..default()
    });

    // Bird calls — spatial, scattered around the scene
    for pos in bird_positions() {
        commands.spawn((
            AudioEmitter {
                source: assets.load(\"ambient/birds_chirp.ogg\"),
                volume: 0.3,
                looping: true,
                spatial: true,
                ..default()
            },
            SpatialAudio { max_distance: 25.0, ..default() },
            Transform::from_translation(pos),
        ));
    }

    // Distant river
    commands.spawn((
        AudioEmitter {
            source: assets.load(\"ambient/river_flow.ogg\"),
            volume: 0.6,
            looping: true,
            spatial: true,
            ..default()
        },
        SpatialAudio { min_distance: 5.0, max_distance: 60.0, ..default() },
        Transform::from_xyz(40.0, -2.0, 15.0),
    ));
}"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 6. Scripting API
                    // ─────────────────────────────────────────────────────
                    <section id="scripting" class="docs-section">
                        <h2 class="section-anchor">"6. Scripting API"</h2>

                        <div id="scripting-rune" class="docs-block">
                            <h3>"Rune API"</h3>
                            <p>
                                "The Rune scripting API provides full access to the audio system.
                                Play sounds, control volume, trigger music transitions, and respond
                                to audio events — all from Rune scripts."
                            </p>
                            <pre class="code-block"><code>{"// Rune: Playing sounds
let sound = Audio::Load(\"sfx/coin_pickup.wav\");
Audio::Play(sound);

// Play at a position
Audio::PlayAt(sound, Vec3(10.0, 0.0, 5.0));

// Control playback
let emitter = Audio::Play(sound);
Audio::SetVolume(emitter, 0.5);
Audio::SetPitch(emitter, 1.2);
Audio::Pause(emitter);
Audio::Resume(emitter);
Audio::Stop(emitter);

// Music control
Audio::PlayMusic(\"music/boss_fight.ogg\", 2.0);  // 2s fade-in
Audio::StopMusic(3.0);  // 3s fade-out
Audio::SetMusicVolume(0.8);"}</code></pre>
                        </div>

                        <div id="scripting-luau" class="docs-block">
                            <h3>"Luau API"</h3>
                            <p>
                                "The Luau scripting API mirrors the Rune API with Lua-style syntax.
                                Identical functionality, familiar syntax for Lua developers."
                            </p>
                            <pre class="code-block"><code>{"-- Luau: Playing sounds
local sound = Audio:Load(\"sfx/coin_pickup.wav\")
Audio:Play(sound)

-- Play with options
Audio:Play(sound, {
    volume = 0.8,
    pitch = 1.0,
    position = Vector3.new(10, 0, 5),
    loop = false,
})

-- Control playback
local emitter = Audio:Play(sound)
Audio:SetVolume(emitter, 0.5)
Audio:SetPitch(emitter, 1.2)
Audio:Stop(emitter)

-- Music
Audio:PlayMusic(\"music/boss_fight.ogg\", { fadeIn = 2.0 })
Audio:CrossfadeTo(\"music/victory.ogg\", { duration = 3.0 })"}</code></pre>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Function"</th>
                                            <th>"Rune Syntax"</th>
                                            <th>"Luau Syntax"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td>"Play sound"</td>
                                            <td><code>"Audio::Play(id)"</code></td>
                                            <td><code>"Audio:Play(id)"</code></td>
                                        </tr>
                                        <tr>
                                            <td>"Set volume"</td>
                                            <td><code>"Audio::SetVolume(e, v)"</code></td>
                                            <td><code>"Audio:SetVolume(e, v)"</code></td>
                                        </tr>
                                        <tr>
                                            <td>"Stop sound"</td>
                                            <td><code>"Audio::Stop(e)"</code></td>
                                            <td><code>"Audio:Stop(e)"</code></td>
                                        </tr>
                                        <tr>
                                            <td>"Play music"</td>
                                            <td><code>"Audio::PlayMusic(path, fade)"</code></td>
                                            <td><code>"Audio:PlayMusic(path, opts)"</code></td>
                                        </tr>
                                        <tr>
                                            <td>"Set listener"</td>
                                            <td><code>"Audio::SetListener(entity)"</code></td>
                                            <td><code>"Audio:SetListener(entity)"</code></td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                        </div>

                        <div id="scripting-events" class="docs-block">
                            <h3>"Event Triggers"</h3>
                            <p>
                                "Audio events let you trigger sounds in response to game events.
                                Register handlers that fire when collisions occur, animations play,
                                health changes, or any custom event is emitted."
                            </p>
                            <pre class="code-block"><code>{"// Rune: Event-driven audio triggers
on(\"collision\", |event| {
    let impact_force = event.force;
    if impact_force > 10.0 {
        let vol = clamp(impact_force / 100.0, 0.3, 1.0);
        Audio::PlayAt(\"sfx/impact_hard.wav\", event.position);
        Audio::SetVolume(LAST, vol);
    }
});

on(\"player_damage\", |event| {
    Audio::Play(\"sfx/hit_pain.wav\");
    if event.health < 0.2 {
        Audio::Play(\"sfx/heartbeat.wav\");
    }
});

on(\"door_open\", |event| {
    Audio::PlayAt(\"sfx/door_creak.wav\", event.position);
});"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Note:"</strong>
                                " Event-driven audio is the recommended pattern for gameplay sounds.
                                It decouples audio from game logic, making both easier to maintain
                                and test independently."
                            </div>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 7. Audio Zones
                    // ─────────────────────────────────────────────────────
                    <section id="zones" class="docs-section">
                        <h2 class="section-anchor">"7. Audio Zones"</h2>

                        <div id="zones-reverb" class="docs-block">
                            <h3>"Reverb Zones"</h3>
                            <p>
                                "Reverb zones apply environmental acoustics to sounds within a
                                defined volume. When the listener enters a zone, all audio is
                                processed through the zone's reverb settings, simulating the
                                acoustics of different spaces."
                            </p>
                            <pre class="code-block"><code>{"// Create a reverb zone for a cathedral interior
commands.spawn((
    ReverbZone {
        preset: ReverbPreset::Cathedral,
        wet_mix: 0.6,
        dry_mix: 0.4,
        decay_time: 4.5,    // seconds
        early_reflections: 0.3,
        diffusion: 0.9,
    },
    Transform::from_xyz(0.0, 10.0, 0.0),
    Collider::cuboid(20.0, 15.0, 30.0),  // Zone boundary
    Name::new(\"CathedralReverb\"),
));"}</code></pre>
                            <div class="feature-grid">
                                <div class="feature-item">
                                    <h4>"Reverb"</h4>
                                    <p>"Simulates sound reflections in enclosed spaces like halls and caves"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Echo"</h4>
                                    <p>"Distinct delayed reflections for canyons, mountains, large rooms"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Muffle"</h4>
                                    <p>"Low-pass filtering for sounds through walls, underwater, inside vehicles"</p>
                                </div>
                                <div class="feature-item">
                                    <h4>"Occlusion"</h4>
                                    <p>"Geometry-aware filtering when objects block the sound path"</p>
                                </div>
                            </div>
                        </div>

                        <div id="zones-transitions" class="docs-block">
                            <h3>"Zone Transitions"</h3>
                            <p>
                                "When the listener moves between zones, the audio system smoothly
                                blends between their reverb settings. Overlapping zones are mixed
                                based on the listener's position within each volume."
                            </p>
                            <pre class="code-block"><code>{"// Configure zone transition behavior
AudioZoneConfig {
    blend_distance: 3.0,   // meters of gradual transition
    blend_curve: BlendCurve::Smooth,  // Linear, Smooth, or Step
    priority_mode: ZonePriority::Nearest,  // or Loudest, Latest
}

// Zones can overlap — the system interpolates between them
// Example: a cave entrance where outdoor reverb blends into cave reverb
//
//   [Outdoor Zone]----[Blend Region]----[Cave Zone]
//   dry, open          gradual mix       wet, echoey"}</code></pre>
                        </div>

                        <div id="zones-presets" class="docs-block">
                            <h3>"Environment Presets"</h3>
                            <p>
                                "Built-in environment presets model common acoustic spaces. Use them
                                directly or as starting points for custom tuning."
                            </p>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Preset"</th>
                                            <th>"Decay (s)"</th>
                                            <th>"Wet Mix"</th>
                                            <th>"Character"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Outdoor"</code></td>
                                            <td>"0.3"</td>
                                            <td>"0.1"</td>
                                            <td>"Open, minimal reflections"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Room"</code></td>
                                            <td>"0.8"</td>
                                            <td>"0.3"</td>
                                            <td>"Small enclosed space"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Hall"</code></td>
                                            <td>"2.5"</td>
                                            <td>"0.5"</td>
                                            <td>"Large hall, concert venue"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Cathedral"</code></td>
                                            <td>"4.5"</td>
                                            <td>"0.6"</td>
                                            <td>"Massive reverberant space"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Cave"</code></td>
                                            <td>"3.0"</td>
                                            <td>"0.7"</td>
                                            <td>"Rocky, diffuse reflections"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Underwater"</code></td>
                                            <td>"1.5"</td>
                                            <td>"0.8"</td>
                                            <td>"Muffled, heavy low-pass filter"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Tunnel"</code></td>
                                            <td>"2.0"</td>
                                            <td>"0.5"</td>
                                            <td>"Cylindrical, focused echoes"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Bathroom"</code></td>
                                            <td>"1.2"</td>
                                            <td>"0.6"</td>
                                            <td>"Hard surfaces, bright reflections"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <pre class="code-block"><code>{"// Use a preset and customize it
ReverbZone {
    preset: ReverbPreset::Cave,
    // Override specific values from the preset
    decay_time: 5.0,  // Even longer decay for a huge cavern
    ..ReverbPreset::Cave.defaults()
}"}</code></pre>
                        </div>
                    </section>

                    // ─────────────────────────────────────────────────────
                    // 8. Performance
                    // ─────────────────────────────────────────────────────
                    <section id="performance" class="docs-section">
                        <h2 class="section-anchor">"8. Performance"</h2>

                        <div id="performance-streaming" class="docs-block">
                            <h3>"Audio Streaming"</h3>
                            <p>
                                "Large audio files (music tracks, long ambient loops) are streamed
                                from disk rather than loaded entirely into memory. The audio engine
                                reads ahead in a background thread, keeping a small buffer in memory."
                            </p>
                            <ul class="docs-list">
                                <li><strong>"Automatic Streaming"</strong>" — Files over 512KB are streamed by default"</li>
                                <li><strong>"Configurable Threshold"</strong>" — Adjust the streaming cutoff per asset"</li>
                                <li><strong>"Read-Ahead Buffer"</strong>" — 2 seconds of audio buffered ahead"</li>
                                <li><strong>"Seek Support"</strong>" — Streaming files support random seek"</li>
                            </ul>
                            <pre class="code-block"><code>{"// Force streaming or preloading for specific assets
AudioLoadSettings {
    mode: AudioLoadMode::Stream,  // Always stream this file
    buffer_ahead: Duration::from_secs(3),  // 3s read-ahead
}

// Or force preload for a short file you need instant access to
AudioLoadSettings {
    mode: AudioLoadMode::Preload,  // Decode fully into memory
}"}</code></pre>
                            <div class="docs-callout info">
                                <strong>"Memory Note:"</strong>
                                " A 3-minute OGG music track uses approximately 5MB when streamed
                                (buffer only) versus 30MB when fully preloaded. Always stream music
                                tracks and long ambient loops."
                            </div>
                        </div>

                        <div id="performance-voices" class="docs-block">
                            <h3>"Voice Limiting"</h3>
                            <p>
                                "Voice limiting prevents audio overload by capping the number of
                                simultaneous sounds. When the limit is reached, the system uses a
                                priority-based eviction policy to determine which sounds to steal."
                            </p>
                            <div class="comparison-table">
                                <table class="docs-table">
                                    <thead>
                                        <tr>
                                            <th>"Priority"</th>
                                            <th>"Value"</th>
                                            <th>"Use Case"</th>
                                            <th>"Steal Behavior"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        <tr>
                                            <td><code>"Critical"</code></td>
                                            <td>"0"</td>
                                            <td>"UI, dialogue, alerts"</td>
                                            <td>"Never stolen"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"High"</code></td>
                                            <td>"1"</td>
                                            <td>"Player sounds, weapons"</td>
                                            <td>"Steals Low/Normal"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Normal"</code></td>
                                            <td>"2"</td>
                                            <td>"NPC voices, impacts"</td>
                                            <td>"Steals Low"</td>
                                        </tr>
                                        <tr>
                                            <td><code>"Low"</code></td>
                                            <td>"3"</td>
                                            <td>"Ambient detail, distant"</td>
                                            <td>"First to be stolen"</td>
                                        </tr>
                                    </tbody>
                                </table>
                            </div>
                            <pre class="code-block"><code>{"// Configure voice limits
AudioConfig {
    max_voices: 64,           // Hard cap on simultaneous sounds
    voice_steal_policy: StealPolicy::LowestPriority,
    ..default()
}

// Set priority per emitter
AudioEmitter {
    source: explosion_sound,
    priority: AudioPriority::High,
    ..default()
}"}</code></pre>
                        </div>

                        <div id="performance-pooling" class="docs-block">
                            <h3>"Channel Pooling"</h3>
                            <p>
                                "Channel pooling pre-allocates audio channels to avoid runtime
                                allocation. Channels are recycled when sounds finish, keeping memory
                                usage stable and avoiding allocation spikes during intense audio
                                moments."
                            </p>
                            <pre class="code-block"><code>{"// Configure channel pools
AudioConfig {
    pools: vec![
        ChannelPool { name: \"sfx\",     channels: 32, priority: Normal },
        ChannelPool { name: \"music\",   channels: 4,  priority: Critical },
        ChannelPool { name: \"ambient\", channels: 16, priority: Low },
        ChannelPool { name: \"ui\",      channels: 8,  priority: Critical },
        ChannelPool { name: \"voice\",   channels: 4,  priority: High },
    ],
    ..default()
}"}</code></pre>
                            <div class="docs-callout success">
                                <strong>"Performance Checklist:"</strong>
                                <ul class="docs-list">
                                    <li>"Stream music and long ambient loops (over 5 seconds)"</li>
                                    <li>"Preload short SFX (under 2 seconds) for zero-latency triggers"</li>
                                    <li>"Use sound pools to avoid duplicate loads of similar sounds"</li>
                                    <li>"Set voice limits appropriate for your target platform"</li>
                                    <li>"Assign priorities so critical audio is never stolen"</li>
                                    <li>"Profile with the audio debugger overlay (F8 in dev builds)"</li>
                                </ul>
                            </div>
                        </div>
                    </section>

                    // Navigation footer
                    <nav class="docs-nav-footer">
                        <a href="/docs/realism" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Realism Platform"</span>
                            </div>
                        </a>
                        <a href="/docs/publishing" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Publishing"</span>
                            </div>
                            <img src="/assets/icons/arrow-right.svg" alt="Next" />
                        </a>
                    </nav>
                </main>
            </div>

            <Footer />
        </div>
    }
}
