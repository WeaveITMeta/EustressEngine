// =============================================================================
// Eustress Web - DMCA / Copyright Policy Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn DmcaPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />
            
            <div class="legal-container">
                <div class="legal-header">
                    <h1>"DMCA & Copyright Policy"</h1>
                    <p class="legal-updated">"Last updated: December 6, 2025"</p>
                </div>
                
                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"Respect for Intellectual Property"</h2>
                        <p>"Eustress Engine respects the intellectual property rights of others and expects users to do the same. We respond to notices of alleged copyright infringement in accordance with the Digital Millennium Copyright Act (DMCA)."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Reporting Copyright Infringement"</h2>
                        <p>"If you believe your copyrighted work has been copied in a way that constitutes infringement, please provide our DMCA Agent with the following information:"</p>
                        <ol>
                            <li>"A physical or electronic signature of the copyright owner or authorized agent"</li>
                            <li>"Identification of the copyrighted work claimed to be infringed"</li>
                            <li>"Identification of the infringing material and its location on our Service"</li>
                            <li>"Your contact information (address, phone number, email)"</li>
                            <li>"A statement that you have a good faith belief that the use is not authorized"</li>
                            <li>"A statement, under penalty of perjury, that the information is accurate and you are authorized to act on behalf of the copyright owner"</li>
                        </ol>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"DMCA Agent Contact"</h2>
                        <p>"Send DMCA notices to:"</p>
                        <p>"Email: dmca@eustress.dev"</p>
                        <p>"Subject line: DMCA Takedown Request"</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Counter-Notification"</h2>
                        <p>"If you believe your content was wrongly removed, you may submit a counter-notification containing:"</p>
                        <ol>
                            <li>"Your physical or electronic signature"</li>
                            <li>"Identification of the removed material and its previous location"</li>
                            <li>"A statement under penalty of perjury that you have a good faith belief the material was removed by mistake"</li>
                            <li>"Your name, address, phone number, and consent to jurisdiction"</li>
                        </ol>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Repeat Infringers"</h2>
                        <p>"We maintain a policy of terminating accounts of users who are repeat infringers. Users who receive multiple valid DMCA notices may have their accounts permanently suspended."</p>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Content Guidelines"</h2>
                        <p>"To avoid copyright issues, ensure your content:"</p>
                        <ul>
                            <li>"Is original or you have rights to use it"</li>
                            <li>"Properly attributes third-party assets"</li>
                            <li>"Uses only licensed or royalty-free resources"</li>
                            <li>"Does not copy substantial portions of others' work"</li>
                        </ul>
                    </section>
                    
                    <section class="legal-section">
                        <h2>"Trademark Policy"</h2>
                        <p>"Do not use trademarks (including \"Eustress\") in ways that may confuse users about affiliation or endorsement. Fan content is welcome but must be clearly identified as unofficial."</p>
                    </section>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}
