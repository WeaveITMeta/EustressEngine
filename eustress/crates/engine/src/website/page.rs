//! # Baking the sim page (PageSpec v1)
//!
//! Turns the Website service's theme fields, its image slots and its
//! `PageLink` children into the one JSON document the Worker's
//! `PUT /api/simulations/{id}/page` takes, plus the image bytes it uploads
//! beside it. The contract and the rules are in
//! `docs/design/SIM_PAGE_THEME_AND_LINKS.md` and
//! [`eustress_common::page_spec`]; this module only reads the Space and does
//! the file I/O, and `page_spec` decides what is valid.
//!
//! ## Separate from the numbers manifest
//!
//! The numbers manifest ([`super::bake`]) fails a publish on a bad Reference.
//! The page is the opposite: an invalid entry is dropped with a warning and the
//! rest publishes, because a mistyped colour must not stop a Space going out.
//! A Space with no theme chosen, no image and no `PageLink` bakes nothing
//! ([`PageBake::page`] is `None`), so every existing Space publishes as before.
//!
//! ## The README
//!
//! `Website/README.md`, when there is one, is the page's About tab
//! (`docs/design/SIM_PAGE_README_AND_TABS.md`). It bakes on its own
//! ([`PageBake::readme`]), so a README alone, with no theme or link, still
//! uploads. The file is refused over [`page_spec::README_MAX_BYTES`] or when it
//! is not UTF-8; each image it shows is read here and checked like a page
//! image, and [`page_spec::rewrite_readme`] points it at its `readme-N` slot or
//! shows its alt text. With no usable README the bake is
//! [`ReadmeBake::Absent`], and the publish removes any README published before.
//!
//! ## Reads through the Space's source
//!
//! Like the manifest bake, the reads go through a [`SpaceSource`], so a Space
//! held in the world database and one on disk bake the same.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use bevy::prelude::*;
use eustress_common::page_spec::{self, PageInput};

use super::bake::SERVICE_FOLDER;
use crate::space::space_source::{ActiveSpaceSource, DiskSource, SpaceSource};

/// One image to upload: the slot it goes to and its checked bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct PageImage {
    /// `logo`, `banner`, `gallery-N`, or `readme-N` for a README image.
    pub slot: String,
    /// `image/png`, `image/jpeg` or `image/webp`, by the bytes' signature.
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// A baked page, ready to upload.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingPage {
    /// The PageSpec v1 JSON body for `PUT .../page`.
    pub spec_json: String,
    /// Images to upload to `PUT .../page/image/{slot}` before the spec.
    pub images: Vec<PageImage>,
    /// How many links the spec carries.
    pub link_count: usize,
}

/// The README for the page's About tab, baked apart from the PageSpec.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ReadmeBake {
    /// No usable `Website/README.md`: none, empty, or refused with a warning.
    /// The publish sends `DELETE .../page/readme` so a README published before
    /// does not outlive the file.
    #[default]
    Absent,
    /// Upload each image to `PUT .../page/image/readme-N`, then `markdown` to
    /// `PUT .../page/readme`.
    Present {
        /// The README with each image that loaded pointed at its slot
        /// (`eustress-image:readme-N`) and every other image as its alt text.
        markdown: String,
        /// The README images in slot order, `readme-0` upward.
        images: Vec<PageImage>,
    },
}

/// The result of a page bake.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PageBake {
    /// `None` when nothing was set.
    pub page: Option<PendingPage>,
    /// The README, independent of `page`: a README alone still uploads.
    pub readme: ReadmeBake,
    /// One line per entry dropped, and why, for the publish log.
    pub warnings: Vec<String>,
}

/// Bake the page for the Space the world has open.
pub fn bake_page_from_world(world: &World, space_root: &Path) -> Result<PageBake, String> {
    let source: Arc<dyn SpaceSource> = world
        .get_resource::<ActiveSpaceSource>()
        .map(|s| s.0.clone())
        .unwrap_or_else(|| Arc::new(DiskSource::new(space_root)));
    bake_page(source.as_ref())
}

/// The page settings in a `_service.toml`: theme, image paths and, from the
/// `PageLink` children, links. `Ok(None)` when the Space has no Website service.
pub fn read_page_input(source: &dyn SpaceSource) -> Result<Option<(PageInput, Vec<String>)>, String> {
    let service_rel = format!("{SERVICE_FOLDER}/_service.toml");
    let Ok(text) = source.read_to_string(&service_rel) else {
        return Ok(None);
    };
    let doc: toml::Value = text.parse().map_err(|e: toml::de::Error| format!("{service_rel}: {e}"))?;

    let mut input = PageInput::default();
    page_spec::read_service_doc(&doc, &mut input);

    // PageLink children, in folder-name order so a bake is deterministic.
    let mut notes: Vec<String> = Vec::new();
    let entries = match source.list(SERVICE_FOLDER) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("cannot list the {SERVICE_FOLDER} folder: {e}")),
    };
    let mut rels: Vec<(String, String)> = Vec::new();
    for entry in entries {
        if entry.is_dir {
            let marker = format!("{}/_instance.toml", entry.rel_path);
            if source.exists(&marker) {
                rels.push((entry.name.clone(), marker));
            }
        }
    }
    rels.sort();
    for (name, rel) in rels {
        let Ok(text) = source.read_to_string(&rel) else {
            continue;
        };
        let doc: toml::Value = match text.parse() {
            Ok(d) => d,
            Err(e) => {
                notes.push(format!("link {name} dropped: {rel} did not parse: {e}"));
                continue;
            }
        };
        if let Some(link) = page_spec::link_from_instance_doc(&name, &doc) {
            input.links.push(link);
        }
    }
    Ok(Some((input, notes)))
}

/// Bake the page from a Space's source: the PageSpec ([`bake_spec`]) and,
/// apart from it, the README ([`bake_readme`]).
pub fn bake_page(source: &dyn SpaceSource) -> Result<PageBake, String> {
    let mut warnings: Vec<String> = Vec::new();
    let page = bake_spec(source, &mut warnings)?;
    let readme = bake_readme(source, &mut warnings);
    Ok(PageBake { page, readme, warnings: prefixed(warnings) })
}

/// The PageSpec: read the settings, read and check every image, then let
/// [`page_spec::build_page_spec`] apply the rules. `None` when the Space has
/// no Website service or set nothing.
fn bake_spec(source: &dyn SpaceSource, warnings: &mut Vec<String>) -> Result<Option<PendingPage>, String> {
    let Some((mut input, notes)) = read_page_input(source)? else {
        return Ok(None);
    };
    warnings.extend(notes);

    // Read and check each image first, and blank the ones that fail, so the
    // spec never names a slot nothing will be uploaded to.
    let mut loaded: HashMap<String, (&'static str, Vec<u8>)> = HashMap::new();
    let mut load = |what: &str, raw: &mut String, warnings: &mut Vec<String>| {
        if raw.trim().is_empty() {
            return;
        }
        // A path that fails its own rules is left for the builder to name.
        let Ok(clean) = page_spec::clean_image_path(raw) else {
            return;
        };
        let checked = source
            .read(&clean)
            .map_err(|e| format!("cannot read it: {e}"))
            .and_then(|bytes| page_spec::check_image_bytes(&bytes).map(|ct| (ct, bytes)));
        match checked {
            Ok(found) => {
                loaded.insert(clean, found);
            }
            Err(e) => {
                warnings.push(format!("{what} image {clean} dropped: {e}"));
                raw.clear();
            }
        }
    };
    load("logo", &mut input.logo, warnings);
    load("banner", &mut input.banner, warnings);
    for raw in input.gallery.iter_mut() {
        load("gallery", raw, warnings);
    }

    let build = page_spec::build_page_spec(&input);
    warnings.extend(build.warnings);

    let Some(spec) = build.spec else {
        return Ok(None);
    };
    let link_count = spec["links"].as_array().map(|l| l.len()).unwrap_or(0);
    let mut images: Vec<PageImage> = Vec::new();
    for (slot, path) in build.images {
        if let Some((content_type, bytes)) = loaded.get(&path) {
            images.push(PageImage { slot, content_type, bytes: bytes.clone() });
        }
    }
    let spec_json = serde_json::to_string(&spec).map_err(|e| format!("page spec serialize: {e}"))?;
    Ok(Some(PendingPage { spec_json, images, link_count }))
}

/// The README: `Website/README.md` read through the source, refused (with a
/// warning) over [`page_spec::README_MAX_BYTES`], when it is not UTF-8, or when
/// its image rewrite takes it over the cap; each image it shows read and
/// checked with [`page_spec::check_image_bytes`]. A Space with no README, or
/// an empty one, is [`ReadmeBake::Absent`] with no warning.
pub fn bake_readme(source: &dyn SpaceSource, warnings: &mut Vec<String>) -> ReadmeBake {
    let rel = page_spec::readme_rel_path();
    let bytes = match source.read(&rel) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ReadmeBake::Absent,
        Err(e) => {
            warnings.push(format!("{} not uploaded: cannot read it: {e}", page_spec::README_FILE));
            return ReadmeBake::Absent;
        }
    };
    let text = match page_spec::check_readme_text(&bytes) {
        Ok(text) => text,
        Err(e) => {
            warnings.push(format!(
                "{} not uploaded: {e}. The About tab shows no README until it is fixed.",
                page_spec::README_FILE
            ));
            return ReadmeBake::Absent;
        }
    };
    if text.trim().is_empty() {
        return ReadmeBake::Absent;
    }

    // Each image is read once, when the rewrite first meets it, and kept for
    // the upload.
    let mut loaded: HashMap<String, (&'static str, Vec<u8>)> = HashMap::new();
    let build = page_spec::rewrite_readme(text, |path| {
        let bytes = source.read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => format!("there is no file {path} in the Space"),
            _ => format!("cannot read {path}: {e}"),
        })?;
        let content_type = page_spec::check_image_bytes(&bytes).map_err(|e| format!("{path}: {e}"))?;
        loaded.insert(path.to_string(), (content_type, bytes));
        Ok(())
    });
    warnings.extend(build.warnings);

    // The Worker caps the text it receives, and a short path rewritten to
    // its slot grows.
    if build.markdown.len() > page_spec::README_MAX_BYTES {
        warnings.push(format!(
            "{} not uploaded: with its images pointed at their slots it is {:.1} KiB; the limit is {} KiB",
            page_spec::README_FILE,
            build.markdown.len() as f64 / 1024.0,
            page_spec::README_MAX_BYTES / 1024
        ));
        return ReadmeBake::Absent;
    }

    let mut images: Vec<PageImage> = Vec::new();
    for (slot, path) in build.images {
        if let Some((content_type, bytes)) = loaded.remove(&path) {
            images.push(PageImage { slot, content_type, bytes });
        }
    }
    ReadmeBake::Present { markdown: build.markdown, images }
}

fn prefixed(warnings: Vec<String>) -> Vec<String> {
    warnings.into_iter().map(|w| format!("Website page: {w}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::space_source::RecordsSource;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn f(path: &str, bytes: impl AsRef<[u8]>) -> (String, Vec<u8>) {
        (path.to_string(), bytes.as_ref().to_vec())
    }

    fn space(files: Vec<(String, Vec<u8>)>) -> RecordsSource {
        RecordsSource::new(files)
    }

    fn link_file(kind: &str, url: &str, label: &str) -> Vec<u8> {
        format!(
            "[metadata]\nclass_name = \"PageLink\"\n\n[attributes]\nkind = \"{kind}\"\nurl = \"{url}\"\nlabel = \"{label}\"\n"
        )
        .into_bytes()
    }

    #[test]
    fn a_space_with_no_website_service_bakes_nothing() {
        let bake = bake_page(&space(vec![f("Workspace/a/_instance.toml", b"x")])).unwrap();
        assert_eq!(bake, PageBake::default());
        assert_eq!(bake.readme, ReadmeBake::Absent, "no README means the publish removes one");
    }

    fn readme_of(bake: &PageBake) -> (&str, &[PageImage]) {
        match &bake.readme {
            ReadmeBake::Present { markdown, images } => (markdown.as_str(), images.as_slice()),
            ReadmeBake::Absent => panic!("expected a README, got Absent; warnings {:?}", bake.warnings),
        }
    }

    #[test]
    fn a_readme_alone_bakes_with_no_page() {
        let template = include_str!("../../../common/assets/service_templates/Website/_service.toml");
        let readme = "# Battery lab\n\nSee [our paper](https://example.edu/paper).\n\n![Cell](../Assets/cell.png)\n";
        for files in [
            // The scaffolded service, nothing chosen.
            vec![f("Website/_service.toml", template.as_bytes()), f("Website/README.md", readme), f("Assets/cell.png", PNG)],
            // No _service.toml at all.
            vec![f("Website/README.md", readme), f("Assets/cell.png", PNG)],
        ] {
            let bake = bake_page(&space(files)).unwrap();
            assert!(bake.page.is_none(), "no theme, image or link was set");
            assert!(bake.warnings.is_empty(), "{:?}", bake.warnings);
            let (markdown, images) = readme_of(&bake);
            assert_eq!(
                markdown,
                "# Battery lab\n\nSee [our paper](https://example.edu/paper).\n\n![Cell](eustress-image:readme-0)\n"
            );
            assert_eq!(images.len(), 1);
            assert_eq!(images[0].slot, "readme-0");
            assert_eq!(images[0].content_type, "image/png");
            assert_eq!(images[0].bytes, PNG);
        }
    }

    #[test]
    fn a_readme_bakes_beside_a_page_without_touching_it() {
        let service = "[service]\ntheme_preset = \"paper\"\nimage_logo = \"Assets/logo.png\"\n";
        let bake = bake_page(&space(vec![
            f("Website/_service.toml", service.as_bytes()),
            f("Website/README.md", "![Logo](../Assets/logo.png)"),
            f("Assets/logo.png", PNG),
        ]))
        .unwrap();
        assert!(bake.warnings.is_empty(), "{:?}", bake.warnings);
        let page = bake.page.as_ref().expect("a theme was chosen");
        let slots: Vec<&str> = page.images.iter().map(|i| i.slot.as_str()).collect();
        assert_eq!(slots, vec!["logo"], "README images are not page images");
        let (markdown, images) = readme_of(&bake);
        assert_eq!(markdown, "![Logo](eustress-image:readme-0)");
        assert_eq!(images[0].slot, "readme-0", "the same file uploads to its own README slot too");
    }

    #[test]
    fn an_oversize_or_non_utf8_readme_is_dropped_with_a_warning() {
        let over = vec![b'a'; page_spec::README_MAX_BYTES + 1];
        let bake = bake_page(&space(vec![f("Website/README.md", &over)])).unwrap();
        assert_eq!(bake.readme, ReadmeBake::Absent);
        assert_eq!(bake.warnings.len(), 1, "{:?}", bake.warnings);
        assert!(bake.warnings[0].starts_with("Website page: README.md not uploaded"), "{}", bake.warnings[0]);
        assert!(bake.warnings[0].contains("32 KiB"), "{}", bake.warnings[0]);

        // Exactly at the cap still uploads.
        let at_cap = vec![b'a'; page_spec::README_MAX_BYTES];
        let bake = bake_page(&space(vec![f("Website/README.md", &at_cap)])).unwrap();
        assert_eq!(readme_of(&bake).0.len(), page_spec::README_MAX_BYTES);

        let bake = bake_page(&space(vec![f("Website/README.md", [0xC3, 0x28, b'x'])])).unwrap();
        assert_eq!(bake.readme, ReadmeBake::Absent);
        assert!(bake.warnings[0].contains("UTF-8"), "{:?}", bake.warnings);
    }

    #[test]
    fn a_readme_the_rewrite_takes_over_the_cap_is_dropped() {
        // 11 bytes each; each grows to 29 once pointed at its slot.
        let text = "![](a.png) ".repeat(2900);
        assert!(text.len() <= page_spec::README_MAX_BYTES);
        let bake = bake_page(&space(vec![f("Website/README.md", text), f("Website/a.png", PNG)])).unwrap();
        assert_eq!(bake.readme, ReadmeBake::Absent);
        assert!(bake.warnings.iter().any(|w| w.contains("pointed at their slots")), "{:?}", bake.warnings);
    }

    #[test]
    fn an_empty_readme_is_absent_without_a_warning() {
        let bake = bake_page(&space(vec![f("Website/README.md", " \n\n")])).unwrap();
        assert_eq!(bake.readme, ReadmeBake::Absent);
        assert!(bake.warnings.is_empty(), "{:?}", bake.warnings);
    }

    #[test]
    fn a_missing_or_bad_readme_image_becomes_alt_text_with_a_warning() {
        let readme = "![Cell](Assets/missing.png) ![Gif](notpng.png) ![Ok](../Assets/ok.png) ![Remote](https://evil.example/p.png)";
        let bake = bake_page(&space(vec![
            f("Website/README.md", readme),
            f("Website/notpng.png", b"GIF89a not a png"),
            f("Assets/ok.png", PNG),
        ]))
        .unwrap();
        let (markdown, images) = readme_of(&bake);
        assert_eq!(markdown, "Cell Gif ![Ok](eustress-image:readme-0) Remote");
        let slots: Vec<(&str, &str)> = images.iter().map(|i| (i.slot.as_str(), i.content_type)).collect();
        assert_eq!(slots, vec![("readme-0", "image/png")]);
        assert_eq!(bake.warnings.len(), 3, "{:?}", bake.warnings);
        assert!(bake.warnings.iter().all(|w| w.starts_with("Website page: README.md image ")), "{:?}", bake.warnings);
        assert!(bake.warnings[0].contains("Website/Assets/missing.png"), "{}", bake.warnings[0]);
        assert!(bake.warnings[1].contains("PNG, JPEG or WebP"), "{}", bake.warnings[1]);
        assert!(bake.warnings[2].contains("remote"), "{}", bake.warnings[2]);
    }

    #[test]
    fn the_scaffolded_service_bakes_nothing() {
        let template = include_str!("../../../common/assets/service_templates/Website/_service.toml");
        let bake = bake_page(&space(vec![f("Website/_service.toml", template.as_bytes())])).unwrap();
        assert!(bake.page.is_none(), "the shipped defaults mean no theme chosen");
        assert!(bake.warnings.is_empty(), "{:?}", bake.warnings);
    }

    #[test]
    fn a_theme_a_logo_and_links_bake_and_upload_their_images() {
        let service = "[service]\nclass_name = \"Website\"\nnamespace = \"x\"\ntheme_preset = \"arcade\"\ntheme_color_primary = \"#ff00aa\"\ntheme_font = \"jetbrains-mono\"\nslideshow_interval_s = 12\nimage_logo = \"Assets/logo.png\"\nimage_gallery_1 = \"Assets/g.png\"\n";
        let bake = bake_page(&space(vec![
            f("Website/_service.toml", service.as_bytes()),
            f("Website/lab/_instance.toml", &link_file("website", "https://example-robotics.com", "Our lab")),
            f("Website/yt/_instance.toml", &link_file("social", "https://youtube.com/@example", "YouTube")),
            f("Website/specific_energy/_instance.toml", b"[metadata]\nclass_name = \"Reference\"\n\n[attributes]\nkind = \"count\"\n"),
            f("Assets/logo.png", PNG),
            f("Assets/g.png", PNG),
        ]))
        .unwrap();
        assert!(bake.warnings.is_empty(), "{:?}", bake.warnings);
        let page = bake.page.expect("a theme was chosen");
        assert_eq!(page.link_count, 2, "the Reference is not a link");
        let spec: serde_json::Value = serde_json::from_str(&page.spec_json).unwrap();
        assert_eq!(spec["schema"], 1);
        assert_eq!(spec["theme"]["preset"], "arcade");
        assert_eq!(spec["theme"]["colors"]["primary"], "#FF00AA");
        assert_eq!(spec["theme"]["font"], "jetbrains-mono");
        assert_eq!(spec["theme"]["slideshow_interval_s"], 12);
        assert_eq!(spec["images"]["logo"], "logo");
        assert_eq!(spec["images"]["gallery"], serde_json::json!(["gallery-0"]));
        let slots: Vec<&str> = page.images.iter().map(|i| i.slot.as_str()).collect();
        assert_eq!(slots, vec!["logo", "gallery-0"]);
        assert!(page.images.iter().all(|i| i.content_type == "image/png"));
    }

    #[test]
    fn a_bad_image_is_dropped_with_a_warning_and_the_page_still_bakes() {
        let service = "[service]\nimage_logo = \"Assets/missing.png\"\nimage_banner = \"Assets/notpng.png\"\nimage_gallery_0 = \"Assets/big.png\"\n";
        let mut big = PNG.to_vec();
        big.resize(page_spec::MAX_IMAGE_BYTES + 1, 0);
        let bake = bake_page(&space(vec![
            f("Website/_service.toml", service.as_bytes()),
            f("Website/lab/_instance.toml", &link_file("website", "https://example-robotics.com", "Our lab")),
            f("Assets/notpng.png", b"GIF89a not a png"),
            f("Assets/big.png", &big),
        ]))
        .unwrap();
        assert_eq!(bake.warnings.len(), 3, "{:?}", bake.warnings);
        assert!(bake.warnings.iter().all(|w| w.starts_with("Website page: ")));
        let page = bake.page.expect("the link still publishes");
        assert!(page.images.is_empty(), "no image survived");
        let spec: serde_json::Value = serde_json::from_str(&page.spec_json).unwrap();
        assert!(spec["images"].get("logo").is_none());
        assert_eq!(spec["images"]["gallery"], serde_json::json!([]));
    }

    #[test]
    fn a_risky_looking_link_is_dropped_before_upload() {
        let bake = bake_page(&space(vec![
            f("Website/_service.toml", b"[service]\nnamespace = \"x\"\n"),
            f("Website/a/_instance.toml", &link_file("shop", "http://shop.example-robotics.com", "Shop")),
            f("Website/b/_instance.toml", &link_file("shop", "https://127.0.0.1/pay", "Pay")),
        ]))
        .unwrap();
        assert!(bake.page.is_none(), "both links were dropped and nothing else was set");
        assert_eq!(bake.warnings.len(), 2, "{:?}", bake.warnings);
    }
}
