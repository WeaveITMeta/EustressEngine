//! Gallery, sign-in, download, and join.
//!
//! HTTP and the world download run on worker threads. The Bevy frame only
//! reads a channel. Joining a host uses [`eustress_networking::native::start_join`]
//! and [`eustress_networking::begin_join`], the same pair the desktop Player uses.
//! The published-world download is [`eustress_networking::published::fetch_published`].

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use eustress_common::api_base;
use eustress_common::input_actions::InputState;
use eustress_networking::published::{self, DirChunkCache};
use eustress_networking::session::{begin_join, ChunkCacheRes, NetNotice, WorldDownloaded};
use eustress_networking::JoinLink;

use crate::gallery::{self, Card};
use crate::links::{self, LaunchKind};
use crate::platform;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Gallery,
    Playing,
}

#[derive(Resource)]
pub struct MobileApp {
    pub mode: Mode,
    pub status: String,
    pub account: String,
    pub cards: Vec<Card>,
    pub show_sign_in: bool,
    pub search: String,
    pub playing: bool,
    /// Name written into the opened tree's local Player. A join link sets it.
    /// A published Space uses `Player`.
    pub player_name: String,
    gallery_rx: Option<Mutex<Receiver<Result<Vec<Card>, String>>>>,
    auth_rx: Option<Mutex<Receiver<Result<crate::auth::SignedIn, String>>>>,
    fetch_rx: Option<Mutex<Receiver<Result<eustress_networking::session::DownloadedWorld, String>>>>,
    restore_rx: Option<Mutex<Receiver<Result<Option<crate::auth::SignedIn>, String>>>>,
    seen_intent: String,
    launch_done: bool,
    /// The avatar for the open Space has been requested. The Space opener
    /// clears this when a new world replaces the previous one.
    pub(crate) spawned: bool,
    /// A host session is running, as opposed to a published Space played solo.
    pub(crate) joined: bool,
    token: Option<String>,
}

impl Default for MobileApp {
    fn default() -> Self {
        Self {
            mode: Mode::Gallery,
            status: "Loading the gallery…".into(),
            account: "Signed out".into(),
            cards: Vec::new(),
            show_sign_in: false,
            search: String::new(),
            playing: false,
            player_name: "Player".into(),
            gallery_rx: None,
            auth_rx: None,
            fetch_rx: None,
            restore_rx: None,
            seen_intent: String::new(),
            launch_done: false,
            spawned: false,
            joined: false,
            token: None,
        }
    }
}

pub enum Command {
    Search(String),
    Play(String),
    SignIn,
    SubmitIdentity(String),
    CancelSignIn,
    Back,
}

pub fn data_root() -> PathBuf {
    platform::files_dir().join("eustress")
}

pub fn startup(mut app: ResMut<MobileApp>, mut input: ResMut<InputState>) {
    input.capture_all = true;
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        input.touch_zones = true;
    }
    let api = api_base::api_base().to_string();
    app.restore_rx = Some(Mutex::new(spawn_restore()));
    app.gallery_rx = Some(Mutex::new(spawn_gallery(api, String::new())));
}

pub fn drive(world: &mut World) {
    let commands = crate::shell::take_commands();
    for command in commands {
        apply(world, command);
    }
    poll(world);
    sync_chrome(world);
}

fn apply(world: &mut World, command: Command) {
    match command {
        Command::Search(text) => {
            let mut app = world.resource_mut::<MobileApp>();
            if app.search == text {
                return;
            }
            app.search = text.clone();
            app.status = "Searching…".into();
            let api = api_base::api_base().to_string();
            app.gallery_rx = Some(Mutex::new(spawn_gallery(api, text)));
        }
        Command::Play(id) => begin_published(world, id),
        Command::SignIn => {
            world.resource_mut::<MobileApp>().show_sign_in = true;
        }
        Command::SubmitIdentity(text) => {
            let mut app = world.resource_mut::<MobileApp>();
            app.status = "Signing in…".into();
            app.auth_rx = Some(Mutex::new(spawn_auth(text)));
        }
        Command::CancelSignIn => {
            world.resource_mut::<MobileApp>().show_sign_in = false;
        }
        Command::Back => {
            let mut app = world.resource_mut::<MobileApp>();
            app.mode = Mode::Gallery;
            app.playing = false;
            app.spawned = false;
            app.status = "Gallery".into();
            if let Some(mut input) = world.get_resource_mut::<InputState>() {
                input.capture_all = true;
                input.release_touch();
            }
            world.write_message(eustress_common::avatar::DespawnAllAvatars);
        }
    }
}

fn begin_published(world: &mut World, id: String) {
    let mut app = world.resource_mut::<MobileApp>();
    if app.fetch_rx.is_some() {
        app.status = "A download is already running.".into();
        return;
    }
    app.playing = true;
    app.mode = Mode::Playing;
    app.status = format!("Downloading {id}…");
    let api = api_base::api_base().to_string();
    let token = app.token.clone();
    let root = data_root();
    app.fetch_rx = Some(Mutex::new(spawn_fetch(api, id, token, root)));
    if let Some(mut input) = world.get_resource_mut::<InputState>() {
        input.capture_all = false;
    }
}

fn poll(world: &mut World) {
    if let Some(uri) = platform::view_intent_uri() {
        let seen = world.resource::<MobileApp>().seen_intent.clone();
        if uri != seen {
            world.resource_mut::<MobileApp>().seen_intent = uri.clone();
            open_text(world, &uri);
        }
    }
    if !world.resource::<MobileApp>().launch_done {
        world.resource_mut::<MobileApp>().launch_done = true;
        let args: Vec<String> = std::env::args().collect();
        match links::parse_args(&args, api_base::api_base()) {
            Ok(launch) => open_launch(world, launch.kind),
            Err(err) => world.resource_mut::<MobileApp>().status = err,
        }
    }

    let gallery = world.resource_mut::<MobileApp>().gallery_rx.take();
    if let Some(rx) = gallery {
        match recv(&rx) {
            Ok(Ok(cards)) => {
                let mut app = world.resource_mut::<MobileApp>();
                let n = cards.len();
                app.cards = cards;
                if app.mode == Mode::Gallery {
                    app.status = format!("{n} spaces");
                }
            }
            Ok(Err(err)) => world.resource_mut::<MobileApp>().status = err,
            Err(TryRecvError::Empty) => world.resource_mut::<MobileApp>().gallery_rx = Some(rx),
            Err(TryRecvError::Disconnected) => world.resource_mut::<MobileApp>().status = "The gallery request stopped.".into(),
        }
    }

    let restore = world.resource_mut::<MobileApp>().restore_rx.take();
    if let Some(rx) = restore {
        match recv(&rx) {
            Ok(Ok(Some(signed))) => note_signed_in(world, signed),
            Ok(Ok(None)) => {}
            Ok(Err(err)) => world.resource_mut::<MobileApp>().status = err,
            Err(TryRecvError::Empty) => world.resource_mut::<MobileApp>().restore_rx = Some(rx),
            Err(TryRecvError::Disconnected) => {}
        }
    }

    let auth = world.resource_mut::<MobileApp>().auth_rx.take();
    if let Some(rx) = auth {
        match recv(&rx) {
            Ok(Ok(signed)) => {
                note_signed_in(world, signed);
                world.resource_mut::<MobileApp>().show_sign_in = false;
            }
            Ok(Err(err)) => world.resource_mut::<MobileApp>().status = err,
            Err(TryRecvError::Empty) => world.resource_mut::<MobileApp>().auth_rx = Some(rx),
            Err(TryRecvError::Disconnected) => world.resource_mut::<MobileApp>().status = "Sign-in stopped.".into(),
        }
    }

    let fetch = world.resource_mut::<MobileApp>().fetch_rx.take();
    if let Some(rx) = fetch {
        match recv(&rx) {
            Ok(Ok(world_dl)) => {
                let files: usize = world_dl.spaces.iter().map(|(_, records)| records.len()).sum::<usize>() + world_dl.assets.len();
                world.resource_mut::<MobileApp>().status = format!("Cached {files} files. Opening the Space…");
                world.write_message(WorldDownloaded(Arc::new(world_dl)));
            }
            Ok(Err(err)) => world.resource_mut::<MobileApp>().status = err,
            Err(TryRecvError::Empty) => world.resource_mut::<MobileApp>().fetch_rx = Some(rx),
            Err(TryRecvError::Disconnected) => world.resource_mut::<MobileApp>().status = "The download stopped.".into(),
        }
    }

    let mut notices = Vec::new();
    if let Some(mut reader) = world.get_resource_mut::<Messages<NetNotice>>() {
        for notice in reader.drain() {
            notices.push(notice);
        }
    }
    for notice in notices {
        let line = match notice {
            NetNotice::Joined { host_name, .. } => format!("Joined {host_name}"),
            NetNotice::JoinFailed { reason } => format!("Could not join: {reason}"),
            NetNotice::Disconnected { reason } => format!("Disconnected: {reason}"),
            NetNotice::Downloading { received, total } => format!("Receiving the world {received}/{total}"),
            _ => continue,
        };
        world.resource_mut::<MobileApp>().status = line;
    }
}

fn open_text(world: &mut World, uri: &str) {
    match links::parse_uri(uri, api_base::api_base()) {
        Ok(launch) => open_launch(world, launch.kind),
        Err(err) => world.resource_mut::<MobileApp>().status = err,
    }
}

fn open_launch(world: &mut World, kind: LaunchKind) {
    match kind {
        LaunchKind::Gallery => {}
        LaunchKind::Published { sim_id } => begin_published(world, sim_id),
        LaunchKind::Join { link, name, ticket } => begin_network(world, link, name, ticket),
    }
}

fn begin_network(world: &mut World, link: JoinLink, name: String, ticket: Option<String>) {
    if world.resource::<MobileApp>().joined {
        return;
    }
    let root = data_root();
    match eustress_networking::native::start_join(&link) {
        Ok(net) => {
            let mut commands = world.commands();
            commands.insert_resource(ChunkCacheRes(Box::new(DirChunkCache::new(root.join("echk")))));
            begin_join(&mut commands, net, &name, ticket);
            let mut app = world.resource_mut::<MobileApp>();
            app.joined = true;
            app.playing = true;
            app.mode = Mode::Playing;
            app.status = format!("Joining {}:{}…", link.host, link.port);
            app.player_name = name;
            if let Some(mut input) = world.get_resource_mut::<InputState>() {
                input.capture_all = false;
            }
        }
        Err(err) => world.resource_mut::<MobileApp>().status = err,
    }
}

fn note_signed_in(world: &mut World, signed: crate::auth::SignedIn) {
    let mut app = world.resource_mut::<MobileApp>();
    app.account = signed.username;
    app.token = Some(signed.token);
    app.status = format!("Signed in as {}", app.account);
}

fn sync_chrome(world: &mut World) {
    let playing = world.resource::<MobileApp>().playing;
    let show_sign_in = world.resource::<MobileApp>().show_sign_in;
    if let Some(mut input) = world.get_resource_mut::<InputState>() {
        input.insets = platform::insets();
        input.capture_all = !playing || show_sign_in;
        if let Some(mut focus) = world.get_resource_mut::<eustress_common::play_session::GuiKeyboardFocus>() {
            focus.0 = show_sign_in;
        }
    }
    let jump = world.resource::<eustress_common::input_actions::Actions>().pressed_this_frame("Jump");
    if jump {
        platform::haptic(eustress_bridge_core::HapticKind::ImpactMedium);
    }
    crate::shell::present(world);
}

fn recv<T>(slot: &Mutex<Receiver<T>>) -> Result<T, TryRecvError> {
    slot.lock().unwrap_or_else(|err| err.into_inner()).try_recv()
}

fn spawn_gallery(api: String, query: String) -> Receiver<Result<Vec<Card>, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("eustress-gallery".into())
        .spawn(move || {
            let _ = tx.send(gallery::fetch(&api, &query));
        })
        .ok();
    rx
}

fn spawn_auth(text: String) -> Receiver<Result<crate::auth::SignedIn, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("eustress-auth".into())
        .spawn(move || {
            let _ = tx.send(crate::auth::sign_in(&text));
        })
        .ok();
    rx
}

fn spawn_restore() -> Receiver<Result<Option<crate::auth::SignedIn>, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("eustress-auth-restore".into())
        .spawn(move || {
            let _ = tx.send(crate::auth::restore());
        })
        .ok();
    rx
}

fn spawn_fetch(
    api: String,
    id: String,
    token: Option<String>,
    root: PathBuf,
) -> Receiver<Result<eustress_networking::session::DownloadedWorld, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("eustress-fetch".into())
        .spawn(move || {
            let _ = std::fs::create_dir_all(&root);
            let result = published::fetch_published(&api, &id, token.as_deref(), &root);
            let _ = tx.send(result);
        })
        .ok();
    rx
}
