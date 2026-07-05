// =============================================================================
// Eustress Web - Support Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn SupportPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />

            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Stuck? Let's get you unstuck."</h1>
                    <p class="legal-updated">"Eustress is a small, builder-run operation. Real people, no phone tree. Pick the channel that fits your problem and we answer as fast as we can."</p>
                </div>

                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"Start here: pick the right channel"</h2>
                        <p>"Help moves faster when it lands in the right place."</p>
                        <p>"Three doors, each built for a different kind of problem."</p>
                        <ul>
                            <li>"Discord: community and quick questions. The fastest way to get a human and other builders who have hit the same wall. https://discord.gg/DGP9my8DYN"</li>
                            <li>"GitHub issues: bugs and feature requests. Public, tracked, and tied straight to the code. https://github.com/WeaveITMeta/EustressEngine/issues"</li>
                            <li>"Email: account, billing, and anything private. support@eustress.dev"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"Quick questions and getting moving"</h2>
                        <p>"Not sure where a button is. Not sure if something is a bug or just you."</p>
                        <p>"Ask in Discord. Someone is usually around, and odds are good another builder already solved it."</p>
                        <p>"Want to try first before you ask. Learn and the docs cover the common path."</p>
                        <ul>
                            <li>"Learn: hands-on, step by step. /learn"</li>
                            <li>"Getting Started docs: install, first space, first build. /docs/getting-started"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"Found a bug or want a feature"</h2>
                        <p>"Bugs and feature requests go to GitHub issues."</p>
                        <p>"Public on purpose. You can see what is already known, follow the fix, and you are not shouting into a void."</p>
                        <p>"A good report saves everyone a round trip."</p>
                        <ul>
                            <li>"What you did, what you expected, what actually happened."</li>
                            <li>"Your OS and which version or build you are on."</li>
                            <li>"Screenshots, logs, or a tiny repro if you have them."</li>
                            <li>"Search open issues first; if it is already filed, add your details instead of opening a duplicate."</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"Accounts and identity (KYC)"</h2>
                        <p>"Anything tied to who you are belongs in email, not a public channel."</p>
                        <p>"Sign-in trouble, identity verification, locked accounts, data requests: support@eustress.dev."</p>
                        <p>"Never post identity documents, recovery codes, or private details in Discord or a GitHub issue. Keep that to email."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Bliss and Tickets"</h2>
                        <p>"Tickets are what you buy. Bliss is what you earn for real contribution, and it can cash out."</p>
                        <p>"Questions about how the economy works are fair game in Discord; we are happy to walk through it."</p>
                        <p>"Anything touching your balance, a payout, or a charge is private. Send those to support@eustress.dev."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Downloads, installs, and updates"</h2>
                        <p>"Most install snags are covered in the getting-started docs; that is the fastest path."</p>
                        <p>"If the install still will not cooperate, bring it to Discord with your OS and what you are seeing."</p>
                        <p>"If it is reproducible and looks like a real defect, file it on GitHub so it gets tracked and fixed."</p>
                        <ul>
                            <li>"Getting Started: /docs/getting-started"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"Commercial and license questions"</h2>
                        <p>"Using Eustress for something serious. Forking it. Building a business on top."</p>
                        <p>"Email support@eustress.dev with what you are building and what you need cleared up."</p>
                        <p>"Real questions get real answers. We would rather you ask up front than guess."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"What to expect from us"</h2>
                        <p>"Straight talk: this is a small, builder-run operation. One founder plus AI tooling, community-first."</p>
                        <p>"No call center, no scripted lines, no fake guaranteed response time we cannot honor."</p>
                        <p>"What you get instead: honest answers, and the same care that goes into the engine. We read everything and we answer as fast as we can. Discord is usually the fastest."</p>
                    </section>

                    <section class="legal-section">
                        <a href="https://discord.gg/DGP9my8DYN" target="_blank" rel="noopener" class="btn-primary-steel">"Join the Discord"<span class="btn-icon">"→"</span></a>
                    </section>
                </div>
            </div>

            <Footer />
        </div>
    }
}
