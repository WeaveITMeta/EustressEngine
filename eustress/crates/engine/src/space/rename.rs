//! File > Rename: rename the open Space's folder, or its Universe's, while
//! Studio has the Space open.
//!
//! The open Space's world database, file watcher and caches hold files open
//! inside its folder, and Windows will not rename a folder with files open in
//! it. So a rename writes the Space to disk, steps Studio onto the scratch
//! Space (the same switch File > Open makes, which lets go of everything the
//! Space held), renames the folder as soon as Windows allows, and opens the
//! Space again under its new name. If the folder stays in use past
//! [`RENAME_WAIT`], Studio opens the Space again under its old name and says
//! why. Nothing is half renamed: the folder rename is one step, and it either
//! happens or it does not.
//!
//! The folder rename itself is `eustress_tools::universe_tools::
//! rename_space_folder` / `rename_universe_folder`, the one the
//! `rename_space` and `rename_universe` tools use.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use eustress_tools::universe_tools::{self as folders, FolderRenameError};

use crate::notifications::NotificationManager;
use crate::ui::slint_ui::SlintUiState;

/// Which folder File > Rename renames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameTarget {
    /// The open Space.
    Space,
    /// The Universe that holds the open Space.
    Universe,
}

impl RenameTarget {
    /// The Rename dialog's name for it.
    pub fn key(self) -> &'static str {
        match self {
            RenameTarget::Space => "space",
            RenameTarget::Universe => "universe",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "space" => Some(RenameTarget::Space),
            "universe" => Some(RenameTarget::Universe),
            _ => None,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            RenameTarget::Space => "Space",
            RenameTarget::Universe => "Universe",
        }
    }
}

/// How long a rename waits for the folder to be free before it gives up and
/// opens the Space again under its old name. Long enough for a save's git
/// commit to finish.
pub const RENAME_WAIT: Duration = Duration::from_secs(30);

/// How often it tries the rename while the folder is in use.
const RETRY_EVERY: Duration = Duration::from_millis(250);

/// What a rename moves: one folder, and the open Space before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    pub target: RenameTarget,
    /// The folder that holds the one renamed.
    pub parent: PathBuf,
    pub old_name: String,
    pub new_name: String,
    /// The open Space now.
    pub space_before: PathBuf,
    /// The same Space after the rename.
    pub space_after: PathBuf,
}

impl RenamePlan {
    pub fn from_dir(&self) -> PathBuf {
        self.parent.join(&self.old_name)
    }

    pub fn to_dir(&self) -> PathBuf {
        self.parent.join(&self.new_name)
    }
}

/// Works out renaming the open Space at `space_root`, or `universe_root` (the
/// Universe that holds it), to `new_name`. Changes nothing; the error is a
/// sentence for the person.
pub fn plan(
    target: RenameTarget,
    space_root: &Path,
    universe_root: Option<&Path>,
    new_name: &str,
) -> Result<RenamePlan, String> {
    let folder = match target {
        RenameTarget::Space => space_root.to_path_buf(),
        RenameTarget::Universe => universe_root
            .ok_or("This Space is not inside a Universe folder, so there is no Universe to rename.")?
            .to_path_buf(),
    };
    let (Some(parent), Some(old_name)) = (folder.parent(), folder.file_name().and_then(|n| n.to_str())) else {
        return Err(format!("{} cannot be renamed.", folder.display()));
    };
    folders::validate_path_component(new_name)?;
    if new_name == old_name {
        return Err("That is already its name.".to_string());
    }
    let case_only = old_name.to_lowercase() == new_name.to_lowercase();
    let to_dir = parent.join(new_name);
    if to_dir.exists() && !case_only {
        return Err(format!("There is already a folder named '{new_name}' there. Pick another name."));
    }
    let space_after = match target {
        RenameTarget::Space => to_dir,
        RenameTarget::Universe => {
            let inside = space_root
                .strip_prefix(&folder)
                .map_err(|_| "The open Space is not inside its Universe folder.".to_string())?;
            to_dir.join(inside)
        }
    };
    Ok(RenamePlan {
        target,
        parent: parent.to_path_buf(),
        old_name: old_name.to_string(),
        new_name: new_name.to_string(),
        space_before: space_root.to_path_buf(),
        space_after,
    })
}

/// The rename under way, if any.
#[derive(Resource, Default)]
pub struct SpaceRename(Option<RenameJob>);

impl SpaceRename {
    pub fn is_running(&self) -> bool {
        self.0.is_some()
    }
}

struct RenameJob {
    plan: RenamePlan,
    /// When it gives up and opens the Space again under its old name.
    deadline: Instant,
    next_try: Instant,
}

fn folder_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

fn open_space_root(world: &World) -> Option<PathBuf> {
    world.get_resource::<crate::space::SpaceRoot>().map(|r| r.0.clone())
}

fn playing(world: &World) -> bool {
    world
        .get_resource::<State<crate::play_mode::PlayModeState>>()
        .is_some_and(|s| *s.get() != crate::play_mode::PlayModeState::Editing)
}

fn running(world: &World) -> bool {
    world.get_resource::<SpaceRename>().is_some_and(SpaceRename::is_running)
}

/// The Rename dialog's state: shown or not, working or not, and its message.
fn show_dialog(world: &World, show: bool, busy: bool, error: &str) {
    let Some(slint) = world.get_non_send_resource::<SlintUiState>() else { return };
    let ui = &slint.window;
    ui.set_rename_busy(busy);
    ui.set_rename_error(crate::ui::display_text::shown_block(error));
    ui.set_show_rename_dialog(show);
}

/// Shows the Rename dialog with `target` chosen and the current names filled
/// in. Says so in the dialog when a rename cannot run now.
pub fn open_dialog(world: &mut World, target: RenameTarget) {
    let Some(space_root) = open_space_root(world) else { return };
    let universe = crate::space::universe_root_for_path(&space_root);
    let space_name = folder_name(&space_root);
    let universe_name = universe.as_deref().map(folder_name).unwrap_or_default();
    // Without a Universe folder there is only the Space to rename.
    let target = if universe.is_none() { RenameTarget::Space } else { target };
    let why_not = if running(world) {
        "A rename is already under way."
    } else if playing(world) {
        "Stop Play before renaming."
    } else if crate::space::format_migration::is_scratch_space(&space_root) {
        "This is the scratch Space, which is not yours to rename. Open a Space first."
    } else {
        ""
    };
    if let Some(slint) = world.get_non_send_resource::<SlintUiState>() {
        let ui = &slint.window;
        ui.set_rename_space_name(space_name.as_str().into());
        ui.set_rename_universe_name(universe_name.as_str().into());
        ui.set_rename_target(target.key().into());
        let current = match target {
            RenameTarget::Space => &space_name,
            RenameTarget::Universe => &universe_name,
        };
        ui.set_rename_new_name(current.as_str().into());
    }
    show_dialog(world, true, running(world), why_not);
}

/// Starts renaming the open Space's folder, or its Universe's, to `new_name`
/// (the Rename dialog's Rename). A rename that cannot start says why in the
/// dialog and changes nothing.
pub fn begin(world: &mut World, target: RenameTarget, new_name: &str) {
    match start(world, target, new_name) {
        Ok(()) => show_dialog(world, true, true, ""),
        Err(why) => show_dialog(world, true, false, &why),
    }
}

fn start(world: &mut World, target: RenameTarget, new_name: &str) -> Result<(), String> {
    if running(world) {
        return Err("A rename is already under way.".to_string());
    }
    if playing(world) {
        return Err("Stop Play before renaming.".to_string());
    }
    let space_root = open_space_root(world).ok_or("No Space is open.")?;
    if crate::space::format_migration::is_scratch_space(&space_root) {
        return Err("This is the scratch Space, which is not yours to rename. Open a Space first.".to_string());
    }
    let universe = crate::space::universe_root_for_path(&space_root);
    let plan = plan(target, &space_root, universe.as_deref(), new_name)?;
    // A Universe is a folder directly in the Eustress folder; nothing above
    // that is ever renamed from here.
    if target == RenameTarget::Universe && plan.parent != crate::space::workspace_root() {
        return Err("That Universe is not in the Eustress folder, so Studio does not rename it.".to_string());
    }

    // Everything in memory goes to disk first. A Space that cannot be written
    // in full is not renamed.
    let flush = crate::ui::file_event_handler::flush_space(world, crate::ui::file_event_handler::FlushTerrain::Save)
        .map_err(|why| format!("The Space could not be saved, so it was not renamed: {why}"))?;
    if flush.terrain_unsaved {
        return Err("Some terrain edits could not be saved, so the Space was not renamed. Save it, then try again.".to_string());
    }
    if flush.report.errors > 0 {
        return Err(format!(
            "{} part(s) could not be saved, so the Space was not renamed. Save it, then try again.",
            flush.report.errors
        ));
    }

    // Step onto the scratch Space. This is File > Open's switch: it lets go
    // of the world database, the file watcher and everything else the Space
    // held, over the next frames.
    let scratch = crate::space::format_migration::scratch_space_root();
    crate::space::space_ops::open_space(world, &scratch);
    if open_space_root(world).as_deref() != Some(scratch.as_path()) {
        return Err("Studio could not step away from the Space to rename it.".to_string());
    }

    info!("Renaming {} '{}' to '{}'", target.noun(), plan.old_name, plan.new_name);
    let now = Instant::now();
    world.get_resource_or_insert_with(SpaceRename::default).0 = Some(RenameJob {
        plan,
        deadline: now + RENAME_WAIT,
        next_try: now,
    });
    Ok(())
}

/// Tries the rename under way, every [`RETRY_EVERY`] until the folder is
/// free or [`RENAME_WAIT`] has passed. Does nothing while no rename runs.
pub fn pump(world: &mut World) {
    let Some((plan, deadline, next_try)) = world
        .get_resource::<SpaceRename>()
        .and_then(|r| r.0.as_ref())
        .map(|job| (job.plan.clone(), job.deadline, job.next_try))
    else {
        return;
    };
    let now = Instant::now();
    if now < next_try {
        return;
    }

    // The person opened another Space meanwhile (the dialog covers the
    // window, but a portal or an agent can still switch). Stop, and leave
    // both folders as they are.
    let on_scratch = open_space_root(world).is_some_and(|root| crate::space::format_migration::is_scratch_space(&root));
    if !on_scratch {
        if let Some(mut job) = world.get_resource_mut::<SpaceRename>() {
            job.0 = None;
        }
        let why = format!(
            "The rename stopped because another Space was opened. '{}' keeps its name.",
            plan.old_name
        );
        show_dialog(world, true, false, &why);
        if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
            n.warning(why);
        }
        return;
    }

    let result = match plan.target {
        RenameTarget::Space => folders::rename_space_folder(&plan.parent, &plan.old_name, &plan.new_name).map(|_| ()),
        RenameTarget::Universe => folders::rename_universe_folder(&plan.parent, &plan.old_name, &plan.new_name).map(|_| ()),
    };
    match result {
        Ok(()) => finish(world, &plan),
        Err(FolderRenameError::Busy(_)) if now < deadline => {
            if let Some(mut job) = world.get_resource_mut::<SpaceRename>() {
                if let Some(job) = job.0.as_mut() {
                    job.next_try = now + RETRY_EVERY;
                }
            }
        }
        Err(e) => give_up(world, &plan, &e),
    }
}

/// The folder has its new name: everything that remembered the old path
/// follows, and the Space opens again.
fn finish(world: &mut World, plan: &RenamePlan) {
    if let Some(mut job) = world.get_resource_mut::<SpaceRename>() {
        job.0 = None;
    }
    let (from, to) = (plan.from_dir(), plan.to_dir());
    if let Some(mut settings) = world.get_resource_mut::<crate::editor_settings::EditorSettings>() {
        settings.follow_rename(&from, &to);
    }
    if let Some(mut tabs) = world.get_resource_mut::<crate::ui::center_tabs::CenterTabManager>() {
        tabs.follow_rename(&from, &to);
    }
    crate::space::space_ops::open_space(world, &plan.space_after);
    show_dialog(world, false, false, "");
    let done = format!("Renamed the {} '{}' to '{}'.", plan.target.noun(), plan.old_name, plan.new_name);
    info!("{done}");
    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        n.success(done);
    }
}

/// The folder could not be renamed: the Space opens again under its old name,
/// and the dialog says why.
fn give_up(world: &mut World, plan: &RenamePlan, error: &FolderRenameError) {
    if let Some(mut job) = world.get_resource_mut::<SpaceRename>() {
        job.0 = None;
    }
    crate::space::space_ops::open_space(world, &plan.space_before);
    let why = match error {
        FolderRenameError::Busy(e) => format!(
            "Studio could not rename '{}' because a program still has files open in it ({e}). \
             Close other programs using it, then try again. It is open again under its old name.",
            plan.old_name
        ),
        other => format!("Studio could not rename '{}': {other}. It is open again under its old name.", plan.old_name),
    };
    warn!("{why}");
    show_dialog(world, true, false, &why);
    if let Some(mut n) = world.get_resource_mut::<NotificationManager>() {
        n.error(why);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("eustress_file_rename_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_space_rename_reopens_the_renamed_folder() {
        let r = root("space");
        let space = r.join("Voltec").join("Spaces").join("Lot");
        std::fs::create_dir_all(&space).unwrap();
        let p = plan(RenameTarget::Space, &space, Some(&r.join("Voltec")), "Parking Lot").unwrap();
        assert_eq!(p.from_dir(), space);
        assert_eq!(p.to_dir(), r.join("Voltec").join("Spaces").join("Parking Lot"));
        assert_eq!(p.space_after, p.to_dir());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn a_universe_rename_carries_the_open_space_with_it() {
        let r = root("universe");
        let universe = r.join("Voltec");
        for space in [universe.join("Spaces").join("Lot"), universe.join("Legacy")] {
            std::fs::create_dir_all(&space).unwrap();
            let p = plan(RenameTarget::Universe, &space, Some(&universe), "Voltec Motors").unwrap();
            assert_eq!(p.from_dir(), universe);
            assert_eq!(p.space_after, r.join("Voltec Motors").join(space.strip_prefix(&universe).unwrap()));
        }
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn a_plan_refuses_what_a_rename_must_not_do() {
        let r = root("refuse");
        let spaces = r.join("U").join("Spaces");
        std::fs::create_dir_all(spaces.join("A")).unwrap();
        std::fs::create_dir_all(spaces.join("B")).unwrap();
        let a = spaces.join("A");
        assert!(plan(RenameTarget::Space, &a, Some(&r.join("U")), "B").is_err(), "onto another Space");
        assert!(plan(RenameTarget::Space, &a, Some(&r.join("U")), "A").is_err(), "the same name");
        assert!(plan(RenameTarget::Space, &a, Some(&r.join("U")), "../B").is_err(), "a path");
        assert!(plan(RenameTarget::Space, &a, Some(&r.join("U")), "").is_err(), "empty");
        assert!(plan(RenameTarget::Universe, &a, None, "V").is_err(), "no Universe");
        assert!(plan(RenameTarget::Space, &a, Some(&r.join("U")), "a").is_ok(), "a change of case");
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn the_recent_list_and_next_launch_follow_a_rename() {
        let mut settings = crate::editor_settings::EditorSettings::default();
        let from = Path::new("C:/E/Voltec");
        let to = Path::new("C:/E/Voltec Motors");
        let lot = from.join("Spaces").join("Lot");
        settings.recent_spaces = vec![
            lot.to_string_lossy().to_string(),
            "C:/E/Other/Spaces/Yard".to_string(),
            to.join("Spaces").join("Lot").to_string_lossy().to_string(),
        ];
        settings.last_space_path = Some(lot.to_string_lossy().to_string());
        settings.follow_rename(from, to);
        let moved = to.join("Spaces").join("Lot").to_string_lossy().to_string();
        assert_eq!(settings.last_space_path.as_deref(), Some(moved.as_str()));
        assert_eq!(settings.recent_spaces, vec![moved, "C:/E/Other/Spaces/Yard".to_string()], "moved, others kept, no duplicate");
    }

    #[test]
    fn the_scratch_space_is_recognised() {
        assert!(crate::space::format_migration::is_scratch_space(&std::env::temp_dir().join("Eustress scratch Space")));
        assert!(!crate::space::format_migration::is_scratch_space(Path::new("C:/E/U/Spaces/Eustress scratch Space")));
    }
}
