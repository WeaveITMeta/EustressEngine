// =============================================================================
// Eustress Web - Audio Documentation Page
// =============================================================================
// Audio: the Sound object, the SoundService folder, the audio stack compiled
// into Eustress Engine today, and the playback path that is still to be wired.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

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
                TocSubsection { id: "overview-today", title: "Audio Today" },
                TocSubsection { id: "overview-stack", title: "The Audio Stack" },
            ],
        },
        TocSection {
            id: "sound",
            title: "The Sound Object",
            subsections: vec![
                TocSubsection { id: "sound-add", title: "Adding a Sound" },
                TocSubsection { id: "sound-properties", title: "Properties and Defaults" },
                TocSubsection { id: "sound-import", title: "Sounds from Roblox" },
            ],
        },
        TocSection {
            id: "files",
            title: "Files and Formats",
            subsections: vec![
                TocSubsection { id: "files-folder", title: "Audio Files in a Space" },
                TocSubsection { id: "files-formats", title: "Formats" },
            ],
        },
        TocSection {
            id: "scripting",
            title: "Scripting",
            subsections: vec![
                TocSubsection { id: "scripting-luau", title: "Luau" },
                TocSubsection { id: "scripting-rune", title: "Rune" },
            ],
        },
        TocSection {
            id: "roadmap",
            title: "What's Next",
            subsections: vec![
                TocSubsection { id: "roadmap-playback", title: "Playback" },
                TocSubsection { id: "roadmap-spatial", title: "Spatial Sound" },
            ],
        },
    ]
}

/// Where a Sound's journey ends today: its file loads into Studio as an
/// object, and the two links after that are not in place yet.
#[component]
fn SoundPathDiagram() -> impl IntoView {
    view! {
        <figure class="docs-figure">
            <svg class="docs-diagram" viewBox="0 0 640 170" role="img"
                aria-label="A Sound file loads into Studio as a Sound object. The step from the object to a Bevy audio player is not wired yet, and no decoder is built in to turn the file into sound.">
                <defs>
                    <marker id="snd-arrow" viewBox="0 0 10 10" refX="9" refY="5"
                        markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                        <path d="M 0 0 L 10 5 L 0 10 z" class="dg-arrowhead"></path>
                    </marker>
                </defs>

                <rect x="10" y="60" width="130" height="56" rx="8" class="dg-box"></rect>
                <text x="75" y="84" class="dg-label" text-anchor="middle">"Sound file"</text>
                <text x="75" y="102" class="dg-note" text-anchor="middle">"_instance.toml"</text>

                <line x1="140" y1="88" x2="173" y2="88" class="dg-line dg-line-accent" marker-end="url(#snd-arrow)"></line>
                <text x="157" y="52" class="dg-note" text-anchor="middle">"loads"</text>

                <rect x="175" y="60" width="130" height="56" rx="8" class="dg-box dg-box-accent"></rect>
                <text x="240" y="84" class="dg-label" text-anchor="middle">"Sound object"</text>
                <text x="240" y="102" class="dg-note" text-anchor="middle">"in Studio"</text>

                <line x1="305" y1="88" x2="338" y2="88" class="dg-line dg-line-dashed" marker-end="url(#snd-arrow)"></line>
                <text x="322" y="140" class="dg-note" text-anchor="middle">"not wired yet"</text>

                <rect x="340" y="60" width="130" height="56" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="405" y="84" class="dg-label" text-anchor="middle">"Audio player"</text>
                <text x="405" y="102" class="dg-note" text-anchor="middle">"Bevy audio"</text>

                <line x1="470" y1="88" x2="503" y2="88" class="dg-line dg-line-dashed" marker-end="url(#snd-arrow)"></line>
                <text x="487" y="140" class="dg-note" text-anchor="middle">"no decoder"</text>

                <rect x="505" y="60" width="125" height="56" rx="8" class="dg-box dg-box-muted"></rect>
                <text x="567" y="84" class="dg-label" text-anchor="middle">"Speakers"</text>
                <text x="567" y="102" class="dg-note" text-anchor="middle">"output device"</text>
            </svg>
            <figcaption>
                "A Sound's file loads into Studio as an object today. Two links come next: the step
                that turns the object into a Bevy audio player, and a decoder for its audio file."
            </figcaption>
        </figure>
    }
}

/// Audio documentation page.
#[component]
pub fn DocsAudioPage() -> impl IntoView {
    let active_section = RwSignal::new("overview".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-audio"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/audio.svg" alt="Audio" class="toc-icon" />
                        <h2>"Audio"</h2>
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

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/learn">"Learn"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Audio"</span>
                        </div>
                        <h1 class="docs-title">"Audio"</h1>
                        <p class="docs-subtitle">
                            "Audio in Eustress is built around the Sound object: a Sound records which audio
                            file to play and how loud, how fast and how far it carries, and SoundService is
                            the folder where a Space keeps its audio. Eustress Engine stores and imports
                            Sounds today; playing them is the next step."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "7 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/cube.svg" alt="Level" />
                                "Beginner"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "Updated Sep 2026"
                            </span>
                        </div>
                    </header>

                    // =========================================================
                    // OVERVIEW
                    // =========================================================
                    <section id="overview" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"01"</span>
                            "Overview"
                        </h2>

                        <div id="overview-today" class="subsection">
                            <h3>"Audio Today"</h3>
                            <p>
                                "A Sound is an object that describes a piece of audio: the file it plays, its
                                volume, its speed, whether it loops, and how its loudness falls off with
                                distance. You can insert Sounds in Studio, edit their settings in their files,
                                and bring them in from Roblox places with their settings intact."
                            </p>
                            <p>
                                "Eustress Engine does not play audio yet. The settings a Sound holds are kept
                                with the Space, ready for the playback step described in "
                                <a href="#roadmap">"What's Next"</a>"."
                            </p>
                            <SoundPathDiagram />
                        </div>

                        <div id="overview-stack" class="subsection">
                            <h3>"The Audio Stack"</h3>
                            <p>
                                "Eustress runs on Bevy, and Bevy's audio plugin is compiled into Eustress
                                Engine. This is what each part of the stack does today:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Part"</th><th>"In Eustress Engine today"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Bevy audio"</td><td>"Built in. At startup it opens your default audio output device through the rodio and cpal libraries."</td></tr>
                                    <tr><td>"Decoders"</td><td>"None built in. Bevy decodes each format only when its feature (vorbis, wav, mp3, flac) is switched on, and none is."</td></tr>
                                    <tr><td>"Kira"</td><td>"Not used."</td></tr>
                                    <tr><td>"Eustress Player"</td><td>"Built without Bevy audio."</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // THE SOUND OBJECT
                    // =========================================================
                    <section id="sound" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"02"</span>
                            "The Sound Object"
                        </h2>

                        <div id="sound-add" class="subsection">
                            <h3>"Adding a Sound"</h3>
                            <p>
                                "Open the Insert menu and pick "<strong>"Sound"</strong>" under "
                                <strong>"Audio"</strong>". Studio writes a new "<code>"Sound"</code>" folder
                                with an "<code>"_instance.toml"</code>" into the folder of the object you
                                have selected, or into SoundService when nothing is selected. Sound is the
                                only audio class the Insert menu offers."
                            </p>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"A Sound in SoundService is not listed"</strong>
                                    <p>
                                        "The Explorer shows SoundService as a single row without its contents,
                                        so a Sound inserted there does not appear under it. Select a part or a
                                        folder first to keep the Sound where you can see it."
                                    </p>
                                </div>
                            </div>
                        </div>

                        <div id="sound-properties" class="subsection">
                            <h3>"Properties and Defaults"</h3>
                            <p>
                                "A Sound's settings live in the "<code>"[sound]"</code>" table of its "
                                <code>"_instance.toml"</code>". This is the table a new Sound starts with:"
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"SoundService/Sound/_instance.toml"</span>
                                </div>
                                <pre><code class="language-toml">{r#"[sound]
looped = false
playback_speed = 1.0
playing = false
rolloff_max_distance = 10000.0
rolloff_min_distance = 10.0
rolloff_mode = "InverseTapered"
sound_id = ""
time_position = 0.0
volume = 0.5"#}</code></pre>
                            </div>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Key"</th><th>"Default"</th><th>"Holds"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>"sound_id"</code></td><td>"empty"</td><td>"The audio file or asset to play"</td></tr>
                                    <tr><td><code>"volume"</code></td><td>"0.5"</td><td>"Loudness, from 0 to 1"</td></tr>
                                    <tr><td><code>"playing"</code></td><td>"false"</td><td>"Whether the Sound is playing"</td></tr>
                                    <tr><td><code>"looped"</code></td><td>"false"</td><td>"Whether it starts again when it ends"</td></tr>
                                    <tr><td><code>"playback_speed"</code></td><td>"1.0"</td><td>"Speed multiplier; 1 is normal speed"</td></tr>
                                    <tr><td><code>"time_position"</code></td><td>"0.0"</td><td>"Playback position, in seconds"</td></tr>
                                    <tr><td><code>"rolloff_min_distance"</code></td><td>"10.0"</td><td>"Within this many meters, a spatial Sound is at full volume"</td></tr>
                                    <tr><td><code>"rolloff_max_distance"</code></td><td>"10000.0"</td><td>"Beyond this many meters, it is silent"</td></tr>
                                    <tr><td><code>"rolloff_mode"</code></td><td>"InverseTapered"</td><td>"How loudness falls between those two distances"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Distances are in meters, like everything else in a Space. A Sound is spatial
                                by default, so its position in the world will decide how loud it is and which
                                ear it favors once playback is wired."
                            </p>
                        </div>

                        <div id="sound-import" class="subsection">
                            <h3>"Sounds from Roblox"</h3>
                            <p>
                                "When you import a Roblox place, each Sound keeps its Volume, Looped,
                                Playing, PlaybackSpeed, TimePosition, RollOffMinDistance, RollOffMaxDistance
                                and RollOffMode in the same "<code>"[sound]"</code>" table, and its SoundId
                                is recorded as an asset reference. The whole import workflow is in "
                                <a href="/docs/importing">"Importing"</a>"."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // FILES AND FORMATS
                    // =========================================================
                    <section id="files" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"03"</span>
                            "Files and Formats"
                        </h2>

                        <div id="files-folder" class="subsection">
                            <h3>"Audio Files in a Space"</h3>
                            <p>
                                "SoundService is the only folder where the Space loader looks for audio
                                files. When a Space opens, an "<code>".ogg"</code>", "<code>".mp3"</code>",
                                "<code>".wav"</code>" or "<code>".flac"</code>" file there is recognized as
                                audio and noted in the log as not yet loadable; no object is created for it.
                                Audio files in any other folder are skipped."
                            </p>
                            <p>
                                "SoundService's own settings, such as AmbientReverb and DopplerScale, are
                                listed on the "<a href="/docs/services#content-sound">"Services"</a>" page. No
                                system reads them yet."
                            </p>
                        </div>

                        <div id="files-formats" class="subsection">
                            <h3>"Formats"</h3>
                            <p>
                                "The loader knows four audio extensions by name. None of them can be decoded
                                yet, because no Bevy decoder feature is switched on:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Extension"</th><th>"Recognized in SoundService"</th><th>"Decoder built in"</th><th>"Bevy feature that adds it"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td><code>".ogg"</code></td><td>"Yes"</td><td>"No"</td><td><code>"vorbis"</code>" (also "<code>".oga"</code>", "<code>".spx"</code>")"</td></tr>
                                    <tr><td><code>".mp3"</code></td><td>"Yes"</td><td>"No"</td><td><code>"mp3"</code></td></tr>
                                    <tr><td><code>".wav"</code></td><td>"Yes"</td><td>"No"</td><td><code>"wav"</code></td></tr>
                                    <tr><td><code>".flac"</code></td><td>"Yes"</td><td>"No"</td><td><code>"flac"</code></td></tr>
                                </tbody>
                            </table>
                        </div>
                    </section>

                    // =========================================================
                    // SCRIPTING
                    // =========================================================
                    <section id="scripting" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"04"</span>
                            "Scripting"
                        </h2>

                        <div id="scripting-luau" class="subsection">
                            <h3>"Luau"</h3>
                            <p>
                                <code>{r#"Instance.new("Sound")"#}</code>" returns a Sound table with "
                                <code>"SoundId"</code>" (empty), "<code>"Volume"</code>" (1), "
                                <code>"Playing"</code>" (false) and "<code>"Looped"</code>" (false). The "
                                <code>"SoundService"</code>" global has one function, "
                                <code>"PlayLocalSound"</code>", which sets a Sound's "<code>"Playing"</code>
                                " to true and writes its SoundId to the log. No sound is heard."
                            </p>
                            <div class="code-block">
                                <div class="code-header">
                                    <span class="code-lang">"Luau"</span>
                                </div>
                                <pre><code class="language-lua">{r#"local click = Instance.new("Sound")
click.SoundId = "sounds/click.ogg"
click.Volume = 0.8

-- A dot, not a colon: the Sound must be the first argument.
SoundService.PlayLocalSound(click)
print(click.Playing)  -- true; the log shows the SoundId"#}</code></pre>
                            </div>
                            <div class="callout callout-advanced">
                                <img src="/assets/icons/settings.svg" alt="Advanced" />
                                <div>
                                    <strong>"The Roblox colon form marks SoundService"</strong>
                                    <p>
                                        "PlayLocalSound treats its first argument as the Sound. Written as "
                                        <code>"SoundService:PlayLocalSound(click)"</code>", the first argument is
                                        SoundService itself, so SoundService is marked as playing and "
                                        <code>"click"</code>" is left unchanged."
                                    </p>
                                </div>
                            </div>
                            <p>
                                "SoundService is a global in Luau rather than a member of "
                                <code>"game"</code>", so "<code>{r#"game:GetService("SoundService")"#}</code>
                                " raises an error. "<a href="/docs/scripting">"Scripting"</a>" covers the
                                Luau runtime as a whole."
                            </p>
                        </div>

                        <div id="scripting-rune" class="subsection">
                            <h3>"Rune"</h3>
                            <p>
                                "The "<code>"eustress"</code>" module registers a Sound handle type whose
                                fields ("<code>"entity_id"</code>", "<code>"sound_id"</code>", "
                                <code>"volume"</code>", "<code>"playing"</code>", "<code>"looped"</code>")
                                scripts can read, and three functions: "<code>"sound_play"</code>" and "
                                <code>"sound_stop"</code>" set the handle's "<code>"playing"</code>" field,
                                and "<code>"sound_set_volume"</code>" sets its "<code>"volume"</code>",
                                clamped to 0 to 1. No Rune function returns a Sound handle yet, so a script
                                cannot reach a Sound this way today."
                            </p>
                        </div>
                    </section>

                    // =========================================================
                    // WHAT'S NEXT
                    // =========================================================
                    <section id="roadmap" class="docs-section">
                        <h2 class="section-title">
                            <span class="section-number">"05"</span>
                            "What's Next"
                        </h2>

                        <div id="roadmap-playback" class="subsection">
                            <h3>"Playback"</h3>
                            <p>
                                "The code that turns a Sound into a Bevy audio player is already written and
                                registered. It will run once the Space loader creates Sounds through it and a
                                decoder feature is switched on. It loads the SoundId as a Bevy asset path and
                                maps the rest of the Sound like this:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Sound"</th><th>"Bevy playback"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Looped"</td><td>"Loop, or play once"</td></tr>
                                    <tr><td>"Volume"</td><td>"Linear volume, never below 0"</td></tr>
                                    <tr><td>"Playback speed"</td><td>"Speed, at least 0.01"</td></tr>
                                    <tr><td>"Playing"</td><td>"Paused while false"</td></tr>
                                    <tr><td>"Spatial"</td><td>"Spatial playback, scaled by roll-off (below)"</td></tr>
                                </tbody>
                            </table>
                        </div>

                        <div id="roadmap-spatial" class="subsection">
                            <h3>"Spatial Sound"</h3>
                            <p>
                                "Bevy's spatial audio pans between two ears and fades with distance on its
                                own, with no per-sound falloff setting. The spawner will emulate each
                                roll-off mode by scaling the Sound's position by its minimum distance:"
                            </p>
                            <table class="docs-table">
                                <thead>
                                    <tr><th>"Roll-off mode"</th><th>"Position scale"</th></tr>
                                </thead>
                                <tbody>
                                    <tr><td>"Inverse (the default), Logarithmic, Custom"</td><td>"1 / min distance"</td></tr>
                                    <tr><td>"InverseSquared"</td><td>"1 / min distance²"</td></tr>
                                    <tr><td>"Linear"</td><td>"1 / (2 × min distance)"</td></tr>
                                    <tr><td>"None"</td><td>"0, so position has no effect"</td></tr>
                                </tbody>
                            </table>
                            <p>
                                "Sounds will also need a listener on the camera so they pan with your view;
                                no camera carries one yet."
                            </p>
                            <div class="future-cta">
                                <p><strong>"Author your Sounds now. They will play where you put them."</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="/docs/scripting" class="btn-secondary-steel">"Scripting Docs"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/ui" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"UI Systems"</span>
                            </div>
                        </a>
                        <a href="/docs/scripting" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Scripting"</span>
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
