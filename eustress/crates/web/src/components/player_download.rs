// =============================================================================
// Eustress Web - Player Download
// =============================================================================
// The Eustress Player's download buttons, read from its release manifest so
// they follow each release. The Player section of /download and the
// /downloads/player page render these buttons; the Play dialog on a
// simulation's page reads the same release through `use_player_release`.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::releases::{fetch_player_release, DesktopPlatform, ReleaseState};

/// The Player's latest release, looked up once in the browser.
///
/// An effect rather than a fetch at construction: the download pages are
/// prerendered, and the static copy keeps `Checking`, which reads correctly
/// without scripts.
pub fn use_player_release() -> RwSignal<ReleaseState> {
    let state = RwSignal::new(ReleaseState::Checking);
    Effect::new(move |_| {
        spawn_local(async move {
            state.set(fetch_player_release().await);
        });
    });
    state
}

/// One button per desktop platform: a download when the release has a build
/// for it, "Coming soon" when it does not. Before the first release there are
/// no buttons at all, only a line saying so.
#[component]
pub fn PlayerPlatformButtons(release: RwSignal<ReleaseState>) -> impl IntoView {
    move || match release.get() {
        ReleaseState::Checking => view! {
            <p class="download-desc">"Looking up the latest Player release\u{2026}"</p>
        }
        .into_any(),
        ReleaseState::Unreleased => view! {
            <p class="download-desc">
                "Eustress Player has not had its first release yet. Its downloads for "
                "Windows, macOS and Linux appear here as soon as it does."
            </p>
        }
        .into_any(),
        ReleaseState::Unreachable => view! {
            <p class="download-desc">
                "The Player's downloads could not be loaded. Refresh the page to try again."
            </p>
        }
        .into_any(),
        ReleaseState::Released(manifest) => {
            let released = if manifest.date.is_empty() {
                format!("Version {}", manifest.version)
            } else {
                format!("Version {}, released {}", manifest.version, manifest.date)
            };
            let buttons = DesktopPlatform::ALL
                .into_iter()
                .map(|platform| match manifest.player_download(platform) {
                    // The checksum rides on the tooltip for anyone verifying
                    // the file; latest.json lists it beside every download.
                    Some(download) => view! {
                        <a
                            href=download.url
                            class=format!("platform-btn {}", platform.css_class())
                            title=(!download.asset.sha256.is_empty())
                                .then(|| format!("SHA-256 {}", download.asset.sha256))
                        >
                            <img src=platform.icon() alt="" />
                            <div class="btn-text">
                                <span class="btn-label">"Download for"</span>
                                <span class="btn-platform">{platform.label()}</span>
                            </div>
                            <span class="btn-size">{download.asset.size_label()}</span>
                        </a>
                    }
                    .into_any(),
                    None => view! {
                        <div
                            class=format!("platform-btn {} unavailable", platform.css_class())
                            aria-disabled="true"
                        >
                            <img src=platform.icon() alt="" />
                            <div class="btn-text">
                                <span class="btn-label">"Coming soon"</span>
                                <span class="btn-platform">{platform.label()}</span>
                            </div>
                        </div>
                    }
                    .into_any(),
                })
                .collect_view();
            view! {
                <p class="download-desc">{released}</p>
                <div class="platform-buttons">{buttons}</div>
            }
            .into_any()
        }
    }
}
