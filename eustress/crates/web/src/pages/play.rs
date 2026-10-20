use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;
use crate::api::ApiClient;

/// `POST /api/simulations/{id}/play`: counts the visit and says how to open
/// the simulation. A published simulation plays solo in the Eustress Player,
/// which downloads its world once and keeps it cached.
#[derive(Clone, Debug, serde::Deserialize)]
struct PlayResponse {
    status: String,
    #[serde(default)]
    launch: Option<LaunchInfo>,
    #[serde(default)]
    simulation: Option<SimInfo>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LaunchInfo {
    /// `eustress-player://play/<id>`, which opens the Player where it is installed.
    #[serde(default)]
    link: Option<String>,
    command: String,
    args: Vec<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct SimInfo {
    name: String,
}

#[component]
pub fn PlayPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let params = leptos_router::hooks::use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();

    let play_status = RwSignal::new("Preparing...".to_string());
    let launch = RwSignal::new(None::<LaunchInfo>);
    let sim_name = RwSignal::new(String::new());
    let loading = RwSignal::new(true);

    // Call play API on mount
    {
        let api_url = app_state.api_url.clone();
        let sim_id = id();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            let answer = client.post::<PlayResponse, _>(&format!("/api/simulations/{}/play", sim_id), &empty).await;
            loading.set(false);
            match answer {
                Ok(resp) => {
                    if let Some(sim) = &resp.simulation {
                        sim_name.set(sim.name.clone());
                    }
                    match (resp.status.as_str(), resp.launch) {
                        ("solo", Some(info)) => {
                            play_status.set("Plays in the Eustress Player on your computer.".to_string());
                            launch.set(Some(info));
                        }
                        (status, _) => play_status.set(format!("Status: {}", status)),
                    }
                }
                Err(e) => {
                    play_status.set(format!("This simulation could not be opened: {}", e));
                }
            }
        });
    }

    view! {
        <div class="page page-play">
            <CentralNav active="".to_string() />

            <main class="play-page">
                <div class="play-content">
                    <div class="play-header">
                        <h1>{move || {
                            let name = sim_name.get();
                            if name.is_empty() { "Launching...".to_string() } else { name }
                        }}</h1>
                        <p class="play-subtitle">{move || play_status.get()}</p>
                    </div>

                    {move || {
                        if let Some(info) = launch.get() {
                            let command = format!("{} {}", info.command, info.args.join(" "));
                            let link = info.link.clone().unwrap_or_default();
                            view! {
                                <div class="play-not-installed">
                                    <h2>"Play on your computer"</h2>
                                    <p class="play-subtitle">
                                        "The Eustress Player downloads this world once, keeps it cached, and runs it locally."
                                    </p>
                                    <div class="play-actions">
                                        <a href=link class="btn-download-player">"Open in Eustress Player"</a>
                                    </div>
                                    <p class="play-subtitle">"Or run it from a terminal:"</p>
                                    <code class="play-id">{command}</code>
                                </div>
                            }.into_any()
                        } else if loading.get() {
                            view! {
                                <div class="play-status">
                                    <div class="play-spinner"></div>
                                </div>
                            }.into_any()
                        } else {
                            ().into_any()
                        }
                    }}

                    <div class="play-actions">
                        <a href="/downloads/player" class="play-download-btn">"Download Eustress Player"</a>
                        <a href={move || format!("/simulation/{}", id())} class="play-back-btn">"Back to Details"</a>
                    </div>
                </div>
            </main>

            <Footer />
        </div>
    }
}
