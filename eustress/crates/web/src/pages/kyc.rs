// =============================================================================
// Eustress Web - KYC Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn KycPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />

            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Real money needs real people."</h1>
                    <p class="legal-updated">"Bliss cashes out to real USD. That means real dollars reach real creators. So we verify who you are before you ever earn one. KYC from day one. Not a feature we bolted on later."</p>
                </div>

                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"The stake"</h2>
                        <p>"Most platforms let you stay anonymous until money shows up. Then they scramble."</p>
                        <p>"We do it backwards. We verify first."</p>
                        <p>"Here is the hard truth: money plus anonymity is how platforms get gamed and how communities get hurt."</p>
                        <p>"If you can earn real cash behind a fake name, someone will. And the people building in good faith pay for it."</p>
                        <p>"So identity comes first. Always."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"What KYC actually means here"</h2>
                        <p>"KYC stands for Know Your Customer. We take that literally."</p>
                        <p>"Before you transact, we confirm you are a real person in a jurisdiction we support."</p>
                        <ul>
                            <li>"Identity verified at sign-in, not after the fact"</li>
                            <li>"About 72 supported jurisdictions, IRS QI-approved"</li>
                            <li>"Unsupported jurisdictions are gated right at the door"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"One pass. One clear answer."</h2>
                        <p>"Verification is not a maze of forms and waiting. It runs as one automated pass."</p>
                        <p>"We check the document. We read it with OCR. We run criminal background screening."</p>
                        <p>"Out the other side comes a risk decision. APPROVE or REJECT. No guessing, no limbo."</p>
                        <ul>
                            <li>"Document verification"</li>
                            <li>"OCR to read and match the document"</li>
                            <li>"Criminal background screening"</li>
                            <li>"A single risk decision returned to you"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"You are a cryptographic identity"</h2>
                        <p>"Every account is anchored to an Ed25519 keypair."</p>
                        <p>"That is not jargon for its own sake. It means your identity is yours, provable, and hard to forge."</p>
                        <p>"One real person. One verified key. That is the foundation everything else is built on."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"What it blocks, honestly"</h2>
                        <p>"This is screening, not a guarantee of perfection. We will not claim a number we cannot stand behind."</p>
                        <p>"What it does do is enforce real rules at the gate."</p>
                        <ul>
                            <li>"Disqualifying criminal records are blocked per jurisdiction rules"</li>
                            <li>"Sanctioned regions do not get in"</li>
                            <li>"Unsupported jurisdictions are gated at sign-in, not strung along"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"How this protects you"</h2>
                        <p>"When the creator next to you is verified, you are building next to someone accountable."</p>
                        <p>"That changes everything about who you can trust and what you can stake on this platform."</p>
                        <ul>
                            <li>"Creators are accountable, because they are known"</li>
                            <li>"A community where bad actors cannot hide behind a name"</li>
                            <li>"Trust solid enough to build a real business on"</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"Your data, handled straight"</h2>
                        <p>"We ask for verification documents because the law and your safety require it. Then we protect them."</p>
                        <p>"KYC documents are encrypted and stored in Cloudflare R2."</p>
                        <p>"Your data is used for verification and compliance. That is the whole list."</p>
                        <p>"We do not sell it. Verified, secured, and used for exactly what we told you."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Get verified, then build"</h2>
                        <p>"The bar is real on purpose. It is the same bar that keeps your earnings real."</p>
                        <p>"Pass once. Then go make something worth cashing out."</p>
                    </section>

                    <section class="legal-section">
                        <a href="/login" class="btn-primary-steel">"Get Verified"<span class="btn-icon">"→"</span></a>
                    </section>
                </div>
            </div>

            <Footer />
        </div>
    }
}
