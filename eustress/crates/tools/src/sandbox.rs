//! One containment check for every path a tool's caller supplies, and the
//! rules for running git on a caller's behalf.
//!
//! A caller's path is a request, never an address. [`resolve`] accepts it only
//! when it is relative, every part of it is an ordinary name, it never names a
//! `.git` folder, and the place it reaches stays under the root once every
//! junction and link on disk is followed. Every tool that turns an input into
//! a file path goes through here, so the rules live in one place.
//!
//! Windows makes several spellings reach the same file, and each is refused
//! before it can be tried: a drive letter or `C:` prefix, a UNC or `\\?\`
//! path, an alternate data stream (`file.txt:stream`), a name with a trailing
//! dot or space (Windows drops them, so `.git.` is `.git`), and the reserved
//! device names (`CON`, `NUL`, `COM1`...). An 8.3 short name such as `GIT~1`
//! passes the name check but not containment: [`resolve`] canonicalises what
//! exists, which turns a short name into its long one, and checks that again.
//!
//! What the check cannot see is a link created between the check and the
//! tool's use of the path; the tools run in the engine's own process, so that
//! would need something else already writing into the Universe.
//!
//! git runs with every hook off ([`git_command`]): a hook is a program the
//! repository names, so it is never run for a caller. A name a caller gives
//! git is checked first ([`git_revision`], [`git_branch_name`]), so it can
//! never be read as an option.

use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Why a caller's path was refused. The message names the rule, so the
/// caller can correct the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A drive, a root, or a UNC or verbatim prefix.
    Absolute(String),
    /// A `..` step.
    ParentStep(String),
    /// A `.git` folder, in any spelling.
    GitFolder(String),
    /// A name Windows would read as something else.
    BadName { path: String, name: String, why: &'static str },
    /// The path reaches outside the root once links are followed.
    Escapes(String),
    /// A file the engine keeps in a `.eustress` folder for itself: its
    /// bridge port, socket pointer, LSP port, command queue or snapshot.
    EngineOwned(String),
    /// An absolute path that is not on this machine's own drives: a network,
    /// device or verbatim path, or a drive mapped to a network share.
    NotLocal(String),
    /// The root itself could not be resolved.
    Root(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Absolute(p) => write!(f, "'{p}' is absolute; give a path relative to the folder"),
            Refusal::ParentStep(p) => write!(f, "'{p}' steps out with '..'"),
            Refusal::GitFolder(p) => write!(f, "'{p}' is inside a .git folder, which tools never touch"),
            Refusal::BadName { path, name, why } => write!(f, "'{path}': the name '{name}' {why}"),
            Refusal::Escapes(p) => write!(f, "'{p}' leads outside the folder"),
            Refusal::EngineOwned(p) => write!(
                f,
                "'{p}' is a file the engine keeps for itself (its port, socket, command queue or snapshot), which tools never touch"
            ),
            Refusal::NotLocal(p) => write!(
                f,
                "'{p}' is not a path on this machine's own drives; a network, device or verbatim path is never opened"
            ),
            Refusal::Root(e) => write!(f, "the folder could not be resolved: {e}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// Windows' reserved device names, which name a device in every folder.
const DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
    "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// What the engine keeps in a `.eustress` folder for itself. Clients trust
/// these (the bridge client connects to the port `engine.port` names), so a
/// tool that rewrote one could point a client at another engine, or queue
/// commands no tool call was checked for; `host` and `hosts` hold
/// multiplayer join keys and pins, and `engine.token` is kept for a bridge
/// token should one ever sit there.
const ENGINE_OWNED: &[&str] = &[
    "engine.port",
    "lsp.port",
    "engine.sock",
    "engine.token",
    "sim-commands.jsonl",
    "runtime-snapshot.json",
    "instances",
    "host",
    "hosts",
];

/// True when `path` names one of [`ENGINE_OWNED`] directly inside a
/// `.eustress` folder, in any case.
fn is_engine_owned(path: &Path) -> bool {
    let names: Vec<String> = path.components().filter_map(|c| match c {
        Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
        _ => None,
    }).collect();
    names.windows(2).any(|w| {
        w[0].eq_ignore_ascii_case(".eustress") && ENGINE_OWNED.iter().any(|owned| w[1].eq_ignore_ascii_case(owned))
    })
}

/// True for `.git` in any spelling that reaches it: any case, with trailing
/// dots or spaces.
pub fn is_git_name(name: &str) -> bool {
    name.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(".git")
}

fn check_name(raw: &str, name: &str) -> Result<(), Refusal> {
    let bad = |why| Refusal::BadName { path: raw.to_string(), name: name.to_string(), why };
    if is_git_name(name) {
        return Err(Refusal::GitFolder(raw.to_string()));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(bad("ends with a dot or space, which Windows drops"));
    }
    if name.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) {
        return Err(bad("holds a character no file name can have"));
    }
    let stem = name.split('.').next().unwrap_or("").trim_end();
    if DEVICE_NAMES.iter().any(|d| d.eq_ignore_ascii_case(stem)) {
        return Err(bad("is a Windows device name"));
    }
    Ok(())
}

/// The caller's path as a clean relative path, checked by its text alone.
/// Empty means the root itself.
pub fn relative(raw: &str) -> Result<PathBuf, Refusal> {
    let cleaned = raw.trim().replace('\\', "/");
    // A colon is a drive (`C:`), a drive-relative path (`C:x`), or an
    // alternate data stream (`a.txt:s`); none is a name inside the folder.
    if cleaned.contains(':') {
        return Err(Refusal::Absolute(raw.to_string()));
    }
    let mut out = PathBuf::new();
    for part in Path::new(&cleaned).components() {
        match part {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                check_name(raw, &name)?;
                out.push(name.as_ref());
            }
            Component::CurDir => {}
            Component::ParentDir => return Err(Refusal::ParentStep(raw.to_string())),
            Component::RootDir | Component::Prefix(_) => return Err(Refusal::Absolute(raw.to_string())),
        }
    }
    if is_engine_owned(&out) {
        return Err(Refusal::EngineOwned(raw.to_string()));
    }
    Ok(out)
}

/// The part of `path` that exists on disk.
fn existing_prefix(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) if existing.file_name().is_some() => existing = parent.to_path_buf(),
            _ => break,
        }
    }
    existing
}

/// `raw` under `root`, or why not. The path returned is `root` joined with the
/// clean relative path (not a canonical `\\?\` form), so tools can show it and
/// strip `root` from it as before.
pub fn resolve(root: &Path, raw: &str) -> Result<PathBuf, Refusal> {
    let rel = relative(raw)?;
    let root_canon = root.canonicalize().map_err(|e| Refusal::Root(format!("{}: {e}", root.display())))?;
    let joined = if rel.as_os_str().is_empty() { root.to_path_buf() } else { root.join(&rel) };

    // Follow every junction and link in the part that exists; a short name
    // comes back long.
    let canon = existing_prefix(&joined)
        .canonicalize()
        .map_err(|_| Refusal::Escapes(raw.to_string()))?;
    let inside = canon.strip_prefix(&root_canon).map_err(|_| Refusal::Escapes(raw.to_string()))?;
    if inside.components().any(|c| matches!(c, Component::Normal(n) if is_git_name(&n.to_string_lossy()))) {
        return Err(Refusal::GitFolder(raw.to_string()));
    }
    // Followed links and long names included.
    if is_engine_owned(inside) {
        return Err(Refusal::EngineOwned(raw.to_string()));
    }
    Ok(joined)
}

/// A caller's absolute path to something on this machine's own drives,
/// such as a file to import. On Windows only a drive-letter path (`C:\...`)
/// is taken. A network path (`\\host\share`, `//host/share`), a verbatim or
/// device path (`\\?\`, `\\.\`), a drive-relative `C:x` and a second `:` (an
/// alternate data stream) are refused by their text, before anything touches
/// them, because opening a network path sends the person's sign-in challenge
/// to whichever host it names. A drive letter mapped to a network share is
/// refused once the path resolves to one. Elsewhere, any rooted path; a
/// network mount there is not told apart from a local folder.
///
/// Resolving opens the path, so a symbolic link in a local folder that points
/// at a network target is followed once before it is refused. Making such a
/// link takes a privilege no tool has, so that case is left out.
pub fn local_absolute(raw: &str) -> Result<PathBuf, Refusal> {
    let text = raw.trim();
    let path = Path::new(text);
    let refused = || Refusal::NotLocal(raw.to_string());
    #[cfg(windows)]
    {
        use std::path::Prefix;
        match path.components().next() {
            Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)) => {}
            _ => return Err(refused()),
        }
        if !path.has_root() || text.get(2..).is_some_and(|rest| rest.contains(':')) {
            return Err(refused());
        }
        if let Ok(canon) = path.canonicalize() {
            if !matches!(canon.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::VerbatimDisk(_) | Prefix::Disk(_))) {
                return Err(refused());
            }
        }
    }
    #[cfg(not(windows))]
    if !path.is_absolute() {
        return Err(refused());
    }
    Ok(path.to_path_buf())
}

/// True when `path` is `root` or inside it, once both are resolved on disk.
/// False when either cannot be resolved.
pub fn is_within(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => false,
    }
}

/// A single file or folder name the caller chose (a new Space, an
/// experiment): one ordinary name, no separators. Returned trimmed.
pub fn plain_name(raw: &str) -> Result<String, Refusal> {
    let name = raw.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(Refusal::BadName {
            path: raw.to_string(),
            name: name.to_string(),
            why: "must be one name, without '/' or '\\'",
        });
    }
    if name.contains(':') {
        return Err(Refusal::Absolute(raw.to_string()));
    }
    check_name(raw, name)?;
    Ok(name.to_string())
}

// ── git ────────────────────────────────────────────────────────────────────

/// Arguments that turn off every hook in the repository a git command runs
/// in: `core.hooksPath` pointed at the null device, where no hook can exist.
/// They go before the subcommand.
pub const GIT_NO_HOOKS: [&str; 2] =
    ["-c", if cfg!(windows) { "core.hooksPath=NUL" } else { "core.hooksPath=/dev/null" }];

/// `git` with hooks off, running in `dir`, with no console window on Windows.
/// Every git command a tool or the engine runs starts here.
pub fn git_command(dir: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    cmd.args(GIT_NO_HOOKS).current_dir(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A revision a caller names (a branch, a tag, a commit, `HEAD~2`), refused
/// when git would read it as an option (it starts with `-`) or it holds
/// whitespace, a control character or `..`.
pub fn git_revision(raw: &str) -> Result<&str, String> {
    let rev = raw.trim();
    if rev.is_empty() {
        return Err("a revision is required".to_string());
    }
    if rev.starts_with('-') {
        return Err(format!("'{rev}' starts with '-', which git reads as an option"));
    }
    if rev.chars().any(|c| c.is_whitespace() || c.is_control()) || rev.contains("..") {
        return Err(format!("'{rev}' is not one revision"));
    }
    Ok(rev)
}

/// A branch name a caller chose, by git's rules for one (`git
/// check-ref-format --branch`), and never one that starts with `-`.
pub fn git_branch_name(raw: &str) -> Result<&str, String> {
    let name = raw.trim();
    let bad = |why: &str| Err(format!("'{name}' is not a branch name: {why}"));
    if name.is_empty() {
        return Err("a branch name is required".to_string());
    }
    if name.starts_with('-') {
        return bad("it starts with '-', which git reads as an option");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\')) {
        return bad("it holds a space, a control character or one of ~ ^ : ? * [ \\");
    }
    if name.contains("..") || name.contains("//") || name.contains("@{") || name == "@" {
        return bad("it holds '..', '//' or '@{'");
    }
    if name.starts_with('/') || name.ends_with('/') || name.ends_with('.') || name.ends_with(".lock") {
        return bad("it starts or ends with '/', or ends with '.' or '.lock'");
    }
    if name.split('/').any(|part| part.starts_with('.')) {
        return bad("a part of it starts with '.'");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eustress-sandbox-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Spaces/Space1/Workspace/Tower")).unwrap();
        std::fs::create_dir_all(root.join("Spaces/Space1/.git/hooks")).unwrap();
        root
    }

    /// A network, device or verbatim path is refused by its text, so it is
    /// never opened; a drive-letter path on this machine is taken.
    #[cfg(windows)]
    #[test]
    fn only_paths_on_local_drives_are_taken() {
        for bad in [
            r"\\attacker-host\share\x.ply",
            "//attacker-host/share/x.ply",
            r"\\?\C:\x.ply",
            r"\\?\UNC\host\share\x.ply",
            r"\\.\pipe\x",
            r"\??\UNC\host\share\x.ply",
            r"\??\C:\x.ply",
            "//?/UNC/host/share/x.ply",
            "//./pipe/name",
            r"\\host \share\x.ply",
            r"\\host.\share\x.ply",
            "C:x.ply",
            r"C:\a\x.ply:stream",
            r"\x.ply",
            "x.ply",
        ] {
            assert!(matches!(local_absolute(bad), Err(Refusal::NotLocal(_))), "{bad}");
        }
        let here = std::env::temp_dir().join("x.ply");
        assert_eq!(local_absolute(&here.to_string_lossy()).unwrap(), here);
    }

    #[test]
    fn within_means_the_root_or_under_it() {
        let root = temp_root("within");
        assert!(is_within(&root, &root));
        assert!(is_within(&root.join("Spaces/Space1"), &root));
        assert!(!is_within(root.parent().unwrap(), &root));
        assert!(!is_within(&root.join("missing"), &root), "a path that does not resolve is not inside");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The engine's own files in a `.eustress` folder are refused in any
    /// case and at any depth; the rest of `.eustress` is not.
    #[test]
    fn the_engines_own_files_are_refused() {
        let root = temp_root("owned");
        std::fs::create_dir_all(root.join(".eustress")).unwrap();
        std::fs::write(root.join(".eustress/engine.port"), "50123").unwrap();
        for bad in [
            ".eustress/engine.port",
            ".EUSTRESS/Engine.Port",
            ".eustress\\lsp.port",
            ".eustress/engine.sock",
            ".eustress/sim-commands.jsonl",
            ".eustress/runtime-snapshot.json",
            ".eustress/instances/123.json",
            ".eustress/engine.token",
            ".eustress/host/Game/join.toml",
            ".eustress/hosts/50123.link",
            "Spaces/Space1/.eustress/engine.port",
        ] {
            assert!(matches!(resolve(&root, bad), Err(Refusal::EngineOwned(_))), "{bad}");
        }
        assert!(resolve(&root, ".eustress/sync.toml").is_ok());
        assert!(resolve(&root, ".eustress/knowledge/notes.md").is_ok());
        assert!(resolve(&root, "Spaces/Space1/engine.port").is_ok(), "only inside a .eustress folder");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ordinary_paths_resolve_under_the_root() {
        let root = temp_root("ok");
        assert_eq!(resolve(&root, "Spaces/Space1/Workspace/Tower").unwrap(), root.join("Spaces/Space1/Workspace/Tower"));
        assert_eq!(resolve(&root, "Spaces\\Space1\\Workspace\\New.txt").unwrap(), root.join("Spaces/Space1/Workspace/New.txt"));
        assert_eq!(resolve(&root, "Spaces/Space1/New/Deeper/x.toml").unwrap(), root.join("Spaces/Space1/New/Deeper/x.toml"));
        assert_eq!(resolve(&root, "").unwrap().as_os_str(), root.as_os_str());
        assert!(resolve(&root, "Spaces/Space1/.gitignore").is_ok(), ".gitignore is not .git");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_spelling_of_git_is_refused() {
        let root = temp_root("git");
        for p in [
            "Spaces/Space1/.git/hooks/pre-commit",
            "Spaces/Space1/.GIT/hooks/pre-commit",
            "Spaces/Space1/.git./hooks/pre-commit",
            "Spaces/Space1/.git /hooks/pre-commit",
            "Spaces\\Space1\\.git\\config",
        ] {
            assert!(matches!(resolve(&root, p), Err(Refusal::GitFolder(_))), "{p}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn absolute_and_parent_paths_are_refused() {
        let root = temp_root("abs");
        for p in ["C:/Windows/System32", "C:Windows", "/etc/passwd", "\\\\server\\share\\x", "\\\\?\\C:\\x", "Spaces/a.txt:stream"] {
            assert!(matches!(resolve(&root, p), Err(Refusal::Absolute(_))), "{p}");
        }
        for p in ["..", "../outside", "Spaces/../../outside", "Spaces\\..\\..\\outside"] {
            assert!(matches!(resolve(&root, p), Err(Refusal::ParentStep(_))), "{p}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn windows_lookalike_names_are_refused() {
        for p in ["Spaces/CON", "Spaces/nul.txt", "Spaces/COM1.toml", "Spaces/name.", "Spaces/name /x", "Spaces/a?b"] {
            assert!(matches!(relative(p), Err(Refusal::BadName { .. })), "{p}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_root_is_refused() {
        let root = temp_root("link");
        let outside = std::env::temp_dir().join(format!("eustress-sandbox-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("Spaces/Space1/Workspace/Out")).unwrap();
        assert!(matches!(resolve(&root, "Spaces/Space1/Workspace/Out/x.txt"), Err(Refusal::Escapes(_))));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    #[test]
    fn a_junction_out_of_the_root_is_refused() {
        let root = temp_root("junction");
        let outside = std::env::temp_dir().join(format!("eustress-sandbox-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        let link = root.join("Spaces/Space1/Workspace/Out");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if made {
            assert!(matches!(resolve(&root, "Spaces/Space1/Workspace/Out/x.txt"), Err(Refusal::Escapes(_))));
            let _ = std::fs::remove_dir(&link);
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_plain_name_is_one_ordinary_name() {
        assert_eq!(plain_name(" My Space ").unwrap(), "My Space");
        for bad in ["", "..", "a/b", "a\\b", "C:x", ".git", "CON", "x."] {
            assert!(plain_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_name_for_git_is_never_an_option() {
        for good in ["main", "exp/run_1-20260930_101500", "feature/x_y", "v0001"] {
            assert_eq!(git_branch_name(good), Ok(good), "{good}");
        }
        for bad in [
            "", "-x", "--orphan=evil", "a b", "a..b", "a~1", "a^", "a:b", "a?", "a*", "a[", "a\\b",
            ".hidden", "a/.b", "a/", "/a", "a.lock", "a.", "a//b", "a@{1}", "@",
        ] {
            assert!(git_branch_name(bad).is_err(), "{bad}");
        }
        for good in ["HEAD", "HEAD~3", "HEAD^", "v0001", "main@{1}", "abc1234"] {
            assert_eq!(git_revision(good), Ok(good), "{good}");
        }
        for bad in ["", "-p", " --output=x", "a b", "a..b", "a\nb"] {
            assert!(git_revision(bad).is_err(), "{bad:?}");
        }
    }

    /// A hook planted in a repository never runs through [`git_command`]. The
    /// same command without it runs the hook, so the test can fail.
    #[test]
    fn git_runs_no_hook() {
        let repo = std::env::temp_dir().join(format!("eustress-sandbox-hooks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&repo);
        std::fs::create_dir_all(&repo).unwrap();
        let run = |hooks_off: bool, args: &[&str]| {
            let mut cmd = if hooks_off { git_command(&repo) } else { std::process::Command::new("git") };
            cmd.current_dir(&repo)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        };
        if !run(true, &["init", "--quiet"]) {
            return; // No git on this machine.
        }
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        assert!(run(true, &["add", "a.txt"]) && run(true, &["commit", "--quiet", "-m", "a"]));
        let hook = repo.join(".git/hooks/post-checkout");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, "#!/bin/sh\ntouch hook_ran\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert!(run(true, &["checkout", "--quiet", "-b", "quiet"]));
        assert!(!repo.join("hook_ran").exists(), "the hook ran through git_command");
        assert!(run(false, &["checkout", "--quiet", "-b", "loud"]));
        assert!(repo.join("hook_ran").exists(), "without git_command the hook runs, so this test can fail");
        let _ = std::fs::remove_dir_all(&repo);
    }
}
