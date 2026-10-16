//! # Where a Workshop product's meshes live
//!
//! A product the Workshop designs has parts whose `mesh` paths read
//! `assets/meshes/<product>/<part>.glb`. Those meshes are written to
//! `<Universe>/assets/meshes/<product>/`: a visible folder in the Universe, not
//! under `.eustress`, because that is the folder export ships and the folder the
//! loader resolves an `assets/...` path against after the part's own folder and
//! the Space.
//!
//! Earlier builds wrote them to `<Universe>/.eustress/assets/meshes/<product>/`,
//! where nothing found them: the product drew as blocks, and a publish carried no
//! meshes. [`migrate_product_meshes`] moves what is already there, once, and
//! only what is the Workshop's:
//!
//! * a **folder** under `.eustress/assets/meshes/` whose name is a product of
//!   some Space in the Universe (`Workspace/<product>/` holds the Workshop's
//!   `generate_meshes.py`, or a part file naming `assets/meshes/<product>/`);
//!   loose files and folders that belong to nothing, such as what the Assets
//!   panel's Import writes there, stay where they are;
//! * never over a file that is already at the destination, and never through a
//!   link;
//! * not a product whose part files name the `.eustress` path outright, because
//!   moving its meshes would break those parts and rewriting a person's part
//!   files is not this module's job. The loader reads both roots, so such a
//!   product keeps working where it is.
//!
//! Every step is best effort and reported, never fatal.

use std::fs;
use std::path::{Path, PathBuf};

/// The Universe a Space belongs to.
///
/// Standard layout is `<Universe>/spaces/<Space>/`, so the Universe is two levels
/// up. A Space directly inside its Universe (no `spaces` folder) is the older
/// layout, and the Universe is one level up.
pub fn universe_root_of(space_root: &Path) -> PathBuf {
    match space_root.parent() {
        Some(parent) => {
            let is_spaces_dir = parent
                .file_name()
                .map(|n| n == "Spaces" || n == "spaces")
                .unwrap_or(false);
            if is_spaces_dir {
                parent.parent().unwrap_or(space_root).to_path_buf()
            } else {
                parent.to_path_buf()
            }
        }
        None => space_root.to_path_buf(),
    }
}

/// `<Universe>/assets/meshes/<product>/`: where a product's meshes are written.
pub fn product_mesh_dir(space_root: &Path, safe_product_name: &str) -> PathBuf {
    universe_root_of(space_root)
        .join("assets")
        .join("meshes")
        .join(safe_product_name)
}

/// What a migration did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Migration {
    /// Files moved to `<Universe>/assets/meshes/<product>/`.
    pub moved_files: usize,
    /// Products with at least one file moved.
    pub moved_products: Vec<String>,
    /// Files left where they were: a file already at the destination, a link,
    /// or one that could not be moved.
    pub kept: Vec<PathBuf>,
    /// Products left alone because their part files name the `.eustress` path.
    pub pinned_products: Vec<String>,
}

/// Move the Workshop's product meshes out of `.eustress/assets/meshes/`. Safe to
/// run again: what has moved is gone from the old folder, so a second run finds
/// nothing.
pub fn migrate_product_meshes(universe_root: &Path) -> Migration {
    let mut report = Migration::default();
    let old_root = universe_root.join(".eustress").join("assets").join("meshes");
    let Ok(entries) = fs::read_dir(&old_root) else {
        return report;
    };
    let new_root = universe_root.join("assets").join("meshes");
    let spaces = candidate_spaces(universe_root);

    let mut folders: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir() && !t.is_symlink()).unwrap_or(false))
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .collect();
    folders.sort();

    for (product, from) in folders {
        match classify_product(&spaces, &product) {
            Product::NotWorkshop => {}
            Product::PinnedToOldRoot => report.pinned_products.push(product),
            Product::Workshop => {
                let before = report.moved_files;
                move_folder_contents(&from, &new_root.join(&product), &mut report);
                if report.moved_files > before {
                    report.moved_products.push(product);
                }
            }
        }
    }
    report
}

/// The Space folders of a Universe: those under `spaces` (or `Spaces`), and any
/// folder directly inside it that has a `Workspace` (the older layout).
fn candidate_spaces(universe_root: &Path) -> Vec<PathBuf> {
    let mut spaces = Vec::new();
    for dir in ["spaces", "Spaces"] {
        if let Ok(rd) = fs::read_dir(universe_root.join(dir)) {
            spaces.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    if let Ok(rd) = fs::read_dir(universe_root) {
        spaces.extend(
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.join("Workspace").is_dir()),
        );
    }
    spaces.sort();
    spaces.dedup();
    spaces
}

#[derive(Debug, PartialEq, Eq)]
enum Product {
    NotWorkshop,
    Workshop,
    PinnedToOldRoot,
}

/// Whether a folder name under `.eustress/assets/meshes/` is a Workshop product
/// of any of these Spaces. A Space that pins it to the old path decides it.
fn classify_product(spaces: &[PathBuf], product: &str) -> Product {
    let mut found = false;
    for space in spaces {
        let dir = space.join("Workspace").join(product);
        if !dir.is_dir() {
            continue;
        }
        let (own_script, new_path, old_path) = evidence(&dir, product);
        if old_path {
            return Product::PinnedToOldRoot;
        }
        found |= own_script || new_path;
    }
    if found {
        Product::Workshop
    } else {
        Product::NotWorkshop
    }
}

/// What a product folder in a Space says about its meshes: it holds the
/// Workshop's `generate_meshes.py`; a part file names `assets/meshes/<product>/`;
/// a part file names the `.eustress/assets/meshes/<product>/` path outright.
fn evidence(product_dir: &Path, product: &str) -> (bool, bool, bool) {
    let own_script = product_dir.join("generate_meshes.py").is_file();
    let new_path = format!("assets/meshes/{product}/");
    let old_path = format!(".eustress/assets/meshes/{product}/");
    let mut budget = 400usize;
    let (mut names_new, mut names_old) = (false, false);
    scan_toml(product_dir, 4, &mut budget, &mut |text| {
        let text = text.replace('\\', "/");
        names_old |= text.contains(&old_path);
        names_new |= text.contains(&new_path);
    });
    (own_script, names_new, names_old)
}

/// Read the `.toml` files under `dir` (to `depth` levels, at most `budget`
/// files, none over 1 MB) and hand each one's text to `visit`.
fn scan_toml(dir: &Path, depth: usize, budget: &mut usize, visit: &mut dyn FnMut(&str)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if depth > 0 {
                scan_toml(&path, depth - 1, budget, visit);
            }
        } else if path.extension().is_some_and(|e| e == "toml") {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            if entry.metadata().map(|m| m.len() <= 1_000_000).unwrap_or(false) {
                if let Ok(text) = fs::read_to_string(&path) {
                    visit(&text);
                }
            }
        }
    }
}

/// Move every file under `from` to the same place under `to`, leaving behind
/// whatever cannot move, and remove `from` if that empties it.
fn move_folder_contents(from: &Path, to: &Path, report: &mut Migration) {
    if fs::create_dir_all(to).is_err() {
        report.kept.push(from.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(from) else { return };
    for entry in entries.flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        let Ok(kind) = entry.file_type() else {
            report.kept.push(source);
            continue;
        };
        if kind.is_symlink() {
            report.kept.push(source);
        } else if kind.is_dir() {
            move_folder_contents(&source, &target, report);
        } else if target.exists() {
            report.kept.push(source);
        } else if fs::rename(&source, &target).is_ok() || copy_then_remove(&source, &target) {
            report.moved_files += 1;
        } else {
            report.kept.push(source);
        }
    }
    // Succeeds only when empty, so a folder that kept a file stays.
    let _ = fs::remove_dir(from);
}

/// For a move across volumes, where a rename is refused.
fn copy_then_remove(source: &Path, target: &Path) -> bool {
    fs::copy(source, target).is_ok() && fs::remove_file(source).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A Universe folder in the temp directory, removed when dropped.
    struct Universe(PathBuf);
    impl Universe {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "eustress_product_meshes_{}_{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Universe(dir)
        }
        fn put(&self, rel: &str, bytes: &[u8]) -> PathBuf {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            path
        }
        fn has(&self, rel: &str) -> bool {
            self.0.join(rel).exists()
        }
    }
    impl Drop for Universe {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const OLD: &str = ".eustress/assets/meshes";
    const NEW: &str = "assets/meshes";

    #[test]
    fn meshes_are_written_to_the_universe_assets_folder_not_dot_eustress() {
        let standard = Path::new("/u/Tucson/spaces/Space1");
        assert_eq!(
            product_mesh_dir(standard, "Drone"),
            PathBuf::from("/u/Tucson").join("assets").join("meshes").join("Drone")
        );
        let capital = Path::new("/u/Tucson/Spaces/Space1");
        assert_eq!(universe_root_of(capital), PathBuf::from("/u/Tucson"));
        // The older layout: the Space sits directly in its Universe.
        let legacy = Path::new("/u/Tucson/Space1");
        assert_eq!(universe_root_of(legacy), PathBuf::from("/u/Tucson"));
        assert!(!product_mesh_dir(standard, "Drone").to_string_lossy().contains(".eustress"));
    }

    #[test]
    fn a_product_with_the_workshops_script_has_its_meshes_moved() {
        let u = Universe::new();
        u.put("spaces/Space1/Workspace/Drone/generate_meshes.py", b"# blender");
        u.put(&format!("{OLD}/Drone/body.glb"), b"body");
        u.put(&format!("{OLD}/Drone/rotor.glb"), b"rotor");

        let report = migrate_product_meshes(&u.0);

        assert_eq!(report.moved_files, 2, "{report:?}");
        assert_eq!(report.moved_products, vec!["Drone".to_string()]);
        assert!(report.kept.is_empty() && report.pinned_products.is_empty());
        assert_eq!(fs::read(u.0.join(NEW).join("Drone/body.glb")).unwrap(), b"body");
        assert!(u.has(&format!("{NEW}/Drone/rotor.glb")));
        assert!(!u.has(&format!("{OLD}/Drone")), "the emptied folder is removed");
    }

    #[test]
    fn a_product_found_only_by_its_part_files_is_moved_too() {
        let u = Universe::new();
        u.put(
            "spaces/Space1/Workspace/Car/Wheel/_instance.toml",
            b"[asset]\nmesh = \"assets/meshes/Car/wheel.glb\"\n",
        );
        u.put(&format!("{OLD}/Car/wheel.glb"), b"wheel");
        let report = migrate_product_meshes(&u.0);
        assert_eq!(report.moved_products, vec!["Car".to_string()], "{report:?}");
        assert!(u.has(&format!("{NEW}/Car/wheel.glb")));
    }

    #[test]
    fn what_is_not_the_workshops_stays_where_it_is() {
        let u = Universe::new();
        // The Assets panel's Import writes loose files here, and a person may keep
        // folders of their own. Neither is a Workshop product.
        u.put(&format!("{OLD}/loose.glb"), b"imported");
        u.put(&format!("{OLD}/MyPack/a.glb"), b"pack");
        // A folder named like a product of no Space.
        u.put(&format!("{OLD}/Ghost/g.glb"), b"ghost");
        // A Workspace folder with no Workshop evidence in it.
        u.put("spaces/Space1/Workspace/MyPack/notes.md", b"mine");

        let report = migrate_product_meshes(&u.0);

        assert_eq!(report, Migration::default());
        for rel in ["loose.glb", "MyPack/a.glb", "Ghost/g.glb"] {
            assert!(u.has(&format!("{OLD}/{rel}")), "{rel} must not move");
            assert!(!u.has(&format!("{NEW}/{rel}")), "{rel} must not be copied");
        }
    }

    #[test]
    fn a_file_already_at_the_destination_is_never_overwritten() {
        let u = Universe::new();
        u.put("spaces/Space1/Workspace/Drone/generate_meshes.py", b"#");
        u.put(&format!("{OLD}/Drone/body.glb"), b"old body");
        u.put(&format!("{OLD}/Drone/rotor.glb"), b"old rotor");
        u.put(&format!("{NEW}/Drone/body.glb"), b"new body");

        let report = migrate_product_meshes(&u.0);

        assert_eq!(report.moved_files, 1, "{report:?}");
        assert_eq!(report.kept.len(), 1);
        assert_eq!(fs::read(u.0.join(NEW).join("Drone/body.glb")).unwrap(), b"new body");
        assert!(u.has(&format!("{OLD}/Drone/body.glb")), "the colliding file stays");
        assert!(u.has(&format!("{NEW}/Drone/rotor.glb")));
    }

    #[test]
    fn a_product_whose_parts_name_the_old_path_is_left_alone() {
        let u = Universe::new();
        u.put("spaces/Space1/Workspace/Drone/generate_meshes.py", b"#");
        u.put(
            "spaces/Space1/Workspace/Drone/Body/_instance.toml",
            b"[asset]\nmesh = \"../../../.eustress/assets/meshes/Drone/body.glb\"\n",
        );
        u.put(&format!("{OLD}/Drone/body.glb"), b"body");

        let report = migrate_product_meshes(&u.0);

        assert_eq!(report.pinned_products, vec!["Drone".to_string()], "{report:?}");
        assert_eq!(report.moved_files, 0);
        assert!(u.has(&format!("{OLD}/Drone/body.glb")), "the part's own path still works");
    }

    #[test]
    fn running_it_again_finds_nothing_to_do() {
        let u = Universe::new();
        u.put("spaces/Space1/Workspace/Drone/generate_meshes.py", b"#");
        u.put(&format!("{OLD}/Drone/body.glb"), b"body");
        assert_eq!(migrate_product_meshes(&u.0).moved_files, 1);
        assert_eq!(migrate_product_meshes(&u.0), Migration::default());
        assert!(u.has(&format!("{NEW}/Drone/body.glb")));
    }

    #[test]
    fn a_universe_with_no_old_folder_is_a_no_op() {
        let u = Universe::new();
        u.put("spaces/Space1/Workspace/Drone/generate_meshes.py", b"#");
        assert_eq!(migrate_product_meshes(&u.0), Migration::default());
        assert!(!u.has(NEW), "nothing is created when there is nothing to move");
    }

    #[test]
    fn a_product_is_found_in_any_space_of_the_universe_and_in_the_older_layout() {
        let u = Universe::new();
        // One product in a second Space, one in a Space directly inside the Universe.
        u.put("spaces/Other/Workspace/Boat/generate_meshes.py", b"#");
        u.put("OldSpace/Workspace/Plane/generate_meshes.py", b"#");
        u.put(&format!("{OLD}/Boat/hull.glb"), b"hull");
        u.put(&format!("{OLD}/Plane/wing.glb"), b"wing");
        let report = migrate_product_meshes(&u.0);
        assert_eq!(report.moved_products, vec!["Boat".to_string(), "Plane".to_string()], "{report:?}");
    }

    #[test]
    fn nested_folders_move_with_their_files() {
        let u = Universe::new();
        u.put("spaces/Space1/Workspace/Drone/generate_meshes.py", b"#");
        u.put(&format!("{OLD}/Drone/lods/body_lod1.glb"), b"lod");
        let report = migrate_product_meshes(&u.0);
        assert_eq!(report.moved_files, 1, "{report:?}");
        assert!(u.has(&format!("{NEW}/Drone/lods/body_lod1.glb")));
        assert!(!u.has(&format!("{OLD}/Drone")));
    }
}
