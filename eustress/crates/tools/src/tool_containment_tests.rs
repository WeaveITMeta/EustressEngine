//! Each tool refuses a path or a name that reaches outside what it may touch,
//! and never hands git an option or a hook. Every test here failed on the
//! tools before they shared [`crate::sandbox`].

use crate::{ToolContext, ToolHandler};
use serde_json::json;
use std::path::PathBuf;

/// A Universe with one Space (`Spaces/S`), its Workspace and its git folder,
/// and a folder beside the Universe that no tool may reach.
struct Fixture {
    root: PathBuf,
    universe: PathBuf,
    space: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("eus_containment_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let universe = root.join("U");
        let space = universe.join("Spaces").join("S");
        let outside = root.join("Outside");
        for dir in [space.join("Workspace"), space.join(".git").join("hooks"), outside.clone()] {
            std::fs::create_dir_all(dir).unwrap();
        }
        Fixture { root, universe, space, outside }
    }

    fn ctx(&self) -> ToolContext {
        // Constructed field by field: ToolContext does not implement Default.
        ToolContext {
            space_root: self.space.clone(),
            universe_root: self.universe.clone(),
            user_id: None,
            username: None,
            luau_executor: None,
            display_unit: None,
            cancelled: None,
            permissions: crate::capability::Permissions::full(),
        }
    }

    fn outside_is_empty(&self) -> bool {
        std::fs::read_dir(&self.outside).unwrap().next().is_none()
    }

    /// A real repository at the Space with one commit, or `None` on a machine
    /// without git.
    fn repo(&self) -> Option<()> {
        let git = |args: &[&str]| {
            crate::sandbox::git_command(&self.space)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|_| ())
        };
        git(&["init", "--quiet"])?;
        std::fs::write(self.space.join("a.txt"), "a").unwrap();
        git(&["add", "a.txt"])?;
        git(&["commit", "--quiet", "-m", "a"])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn write_file_never_writes_into_git() {
    let f = Fixture::new("write_git");
    for path in [
        "Spaces/S/.git/hooks/post-checkout",
        "Spaces/S/.GIT/hooks/post-checkout",
        "Spaces/S/.git./hooks/post-checkout",
    ] {
        let r = crate::file_tools::WriteFileTool.execute(json!({ "path": path, "content": "#!/bin/sh\n" }), &f.ctx());
        assert!(!r.success, "{path}: {}", r.content);
    }
    assert!(!f.space.join(".git/hooks/post-checkout").exists());
}

#[test]
fn write_file_never_overwrites_an_instance_record_in_any_case() {
    let f = Fixture::new("write_instance");
    let r = crate::file_tools::WriteFileTool.execute(
        json!({ "path": "Spaces/S/Workspace/Part/_INSTANCE.TOML", "content": "x" }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
}

#[test]
fn stage_file_change_never_reads_outside() {
    let f = Fixture::new("stage");
    let secret = f.outside.join("secret.txt");
    std::fs::write(&secret, "secret").unwrap();
    let r = crate::diff_tools::StageFileChangeTool.execute(
        json!({ "path": secret.to_string_lossy(), "change_type": "modify", "content": "x", "summary": "s" }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
}

#[test]
fn create_entity_stays_in_workspace() {
    let f = Fixture::new("create_parent");
    let r = crate::entity_tools::CreateEntityTool.execute(
        json!({ "name": "Escaped", "parent": f.outside.to_string_lossy() }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
    assert!(f.outside_is_empty(), "nothing may be created outside Workspace");
}

#[test]
fn update_and_delete_entity_take_one_name() {
    let f = Fixture::new("entity_name");
    let victim = f.space.join("Victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("_instance.toml"), "[metadata]\nclass_name = \"Part\"\n\n[properties]\n").unwrap();
    for name in [r"..\Victim".to_string(), victim.to_string_lossy().into_owned()] {
        let r = crate::entity_tools::UpdateEntityTool.execute(json!({ "name": name, "anchored": false }), &f.ctx());
        assert!(!r.success, "update {name}: {}", r.content);
        let r = crate::entity_tools::DeleteEntityTool.execute(json!({ "name": name }), &f.ctx());
        assert!(!r.success, "delete {name}: {}", r.content);
    }
    let doc = std::fs::read_to_string(victim.join("_instance.toml")).expect("the folder outside Workspace survives");
    assert!(!doc.contains("anchored"), "and is not edited");
}

/// An approved delete is a move into the Space trash, as Studio's Delete is:
/// the folder and everything in it can be taken back out.
#[test]
fn delete_entity_moves_the_folder_into_the_space_trash() {
    let f = Fixture::new("delete_trash");
    let part = f.space.join("Workspace").join("Crate");
    std::fs::create_dir_all(part.join("meshes")).unwrap();
    std::fs::write(part.join("_instance.toml"), "[metadata]\nclass_name = \"Part\"\n").unwrap();
    std::fs::write(part.join("meshes").join("crate.glb"), b"glb").unwrap();
    let r = crate::entity_tools::DeleteEntityTool.execute(json!({ "name": "Crate" }), &f.ctx());
    assert!(r.success, "{}", r.content);
    assert!(!part.exists(), "gone from the Workspace");
    let trashed = f.space.join(".eustress").join("trash").join("Crate");
    assert!(r.content.contains(&trashed.display().to_string()), "the reply names where it went: {}", r.content);
    assert_eq!(r.structured_data.unwrap()["trash"], trashed.to_string_lossy().as_ref());
    assert!(std::fs::read_to_string(trashed.join("_instance.toml")).unwrap().contains("class_name"));
    assert_eq!(std::fs::read(trashed.join("meshes").join("crate.glb")).unwrap(), b"glb", "and everything in it");
}

/// A trash that leads out of the Space (a junction a copied Space carried
/// in) is refused, and the part stays exactly where it was.
#[cfg(windows)]
#[test]
fn a_refused_trash_move_leaves_the_part_in_place() {
    let f = Fixture::new("delete_trash_junction");
    std::fs::create_dir_all(f.space.join(".eustress")).unwrap();
    let made = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(f.space.join(".eustress").join("trash"))
        .arg(&f.outside)
        .output()
        .unwrap();
    assert!(made.status.success(), "mklink: {}", String::from_utf8_lossy(&made.stdout));
    let part = f.space.join("Workspace").join("Crate");
    std::fs::create_dir_all(&part).unwrap();
    std::fs::write(part.join("_instance.toml"), "[metadata]\nclass_name = \"Part\"\n").unwrap();
    let r = crate::entity_tools::DeleteEntityTool.execute(json!({ "name": "Crate" }), &f.ctx());
    assert!(!r.success && r.content.contains("nothing was deleted"), "{}", r.content);
    assert_eq!(
        std::fs::read_to_string(part.join("_instance.toml")).unwrap(),
        "[metadata]\nclass_name = \"Part\"\n",
        "left exactly where it was"
    );
    assert_eq!(std::fs::read_dir(&f.outside).unwrap().count(), 0, "nothing reached the other folder");
    std::fs::remove_dir(f.space.join(".eustress").join("trash")).unwrap();
}

#[test]
fn website_remove_reference_removes_only_a_reference() {
    let f = Fixture::new("website");
    let victim = f.space.join("Victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::write(victim.join("_instance.toml"), "").unwrap();
    std::fs::create_dir_all(f.space.join("Website")).unwrap();
    let r = crate::website_tools::WebsiteRemoveReferenceTool.execute(json!({ "key": "../Victim" }), &f.ctx());
    assert!(!r.success, "{}", r.content);
    assert!(victim.join("_instance.toml").is_file(), "a folder outside the Website service survives");
}

#[test]
fn generate_docs_writes_only_markdown_in_the_space() {
    let f = Fixture::new("docs");
    let r = crate::script_tools::GenerateDocsTool.execute(
        json!({ "output_path": f.outside.join("README.md").to_string_lossy() }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
    assert!(f.outside_is_empty());
    std::fs::write(f.space.join("Notes.txt"), "keep").unwrap();
    let r = crate::script_tools::GenerateDocsTool.execute(json!({ "output_path": "Notes.txt" }), &f.ctx());
    assert!(!r.success, "{}", r.content);
    assert_eq!(std::fs::read_to_string(f.space.join("Notes.txt")).unwrap(), "keep");
}

#[test]
fn universe_and_session_names_are_one_name() {
    let f = Fixture::new("universe_names");
    let r = crate::universe_tools::SetDefaultUniverseTool.execute(
        json!({ "universe": f.outside.to_string_lossy() }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
    let r = crate::universe_tools::GetConversationTool.execute(json!({ "session_id": "../../../Outside/x" }), &f.ctx());
    assert!(!r.success && r.content.contains("Invalid"), "{}", r.content);
}

#[test]
fn an_experiment_name_is_one_name() {
    let f = Fixture::new("experiment");
    let r = crate::simulation_tools::RunExperimentTool.execute(
        json!({ "name": "../../Outside/x", "duration_s": 1 }),
        &f.ctx(),
    );
    assert!(!r.success && r.content.contains("Invalid experiment name"), "{}", r.content);
}

#[test]
fn feedback_diff_never_passes_an_option_to_git() {
    let f = Fixture::new("diff_option");
    if f.repo().is_none() {
        return; // No git on this machine.
    }
    let r = crate::git_tools::FeedbackDiffTool.execute(
        json!({ "base": format!("--output={}", f.outside.join("written").display()), "compare": "HEAD" }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
    assert!(f.outside_is_empty(), "git wrote a file");
}

#[test]
fn git_branch_never_passes_an_option_to_git() {
    let f = Fixture::new("branch_option");
    if f.repo().is_none() {
        return; // No git on this machine.
    }
    let r = crate::git_tools::GitBranchTool.execute(json!({ "action": "switch", "name": "--orphan=evil" }), &f.ctx());
    assert!(!r.success, "{}", r.content);
}

#[test]
fn git_tools_run_no_hook() {
    let f = Fixture::new("branch_hook");
    if f.repo().is_none() {
        return; // No git on this machine.
    }
    let hook = f.space.join(".git").join("hooks").join("post-checkout");
    std::fs::write(&hook, "#!/bin/sh\ntouch hook_ran\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let r = crate::git_tools::GitBranchTool.execute(json!({ "action": "create", "name": "next" }), &f.ctx());
    assert!(r.success, "{}", r.content);
    assert!(!f.space.join("hook_ran").exists(), "the post-checkout hook ran");
}

/// More output than the OS pipe holds. Read only after the command exited,
/// the command waited for a reader until the timeout killed it.
#[test]
fn run_bash_finishes_a_command_that_prints_a_lot() {
    let bash_runs = std::process::Command::new("bash")
        .args(["-c", "exit 0"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !bash_runs {
        return; // No bash on this machine.
    }
    let f = Fixture::new("bash_output");
    let r = crate::shell_tools::RunBashTool.execute(
        json!({ "command": "yes a | head -c 200000", "timeout_seconds": 30 }),
        &f.ctx(),
    );
    let data = r.structured_data.clone().unwrap_or_default();
    assert_eq!(data["timed_out"], json!(false), "{}", r.content);
    assert!(r.success, "{}", r.content);
    assert!(r.content.contains("200000 bytes total"), "{}", r.content);
}

#[test]
fn run_bash_stays_in_the_universe() {
    let f = Fixture::new("bash_cwd");
    let r = crate::shell_tools::RunBashTool.execute(
        json!({ "command": "exit 0", "cwd": "Spaces/S/.git" }),
        &f.ctx(),
    );
    assert!(!r.success, "{}", r.content);
}

/// A CAD part's name is one folder in its parent: never a path, `..`, a
/// drive, `.git`, a device name, or a quote that would break its TOML.
#[test]
fn cad_create_part_names_one_folder_in_its_parent() {
    let f = Fixture::new("cad_name");
    let escaped = f.outside.join("Escaped");
    let absolute = escaped.to_string_lossy().into_owned();
    for name in ["../../../../Outside/Escaped", absolute.as_str(), "..", ".git", "a/b", "a\\b", "CON", "x\"y", ""] {
        let r = crate::cad_tools::CadCreatePartTool.execute(json!({ "name": name }), &f.ctx());
        assert!(!r.success, "{name:?}: {}", r.content);
    }
    assert!(!escaped.exists(), "nothing is created outside the Universe");
    assert!(!f.space.join(".git").join("_instance.toml").exists());
    assert!(!f.space.join("Workspace").join(".git").exists());
}

/// A network path is refused by its text, so the engine never opens it
/// (opening one sends the person's sign-in challenge to the host it names).
#[cfg(windows)]
#[test]
fn absolute_paths_from_a_caller_never_reach_the_network() {
    let f = Fixture::new("unc");
    for path in [r"\\attacker-host\share\x.ply", "//attacker-host/share/x.ply", r"\\?\UNC\attacker-host\share\x.ply"] {
        let r = crate::entity_tools::InsertGaussianSplatsTool.execute(json!({ "path": path }), &f.ctx());
        assert!(!r.success && r.content.contains("path refused"), "{path}: {}", r.content);
        let r = crate::universe_tools::ListSpacesTool.execute(json!({ "universe": path }), &f.ctx());
        assert!(!r.success && r.content.contains("Invalid Universe path"), "{path}: {}", r.content);
    }
}

/// A repository that holds the Universe (one the upward walk finds above
/// it) is read, but never committed to, branched or merged by a tool.
#[test]
fn git_changes_only_a_repository_inside_the_universe() {
    let f = Fixture::new("git_above");
    std::fs::remove_dir_all(f.space.join(".git")).unwrap();
    std::fs::create_dir_all(f.root.join(".git")).unwrap();
    let ctx = f.ctx();
    let commit = crate::git_tools::GitCommitTool.execute(json!({ "message": "m" }), &ctx);
    assert!(!commit.success && commit.content.contains("person's to make"), "{}", commit.content);
    for action in ["create", "switch", "delete", "merge"] {
        let r = crate::git_tools::GitBranchTool.execute(json!({ "action": action, "name": "feature" }), &ctx);
        assert!(!r.success && r.content.contains("person's to make"), "{action}: {}", r.content);
    }
}

/// Every tool source, with the file writes, write-helper calls and process
/// launches it makes outside its tests, and how they are confined (an audit
/// of 2026-10-01). Every path written is either the return of
/// `sandbox::resolve` on the caller's input, or built only from the Space and
/// Universe roots and names that passed a check. A file that writes through
/// another crate or the engine says whether the caller's input names the
/// place.
const WRITES: &[(&str, usize, &str)] = &[
    ("ai_camera_tools.rs", 0, "the engine writes its capture to a fixed .eustress file, read back here; the caller names no path"),
    ("cad_tools.rs", 12, "resolved paths; the new part's folder, named through sandbox::plain_name; <universe>/.eustress/assets/cad/<id>, the id passing is_valid_library_id and plain_name"),
    ("capability.rs", 0, "no files"),
    ("data_tools.rs", 0, "reads only, through sandbox::resolve"),
    ("diff_tools.rs", 3, "the staged path, resolved again when the change is applied"),
    ("embedvec_tools.rs", 0, "computes and reads; writes nothing"),
    ("entity_tools.rs", 8, "resolved paths; a Workspace folder named by sanitize_entity_name; delete_entity moves into <space>/.eustress/trash (space_trash::move_to_trash), never a permanent removal; <universe>/assets/splats/<file name>.ply, copied from a resolved path or a local-drive one (sandbox::local_absolute)"),
    ("file_tools.rs", 2, "write_file's resolved path; an _instance.toml is refused"),
    ("git_tools.rs", 1, "one git process (sandbox::git_command, hooks off); add, commit and branch changes only in a repository inside the Universe (git_write); names pass git_branch_name or follow --"),
    ("image_result.rs", 0, "reads the file the engine names"),
    ("import_tools.rs", 5, "the GLB copied to a resolved Space path through a temporary file beside it (create_dir_all, copy, rename, and remove_file of that temporary on failure), its source a resolved Universe path; Parts through create_instance in a resolved folder under Workspace"),
    ("lib.rs", 0, "no files"),
    ("memory_tools.rs", 0, "remember returns its key and value to the Workshop, which stores them; the tool writes nothing; rules are read from .md files"),
    ("mesh_tools.rs", 0, "reads only: the Part's own mesh and record; the engine writes the pieces and moves the original"),
    ("moderation_tools.rs", 0, "the moderation API over the network; no files"),
    ("modes.rs", 0, "no files"),
    ("particle_sim_tools.rs", 3, "section files and instances in a resolved folder, or in Workspace"),
    ("physics_tools.rs", 0, "computes only"),
    ("registry.rs", 0, "no files"),
    ("sandbox.rs", 2, "git_command, the one builder of a git process, with every hook off"),
    ("script_tools.rs", 14, "SoulService/<slug_safe name>, Workspace/.generated/<sanitized name>, and a resolved .md path"),
    ("shell_tools.rs", 2, "run_bash's shell, in a resolved working folder; what the command writes is not confined, so it is Execute class and a person approves each command"),
    ("sim_ipc.rs", 1, "the engine's own command queue, <universe>/.eustress/sim-commands.jsonl or a registered instance's"),
    ("simulation_tools.rs", 6, "<universe>/.eustress/datastore/<[A-Za-z0-9_-] name>.json, a resolved tag file, .eustress/experiments/<plain_name>-<time>.json, and one git process making an experiment branch only in a repository inside the Universe"),
    ("spatial_tools.rs", 0, "reads only"),
    ("universe_tools.rs", 31, "Universe and Space folders whose names pass plain_name and the reserved-name check, the files a new Space is made with, and .default_universe; a Universe named by path must be on a local drive (sandbox::local_absolute)"),
    ("website_tools.rs", 5, "Space/Website/_service.toml and Website/<[A-Za-z0-9_] key> folders"),
];

/// What counts: the std and tokio file calls, this crate's helpers that wrap
/// them, links and permissions, and launching a process (a process can write
/// anything).
///
/// What the count does NOT do, so a reviewer knows: it counts calls per
/// file, so a change to the PATH an existing call writes passes unnoticed,
/// and any diff in a listed file still needs a person to read it; it sees a
/// call only by these spellings (imports that rename them are refused by
/// `file_and_process_modules_are_never_imported_under_another_name`); and it
/// finds a test module's end by counting braces, which is why a test module
/// must come last in its file (`without_tests` panics otherwise).
const WRITE_CALLS: &[&str] = &[
    "fs::write(", "File::create(", "OpenOptions::new(", "tokio::fs::", "fs::rename(", "fs::remove_file(",
    "fs::remove_dir(", "fs::remove_dir_all(", "fs::copy(", "fs::create_dir(", "fs::create_dir_all(",
    "save_toml_file(", "create_instance(", "write_glb(", "append_json_line(",
    "File::options(", "Command::new(", "git_command(", "move_to_trash(",
    "fs::set_permissions(", "fs::hard_link(", "symlink_file(", "symlink_dir(", "create_new(",
];

/// `text` without its `#[cfg(test)]` items: each is skipped from its
/// attribute to the brace that closes the item, or to the `;` that ends a
/// one-line item. Braces are counted without reading strings or comments,
/// so a block item must be followed only by other `#[cfg(test)]` items: a
/// lone brace in a test's string could otherwise swallow the code after it.
fn without_tests(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[cfg(test)]") {
            let (mut depth, mut opened) = (0i32, false);
            while i < lines.len() {
                for c in lines[i].chars() {
                    match c {
                        '{' => {
                            depth += 1;
                            opened = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                i += 1;
                if opened && depth <= 0 {
                    let next = lines[i..].iter().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("//"));
                    assert!(
                        next.map_or(true, |l| l.starts_with("#[cfg(test)]")),
                        "a #[cfg(test)] block is followed by other code ({}): move the tests to the end of the file, \
                         so the write count knows where they stop",
                        next.unwrap_or("")
                    );
                    break;
                }
                if !opened && lines[i - 1].trim_end().ends_with(';') {
                    break;
                }
            }
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

fn write_calls(text: &str) -> usize {
    let body = without_tests(&text.replace("\r\n", "\n"));
    WRITE_CALLS.iter().map(|p| body.matches(p).count()).sum()
}

/// Each `use` statement in `text`, its lines joined, so an import spread over
/// several lines is read whole.
fn use_statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if current.is_none() {
            let l = l.strip_prefix("pub ").unwrap_or(l);
            let l = l.strip_prefix("pub(crate) ").unwrap_or(l);
            if l.starts_with("use ") {
                current = Some(String::new());
            }
        }
        if let Some(statement) = current.as_mut() {
            statement.push_str(l);
            statement.push(' ');
            if l.contains(';') {
                out.push(current.take().unwrap());
            }
        }
    }
    out
}

/// Every tool source but this one, by file name.
fn tool_sources() -> Vec<(String, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&src)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), p))
        .filter(|(name, _)| name != "tool_containment_tests.rs")
        .map(|(name, p)| (name, std::fs::read_to_string(p).unwrap()))
        .collect();
    out.sort();
    out
}

/// Every way a tool reaches a file is accounted for: the protection is only
/// as good as the weakest of them. A write call or process launch added (or
/// removed) anywhere in the crate, or a new source file, fails here until it
/// is reviewed (through sandbox::resolve, or a path built only from the
/// roots and checked names) and [`WRITES`] is updated.
#[test]
fn every_file_write_is_accounted_for() {
    let sources = tool_sources();
    for (name, text) in &sources {
        let Some((_, listed, _)) = WRITES.iter().find(|(file, _, _)| *file == name.as_str()) else {
            panic!("{name} is not in WRITES: list it, with its write calls and how they are confined (0 and why, if none)");
        };
        let count = write_calls(text);
        assert_eq!(
            count, *listed,
            "{name} makes {count} write call(s) or process launch(es) outside its tests and WRITES lists {listed}: \
             review each new one (through sandbox::resolve, or a path built only from the Space and Universe roots \
             and checked names), then update WRITES"
        );
    }
    for (file, _, _) in WRITES {
        assert!(sources.iter().any(|(name, _)| name.as_str() == *file), "{file} is listed in WRITES but is not in src/");
    }
}

/// The count sees a call only by its full spelling, so the crate never
/// imports std::fs or tokio::fs, or their functions, and never renames a
/// process type: `use std::fs as f; f::write(..)` would slip past it.
#[test]
fn file_and_process_modules_are_never_imported_under_another_name() {
    for (name, text) in tool_sources() {
        for statement in use_statements(&text) {
            let fs = statement.contains("std::fs")
                || statement.contains("tokio::fs")
                || (statement.contains("std::{") && statement.contains("fs"))
                || (statement.contains("tokio::{") && statement.contains("fs"));
            let renamed_process = statement.contains("process") && statement.contains(" as ");
            assert!(
                !fs && !renamed_process,
                "{name}: `{statement}`: spell file calls out in full (std::fs::write), so every_file_write_is_accounted_for sees them"
            );
        }
    }
}

#[test]
fn the_write_count_skips_tests_and_counts_helpers() {
    let text = "fn a() { std::fs::write(p, b); save_toml_file(&m, &p); }\nfn b() { std::fs::create_dir_all(d); std::fs::remove_dir_all(d); std::process::Command::new(\"git\"); }\n#[cfg(test)]\nmod tests {\n    fn t() { std::fs::write(q, b); }\n}\n#[cfg(test)]\nuse std::fs::write;\n";
    assert_eq!(write_calls(text), 5);
}

#[test]
#[should_panic(expected = "followed by other code")]
fn a_test_block_before_other_code_is_refused() {
    write_calls("#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn after() { std::fs::write(p, b); }\n");
}

#[test]
fn a_use_over_several_lines_is_read_whole() {
    let statements = use_statements("use std::{\n    fs::write,\n    io,\n};\nfn x() {}\n");
    assert_eq!(statements.len(), 1);
    assert!(statements[0].contains("std::{") && statements[0].contains("fs::write"), "{statements:?}");
}
