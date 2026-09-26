// =============================================================================
// Eustress Web - Download Player Page (Industrial Design)
// =============================================================================
// Eustress Player plays published simulations and joins multiplayer sessions;
// Eustress Engine is what makes them. The download buttons and the version
// come from the Player's release manifest, player/latest.json.
// =============================================================================

use leptos::prelude::*;
use crate::api::releases::ReleaseState;
use crate::components::{use_player_release, CentralNav, Footer, PlayerPlatformButtons};

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Download Eustress Player page — play experiences without the full engine.
#[component]
pub fn DownloadPlayerPage() -> impl IntoView {
    let release = use_player_release();

    view! {
        <div class="page page-download-industrial">
            <CentralNav active="".to_string() />

            // Background
            <div class="download-bg">
                <div class="download-grid-overlay"></div>
                <div class="download-glow glow-1"></div>
                <div class="download-glow glow-2"></div>
            </div>

            // Hero Section
            <section class="download-hero">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"DOWNLOAD"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="download-title">"Eustress Player"</h1>
                <p class="download-tagline">"Play published simulations and join multiplayer sessions. Free."</p>

                // Version Info: the badge appears once a release exists.
                <div class="version-info">
                    {move || match release.get() {
                        ReleaseState::Released(manifest) => Some(view! {
                            <span class="version-badge">{format!("v{}", manifest.version)}</span>
                        }),
                        _ => None,
                    }}
                    <span class="version-label">"Public Alpha"</span>
                </div>
            </section>

            // Primary Download Section
            <section class="primary-download">
                <div class="download-card-main">
                    <div class="download-icon-large">
                        <img src="/assets/icons/gamepad.svg" alt="Player" />
                    </div>

                    <h2>"Download for Your Platform"</h2>
                    <p class="download-desc">"For Windows, macOS and Linux."</p>

                    // Buttons from player/latest.json, so each release updates
                    // them without a site change.
                    <PlayerPlatformButtons release=release />
                </div>
            </section>

            // Installing. The builds are not signed yet, so Windows and macOS
            // each ask once before the first run. Shown when there is a
            // release to install, since the Linux command names its version.
            {move || match release.get() {
                ReleaseState::Released(manifest) => Some(view! {
                    <section class="requirements-section">
                        <div class="section-header-industrial">
                            <img src="/assets/icons/download.svg" alt="" class="section-icon" />
                            <h2>"Installing"</h2>
                        </div>

                        <div
                            class="requirements-row"
                            style="grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));"
                        >
                            <div class="req-card">
                                <h3>"Windows"</h3>
                                <ul>
                                    <li>"Installs for your account with no administrator prompt, and opens eustress-player:// links from the gallery."</li>
                                    <li>"The installer is not signed yet, so SmartScreen may stop it. Choose More info, then Run anyway."</li>
                                </ul>
                            </div>

                            <div class="req-card">
                                <h3>"macOS (Apple Silicon)"</h3>
                                <ul>
                                    <li>"Open the disk image and drag Eustress Player to Applications."</li>
                                    <li>"The app is not signed or notarized yet, so macOS blocks the first open. In System Settings, open Privacy & Security and choose Open Anyway."</li>
                                </ul>
                            </div>

                            <div class="req-card">
                                <h3>"Linux"</h3>
                                <ul>
                                    <li>"Unpack the archive and run its installer, which also opens eustress-player:// links:"</li>
                                    <li>
                                        <code>
                                            {format!(
                                                "tar xzf eustress-player-v{}-linux-x64.tar.gz && ./eustress-player/install.sh",
                                                manifest.version
                                            )}
                                        </code>
                                    </li>
                                    <li>"To remove it: " <code>"./eustress-player/install.sh --uninstall"</code></li>
                                </ul>
                            </div>
                        </div>
                    </section>
                }),
                _ => None,
            }}

            // Player vs Engine comparison
            <section class="player-comparison">
                <div class="section-header-industrial">
                    <img src="/assets/icons/help.svg" alt="Info" class="section-icon" />
                    <h2>"Player or Engine: Which Do You Need?"</h2>
                </div>

                <div class="comparison-row">
                    <div class="comparison-card player-card">
                        <div class="card-icon">
                            <img src="/assets/icons/gamepad.svg" alt="Player" />
                        </div>
                        <h3>"Eustress Player"</h3>
                        <p class="card-tagline">"For playing experiences"</p>
                        <ul>
                            <li>"Browse and join community experiences"</li>
                            <li>"Opens simulations straight from the gallery"</li>
                            <li>"Auto-updates silently"</li>
                            <li>"Optimized for fast loading"</li>
                            <li>"Friends list and chat"</li>
                            <li>"Free forever"</li>
                        </ul>
                        <div class="card-audience">"Best for: Players, gamers, explorers"</div>
                    </div>

                    <div class="comparison-card engine-card">
                        <div class="card-icon">
                            <img src="/assets/icons/eustress-gear.svg" alt="Engine" />
                        </div>
                        <h3>"Eustress Engine"</h3>
                        <p class="card-tagline">"For building experiences"</p>
                        <ul>
                            <li>"Full 3D editor with tools"</li>
                            <li>"Soul and Rune scripting"</li>
                            <li>"Asset pipeline and import"</li>
                            <li>"Physics and terrain editing"</li>
                            <li>"Publish to all platforms"</li>
                            <li>"Free forever"</li>
                        </ul>
                        <div class="card-audience">"Best for: Developers, creators, studios"</div>
                        <a href="/download" class="btn-secondary-steel card-btn">"Download Engine Instead"</a>
                    </div>
                </div>
            </section>

            // System Requirements
            <section class="requirements-section">
                <div class="section-header-industrial">
                    <img src="/assets/icons/settings.svg" alt="Requirements" class="section-icon" />
                    <h2>"System Requirements"</h2>
                </div>

                <div class="requirements-row">
                    <div class="req-card">
                        <h3>"Minimum"</h3>
                        <ul>
                            <li><strong>"OS:"</strong>" Windows 10 / macOS 12 / Ubuntu 22.04"</li>
                            <li><strong>"CPU:"</strong>" Intel i3 / AMD Ryzen 3"</li>
                            <li><strong>"RAM:"</strong>" 4 GB"</li>
                            <li><strong>"GPU:"</strong>" GTX 750 Ti / RX 560"</li>
                            <li><strong>"Storage:"</strong>" 2 GB SSD"</li>
                            <li><strong>"Network:"</strong>" Broadband internet"</li>
                        </ul>
                    </div>

                    <div class="req-card recommended">
                        <h3>"Recommended"</h3>
                        <ul>
                            <li><strong>"OS:"</strong>" Windows 11 / macOS 14 / Ubuntu 24.04"</li>
                            <li><strong>"CPU:"</strong>" Intel i5 / AMD Ryzen 5"</li>
                            <li><strong>"RAM:"</strong>" 8 GB"</li>
                            <li><strong>"GPU:"</strong>" GTX 1060 / RX 580"</li>
                            <li><strong>"Storage:"</strong>" 5 GB SSD"</li>
                            <li><strong>"Network:"</strong>" 25+ Mbps"</li>
                        </ul>
                    </div>
                </div>
            </section>

            // Features
            <section class="player-features">
                <div class="section-header-industrial">
                    <img src="/assets/icons/sparkles.svg" alt="Features" class="section-icon" />
                    <h2>"What Can You Do?"</h2>
                </div>

                <div class="features-highlight">
                    <div class="feature-item">
                        <img src="/assets/icons/web.svg" alt="Browse" />
                        <div>
                            <h4>"Browse Experiences"</h4>
                            <p>"Discover thousands of games, simulations, and creative worlds built by the community"</p>
                        </div>
                    </div>

                    <div class="feature-item">
                        <img src="/assets/icons/users.svg" alt="Multiplayer" />
                        <div>
                            <h4>"Play With Friends"</h4>
                            <p>"Join friends in real-time multiplayer. Voice chat, friend lists, and party invites built in"</p>
                        </div>
                    </div>

                    <div class="feature-item">
                        <img src="/assets/icons/rocket.svg" alt="Fast" />
                        <div>
                            <h4>"Instant Loading"</h4>
                            <p>"Experiences stream in progressively. Start playing in seconds, not minutes"</p>
                        </div>
                    </div>

                    <div class="feature-item">
                        <img src="/assets/icons/cube.svg" alt="XR" />
                        <div>
                            <h4>"VR and XR Ready"</h4>
                            <p>"Play experiences in VR with OpenXR support for Meta Quest, PSVR2, and SteamVR headsets"</p>
                        </div>
                    </div>
                </div>
            </section>

            // Quick Links
            <section class="quick-links">
                <div class="links-grid">
                    <a href="/gallery" class="quick-link-card">
                        <img src="/assets/icons/gamepad.svg" alt="Gallery" />
                        <h3>"Browse Experiences"</h3>
                        <p>"Find games and worlds to play"</p>
                    </a>

                    <a href="/community" class="quick-link-card">
                        <img src="/assets/icons/users.svg" alt="Community" />
                        <h3>"Community"</h3>
                        <p>"Join players worldwide"</p>
                    </a>

                    <a href="/download" class="quick-link-card">
                        <img src="/assets/icons/eustress-gear.svg" alt="Engine" />
                        <h3>"Get the Engine"</h3>
                        <p>"Build your own experiences"</p>
                    </a>

                    <a href="https://discord.gg/DGP9my8DYN" class="quick-link-card">
                        <img src="/assets/icons/discord.svg" alt="Discord" />
                        <h3>"Discord"</h3>
                        <p>"Chat with the community"</p>
                    </a>
                </div>
            </section>

            <Footer />
        </div>
    }
}
