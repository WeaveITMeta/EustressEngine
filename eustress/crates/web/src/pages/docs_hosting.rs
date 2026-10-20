// =============================================================================
// Eustress Web - Hosting Guide
// =============================================================================
// Host a Space from your own computer, the way people run a game server at
// home: Studio's Settings > Hosting, the Windows firewall, the router's port
// forward, and joining by link. Studio's "Port forwarding guide" button opens
// this page (engine ui/slint/settings.slint).
//
// The page also has the direct join box: paste the link a host shared and the
// browser opens it in the Player. `direct_join_link` rebuilds that link from
// its parts and keeps only the address, the key and the pin.
// =============================================================================

use leptos::prelude::*;

use crate::api::live::is_join_link;
use crate::components::play_chooser::open_player;
use crate::components::{CentralNav, Footer};

struct TocEntry {
    id: &'static str,
    title: &'static str,
}

const TOC: &[TocEntry] = &[
    TocEntry { id: "overview", title: "How hosting works" },
    TocEntry { id: "studio", title: "1. Studio" },
    TocEntry { id: "firewall", title: "2. Windows firewall" },
    TocEntry { id: "router", title: "3. Your router" },
    TocEntry { id: "check", title: "4. Check it works" },
    TocEntry { id: "join", title: "Joining a host" },
    TocEntry { id: "trouble", title: "When it does not work" },
    TocEntry { id: "safety", title: "Safety" },
];

const NOT_A_LINK: &str = "That is not a join link. Copy the whole link the host shared; it starts with eustress-player://join/.";
const ADDRESS_ALONE: &str = "An address alone is not enough. Ask the host for the whole join link: its key lets you in, and its pin proves you reached the right computer.";
const NO_PORT: &str = "The link names no port. Copy the whole link again.";
const BAD_HOST: &str = "The link's address is not one the Player can reach. Copy the whole link again.";
const IPV6: &str = "The link names an IPv6 address, and a host listens on IPv4 only. Ask the host for a link with their IPv4 address.";
const NO_KEY: &str = "The link has no join key. Copy the whole link again, from the server that is running now.";
const NO_PIN: &str = "The link has no certificate pin. Copy the whole link again, from the server that is running now.";
const TWICE: &str = "The link names its key or pin twice. Copy the whole link again.";

/// The link to open in the Player for what someone pasted: a join link
/// (`eustress-player://join/` or `eustress://join/`), rebuilt from its
/// address, port, key and pin. Anything else in it (a gallery's ticket, an
/// API name) stays behind: a ticket is single use and belongs to the Play
/// button that minted it, and a link never chooses the Player's API. A
/// signed-in Player mints its own ticket for the host, as it does for any
/// join link. The reasons are fixed text and never quote the paste, which
/// holds the key.
pub fn direct_join_link(pasted: &str) -> Result<String, &'static str> {
    let pasted = pasted.trim();
    let rest = ["eustress-player://join/", "eustress://join/"]
        .iter()
        .find(|scheme| pasted.get(..scheme.len()).is_some_and(|head| head.eq_ignore_ascii_case(scheme)))
        .map(|scheme| &pasted[scheme.len()..]);
    let Some(rest) = rest else {
        // `host:port` and nothing else: someone typed what Minecraft asks for.
        let address_alone = !pasted.contains("://")
            && pasted.rsplit_once(':').is_some_and(|(host, port)| {
                !host.is_empty() && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
            });
        return Err(if address_alone { ADDRESS_ALONE } else { NOT_A_LINK });
    };
    let (authority, query) = rest.split_once('?').ok_or(NO_KEY)?;
    let authority = authority.trim_end_matches('/');
    if authority.starts_with('[') {
        return Err(IPV6);
    }
    let (host, port) = authority.rsplit_once(':').ok_or(NO_PORT)?;
    if host.contains(':') {
        return Err(IPV6);
    }
    if host.is_empty() || host.len() > 253 || !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return Err(BAD_HOST);
    }
    let port = port.parse::<u16>().ok().filter(|p| *p != 0).ok_or(NO_PORT)?;
    let (mut key, mut pin) = (None, None);
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let slot = match name {
            "key" => &mut key,
            "pin" => &mut pin,
            _ => continue,
        };
        if slot.replace(value).is_some() {
            return Err(TWICE);
        }
    }
    let key = key
        .filter(|k: &&str| (16..=64).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric()))
        .ok_or(NO_KEY)?;
    let pin = pin
        .filter(|p: &&str| p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or(NO_PIN)?
        .to_ascii_lowercase();
    let link = format!("eustress-player://join/{host}:{port}?key={key}&pin={pin}");
    if !is_join_link(&link) {
        return Err(NOT_A_LINK);
    }
    Ok(link)
}

/// Paste a host's link, open it in the Player.
#[component]
fn DirectJoin() -> impl IntoView {
    let pasted = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<&'static str>);
    view! {
        <div class="direct-join">
            <input
                type="text"
                class="form-input"
                placeholder="eustress-player://join/203.0.113.7:7777?key=...&pin=..."
                autocomplete="off"
                spellcheck="false"
                prop:value=move || pasted.get()
                on:input=move |ev| pasted.set(event_target_value(&ev))
            />
            <button
                class="btn-primary-glow"
                on:click=move |_| {
                    match direct_join_link(&pasted.get_untracked()) {
                        Ok(link) => {
                            problem.set(None);
                            open_player(&link);
                        }
                        Err(why) => problem.set(Some(why)),
                    }
                }
            >
                "Open in the Player"
            </button>
            {move || problem.get().map(|why| view! { <p class="direct-join-problem">{why}</p> })}
        </div>
    }
}

#[component]
pub fn DocsHostingPage() -> impl IntoView {
    view! {
        <div class="page page-docs">
            <CentralNav active="learn".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-networking"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/network.svg" alt="Hosting" class="toc-icon" />
                        <h2>"Hosting"</h2>
                    </div>
                    <nav class="toc-nav">
                        {TOC.iter().map(|entry| view! {
                            <div class="toc-section">
                                <a href=format!("#{}", entry.id) class="toc-section-title">{entry.title}</a>
                            </div>
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
                            <span class="current">"Hosting"</span>
                        </div>
                        <h1 class="docs-title">"Host from your computer"</h1>
                        <p class="docs-subtitle">
                            "Studio can host the Space you are playing for other people, the way a game server
                            runs at home. Players on your own network join right away. Players on the internet
                            reach you through a port forward on your router. This page walks through both, in
                            four steps."
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "8 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/check.svg" alt="Updated" />
                                "Updated Oct 2026"
                            </span>
                        </div>
                    </header>

                    <section id="overview" class="docs-section">
                        <h2 class="section-title">"How hosting works"</h2>
                        <p>
                            "Your computer is the server. It runs the Space's scripts and physics, and every
                            player downloads the world from it once, then sees every avatar move live. The
                            connection is UDP on one port, 7777 unless you change it."
                        </p>
                        <table class="docs-table">
                            <thead>
                                <tr><th>"Who joins"</th><th>"What you set up"</th></tr>
                            </thead>
                            <tbody>
                                <tr><td>"Someone on this computer"</td><td>"Nothing. Start Server and join."</td></tr>
                                <tr><td>"Someone on your home or office network"</td><td>"Turn on Players on other computers can join, and allow the port in the Windows firewall."</td></tr>
                                <tr><td>"Someone on the internet"</td><td>"All of the above, plus a port forward on your router."</td></tr>
                            </tbody>
                        </table>
                        <p>
                            "Cloud hosting on dedicated servers, where nothing has to run on your computer, comes
                            later."
                        </p>
                    </section>

                    <section id="studio" class="docs-section">
                        <h2 class="section-title">"1. Studio"</h2>
                        <ol>
                            <li>"Open Settings, then Hosting."</li>
                            <li>"Turn on Players on other computers can join."</li>
                            <li>"Leave the port at 7777 unless another program uses it."</li>
                            <li>
                                "Leave Public address empty. When the server starts, Studio asks Cloudflare's
                                public echo which address your connection comes from. Type an address or a name
                                only when you know better, for example a name you own that points at your home."
                            </li>
                            <li>"Open your Space and press Start Server (F9)."</li>
                        </ol>
                        <p>
                            "The Hosting page then says where players reach you, for example Hosting on
                            81.2.69.160:7777, and what your router has to forward. The Output shows the join
                            link to share."
                        </p>
                    </section>

                    <section id="firewall" class="docs-section">
                        <h2 class="section-title">"2. Windows firewall"</h2>
                        <p>
                            "The first time the server listens beyond this computer, Windows asks whether
                            Eustress Studio may use the network. Tick Private networks and choose Allow. If you
                            missed that window, add the rule yourself:"
                        </p>
                        <ol>
                            <li>"Open Start, type Windows Defender Firewall with Advanced Security, and open it."</li>
                            <li>"Choose Inbound Rules, then New Rule."</li>
                            <li>"Choose Port, then UDP, and type 7777 (or the port you chose)."</li>
                            <li>"Choose Allow the connection."</li>
                            <li>"Tick Private. Tick Public only if this computer is on a network marked public."</li>
                            <li>"Name it Eustress hosting and finish."</li>
                        </ol>
                        <p>"Or run this once in PowerShell as an administrator:"</p>
                        <pre class="code-block"><code>"New-NetFirewallRule -DisplayName \"Eustress hosting\" -Direction Inbound -Protocol UDP -LocalPort 7777 -Action Allow -Profile Private"</code></pre>
                        <p>"The rule is UDP. A TCP rule for the same port does not help: the connection is QUIC, which runs over UDP."</p>
                    </section>

                    <section id="router" class="docs-section">
                        <h2 class="section-title">"3. Your router"</h2>
                        <p>
                            "A router hands internet traffic to the right computer at home only when a rule
                            tells it to. That rule is the port forward. Every router names it differently, so
                            these steps are in general terms."
                        </p>
                        <ol>
                            <li>
                                "Find this computer's local address. Studio's Hosting page shows it in
                                brackets, for example 192.168.1.20. You can also run ipconfig and read the IPv4
                                Address line."
                            </li>
                            <li>
                                "Open the router's page in a browser. Its address is the Default Gateway in
                                ipconfig, often 192.168.1.1 or 192.168.0.1. The login is usually on a sticker on
                                the router."
                            </li>
                            <li>
                                "Find the forwarding section. It is called Port Forwarding, Virtual Server, NAT,
                                Applications, or Gaming, often under Advanced."
                            </li>
                            <li>
                                "Add a rule: protocol UDP, outside port 7777, inside port 7777, and this
                                computer's local address. Save it."
                            </li>
                            <li>
                                "Reserve the address so it does not change: look for DHCP Reservation or Static
                                Lease and pin this computer to the address you used. Otherwise the rule can point
                                at another device after a restart."
                            </li>
                        </ol>
                        <div class="callout callout-info">
                            <img src="/assets/icons/help.svg" alt="Info" />
                            <div>
                                <strong>"A different outside port"</strong>
                                <p>
                                    "If 7777 is taken on the router, forward another outside port, for example
                                    30000, to 7777 on this computer. Then type your address with that port in
                                    Public address, as 81.2.69.160:30000, so the link players get names it."
                                </p>
                            </div>
                        </div>
                    </section>

                    <section id="check" class="docs-section">
                        <h2 class="section-title">"4. Check it works"</h2>
                        <p>
                            "Studio cannot test the port forward from inside your own network, and many routers
                            cannot loop a connection back in. Ask a friend to join, or join from a phone hotspot
                            on a laptop. On your own network, use the local link instead: the Output shows it."
                        </p>
                    </section>

                    <section id="join" class="docs-section">
                        <h2 class="section-title">"Joining a host"</h2>
                        <p>
                            "A host shares a join link. It holds the address, a key that lets players in, and a
                            pin that proves the Player reached the right computer. The key and the pin change
                            every time the server starts, so share the link from the server that is running now.
                            An address alone is not enough."
                        </p>
                        <p>"Paste the link here to open it in the Eustress Player:"</p>
                        <DirectJoin />
                        <p>"Or from a terminal:"</p>
                        <pre class="code-block"><code>"eustress-client \"eustress-player://join/81.2.69.160:7777?key=...&pin=...\""</code></pre>
                        <p>
                            "A session listed in the Gallery checks every join with the Gallery first, so a
                            player signs in to the Player before joining it."
                        </p>
                    </section>

                    <section id="trouble" class="docs-section">
                        <h2 class="section-title">"When it does not work"</h2>
                        <table class="docs-table">
                            <thead>
                                <tr><th>"What you see"</th><th>"What it usually means"</th></tr>
                            </thead>
                            <tbody>
                                <tr><td>"Players on your network can join, players outside cannot"</td><td>"The port forward is missing, points at an old local address, or is TCP instead of UDP."</td></tr>
                                <tr><td>"Nobody else can join at all"</td><td>"The firewall blocks UDP 7777, or Players on other computers can join is off. Settings apply the next time a server starts."</td></tr>
                                <tr><td>"The router's internet address is not the one Studio shows"</td><td>"Your provider shares one address among many homes (carrier-grade NAT, often 100.64 to 100.127). A forward cannot work there; ask the provider for a public address, or wait for cloud hosting."</td></tr>
                                <tr><td>"Two routers, one behind the other"</td><td>"Forward the port on both, or put the first one in bridge mode."</td></tr>
                                <tr><td>"Studio could not find your public address"</td><td>"Type it in Public address. Searching what is my IP in a browser on this computer shows it."</td></tr>
                                <tr><td>"The join is refused at once"</td><td>"The link is from an earlier server. Copy it again."</td></tr>
                            </tbody>
                        </table>
                        <p>
                            "IPv6: a host listens on IPv4 only for now, so a connection with an IPv6 address alone
                            cannot host for the internet yet."
                        </p>
                    </section>

                    <section id="safety" class="docs-section">
                        <h2 class="section-title">"Safety"</h2>
                        <ul>
                            <li>"Anyone with the link sees your public address. Share it with people you mean to play with."</li>
                            <li>"List in Gallery shows the session, and your public address, to signed-in visitors of its Gallery page. It is off until you turn it on."</li>
                            <li>"The key keeps out anyone without the link. A new key comes with every server start, so an old link stops working."</li>
                            <li>"Players never receive server scripts or ServerStorage; they run on your computer only."</li>
                            <li>"Stop Server closes the port to players at once. Remove the router rule when you stop hosting for good."</li>
                        </ul>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/docs/networking" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Previous" />
                            <div>
                                <span class="nav-label">"Previous"</span>
                                <span class="nav-title">"Networking"</span>
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pin() -> String {
        "AB".repeat(32)
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// A host's link opens as itself, rebuilt from its parts; a gallery's
    /// ticket and API name stay behind.
    #[test]
    fn hosting_direct_join_keeps_only_the_address_key_and_pin() {
        let pin = pin();
        let want = format!("eustress-player://join/81.2.69.160:7777?key={KEY}&pin={}", pin.to_ascii_lowercase());
        assert_eq!(direct_join_link(&format!("  eustress-player://join/81.2.69.160:7777?key={KEY}&pin={pin} ")), Ok(want.clone()));
        assert_eq!(direct_join_link(&format!("EUSTRESS://join/81.2.69.160:7777/?pin={pin}&key={KEY}")), Ok(want.clone()));
        assert_eq!(
            direct_join_link(&format!("eustress-player://join/81.2.69.160:7777?key={KEY}&pin={pin}&ticket=abc.def&api=staging")),
            Ok(want),
            "the ticket and the API stay behind"
        );
        assert_eq!(
            direct_join_link(&format!("eustress-player://join/play.example.com:30000?key={KEY}&pin={pin}")),
            Ok(format!("eustress-player://join/play.example.com:30000?key={KEY}&pin={}", pin.to_ascii_lowercase()))
        );
    }

    /// What cannot join is refused in fixed words that never quote the
    /// paste, which holds the key.
    #[test]
    fn hosting_direct_join_refuses_what_cannot_join_without_quoting_it() {
        let pin = pin();
        let link = |rest: &str| format!("eustress-player://join/{rest}");
        let cases = [
            ("81.2.69.160:7777".to_string(), ADDRESS_ALONE),
            ("https://example.com/".to_string(), NOT_A_LINK),
            ("javascript:alert(1)".to_string(), NOT_A_LINK),
            (link(&format!("81.2.69.160?key={KEY}&pin={pin}")), NO_PORT),
            (link(&format!("81.2.69.160:0?key={KEY}&pin={pin}")), NO_PORT),
            (link(&format!("81.2.69.160:99999?key={KEY}&pin={pin}")), NO_PORT),
            (link(&format!("[2001:db8::7]:7777?key={KEY}&pin={pin}")), IPV6),
            (link(&format!("2001:db8::7:7777?key={KEY}&pin={pin}")), IPV6),
            (link(&format!("bad_host!:7777?key={KEY}&pin={pin}")), BAD_HOST),
            (link("81.2.69.160:7777"), NO_KEY),
            (link(&format!("81.2.69.160:7777?pin={pin}")), NO_KEY),
            (link(&format!("81.2.69.160:7777?key=short&pin={pin}")), NO_KEY),
            (link(&format!("81.2.69.160:7777?key={KEY}")), NO_PIN),
            (link(&format!("81.2.69.160:7777?key={KEY}&pin=abcd")), NO_PIN),
            (link(&format!("81.2.69.160:7777?key={KEY}&key={KEY}&pin={pin}")), TWICE),
        ];
        for (pasted, want) in cases {
            let got = direct_join_link(&pasted);
            assert_eq!(got, Err(want), "{pasted}");
            assert!(!want.contains(KEY) && !want.contains(&pin), "a reason quoted the paste");
        }
    }
}
