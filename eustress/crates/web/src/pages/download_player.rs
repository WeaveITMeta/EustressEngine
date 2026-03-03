// =============================================================================
// Eustress Web - Download Player Page (Industrial Design)
// =============================================================================
// Eustress Player is the lightweight client for playing experiences.
// Separate from the Engine (creation tool), the Player is optimized
// for fast loading, low resource usage, and seamless multiplayer.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Download Eustress Player page — play experiences without the full engine.
#[component]
pub fn DownloadPlayerPage() -> impl IntoView {
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
                <p class="download-tagline">"Play thousands of community-created experiences. Free, fast, and lightweight."</p>

                // Version Info
                <div class="version-info">
                    <span class="version-badge">"v0.16.1"</span>
                    <span class="version-label">"Public Beta"</span>
                </div>
            </section>

            // Primary Download Section
            <section class="primary-download">
                <div class="download-card-main">
                    <div class="download-icon-large">
                        <img src="/assets/icons/gamepad.svg" alt="Player" />
                    </div>

                    <h2>"Download for Your Platform"</h2>
                    <p class="download-desc">"Eustress Player is available for Windows, macOS, Linux, and mobile"</p>

                    // Platform Buttons
                    <div class="platform-buttons">
                        <a href="https://downloads.eustress.dev/player/windows/EustressPlayer-Setup.exe" class="platform-btn windows">
                            <img src="/assets/icons/windows.svg" alt="Windows" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">"Windows"</span>
                            </div>
                            <span class="btn-size">"~85 MB"</span>
                        </a>

                        <a href="https://downloads.eustress.dev/player/mac/EustressPlayer.dmg" class="platform-btn macos">
                            <img src="/assets/icons/macos.svg" alt="macOS" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">"macOS"</span>
                            </div>
                            <span class="btn-size">"~90 MB"</span>
                        </a>

                        <a href="https://downloads.eustress.dev/player/linux/EustressPlayer.AppImage" class="platform-btn linux">
                            <img src="/assets/icons/linux.svg" alt="Linux" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">"Linux"</span>
                            </div>
                            <span class="btn-size">"~80 MB"</span>
                        </a>
                    </div>

                    // Mobile links
                    <div class="mobile-links">
                        <span class="mobile-label">"Also available on"</span>
                        <div class="mobile-badges">
                            <a href="https://apps.apple.com/app/eustress-player" class="store-badge">
                                <img src="/assets/icons/ios.svg" alt="iOS" />
                                "App Store"
                            </a>
                            <a href="https://play.google.com/store/apps/details?id=dev.eustress.player" class="store-badge">
                                <img src="/assets/icons/android.svg" alt="Android" />
                                "Google Play"
                            </a>
                        </div>
                    </div>
                </div>
            </section>

            // Player vs Engine comparison
            <section class="player-comparison">
                <div class="section-header-industrial">
                    <img src="/assets/icons/info.svg" alt="Info" class="section-icon" />
                    <h2>"Player vs Engine — What is the Difference?"</h2>
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
                            <li>"Lightweight (~85 MB)"</li>
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
                        <img src="/assets/icons/globe.svg" alt="Browse" />
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
