// =============================================================================
// Eustress Web - The drawing of a listing's themed page (banner, logo, gallery, links)
// =============================================================================
// What a creator set in the Website service and the Worker approved (src/api/sim_page.rs holds the data and every rule). This file
// only draws it: pieces the listing page (pages/listing.rs) places in its own layout. A listing with no themed page gets none of
// them, and the page is what it was.
//
// Author text is only ever drawn as a text node (a label) or as an attribute that passed a rule: a link's address (https only), an
// image address (the API's own origin). The colours never come through here; the page's root element carries them as custom
// properties (api::sim_page::root_style).
//
// Table of Contents:
// 1. The pieces
// 2. The gallery
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::sim_page::{gallery_srcs, grouped, image_src, motion_plan, theme_view, visible_links, LinkKind, Motion, MotionPlan, SimPage, ThemeView};

// -----------------------------------------------------------------------------
// 1. The pieces
// -----------------------------------------------------------------------------

/// The theme the page applies, or nothing for a page with no theme.
pub fn theme_of(page: &Option<SimPage>) -> Option<ThemeView> {
    page.as_ref().and_then(|p| p.theme.as_ref()).map(theme_view)
}

/// The plan for time on this page, with the reader's own request for less motion applied.
pub fn plan_of(page: &Option<SimPage>) -> MotionPlan {
    let reduced = crate::utils::prefers_reduced_motion();
    match page.as_ref().and_then(|p| p.theme.as_ref()) {
        Some(t) => motion_plan(Motion::parse(&t.motion), reduced, t.slideshow_interval_s, t.banner_fade_ms),
        None => motion_plan(Motion::None, reduced, None, None),
    }
}

/// The banner across the top. A banner that does not load leaves nothing behind.
#[component]
fn Banner(src: String) -> impl IntoView {
    let failed = RwSignal::new(false);
    view! {
        <div class="sp-banner" class:sp-gone=move || failed.get()>
            <img class="sp-banner-img" src=src alt="" decoding="async" on:error=move |_| failed.set(true) />
        </div>
    }
}

/// The banner, when the Worker approved one.
pub fn banner_view(page: &Option<SimPage>, api_base: &str) -> AnyView {
    match page.as_ref().and_then(|p| p.images.banner.as_deref()).and_then(|u| image_src(u, api_base)) {
        Some(src) => view! { <Banner src=src /> }.into_any(),
        None => ().into_any(),
    }
}

/// The logo that sits in the title, next to the name.
#[component]
fn Logo(src: String) -> impl IntoView {
    let failed = RwSignal::new(false);
    view! { <img class="sp-logo" class:sp-gone=move || failed.get() src=src alt="" decoding="async" on:error=move |_| failed.set(true) /> }
}

pub fn logo_view(page: &Option<SimPage>, api_base: &str) -> AnyView {
    match page.as_ref().and_then(|p| p.images.logo.as_deref()).and_then(|u| image_src(u, api_base)) {
        Some(src) => view! { <Logo src=src /> }.into_any(),
        None => ().into_any(),
    }
}

/// A small icon for a kind of link: plain paths, drawn by the stylesheet (`.sp-lic svg`).
fn kind_icon(kind: LinkKind) -> AnyView {
    match kind {
        LinkKind::Website => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.5 3 14.5 0 18M12 3c-3 3.5-3 14.5 0 18"/></svg> }.into_any(),
        LinkKind::Social => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="12" r="2.5"/><circle cx="18" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M8.2 10.8l7.6-3.6M8.2 13.2l7.6 3.6"/></svg> }.into_any(),
        LinkKind::Shop => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 8h14l-1 12H6z"/><path d="M9 8a3 3 0 0 1 6 0"/></svg> }.into_any(),
        LinkKind::Other => view! { <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg> }.into_any(),
    }
}

/// The Links card: each approved link under its kind, the label and, beside it in muted text, where it goes. Opens in a new tab with
/// `nofollow noopener noreferrer ugc`. Nothing is drawn when no link passes the rules.
pub fn links_view(page: &Option<SimPage>) -> AnyView {
    let Some(page) = page else { return ().into_any() };
    let groups = grouped(&visible_links(&page.links));
    if groups.is_empty() {
        return ().into_any();
    }
    view! {
        <section class="g-csec sp-links-sec">
            <h2>"Links"</h2>
            <div class="g-glass sp-links">
                {groups.into_iter().map(|(kind, items)| view! {
                    <div class="sp-lgroup">
                        <h3 class="sp-lkind">{kind.title()}</h3>
                        <ul class="sp-llist">
                            {items.into_iter().map(|l| view! {
                                <li>
                                    <a class="sp-link" href=l.url target="_blank" rel="nofollow noopener noreferrer ugc">
                                        <span class="sp-lic" aria-hidden="true">{kind_icon(kind)}</span>
                                        <span class="sp-llabel">{l.label}</span>
                                        <span class="sp-ldom">{l.domain}</span>
                                    </a>
                                </li>
                            }).collect_view()}
                        </ul>
                    </div>
                }).collect_view()}
            </div>
        </section>
    }
    .into_any()
}

/// The gallery slideshow, when the Worker approved at least one picture.
pub fn gallery_view(page: &Option<SimPage>, api_base: &str) -> AnyView {
    let Some(p) = page else { return ().into_any() };
    let images = gallery_srcs(&p.images, api_base);
    if images.is_empty() {
        return ().into_any();
    }
    let plan = plan_of(page);
    view! {
        <section class="g-csec sp-gallery-sec">
            <h2>"Gallery"</h2>
            <Gallery images=images plan=plan />
        </section>
    }
    .into_any()
}

// -----------------------------------------------------------------------------
// 2. The gallery
// -----------------------------------------------------------------------------

/// A slideshow with arrows and dots. It advances by itself only when the plan says so (`none`, or a reader who asked for less
/// motion, never); the arrows, the dots and the keyboard (left and right) always work. It rests while the pointer or focus is on it.
#[component]
fn Gallery(images: Vec<String>, plan: MotionPlan) -> impl IntoView {
    let broken = RwSignal::new(Vec::<String>::new());
    let shown = {
        let all = images.clone();
        Memo::new(move |_| all.iter().filter(|u| !broken.with(|b| b.contains(*u))).cloned().collect::<Vec<String>>())
    };
    let at = RwSignal::new(0usize);
    let paused = RwSignal::new(false);
    // Disposed with the page, which is what stops the timer below.
    let alive = StoredValue::new(());

    // The timer starts in the browser only (an Effect never runs in the prerender), and only when the reader has not asked for less
    // motion: asked again here, at the moment of starting.
    Effect::new(move |_| {
        if !plan.animate || crate::utils::prefers_reduced_motion() {
            return;
        }
        spawn_local(async move {
            loop {
                TimeoutFuture::new(plan.interval_ms).await;
                if alive.try_get_value().is_none() {
                    return;
                }
                if paused.get_untracked() {
                    continue;
                }
                let n = shown.with_untracked(|s| s.len());
                if n > 1 {
                    at.update(|i| *i = ((*i).min(n - 1) + 1) % n);
                }
            }
        });
    });

    let step = move |by: i64| {
        let n = shown.with_untracked(|s| s.len()) as i64;
        if n > 1 {
            at.update(|i| *i = ((((*i as i64).min(n - 1)) + by).rem_euclid(n)) as usize);
        }
    };

    view! {
        <div
            class="sp-gallery"
            role="group"
            aria-roledescription="carousel"
            aria-label="Gallery"
            on:mouseenter=move |_| paused.set(true)
            on:mouseleave=move |_| paused.set(false)
            on:focusin=move |_| paused.set(true)
            on:focusout=move |_| paused.set(false)
            on:keydown=move |ev| match ev.key().as_str() {
                "ArrowLeft" => step(-1),
                "ArrowRight" => step(1),
                _ => {}
            }
        >
            <div class="sp-gal-frame">
                {move || {
                    let list = shown.get();
                    let n = list.len();
                    list.into_iter().enumerate().map(|(i, src)| {
                        let url = src.clone();
                        let alt = format!("Gallery image {} of {}", i + 1, n);
                        view! {
                            <img
                                class="sp-slide"
                                class:on=move || at.get().min(n.saturating_sub(1)) == i
                                src=src
                                alt=alt
                                decoding="async"
                                on:error=move |_| broken.update(|b| if !b.contains(&url) { b.push(url.clone()) })
                            />
                        }
                    }).collect_view()
                }}
            </div>
            {move || (shown.with(|s| s.len()) > 1).then(|| view! {
                <button type="button" class="sp-arrow sp-prev" aria-label="Previous image" on:click=move |_| step(-1)><span aria-hidden="true">"\u{2039}"</span></button>
                <button type="button" class="sp-arrow sp-next" aria-label="Next image" on:click=move |_| step(1)><span aria-hidden="true">"\u{203a}"</span></button>
            })}
            {move || {
                let n = shown.with(|s| s.len());
                (n > 1).then(|| view! {
                    <div class="sp-dots">
                        {(0..n).map(|i| view! {
                            <button
                                type="button"
                                class="sp-dot"
                                class:on=move || at.get().min(n - 1) == i
                                aria-label=format!("Show image {} of {}", i + 1, n)
                                aria-current=move || (at.get().min(n - 1) == i).to_string()
                                on:click=move |_| at.set(i)
                            ></button>
                        }).collect_view()}
                    </div>
                })
            }}
        </div>
    }
}
