use leptos::prelude::*;
use crate::components::{CentralNav, Footer};
use crate::state::AppState;
use crate::api::ApiClient;

#[derive(Clone, Debug, serde::Deserialize)]
struct PlayResponse {
    status: String,
    #[serde(default)]
    server: Option<ServerInfo>,
    #[serde(default)]
    launch: Option<LaunchInfo>,
    #[serde(default)]
    simulation: Option<SimInfo>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct ServerInfo {
    node_id: String,
    address: String,
    port: u16,
    protocol: String,
    players: u32,
    max_players: u32,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct LaunchInfo {
    command: String,
    args: Vec<String>,
    r2_key: Option<String>,
    pak_url: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct SimInfo {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
}

#[component]
pub fn PlayPage() -> impl IntoView {
    let app_state = expect_context::<AppState>();
    let params = leptos_router::hooks::use_params_map();
    let id = move || params.read().get("id").unwrap_or_default();

    let play_status = RwSignal::new("Finding server...".to_string());
    let server_info = RwSignal::new(None::<ServerInfo>);
    let sim_name = RwSignal::new(String::new());

    // Call play API on mount
    {
        let api_url = app_state.api_url.clone();
        let sim_id = id();
        wasm_bindgen_futures::spawn_local(async move {
            let client = ApiClient::new(&api_url);
            let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            match client.post::<PlayResponse, _>(&format!("/api/simulations/{}/play", sim_id), &empty).await {
                Ok(resp) => {
                    if let Some(sim) = &resp.simulation {
                        sim_name.set(sim.name.clone());
                    }
                    match resp.status.as_str() {
                        "ready" => {
                            if let Some(server) = resp.server {
                                play_status.set(format!("Server ready at {}:{}", server.address, server.port));
                                server_info.set(Some(server));
                            }
                        }
                        "spawn" => {
                            play_status.set("No server available. Download Eustress Engine to host locally.".to_string());
                        }
                        _ => {
                            play_status.set(format!("Status: {}", resp.status));
                        }
                    }
                }
                Err(e) => {
                    play_status.set(format!("Failed to connect: {:?}", e));
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
                        if let Some(server) = server_info.get() {
                            view! {
                                <div class="play-server-card">
                                    <h2>"Server Ready"</h2>
                                    <div class="play-server-details">
                                        <div class="play-detail-row">
                                            <span class="play-label">"Address"</span>
                                            <span class="play-value">{format!("{}:{}", server.address, server.port)}</span>
                                        </div>
                                        <div class="play-detail-row">
                                            <span class="play-label">"Protocol"</span>
                                            <span class="play-value">{server.protocol.to_uppercase()}</span>
                                        </div>
                                        <div class="play-detail-row">
                                            <span class="play-label">"Players"</span>
                                            <span class="play-value">{format!("{}/{}", server.players, server.max_players)}</span>
                                        </div>
                                    </div>
                                    <a href="/download" class="play-join-btn">"Launch Player"</a>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <div class="play-status-card">
                                    <div class="play-spinner"></div>
                                </div>
                            }.into_any()
                        }
                    }}

                    <div class="play-actions">
                        <a href="/download" class="play-download-btn">"Download Eustress Engine"</a>
                        <a href={move || format!("/simulation/{}", id())} class="play-back-btn">"Back to Details"</a>
                    </div>
                </div>
            </main>

            <Footer />
        </div>
    }
}
