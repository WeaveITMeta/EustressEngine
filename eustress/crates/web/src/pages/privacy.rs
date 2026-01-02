// =============================================================================
// Eustress Web - Privacy Policy Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn PrivacyPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />
            
            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Privacy Policy"</h1>
                    <p class="legal-updated">"Last updated: December 6, 2025"</p>
                </div>
                
                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"1. Information We Collect"</h2>
                        <h3>"Account Information"</h3>
                        <p>"When you sign in via Discord, we receive:"</p>
                        <ul>
                            <li>"Discord ID and username"</li>
                            <li>"Profile picture (if public)"</li>
                            <li>"Email address (for account recovery)"</li>
                        </ul>
                        
                        <h3>"Usage Data"</h3>
                        <p>"We automatically collect:"</p>
                        <ul>
                            <li>"Games played and time spent"</li>
                            <li>"Content created and published"</li>
                            <li>"Marketplace transactions"</li>
                            <li>"Device and browser information"</li>
                            <li>"IP address and general location"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"2. How We Use Your Information"</h2>
                        <ul>
                            <li>"Provide and improve the Service"</li>
                            <li>"Process transactions and payments"</li>
                            <li>"Communicate with you about your account"</li>
                            <li>"Enforce our terms and policies"</li>
                            <li>"Detect and prevent fraud"</li>
                            <li>"Personalize your experience"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"3. Information Sharing"</h2>
                        <p>"We do not sell your personal information. We may share data with:"</p>
                        <ul>
                            <li>"Service providers (hosting, analytics, payment processing)"</li>
                            <li>"Discord for authentication"</li>
                            <li>"Law enforcement when legally required"</li>
                            <li>"Other users (public profile information only)"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"4. Data Retention"</h2>
                        <p>"We retain your data for as long as your account is active. After account deletion, we may retain certain data for legal compliance for up to 7 years."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"5. Your Rights"</h2>
                        <p>"Depending on your location, you may have the right to:"</p>
                        <ul>
                            <li>"Access your personal data"</li>
                            <li>"Correct inaccurate data"</li>
                            <li>"Delete your data"</li>
                            <li>"Export your data"</li>
                            <li>"Opt out of marketing communications"</li>
                        </ul>
                        <p>"To exercise these rights, contact privacy@eustress.dev"</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"6. Children's Privacy"</h2>
                        <p>"Users under 13 are not permitted to create accounts. For users 13-17, we implement additional protections including:"</p>
                        <ul>
                            <li>"Parental consent requirements"</li>
                            <li>"Restricted social features"</li>
                            <li>"Purchase limits and approvals"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"7. Security"</h2>
                        <p>"We implement industry-standard security measures including encryption, secure authentication, and regular security audits. However, no system is 100% secure."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"8. International Transfers"</h2>
                        <p>"Your data may be processed in countries outside your residence. We ensure appropriate safeguards are in place for international transfers."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"9. Changes to This Policy"</h2>
                        <p>"We may update this policy periodically. We will notify you of significant changes via email or in-app notification."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"10. Contact Us"</h2>
                        <p>"For privacy questions or concerns:"</p>
                        <p>"Email: privacy@eustress.dev"</p>
                    </section>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}
