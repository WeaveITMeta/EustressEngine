// =============================================================================
// Eustress Web - A listing's art: cover or capture, with living previews
// =============================================================================
// The picture on a card and in a listing's hero. A creator's own cover is shown when there is
// one: a wide cover fills the frame, a portrait cover (box art) stands on the capture like a
// game box. Without a cover the 16:9 capture fills the frame. While `active` (the card is
// hovered or focused) the frame crossfades through the review captures Studio took at publish,
// asked for once, on first use; a reader who prefers reduced motion sees the first capture
// instead of a cycle. Nothing here is invented: with no image the frame is a quiet gradient.
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::catalog::{self, art_of, cover_is_portrait, ArtKind, Listing};
use crate::api::ApiClient;
use crate::state::AppState;

/// How long a preview stays up before the next one fades in.
const PREVIEW_MS: u32 = 1_100;

/// Which of `count` layers is up after `ticks` steps of a cycle that starts at the main art (layer 0).
pub fn layer_at(ticks: usize, count: usize) -> usize {
    if count == 0 { 0 } else { ticks % count }
}

/// What a frame shows while it is `active`: a cycle through the main art and every preview, one preview held still for a reader
/// who prefers reduced motion, or the main art alone when there are no previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Cycle,
    HoldFirstPreview,
    Still,
}

pub fn motion_for(previews: usize, reduced: bool) -> Motion {
    match (previews, reduced) {
        (0, _) => Motion::Still,
        (_, true) => Motion::HoldFirstPreview,
        (_, false) => Motion::Cycle,
    }
}

#[component]
pub fn ListingArt(
    listing: Listing,
    /// The card is hovered or has the focus: the previews play.
    #[prop(into)]
    active: Signal<bool>,
    /// The picture is the first thing on the page (the lead Space): load it at once instead of when it scrolls near.
    #[prop(optional)]
    eager: bool,
) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let id = listing.id.clone();
    let portrait = cover_is_portrait(&listing);
    let capture = listing.thumbnail_url.clone().filter(|u| u.starts_with("https://") || u.starts_with('/'));
    let cover = art_of(&listing).filter(|(kind, _)| *kind == ArtKind::Cover).map(|(_, url)| url);
    // The Worker names a cover from the moment it is set, and serves it only once it is approved: until then the address answers
    // 404, and the capture takes its place.
    let cover_ok = RwSignal::new(true);
    // A portrait cover stands on the capture; any other cover fills the frame by itself.
    let backdrop = Signal::derive({
        let (capture, cover) = (capture.clone(), cover.clone());
        move || if portrait || !cover_ok.get() { capture.clone() } else { cover.clone().or_else(|| capture.clone()) }
    });
    let box_art = Signal::derive({
        let cover = cover.clone();
        move || (portrait && cover_ok.get()).then(|| cover.clone()).flatten()
    });

    let previews = RwSignal::new(Vec::<String>::new());
    let asked = StoredValue::new(false);
    let tick = RwSignal::new(0usize);
    let run = StoredValue::new(0u64);

    // The first time the card is active, ask which previews there are; they play from then on while it stays active.
    Effect::new(move |_| {
        if !active.get() {
            run.update_value(|r| *r += 1);
            tick.set(0);
            return;
        }
        let my_run = run.get_value() + 1;
        run.set_value(my_run);
        let id = id.clone();
        spawn_local(async move {
            if !asked.get_value() {
                asked.set_value(true);
                let client = ApiClient::new(&api_url.get_value());
                previews.set(catalog::fetch_previews(&client, &id).await.into_iter().map(|p| p.url).filter(|u| u.starts_with("https://") || u.starts_with('/')).collect());
            }
            let count = previews.get_untracked().len();
            match motion_for(count, crate::utils::prefers_reduced_motion()) {
                Motion::Still => {}
                Motion::HoldFirstPreview => {
                    if run.try_get_value() == Some(my_run) {
                        tick.set(1);
                    }
                }
                Motion::Cycle => loop {
                    TimeoutFuture::new(PREVIEW_MS).await;
                    if run.try_get_value() != Some(my_run) {
                        return;
                    }
                    tick.update(|t| *t += 1);
                }
            }
        });
    });

    // Layer 0 is the backdrop; 1.. are the previews. The one that is up has class "on".
    let layers = move || {
        let count = previews.get().len() + 1;
        layer_at(tick.get(), count)
    };

    view! {
        <div class="art" class:art-box=move || box_art.get().is_some() class:art-empty=move || backdrop.get().is_none()>
            {move || backdrop.get().map(|src| view! { <img class="art-img" class:on=move || layers() == 0 src=src alt="" loading=if eager { "eager" } else { "lazy" } decoding="async" on:error=move |_| cover_ok.set(false) /> })}
            {move || previews.get().into_iter().enumerate().map(|(i, src)| view! {
                <img class="art-img" class:on=move || layers() == i + 1 src=src alt="" loading="lazy" decoding="async" />
            }).collect_view()}
            {move || box_art.get().map(|src| view! { <img class="art-box-img" src=src alt="" loading="lazy" decoding="async" on:error=move |_| cover_ok.set(false) /> })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cycle_starts_at_the_main_art_visits_every_preview_and_wraps() {
        assert_eq!((0..7).map(|t| layer_at(t, 3)).collect::<Vec<_>>(), [0, 1, 2, 0, 1, 2, 0]);
        assert_eq!(layer_at(5, 1), 0, "one layer never moves");
        assert_eq!(layer_at(5, 0), 0, "none is safe");
    }

    #[test]
    fn a_reader_who_prefers_reduced_motion_never_gets_a_cycle() {
        assert_eq!(motion_for(4, false), Motion::Cycle);
        assert_eq!(motion_for(4, true), Motion::HoldFirstPreview);
        assert_eq!(motion_for(0, false), Motion::Still);
        assert_eq!(motion_for(0, true), Motion::Still);
    }
}
