// =============================================================================
// Eustress Web - Legal act pages (/legal/:act)
// =============================================================================
//
// One page per law Eustress answers to: CCPA, COPPA, CSAM, DMCA, GDPR and the
// TAKE IT DOWN Act. Every page uses the same layout (the Gallery look): what the
// law is in plain words, what it means on Eustress, your rights and how to use
// them, how to file, the response times we commit to, and the date of the text.
//
// The rule for every sentence here: it states what Eustress does today. Where
// something is not built yet, the page says so in the same place a reader would
// look for it. The tests at the end hold the lines the release audit found
// broken elsewhere (no NCMEC reporting claim, no parental-consent flow, no DMCA
// registration number, no second takedown form).
//
// These pages are unlisted (no sitemap entry, `noindex`) until McKale approves
// the legal text; the page says it is a draft until then.

use leptos::prelude::*;
use crate::components::{CentralNav, Footer};

/// The date of the text on every act page.
pub const UPDATED: &str = "October 8, 2026";

/// True while the legal text waits for McKale's review (and counsel's). Every
/// page shows a "Draft for legal review" chip while it is true.
pub const DRAFT: bool = true;

/// Where a reader acts on a right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contact {
    /// An email address, with the subject line we ask for.
    Mail { address: &'static str, subject: &'static str },
    /// A page on this site, or an outside page.
    Page { href: &'static str, label: &'static str },
    /// Nothing to send: the right is automatic or there is nothing to ask for.
    Nothing,
}

impl Contact {
    /// The link's address. Spaces in a subject become %20 so mail apps keep the whole line.
    pub fn href(&self) -> Option<String> {
        match self {
            Contact::Mail { address, subject } => Some(format!("mailto:{address}?subject={}", subject.replace(' ', "%20"))),
            Contact::Page { href, .. } => Some((*href).to_string()),
            Contact::Nothing => None,
        }
    }

    /// The words on the link.
    pub fn label(&self) -> Option<String> {
        match self {
            Contact::Mail { address, .. } => Some(format!("Email {address}")),
            Contact::Page { label, .. } => Some((*label).to_string()),
            Contact::Nothing => None,
        }
    }

    fn external(&self) -> bool {
        matches!(self, Contact::Page { href, .. } if href.starts_with("http"))
    }
}

/// How a right can be used today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// Built in: on the site or in Studio.
    SelfService,
    /// A person at Eustress carries it out when you write to us.
    ByRequest,
    /// Not built yet. The card says what to do instead.
    NotYet,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Availability::SelfService => "Available",
            Availability::ByRequest => "By request",
            Availability::NotYet => "Not yet available",
        }
    }

    fn chip_class(self) -> &'static str {
        match self {
            Availability::SelfService => "g-chip live",
            Availability::ByRequest => "g-chip",
            Availability::NotYet => "g-chip warn",
        }
    }
}

/// One right a reader has under the law, and how to use it on Eustress.
#[derive(Clone, Copy, Debug)]
pub struct Right {
    pub name: &'static str,
    pub what: &'static str,
    pub how: &'static str,
    pub availability: Availability,
    pub contact: Contact,
}

/// One row of "what we hold".
#[derive(Clone, Copy, Debug)]
pub struct Held {
    pub what: &'static str,
    pub detail: &'static str,
    pub kept: &'static str,
}

/// Everything one act page says.
#[derive(Clone, Copy, Debug)]
pub struct Act {
    /// The route segment: /legal/{slug}.
    pub slug: &'static str,
    /// The short name on the card and in the kicker.
    pub short: &'static str,
    /// The law's full name.
    pub name: &'static str,
    /// The one line on the /acts card.
    pub card: &'static str,
    /// Who the law protects.
    pub applies: &'static str,
    /// 01: the law in plain words.
    pub law: &'static [&'static str],
    /// 02: what it means on Eustress.
    pub on_eustress: &'static [&'static str],
    /// 02, continued: the data table, for the privacy laws.
    pub held: &'static [Held],
    /// 03: rights and how to use them.
    pub rights: &'static [Right],
    /// 04: how to file a request or report, in order.
    pub file: &'static [&'static str],
    /// 04: where the request goes.
    pub file_contact: Contact,
    /// 05: the response times we commit to.
    pub response: &'static [(&'static str, &'static str)],
}

const PRIVACY: &str = "privacy@eustress.dev";
const SAFETY: &str = "safety@eustress.dev";
const DMCA: &str = "dmca@eustress.dev";

/// What Eustress holds about an account, for the CCPA and GDPR pages.
pub const DATA_HELD: &[Held] = &[
    Held {
        what: "Account",
        detail: "Your username, the public half of your identity key (the identity file itself stays with you), your date of birth, your email address if you gave one, your time zone, and who you follow and favorite.",
        kept: "While the account exists.",
    },
    Held {
        what: "Identity verification",
        detail: "The images of the government ID you uploaded at signup, the date of birth read from it, the result of the check, a one-way fingerprint of the ID number that stops one ID from opening two accounts, and the result of the background screening run at signup.",
        kept: "Up to 7 years.",
    },
    Held {
        what: "Purchases and payouts",
        detail: "A receipt for every purchase and every payout. Stripe processes the payment; Eustress never sees or stores your card number.",
        kept: "As long as tax and accounting records require.",
    },
    Held {
        what: "Published Spaces",
        detail: "The Spaces you publish, the captures and summary Studio sends with them for review, and the review cases about them.",
        kept: "While the listing exists. A case under a legal hold is kept until a person releases it.",
    },
    Held {
        what: "Studio usage counts",
        detail: "Which Studio tools are clicked, counted per session under a random install ID that is never joined to your account. On by default; turn it off in Studio under Settings, Notifications, Privacy.",
        kept: "As totals per tool.",
    },
    Held {
        what: "Website visits",
        detail: "Daily totals of page views and visitors. A visitor is counted once a day by a one-way hash of the network address, the browser and the date.",
        kept: "The hash for 2 days; the daily totals for about 13 months.",
    },
];

const PROCESSORS: &str = "Services that handle this data for us: Cloudflare (hosting, storage and email), Stripe (payments and payouts), xAI (reads uploaded ID documents to verify them, runs the signup screening, and helps review published Spaces) and TypeSafe (classifies the text summary of a published Space). Eustress does not sell personal information and does not share it for advertising. The site carries no advertising trackers.";

pub const CCPA: Act = Act {
    slug: "ccpa",
    short: "CCPA",
    name: "California Consumer Privacy Act",
    card: "What we hold about California residents, and how to see it, correct it or have it deleted.",
    applies: "California residents",
    law: &[
        "The California Consumer Privacy Act, as amended by the California Privacy Rights Act, gives people who live in California rights over the personal information a business collects about them.",
        "You can ask what a business holds about you and ask for a copy, ask it to delete or correct that information, tell it not to sell or share it, and limit how it uses sensitive information such as a government ID. A business may not treat you worse for using these rights.",
        "Eustress honors these rights for every California resident, whether or not the law's size thresholds apply to us.",
    ],
    on_eustress: &[
        "This is everything Eustress holds about an account, and how long we keep it.",
        PROCESSORS,
    ],
    held: DATA_HELD,
    rights: &[
        Right {
            name: "Know and get a copy",
            what: "Learn what we hold about you, where it came from and who handles it, and get a copy.",
            how: "Signed in, Purchases shows every receipt and Settings shows your email and time zone. For a copy of everything in the table above, write to us from the email on your account, or give your username. A download button is not built yet.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: PRIVACY, subject: "CCPA request: copy of my data" },
        },
        Right {
            name: "Delete",
            what: "Have your account and the information tied to it deleted.",
            how: "There is no delete button yet. An administrator deletes the account for you: the account record, username, sign-in key, email, time zone, follows and favorites, and every signed-in session. Not removed by this today: identity verification and screening records (kept up to 7 years), purchase records, and review cases.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: PRIVACY, subject: "CCPA request: delete my account" },
        },
        Right {
            name: "Correct",
            what: "Fix information about you that is wrong.",
            how: "Change your email address in Settings. For anything else, write to us and say what is wrong.",
            availability: Availability::SelfService,
            contact: Contact::Mail { address: PRIVACY, subject: "CCPA request: correct my data" },
        },
        Right {
            name: "Opt out of sale or sharing",
            what: "Stop a business from selling your information or sharing it for advertising.",
            how: "Eustress does not sell personal information or share it for advertising, so there is nothing to turn off.",
            availability: Availability::SelfService,
            contact: Contact::Nothing,
        },
        Right {
            name: "Limit sensitive information",
            what: "Limit how a business uses sensitive information, such as a government ID.",
            how: "Your ID and date of birth are used only to verify your identity and age, to screen signups, and to decide payouts and the Adult content setting. Nothing else reads them.",
            availability: Availability::SelfService,
            contact: Contact::Nothing,
        },
        Right {
            name: "Studio usage counts",
            what: "Stop the anonymous counts of which Studio tools you click.",
            how: "In Studio, open Settings, Notifications, Privacy and turn usage counts off. Counts not yet sent are dropped at once.",
            availability: Availability::SelfService,
            contact: Contact::Nothing,
        },
    ],
    file: &[
        "Write to privacy@eustress.dev from the email on your account. If you have no email on file, give your username.",
        "Say which right you are using: a copy, deletion or correction.",
        "We confirm the request comes from the account holder before we act, by replying to the email on the account. An authorized agent can file for you with your written permission.",
        "We tell you what we did, and what we could not do and why.",
    ],
    file_contact: Contact::Mail { address: PRIVACY, subject: "CCPA request" },
    response: &[
        ("Receipt", "Within 10 business days"),
        ("Answer", "Within 45 days of receipt"),
        ("Extension", "Once, by up to 45 more days, and we tell you why within the first 45"),
    ],
};

pub const COPPA: Act = Act {
    slug: "coppa",
    short: "COPPA",
    name: "Children's Online Privacy Protection Act",
    card: "Eustress is for adults. No one under 18 can open an account.",
    applies: "Children under 13 and their parents",
    law: &[
        "The Children's Online Privacy Protection Act is a US law that protects children under 13. A website or app aimed at children, or one that knowingly collects personal information from a child under 13, must first get a parent's verifiable consent, and must let parents see and delete what it collected.",
    ],
    on_eustress: &[
        "Eustress accounts are for adults only. Signing up on eustress.dev asks for your date of birth and a government ID, and the account is refused unless the ID shows you are at least 18, or the age of majority where you live if that is higher.",
        "Eustress is not aimed at children, and we do not knowingly collect personal information from anyone under 13. Because no one under 18 can open an account, Eustress has no parental consent process.",
        "If we learn that an account belongs to someone under 18, we close it and delete its data.",
    ],
    held: &[],
    rights: &[
        Right {
            name: "Report a child's account",
            what: "Tell us an account belongs to someone under 18.",
            how: "Send the username, or a link to the profile or a Space it published. We close the account and delete its data. Today an administrator deletes the account and its sign-in directly, and removes its identity verification records by hand in the same step.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: SAFETY, subject: "Underage account" },
        },
        Right {
            name: "Ask what we hold about your child",
            what: "As a parent or guardian, learn what we hold about your child and have it deleted.",
            how: "Write to us with the username. We confirm you are the parent or guardian before we share anything.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: PRIVACY, subject: "Parent request" },
        },
    ],
    file: &[
        "Write to safety@eustress.dev with the username, or a link to the profile or a Space.",
        "Say why you believe the account holder is under 18. You do not need proof to report it.",
        "We close the account while we check, and delete its data once the check is done.",
    ],
    file_contact: Contact::Mail { address: SAFETY, subject: "Underage account" },
    response: &[
        ("Account closed", "Within 2 business days of the report"),
        ("Data deleted", "Within 30 days of the report"),
    ],
};

pub const CSAM: Act = Act {
    slug: "csam",
    short: "CSAM Policy",
    name: "Child Sexual Abuse Material",
    card: "Zero tolerance. How flagged content is locked and preserved, and how to report it.",
    applies: "Everyone",
    law: &[
        "Child sexual abuse material (CSAM) is illegal to make, possess or share in the United States and nearly everywhere else. US law requires online services to report apparent CSAM to the National Center for Missing & Exploited Children (NCMEC), which works with law enforcement.",
    ],
    on_eustress: &[
        "Zero tolerance. Eustress bans any depiction, description, simulation or text involving sexual activity, exploitation, grooming or nudity of anyone under 18, whether real, fictional, drawn, generated or described.",
        "Every Space is reviewed by AI before the Gallery lists it. A Space flagged for this is never listed and goes to a person, whatever the confidence. Its files are locked, so they cannot be opened, played, downloaded or changed.",
        "The files are kept unchanged as evidence under a legal hold with no end date. Only a person can decide on the case. The account that published it is not told why.",
        "A report through the TAKE IT DOWN form that says the person shown is under 18 locks the Space at once and alerts our staff.",
        "A person decides what happens to the account, up to a permanent ban.",
        "Eustress's registration to file reports with NCMEC's CyberTipline is in progress. Until it is complete, the evidence is preserved as described here and a person on our staff handles every case. You can report to NCMEC directly at any time.",
    ],
    held: &[],
    rights: &[
        Right {
            name: "Report it on Eustress",
            what: "Have a Space locked and reviewed.",
            how: "Use the TAKE IT DOWN form and check that the person shown is under 18; the Space is locked the moment a valid request arrives. Or email us the link. Never send us copies of the material: send the link or the Space's id.",
            availability: Availability::SelfService,
            contact: Contact::Page { href: "/take-it-down", label: "Open the request form" },
        },
        Right {
            name: "Report it by email",
            what: "Reach a person on our staff.",
            how: "Send the link or the Space's id, and what you saw. Never attach the material.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: SAFETY, subject: "Child safety report" },
        },
        Right {
            name: "Report it to NCMEC",
            what: "Report directly to the National Center for Missing & Exploited Children.",
            how: "Use the CyberTipline online, or call 1-800-843-5678. If a child is in danger now, call 911 or your local police first.",
            availability: Availability::SelfService,
            contact: Contact::Page { href: "https://report.cybertip.org", label: "Open the CyberTipline" },
        },
    ],
    file: &[
        "Find the link to the Space, or its id. Do not download, copy or screenshot the material.",
        "Use the TAKE IT DOWN form and check that the person shown is under 18, or email safety@eustress.dev with the link.",
        "If a child is in danger now, call 911 or your local police first.",
    ],
    file_contact: Contact::Page { href: "/take-it-down", label: "Open the request form" },
    response: &[
        ("Through the form", "The Space is locked automatically when a valid request arrives"),
        ("By email", "A person locks the Space within 24 hours"),
        ("Evidence", "Kept under a legal hold with no end date"),
    ],
};

pub const DMCA_ACT: Act = Act {
    slug: "dmca",
    short: "DMCA",
    name: "Digital Millennium Copyright Act",
    card: "How to send a copyright notice or counter-notice, and what we do with it.",
    applies: "Copyright owners and the people who publish on Eustress",
    law: &[
        "The Digital Millennium Copyright Act is a US law. It lets a copyright owner ask an online service to take down material that infringes their work, and lets the person who posted it answer with a counter-notice.",
        "A service that follows this process, and names an agent with the US Copyright Office, is not liable for what its users post.",
    ],
    on_eustress: &[
        "Eustress's designated agent registration with the US Copyright Office is pending. Until it is listed, send notices to dmca@eustress.dev.",
        "When a notice is complete, an administrator takes the Space or asset out of the Gallery and tells its author, with a copy of the notice.",
        "An account with repeated valid notices against it can be suspended or closed by an administrator.",
        "The full copyright policy, with what a notice and a counter-notice must contain, is on the DMCA page.",
    ],
    held: &[],
    rights: &[
        Right {
            name: "Send a notice",
            what: "As a copyright owner, or someone authorized to act for one, ask us to take down infringing material.",
            how: "Include your signature (a typed full name is enough), the work you own, the link to the material on Eustress, your contact details, a statement of good faith, and a statement under penalty of perjury that the notice is accurate and you are authorized.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: DMCA, subject: "DMCA Takedown Request" },
        },
        Right {
            name: "Send a counter-notice",
            what: "As the author, answer a notice you believe is a mistake or a misidentification.",
            how: "Include your signature, what was removed and where it was, a statement under penalty of perjury that it was removed by mistake, your name, address and phone number, and your consent to the jurisdiction of a US federal court.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: DMCA, subject: "DMCA Counter-Notice" },
        },
        Right {
            name: "Read the full policy",
            what: "The whole copyright and trademark policy.",
            how: "Every element a notice and a counter-notice must have, repeat infringers, and trademarks.",
            availability: Availability::SelfService,
            contact: Contact::Page { href: "/dmca", label: "Open the DMCA page" },
        },
    ],
    file: &[
        "Email dmca@eustress.dev with the subject \"DMCA Takedown Request\".",
        "Include every element listed under Send a notice. An incomplete notice gets a reply that says what is missing.",
        "We take the material down and send the author a copy of your notice.",
        "If the author sends a valid counter-notice, we forward it to you and restore the material in 10 to 14 business days, unless you tell us you have filed a court action.",
    ],
    file_contact: Contact::Mail { address: DMCA, subject: "DMCA Takedown Request" },
    response: &[
        ("Receipt", "Within 2 business days"),
        ("Takedown", "Within 2 business days of a complete notice"),
        ("Restore after a counter-notice", "10 to 14 business days, unless a court action is filed"),
    ],
};

pub const GDPR: Act = Act {
    slug: "gdpr",
    short: "GDPR",
    name: "General Data Protection Regulation",
    card: "Your rights in the EU and UK: access, correction, erasure, portability and objection.",
    applies: "People in the European Union and the United Kingdom",
    law: &[
        "The General Data Protection Regulation, and the UK's version of it, protect personal data about people in the EU and the UK. A service must have a lawful reason for each use of your data, tell you what it holds, and let you see, correct, erase, move and object to it.",
        "You can also complain to the data protection authority where you live.",
    ],
    on_eustress: &[
        "This is everything Eustress holds about an account, and how long we keep it.",
        "Why we use it: to run your account and what you buy (the contract with you); to verify identity and age and screen signups (our legal duties around payouts, and keeping the platform safe); and Studio usage counts and website visits (improving Eustress, with an off switch for Studio).",
        "Signup runs an automated background screening. If it refuses an account, a person reviews the refusal when you ask.",
        "Cloudflare stores and serves data from its network around the world, so your data may be processed outside the EU and the UK.",
        PROCESSORS,
    ],
    held: DATA_HELD,
    rights: &[
        Right {
            name: "Access",
            what: "Learn what we hold about you and get a copy.",
            how: "Signed in, Purchases shows every receipt and Settings shows your email and time zone. For a copy of everything in the table above, write to us. A download button is not built yet.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: PRIVACY, subject: "GDPR request: access" },
        },
        Right {
            name: "Rectification",
            what: "Fix information about you that is wrong.",
            how: "Change your email address in Settings. For anything else, write to us and say what is wrong.",
            availability: Availability::SelfService,
            contact: Contact::Mail { address: PRIVACY, subject: "GDPR request: rectification" },
        },
        Right {
            name: "Erasure",
            what: "Have your data deleted.",
            how: "There is no delete button yet. An administrator deletes the account for you: the account record, username, sign-in key, email, time zone, follows and favorites, and every signed-in session. Not removed by this today: identity verification and screening records (kept up to 7 years), purchase records, and review cases. We tell you which records we keep and why.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: PRIVACY, subject: "GDPR request: erasure" },
        },
        Right {
            name: "Portability",
            what: "Get your data in a file another service can read.",
            how: "Write to us and we send a machine-readable copy. A self-service export is not built yet.",
            availability: Availability::NotYet,
            contact: Contact::Mail { address: PRIVACY, subject: "GDPR request: portability" },
        },
        Right {
            name: "Object or restrict",
            what: "Stop or pause a use of your data.",
            how: "Studio usage counts: turn them off in Studio under Settings, Notifications, Privacy. Anything else: write to us and say which use.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: PRIVACY, subject: "GDPR request: objection" },
        },
        Right {
            name: "A person reviews an automated decision",
            what: "Have a person look at a decision a machine made about you.",
            how: "A signup refused by the screening: write to support@eustress.dev, the address the refusal names. A Space the review did not list: ask for a review from the listing's case.",
            availability: Availability::ByRequest,
            contact: Contact::Mail { address: "support@eustress.dev", subject: "Screening review" },
        },
    ],
    file: &[
        "Write to privacy@eustress.dev from the email on your account. If you have no email on file, give your username.",
        "Say which right you are using.",
        "We confirm the request comes from the account holder before we act, by replying to the email on the account.",
        "We tell you what we did. If we keep any record, we tell you which one and why.",
    ],
    file_contact: Contact::Mail { address: PRIVACY, subject: "GDPR request" },
    response: &[
        ("Answer", "Within one month of receipt"),
        ("Extension", "By up to two more months for a complex request, and we tell you why within the first month"),
    ],
};

pub const TAKE_IT_DOWN: Act = Act {
    slug: "take-it-down",
    short: "TAKE IT DOWN",
    name: "TAKE IT DOWN Act",
    card: "Remove an intimate image or video shared without consent, real or made by AI.",
    applies: "Anyone shown in an intimate image shared without their consent",
    law: &[
        "The TAKE IT DOWN Act is a US law signed in 2025. It makes it a federal crime to publish an intimate image or video of a person without their consent, including one made or altered by AI.",
        "Since May 19, 2026, platforms must give people a clear way to ask for removal, remove the content within 48 hours of a valid request, and make reasonable efforts to remove identical copies. The Federal Trade Commission enforces it.",
    ],
    on_eustress: &[
        "A valid request removes the content at once, automatically, with no person in the loop. Every Space it names stops being listed, opened, played, downloaded or shown to anyone but an administrator.",
        "Nothing is deleted at that moment: the files are kept unchanged as evidence, under a legal hold.",
        "Exact copies are blocked: the fingerprint of every file in the Space goes on a blocklist, uploads of the same bytes are refused, and other Spaces holding them are locked. A copy that was re-encoded, resized or cropped is not matched automatically; send its link too.",
        "The author is told, with the case reference and how to ask a person to review it. The author is never told who asked.",
        "Only a person's decision restores the content.",
        "If the person shown is under 18, the request is handled as child sexual abuse material: see the CSAM policy.",
    ],
    held: &[],
    rights: &[
        Right {
            name: "Ask for removal",
            what: "As the person shown, or someone authorized to act for them, have the content removed.",
            how: "Use the request form. No account is needed.",
            availability: Availability::SelfService,
            contact: Contact::Page { href: "/take-it-down", label: "Open the request form" },
        },
        Right {
            name: "Follow your case",
            what: "See where your request stands.",
            how: "The form gives you a case reference shaped like EC-XXXX-XXXX-XXXX-XXXX-XXXX and a link to its status page.",
            availability: Availability::SelfService,
            contact: Contact::Nothing,
        },
        Right {
            name: "Ask a person to review a removal",
            what: "As the author of a removed Space, ask a person to look at it.",
            how: "Signed in, open the case from your account's case history and choose Ask for a review.",
            availability: Availability::SelfService,
            contact: Contact::Page { href: "/account/cases", label: "Open your cases" },
        },
    ],
    file: &[
        "Open the request form. No account is needed.",
        "Give your name and type it again as your signature, and say whether you are the person shown or are acting for them (with their name).",
        "Give an email where we can reach you, the links or ids of the Spaces (up to 20), and a short description.",
        "Confirm in good faith that the content was shared without consent, and send.",
        "Keep the case reference the page shows you.",
    ],
    file_contact: Contact::Page { href: "/take-it-down", label: "Open the request form" },
    response: &[
        ("Removal", "Immediate and automatic on a valid request"),
        ("The law's limit", "48 hours"),
        ("Restoration", "Only by a person's decision"),
    ],
};

/// Every act page, in the order the cards show them.
pub const ACTS: &[Act] = &[CCPA, COPPA, CSAM, DMCA_ACT, GDPR, TAKE_IT_DOWN];

/// The act for a route segment.
pub fn act_for(slug: &str) -> Option<&'static Act> {
    ACTS.iter().find(|a| a.slug == slug)
}

/// The page's own route.
pub fn route_of(act: &Act) -> String {
    format!("/legal/{}", act.slug)
}

/// /legal/:act
#[component]
pub fn LegalActPage() -> impl IntoView {
    let params = leptos_router::hooks::use_params_map();
    let slug = move || params.read().get("act").unwrap_or_default();

    // Unlisted until McKale approves the text: tell crawlers that run scripts, as `_headers` tells those that do not.
    #[cfg(not(feature = "ssr"))]
    {
        let held = StoredValue::new_local(None::<web_sys::Element>);
        Effect::new(move |_| {
            let doc = document();
            if let Ok(meta) = doc.create_element("meta") {
                let _ = meta.set_attribute("name", "robots");
                let _ = meta.set_attribute("content", "noindex, nofollow");
                if let Ok(Some(head)) = doc.query_selector("head") {
                    let _ = head.append_child(&meta);
                    held.set_value(Some(meta));
                }
            }
        });
        on_cleanup(move || {
            held.update_value(|slot| {
                if let Some(meta) = slot.take() {
                    meta.remove();
                }
            });
        });
    }

    view! {
        <div class="page g-page lg-page">
            <CentralNav active="".to_string() />
            <div class="g-backdrop" aria-hidden="true"></div>
            <main class="g-main">
                {move || match act_for(&slug()) {
                    Some(act) => view! { <LegalActBody act=*act /> }.into_any(),
                    None => view! { <LegalActMissing /> }.into_any(),
                }}
            </main>
            <Footer />
        </div>
    }
}

/// Everything an act page says, without the site's frame (so tests can render it alone).
#[component]
pub fn LegalActBody(act: Act) -> impl IntoView {
    let response_first = act.response.first().map(|(_, v)| *v).unwrap_or("");
    view! {
        <div class="g-wrap lg-wrap">
            <nav class="g-crumbs" aria-label="Breadcrumb">
                <a href="/acts">"Legal"</a>
                <span aria-hidden="true">"/"</span>
                <span class="g-crumb-now" aria-current="page">{act.short}</span>
            </nav>

            <header class="lg-head g-rise">
                <div class="g-kicker"><span class="g-kick-rule"></span>{format!("Legal / {}", act.short)}</div>
                <h1 class="lg-title">{act.name}</h1>
                <p class="lg-lede">{act.card}</p>
                <div class="g-chips">
                    <span class="g-chip">{format!("Protects: {}", act.applies)}</span>
                    {DRAFT.then(|| view! { <span class="g-chip warn">"Draft for legal review"</span> })}
                </div>
            </header>

            <div class="lg-grid">
                <article class="lg-body">
                    <LegalSection n="01" title="The law in plain words">
                        {act.law.iter().map(|p| view! { <p>{*p}</p> }).collect_view()}
                    </LegalSection>

                    <LegalSection n="02" title="What it means on Eustress">
                        {act.on_eustress.iter().map(|p| view! { <p>{*p}</p> }).collect_view()}
                        {(!act.held.is_empty()).then(|| view! {
                            <div class="lg-table g-glass" role="table" aria-label="What Eustress holds">
                                <div class="lg-tr lg-th" role="row">
                                    <span role="columnheader">"What"</span>
                                    <span role="columnheader">"What it contains"</span>
                                    <span role="columnheader">"How long we keep it"</span>
                                </div>
                                {act.held.iter().map(|h| view! {
                                    <div class="lg-tr" role="row">
                                        <span role="rowheader" class="lg-tk">{h.what}</span>
                                        <span role="cell">{h.detail}</span>
                                        <span role="cell" class="lg-kept">{h.kept}</span>
                                    </div>
                                }).collect_view()}
                            </div>
                        })}
                    </LegalSection>

                    <LegalSection n="03" title="Your rights and how to use them">
                        <div class="lg-rights">
                            {act.rights.iter().map(|r| view! { <RightCard right=*r /> }).collect_view()}
                        </div>
                    </LegalSection>

                    <LegalSection n="04" title="How to file a request or report">
                        <ol class="lg-steps">
                            {act.file.iter().map(|s| view! { <li>{*s}</li> }).collect_view()}
                        </ol>
                        <div class="g-actions"><ContactLink contact=act.file_contact primary=true /></div>
                    </LegalSection>

                    <LegalSection n="05" title="The response times we commit to">
                        <dl class="g-kv g-glass lg-kv">
                            {act.response.iter().map(|(k, v)| view! { <dt>{*k}</dt><dd>{*v}</dd> }).collect_view()}
                        </dl>
                    </LegalSection>

                    <p class="lg-note">
                        {format!("Last updated {UPDATED}. This page states what Eustress does today; where something is not built yet, it says so. It is not legal advice.")}
                    </p>
                </article>

                <aside class="lg-aside">
                    <div class="g-glass lg-glance">
                        <h2>"At a glance"</h2>
                        <dl class="lg-glance-kv">
                            <dt>"File with"</dt>
                            <dd><ContactLink contact=act.file_contact primary=false /></dd>
                            <dt>"First response"</dt>
                            <dd>{response_first}</dd>
                            <dt>"Updated"</dt>
                            <dd class="g-mono">{UPDATED}</dd>
                        </dl>
                    </div>
                    <nav class="g-glass lg-acts" aria-label="Legal acts">
                        <h2>"Acts"</h2>
                        {ACTS.iter().map(|a| {
                            let here = a.slug == act.slug;
                            view! {
                                <a href=route_of(a) class:on=here aria-current=here.then_some("page")>
                                    <span class="g-mono">{a.short}</span>
                                    <span class="lg-acts-name">{a.name}</span>
                                </a>
                            }
                        }).collect_view()}
                    </nav>
                </aside>
            </div>
        </div>
    }
}

/// One numbered section: "01", a title, a rule, then the content.
#[component]
fn LegalSection(n: &'static str, title: &'static str, children: Children) -> impl IntoView {
    // Two strings: the view moves each attribute value, in an order of its own.
    let id = format!("s{n}");
    let labelled_by = id.clone();
    view! {
        <section class="lg-sec" aria-labelledby=labelled_by>
            <div class="lg-sec-head">
                <span class="lg-num g-mono" aria-hidden="true">{n}</span>
                <h2 id=id>{title}</h2>
                <span class="g-rule"></span>
            </div>
            {children()}
        </section>
    }
}

#[component]
fn RightCard(right: Right) -> impl IntoView {
    view! {
        <div class="g-glass lg-right">
            <span class=right.availability.chip_class()>{right.availability.label()}</span>
            <h3>{right.name}</h3>
            <p class="lg-what">{right.what}</p>
            <p class="lg-how">{right.how}</p>
            <ContactLink contact=right.contact primary=false />
        </div>
    }
}

#[component]
fn ContactLink(contact: Contact, primary: bool) -> impl IntoView {
    let external = contact.external();
    match (contact.href(), contact.label()) {
        (Some(href), Some(label)) => view! {
            <a
                class=if primary { "g-btn g-btn-primary" } else { "lg-link" }
                href=href
                target=external.then_some("_blank")
                rel=external.then_some("noopener noreferrer")
            >
                {label}
            </a>
        }
        .into_any(),
        _ => ().into_any(),
    }
}

#[component]
fn LegalActMissing() -> impl IntoView {
    view! {
        <div class="g-wrap">
            <div class="g-glass g-none" role="status">
                <h1 class="g-none-title">"We don't have a page for that act"</h1>
                <p>"Every act Eustress answers to is listed on the legal page."</p>
                <div class="g-actions"><a href="/acts" class="g-btn">"Go to Legal"</a></div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every string a reader sees on an act page.
    fn visible(act: &Act) -> Vec<&'static str> {
        let mut out = vec![act.short, act.name, act.card, act.applies];
        out.extend(act.law.iter().copied());
        out.extend(act.on_eustress.iter().copied());
        for h in act.held {
            out.extend([h.what, h.detail, h.kept]);
        }
        for r in act.rights {
            out.extend([r.name, r.what, r.how]);
        }
        out.extend(act.file.iter().copied());
        for (k, v) in act.response {
            out.extend([*k, *v]);
        }
        out
    }

    fn all_text(act: &Act) -> String {
        visible(act).join("\n").to_lowercase()
    }

    #[test]
    fn six_acts_with_their_routes() {
        let slugs: Vec<_> = ACTS.iter().map(|a| a.slug).collect();
        assert_eq!(slugs, ["ccpa", "coppa", "csam", "dmca", "gdpr", "take-it-down"]);
        for a in ACTS {
            assert_eq!(act_for(a.slug).map(|f| f.slug), Some(a.slug));
            assert_eq!(route_of(a), format!("/legal/{}", a.slug));
        }
        assert!(act_for("tida").is_none());
    }

    #[test]
    fn every_page_fills_the_shared_layout() {
        for a in ACTS {
            assert!(!a.law.is_empty(), "{}: the law", a.slug);
            assert!(!a.on_eustress.is_empty(), "{}: on Eustress", a.slug);
            assert!(!a.rights.is_empty(), "{}: rights", a.slug);
            assert!(!a.file.is_empty(), "{}: how to file", a.slug);
            assert!(!a.response.is_empty(), "{}: response times", a.slug);
            assert!(a.file_contact.href().is_some(), "{}: where to file", a.slug);
            for s in visible(a) {
                assert!(!s.trim().is_empty(), "{}: an empty string", a.slug);
            }
        }
    }

    #[test]
    fn no_dashes_a_reader_would_see_as_em_dashes() {
        for a in ACTS {
            for s in visible(a) {
                assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'), "{}: {s}", a.slug);
            }
        }
    }

    #[test]
    fn the_privacy_pages_list_what_we_hold() {
        for a in [&CCPA, &GDPR] {
            assert_eq!(a.held.len(), DATA_HELD.len(), "{}", a.slug);
            let t = all_text(a);
            for word in ["account", "identity verification", "purchases", "studio usage", "website visits"] {
                assert!(t.contains(word), "{}: {word}", a.slug);
            }
        }
    }

    // The release audit's contradictions, held so no later edit brings them back.

    #[test]
    fn coppa_states_adults_only_and_runs_no_consent_flow() {
        let t = all_text(&COPPA);
        assert!(t.contains("at least 18"));
        assert!(t.contains("no parental consent process"));
        // The law section describes what COPPA requires; nothing on Eustress's side may offer it.
        let ours: String = COPPA.on_eustress.iter().chain(COPPA.file.iter()).copied().collect::<Vec<_>>().join(" ").to_lowercase();
        for r in COPPA.rights {
            assert!(!format!("{} {}", r.what, r.how).to_lowercase().contains("consent"), "{}", r.name);
        }
        assert!(!ours.contains("verifiable"));
    }

    #[test]
    fn csam_never_claims_eustress_reports_to_ncmec() {
        let t = all_text(&CSAM);
        for claim in ["we report", "eustress reports", "reported to ncmec", "immediate reporting", "we notify ncmec", "reports are filed"] {
            assert!(!t.contains(claim), "{claim}");
        }
        assert!(t.contains("registration to file reports with ncmec's cybertipline is in progress"));
        assert!(t.contains("legal hold"));
    }

    #[test]
    fn dmca_names_no_registration_number() {
        let t = all_text(&DMCA_ACT);
        assert!(t.contains("registration with the us copyright office is pending"));
        assert!(t.contains(DMCA));
        for claim in ["registration number", "dmca-1", "registered agent"] {
            assert!(!t.contains(claim), "{claim}");
        }
    }

    #[test]
    fn take_it_down_links_to_the_one_form_and_builds_none() {
        assert_eq!(TAKE_IT_DOWN.file_contact.href().as_deref(), Some("/take-it-down"));
        assert!(TAKE_IT_DOWN.rights.iter().any(|r| r.contact.href().as_deref() == Some("/take-it-down")));
        let owner = Owner::new();
        let html = owner.with(|| view! { <LegalActBody act=TAKE_IT_DOWN /> }.to_html());
        assert!(!html.contains("<form"), "the request form lives on /take-it-down");
        assert!(!html.contains("<input"));
    }

    #[test]
    fn a_page_renders_every_section_and_its_contacts() {
        let owner = Owner::new();
        let html = owner.with(|| view! { <LegalActBody act=CCPA /> }.to_html());
        for heading in [
            "The law in plain words",
            "What it means on Eustress",
            "Your rights and how to use them",
            "How to file a request or report",
            "The response times we commit to",
        ] {
            assert!(html.contains(heading), "{heading}");
        }
        assert!(html.contains("mailto:privacy@eustress.dev?subject=CCPA%20request"));
        assert!(html.contains(UPDATED));
        assert!(html.contains("Draft for legal review") == DRAFT);
        // The side list links every act, and marks this one.
        for a in ACTS {
            assert!(html.contains(&format!("href=\"/legal/{}\"", a.slug)), "{}", a.slug);
        }
        assert!(html.contains("aria-current=\"page\""));
    }

    #[test]
    fn outside_links_open_in_a_new_tab() {
        let owner = Owner::new();
        let html = owner.with(|| view! { <LegalActBody act=CSAM /> }.to_html());
        assert!(html.contains("href=\"https://report.cybertip.org\""));
        assert!(html.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn unlisted_until_the_text_is_approved() {
        let sitemap = include_str!("../../sitemap.xml");
        let llms = include_str!("../../llms.txt");
        assert!(!sitemap.contains("/legal/"), "the act pages stay out of the sitemap until McKale approves the text");
        assert!(!llms.contains("/legal/"));
        let headers = include_str!("../../_headers");
        let at = headers.find("/legal/*").expect("_headers keeps /legal/* out of search");
        assert!(headers[at..].lines().take(3).any(|l| l.trim().eq_ignore_ascii_case("X-Robots-Tag: noindex, nofollow")));
    }
}
