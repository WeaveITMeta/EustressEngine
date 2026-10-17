// =============================================================================
// Eustress Web - Search: Ctrl+K over Spaces, makers and tags
// =============================================================================
// One search box for the whole site, opened from the Gallery's search pill or with Ctrl+K (Cmd+K on a Mac). It reads what the
// Worker lists: Spaces by name, description, author and tag (`GET /api/simulations?q=`), makers who have a listed Space
// (`GET /api/makers?q=`), and the tags in use (`GET /api/simulations/tags`, matched here). With nothing typed it offers the newest
// Spaces and the tags in use. A route a deployment lacks answers nothing, never an error. Nothing here is invented.
//
// The pure rules (which rows, in which order, where Enter goes, how the arrow keys wrap) are plain functions with tests; the
// component is a combobox over a listbox: the focus stays in the input and `aria-activedescendant` names the row Enter opens.
// =============================================================================

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::catalog::{self, art_of, card_tags, Listing, Maker, TagCount};
use crate::api::ApiClient;
use crate::state::AppState;
use crate::utils::after_event;

/// Rows a group may show, so the box stays one screen tall.
const MAX_SPACES: usize = 5;
const MAX_MAKERS: usize = 3;
const MAX_TAGS: usize = 4;
/// How long the box waits after a key before asking the Worker, so a word typed quickly is one request.
const TYPING_PAUSE_MS: u32 = 180;

// -----------------------------------------------------------------------------
// 1. What the box can open
// -----------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Space(Listing),
    Maker(Maker),
    /// A tag, with how many Spaces use it when that is known. A tag the visitor typed that is not in the top list has no count.
    Tag { tag: String, count: Option<u32> },
}

/// What Enter does for a row: the page it opens.
pub fn hit_path(hit: &Hit) -> String {
    match hit {
        Hit::Space(l) => format!("/simulation/{}", urlencoding::encode(&l.id)),
        Hit::Maker(m) => format!("/profile/{}", urlencoding::encode(&m.username)),
        Hit::Tag { tag, .. } => format!("/gallery?tag={}", urlencoding::encode(tag)),
    }
}

/// The text the visitor typed, trimmed. A search for "#horror" is a search for the tag, so the leading marks are kept apart.
pub fn normalise(query: &str) -> String {
    query.trim().to_string()
}

/// The tag a query names, when it is written as one ("#horror", "# horror" and "##horror" all name `horror`).
pub fn tag_in(query: &str) -> Option<String> {
    let rest = query.trim_start().strip_prefix('#')?;
    let tag = rest.trim_start_matches('#').trim().to_lowercase().replace(' ', "-");
    (!tag.is_empty()).then_some(tag)
}

/// The rows to show: Spaces, then makers, then tags. A query written as a tag shows tags first and nothing else, since nobody
/// types "#" to find a person. Each group is capped.
pub fn build_hits(query: &str, spaces: &[Listing], makers: &[Maker], tags: &[TagCount]) -> Vec<Hit> {
    let q = normalise(query);
    if let Some(tag) = tag_in(&q) {
        let mut out: Vec<Hit> = tags
            .iter()
            .filter(|t| t.tag.contains(&tag))
            .take(MAX_TAGS + 2)
            .map(|t| Hit::Tag { tag: t.tag.clone(), count: Some(t.count) })
            .collect();
        // The exact tag is always one row, so a tag nobody ranks can still be opened.
        if !out.iter().any(|h| matches!(h, Hit::Tag { tag: t, .. } if *t == tag)) {
            out.insert(0, Hit::Tag { tag, count: None });
        }
        return out;
    }
    let needle = q.to_lowercase();
    let mut out: Vec<Hit> = spaces.iter().take(MAX_SPACES).cloned().map(Hit::Space).collect();
    out.extend(makers.iter().take(MAX_MAKERS).cloned().map(Hit::Maker));
    if !needle.is_empty() {
        out.extend(
            tags.iter().filter(|t| t.tag.contains(&needle)).take(MAX_TAGS).map(|t| Hit::Tag { tag: t.tag.clone(), count: Some(t.count) }),
        );
    } else {
        out.extend(tags.iter().take(MAX_TAGS).map(|t| Hit::Tag { tag: t.tag.clone(), count: Some(t.count) }));
    }
    out
}

/// The arrow keys: one row down or up, wrapping at the ends. With no rows there is nowhere to go.
pub fn move_active(current: usize, len: usize, down: bool) -> usize {
    if len == 0 {
        0
    } else if down {
        (current + 1) % len
    } else {
        (current + len - 1) % len
    }
}

fn group_of(hit: &Hit) -> &'static str {
    match hit {
        Hit::Space(_) => "Spaces",
        Hit::Maker(_) => "Makers",
        Hit::Tag { .. } => "Tags",
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

// -----------------------------------------------------------------------------
// 2. The box
// -----------------------------------------------------------------------------

/// Whether the box is open, and the control that opened it (the focus returns to it on close).
#[derive(Clone, Copy)]
pub struct PaletteSlot {
    open: RwSignal<bool>,
    opener: StoredValue<Option<web_sys::HtmlElement>, LocalStorage>,
}

impl PaletteSlot {
    pub fn open(&self) {
        use wasm_bindgen::JsCast;
        if self.open.get_untracked() {
            return;
        }
        let active = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element())
            .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
        self.opener.set_value(active);
        self.open.set(true);
    }

    fn close(&self) {
        self.open.set(false);
        if let Some(opener) = self.opener.try_update_value(|o| o.take()).flatten() {
            let _ = opener.focus();
        }
    }
}

pub fn provide_palette() {
    provide_context(PaletteSlot { open: RwSignal::new(false), opener: StoredValue::new_local(None) });
}

/// The pill that opens the box, drawn like a search field: "Search Spaces, Makers, #tags" and the shortcut.
#[component]
pub fn SearchPill() -> impl IntoView {
    let slot = expect_context::<PaletteSlot>();
    let hint = if crate::components::play_chooser::Viewer::detect() == crate::components::play_chooser::Viewer::Mac { "\u{2318} K" } else { "Ctrl K" };
    view! {
        <button type="button" class="cp-pill" aria-haspopup="dialog" on:click=move |_| slot.open()>
            <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/></svg>
            <span>"Search Spaces, Makers, #tags"</span>
            <kbd>{hint}</kbd>
        </button>
    }
}

/// Mounted once, inside the router: listens for Ctrl+K and draws the box when it is open.
#[component]
pub fn CommandPaletteHost() -> impl IntoView {
    let slot = expect_context::<PaletteSlot>();

    #[cfg(not(feature = "ssr"))]
    {
        let handle = window_event_listener(leptos::ev::keydown, move |ev: leptos::ev::KeyboardEvent| {
            if (ev.ctrl_key() || ev.meta_key()) && ev.key().eq_ignore_ascii_case("k") {
                ev.prevent_default();
                if slot.open.get_untracked() {
                    slot.close();
                } else {
                    slot.open();
                }
            }
        });
        on_cleanup(move || handle.remove());
    }

    view! { {move || slot.open.get().then(|| view! { <Palette on_close=Callback::new(move |_| slot.close()) /> })} }
}

/// A Space row's thumbnail, or nothing when the Space has no picture. The Worker answers 404 for a capture it will not serve
/// (animated, transparent, several images, not an image, too large), so a picture that fails to load clears art_ok and the box
/// is left empty, as for a Space with no picture, never the browser's broken-image icon. Each row makes its own signal.
fn row_thumb(art: Option<String>, art_ok: RwSignal<bool>) -> impl IntoView {
    move || art.clone().filter(|_| art_ok.get()).map(|u| view! { <img src=u alt="" loading="lazy" decoding="async" on:error=move |_| art_ok.set(false) /> })
}

#[component]
fn Palette(on_close: Callback<()>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let navigate = leptos_router::hooks::use_navigate();

    let query = RwSignal::new(String::new());
    let hits = RwSignal::new(Vec::<Hit>::new());
    let active = RwSignal::new(0usize);
    let busy = RwSignal::new(true);
    let asked = RwSignal::new(false);
    let input = NodeRef::<leptos::html::Input>::new();
    // Each key starts a new search; an answer that comes back after a newer key is thrown away.
    let turn = StoredValue::new(0u64);
    // The tags in use are read once, and filtered here as the visitor types.
    let tags = StoredValue::new(Vec::<TagCount>::new());
    let tags_loaded = StoredValue::new(false);

    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });

    let search = move |text: String| {
        let my_turn = turn.get_value() + 1;
        turn.set_value(my_turn);
        spawn_local(async move {
            if !text.is_empty() {
                TimeoutFuture::new(TYPING_PAUSE_MS).await;
                if turn.try_get_value() != Some(my_turn) {
                    return;
                }
            }
            let client = ApiClient::new(&api_url.get_value());
            if !tags_loaded.get_value() {
                let list = catalog::fetch_popular_tags(&client).await;
                if turn.try_get_value() != Some(my_turn) {
                    return;
                }
                tags.set_value(list);
                tags_loaded.set_value(true);
            }
            let (spaces, makers) = if tag_in(&text).is_some() {
                (Vec::new(), Vec::new())
            } else if text.is_empty() {
                (catalog::fetch_listings(&client).await.unwrap_or_default(), Vec::new())
            } else {
                futures::future::join(catalog::search_listings(&client, &text), catalog::search_makers(&client, &text)).await
            };
            if turn.try_get_value() != Some(my_turn) {
                return;
            }
            let rows = build_hits(&text, &spaces, &makers, &tags.get_value());
            active.set(0);
            hits.set(rows);
            busy.set(false);
            asked.set(true);
        });
    };

    // The first view: the newest Spaces and the tags in use.
    search(String::new());

    let close = move || after_event(move || on_close.run(()));
    let go = {
        let navigate = navigate.clone();
        move |hit: &Hit| {
            let path = hit_path(hit);
            let navigate = navigate.clone();
            after_event(move || {
                on_close.run(());
                navigate(&path, Default::default());
            });
        }
    };

    let keys = {
        let go = go.clone();
        move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
            "Escape" => {
                ev.prevent_default();
                close();
            }
            "ArrowDown" | "ArrowUp" => {
                ev.prevent_default();
                active.set(move_active(active.get_untracked(), hits.get_untracked().len(), ev.key() == "ArrowDown"));
            }
            "Enter" => {
                ev.prevent_default();
                let rows = hits.get_untracked();
                if let Some(hit) = rows.get(active.get_untracked()) {
                    go(hit);
                }
            }
            // The box has one control; Tab stays on it rather than leaving the dialog.
            "Tab" => ev.prevent_default(),
            _ => {}
        }
    };

    let on_input = move |ev: leptos::ev::Event| {
        let text = event_target_value(&ev);
        query.set(text.clone());
        busy.set(true);
        search(normalise(&text));
    };

    view! {
        <div class="cp-scrim" on:click=move |ev| { if ev.target() == ev.current_target() { close(); } }>
            <div class="cp" role="dialog" aria-modal="true" aria-label="Search" on:keydown=keys>
                <div class="cp-q">
                    <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/></svg>
                    <input
                        node_ref=input
                        type="text"
                        role="combobox"
                        aria-autocomplete="list"
                        aria-expanded="true"
                        aria-controls="cp-list"
                        aria-label="Search Spaces, Makers and tags"
                        aria-activedescendant=move || (!hits.get().is_empty()).then(|| format!("cp-opt-{}", active.get()))
                        autocomplete="off"
                        autocapitalize="off"
                        spellcheck="false"
                        placeholder="Search Spaces, Makers, #tags"
                        prop:value=move || query.get()
                        on:input=on_input
                    />
                    <button type="button" class="cp-x" on:click=move |_| close()>"Esc"</button>
                </div>
                <div class="cp-status" role="status" aria-live="polite">
                    {move || {
                        if busy.get() { String::new() } else {
                            let n = hits.get().len();
                            match n { 0 => "No results".to_string(), 1 => "1 result".to_string(), _ => format!("{n} results") }
                        }
                    }}
                </div>
                <ul class="cp-list" id="cp-list" role="listbox" aria-label="Results">
                    {move || {
                        let rows = hits.get();
                        if rows.is_empty() {
                            let q = query.get();
                            let shown = if busy.get() && !asked.get() {
                                view! { <li class="cp-empty" role="presentation"><span class="cp-skel"></span><span class="cp-skel short"></span></li> }.into_any()
                            } else if busy.get() {
                                view! { <li class="cp-empty" role="presentation">"Searching..."</li> }.into_any()
                            } else if q.trim().is_empty() {
                                view! { <li class="cp-empty" role="presentation">"Nothing is published yet. Spaces appear here once they are approved."</li> }.into_any()
                            } else {
                                view! { <li class="cp-empty" role="presentation">{format!("Nothing matches \"{}\". Try a tag such as #horror, or a Maker's name.", q.trim())}</li> }.into_any()
                            };
                            return shown;
                        }
                        let heading_for = {
                            let rows = rows.clone();
                            move |i: usize| (i == 0 || group_of(&rows[i - 1]) != group_of(&rows[i])).then(|| group_of(&rows[i]))
                        };
                        let empty_query = query.get().trim().is_empty();
                        rows.iter().cloned().enumerate().map(|(i, hit)| {
                            let heading = heading_for(i).map(|h| {
                                let h = if empty_query && h == "Spaces" { "New" } else if empty_query && h == "Tags" { "Tags in use" } else { h };
                                view! { <li class="cp-head" role="presentation">{h}</li> }
                            });
                            let hit_for_click = hit.clone();
                            let body = match hit {
                                Hit::Space(l) => {
                                    let art = art_of(&l).map(|(_, url)| url);
                                    let by = l.author_name.clone().filter(|n| !n.is_empty()).map(|n| format!("@{n}")).unwrap_or_default();
                                    let tag = card_tags(&l).into_iter().next().map(|t| format!("#{t}"));
                                    let players = (l.max_players > 1).then(|| format!("up to {} players", l.max_players));
                                    let sub = [Some(by).filter(|s| !s.is_empty()), tag, players].into_iter().flatten().collect::<Vec<_>>().join(" \u{00b7} ");
                                    view! {
                                        <span class="cp-thumb">{row_thumb(art, RwSignal::new(true))}</span>
                                        <span class="cp-text"><b>{if l.name.is_empty() { "Untitled Space".to_string() } else { l.name.clone() }}</b><small>{sub}</small></span>
                                    }.into_any()
                                }
                                Hit::Maker(m) => {
                                    let initial = m.username.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                                    view! {
                                        <span class="cp-ic" aria-hidden="true">{initial}</span>
                                        <span class="cp-text"><b>{format!("@{}", m.username)}</b><small>{plural(m.space_count, "Space", "Spaces")}</small></span>
                                    }.into_any()
                                }
                                Hit::Tag { tag, count } => view! {
                                    <span class="cp-ic" aria-hidden="true">"#"</span>
                                    <span class="cp-text"><b>{format!("#{tag}")}</b><small>{match count { Some(n) => plural(n, "Space", "Spaces"), None => "Search this tag".to_string() }}</small></span>
                                }.into_any(),
                            };
                            let go = go.clone();
                            view! {
                                {heading}
                                <li
                                    id=format!("cp-opt-{i}")
                                    class="cp-row"
                                    class:on=move || active.get() == i
                                    role="option"
                                    aria-selected=move || (active.get() == i).to_string()
                                    on:mousemove=move |_| { if active.get_untracked() != i { active.set(i); } }
                                    on:click=move |_| go(&hit_for_click)
                                >
                                    {body}
                                    <small class="cp-key">{move || if active.get() == i { "Enter" } else { "" }}</small>
                                </li>
                            }
                        }).collect_view().into_any()
                    }}
                </ul>
                <div class="cp-foot"><span>"\u{2191}\u{2193} move"</span><span>"Enter open"</span><span>"Esc close"</span></div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str, name: &str) -> Listing {
        Listing { id: id.to_string(), name: name.to_string(), ..Default::default() }
    }
    fn maker(name: &str) -> Maker {
        Maker { username: name.to_string(), avatar_url: None, space_count: 2, visits: 0 }
    }
    fn tag(name: &str, count: u32) -> TagCount {
        TagCount { tag: name.to_string(), count }
    }

    #[test]
    fn rows_come_in_the_order_spaces_makers_tags_and_each_group_is_capped() {
        let spaces: Vec<Listing> = (0..9).map(|i| space(&format!("s{i}"), "Box")).collect();
        let makers: Vec<Maker> = (0..6).map(|i| maker(&format!("box{i}"))).collect();
        let tags: Vec<TagCount> = (0..9).map(|i| tag(&format!("box{i}"), 1)).collect();
        let rows = build_hits("box", &spaces, &makers, &tags);
        assert_eq!(rows.iter().filter(|h| matches!(h, Hit::Space(_))).count(), MAX_SPACES);
        assert_eq!(rows.iter().filter(|h| matches!(h, Hit::Maker(_))).count(), MAX_MAKERS);
        assert_eq!(rows.iter().filter(|h| matches!(h, Hit::Tag { .. })).count(), MAX_TAGS);
        let order: Vec<&str> = rows.iter().map(group_of).collect();
        let mut sorted = order.clone();
        sorted.dedup();
        assert_eq!(sorted, ["Spaces", "Makers", "Tags"], "one run of each group, in that order");
    }

    #[test]
    fn a_word_finds_the_tags_that_contain_it_and_no_others() {
        let tags = [tag("horror", 3), tag("racing", 2), tag("co-op-horror", 1)];
        let rows = build_hits("horr", &[], &[], &tags);
        let found: Vec<&str> = rows.iter().filter_map(|h| if let Hit::Tag { tag, .. } = h { Some(tag.as_str()) } else { None }).collect();
        assert_eq!(found, ["horror", "co-op-horror"]);
    }

    #[test]
    fn a_hash_means_a_tag_search_shows_only_tags_and_always_offers_the_exact_tag() {
        let spaces = [space("a", "Horror Hotel")];
        let makers = [maker("horror_fan")];
        let tags = [tag("horror", 3)];
        let rows = build_hits("#horror", &spaces, &makers, &tags);
        assert_eq!(rows, vec![Hit::Tag { tag: "horror".to_string(), count: Some(3) }], "Spaces and makers are not offered for a tag");
        let rows = build_hits("# brand new", &[], &[], &tags);
        assert_eq!(rows, vec![Hit::Tag { tag: "brand-new".to_string(), count: None }], "a tag nobody ranks can still be opened");
        assert_eq!(tag_in("#"), None);
        assert_eq!(tag_in("horror"), None);
        assert_eq!(tag_in("##Horror"), Some("horror".to_string()));
    }

    #[test]
    fn with_nothing_typed_the_newest_spaces_and_the_tags_in_use_are_offered() {
        let rows = build_hits("   ", &[space("a", "A"), space("b", "B")], &[maker("never")], &[tag("horror", 3)]);
        assert!(matches!(rows[0], Hit::Space(_)));
        assert!(matches!(rows.last(), Some(Hit::Tag { .. })));
    }

    #[test]
    fn enter_opens_the_page_the_row_names_and_a_name_cannot_change_the_path() {
        assert_eq!(hit_path(&Hit::Space(space("07d0-84d8", "x"))), "/simulation/07d0-84d8");
        assert_eq!(hit_path(&Hit::Maker(maker("Sim Builder"))), "/profile/Sim%20Builder");
        assert_eq!(hit_path(&Hit::Tag { tag: "co-op".to_string(), count: None }), "/gallery?tag=co-op");
        assert_eq!(hit_path(&Hit::Space(space("a/../b?x=1", "x"))), "/simulation/a%2F..%2Fb%3Fx%3D1");
    }

    #[test]
    fn the_arrow_keys_wrap_and_an_empty_list_has_nowhere_to_go() {
        assert_eq!(move_active(0, 3, true), 1);
        assert_eq!(move_active(2, 3, true), 0);
        assert_eq!(move_active(0, 3, false), 2);
        assert_eq!(move_active(5, 0, true), 0);
    }

    /// What a Space row's thumbnail box draws for this picture, rendered on the server. art_ok is the signal the img's on:error clears.
    fn thumb_html(art: Option<&str>, art_ok: bool) -> String {
        use leptos::tachys::view::RenderHtml;
        let owner = Owner::new();
        owner.with(|| row_thumb(art.map(str::to_string), RwSignal::new(art_ok)).to_html())
    }

    #[test]
    fn a_row_thumbnail_that_will_not_load_leaves_the_empty_box_a_row_without_one_has() {
        let url = "https://api.example.dev/a/thumbnail";
        let shown = thumb_html(Some(url), true);
        assert!(shown.contains("<img") && shown.contains(url), "{shown}");
        let none = thumb_html(None, true);
        assert!(!none.contains("<img"), "{none}");
        // art_ok is false once the img's on:error has run: the row then draws exactly what it draws for a Space with no picture.
        let failed = thumb_html(Some(url), false);
        assert_eq!(failed, none, "a picture that failed leaves the box empty, not the browser's broken-image icon");
        assert!(!failed.contains("<img") && !failed.contains(url));
    }

    #[test]
    fn the_row_thumbnail_has_an_error_handler_that_clears_its_own_signal() {
        let page = include_str!("command_palette.rs").split(concat!("#[cfg", "(test)]")).next().unwrap();
        assert_eq!(page.matches("src=u alt=\"\"").count(), 1, "one picture tag in the palette");
        let at = page.find("src=u alt=\"\"").unwrap();
        let start = page[..at].rfind("<img").expect("an img tag");
        let tag = &page[start..start + page[start..].find("/>").expect("the tag ends")];
        assert!(tag.contains("on:error=move |_| art_ok.set(false)"), "{tag}");
        assert!(page.contains("art.clone().filter(|_| art_ok.get())"), "the picture is drawn only while art_ok");
        // One signal for each row: a signal shared by the rows would blank every row at the first failure.
        assert!(page.contains("<span class=\"cp-thumb\">{row_thumb(art, RwSignal::new(true))}</span>"));
    }

    #[test]
    fn counts_read_as_a_person_would_say_them() {
        assert_eq!(plural(1, "Space", "Spaces"), "1 Space");
        assert_eq!(plural(0, "Space", "Spaces"), "0 Spaces");
        assert_eq!(plural(12, "Space", "Spaces"), "12 Spaces");
    }
}
