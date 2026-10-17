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

    // The pages that put a listing's picture or a person's avatar into an img, and how many such imgs each one has. Each must have an
    // on:error handler, so a picture the Worker answers 404 for (animated, transparent, several images, not an image, too large) shows
    // the page's own empty state, never the browser's broken-image icon. The number is what the scan must find in the file: a scan
    // that finds fewer has stopped reading it. community.rs is not scanned here. settings.rs expects none of its own: its avatar is
    // drawn by account_avatar in nav.rs, and a bare img added there would be counted.
    const PAGES: [(&str, &str, usize); 10] = [
        ("projects.rs", include_str!("../pages/projects.rs"), 1),
        ("marketplace.rs", include_str!("../pages/marketplace.rs"), 1),
        ("marketplace_item.rs", include_str!("../pages/marketplace_item.rs"), 1),
        ("listing.rs", include_str!("../pages/listing.rs"), 2),
        ("profile.rs", include_str!("../pages/profile.rs"), 1),
        ("command_palette.rs", include_str!("command_palette.rs"), 1),
        ("dashboard.rs", include_str!("../pages/dashboard.rs"), 1),
        ("gallery.rs", include_str!("../pages/gallery.rs"), 1),
        ("nav.rs", include_str!("nav.rs"), 1),
        ("settings.rs", include_str!("../pages/settings.rs"), 0),
    ];

    /// The src values that are paths under /assets by construction: the category icons, which category_icon builds.
    const ASSET_BINDINGS: [&str; 2] = ["icon", "icon.clone()"];

    /// Scan a page's code (its tests and its comment lines left out) for img tags whose src is not a literal path under /assets.
    /// Returns how many there are, and the ones that have no on:error handler.
    fn imgs_without_a_handler(source: &str) -> (usize, Vec<String>) {
        let code = source.split(concat!("#[cfg", "(test)]")).next().unwrap();
        let code = code.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let (mut guarded, mut bare) = (0, Vec::new());
        let mut rest = code.as_str();
        while let Some(at) = rest.find("<img") {
            let tail = &rest[at + 4..];
            // An img element: the name ends at white space.
            if !tail.starts_with(char::is_whitespace) {
                rest = tail;
                continue;
            }
            let end = tail.find("/>").expect("an img tag ends in />");
            let tag = &tail[..end];
            rest = &tail[end..];
            let src = tag.match_indices("src=").find(|(i, _)| tag[..*i].ends_with(char::is_whitespace)).map(|(i, _)| &tag[i + 4..]);
            let is_asset = match src {
                Some(value) if value.starts_with('"') => value[1..].starts_with("/assets/"),
                Some(value) => ASSET_BINDINGS.contains(&value.split_whitespace().next().unwrap_or("")),
                None => false,
            };
            if is_asset {
                continue;
            }
            guarded += 1;
            if !tag.contains("on:error=") {
                bare.push(tag.trim().to_string());
            }
        }
        (guarded, bare)
    }

    #[test]
    fn every_listing_picture_and_avatar_on_these_pages_has_an_error_handler() {
        for (name, source, expected) in PAGES {
            let (guarded, bare) = imgs_without_a_handler(source);
            assert_eq!(guarded, expected, "{name}: the scan found a different number of pictures from outside /assets");
            assert!(bare.is_empty(), "{name}: a picture with no on:error handler: {bare:?}");
        }
    }

    #[test]
    fn the_scan_tells_a_picture_from_an_asset_icon_and_a_handled_one_from_a_bare_one() {
        let sample = concat!(
            "view! { <img src=\"/assets/icons/cube.svg\" alt=\"\" /> }\n",
            "// <img src=src alt=\"\" /> a comment that names one\n",
            "view! { <img class=\"a\" src=src alt=\"\" on:error=move |_| ok.set(false) /> }\n",
            "view! { <img src=icon.clone() alt=\"\" /> }\n",
            "view! { <img class=\"b\" src=url alt=\"\" loading=\"lazy\" /> }\n",
            "view! { <img class=\"c\" alt=\"\" src=move || pic.get() /> }\n",
            "view! { <img\n  class=\"d\"\n  src=src\n  alt=\"\" /> }\n",
            "view! { <img src=\"https://evil.example/a.png\" alt=\"\" /> }\n",
        );
        let (guarded, bare) = imgs_without_a_handler(sample);
        assert_eq!(guarded, 5, "the handled one and the four bare ones; the asset literal, the comment and the icon are not pictures");
        assert_eq!(bare.len(), 4, "{bare:?}");
        assert!(bare.iter().all(|tag| !tag.contains("on:error")));
    }

    #[test]
    fn the_icon_the_scan_lets_through_is_always_a_path_under_assets() {
        use crate::pages::marketplace::{category_icon, CATEGORIES};
        for key in CATEGORIES.iter().map(|c| c.0).chain(["", "holograms", "../x"]) {
            let icon = category_icon(key);
            assert!(icon.starts_with("/assets/icons/") && icon.ends_with(".svg") && !icon.contains(".."), "{key}: {icon}");
        }
        for (name, source, _) in PAGES {
            if source.contains("src=icon") {
                assert!(source.contains("let icon = category_icon("), "{name}: src=icon is not the category's icon");
            }
        }
    }

    #[test]
    fn a_reader_who_prefers_reduced_motion_never_gets_a_cycle() {
        assert_eq!(motion_for(4, false), Motion::Cycle);
        assert_eq!(motion_for(4, true), Motion::HoldFirstPreview);
        assert_eq!(motion_for(0, false), Motion::Still);
        assert_eq!(motion_for(0, true), Motion::Still);
    }
}
