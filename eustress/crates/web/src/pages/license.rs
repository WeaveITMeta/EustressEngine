// =============================================================================
// Eustress Web - License Page (PolyForm Shield + Commercial dual license)
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn LicensePage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />

            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Build anything. Sell anything. Just don't sell Eustress."</h1>
                    <p class="legal-updated">"Eustress is source-available under the PolyForm Shield License 1.0.0, free for everyone with one exception: competing with the engine itself. A commercial license covers everything the Shield license does not."</p>
                </div>

                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"Two licenses. One line between them."</h2>
                        <p>"Every file in the repository ships under PolyForm Shield 1.0.0. That license is free, permanent, and requires no signup, no key, and no conversation with us."</p>
                        <p>"It grants you the right to use, run, modify, fork, and distribute Eustress for any purpose except one: providing a product that competes with Eustress itself."</p>
                        <p>"If you need that one thing, or if your legal team needs conventional commercial terms, the Eustress Commercial License exists for exactly that."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"What the Shield license gives you, free, forever."</h2>
                        <ul>
                            <li>"Build and sell games, simulations, digital twins, training environments, and visualizations made with Eustress. Keep 100% of the revenue. No royalty, no platform tax, no threshold."</li>
                            <li>"Use it in production, at a company of any size, without asking."</li>
                            <li>"Modify the source, keep your changes private or publish them."</li>
                            <li>"Fork it. If Eustress vanished tomorrow, your fork would still compile and still ship."</li>
                            <li>"Academic work, research, evaluation, personal projects: all covered."</li>
                        </ul>
                        <p>"The free tier is the whole engine, not a crippled demo. The thing we open is the thing we run."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"The one thing it does not give you."</h2>
                        <p>"You may not take Eustress and offer it, a fork of it, or a substantially similar engine, editor, or simulation platform as a product of your own, hosted or distributed, paid or free."</p>
                        <p>"That is the entire restriction. It is called a shield for a reason: it protects the project from being strip-mined by a larger competitor, so it can stay free for everyone actually building with it."</p>
                        <p>"Your game is not a competing product. Your digital twin is not a competing product. Your consulting work built on Eustress is not a competing product. A rebranded Eustress-as-a-service is."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"The commercial license."</h2>
                        <p>"If your use crosses that line, or your organization needs terms the Shield license cannot offer, we sell a commercial license, negotiated per organization."</p>
                        <ul>
                            <li>"Rights beyond the noncompete, where we choose to grant them."</li>
                            <li>"A perpetual grant for a specified version range, independent of the public repo."</li>
                            <li>"Warranties, indemnification, and support SLAs for procurement and legal teams."</li>
                            <li>"Pricing scales with the rights granted, not with revenue the Shield license already permits."</li>
                        </ul>
                        <p>"No surprise invoice. If you are building with Eustress rather than building a substitute for it, you will never need this page's second half."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Why not plain open source?"</h2>
                        <p>"We tried the pure route in our heads a hundred times. The failure mode is always the same: a hyperscaler forks the engine, outspends the community, and the original starves."</p>
                        <p>"PolyForm Shield is the honest middle. The source is fully readable, forkable, and free for builders, and the one entity it says no to is a competitor reselling our own engine against us."</p>
                        <p>"Commercial revenue from that line flows back into the platform economy that pays builders. We win when builders win. That is the only deal that holds."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Read it for yourself."</h2>
                        <p>"Do not take our summary as the contract. The LICENSE file is the contract, and PolyForm publishes the canonical text."</p>
                        <p>"If you are planning something big, or you are not sure which side of the line you are on, ask us. The answer is usually: you are fine."</p>
                        <p>"Commercial and licensing questions: "<a href="mailto:licensing@eustress.dev">"licensing@eustress.dev"</a></p>
                    </section>

                    <section class="legal-section legal-cta-row">
                        <a href="https://github.com/WeaveITMeta/EustressEngine/blob/main/LICENSE" target="_blank" rel="noopener" class="btn-primary-steel">"Read the LICENSE on GitHub"<span class="btn-icon">"→"</span></a>
                        <a href="https://polyformproject.org/licenses/shield/1.0.0" target="_blank" rel="noopener" class="btn-secondary-steel">"PolyForm Shield 1.0.0 (canonical text)"<span class="btn-icon">"→"</span></a>
                    </section>
                </div>
            </div>

            <Footer />
        </div>
    }
}
