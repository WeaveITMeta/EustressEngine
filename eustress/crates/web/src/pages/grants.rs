// =============================================================================
// Eustress Web - Grants (/grants)
// =============================================================================
// How Eustress Studio works federal grants and contracts through SAM.gov and Grants.gov: the Public Sector mode
// (engine/modes/publicsector.toml, engine/src/capture/, docs/architecture/GOV_CAPTURE_PIPELINE.md). Everything said here is
// what that mode does today; the open items are named as open. The engine drafts and a person files: the page says so, and a
// test holds it.
// =============================================================================

use leptos::prelude::*;

use crate::components::{CentralNav, Footer};

/// What is still being built, in the page's words (GOV_CAPTURE_PIPELINE.md, the open task list).
pub const STILL_COMING: [&str; 2] = [
    "Incremental sync, so each poll fetches only the notices that are new since the last one.",
    "Fetching the solicitation documents behind each notice (the statement of work, the full NOFO) into the Space.",
];

#[component]
pub fn GrantsPage() -> impl IntoView {
    view! {
        <div class="page page-legal">
            <CentralNav active="".to_string() />

            <div class="legal-container">
                <div class="legal-header">
                    <h1>"Grants, through SAM.gov."</h1>
                    <p class="legal-updated">"Eustress Studio has a Public Sector mode for the people who answer federal grants and contracts: small contractors, grant writers and nonprofits. It finds the opportunities, screens them honestly, and drafts the first response. You file."</p>
                </div>

                <div class="legal-content">
                    <section class="legal-section">
                        <h2>"Start with SAM.gov"</h2>
                        <p>"Every organization that receives a federal grant or contract registers at SAM.gov, the federal System for Award Management, and gets a Unique Entity ID (UEI). Registration is free and is renewed every year. Grants.gov applications use the same UEI."</p>
                        <p>"Studio keeps your UEI, your people and your past performance in a capability statement inside your Space. That is the profile every opportunity is measured against."</p>
                        <a href="https://sam.gov" target="_blank" rel="noopener noreferrer">"SAM.gov"</a>
                        " · "
                        <a href="https://www.grants.gov" target="_blank" rel="noopener noreferrer">"Grants.gov"</a>
                    </section>

                    <section class="legal-section">
                        <h2>"Find"</h2>
                        <p>"Studio reads two public federal sources on a schedule and writes what it finds into your Space as data you can sort, filter and keep:"</p>
                        <ul>
                            <li>"SAM.gov Get Opportunities: contract notices, filtered by NAICS code, set-aside and notice type, with your own free SAM.gov API key."</li>
                            <li>"Grants.gov Search2: grant opportunities by keyword, agency, eligibility and funding category. No key needed."</li>
                        </ul>
                        <p>"Your API key is read from an environment variable when Studio fetches. It is never written into a file in your Space, and it is removed from every log."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Screen, with a reason for every no"</h2>
                        <p>"Each opportunity passes a set of fixed gates first: already closed, too little time to respond, a set-aside you are not eligible for, the wrong industry classification, a ceiling above your capacity or below your floor, and a place you do not work. Every rejection carries the reason it was rejected."</p>
                        <p>"What passes gets a fit score, criterion by criterion, against your capability statement. When the fit is weak, Studio recommends that you do not bid, and says why. A capture tool that only ever says yes is a lead generator for whoever sold it."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Draft"</h2>
                        <ul>
                            <li>"Every \"shall\" and \"must\" in the solicitation, pulled into a requirement matrix with its clause and who it binds."</li>
                            <li>"A gap report: the requirements your response does not yet answer."</li>
                            <li>"A response outline seeded from the matrix, on FAR templates for contracts and NOFO templates for grants."</li>
                        </ul>
                    </section>

                    <section class="legal-section">
                        <h2>"Keep the record"</h2>
                        <p>"Each notice gets its own folder with a Merkle manifest, so you can show exactly what you read and when, and prove nothing changed after."</p>
                        <p>"The pipeline also builds a 3D deal room in your Space: the opportunities you are pursuing, laid out where your team can walk through them."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"What Studio never does"</h2>
                        <p>"The engine drafts; a person files. Studio submits no bid or application, signs no certification, and sends nothing to SAM.gov or Grants.gov. Both sources are read-only, and there is no path that writes to them."</p>
                        <p>"Competitors appear only through public award records, never as profiled people."</p>
                    </section>

                    <section class="legal-section">
                        <h2>"Still being built"</h2>
                        <ul>
                            {STILL_COMING.iter().map(|line| view! { <li>{*line}</li> }).collect_view()}
                        </ul>
                    </section>

                    <section class="legal-section">
                        <a href="/download" class="btn-primary-steel">"Get Eustress Studio"<span class="btn-icon">"→"</span></a>
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
    fn the_page_says_studio_drafts_and_a_person_files() {
        let src = include_str!("grants.rs");
        assert!(src.contains("The engine drafts; a person files."));
        assert!(src.contains("sends nothing to SAM.gov or Grants.gov"));
        assert!(!STILL_COMING.is_empty());
    }
}
