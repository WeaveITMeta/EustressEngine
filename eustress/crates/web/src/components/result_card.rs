// =============================================================================
// Eustress Web - The compact card a link in an email lands on
// =============================================================================
// One card per state, centred in the height of the window, with an icon and a
// title that says what happened. Not a hero: the title is card-sized and appears
// once. Used by the pages /verify-email, /cancel-email-change,
// /undo-key-replacement and /replacement-key.
// =============================================================================

use leptos::prelude::*;

use crate::components::{CentralNav, Footer};

/// The colour of the icon: green for done, blue for information, amber for a
/// warning the reader must weigh, red for something that went wrong, grey for
/// the rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Ok,
    Info,
    Warn,
    Bad,
    Plain,
}

impl Tone {
    pub fn class(self) -> &'static str {
        match self {
            Tone::Ok => "result-icon ok",
            Tone::Info => "result-icon info",
            Tone::Warn => "result-icon warn",
            Tone::Bad => "result-icon bad",
            Tone::Plain => "result-icon",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Check,
    Clock,
    Shield,
    BrokenLink,
    Mail,
    Alert,
    Key,
    /// A turning ring, for "working on it".
    Busy,
}

fn glyph_svg(glyph: Glyph) -> AnyView {
    match glyph {
        Glyph::Check => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg> }.into_any(),
        Glyph::Clock => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg> }.into_any(),
        Glyph::Shield => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l7 3v5c0 4.5-3 8.2-7 10-4-1.8-7-5.5-7-10V6l7-3z"/><path d="M9.5 12l2 2 3.5-4"/></svg> }.into_any(),
        Glyph::BrokenLink => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/><path d="M4 4l16 16"/></svg> }.into_any(),
        Glyph::Mail => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2"/><path d="M3 7l9 6 9-6"/></svg> }.into_any(),
        Glyph::Alert => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3.5l9.5 16.5h-19L12 3.5z"/><path d="M12 10v4.5M12 17.5h.01"/></svg> }.into_any(),
        Glyph::Key => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="8" cy="12" r="3.5"/><path d="M11.5 12H21M18 12v3M15 12v2"/></svg> }.into_any(),
        Glyph::Busy => view! { <div class="result-spinner" aria-hidden="true"></div> }.into_any(),
    }
}

/// The page around a card: the site's navigation at the top, the footer at the
/// bottom, and the card in the middle of what is left.
#[component]
pub fn ResultPage(children: Children) -> impl IntoView {
    view! {
        <div class="page result-page">
            <CentralNav active="".to_string() />
            <main class="result-main">{children()}</main>
            <Footer />
        </div>
    }
}

/// One state of a page. Focus moves to the card when it appears, so a person using a
/// keyboard or a screen reader lands on the new title instead of on a button that
/// is gone; the card is also a polite live region.
#[component]
pub fn ResultCard(
    tone: Tone,
    glyph: Glyph,
    #[prop(into)] title: String,
    /// A wider card, left-aligned, for a page that holds a form.
    #[prop(optional)]
    wide: bool,
    children: Children,
) -> impl IntoView {
    let card = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if let Some(el) = card.get() {
            let _ = el.focus();
        }
    });
    let class = if wide { "result-card wide" } else { "result-card" };
    view! {
        <div class=class tabindex="-1" role="status" aria-live="polite" node_ref=card>
            <div class=tone.class()>{glyph_svg(glyph)}</div>
            <h1>{title}</h1>
            {children()}
        </div>
    }
}
