// =============================================================================
// Eustress Web - Friends Page (Industrial Design)
// =============================================================================
// Friends management: online/offline list, friend requests, user search,
// presence indicators, and quick-join for friends in experiences.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use crate::api::{self, ApiClient};
use crate::api::friends::{Friend, FriendRequest, get_friends, get_friend_requests};
use crate::components::{CentralNav, Footer};
use crate::pages::profile::{follow_path, follow_shown, viewer_of, FollowRequest, FollowShown};
use crate::state::{AppState, AuthState};

// -----------------------------------------------------------------------------
// Data Types
// -----------------------------------------------------------------------------

/// Active tab in the friends panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FriendsTab {
    Online,
    All,
    Requests,
    Search,
}

impl FriendsTab {
    fn label(&self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::All => "All Friends",
            Self::Requests => "Requests",
            Self::Search => "Add Friend",
        }
    }
}

// -----------------------------------------------------------------------------
// Main Component
// -----------------------------------------------------------------------------

/// Friends management page — industrial design.
#[component]
pub fn FriendsPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let auth = app_state.auth;

    // Tab state
    let active_tab = RwSignal::new(FriendsTab::Online);

    // Friends data
    let friends = RwSignal::new(Vec::<Friend>::new());
    let friend_requests = RwSignal::new(Vec::<FriendRequest>::new());
    let is_loading = RwSignal::new(true);

    // Search state
    let search_query = RwSignal::new(String::new());
    let search_results = RwSignal::new(Vec::<api::community::PublicUser>::new());
    let is_searching = RwSignal::new(false);
    // People followed from a search result during this visit, the person a follow or unfollow is waiting on, and what to say
    // when a search or a follow failed.
    let followed = RwSignal::new(Vec::<String>::new());
    let follow_busy = RwSignal::new(Option::<String>::None);
    let notice = RwSignal::new(Option::<String>::None);

    // Fetch friends on mount
    {
        let api_url = app_state.api_url.clone();
        let auth_clone = auth;
        spawn_local(async move {
            if let AuthState::Authenticated(_user) = auth_clone.get_untracked() {
                let client = ApiClient::new(&api_url);
                if let Ok(friend_list) = get_friends(&client).await {
                    friends.set(friend_list);
                }
                if let Ok(requests) = get_friend_requests(&client).await {
                    friend_requests.set(requests);
                }
                is_loading.set(false);
            } else {
                is_loading.set(false);
            }
        });
    }

    // Store api_url in a StoredValue so closures capture only Copy types
    let api_url_stored = StoredValue::new(app_state.api_url.clone());

    // Search handler
    let do_search = move || {
        let query = search_query.get();
        if query.len() < 2 {
            search_results.set(vec![]);
            return;
        }
        is_searching.set(true);
        notice.set(None);
        let api_url = api_url_stored.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&api_url);
            match api::community::search_users(&client, &query, Some(20)).await {
                Ok(response) => search_results.set(response.users),
                Err(e) => {
                    search_results.set(vec![]);
                    notice.set(Some(api::failure_sentence(&e)));
                }
            }
            is_searching.set(false);
        });
    };

    // Follow or unfollow a person from a search result, by username: the profile page's routes. The result changes only once
    // the Worker said yes.
    let toggle_follow = move |username: String| {
        if follow_busy.get_untracked().is_some() {
            return;
        }
        let was = is_followed(&followed.get_untracked(), &username);
        follow_busy.set(Some(username.clone()));
        notice.set(None);
        let api_url = api_url_stored.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let answer = client.post::<serde_json::Value, _>(follow_path(!was), &FollowRequest { username: &username }).await;
            follow_busy.try_set(None);
            match answer {
                Ok(_) => {
                    followed.try_update(|list| *list = with_follow(list, &username, !was));
                }
                Err(e) => {
                    notice.try_set(Some(api::failure_sentence(&e)));
                }
            }
        });
    };

    view! {
        <div class="page page-friends-industrial">
            <CentralNav active="friends".to_string() />

            // Background
            <div class="friends-bg">
                <div class="friends-grid-overlay"></div>
                <div class="friends-glow glow-1"></div>
            </div>

            // Header
            <section class="friends-header">
                <div class="hero-header">
                    <div class="header-line"></div>
                    <span class="header-tag">"FRIENDS"</span>
                    <div class="header-line"></div>
                </div>
                <h1 class="friends-title">"Friends"</h1>
                <p class="friends-subtitle">"See who is online, manage requests, and find new friends"</p>
            </section>

            // Auth gate
            {move || {
                let auth_state = auth.get();
                match auth_state {
                    AuthState::Authenticated(_) => view! {
                        <div class="friends-content">
                            // Tab bar
                            <div class="friends-tabs">
                                {[FriendsTab::Online, FriendsTab::All, FriendsTab::Requests, FriendsTab::Search]
                                    .into_iter()
                                    .map(|tab| {
                                        let is_active = move || active_tab.get() == tab;
                                        let request_count = move || {
                                            if tab == FriendsTab::Requests {
                                                let count = friend_requests.get().len();
                                                if count > 0 { format!(" ({})", count) } else { String::new() }
                                            } else {
                                                String::new()
                                            }
                                        };
                                        view! {
                                            <button
                                                class="friends-tab"
                                                class:active=is_active
                                                on:click=move |_| active_tab.set(tab)
                                            >
                                                {tab.label()}
                                                <span class="tab-badge">{request_count}</span>
                                            </button>
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                }
                            </div>

                            // Tab content
                            <div class="friends-panel">
                                // Online friends tab
                                {move || {
                                    if active_tab.get() == FriendsTab::Online {
                                        let online_friends: Vec<Friend> = friends.get()
                                            .into_iter()
                                            .filter(|f| f.online)
                                            .collect();
                                        view! {
                                            <div class="friends-list">
                                                {if online_friends.is_empty() {
                                                    view! {
                                                        <div class="empty-state">
                                                            <p class="empty-text">"No friends online right now"</p>
                                                            <p class="empty-hint">"Your friends will appear here when they are online"</p>
                                                        </div>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <div class="friend-cards">
                                                            {online_friends.into_iter().map(|friend| {
                                                                let in_experience = friend.current_server_id.is_some();
                                                                let initial = friend.display_name.chars().next().unwrap_or('?').to_string();
                                                                let name = friend.display_name.clone();
                                                                let handle = crate::pages::profile::handle_of(&friend.username);
                                                                view! {
                                                                    <div class="friend-card online">
                                                                        <div class="friend-avatar">
                                                                            <div class="avatar-placeholder">{initial}</div>
                                                                            <span class="status-dot online"></span>
                                                                        </div>
                                                                        <div class="friend-info">
                                                                            <span class="friend-name">{name}</span>
                                                                            <span class="friend-handle">{handle}</span>
                                                                            <span class="friend-status">
                                                                                {if in_experience { "In a Simulation" } else { "Online" }}
                                                                            </span>
                                                                        </div>
                                                                        {if in_experience {
                                                                            view! {
                                                                                <button class="btn-join-friend">"Join"</button>
                                                                            }.into_any()
                                                                        } else {
                                                                            view! { <span></span> }.into_any()
                                                                        }}
                                                                    </div>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        </div>
                                                    }.into_any()
                                                }}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }
                                }}

                                // All friends tab
                                {move || {
                                    if active_tab.get() == FriendsTab::All {
                                        let all = friends.get();
                                        view! {
                                            <div class="friends-list">
                                                <div class="list-header">
                                                    <span class="list-count">{format!("{} friends", all.len())}</span>
                                                </div>
                                                <div class="friend-cards">
                                                    {all.into_iter().map(|friend| {
                                                        let is_online = friend.online;
                                                        let initial = friend.display_name.chars().next().unwrap_or('?').to_string();
                                                        let name = friend.display_name.clone();
                                                        let handle = crate::pages::profile::handle_of(&friend.username);
                                                        let profile_link = format!("/profile/{}", friend.username);
                                                        view! {
                                                            <div class="friend-card" class:online=is_online>
                                                                <div class="friend-avatar">
                                                                    <div class="avatar-placeholder">{initial}</div>
                                                                    <span class="status-dot" class:online=is_online></span>
                                                                </div>
                                                                <div class="friend-info">
                                                                    <a href=profile_link class="friend-name">
                                                                        {name}
                                                                    </a>
                                                                    <span class="friend-handle">{handle}</span>
                                                                    <span class="friend-status">
                                                                        {if is_online { "Online" } else { "Offline" }}
                                                                    </span>
                                                                </div>
                                                            </div>
                                                        }
                                                    }).collect::<Vec<_>>()}
                                                </div>
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }
                                }}

                                // Requests tab
                                {move || {
                                    if active_tab.get() == FriendsTab::Requests {
                                        let reqs = friend_requests.get();
                                        view! {
                                            <div class="friends-list">
                                                {if reqs.is_empty() {
                                                    view! {
                                                        <div class="empty-state">
                                                            <p class="empty-text">"No pending friend requests"</p>
                                                        </div>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <div class="friend-cards">
                                                            {reqs.into_iter().map(|req| {
                                                                let initial = req.from_username.chars().next().unwrap_or('?').to_string();
                                                                let name = req.from_username.clone();
                                                                view! {
                                                                    <div class="friend-card request">
                                                                        <div class="friend-avatar">
                                                                            <div class="avatar-placeholder">{initial}</div>
                                                                        </div>
                                                                        <div class="friend-info">
                                                                            <span class="friend-name">{name}</span>
                                                                            <span class="friend-status">"Wants to be friends"</span>
                                                                        </div>
                                                                        <div class="request-actions">
                                                                            <button class="btn-accept">"Accept"</button>
                                                                            <button class="btn-decline">"Decline"</button>
                                                                        </div>
                                                                    </div>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        </div>
                                                    }.into_any()
                                                }}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }
                                }}

                                // Search / Add Friend tab
                                {move || {
                                    if active_tab.get() == FriendsTab::Search {
                                        let results = search_results.get();
                                        view! {
                                            <div class="friends-search">
                                                <div class="search-input-row">
                                                    <input
                                                        type="text"
                                                        class="search-input"
                                                        placeholder="Search by username..."
                                                        prop:value=move || search_query.get()
                                                        on:input=move |event| {
                                                            let value = leptos::prelude::event_target_value(&event);
                                                            search_query.set(value);
                                                        }
                                                    />
                                                    <button
                                                        class="btn-search"
                                                        on:click=move |_| do_search()
                                                    >
                                                        "Search"
                                                    </button>
                                                </div>

                                                {move || notice.get().map(|m| view! { <p class="search-status" role="alert">{m}</p> })}

                                                {if is_searching.get() {
                                                    view! { <p class="search-status">"Searching..."</p> }.into_any()
                                                } else if results.is_empty() && !search_query.get().is_empty() {
                                                    view! { <p class="search-status">"No users found"</p> }.into_any()
                                                } else {
                                                    view! {
                                                        <div class="search-results">
                                                            {results.into_iter().map(|user| {
                                                                let initial = user.username.chars().next().unwrap_or('?').to_string();
                                                                let uname = user.username.clone();
                                                                let profile_link = format!("/profile/{}", user.username);
                                                                let display = user.display_name.clone().unwrap_or_default();
                                                                let facts = user.facts().join(" \u{b7} ");
                                                                let own = match auth.get_untracked() {
                                                                    AuthState::Authenticated(me) => me.username == uname,
                                                                    _ => false,
                                                                };
                                                                let follow_name = uname.clone();
                                                                view! {
                                                                    <div class="friend-card search-result">
                                                                        <div class="friend-avatar">
                                                                            <div class="avatar-placeholder">{initial}</div>
                                                                        </div>
                                                                        <div class="friend-info">
                                                                            <a href=profile_link class="friend-name">
                                                                                {crate::pages::profile::handle_of(&uname)}
                                                                            </a>
                                                                            <span class="friend-status">
                                                                                {display}
                                                                            </span>
                                                                            {(!facts.is_empty()).then(|| view! { <span class="friend-status">{facts}</span> })}
                                                                        </div>
                                                                        {move || match follow_shown(&viewer_of(&auth.get()), own, is_followed(&followed.get(), &follow_name)) {
                                                                            FollowShown::Hidden => ().into_any(),
                                                                            FollowShown::SignIn => view! { <a class="g-btn g-btn-primary" href="/login">"Sign in to follow"</a> }.into_any(),
                                                                            FollowShown::Follow => {
                                                                                let name = follow_name.clone();
                                                                                view! {
                                                                                    <button type="button" class="g-btn g-btn-primary" aria-pressed="false" disabled=move || follow_busy.get().is_some() on:click=move |_| toggle_follow(name.clone())>"Follow"</button>
                                                                                }.into_any()
                                                                            }
                                                                            FollowShown::Following => {
                                                                                let name = follow_name.clone();
                                                                                view! {
                                                                                    <span class="g-chip live">"Following"</span>
                                                                                    <button type="button" class="g-btn" aria-pressed="true" disabled=move || follow_busy.get().is_some() on:click=move |_| toggle_follow(name.clone())>"Unfollow"</button>
                                                                                }.into_any()
                                                                            }
                                                                        }}
                                                                    </div>
                                                                }
                                                            }).collect::<Vec<_>>()}
                                                        </div>
                                                    }.into_any()
                                                }}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! { <div></div> }.into_any()
                                    }
                                }}
                            </div>
                        </div>
                    }.into_any(),
                    _ => view! {
                        <div class="auth-gate">
                            <div class="gate-card">
                                <h2>"Sign In to See Friends"</h2>
                                <p>"Connect with other players, see who is online, and join friends in simulations."</p>
                                <a href="/login" class="btn-primary-steel">"Sign In"</a>
                            </div>
                        </div>
                    }.into_any(),
                }
            }}

            <Footer />
        </div>
    }
}

// -----------------------------------------------------------------------------
// Who was followed from a search
// -----------------------------------------------------------------------------

/// Whether `name` is among the people followed from this page's search results. Usernames are exact, as the Worker keys them.
pub fn is_followed(followed: &[String], name: &str) -> bool {
    followed.iter().any(|f| f == name)
}

/// The list once `name` is followed (`now`) or no longer followed, with no name twice.
pub fn with_follow(followed: &[String], name: &str, now: bool) -> Vec<String> {
    let mut list: Vec<String> = followed.iter().filter(|f| f.as_str() != name).cloned().collect();
    if now {
        list.push(name.to_string());
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_follow_from_a_result_is_remembered_and_an_unfollow_forgets_it() {
        let none: Vec<String> = vec![];
        assert!(!is_followed(&none, "ada"));
        let one = with_follow(&none, "ada", true);
        assert!(is_followed(&one, "ada"));
        assert!(!is_followed(&one, "Ada"), "usernames are exact");
        assert!(!is_followed(&one, "adam"));
        let again = with_follow(&one, "ada", true);
        assert_eq!(again, vec!["ada"], "no name twice");
        let two = with_follow(&again, "bob", true);
        assert_eq!(two, vec!["ada", "bob"]);
        assert_eq!(with_follow(&two, "ada", false), vec!["bob"]);
        assert_eq!(with_follow(&two, "zed", false), vec!["ada", "bob"], "an unfollow of someone not listed changes nothing");
    }

    #[test]
    fn a_result_follows_through_the_profile_routes_by_username() {
        assert_eq!(follow_path(true), "/api/social/follow");
        assert_eq!(follow_path(false), "/api/social/unfollow");
        assert_eq!(serde_json::to_string(&FollowRequest { username: "ada_builds" }).unwrap(), r#"{"username":"ada_builds"}"#);
    }
}
