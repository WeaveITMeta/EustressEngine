//! Fetch the signed-in avatar off the render thread, then use the sealed spawner.
use super::{AvatarDescriptor, AvatarSystems, DespawnAllAvatars, SpawnAvatar};
use bevy::prelude::*;
use std::sync::{mpsc, Mutex};

/// Hosts provide their existing session token; it is never logged or persisted here.
#[derive(Message)]
pub struct SpawnSavedAvatar {
    pub token: Option<String>,
    pub at: Vec3,
}

type ResultMessage = (u64, Vec3, Result<AvatarDescriptor, String>);
#[derive(Resource)]
struct ProfileRequests {
    sender: mpsc::Sender<ResultMessage>,
    receiver: Mutex<mpsc::Receiver<ResultMessage>>,
    generation: u64,
}

impl Default for ProfileRequests {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: Mutex::new(receiver),
            generation: 0,
        }
    }
}

pub struct AvatarProfilePlugin;
impl Plugin for AvatarProfilePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnSavedAvatar>()
            .init_resource::<ProfileRequests>()
            .add_systems(
                Update,
                (cancel_pending, request_profile, finish_profile)
                    .chain()
                    .before(AvatarSystems::Lifecycle),
            );
    }
}

fn cancel_pending(
    mut stop: MessageReader<DespawnAllAvatars>,
    mut pending: ResMut<ProfileRequests>,
) {
    if stop.read().next().is_some() {
        stop.clear();
        pending.generation = pending.generation.wrapping_add(1);
    }
}

fn request_profile(
    mut requests: MessageReader<SpawnSavedAvatar>,
    pending: Res<ProfileRequests>,
    mut spawn: MessageWriter<SpawnAvatar>,
) {
    for request in requests.read() {
        // Useful for offline Studio authoring and website descriptor exports.
        if let Some(path) = std::env::var_os("EUSTRESS_AVATAR_FILE") {
            let result = std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|bytes| {
                    serde_json::from_slice::<AvatarDescriptor>(&bytes).map_err(|e| e.to_string())
                })
                .and_then(|d| d.validate().map(|_| d));
            match result {
                Ok(d) => {
                    spawn.write(SpawnAvatar::new(d, request.at));
                }
                Err(e) => {
                    tracing::error!("avatar: cannot load EUSTRESS_AVATAR_FILE: {e}");
                }
            }
            continue;
        }
        let Some(token) = request.token.clone().filter(|t| !t.trim().is_empty()) else {
            spawn.write(SpawnAvatar::new(AvatarDescriptor::default(), request.at));
            continue;
        };
        let sender = pending.sender.clone();
        let generation = pending.generation;
        let at = request.at;
        std::thread::spawn(move || {
            let result = fetch_profile(&token);
            let _ = sender.send((generation, at, result));
        });
    }
}

fn fetch_profile(token: &str) -> Result<AvatarDescriptor, String> {
    #[derive(serde::Deserialize)]
    struct Response {
        descriptor: Option<AvatarDescriptor>,
    }
    let response = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .get("https://api.eustress.dev/api/avatar")
        .set("Authorization", &format!("Bearer {}", token.trim()))
        .call()
        .map_err(|e| format!("profile request failed: {e}"))?;
    use std::io::Read;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(32769)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 32768 {
        return Err("Avatar response exceeds 32 KiB".into());
    }
    let descriptor = serde_json::from_slice::<Response>(&bytes)
        .map_err(|e| e.to_string())?
        .descriptor
        .unwrap_or_default();
    descriptor.validate()?;
    Ok(descriptor)
}

fn finish_profile(pending: Res<ProfileRequests>, mut spawn: MessageWriter<SpawnAvatar>) {
    let Ok(receiver) = pending.receiver.lock() else {
        return;
    };
    while let Ok((generation, at, result)) = receiver.try_recv() {
        if generation != pending.generation {
            continue;
        }
        let descriptor = match result {
            Ok(descriptor) => descriptor,
            Err(error) => {
                tracing::warn!("avatar: {error}; spawning the default avatar for this session");
                AvatarDescriptor::default()
            }
        };
        spawn.write(SpawnAvatar::new(descriptor, at));
    }
}
