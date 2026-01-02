// =============================================================================
// Eustress Web - Terms of Service Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn TermsPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />
            
            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Terms of Service"</h1>
                    <p class="legal-updated">"Last updated: December 6, 2025"</p>
                </div>
                
                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"1. Acceptance of Terms"</h2>
                        <p>"By accessing or using Eustress Engine (\"Service\"), you agree to be bound by these Terms of Service. If you do not agree to these terms, do not use the Service."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"2. Description of Service"</h2>
                        <p>"Eustress Engine is a game development platform that allows users to create, share, and play interactive 3D experiences. The Service includes:"</p>
                        <ul>
                            <li>"Eustress Engine - Game creation tools"</li>
                            <li>"Eustress Player - Platform for playing community games"</li>
                            <li>"Creator Marketplace - Asset and content marketplace"</li>
                            <li>"Bliss Currency - Virtual currency for cosmetics and marketplace purchases"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"3. User Accounts"</h2>
                        <p>"To use certain features, you must create an account. You are responsible for:"</p>
                        <ul>
                            <li>"Maintaining the security of your account"</li>
                            <li>"All activities that occur under your account"</li>
                            <li>"Providing accurate account information"</li>
                        </ul>
                        <p>"You must be at least 13 years old to create an account. Users under 18 require parental consent."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"4. User Content"</h2>
                        <p>"You retain ownership of content you create. By uploading content, you grant Eustress Engine a worldwide, non-exclusive license to host, display, and distribute your content on the platform."</p>
                        <p>"You agree not to upload content that:"</p>
                        <ul>
                            <li>"Infringes on intellectual property rights"</li>
                            <li>"Contains illegal, harmful, or offensive material"</li>
                            <li>"Violates the privacy of others"</li>
                            <li>"Contains malware or malicious code"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"5. Virtual Currency (Bliss)"</h2>
                        <p>"Bliss is a virtual currency with no real-world value. Bliss:"</p>
                        <ul>
                            <li>"Is purchased via our payment system"</li>
                            <li>"Cannot be exchanged for real money"</li>
                            <li>"Is non-refundable except as required by law"</li>
                            <li>"May only be used for cosmetic items and marketplace purchases"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"6. Subscriptions"</h2>
                        <p>"Premium subscriptions (Player Plus, Creator Pro, Bundle) are billed through our payment system. You may cancel at any time, and your benefits continue until the end of the billing period."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"7. Creator Revenue"</h2>
                        <p>"Creators earn a share of marketplace sales (25% free tier, 40% Creator Pro). Revenue is paid in Bliss currency. Minimum payout thresholds may apply."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"8. Prohibited Conduct"</h2>
                        <p>"You agree not to:"</p>
                        <ul>
                            <li>"Harass, bully, or threaten other users"</li>
                            <li>"Exploit bugs or use cheats"</li>
                            <li>"Attempt to gain unauthorized access"</li>
                            <li>"Interfere with the Service's operation"</li>
                            <li>"Engage in fraud or deceptive practices"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"9. Termination"</h2>
                        <p>"We may suspend or terminate your account for violations of these terms. Upon termination, you lose access to your account and any purchased content."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"10. Disclaimer of Warranties"</h2>
                        <p>"The Service is provided \"as is\" without warranties of any kind. We do not guarantee uninterrupted or error-free operation."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"11. Limitation of Liability"</h2>
                        <p>"To the maximum extent permitted by law, Eustress Engine shall not be liable for any indirect, incidental, or consequential damages."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"12. Changes to Terms"</h2>
                        <p>"We may update these terms at any time. Continued use of the Service after changes constitutes acceptance of the new terms."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"13. Contact"</h2>
                        <p>"For questions about these terms, contact us at legal@eustress.dev"</p>
                    </section>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}
