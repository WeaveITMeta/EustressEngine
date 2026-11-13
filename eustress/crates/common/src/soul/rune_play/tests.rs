//! Tests for the Player's Rune driver ([`super`]).

use super::*;
use crate::datamodel::DmValue;
use std::sync::{Arc, Mutex};

/// The driver publishes its tree to the process-wide `datamodel::active` and
/// drains the process-wide script log, so the tests that run it take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

const SOUL_SERVICE: &str = "[service]\nclass_name = \"SoulService\"\n";
const WORKSPACE: &str = "[service]\nclass_name = \"Workspace\"\n";
const STARTER_PLAYER_SCRIPTS: &str = "[service]\nclass_name = \"StarterPlayerScripts\"\n";

/// A `SoulScript` folder as Studio writes one (the Super Station's
/// `StationMind` record), whose source is the file `source_file` names.
fn soul_script(name: &str, source_file: &str) -> String {
    format!(
        "[metadata]\nclass_name = \"SoulScript\"\narchivable = true\nname = \"{name}\"\n\n\
         [properties]\nlanguage = \"rune\"\n\n[script]\nenabled = true\nrun_context = \"Rune\"\nsource = \"{source_file}\"\n"
    )
}

/// A Luau client script folder, as the station's console is recorded.
fn local_script(name: &str, source_file: &str) -> String {
    format!(
        "[metadata]\nclass_name = \"SoulScript\"\narchivable = true\nname = \"{name}\"\n\n\
         [properties]\nlanguage = \"luau\"\n\n[script]\nenabled = true\nrun_context = \"Client\"\nsource = \"{source_file}\"\n"
    )
}

/// An anchored part named `name`, with a `Ticks` attribute.
fn part(name: &str) -> String {
    format!(
        "[metadata]\nclass_name = \"Part\"\nname = \"{name}\"\n\n[properties]\nanchored = true\n\
         color = [1.0, 0.0, 0.0, 1.0]\n\n[attributes]\nTicks = 0.0\n\n\
         [transform]\nposition = [0.0, 1.0, 0.0]\nrotation = [0.0, 0.0, 0.0, 1.0]\nscale = [2.0, 2.0, 2.0]\n"
    )
}

fn records(files: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    files.iter().map(|(k, v)| (k.to_string(), v.as_bytes().to_vec())).collect()
}

/// A world opened as the Player opens a published one.
fn world(files: &[(&str, &str)]) -> PlayDataModel {
    let opened = crate::tree_read::open_player_world(&records(files), "Tester");
    PlayDataModel { dm: Arc::new(parking_lot::Mutex::new(opened.dm)) }
}

fn app(scope: RuneScope) -> App {
    let mut app = App::new();
    app.init_resource::<Time>().insert_resource(scope).add_message::<AppExit>().add_plugins(TreeRunePlugin);
    let mut registry = RuneModuleRegistry::default();
    for module in player_rune_modules() {
        registry.add_module(module);
    }
    app.insert_resource(registry);
    app
}

/// `app` with the Player's LocalScript VM beside the Rune driver.
#[cfg(feature = "mlua")]
fn app_with_luau(scope: RuneScope) -> App {
    let mut app = app(scope);
    app.add_message::<bevy::input::mouse::MouseWheel>().add_plugins(crate::tree_scripts::TreeScriptsPlugin);
    #[cfg(feature = "physics")]
    app.init_resource::<avian3d::collider_tree::ColliderTrees>();
    app
}

/// `n` frames of 1/60 s each, the tree's clock moved as the Play runtime's
/// pull moves a replica's.
fn frames(app: &mut App, dm: &SharedDataModel, n: usize) {
    for _ in 0..n {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_secs_f64(1.0 / 60.0));
        {
            let mut g = dm.lock();
            g.frame.dt = 1.0 / 60.0;
            g.frame.time += 1.0 / 60.0;
        }
        app.update();
    }
}

fn output(dm: &SharedDataModel) -> Vec<(OutputLevel, String, String)> {
    dm.lock().output.iter().map(|l| (l.level, l.source.clone(), l.text.clone())).collect()
}

fn errors(dm: &SharedDataModel) -> Vec<(OutputLevel, String, String)> {
    output(dm).into_iter().filter(|l| l.0 == OutputLevel::Error).collect()
}

fn find(dm: &SharedDataModel, path: &str) -> Option<InstanceId> {
    let g = dm.lock();
    let mut cur = g.root();
    for seg in path.split('.') {
        let next = if cur == g.root() { g.find_service(seg) } else { None };
        cur = next.or_else(|| g.find_first_child(cur, seg, false))?;
    }
    Some(cur)
}

fn attr(dm: &SharedDataModel, path: &str, name: &str) -> Option<DmValue> {
    let id = find(dm, path)?;
    dm.lock().get_attribute(id, name)
}

const MOVER: &str = r#"
use eustress::dm;
use eustress::Vector3;

pub fn on_update(dt) {
    let b = dm::find_path("Workspace.Box");
    if b == 0 { return; }
    dm::set_attribute_number(b, "Ticks", dm::get_attribute_number(b, "Ticks") + 1.0);
    dm::set_color(b, 0.0, 1.0, 0.0);
    let p = dm::get_position(b);
    dm::set_vector3(b, "Size", Vector3::new(3.0, 3.0, 3.0));
    dm::set_attribute_number(b, "SeenY", p.y);
}

pub fn on_exit() {
    dm::set_attribute_bool(dm::find_path("Workspace.Box"), "Exited", true);
}
"#;

/// Discovery follows Studio's compile rule (source, and a `.rune` or
/// `.soul` file), and a joined Player keeps only its own scripts.
#[test]
fn discovery_takes_studios_scripts_and_a_joined_player_only_its_own() {
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/Mind/_instance.toml", &soul_script("Mind", "Mind.rune")),
        ("SoulService/Mind/Mind.rune", "pub fn on_update(dt) {}"),
        ("SoulService/Mind/Mind.md", "# notes"),
        ("SoulService/Notes/_instance.toml", &soul_script("Notes", "Notes.md")),
        ("SoulService/Notes/Notes.md", "# prose, not a program"),
        ("SoulService/Blank/_instance.toml", &soul_script("Blank", "Blank.rune")),
        ("SoulService/Blank/Blank.rune", "  "),
        ("StarterPlayerScripts/_service.toml", STARTER_PLAYER_SCRIPTS),
        ("StarterPlayerScripts/Hud.rune", "pub fn on_update(dt) {}"),
        ("Workspace/_service.toml", WORKSPACE),
        ("Workspace/Spinner.soul", "pub fn on_update(dt) {}"),
    ]);
    let g = w.dm.lock();
    let (all, skipped) = tree_rune_scripts(&g, RuneScope::Everything);
    let names: Vec<&str> = all.iter().map(|s| s.chunk_name.as_str()).collect();
    assert_eq!(names, ["SoulService.Mind", "StarterPlayerScripts.Hud", "Workspace.Spinner"]);
    assert_eq!(all[0].file.as_deref(), Some("SoulService/Mind/Mind.rune"), "the tree records a Rune script's file");
    assert_eq!(all[0].name, "Mind");
    let skipped: Vec<(&str, &str)> = skipped.iter().map(|s| (s.chunk_name.as_str(), s.reason.as_str())).collect();
    assert_eq!(
        skipped,
        [("SoulService.Blank", "its source is empty"), ("SoulService.Notes", "its file is .md, not .rune or .soul")]
    );

    let (mine, _) = tree_rune_scripts(&g, RuneScope::ClientOnly);
    let names: Vec<&str> = mine.iter().map(|s| s.chunk_name.as_str()).collect();
    assert_eq!(names, ["StarterPlayerScripts.Hud"], "the host runs SoulService and Workspace Rune");
}

/// Solo: the script compiles as the tree opens, starts ("Started" in
/// Output, as a Luau start reads), writes the tree through `eustress::dm`
/// every frame, and those writes are what the draw step takes
/// (`take_dirty`). Closing the world runs `on_exit` against it.
#[test]
fn a_script_starts_writes_the_tree_and_stops_with_its_world() {
    let _turn = lock();
    let mut app = app(RuneScope::Everything);
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/Mover/_instance.toml", &soul_script("Mover", "Mover.rune")),
        ("SoulService/Mover/Mover.rune", MOVER),
        ("Workspace/_service.toml", WORKSPACE),
        ("Workspace/Box.part.toml", &part("Box")),
    ]);
    let dm = w.dm.clone();
    let b = find(&dm, "Workspace.Box").expect("the box");
    dm.lock().take_dirty();
    app.insert_resource(w);
    frames(&mut app, &dm, 3);

    assert_eq!(attr(&dm, "Workspace.Box", "Ticks"), Some(DmValue::Number(3.0)), "on_update ran once a frame");
    let color = dm.lock().get_prop(b, "Color").and_then(|v| v.as_color3()).expect("a colour");
    assert_eq!((color.r, color.g, color.b), (0.0, 1.0, 0.0));
    let size = dm.lock().get_prop(b, "Size").and_then(|v| v.as_vector3()).expect("a size");
    assert_eq!((size.x, size.y, size.z), (3.0, 3.0, 3.0), "eustress::Vector3 reached the tree");
    let dirty = dm.lock().take_dirty();
    let drawn: Vec<&String> = dirty.iter().filter(|(id, _)| *id == b).flat_map(|(_, p)| p).collect();
    assert!(
        drawn.iter().any(|p| *p == "Color") && drawn.iter().any(|p| *p == "Size"),
        "the draw step gets the writes: {dirty:?}"
    );
    let out = output(&dm);
    assert!(
        out.iter().any(|l| l.0 == OutputLevel::Info && l.1 == "SoulService.Mover" && l.2 == "Started"),
        "a start line: {out:?}"
    );
    assert!(errors(&dm).is_empty(), "{out:?}");
    assert_eq!(app.world().resource::<TreeRune>().running(), ["SoulService.Mover"]);

    // The world closes: on_exit runs with it published, then nothing runs.
    app.world_mut().remove_resource::<PlayDataModel>();
    frames(&mut app, &dm, 1);
    assert_eq!(attr(&dm, "Workspace.Box", "Exited"), Some(DmValue::Bool(true)), "on_exit reached the tree");
    assert!(app.world().resource::<TreeRune>().running().is_empty());
    assert!(crate::datamodel::active().is_none(), "the tree is published only while scripts run");
    frames(&mut app, &dm, 2);
    assert_eq!(attr(&dm, "Workspace.Box", "Ticks"), Some(DmValue::Number(3.0)), "a closed world's script stops");
}

/// The app closing gives the running scripts their `on_exit`.
#[test]
fn closing_the_app_runs_on_exit() {
    let _turn = lock();
    let mut app = app(RuneScope::Everything);
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/Mover.rune", MOVER),
        ("Workspace/_service.toml", WORKSPACE),
        ("Workspace/Box.part.toml", &part("Box")),
    ]);
    let dm = w.dm.clone();
    app.insert_resource(w);
    frames(&mut app, &dm, 1);
    assert_eq!(attr(&dm, "Workspace.Box", "Exited"), None);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert_eq!(attr(&dm, "Workspace.Box", "Exited"), Some(DmValue::Bool(true)));
}

/// A broken script says why in Output, under its own name and line, and does
/// not stop the others.
#[test]
fn a_compile_error_is_reported_and_the_rest_still_run() {
    let _turn = lock();
    let mut app = app(RuneScope::Everything);
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/Broken/_instance.toml", &soul_script("Broken", "Broken.rune")),
        ("SoulService/Broken/Broken.rune", "pub fn on_update(dt) {\n    let x = ;\n}\n"),
        ("SoulService/Mover/_instance.toml", &soul_script("Mover", "Mover.rune")),
        ("SoulService/Mover/Mover.rune", MOVER),
        ("Workspace/_service.toml", WORKSPACE),
        ("Workspace/Box.part.toml", &part("Box")),
    ]);
    let dm = w.dm.clone();
    app.insert_resource(w);
    frames(&mut app, &dm, 2);
    let errs = errors(&dm);
    assert!(
        errs.iter().any(|e| e.1 == "SoulService.Broken" && e.2.starts_with("Broken:2:")),
        "the compile error names its script and line: {errs:?}"
    );
    assert_eq!(attr(&dm, "Workspace.Box", "Ticks"), Some(DmValue::Number(2.0)), "the other script runs");
}

/// A runtime error reaches Output with its line, as Studio reports it.
#[test]
fn a_runtime_error_is_reported_with_its_line() {
    let _turn = lock();
    let mut app = app(RuneScope::Everything);
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/Bad/_instance.toml", &soul_script("Bad", "Bad.rune")),
        ("SoulService/Bad/Bad.rune", "pub fn on_update(dt) {\n    let n = 1;\n    let f = n + 0.5;\n}\n"),
    ]);
    let dm = w.dm.clone();
    app.insert_resource(w);
    frames(&mut app, &dm, 1);
    let errs = errors(&dm);
    assert!(errs.iter().any(|e| e.1 == "SoulService.Bad" && e.2.starts_with("on_update")), "{errs:?}");
    let line = dm.lock().output.iter().find(|l| l.level == OutputLevel::Error).map(|l| l.line);
    assert_eq!(line, Some(3), "the error points at the line that failed");
}

/// Joined: the host's Rune (SoulService) does not run here; this player's
/// own does, including one the host replicates in later.
#[test]
fn a_joined_player_runs_only_its_own_rune_and_picks_up_arrivals() {
    let _turn = lock();
    let mut app = app(RuneScope::ClientOnly);
    let tick = |name: &str| {
        format!(
            "use eustress::dm;\npub fn on_update(dt) {{\n    let b = dm::find_path(\"Workspace.Box\");\n    \
             dm::set_attribute_number(b, \"{name}\", dm::get_attribute_number(b, \"{name}\") + 1.0);\n}}\n"
        )
    };
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/HostMind.rune", &tick("Host")),
        ("StarterPlayerScripts/_service.toml", STARTER_PLAYER_SCRIPTS),
        ("StarterPlayerScripts/Mine.rune", &tick("Mine")),
        ("Workspace/_service.toml", WORKSPACE),
        ("Workspace/Box.part.toml", &part("Box")),
    ]);
    let dm = w.dm.clone();
    app.insert_resource(w);
    frames(&mut app, &dm, 2);
    assert_eq!(attr(&dm, "Workspace.Box", "Mine"), Some(DmValue::Number(2.0)), "{:?}", output(&dm));
    assert_eq!(attr(&dm, "Workspace.Box", "Host"), None, "the host runs SoulService's Rune, not this Player");

    // The host replicates a script into this player's PlayerScripts.
    {
        let mut g = dm.lock();
        let me = g.local_player.expect("a local player");
        let scripts = g.create_virtual("PlayerScripts", "PlayerScripts", Some(me));
        let s = g.create_virtual("SoulScript", "Late", Some(scripts));
        g.set_prop(s, "Source", DmValue::String(tick("Late"))).unwrap();
    }
    frames(&mut app, &dm, 2);
    assert!(attr(&dm, "Workspace.Box", "Late").is_some(), "{:?}", output(&dm));
    assert_eq!(attr(&dm, "Workspace.Box", "Mine"), Some(DmValue::Number(4.0)), "the first script was not restarted");
    assert!(errors(&dm).is_empty(), "{:?}", output(&dm));
}

/// The station's conversation, in small: a Luau client script writes the
/// question to attributes, the Rune solver answers into another, and the
/// Luau script hears the answer, all on the Player's one tree.
#[cfg(feature = "mlua")]
#[test]
fn luau_asks_and_rune_answers_through_attributes() {
    let _turn = lock();
    let mut app = app_with_luau(RuneScope::Everything);
    let station = "[metadata]\nclass_name = \"Folder\"\nname = \"Station\"\n\n\
                   [attributes]\nQuery = \"\"\nQuerySeq = 0.0\nSeenSeq = 0.0\nAnswer = \"\"\n";
    let console = r#"
local station = workspace:WaitForChild("Station")
station:GetAttributeChangedSignal("Answer"):Connect(function()
    print("console heard: " .. station:GetAttribute("Answer"))
end)
station:SetAttribute("Query", "where do rays meet")
station:SetAttribute("QuerySeq", (station:GetAttribute("QuerySeq") or 0) + 1)
"#;
    let mind = r#"
use eustress::dm;

pub fn on_update(dt) {
    let root = dm::find_path("Workspace.Station");
    if root == 0 { return; }
    let seq = dm::get_attribute_number(root, "QuerySeq");
    if seq != dm::get_attribute_number(root, "SeenSeq") {
        dm::set_attribute_number(root, "SeenSeq", seq);
        let q = dm::get_attribute_string(root, "Query");
        let words = 0;
        for w in q.split(" ") { words += 1; }
        dm::set_attribute_string(root, "Answer", `${words} words: ${q}`);
        dm::log(`answered #${seq}`);
    }
}
"#;
    let w = world(&[
        ("SoulService/_service.toml", SOUL_SERVICE),
        ("SoulService/StationMind/_instance.toml", &soul_script("StationMind", "StationMind.rune")),
        ("SoulService/StationMind/StationMind.rune", mind),
        ("StarterPlayerScripts/_service.toml", STARTER_PLAYER_SCRIPTS),
        ("StarterPlayerScripts/StationConsole/_instance.toml", &local_script("StationConsole", "StationConsole.client.luau")),
        ("StarterPlayerScripts/StationConsole/StationConsole.client.luau", console),
        ("Workspace/_service.toml", WORKSPACE),
        ("Workspace/Station/_instance.toml", station),
    ]);
    let dm = w.dm.clone();
    app.insert_resource(w);
    frames(&mut app, &dm, 4);
    let out = output(&dm);
    assert_eq!(
        attr(&dm, "Workspace.Station", "Answer"),
        Some(DmValue::String("4 words: where do rays meet".into())),
        "{out:?}"
    );
    assert!(out.iter().any(|l| l.2 == "console heard: 4 words: where do rays meet"), "Luau heard Rune: {out:?}");
    assert!(out.iter().any(|l| l.1 == "Rune" && l.2 == "answered #1.0"), "dm::log reached Output: {out:?}");
    assert!(errors(&dm).is_empty(), "{out:?}");
}

/// The real station, when `EUSTRESS_STATION_SPACE` names a copy of its
/// Space (the folder a Player wrote the downloaded world to): StationMind
/// starts with no error, builds its data, and answers a question asked the
/// way StationConsole's `submit` asks it (`Query`, then `QuerySeq + 1`).
#[cfg(feature = "mlua")]
#[test]
fn the_super_station_answers_a_question() {
    let Some(space) = std::env::var_os("EUSTRESS_STATION_SPACE") else { return };
    let _turn = lock();
    let mut app = app_with_luau(RuneScope::Everything);
    let opened = crate::tree_read::open_player_world_dir(std::path::Path::new(&space), "Tester").expect("the Space opens");
    let w = PlayDataModel { dm: Arc::new(parking_lot::Mutex::new(opened.dm)) };
    let dm = w.dm.clone();
    app.insert_resource(w);
    let rune_lines = |dm: &SharedDataModel, errors_only: bool| -> Vec<String> {
        dm.lock()
            .output
            .iter()
            .filter(|l| !errors_only || l.level == OutputLevel::Error)
            .filter(|l| l.source.contains("StationMind") || l.source == "Rune")
            .map(|l| format!("{:?} [{}] {}", l.level, l.source, l.text))
            .collect()
    };
    frames(&mut app, &dm, 30);
    assert!(rune_lines(&dm, true).is_empty(), "{:?}", rune_lines(&dm, true));
    assert_eq!(attr(&dm, "Workspace.SuperStation", "HasData"), Some(DmValue::Bool(true)), "{:?}", output(&dm));

    let before = attr(&dm, "Workspace.SuperStation", "Answer");
    {
        let station = find(&dm, "Workspace.SuperStation").expect("the station");
        let mut g = dm.lock();
        let seq = g.get_attribute(station, "QuerySeq").and_then(|v| v.as_number()).unwrap_or(0.0) + 1.0;
        g.set_attribute(station, "Query", DmValue::String("How do I learn to script a physics simulation in Eustress?".into()))
            .unwrap();
        g.set_attribute(station, "QuerySeq", DmValue::Number(seq)).unwrap();
    }
    // The rings turn and settle, then the reading lands (phase 8).
    let mut answered = false;
    for _ in 0..60 * 30 {
        frames(&mut app, &dm, 1);
        if attr(&dm, "Workspace.SuperStation", "AnswerSeq").and_then(|v| v.as_number()) == Some(1.0) {
            answered = true;
            break;
        }
    }
    let after = attr(&dm, "Workspace.SuperStation", "Answer");
    for line in rune_lines(&dm, false) {
        println!("{line}");
    }
    assert!(answered, "no reading within 30 s of play");
    assert_ne!(after, before, "the reading changed");
    println!("Answer before: {before:?}\nAnswer after: {after:?}");
    assert!(rune_lines(&dm, true).is_empty(), "{:?}", rune_lines(&dm, true));
}
