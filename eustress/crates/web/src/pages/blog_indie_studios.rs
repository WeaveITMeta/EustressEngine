// =============================================================================
// Eustress Web - Blog Post: The Roblox Model Is a Trap
// =============================================================================
// Source: docs/Marketing/EustressIndieStudiosSalesLetter.md
// Audience: indie game studios evaluating engine/platform commitments.
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[derive(Clone, Debug, PartialEq)]
struct TocItem {
    id: &'static str,
    title: &'static str,
}

fn get_toc() -> Vec<TocItem> {
    vec![
        TocItem { id: "platform-tax", title: "The Platform Tax Isn't the Worst Part" },
        TocItem { id: "asset-lockin", title: "Asset Lock-In" },
        TocItem { id: "own-it", title: "What If You Could Just Own It?" },
        TocItem { id: "what-you-get", title: "What You Actually Get" },
        TocItem { id: "bullets", title: "What Keeps Founders Up At Night" },
        TocItem { id: "proof", title: "The Proof Is In The Simulation" },
        TocItem { id: "offer", title: "What You Get When You Come Aboard" },
        TocItem { id: "guarantee", title: "The Guarantee" },
        TocItem { id: "urgency", title: "The Urgency" },
        TocItem { id: "start-today", title: "Start Today" },
        TocItem { id: "ps", title: "P.S." },
    ]
}

#[component]
pub fn BlogIndieStudiosPage() -> impl IntoView {
    let active_section = RwSignal::new("platform-tax".to_string());

    view! {
        <div class="page page-docs">
            <CentralNav active="".to_string() />

            <div class="docs-bg">
                <div class="docs-grid-overlay"></div>
                <div class="docs-glow glow-philosophy"></div>
            </div>

            <div class="docs-layout">
                <aside class="docs-toc">
                    <div class="toc-header">
                        <img src="/assets/icons/brain.svg" alt="Blog" class="toc-icon" />
                        <h2>"On This Page"</h2>
                    </div>
                    <nav class="toc-nav">
                        {get_toc().into_iter().map(|item| {
                            let id = item.id.to_string();
                            let is_active = {
                                let id = id.clone();
                                move || active_section.get() == id
                            };
                            view! {
                                <div class="toc-section">
                                    <a
                                        href=format!("#{}", item.id)
                                        class="toc-section-title"
                                        class:active=is_active
                                    >
                                        {item.title}
                                    </a>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </nav>

                    <div class="toc-footer">
                        <a href="/blog" class="toc-back">
                            <img src="/assets/icons/arrow-left.svg" alt="Back" />
                            "Back to Blog"
                        </a>
                    </div>
                </aside>

                <main class="docs-content">
                    <header class="docs-hero">
                        <div class="docs-breadcrumb">
                            <a href="/blog">"Blog"</a>
                            <span class="separator">"/"</span>
                            <span class="current">"Indie Studios"</span>
                        </div>
                        <div class="hero-header">
                            <div class="header-line"></div>
                            <span class="header-tag">
                                "AN OPEN MESSAGE TO INDIE GAME STUDIOS TIRED OF BUILDING ON SOMEONE ELSE'S LAND"
                            </span>
                            <div class="header-line"></div>
                        </div>
                        <h1 class="docs-title">
                            "The Roblox Model Is a Trap — And Every Studio That Doesn't See It Yet Is Already Inside the Cage"
                        </h1>
                        <p class="docs-subtitle">
                            <em>
                                "A new simulation engine just went open source. It runs a full year of "
                                "world simulation in one second, renders at AAA quality, and takes zero "
                                "percent of your revenue. Here's why that matters more than any feature list."
                            </em>
                        </p>
                        <div class="docs-meta">
                            <span class="meta-item">
                                <img src="/assets/icons/clock.svg" alt="Time" />
                                "12 min read"
                            </span>
                            <span class="meta-item">
                                <img src="/assets/icons/code.svg" alt="Audience" />
                                "Indie Studios"
                            </span>
                        </div>
                    </header>

                    <section class="docs-section">
                        <div class="docs-block">
                            <p>
                                "If you're an indie studio — three people, maybe ten, maybe just you "
                                "and a contractor you trust — this is written for you."
                            </p>
                            <p>
                                "You've shipped something. Or you're close. You know what it costs "
                                "to build in this industry: the late nights, the scope creep, the "
                                "engine bugs you didn't write but have to fix anyway. You know the "
                                "math of platform fees and revenue share better than most accountants. "
                                "And somewhere in the back of your head, you've got a number — the "
                                "number that represents what you "<em>"would"</em>" have made if the "
                                "platform hadn't taken its cut."
                            </p>
                            <p>"That number bothers you. It should."</p>
                            <p>
                                "Because here's what nobody in the engine or platform business wants "
                                "to say out loud: the deal you signed when you chose their platform "
                                "wasn't just a licensing agreement. It was a land lease. You built "
                                "the house. They own the dirt."
                            </p>
                            <p>
                                "I want to tell you about something a small team built in Rust over "
                                "the last few years. Not a game. Not a game engine in the traditional "
                                "sense. Something closer to what you'd get if you took the simulation "
                                "layer underneath a game and made it the "<em>"whole product"</em>
                                " — a universal world-model engine called "<strong>"Eustress"</strong>"."
                            </p>
                            <p>
                                "I'll get to what it does in a minute. But first, I need to make "
                                "sure you understand what's actually at stake here, because if you "
                                "don't feel the problem in your gut, the solution won't mean anything."
                            </p>
                        </div>
                    </section>

                    <section id="platform-tax" class="docs-section">
                        <h2 class="section-anchor">"The Platform Tax Isn't the Worst Part"</h2>
                        <div class="docs-block">
                            <p>
                                "Every indie studio knows about revenue share. Unity's pricing "
                                "implosion in 2023 made it impossible to ignore. Roblox takes 75% "
                                "of what developers earn before a creator sees a dollar. Epic has "
                                "its cut. Steam has its 30%. The platforms will tell you this is "
                                "fair — infrastructure costs money, distribution costs money, they "
                                "built the audience."
                            </p>
                            <p>"Fine. Let's accept that for a second."</p>
                            <p>
                                "The "<em>"real"</em>" problem isn't the percentage. The real problem is "
                                <strong>"what happens when the terms change"</strong>"."
                            </p>
                            <p>
                                "You don't own the platform. You don't have a vote. You built your "
                                "studio's entire technical foundation on infrastructure controlled "
                                "by a board of directors whose incentives are not aligned with yours. "
                                "When Unity changed its runtime fee structure overnight, studios that "
                                "had shipped games — "<em>"finished, live, earning games"</em>
                                " — suddenly faced a retroactive tax on installs they'd already paid to acquire."
                            </p>
                            <div class="docs-callout warning">
                                <strong>"That's not a business relationship."</strong>
                                " That's a hostage situation with a friendly UI."
                            </div>
                            <p>
                                "And it's not a Unity-specific problem. It's a structural problem "
                                "with building on any platform you don't control."
                            </p>
                        </div>
                    </section>

                    <section id="asset-lockin" class="docs-section">
                        <h2 class="section-anchor">"The Thing Nobody Talks About: Asset Lock-In"</h2>
                        <div class="docs-block">
                            <p>"Here's the conversation that doesn't happen enough in indie circles."</p>
                            <p>
                                "You spend two years building a world. Custom shaders, proprietary "
                                "physics behaviors, a simulation system you're genuinely proud of. "
                                "You ship it. It works. Players love it."
                            </p>
                            <p>
                                "Then the engine you built it in gets acquired. Or deprecated. Or "
                                "the licensing terms shift in a direction that makes your business "
                                "model unworkable."
                            </p>
                            <p>
                                "What do you do? You can't take your world with you. The assets "
                                "are in a format tied to that engine. The physics behaviors are "
                                "baked into a system you don't have source access to. You can't "
                                "fork it. You can't fix it. You can't even fully understand it."
                            </p>
                            <p>"You start over. Or you comply."</p>
                            <p>
                                "Most studios comply. Because starting over is existential. And "
                                "the platform knows it."
                            </p>
                        </div>
                    </section>

                    <section id="own-it" class="docs-section">
                        <h2 class="section-anchor">"What If You Could Just… Own It?"</h2>
                        <div class="docs-block">
                            <p>"This is where Eustress comes in."</p>
                            <p>
                                "Eustress is a universal world-model simulation engine written in "
                                "Rust. Open source. Forkable. Yours."
                            </p>
                            <p>
                                "Not \"open source\" in the way some companies use that phrase to "
                                "mean \"you can read the code but good luck changing it.\" Actually "
                                "forkable. If the project went dark tomorrow, you'd have everything "
                                "you need to keep running, keep building, keep shipping. The source "
                                "is yours. The fork is yours. The world you build inside it is yours."
                            </p>
                            <p>"Here's what it does:"</p>
                            <p>
                                "It simulates millions of entities simultaneously. Not \"up to a few "
                                "thousand with good optimization\" — millions. Running a full year "
                                "of simulation time in one second of real time. The kind of simulation "
                                "density that lets you build worlds that actually "<em>"behave"</em>
                                " like worlds, not like game levels pretending to be worlds."
                            </p>
                            <p>
                                "The rendering isn't an afterthought. Eustress ships with AAA-grade "
                                "photoreal output and a Slint UI layer that looks like something a "
                                "major studio spent years building. You can show a screenshot from "
                                "an Eustress-powered project and nobody asks \"is this indie?\" They "
                                "ask \"what studio made this?\""
                            </p>
                            <div class="docs-callout success">
                                <strong>"And the business model is simple:"</strong>
                                " zero licensing fees. Zero revenue share. You ship, you earn, you keep it."
                            </div>
                        </div>
                    </section>

                    <section id="what-you-get" class="docs-section">
                        <h2 class="section-anchor">"What You Actually Get When You Build on Eustress"</h2>
                        <div class="docs-block">
                            <p>
                                "Let me be specific, because vague promises are the currency of "
                                "every platform pitch you've ever heard:"
                            </p>
                            <ul class="docs-list">
                                <li>
                                    <strong>"A simulation substrate that scales to your ambition."</strong>
                                    " Millions of entities. A year of sim time per second. Whether "
                                    "you're building a survival game, a city sim, a persistent world, "
                                    "or something that doesn't have a genre name yet — the engine "
                                    "doesn't cap your ceiling."
                                </li>
                                <li>
                                    <strong>"Photoreal rendering out of the box."</strong>
                                    " The Slint UI and AAA-grade rendering pipeline mean your first "
                                    "build looks like your final build. You're not spending six months "
                                    "making it \"look good enough to show.\" It looks good on day one."
                                </li>
                                <li>
                                    <strong>"Full source access and forkability."</strong>
                                    " You can read it, modify it, fork it, and ship your fork. If "
                                    "you find a bug, you can fix it. If you need a behavior the engine "
                                    "doesn't have, you can add it. No waiting on a support ticket. "
                                    "No praying for a patch in the next quarterly release."
                                </li>
                                <li>
                                    <strong>"No platform tax on your revenue."</strong>
                                    " What you earn, you keep. The math on this compounds fast. Run "
                                    "the numbers on your last twelve months of revenue and apply a "
                                    "30% platform fee. That's the check you didn't have to write."
                                </li>
                                <li>
                                    <strong>"The Bliss contributor economy."</strong>
                                    " Eustress isn't just open source — it has a built-in economy "
                                    "for builders. Contributors earn Bliss tokens for meaningful work "
                                    "on the engine and ecosystem. If your studio contributes improvements "
                                    "back, you're not just being altruistic — you're building equity "
                                    "in the platform itself."
                                </li>
                            </ul>
                        </div>
                    </section>

                    <section id="bullets" class="docs-section">
                        <h2 class="section-anchor">"What Keeps Indie Founders Up At Night (And What Eustress Does About Each One)"</h2>
                        <div class="docs-block">
                            <ul class="docs-list">
                                <li>
                                    "How a studio can simulate a living world with millions of "
                                    "moving parts — weather, economies, factions, wildlife — without "
                                    "a single server farm compromise… and why the \"just use Unity\" "
                                    "crowd hasn't figured out why this matters yet."
                                </li>
                                <li>
                                    "The counterintuitive reason that "<em>"more"</em>" simulation "
                                    "fidelity actually makes your game "<em>"faster"</em>" to build — "
                                    "and why studios that understand this are shipping in half the "
                                    "time of their peers."
                                </li>
                                <li>
                                    "Why \"open source\" only protects you if the engine is actually "
                                    "forkable — and the one clause in most \"open\" licenses that "
                                    "means you're still locked in without knowing it."
                                </li>
                                <li>
                                    "How Eustress's Rust foundation eliminates an entire category "
                                    "of performance bugs that Unreal and Unity developers treat as "
                                    "a cost of doing business — and what that means for your crunch "
                                    "schedule."
                                </li>
                                <li>
                                    "The visual proof that photoreal doesn't require a AAA budget "
                                    "anymore — and why the first time you show an Eustress build to "
                                    "a publisher, the conversation changes."
                                </li>
                                <li>
                                    "Why building on a platform you can fork is the only real answer "
                                    "to the Unity runtime fee problem — and what \"platform risk\" "
                                    "actually costs a studio over a five-year horizon."
                                </li>
                                <li>
                                    "How the Bliss contributor economy turns your engine improvements "
                                    "into assets, not just expenses — and why the studios that figure "
                                    "this out early will have a structural advantage over everyone "
                                    "who doesn't."
                                </li>
                                <li>
                                    "The reason \"the platform Roblox should have been\" isn't "
                                    "marketing language — it's a technical description of what happens "
                                    "when you build a general-purpose simulation substrate instead of "
                                    "a game engine with simulation bolted on."
                                </li>
                            </ul>
                        </div>
                    </section>

                    <section id="proof" class="docs-section">
                        <h2 class="section-anchor">"The Proof Is In The Simulation"</h2>
                        <div class="docs-block">
                            <p>
                                "Eustress runs a year of simulation in one second. That's not a "
                                "benchmark cherry-picked under ideal conditions. That's the baseline."
                            </p>
                            <p>
                                "What does that mean in practice? It means your world doesn't fake "
                                "it. NPC economies don't run on a simplified tick system that "
                                "approximates behavior. Weather doesn't cycle on a timer. Ecosystems "
                                "don't cheat. The simulation is deep enough that emergent behavior — "
                                "the stuff that makes players feel like they discovered something, "
                                "not that a designer scripted it — actually emerges."
                            </p>
                            <p>
                                "The rendering pipeline produces output that competes with studios "
                                "spending ten times what you're spending. The Slint UI layer is "
                                "production-grade. This isn't a tech demo engine. It's a ship-it engine."
                            </p>
                            <p>
                                "And because it's written in Rust, the performance characteristics "
                                "are predictable in ways that C++ engines often aren't. Memory safety "
                                "is structural, not aspirational. The class of bugs that eats weeks "
                                "of crunch in other engines simply doesn't exist here."
                            </p>
                        </div>
                    </section>

                    <section id="offer" class="docs-section">
                        <h2 class="section-anchor">"Here's What You Get When You Come Aboard"</h2>
                        <div class="docs-block">
                            <h3>"Core: Eustress Engine — Full Source Access"</h3>
                            <p>
                                "Everything. The simulation core, the rendering pipeline, the Slint "
                                "UI layer, the Rust toolchain integration. Fork it, ship it, build "
                                "on it. The alternative is a per-seat license on a platform that "
                                "owns your roadmap."
                            </p>

                            <h3>"The Bliss Contributor Economy — Earn While You Build"</h3>
                            <p>
                                "Every meaningful contribution to the Eustress ecosystem earns Bliss "
                                "tokens. Bug fixes, performance improvements, new modules, documentation — "
                                "the work you'd do anyway to make the engine fit your needs now pays "
                                "you back."
                            </p>

                            <h3>"Bonus: The Eustress Studio Fast-Start Kit"</h3>
                            <p>
                                "The fastest path from zero to a running Eustress project. Includes "
                                "reference architectures for the three most common indie use cases — "
                                "open world survival, persistent multiplayer, and simulation-heavy "
                                "strategy — with documented patterns for each. Built by the team that "
                                "built the engine."
                            </p>

                            <h3>"Bonus: Priority Contributor Access"</h3>
                            <p>
                                "Early studios get direct access to the core contributors. Not a "
                                "Discord channel. Not a forum. Actual access to the people who built "
                                "this, for the specific purpose of making sure your first project ships."
                            </p>

                            <div class="docs-callout info">
                                <strong>"Your investment:"</strong>
                                " Free and open source. Optional paid support tiers for studios that "
                                "want direct engineering partnership."
                            </div>
                        </div>
                    </section>

                    <section id="guarantee" class="docs-section">
                        <h2 class="section-anchor">"The Guarantee"</h2>
                        <div class="docs-block">
                            <p>"I'm going to say something the major platforms will never say to you:"</p>
                            <div class="manifesto-quote">
                                <blockquote>
                                    "Fork it. Build on it. If six months from now you decide Eustress "
                                    "isn't the right foundation for your studio, you keep everything "
                                    "you built. The source you forked is yours. The world you built "
                                    "is yours. The improvements you made are yours. We're not holding "
                                    "your work hostage to a subscription."
                                </blockquote>
                            </div>
                            <p>
                                "That's not a safety net. That's a statement about what we believe "
                                "this engine is going to do for you."
                            </p>
                            <p>
                                "The platforms that don't offer this guarantee are the ones who know "
                                "you can't leave. We're offering it because we know you won't want to."
                            </p>
                        </div>
                    </section>

                    <section id="urgency" class="docs-section">
                        <h2 class="section-anchor">"The Urgency — And I'm Going to Be Straight With You About It"</h2>
                        <div class="docs-block">
                            <p>
                                "I'm not going to tell you this offer expires at midnight or that "
                                "there are only 17 spots left. That's not the urgency here."
                            </p>
                            <p>"The urgency is this:"</p>
                            <p>
                                "Right now, your competitors are building on platforms that own them. "
                                "Most of them don't know it yet. They're optimizing inside a cage, "
                                "getting better at working around limitations they've accepted as "
                                "permanent, paying platform taxes they've budgeted as a cost of doing "
                                "business."
                            </p>
                            <p>
                                "The studios that move to a forkable, zero-revenue-share substrate "
                                "in the next 12 months are going to have a structural cost advantage "
                                "that compounds every year. Lower overhead. Full asset ownership. No "
                                "platform risk on their roadmap. A contributor economy that turns "
                                "their engine work into equity."
                            </p>
                            <p>
                                "The studios that don't move are going to keep paying. And the next "
                                "time a major platform changes its terms overnight — and there will "
                                "be a next time — they're going to be exactly as exposed as they are "
                                "today."
                            </p>
                            <div class="docs-callout warning">
                                "The question isn't whether to move. The question is whether you move "
                                <em>"before"</em>" or "<em>"after"</em>" the next Unity-style event "
                                "forces your hand."
                            </div>
                        </div>
                    </section>

                    <section id="start-today" class="docs-section">
                        <h2 class="section-anchor">"Here's What Happens When You Start Today"</h2>
                        <div class="docs-block">
                            <p>
                                "You clone the repo. You run the demo. You see a photoreal world "
                                "simulating millions of entities in real time, and something clicks — "
                                "not as a technical curiosity, but as a business decision."
                            </p>
                            <p>
                                "You fork it. You start building. Your first build looks better than "
                                "anything you've shipped before, and you built it faster because you "
                                "weren't fighting the engine."
                            </p>
                            <p>
                                "You ship. You keep 100% of what you earn. You contribute an improvement "
                                "back to the ecosystem and earn Bliss tokens for it. You're not a "
                                "tenant anymore. You're an owner."
                            </p>
                            <p>"That's the day-after picture. And it's available right now."</p>
                            <div class="future-cta">
                                <p><strong>"Ready to stop renting and start owning?"</strong></p>
                                <div class="cta-buttons">
                                    <a href="/download" class="btn-primary-glow">"Download Eustress"</a>
                                    <a href="https://github.com/WeaveITMeta/EustressEngine" class="btn-secondary-steel" target="_blank" rel="noopener">"Fork on GitHub"</a>
                                </div>
                            </div>
                        </div>
                    </section>

                    <section id="ps" class="docs-section">
                        <h2 class="section-anchor">"P.S."</h2>
                        <div class="docs-block">
                            <p>
                                "If you read nothing else: Eustress is an open-source, forkable "
                                "simulation engine that takes zero percent of your revenue, renders "
                                "at AAA quality, and simulates millions of entities in real time. "
                                "Early studios get direct contributor access and priority onboarding. "
                                "Fork it, build on it, and if it's not the right fit, everything you "
                                "built is still yours — we're not holding your work hostage. The "
                                "studios moving to platforms they own are going to have a structural "
                                "advantage over everyone still paying platform tax. The time to move "
                                "is before the next runtime-fee surprise, not after."
                            </p>
                        </div>
                    </section>

                    <nav class="docs-nav-footer">
                        <a href="/blog" class="nav-prev">
                            <img src="/assets/icons/arrow-left.svg" alt="Back" />
                            <div>
                                <span class="nav-label">"Back to"</span>
                                <span class="nav-title">"Blog Index"</span>
                            </div>
                        </a>
                        <a href="/download" class="nav-next">
                            <div>
                                <span class="nav-label">"Next"</span>
                                <span class="nav-title">"Download Eustress"</span>
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
