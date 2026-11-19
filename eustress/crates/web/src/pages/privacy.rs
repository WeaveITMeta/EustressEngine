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
                    <p class="legal-updated">"Last updated: November 15, 2026"</p>
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

                        <h3>"Playing as a Guest"</h3>
                        <p>"You can play without an account, as a guest. We do not ask guests for a name, email address, age or any other information about themselves."</p>
                        <ul>
                            <li>"When you start a guest session, we keep a one-way fingerprint of your network address, made with a key that changes every day, so it cannot be turned back into your address or matched from one day to the next. We use it only to count how many Spaces you start and how many minutes you play that day, and we delete it, with those counts, within 25 hours of the start of that day (UTC)."</li>
                            <li>"The character you choose (Male or Female) is carried in your guest session's ticket and is not stored."</li>
                            <li>"When the Player downloads a Space without an account, the download is counted once a day by a one-way fingerprint of your network address, which we keep for 2 days."</li>
                            <li>"We do not use cookies for guest play, and we do not link guest play to an account you create later."</li>
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
                            <li>"Our email service provider, GetCSV.io, and the AI providers it uses to write emails, only for marketing emails you chose to receive (see Marketing Emails below)"</li>
                        </ul>

                        <h3>"Marketing Emails"</h3>
                        <p>"Marketing emails are optional. You choose at sign-up with the Marketing Emails box, and you can change your mind at any time in Settings or with the unsubscribe link at the bottom of every marketing email."</p>
                        <p>"Where the law lets the box start ticked (today, the United States), it starts ticked and you can untick it. Everywhere else it starts empty, and only your own tick turns it on. We decide which applies from the country Cloudflare reports for your connection when you sign up."</p>
                        <p>"If you choose to receive them, we send Eustress news, release notes, a summary of your own activity (such as Spaces you published, plays and BLS earned) and holiday promotions: at most one email a week, plus holiday offers."</p>
                        <p>"To write and send these emails, we share your email address, username, plan, country and those activity figures with GetCSV.io, our email service provider. GetCSV.io stores them on Amazon Web Services and uses AI models from xAI and Anthropic to write a version of each email for you. They use this information only to send Eustress emails on our behalf. We share nothing from your identity verification, your chats or your support requests."</p>
                        <p>"We keep a record of each choice you make (what you chose, the words you were shown, the country and the time) as proof of your consent, for as long as your account exists and for 3 years after you turn marketing emails off. When you turn them off, we take you off the next list at once and tell GetCSV.io to stop emailing you straight away. The lists GetCSV.io holds are deleted after 30 days."</p>
                        <p>"Where the law requires your consent for marketing emails, that consent is our legal basis, and you can withdraw it at any time. Elsewhere, our basis is our legitimate interest in telling account holders about Eustress, and you can object at any time in the same ways."</p>
                        <p>"Emails about your account, such as sign-in, security notices and review results, are not marketing and do not depend on this choice."</p>
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
                        <p>"Accounts are for adults only. No one under 18, or under the age of majority where they live if that is higher, can create an account, and every account is verified against a government-issued photo ID at sign-up. If we learn that an account belongs to someone under 18, we close it and delete its data."</p>
                        <p>"Guest play, described in section 1, is open to people of any age and asks for no information about the player. Guests never see or play adult content, cannot buy anything or publish or post content, and their chat messages in live sessions are not delivered to other players."</p>
                        <p>"A parent or guardian with a question about a child's use of Eustress can contact privacy@eustress.dev."</p>
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
