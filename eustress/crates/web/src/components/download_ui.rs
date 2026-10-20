// =============================================================================
// Eustress Web - The download pages: Eustress Player (/downloads/player) and Eustress Studio (/download)
// =============================================================================
// What the program is, whether there is a build to download today, and which of the two programs a visitor needs. One page, drawn
// for either product. Every line is something that exists:
//
//   * The release card follows the product's release manifest (player/latest.json, latest.json). The staging website
//     reads staging/player/latest.json for the Player. With none, it says "Public
//     Alpha: first release coming soon" and offers the email sign-up (components/notify_me.rs); it never shows a download button
//     that goes nowhere.
//   * The visitor's own computer leads: its row in the table is marked, and with a release its button comes first.
//   * The two cards say what each program does today, in points that can be checked against the program. Nothing is promised
//     that is not built: no friends list, no chat, no silent updates, no headset support, no browser Player ("coming soon").
//   * The table says which platforms have a build and which are planned. Minimum memory and graphics are not measured yet, and
//     the page says so instead of listing numbers nobody measured.
//   * Studio's download needs a signed-in account (that rule is older than this page): signed out, the release card says so and
//     links to sign-in instead of showing buttons.
//
// Table of Contents:
// 1. What each platform's row says (pure, tested)
// 2. The page
// 3. The release card
// 4. The two programs
// 5. Requirements and installing
// =============================================================================

use leptos::prelude::*;

use crate::api::release_notify::Product;
use crate::api::releases::{DesktopPlatform, ReleaseManifest, ReleaseState};
use crate::components::notify_me::NotifyMe;
use crate::components::play_chooser::icon;
use crate::components::{use_player_release, use_studio_release, CentralNav, Footer};
use crate::state::AppState;

// -----------------------------------------------------------------------------
// 1. What each platform's row says
// -----------------------------------------------------------------------------

/// Where a platform stands, for the table and for the release card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformStatus {
    /// The current release has a build for it.
    Available,
    /// The first build is the one being prepared: Windows.
    ComingSoon,
    /// Written down as a goal; no release has a build for it yet.
    Planned,
}

impl PlatformStatus {
    pub fn words(self) -> &'static str {
        match self {
            PlatformStatus::Available => "Available",
            PlatformStatus::ComingSoon => "Coming soon",
            PlatformStatus::Planned => "Planned",
        }
    }
}

/// What a platform's row says, from a product's release state: a build in the release makes it available; without one, Windows is
/// the first build to come and the others are planned. While the release is being looked up, nothing is claimed beyond that.
pub fn platform_status(state: &ReleaseState, product: Product, platform: DesktopPlatform) -> PlatformStatus {
    if let ReleaseState::Released(manifest) = state {
        if manifest.download(product, platform).is_some() {
            return PlatformStatus::Available;
        }
    }
    if platform == DesktopPlatform::Windows {
        PlatformStatus::ComingSoon
    } else {
        PlatformStatus::Planned
    }
}

/// The platforms in the order the page lists them: the visitor's own computer first, then the others as they always run.
pub fn platforms_in_order(me: Option<DesktopPlatform>) -> Vec<DesktopPlatform> {
    let mut all = DesktopPlatform::ALL.to_vec();
    if let Some(me) = me {
        all.sort_by_key(|p| *p != me);
    }
    all
}

/// The line in the release card about the visitor's computer.
pub fn visitor_line(product: Product, me: Option<DesktopPlatform>, state: &ReleaseState) -> String {
    match me {
        None => format!("{} runs on Windows, macOS and Linux computers. Open this page on a computer to download it.", product.name()),
        Some(p) => match platform_status(state, product, p) {
            PlatformStatus::Available => format!("You are on {}: this build is for you.", p.short_label()),
            PlatformStatus::ComingSoon => format!("You are on {}: the first build is coming soon.", p.short_label()),
            PlatformStatus::Planned => format!("You are on {}: there is no build for it yet; it is planned.", p.short_label()),
        },
    }
}

/// What a computer needs, as the table's last column says it: only what is true. A platform with no build says so. The two
/// Windows installers differ: the Player installs for the account, Studio's asks for administrator permission.
pub fn needs_words(product: Product, platform: DesktopPlatform, status: PlatformStatus) -> &'static str {
    match (platform, status, product) {
        (DesktopPlatform::Windows, _, Product::Player) => "Windows 10 or later, 64-bit. Installs for your account; no administrator prompt.",
        (DesktopPlatform::Windows, _, Product::Studio) => "Windows 10 or later, 64-bit. The installer asks for administrator permission.",
        (DesktopPlatform::MacOS, PlatformStatus::Available, _) => "A Mac with Apple Silicon.",
        (DesktopPlatform::MacOS, _, _) => "A Mac with Apple Silicon. No release has a Mac build yet.",
        (DesktopPlatform::Linux, PlatformStatus::Available, _) => "A 64-bit Linux computer.",
        (DesktopPlatform::Linux, _, _) => "A 64-bit Linux computer. No release has a Linux build yet.",
    }
}

/// What the Player does today. Each point can be checked against the program.
pub const PLAYER_POINTS: [&str; 5] = [
    "Opens a Space when you press Play in the Gallery",
    "Downloads the Space and runs it on your computer",
    "Joins live sessions hosted from Studio (in testing)",
    "Play on your own without an account",
    "Free; installs for your account, no administrator prompt (Windows)",
];

/// What Studio does today.
pub const STUDIO_POINTS: [&str; 5] = [
    "A 3D editor for building Spaces",
    "Script in Luau or Rune",
    "Imports 3D models and Roblox places",
    "Physics, terrain and real-world units built in",
    "Publish to the Gallery; new Spaces are reviewed automatically before they are listed",
];

fn points_of(product: Product) -> &'static [&'static str] {
    match product {
        Product::Player => &PLAYER_POINTS,
        Product::Studio => &STUDIO_POINTS,
    }
}

/// Where each product's page is.
pub fn page_of(product: Product) -> &'static str {
    match product {
        Product::Player => "/downloads/player",
        Product::Studio => "/download",
    }
}

fn lead_of(product: Product) -> &'static str {
    match product {
        Product::Player => "Play Spaces from the Gallery on your computer. Free.",
        Product::Studio => "Build Spaces, script them and publish them to the Gallery. Free to download.",
    }
}

// -----------------------------------------------------------------------------
// 2. The page
// -----------------------------------------------------------------------------

/// The download page of one product.
#[component]
pub fn ProductDownload(product: Product) -> impl IntoView {
    let player = use_player_release();
    let studio = use_studio_release();
    let release = match product {
        Product::Player => player,
        Product::Studio => studio,
    };
    let me = DesktopPlatform::detect();

    view! {
        <div class="page g-page dl-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main dl-main">
                <div class="g-wrap">
                    <header class="dl-head">
                        <div class="g-kicker"><span class="g-kick-rule"></span>"Download"</div>
                        <h1 class="dl-title">{product.name()}</h1>
                        <p class="dl-lead">{lead_of(product)}</p>
                    </header>

                    <ReleaseCard product=product release=release me=me />

                    <section class="g-section" aria-labelledby="dl-which">
                        <div class="g-head"><h2 id="dl-which">"Player or Studio: which do you need?"</h2><span class="g-rule"></span></div>
                        <div class="dl-compare">
                            <ProgramCard card=Product::Player page=product release=player me=me />
                            <ProgramCard card=Product::Studio page=product release=studio me=me />
                        </div>
                    </section>

                    <section class="g-section" aria-labelledby="dl-req">
                        <div class="g-head"><h2 id="dl-req">"System requirements"</h2><span class="g-rule"></span></div>
                        <Requirements product=product release=release me=me />
                    </section>

                    {move || match release.get() {
                        ReleaseState::Released(manifest) => view! { <Installing product=product manifest=manifest /> }.into_any(),
                        _ => ().into_any(),
                    }}

                    <nav class="dl-links" aria-label="More">
                        <a class="g-btn" href="/gallery">"Browse the Gallery"</a>
                        <a class="g-btn" href="/learn">"Learn to build"</a>
                        <a class="g-btn" href="https://discord.gg/DGP9my8DYN">"Discord"</a>
                    </nav>
                </div>
            </main>
            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// 3. The release card
// -----------------------------------------------------------------------------

#[component]
fn ReleaseCard(product: Product, release: RwSignal<ReleaseState>, me: Option<DesktopPlatform>) -> impl IntoView {
    view! {
        <section id="dl-release" class="g-glass dl-release" aria-labelledby="dl-release-h">
            {move || match release.get() {
                ReleaseState::Checking => view! {
                    <div role="status" aria-busy="true" aria-label="Looking up the latest release">
                        <span class="g-skel w30"></span><span class="g-skel w60 tall"></span><span class="g-skel w40 btn"></span>
                    </div>
                }.into_any(),
                ReleaseState::Unreleased => view! {
                    <div class="dl-release-body">
                        <span class="g-chip warn">"Public Alpha"</span>
                        <h2 id="dl-release-h" class="dl-release-title">"First release coming soon"</h2>
                        <p class="dl-release-text">"There is no download yet, so there is no button to press. When the first Windows build is ready, it appears here."</p>
                        <p class="g-hint">{visitor_line(product, me, &ReleaseState::Unreleased)}</p>
                        <NotifyMe product=product />
                    </div>
                    <dl class="g-spec dl-spec">
                        <div><dt>"Price"</dt><dd>{match product { Product::Player => "Free", Product::Studio => "Free to download" }}</dd></div>
                        <div><dt>"Status"</dt><dd>"Public Alpha"</dd></div>
                        <div><dt>"First build"</dt><dd>"Windows"</dd></div>
                    </dl>
                    {(product == Product::Player).then(|| view! {
                        <div class="dl-release-side">
                            <span class="g-chip"><i class="g-dot off"></i>"Play in browser: coming soon"</span>
                            <p class="g-hint">"Until then, Spaces open in Eustress Player once it is released."</p>
                        </div>
                    })}
                }.into_any(),
                ReleaseState::Unreachable => view! {
                    <div class="dl-release-body" role="alert">
                        <h2 id="dl-release-h" class="dl-release-title">"We could not check for the latest release"</h2>
                        <p class="dl-release-text">"Refresh the page to try again. Nothing is wrong with your computer."</p>
                        <div class="g-actions"><a class="g-btn g-btn-primary" href=page_of(product)>"Refresh"</a></div>
                    </div>
                }.into_any(),
                ReleaseState::Released(manifest) => view! { <Released product=product manifest=manifest me=me /> }.into_any(),
            }}
        </section>
    }
}

/// A release exists: the visitor's own build first, the others below it. Studio's downloads need a signed-in account.
#[component]
fn Released(product: Product, manifest: ReleaseManifest, me: Option<DesktopPlatform>) -> impl IntoView {
    let auth = expect_context::<AppState>().auth;
    let gated = product == Product::Studio;
    let state = ReleaseState::Released(manifest.clone());
    let when = if manifest.date.is_empty() { format!("Version {}", manifest.version) } else { format!("Version {}, released {}", manifest.version, manifest.date) };
    let rows = platforms_in_order(me)
        .into_iter()
        .map(|platform| {
            let is_me = Some(platform) == me;
            let status = platform_status(&state, product, platform);
            let download = manifest.download(product, platform);
            view! {
                <li class="dl-plat" class:me=is_me>
                    <img src=platform.icon() alt="" />
                    <span class="dl-plat-name">{platform.label()}{is_me.then(|| view! { <span class="g-chip on">"Your computer"</span> })}</span>
                    {match download {
                        Some(d) => {
                            let label = format!("Download {}", d.asset.size_label()).trim().to_string();
                            let class = if is_me { "g-btn g-btn-primary" } else { "g-btn" };
                            let title = (!d.asset.sha256.is_empty()).then(|| format!("SHA-256 {}", d.asset.sha256));
                            let href = d.url;
                            view! {
                                {move || if !gated || auth.get().is_authenticated() {
                                    view! { <a class=class href=href.clone() title=title.clone()>{label.clone()}</a> }.into_any()
                                } else {
                                    view! { <a class=class href="/login">"Sign in to download"</a> }.into_any()
                                }}
                            }.into_any()
                        }
                        None => view! { <span class="dl-plat-state">{status.words()}</span> }.into_any(),
                    }}
                </li>
            }
        })
        .collect_view();
    view! {
        <div class="dl-release-body">
            <span class="g-chip warn">"Public Alpha"</span>
            <h2 id="dl-release-h" class="dl-release-title">{format!("{} {}", product.name(), manifest.version)}</h2>
            <p class="dl-release-text">{when}</p>
            <p class="g-hint">{visitor_line(product, me, &state)}</p>
            {gated.then(|| view! { <p class="g-hint">"Studio's downloads need a free account: your identity is verified for platform safety."</p> })}
        </div>
        <ul class="dl-plats">{rows}</ul>
        {(product == Product::Player).then(|| view! { <div class="dl-release-side"><span class="g-chip"><i class="g-dot off"></i>"Play in browser: coming soon"</span></div> })}
    }
}

// -----------------------------------------------------------------------------
// 4. The two programs
// -----------------------------------------------------------------------------

/// One program's card. On its own page the card ends in the way to get it; on the other program's page it ends in the way to that
/// page, where its release card says what there is to download.
#[component]
fn ProgramCard(card: Product, page: Product, release: RwSignal<ReleaseState>, me: Option<DesktopPlatform>) -> impl IntoView {
    let (purpose, best, glyph) = match card {
        Product::Player => ("Play Spaces made by other people.", "Playing Spaces and joining live sessions.", "monitor"),
        Product::Studio => ("Build Spaces and publish them.", "Building and publishing Spaces.", "cube"),
    };
    let own = card == page;
    view! {
        <article class="g-glass dl-card">
            <div class="dl-card-head">
                <span class="dl-ico" aria-hidden="true">{icon(glyph)}</span>
                <div>
                    <h3>{card.name()}</h3>
                    <p class="dl-purpose">{purpose}</p>
                </div>
            </div>
            <ul class="dl-points">{points_of(card).iter().map(|p| view! { <li>{*p}</li> }).collect_view()}</ul>
            <p class="dl-best"><b>"Best for"</b>{best}</p>
            <div class="dl-card-action">
                {move || {
                    let state = release.get();
                    let chip = matches!(state, ReleaseState::Unreleased).then(|| view! { <span class="g-chip warn">"Public Alpha: first release coming soon"</span> });
                    let button = if !own {
                        view! { <a class="g-btn g-btn-primary g-btn-lg" href=page_of(card)>{format!("About {}", card.name())}</a> }.into_any()
                    } else {
                        match &state {
                            ReleaseState::Released(manifest) => {
                                // The visitor's own build, or the first one there is.
                                match platforms_in_order(me).into_iter().find_map(|p| manifest.download(card, p).map(|d| (p, d))) {
                                    Some((platform, _)) => view! {
                                        <a class="g-btn g-btn-primary g-btn-lg" href="#dl-release">{format!("Download for {}", platform.short_label())}</a>
                                    }.into_any(),
                                    None => ().into_any(),
                                }
                            }
                            ReleaseState::Unreleased => view! { <a class="g-btn g-btn-primary g-btn-lg" href="#dl-release">"Get notified"</a> }.into_any(),
                            _ => ().into_any(),
                        }
                    };
                    view! { {chip}{button} }
                }}
            </div>
        </article>
    }
}

// -----------------------------------------------------------------------------
// 5. Requirements and installing
// -----------------------------------------------------------------------------

#[component]
fn Requirements(product: Product, release: RwSignal<ReleaseState>, me: Option<DesktopPlatform>) -> impl IntoView {
    view! {
        <div class="g-glass dl-table-wrap">
            <table class="dl-table">
                <caption class="g-sr">{format!("Which computers {} is built for", product.name())}</caption>
                <thead><tr><th scope="col">"Computer"</th><th scope="col">"Status"</th><th scope="col">"What it needs"</th></tr></thead>
                <tbody>
                    {platforms_in_order(me).into_iter().map(|platform| {
                        let is_me = Some(platform) == me;
                        view! {
                            <tr class:me=is_me>
                                <th scope="row"><span class="dl-comp"><img src=platform.icon() alt="" />{platform.short_label()}{is_me.then(|| view! { <span class="g-chip on">"Your computer"</span> })}</span></th>
                                <td>{move || {
                                    let status = platform_status(&release.get(), product, platform);
                                    let class = match status { PlatformStatus::Available => "g-chip live", PlatformStatus::ComingSoon => "g-chip warn", PlatformStatus::Planned => "g-chip" };
                                    view! { <span class=class>{status.words()}</span> }
                                }}</td>
                                <td>{move || needs_words(product, platform, platform_status(&release.get(), product, platform))}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
        <p class="g-hint dl-note">{format!("{} draws 3D with your graphics card. Minimum memory and graphics requirements have not been measured yet; they are listed here after alpha testing.", product.name())}</p>
    }
}

/// How to install each build the release actually has. The builds are not signed yet, so Windows and macOS each ask once.
#[component]
fn Installing(product: Product, manifest: ReleaseManifest) -> impl IntoView {
    let has = |p: DesktopPlatform| manifest.download(product, p).is_some();
    let version = manifest.version.clone();
    let player = product == Product::Player;
    view! {
        <section class="g-section" aria-labelledby="dl-install">
            <div class="g-head"><h2 id="dl-install">"Installing"</h2><span class="g-rule"></span></div>
            <div class="dl-install-grid">
                {has(DesktopPlatform::Windows).then(|| view! {
                    <article class="g-glass dl-install">
                        <h3>"Windows"</h3>
                        {if player {
                            view! { <p>"Installs for your account with no administrator prompt, and opens eustress-player:// links from the Gallery."</p> }.into_any()
                        } else {
                            view! { <p>"The installer asks for administrator permission."</p> }.into_any()
                        }}
                        <p>"The installer is not signed yet, so SmartScreen may stop it. Choose More info, then Run anyway."</p>
                    </article>
                })}
                {has(DesktopPlatform::MacOS).then(|| view! {
                    <article class="g-glass dl-install">
                        <h3>"macOS (Apple Silicon)"</h3>
                        <p>{format!("Open the disk image and drag {} to Applications.", product.name())}</p>
                        <p>"The app is not signed or notarized yet, so macOS blocks the first open. In System Settings, open Privacy & Security and choose Open Anyway."</p>
                    </article>
                })}
                {has(DesktopPlatform::Linux).then(|| view! {
                    <article class="g-glass dl-install">
                        <h3>"Linux"</h3>
                        {if player {
                            view! {
                                <p>"Unpack the archive and run its installer, which also opens eustress-player:// links:"</p>
                                <p><code>{format!("tar xzf eustress-player-v{version}-linux-x64.tar.gz && ./eustress-player/install.sh")}</code></p>
                                <p>"To remove it: "<code>"./eustress-player/install.sh --uninstall"</code></p>
                            }.into_any()
                        } else {
                            view! { <p>"Unpack the archive, then follow the notes that come with the release."</p> }.into_any()
                        }}
                    </article>
                })}
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::releases::ReleaseAsset;
    use std::collections::BTreeMap;

    fn released(keys: &[&str]) -> ReleaseState {
        let mut platforms = BTreeMap::new();
        for k in keys {
            platforms.insert(k.to_string(), ReleaseAsset { url: format!("https://downloads.example.dev/{k}"), sha256: String::new(), size_bytes: 85_000_000 });
        }
        ReleaseState::Released(ReleaseManifest { version: "0.1.0".to_string(), date: String::new(), changelog_url: String::new(), platforms })
    }

    const P: Product = Product::Player;

    #[test]
    fn with_no_release_windows_is_coming_soon_and_the_others_are_planned() {
        for state in [ReleaseState::Unreleased, ReleaseState::Checking, ReleaseState::Unreachable] {
            for product in [Product::Player, Product::Studio] {
                assert_eq!(platform_status(&state, product, DesktopPlatform::Windows), PlatformStatus::ComingSoon);
                assert_eq!(platform_status(&state, product, DesktopPlatform::MacOS), PlatformStatus::Planned);
                assert_eq!(platform_status(&state, product, DesktopPlatform::Linux), PlatformStatus::Planned);
            }
        }
    }

    #[test]
    fn a_platform_is_available_only_when_the_release_has_a_build_for_it() {
        let state = released(&["windows-x64-installer"]);
        assert_eq!(platform_status(&state, P, DesktopPlatform::Windows), PlatformStatus::Available);
        assert_eq!(platform_status(&state, P, DesktopPlatform::MacOS), PlatformStatus::Planned);
        let both = released(&["windows-x64-installer", "macos-arm64"]);
        assert_eq!(platform_status(&both, P, DesktopPlatform::MacOS), PlatformStatus::Available);
        assert_eq!(platform_status(&both, Product::Studio, DesktopPlatform::MacOS), PlatformStatus::Available);
        // A manifest that lists a platform without an https address is not a build.
        let mut broken = released(&["linux-x64"]);
        if let ReleaseState::Released(m) = &mut broken {
            m.platforms.get_mut("linux-x64").unwrap().url = "http://insecure.example/x".to_string();
        }
        assert_eq!(platform_status(&broken, P, DesktopPlatform::Linux), PlatformStatus::Planned);
    }

    #[test]
    fn the_visitors_own_computer_is_listed_first_and_the_rest_keep_their_order() {
        use DesktopPlatform::*;
        assert_eq!(platforms_in_order(None), [Windows, MacOS, Linux]);
        assert_eq!(platforms_in_order(Some(Windows)), [Windows, MacOS, Linux]);
        assert_eq!(platforms_in_order(Some(MacOS)), [MacOS, Windows, Linux]);
        assert_eq!(platforms_in_order(Some(Linux)), [Linux, Windows, MacOS]);
    }

    #[test]
    fn the_line_about_the_visitors_computer_says_only_what_is_true_of_it() {
        let none = ReleaseState::Unreleased;
        assert!(visitor_line(P, None, &none).contains("Open this page on a computer"));
        assert!(visitor_line(Product::Studio, None, &none).starts_with("Eustress Studio runs"));
        assert!(visitor_line(P, Some(DesktopPlatform::Windows), &none).contains("first build is coming soon"));
        assert!(visitor_line(P, Some(DesktopPlatform::MacOS), &none).contains("planned"));
        assert!(visitor_line(P, Some(DesktopPlatform::Windows), &released(&["windows-x64-installer"])).contains("this build is for you"));
    }

    #[test]
    fn a_row_never_says_there_is_no_build_for_a_platform_that_has_one() {
        use DesktopPlatform::*;
        assert!(needs_words(P, MacOS, PlatformStatus::Planned).contains("No release has a Mac build yet"));
        assert!(!needs_words(P, MacOS, PlatformStatus::Available).contains("No release"));
        assert!(needs_words(P, Linux, PlatformStatus::Planned).contains("No release has a Linux build yet"));
        assert!(!needs_words(P, Linux, PlatformStatus::Available).contains("No release"));
        assert_eq!(needs_words(P, Windows, PlatformStatus::ComingSoon), needs_words(P, Windows, PlatformStatus::Available));
    }

    #[test]
    fn the_two_windows_installers_are_described_as_they_are() {
        let player = needs_words(Product::Player, DesktopPlatform::Windows, PlatformStatus::Available);
        let studio = needs_words(Product::Studio, DesktopPlatform::Windows, PlatformStatus::Available);
        assert!(player.contains("no administrator prompt"));
        assert!(studio.contains("asks for administrator permission"));
        assert!(!studio.contains("no administrator prompt"));
    }

    #[test]
    fn each_product_has_its_own_page_and_its_own_words() {
        assert_eq!(page_of(Product::Player), "/downloads/player");
        assert_eq!(page_of(Product::Studio), "/download");
        assert_ne!(lead_of(Product::Player), lead_of(Product::Studio));
        assert!(lead_of(Product::Player).ends_with("Free."));
    }

    #[test]
    fn nothing_unbuilt_is_promised_in_the_points() {
        let all = PLAYER_POINTS.iter().chain(STUDIO_POINTS.iter()).map(|p| p.to_lowercase()).collect::<Vec<_>>().join(" | ");
        for banned in ["friends", "chat", "auto-update", "automatically update", "silently", "vr", "xr", "voice", "all platforms", "optimized", "forever", "instant", "10x", "10m", "point cloud", "cad"] {
            assert!(!all.contains(banned), "the points must not mention {banned:?}");
        }
        assert_eq!(PLAYER_POINTS.len(), 5);
        assert_eq!(STUDIO_POINTS.len(), 5);
    }
}
