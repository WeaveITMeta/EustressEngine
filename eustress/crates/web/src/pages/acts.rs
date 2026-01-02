// =============================================================================
// Eustress Web - Legal Acts Compliance Page
// =============================================================================

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

#[component]
pub fn ActsPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />
            
            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Legal Compliance"</h1>
                    <p class="legal-updated">"Our commitment to legal and regulatory compliance"</p>
                </div>
                
                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"Regulatory Compliance"</h2>
                        <p>"Eustress Engine is committed to complying with all applicable laws and regulations. Below are the key legal frameworks we adhere to:"</p>
                    </section>
                    
                    <div class="acts-grid">
                        <a href="/acts/ccpa" class="act-card">
                            <h3>"CCPA"</h3>
                            <p class="act-full">"California Consumer Privacy Act"</p>
                            <p class="act-desc">"Privacy rights for California residents including data access, deletion, and opt-out rights."</p>
                        </a>
                        
                        <a href="/acts/coppa" class="act-card">
                            <h3>"COPPA"</h3>
                            <p class="act-full">"Children's Online Privacy Protection Act"</p>
                            <p class="act-desc">"Protections for children under 13, including parental consent requirements."</p>
                        </a>
                        
                        <a href="/acts/csam" class="act-card">
                            <h3>"CSAM Policy"</h3>
                            <p class="act-full">"Child Sexual Abuse Material"</p>
                            <p class="act-desc">"Zero tolerance policy with immediate reporting to NCMEC and law enforcement."</p>
                        </a>
                        
                        <a href="/dmca" class="act-card">
                            <h3>"DMCA"</h3>
                            <p class="act-full">"Digital Millennium Copyright Act"</p>
                            <p class="act-desc">"Copyright protection and takedown procedures for infringing content."</p>
                        </a>
                        
                        <a href="/acts/gdpr" class="act-card">
                            <h3>"GDPR"</h3>
                            <p class="act-full">"General Data Protection Regulation"</p>
                            <p class="act-desc">"EU data protection rights including consent, access, portability, and erasure."</p>
                        </a>
                        
                        <a href="/acts/tida" class="act-card">
                            <h3>"TIDA"</h3>
                            <p class="act-full">"Take It Down Act"</p>
                            <p class="act-desc">"Protections against non-consensual intimate imagery, requiring platforms to remove such content upon request."</p>
                        </a>
                    </div>
                    
                    <section class="legal-section">
                        <h2>"Reporting Violations"</h2>
                        <p>"If you believe there has been a violation of any of these regulations, please contact us:"</p>
                        <ul>
                            <li>"Privacy concerns: "<a href="mailto:privacy@eustress.dev">"privacy@eustress.dev"</a></li>
                            <li>"Copyright/DMCA: "<a href="mailto:dmca@eustress.dev">"dmca@eustress.dev"</a></li>
                            <li>"Child safety: "<a href="mailto:safety@eustress.dev">"safety@eustress.dev"</a></li>
                            <li>"General legal: "<a href="mailto:legal@eustress.dev">"legal@eustress.dev"</a></li>
                        </ul>
                    </section>
                </div>
            </div>
            
            <Footer />
        </div>
    }
}
