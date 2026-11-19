// =============================================================================
// Eustress Web - Quality Gate (/quality-gate)
// =============================================================================
// The review every publication passes before it is listed, and the review a Space's themed page passes before anyone sees it.
// The publication half is the automated listing review in production (Guardian Code; docs/architecture/AI_GUARDIAN_POLICY_v1.0.md,
// docs/moderation/PLAYBOOK.md, docs/policy/COMMUNITY_STANDARDS.md). The page half is the link and image review of the Website
// service's themed page (docs/design/SIM_PAGE_THEME_AND_LINKS.md), which is rolling out: the page says so, and a test holds it
// until that review is live.
// =============================================================================

use leptos::prelude::*;

use crate::components::{CentralNav, Footer};

/// The highest risk score a link may carry and still be shown (SIM_PAGE_THEME_AND_LINKS.md, section 4).
pub const MAX_LINK_RISK: u8 = 25;
/// How often an approved link is reviewed again, in days.
pub const LINK_REREVIEW_DAYS: u8 = 7;

/// What a page may link to (the rubric's Allowed list, in the page's words).
pub const ALLOWED: [&str; 8] = [
    "Real businesses and their stores",
    "Engineering, electronics, robotics, 3D-printing and maker shops, kits and parts",
    "Licensed medical providers, device makers, research and health education",
    "Research labs, universities, papers, patents, startups and real innovations",
    "Creators' merchandise, commissions and donation pages",
    "Licensed firearm retailers selling through the legal process",
    "Social profiles and channels",
    "Documentation, code repositories, news and press",
];

/// What is removed (the rubric's flags, in the page's words).
pub const REMOVED: [&str; 10] = [
    "Drugs, and prescription medicine without a prescription",
    "Illegal weapons and sales that skip a licensed dealer",
    "Counterfeit or stolen goods, accounts and data",
    "Piracy, cracks, cheats and exploits",
    "Hacking for hire and malware",
    "Scams, get-rich-quick and \"investment\" schemes",
    "Gambling and adult content",
    "Pages that ask for a password, seed phrase or card outside their own checkout",
    "Impersonation, hate and private information",
    "Cloaked redirects, and pages that are parked or empty",
];

#[component]
pub fn QualityGatePage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />

            <div class="legal-container">
                <div class="legal-header">
                    <h1>"The Quality Gate."</h1>
                    <p class="legal-updated">"Nothing reaches the Gallery, or a Space's public page, without passing review first. The gate reads what you publish, decides by the Guardian Code and nothing else, and tells you why."</p>
                </div>

                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"Part one: the publication"</h2>
                        <p>"Every publish is reviewed before it is listed. Eustress lists a Space when it does something for real life: it teaches a skill that transfers, models a real place or system, tests an idea about something real, or moves a real problem toward a solution. Fiction is listed too, when it is labelled as fiction and carries an idea worth your time."</p>
                        <p>"How the review works:"</p>
                        <ul>
                            <li>"It reads the whole package: every script in full, the supporting files, and captures of the Space running. No Space is approved until every script has been read."</li>
                            <li>"It decides by the Guardian Code alone. Not popularity, not budget, not the author's reputation, not a reviewer's taste."</li>
                            <li>"It never waits on nobody. A person steps in only for the legal lane and for appeals."</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"What you get back"</h2>
                        <ul>
                            <li>"Listed: the Space is in the Gallery."</li>
                            <li>"Changes requested: each reason, cited to the Code, and the statement the review drafted for you from what it read. Fix and publish again."</li>
                            <li>"Not listed: the reasons, and the right to appeal."</li>
                        </ul>
                        <p>"A Space that is not listed is not a sanction. It still exists and it is still yours. It is only not listed."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Part two: the page"</h2>
                        <p><span class="g-chip warn">"Rolling out"</span></p>
                        <p>"Creators theme their Space's public page from the Website service in Studio: colours, a font, a logo, a banner, gallery images, and links to a website, social channels, shops and papers. The same gate reviews all of it before anyone sees it. Nothing is shown while it waits: a link or an image is hidden until it passes."</p>
                        <p>"How a link is reviewed:"</p>
                        <ul>
                            <li>"A sandboxed scanner visits the page. Eustress's own servers never open it."</li>
                            <li>"Anything malicious or phishing is removed on the spot, and so is a link that lands on a different site than the one it names."</li>
                            <li>"The page's text is judged against the rubric below. Text on the page that tries to instruct the judge counts against the page."</li>
                            <li>{format!("A link shows only with a risk score of {MAX_LINK_RISK} or less out of 100 and no flag. Nothing risky is allowed.")}</li>
                            <li>{format!("Every approved link is reviewed again every {LINK_REREVIEW_DAYS} days and on every publish. A link that turns risky is hidden at once.")}</li>
                        </ul>
                        <p>"A removed link is cut, the rest of the listing is still listed, and the author is told which link and why. A link inside the description is replaced with \"[link removed]\" before the description is shown."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Free market, no black market"</h2>
                        <p>"Allowed:"</p>
                        <ul>{ALLOWED.iter().map(|line| view! { <li>{*line}</li> }).collect_view()}</ul>
                        <p>"Removed:"</p>
                        <ul>{REMOVED.iter().map(|line| view! { <li>{*line}</li> }).collect_view()}</ul>
                        <p>"A listing made for children carries no links at all. An Adults-only listing carries no shop links."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Your theme is data, never code"</h2>
                        <p>"Colours are checked values, fonts and layouts come from a fixed list, and images are checked files. No author HTML, CSS or script ever reaches the page, and every outbound link opens in a new tab with its domain shown beside its label."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Questions about a review"</h2>
                        <p>"Your review, its reasons and your appeal are in Projects. For anything else, Support has the right channel."</p>
                        <a href="/projects" class="btn-primary-steel">"Open Projects"<span class="btn-icon">"→"</span></a>
                        " "
                        <a href="/support" class="btn-secondary-steel">"Support"</a>
                    </section>
                </div>
            </div>

            <Footer />
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_numbers_are_the_link_reviews() {
        // SIM_PAGE_THEME_AND_LINKS.md: approved at a risk score of 25 or less, re-reviewed every 7 days.
        assert_eq!((MAX_LINK_RISK, LINK_REREVIEW_DAYS), (25, 7));
    }

    #[test]
    fn the_page_review_is_marked_as_rolling_out_until_it_is_live() {
        // The Worker's link review (src/link_review.mjs) is not built yet. When it ships, drop the chip and this test together.
        let src = include_str!("quality_gate.rs");
        assert!(src.contains("\"Rolling out\""));
    }
}
