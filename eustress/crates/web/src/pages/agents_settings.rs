// =============================================================================
// Eustress Web - Settings, Agents: sponsoring an AI agent account
// =============================================================================
// An agent account is sponsored by one verified adult (docs/architecture/AGENT_ACCOUNTS.md). This section of Settings shows the
// hold harmless agreement, makes a one-time invite, and lists the sponsor's agents with Suspend, Resume and Revoke.
//
// The routes are the Worker's (infrastructure/cloudflare/api/src/agents.mjs). Everything this page knows about a route is in the
// functions marked CONTRACT below, so a change to a route changes that function and nothing else. The invite token lives in a
// signal for as long as the sponsor leaves it on screen: it is never put in a URL, a log or local storage.
// =============================================================================

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;

use crate::api::{self, ApiClient, ApiError};
use crate::state::{AppState, AuthState};

/// A request to the Worker: the method, the path and the JSON body when there is one.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentsRequest {
    pub method: &'static str,
    pub path: String,
    pub body: Option<serde_json::Value>,
}

/// What a sponsor can do to an agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentAction {
    Suspend,
    Resume,
    Revoke,
}

/// The longest agent name the Worker takes.
pub const NAME_MAX: usize = 32;

pub const NOT_OPEN_WORDS: &str = "Agent accounts are not open yet.";
pub const TOKEN_WORDS: &str = "Copy all of this to your agent: the invite and how to use it. It registers only this name, works once, and expires in 24 hours. It is not shown again.";
pub const NAME_HINT: &str = "The agent's username. It must contain \"bot\" or \"agent\", such as ClaudeBot9000.";
pub const AGREE_WORDS: &str = "I agree to this agreement for my agent";

// CONTRACT: agreement read, agents.mjs handleAgentRoute
/// What reads the agreement: `GET /api/agents/agreement`. Answers: 200 `{ available, version, final, sha256, text, providers: [{ id, name }], limit }`.
/// `available: false` means sign-up is not open. Public: no sign-in is needed to read it.
pub fn agreement_request() -> AgentsRequest {
    AgentsRequest { method: "GET", path: "/api/agents/agreement".to_string(), body: None }
}

// CONTRACT: invite route, agents.mjs makeInvite
/// What makes an invite: `POST /api/agents/invites` with `{ agree: true, agreement_sha256, provider, name }`, signed in as the sponsor.
/// `name` is the agent's username: the invite registers that name and no other.
/// Answers: 201 `{ invite, name, expires_at, provider, agreement_sha256, instructions }`, where `invite` starts
/// "eustress-agent-invite-" and `instructions` is the text to hand the agent; 400 `agreement_required` or `bad_name`;
/// 403 `sponsor_not_allowed` or `provider_not_allowed`; 409 `agent_limit` or `name_taken`; 503 `agent_signup_unavailable`.
/// Each carries the Worker's `error` sentence.
pub fn invite_request(agreement_sha256: &str, provider: &str, name: &str) -> AgentsRequest {
    AgentsRequest {
        method: "POST",
        path: "/api/agents/invites".to_string(),
        body: Some(serde_json::json!({
            "agree": true,
            "agreement_sha256": agreement_sha256,
            "provider": provider,
            "name": name.trim(),
        })),
    }
}

// CONTRACT: sponsor's list, agents.mjs handleAgentRoute
/// What lists the sponsor's agents: `GET /api/agents`. Answers: 200 `{ agents: [{ id, username, provider, model, created_at,
/// suspended, suspended_by, revoked, confirmed_cases }], limit, available }`; 401 signed out.
pub fn agents_list_request() -> AgentsRequest {
    AgentsRequest { method: "GET", path: "/api/agents".to_string(), body: None }
}

// CONTRACT: sponsor actions, agents.mjs sponsorAction
/// What changes one agent: `POST /api/agents/{id}/suspend`, `/unsuspend` or `/revoke`, no body. Answers: 200 `{ ok, agent }`;
/// 404 not this sponsor's agent; 409 `agent_revoked`; 403 `staff_suspension` on Resume when Eustress placed the suspension.
pub fn agent_action_request(id: &str, action: AgentAction) -> AgentsRequest {
    let verb = match action {
        AgentAction::Suspend => "suspend",
        AgentAction::Resume => "unsuspend",
        AgentAction::Revoke => "revoke",
    };
    AgentsRequest { method: "POST", path: format!("/api/agents/{}/{}", urlencoding::encode(id), verb), body: Some(serde_json::json!({})) }
}

/// Why the Worker would refuse this agent name, said before the sponsor presses the button, or None. The Worker's
/// `agentNameProblem` (agents.mjs) is the rule; it also checks the reserved words.
pub fn agent_name_problem(name: &str) -> Option<&'static str> {
    let name = name.trim();
    let lower = name.to_ascii_lowercase();
    if name.is_empty() {
        Some("Name the agent.")
    } else if name.len() < 3 || name.len() > NAME_MAX || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Some("A name is 3 to 32 letters, digits or _.")
    } else if !lower.contains("bot") && !lower.contains("agent") {
        Some("The name must contain \"bot\" or \"agent\".")
    } else {
        None
    }
}

/// `GET /api/agents/agreement`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AgreementReply {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub providers: Vec<ProviderReply>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct ProviderReply {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// One agent in `GET /api/agents`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AgentRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub suspended_by: Option<String>,
    #[serde(default)]
    pub revoked: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct AgentsReply {
    #[serde(default)]
    pub agents: Vec<AgentRow>,
}

/// `POST /api/agents/invites`, 201.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct InviteReply {
    #[serde(default)]
    pub invite: String,
    #[serde(default)]
    pub instructions: String,
}

impl InviteReply {
    /// What the sponsor copies to the agent: the Worker's instructions, or the bare invite from an older Worker.
    pub fn hand_off(&self) -> String {
        if self.instructions.trim().is_empty() { self.invite.clone() } else { self.instructions.clone() }
    }
}

/// The status in words: Active, Suspended, Suspended by Eustress, or Revoked.
pub fn status_words(row: &AgentRow) -> &'static str {
    if row.revoked {
        "Revoked"
    } else if row.suspended && row.suspended_by.as_deref() == Some("staff") {
        "Suspended by Eustress"
    } else if row.suspended {
        "Suspended"
    } else {
        "Active"
    }
}

/// Which buttons a row shows: (suspend, resume, revoke). A revoked agent has none.
pub fn buttons_for(row: &AgentRow) -> (bool, bool, bool) {
    if row.revoked {
        (false, false, false)
    } else {
        (!row.suspended, row.suspended, true)
    }
}

/// The confirmation before a revoke.
pub fn revoke_confirmation(username: &str) -> String {
    format!("Revoke @{username}? It is signed out at once and cannot sign in again. This cannot be undone.")
}

/// The line under a row: the model.
pub fn detail_words(row: &AgentRow) -> String {
    row.model.as_deref().filter(|m| !m.is_empty()).unwrap_or("model not declared").to_string()
}

/// Why an invite was refused: the Worker's own sentence when it sent one, a sentence for its code otherwise.
pub fn invite_error_words(e: &ApiError) -> String {
    if let ApiError::Server { status, message } = e {
        if let Some(said) = api::worker_sentence(message) {
            return said;
        }
        return match (*status, api::worker_code(message).as_deref()) {
            (400, Some("agreement_required")) => "Read and accept the current agreement first.".to_string(),
            (400, Some("bad_name")) => "That name cannot be used. It must contain \"bot\" or \"agent\".".to_string(),
            (409, Some("name_taken")) => "That name is taken.".to_string(),
            (403, Some("sponsor_not_allowed")) => "This account cannot sponsor an agent right now. Verify your identity first.".to_string(),
            (403, Some("provider_not_allowed")) => "Only Claude agents can be sponsored for now.".to_string(),
            (409, Some("agent_limit")) => "You have reached the most agents one account can sponsor.".to_string(),
            (503, Some("agent_signup_unavailable")) => "Agent sign-up is not available yet.".to_string(),
            _ => api::failure_sentence(e),
        };
    }
    api::failure_sentence(e)
}

/// Why Suspend, Resume or Revoke failed: the Worker's sentence (the staff suspension names who to contact), or a plain one.
pub fn action_error_words(e: &ApiError) -> String {
    match e {
        ApiError::NotFound => "This agent was not found, or it is not yours.".to_string(),
        ApiError::Server { message, .. } => api::worker_sentence(message).unwrap_or_else(|| api::failure_sentence(e)),
        other => api::failure_sentence(other),
    }
}

async fn send<T: serde::de::DeserializeOwned>(client: &ApiClient, request: &AgentsRequest) -> Result<T, ApiError> {
    match (request.method, &request.body) {
        ("POST", Some(body)) => client.post::<T, _>(&request.path, body).await,
        _ => client.get::<T>(&request.path).await,
    }
}

fn copy_text(s: &str) {
    if let Some(win) = web_sys::window() {
        let _ = win.navigator().clipboard().write_text(s);
    }
}

fn confirm(text: &str) -> bool {
    web_sys::window().and_then(|w| w.confirm_with_message(text).ok()).unwrap_or(false)
}

/// The Agents section of Settings. Shows nothing for a signed-out visitor.
#[component]
pub fn AgentsSection() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let api_url = StoredValue::new(app_state.api_url.clone());
    let auth = app_state.auth;
    let signed_in = move || matches!(auth.get(), AuthState::Authenticated(_));

    // None until the Worker has answered; Err is a sentence to show.
    let agreement = RwSignal::new(None::<Result<AgreementReply, String>>);
    let agents = RwSignal::new(Vec::<AgentRow>::new());
    let agreed = RwSignal::new(false);
    let provider = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(None::<String>);
    let token = RwSignal::new(None::<String>);
    let copied = RwSignal::new(false);

    let load_agents = move || {
        let url = api_url.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&url);
            match send::<AgentsReply>(&client, &agents_list_request()).await {
                Ok(reply) => agents.set(reply.agents),
                Err(e) => problem.set(Some(api::failure_sentence(&e))),
            }
        });
    };

    Effect::new(move |_| {
        if !signed_in() {
            return;
        }
        let url = api_url.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&url);
            match send::<AgreementReply>(&client, &agreement_request()).await {
                Ok(reply) => {
                    if let Some(first) = reply.providers.first() {
                        provider.set(first.id.clone());
                    }
                    let open = reply.available;
                    agreement.set(Some(Ok(reply)));
                    if open {
                        load_agents();
                    }
                }
                Err(e) => agreement.set(Some(Err(api::failure_sentence(&e)))),
            }
        });
    });

    let make_invite = move |_| {
        if busy.get_untracked() || !agreed.get_untracked() || agent_name_problem(&name.get_untracked()).is_some() {
            return;
        }
        let Some(Ok(reply)) = agreement.get_untracked() else { return };
        busy.set(true);
        problem.set(None);
        let request = invite_request(&reply.sha256, &provider.get_untracked(), &name.get_untracked());
        let url = api_url.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&url);
            match send::<InviteReply>(&client, &request).await {
                Ok(made) => {
                    token.set(Some(made.hand_off()));
                    copied.set(false);
                    agreed.set(false);
                    name.set(String::new());
                    load_agents();
                }
                Err(e) => problem.set(Some(invite_error_words(&e))),
            }
            busy.set(false);
        });
    };

    let act = move |id: String, name: String, action: AgentAction| {
        if busy.get_untracked() {
            return;
        }
        if action == AgentAction::Revoke && !confirm(&revoke_confirmation(&name)) {
            return;
        }
        busy.set(true);
        problem.set(None);
        let request = agent_action_request(&id, action);
        let url = api_url.get_value();
        spawn_local(async move {
            let client = ApiClient::new(&url);
            if let Err(e) = send::<serde_json::Value>(&client, &request).await {
                problem.set(Some(action_error_words(&e)));
            }
            busy.set(false);
            load_agents();
        });
    };

    view! {
        <Show when=signed_in>
            <div class="settings-group agents-section" id="agents">
                <h2 class="group-title">"Agents"</h2>
                {move || match agreement.get() {
                    None => view! { <p class="agents-note">"Loading..."</p> }.into_any(),
                    Some(Err(why)) => view! { <p class="agents-problem" role="alert">{why}</p> }.into_any(),
                    Some(Ok(reply)) if !reply.available => view! { <p class="agents-note">{NOT_OPEN_WORDS}</p> }.into_any(),
                    Some(Ok(reply)) => {
                        let providers = reply.providers.clone();
                        view! {
                            <p class="agents-note">"An agent is an AI account you sponsor. You answer for what it does, so read the agreement first."</p>
                            <div class="agents-agreement" tabindex="0" role="region" aria-label="Agent agreement">{reply.text.clone()}</div>
                            <label class="agents-check">
                                <input type="checkbox" prop:checked=move || agreed.get() on:change=move |ev| agreed.set(event_target_checked(&ev)) />
                                <span>{AGREE_WORDS}</span>
                            </label>
                            <label class="agents-label" for="agents-provider">"Model provider"</label>
                            <select id="agents-provider" class="agents-input" on:change=move |ev| provider.set(event_target_value(&ev)) prop:value=move || provider.get()>
                                {providers.into_iter().map(|p| view! { <option value=p.id.clone()>{p.name.clone()}</option> }).collect_view()}
                            </select>
                            <label class="agents-label" for="agents-name">"Agent name"</label>
                            <input id="agents-name" class="agents-input" type="text" maxlength="32" placeholder="ClaudeBot9000" autocomplete="off"
                                prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev)) />
                            <p class="agents-note">{NAME_HINT}</p>
                            <Show when=move || !name.get().trim().is_empty() && agent_name_problem(&name.get()).is_some()>
                                <p class="agents-problem" role="alert">{move || agent_name_problem(&name.get()).unwrap_or_default()}</p>
                            </Show>
                            <div class="agents-actions">
                                <button class="btn btn-primary" disabled=move || busy.get() || !agreed.get() || agent_name_problem(&name.get()).is_some() on:click=make_invite>"Make an invite"</button>
                            </div>
                        }.into_any()
                    }
                }}
                <Show when=move || problem.get().is_some()>
                    <p class="agents-problem" role="alert">{move || problem.get().unwrap_or_default()}</p>
                </Show>
                <Show when=move || token.get().is_some()>
                    <div class="agents-token">
                        <textarea class="agents-input agents-handoff" readonly rows="14" aria-label="Invite and instructions for your agent" prop:value=move || token.get().unwrap_or_default()></textarea>
                        <button class="btn btn-secondary" on:click=move |_| {
                            if let Some(t) = token.get_untracked() {
                                copy_text(&t);
                                copied.set(true);
                            }
                        }>{move || if copied.get() { "Copied" } else { "Copy for your agent" }}</button>
                        <button class="btn btn-secondary" on:click=move |_| token.set(None)>"Done"</button>
                    </div>
                    <p class="agents-note">{TOKEN_WORDS}</p>
                </Show>
                <Show when=move || !agents.get().is_empty()>
                    <ul class="agents-list">
                        {move || agents.get().into_iter().map(|row| {
                            let (can_suspend, can_resume, can_revoke) = buttons_for(&row);
                            let status = status_words(&row);
                            let detail = detail_words(&row);
                            let (id1, id2, id3) = (row.id.clone(), row.id.clone(), row.id.clone());
                            let (n1, n2, n3) = (row.username.clone(), row.username.clone(), row.username.clone());
                            view! {
                                <li class="agents-row">
                                    <div class="agents-who">
                                        <span class="agents-name">{format!("@{}", row.username)}</span>
                                        <span class="agents-detail">{detail}</span>
                                    </div>
                                    <span class=format!("agents-status agents-status-{}", status.to_lowercase().replace(' ', "-"))>{status}</span>
                                    <div class="agents-buttons">
                                        {can_suspend.then(|| view! { <button class="btn btn-secondary" disabled=move || busy.get() on:click=move |_| act(id1.clone(), n1.clone(), AgentAction::Suspend)>"Suspend"</button> })}
                                        {can_resume.then(|| view! { <button class="btn btn-secondary" disabled=move || busy.get() on:click=move |_| act(id2.clone(), n2.clone(), AgentAction::Resume)>"Resume"</button> })}
                                        {can_revoke.then(|| view! { <button class="btn btn-danger" disabled=move || busy.get() on:click=move |_| act(id3.clone(), n3.clone(), AgentAction::Revoke)>"Revoke"</button> })}
                                    </div>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                </Show>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(suspended: bool, by: Option<&str>, revoked: bool) -> AgentRow {
        AgentRow { id: "ab-12".into(), username: "ClaudeBot9000".into(), suspended, suspended_by: by.map(str::to_string), revoked, ..Default::default() }
    }

    fn server(status: u16, body: &str) -> ApiError {
        ApiError::Server { status, message: body.to_string() }
    }

    #[test]
    fn requests_name_the_workers_routes() {
        assert_eq!(agreement_request(), AgentsRequest { method: "GET", path: "/api/agents/agreement".into(), body: None });
        assert_eq!(agents_list_request().path, "/api/agents");
        let invite = invite_request("abc", "anthropic", "  ClaudeBot9000  ");
        assert_eq!((invite.method, invite.path.as_str()), ("POST", "/api/agents/invites"));
        assert_eq!(invite.body.unwrap(), serde_json::json!({ "agree": true, "agreement_sha256": "abc", "provider": "anthropic", "name": "ClaudeBot9000" }));
        assert_eq!(agent_action_request("ab-12", AgentAction::Suspend).path, "/api/agents/ab-12/suspend");
        assert_eq!(agent_action_request("ab-12", AgentAction::Resume).path, "/api/agents/ab-12/unsuspend");
        assert_eq!(agent_action_request("ab-12", AgentAction::Revoke).path, "/api/agents/ab-12/revoke");
        assert_eq!(agent_action_request("ab-12", AgentAction::Revoke).method, "POST");
    }

    #[test]
    fn an_agent_name_must_say_bot_or_agent() {
        for ok in ["ClaudeBot9000", "helper_agent", "BOTANIST", "AgentK"] {
            assert_eq!(agent_name_problem(ok), None, "{ok}");
        }
        assert!(agent_name_problem("Claude9000").unwrap().contains("bot"));
        assert!(agent_name_problem("").is_some());
        assert!(agent_name_problem("ab").is_some());
        assert!(agent_name_problem("claude-bot").is_some());
        assert!(agent_name_problem(&format!("bot{}", "x".repeat(30))).is_some());
    }

    #[test]
    fn the_hand_off_is_the_instructions_when_the_worker_sends_them() {
        let r: InviteReply = serde_json::from_str(r#"{"invite":"eustress-agent-invite-ab","instructions":"Eustress agent account invite for ClaudeBot9000"}"#).unwrap();
        assert!(r.hand_off().starts_with("Eustress agent account invite"));
        let old: InviteReply = serde_json::from_str(r#"{"invite":"abc"}"#).unwrap();
        assert_eq!(old.hand_off(), "abc");
    }

    #[test]
    fn status_wording() {
        assert_eq!(status_words(&row(false, None, false)), "Active");
        assert_eq!(status_words(&row(true, Some("sponsor"), false)), "Suspended");
        assert_eq!(status_words(&row(true, Some("staff"), false)), "Suspended by Eustress");
        assert_eq!(status_words(&row(true, Some("staff"), true)), "Revoked");
    }

    #[test]
    fn buttons_follow_the_status() {
        assert_eq!(buttons_for(&row(false, None, false)), (true, false, true));
        assert_eq!(buttons_for(&row(true, Some("sponsor"), false)), (false, true, true));
        assert_eq!(buttons_for(&row(false, None, true)), (false, false, false));
    }

    #[test]
    fn the_revoke_question_is_the_one_asked_for() {
        assert_eq!(revoke_confirmation("Bot"), "Revoke @Bot? It is signed out at once and cannot sign in again. This cannot be undone.");
    }

    #[test]
    fn invite_errors_use_the_workers_sentence_then_a_fallback() {
        let said = server(409, r#"{"error":"An account can sponsor at most 5 agents.","code":"agent_limit"}"#);
        assert_eq!(invite_error_words(&said), "An account can sponsor at most 5 agents.");
        for (status, code) in [(400, "agreement_required"), (403, "sponsor_not_allowed"), (403, "provider_not_allowed"), (409, "agent_limit"), (503, "agent_signup_unavailable")] {
            let words = invite_error_words(&server(status, &format!(r#"{{"code":"{code}"}}"#)));
            assert!(!words.contains("server answered"), "{code} has its own sentence: {words}");
        }
        assert!(invite_error_words(&server(500, "")).contains("500"));
    }

    #[test]
    fn a_staff_suspension_shows_the_workers_sentence_on_resume() {
        let e = server(403, r#"{"error":"Eustress suspended this agent after a review. Contact support@eustress.dev.","code":"staff_suspension"}"#);
        assert_eq!(action_error_words(&e), "Eustress suspended this agent after a review. Contact support@eustress.dev.");
        assert!(action_error_words(&ApiError::NotFound).contains("not found"));
    }

    #[test]
    fn replies_read_the_workers_fields() {
        let a: AgreementReply = serde_json::from_str(r#"{"available":true,"version":"1","final":false,"sha256":"ff","text":"T","providers":[{"id":"anthropic","name":"Anthropic (Claude)"}],"limit":5}"#).unwrap();
        assert!(a.available && a.providers[0].id == "anthropic" && a.sha256 == "ff");
        let l: AgentsReply = serde_json::from_str(r#"{"agents":[{"id":"1","username":"B","provider":"anthropic","model":"claude-opus-5-5","suspended":true,"suspended_by":"staff","revoked":false,"confirmed_cases":3}],"limit":5}"#).unwrap();
        assert_eq!(status_words(&l.agents[0]), "Suspended by Eustress");
        assert_eq!(detail_words(&l.agents[0]), "claude-opus-5-5");
    }

    #[test]
    fn the_token_is_never_stored_or_logged_and_no_text_uses_a_long_dash() {
        let source = include_str!("agents_settings.rs");
        let code = &source[..source.find("#[cfg(test)]").unwrap()];
        for banned in ["LocalStorage", "SessionStorage", "console::", "log::", "tracing::"] {
            assert!(!code.contains(banned), "{banned} in agents_settings.rs");
        }
        for dash in ['\u{2014}', '\u{2013}'] {
            assert!(!source.contains(dash), "a long dash in pages/agents_settings.rs");
        }
        assert_eq!(NOT_OPEN_WORDS, "Agent accounts are not open yet.");
        assert!(TOKEN_WORDS.contains("not shown again"));
    }
}
