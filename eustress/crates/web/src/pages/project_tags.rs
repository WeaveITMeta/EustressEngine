// =============================================================================
// Eustress Web - A Space's tags, edited from its settings (/projects)
// =============================================================================
// A Space has tags the author chooses (the rule is src/tags.rs, which mirrors the Worker's tags.mjs). The settings dialog on
// /projects shows them as chips, takes new ones, removes one, and saves. The editor holds at most 8, and an empty list is
// allowed: it means "the review chooses tags for you". When the review chose them, the label "Suggested by review" says so
// until the author changes them.
//
// The route that saves them is PATCH /api/simulations/{id}/tags (docs/networking/LISTING_TAGS.md, confirmed with Jev). Everything
// the page knows about the route is in `tags_edit_request` below, the one function marked CONTRACT, so a change to the route
// changes that function and nothing else.
// =============================================================================

use leptos::prelude::*;

use serde::Deserialize;

use crate::api::{self, ApiClient, ApiError};
use crate::state::AppState;
use crate::tags::{add_tags, check_tags, remove_tag, same_set, suggested_label, TAGS_HELP};

/// A request to the Worker: the method, the path and the JSON body.
#[derive(Clone, Debug, PartialEq)]
pub struct TagsEditRequest {
    pub method: &'static str,
    pub path: String,
    pub body: serde_json::Value,
}

// CONTRACT: tags edit route, confirmed with Jev 2026-10-09 (docs/networking/LISTING_TAGS.md)
/// What saves a Space's tags: `PATCH /api/simulations/{id}/tags` with `{ "tags": [...] }`, signed in as the author. An empty
/// list means the review chooses the tags. Answers: 200 with the tags as stored, `tags_source`, `changed` and `status`; 400 `bad_tags`
/// with the Worker's message; 401 signed out; 404 not the author's; 409 `in_review`. Only this function knows the route.
pub fn tags_edit_request(id: &str, tags: &[String]) -> TagsEditRequest {
    TagsEditRequest {
        method: "PATCH",
        path: format!("/api/simulations/{}/tags", urlencoding::encode(id)),
        body: serde_json::json!({ "tags": tags }),
    }
}

/// The Worker's 200: the tags as stored, who chose them, and whether anything changed (a changed set goes back to review).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct TagsEditReply {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tags_source: Option<String>,
    #[serde(default)]
    pub changed: bool,
    #[serde(default)]
    pub status: String,
}

/// Send the request `tags_edit_request` describes (PATCH or PUT; anything else is sent as PATCH).
async fn save_listing_tags(client: &ApiClient, id: &str, tags: &[String]) -> Result<TagsEditReply, ApiError> {
    let request = tags_edit_request(id, tags);
    match request.method {
        "PUT" => client.put::<TagsEditReply, _>(&request.path, &request.body).await,
        _ => client.patch::<TagsEditReply, _>(&request.path, &request.body).await,
    }
}

/// What the page says after the tags are saved, from what the Worker stored.
pub fn saved_message(name: &str, reply: &TagsEditReply) -> String {
    if !reply.changed {
        format!("{name} already has these tags. Nothing changed, and it stays as it was.")
    } else if reply.tags.is_empty() {
        format!("Tags cleared on {name}. The review will choose them, and your Space goes back to review.")
    } else {
        let shown = reply.tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(" ");
        format!("Tags saved on {name}: {shown}. Your Space goes back to review.")
    }
}

/// Shown beside the editor while a review is running (the Worker answers 409 `in_review`).
pub const IN_REVIEW_WORDS: &str = "This Space is in review. Its tags can change once the review is done.";

/// Why a save failed, in words: the Worker's own message for refused tags (it names the entry), and a sentence for the other answers.
pub fn save_error_words(e: &ApiError) -> String {
    match e {
        ApiError::Server { status: 400, message } => serde_json::from_str::<serde_json::Value>(message)
            .ok()
            .and_then(|v| v.get("error").and_then(|m| m.as_str()).map(str::to_string))
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| format!("The tags were not saved: {}", api::failure_sentence(e))),
        ApiError::Server { status: 409, .. } => IN_REVIEW_WORDS.to_string(),
        ApiError::Unauthorized => "Your sign-in has ended. Sign in again to change the tags.".to_string(),
        ApiError::NotFound => "This Space was not found, or it is not yours.".to_string(),
        other => format!("The tags were not saved: {}", api::failure_sentence(other)),
    }
}

/// The tag editor in a Space's settings: chips with a remove button each, a box to add, the sentence about review, and Save.
/// `on_saved` gets the line to show once the Worker has said yes.
#[component]
pub fn TagEditor(place_id: String, place_name: String, tags: Vec<String>, tags_source: Option<String>, locked: bool, on_saved: Callback<String>) -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let place_id = StoredValue::new(place_id);
    let place_name = StoredValue::new(place_name);
    let saved = StoredValue::new(tags.clone());
    let source = StoredValue::new(tags_source);
    let draft = RwSignal::new(tags);
    let typed = RwSignal::new(String::new());
    let problem = RwSignal::new(None::<String>);
    let saving = RwSignal::new(false);

    let add = move || {
        if locked {
            return;
        }
        match add_tags(&draft.get_untracked(), &typed.get_untracked()) {
            Ok(next) => {
                draft.set(next);
                typed.set(String::new());
                problem.set(None);
            }
            Err(why) => problem.set(Some(why)),
        }
    };
    let changed = move || !same_set(&draft.get(), &saved.get_value());
    // "Suggested by review" while the tags on show are the ones the review chose; once the author changes them they are the author's.
    let label = move || {
        let shown = draft.get();
        let judged = same_set(&shown, &saved.get_value());
        suggested_label(&shown, source.get_value().as_deref().filter(|_| judged), true)
    };

    let save = move |_| {
        if locked || saving.get_untracked() {
            return;
        }
        let next = match check_tags(&draft.get_untracked()) {
            Ok(tags) => tags,
            Err(why) => {
                problem.set(Some(why));
                return;
            }
        };
        saving.set(true);
        problem.set(None);
        let id = place_id.get_value();
        let name = place_name.get_value();
        let url = api_url.get_value();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&url);
            match save_listing_tags(&client, &id, &next).await {
                Ok(reply) => {
                    saving.set(false);
                    on_saved.run(saved_message(&name, &reply));
                }
                Err(e) => {
                    saving.set(false);
                    problem.set(Some(save_error_words(&e)));
                }
            }
        });
    };

    view! {
        <div class="tag-editor">
            <ul class="tag-editor-chips" aria-label="Tags on this Space">
                {move || {
                    let list = draft.get();
                    if list.is_empty() {
                        view! { <li class="tag-editor-none">"No tags yet. The review will choose them."</li> }.into_any()
                    } else {
                        list.into_iter().map(|t| {
                            let gone = t.clone();
                            view! {
                                <li class="tag-editor-chip">
                                    <span>{format!("#{t}")}</span>
                                    <button type="button" class="tag-editor-x" disabled=locked aria-label=format!("Remove the tag {t}") on:click=move |_| {
                                        draft.set(remove_tag(&draft.get_untracked(), &gone));
                                        problem.set(None);
                                    }>"\u{00d7}"</button>
                                </li>
                            }
                        }).collect_view().into_any()
                    }
                }}
            </ul>
            {move || label().map(|words| view! { <p class="tag-editor-note">{words}</p> })}
            {locked.then(|| view! { <p class="tag-editor-note">{IN_REVIEW_WORDS}</p> })}
            <div class="tag-editor-add">
                <input
                    type="text"
                    class="tag-editor-input"
                    placeholder="Add a tag, such as co-op"
                    aria-label="Add a tag"
                    disabled=locked
                    prop:value=move || typed.get()
                    on:input=move |e| typed.set(event_target_value(&e))
                    on:keydown=move |e: web_sys::KeyboardEvent| {
                        if e.key() == "Enter" || e.key() == "," {
                            e.prevent_default();
                            add();
                        }
                    }
                />
                <button type="button" class="btn btn-secondary" disabled=locked on:click=move |_| add()>"Add"</button>
            </div>
            {move || problem.get().map(|why| view! { <p class="tag-editor-problem" role="alert">{why}</p> })}
            <p class="tag-editor-help">{TAGS_HELP}</p>
            <button type="button" class="btn btn-primary tag-editor-save" disabled=move || locked || saving.get() || !changed() on:click=save>
                {move || if saving.get() { "Saving..." } else { "Save tags" }}
            </button>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::projects::{ProjectData, ProjectsResponse};

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_save_request_is_a_patch_to_the_tags_route_and_an_empty_list_means_the_review_chooses() {
        let r = tags_edit_request("07d0", &list(&["zombies", "co-op"]));
        assert_eq!((r.method, r.path.as_str()), ("PATCH", "/api/simulations/07d0/tags"));
        assert_eq!(r.body, serde_json::json!({ "tags": ["zombies", "co-op"] }));
        assert_eq!(tags_edit_request("07d0", &[]).body, serde_json::json!({ "tags": [] }), "empty, not left out");
        assert_eq!(tags_edit_request("a/b", &[]).path, "/api/simulations/a%2Fb/tags", "an id never changes the path");
        assert!(tags_edit_request("07d0", &[]).body.get("genre").is_none());
    }

    #[test]
    fn exactly_one_function_is_marked_as_the_contract_to_confirm() {
        // The marker is built from two pieces so this test does not count itself.
        let marker = concat!("// CONTR", "ACT: tags edit route, confirmed with Jev 2026-10-09");
        let source = include_str!("project_tags.rs");
        assert_eq!(source.matches(marker).count(), 1, "one marker in this file");
        let after = source.split(marker).nth(1).unwrap();
        let next_fn = after.lines().find(|l| l.trim_start().starts_with("pub fn ") || l.trim_start().starts_with("fn ") || l.trim_start().starts_with("async fn ")).unwrap();
        assert!(next_fn.contains("tags_edit_request"), "the marker sits on the function that knows the route: {next_fn}");
        for other in [include_str!("projects.rs"), include_str!("../tags.rs"), include_str!("../api/mod.rs")] {
            assert_eq!(other.matches(marker).count(), 0, "the route is named nowhere else");
        }
    }

    #[test]
    fn the_line_after_a_save_says_what_happened_and_that_review_follows_only_when_something_changed() {
        let reply = |tags: &[&str], changed: bool| TagsEditReply { tags: list(tags), changed, status: "in_review".to_string(), ..Default::default() };
        assert_eq!(saved_message("Box Head", &reply(&["zombies", "co-op"], true)), "Tags saved on Box Head: #zombies #co-op. Your Space goes back to review.");
        assert_eq!(saved_message("Box Head", &reply(&[], true)), "Tags cleared on Box Head. The review will choose them, and your Space goes back to review.");
        let same = saved_message("Box Head", &reply(&["zombies"], false));
        assert_eq!(same, "Box Head already has these tags. Nothing changed, and it stays as it was.");
        assert!(!same.contains("back to review"), "no review when nothing changed");
        for s in [saved_message("A", &reply(&[], true)), saved_message("A", &reply(&["ab"], true)), same, IN_REVIEW_WORDS.to_string()] {
            assert!(!s.contains('\u{2014}') && !s.contains('\u{2013}'));
        }
    }

    #[test]
    fn the_workers_answer_to_a_save_is_read() {
        let r: TagsEditReply = serde_json::from_str("{\"ok\":true,\"id\":\"07d0\",\"tags\":[\"harbour\",\"tides\"],\"tags_source\":\"author\",\"genre\":null,\"changed\":true,\"status\":\"in_review\"}").unwrap();
        assert_eq!((r.tags, r.tags_source.as_deref(), r.changed, r.status.as_str()), (list(&["harbour", "tides"]), Some("author"), true, "in_review"));
        let bare: TagsEditReply = serde_json::from_str("{\"ok\":true}").unwrap();
        assert!(!bare.changed, "a missing changed reads as no change");
    }

    #[test]
    fn a_failed_save_says_why_in_the_workers_words_for_bad_tags() {
        let worker = "{\"error\":\"Entry 2 of tags looks like a phone number, and a tag never carries personal information.\",\"code\":\"bad_tags\"}";
        assert_eq!(save_error_words(&ApiError::Server { status: 400, message: worker.to_string() }), "Entry 2 of tags looks like a phone number, and a tag never carries personal information.");
        assert!(save_error_words(&ApiError::Server { status: 400, message: "not json".to_string() }).starts_with("The tags were not saved"));
        assert_eq!(save_error_words(&ApiError::Server { status: 409, message: "{\"code\":\"in_review\"}".to_string() }), IN_REVIEW_WORDS);
        assert!(save_error_words(&ApiError::Unauthorized).contains("Sign in again"));
        assert!(save_error_words(&ApiError::NotFound).contains("not yours"));
        assert!(save_error_words(&ApiError::Network("down".to_string())).starts_with("The tags were not saved"));
    }

    #[test]
    fn the_reviews_line_about_the_tags_it_chose_is_read_by_the_panel_and_the_case_history() {
        use crate::pages::review_panel::ReviewView;
        use crate::pages::takedown_wire::Feedback;
        let v: ReviewView = serde_json::from_str("{\"status\":\"listed\",\"assigned_tags\":[\"fiction\",\"fantasy\"],\"tags_chosen\":\"Tags chosen for your Space: fiction, fantasy\"}").unwrap();
        assert_eq!((v.assigned_tags.clone(), v.tags_chosen.as_deref()), (list(&["fiction", "fantasy"]), Some("Tags chosen for your Space: fiction, fantasy")));
        let none: ReviewView = serde_json::from_str("{\"status\":\"listed\",\"tags_chosen\":null}").unwrap();
        assert_eq!(none.tags_chosen, None);
        let fb: Feedback = serde_json::from_str("{\"reasons\":[],\"assigned_tags\":[\"fiction\"],\"tags_chosen\":\"Tags chosen for your Space: fiction\"}").unwrap();
        assert_eq!(fb.tags_chosen.as_deref(), Some("Tags chosen for your Space: fiction"));
        assert!(crate::pages::account_cases::feedback_has_content(&fb), "a case with only the tags line has something to show");
        assert!(!crate::pages::account_cases::feedback_has_content(&Feedback { tags_chosen: Some("  ".to_string()), ..Feedback::default() }));
        // Both pages show the line through the one helper.
        for page in [include_str!("review_panel.rs"), include_str!("account_cases.rs")] {
            let live = page.split("#[cfg(test)]").next().unwrap();
            assert!(live.contains("tags_chosen_line("), "the page shows the review's tags line");
        }
    }

    #[test]
    fn a_projects_answer_with_tags_and_no_genre_reads() {
        let json = r#"{"projects":[{"id":"07d0","name":"Box Head","description":null,"thumbnail_url":null,"status":"approved_private","max_players":10,"is_public":false,"version":1,"play_count":3,"favorite_count":1,"last_edited":"2026-10-08","created_at":"2026-09-01T00:00:00Z","published_at":null,"storage_url":null,"tags":["zombies","coop"],"tags_source":"judge"}],"total":1,"page":1,"limit":100}"#;
        let answer: ProjectsResponse = serde_json::from_str(json).unwrap();
        let p: &ProjectData = &answer.projects[0];
        assert_eq!((p.tags.clone().unwrap_or_default(), p.tags_source.as_deref()), (list(&["zombies", "coop"]), Some("judge")));
        // The author sees the label on tags the review chose, and the page asks as the author.
        assert_eq!(suggested_label(&p.tags.clone().unwrap_or_default(), p.tags_source.as_deref(), true), Some("Suggested by review"));
    }

    #[test]
    fn an_older_answer_still_reads_and_its_genre_is_not_used() {
        let json = r#"{"projects":[{"id":"07d0","name":"Box Head","description":null,"thumbnail_url":null,"status":"approved","genre":"Horror","max_players":10,"is_public":true,"version":1,"play_count":3,"favorite_count":1,"last_edited":"2026-10-08","created_at":"2026-09-01T00:00:00Z","published_at":null,"storage_url":null}],"total":1,"page":1,"limit":100}"#;
        let answer: ProjectsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(answer.projects[0].tags, None, "no tags in the answer, and none made from the genre");
        let null_tags = json.replace("\"storage_url\":null", "\"storage_url\":null,\"tags\":null,\"tags_source\":null");
        assert!(serde_json::from_str::<ProjectsResponse>(&null_tags).is_ok(), "null tags read as none");
    }

    #[test]
    fn no_page_shows_or_filters_by_a_genre() {
        // Everything outside the test modules, in the files that draw a listing, a card, a filter or a request.
        let files: [(&str, &str); 8] = [
            ("api/catalog.rs", include_str!("../api/catalog.rs")),
            ("api/gallery.rs", include_str!("../api/gallery.rs")),
            ("components/command_palette.rs", include_str!("../components/command_palette.rs")),
            ("pages/listing.rs", include_str!("listing.rs")),
            ("pages/projects.rs", include_str!("projects.rs")),
            ("pages/project_sell.rs", include_str!("project_sell.rs")),
            ("pages/project_tags.rs", include_str!("project_tags.rs")),
            ("style/play.css", include_str!("../../style/play.css")),
        ];
        for (name, text) in files {
            let live = text.split("#[cfg(test)]").next().unwrap();
            let hits: Vec<&str> = live.lines().filter(|l| l.to_lowercase().contains("genre")).collect();
            assert!(hits.is_empty(), "{name} still says genre: {hits:?}");
        }
        // The one place that reads the word is the Gallery's address, to turn an old link into a tag.
        let gallery = include_str!("gallery.rs").split("#[cfg(test)]").next().unwrap();
        for line in gallery.lines().filter(|l| l.to_lowercase().contains("genre")) {
            let t = line.trim_start();
            assert!(t.starts_with("//") || t.contains("get(\"genre\")") || t.contains("legacy"), "gallery.rs: {line}");
        }
    }

    #[test]
    fn the_tag_editor_text_is_the_sentence_mckale_asked_for() {
        assert_eq!(TAGS_HELP, "Leave them empty and the review chooses tags for you. Changing tags sends your Space back to review.");
        let source = include_str!("project_tags.rs").split("#[cfg(test)]").next().unwrap();
        assert!(source.contains("{TAGS_HELP}"), "the editor shows it");
    }
}
