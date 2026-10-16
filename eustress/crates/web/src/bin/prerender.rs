// =============================================================================
// Eustress Web - Prerender
// =============================================================================
// Renders the public routes of eustress.dev to static HTML.
//
// The site is a client-rendered Leptos app: the shell Trunk emits has an empty
// `#app`, and every route's markup is produced by the WASM bundle in the
// browser. Crawlers and agents that do not execute WebAssembly (every AI
// fetcher, and Googlebot until its renderer gets around to the page) see the
// `<head>` and nothing else. This bin renders each route on a native target
// and writes the result into a copy of the shell, so `dist/docs/website.html`
// carries the page text while the WASM build still boots on top of it
// (`mount_app` in lib.rs empties `#app` before mounting).
//
// Run after `trunk build --release`, from the crate directory:
//
//     cargo run --no-default-features --features ssr --bin prerender
//
// Routes come from `dist/sitemap.xml` unless given as arguments, so the set of
// prerendered pages is the set of public URLs by construction. A route that
// panics during render (a component touching the browser at construction) is
// reported and skipped: a missing prerender degrades to today's behaviour,
// not to a broken page. `--strict` turns any failure into a nonzero exit.
//
// Besides one page per route it writes, into the same `dist/`:
//
// - `404.html`: the router's own not-found page in the shell. Cloudflare Pages
//   serves a top-level 404.html, with a 404 status, for any address that is no
//   file and no rule in `_redirects`.
// - `sitemap.xml`, again, with a `<lastmod>` on every URL: the build date (UTC),
//   or `SOURCE_DATE_EPOCH` when it is set. One date for the whole site because
//   the pages are one bundle; a page's own source file has no better date once
//   the build checkout is shallow.
// - `llms-full.txt`: the full text of the docs and learn pages (and of the
//   Markdown docs `llms.txt` lists), from the markup rendered here, so it is
//   as current as the build.
//
// The head and text rules live in `eustress_web::seo`, which the live app and
// the tests share.
//
// Table of Contents:
// 1. Arguments
// 2. Rendering
// 3. Files written besides the pages
// 4. Main
// =============================================================================

// The same limit as the library, for the same reason: `App().to_html()` is
// instantiated here, so the layout of every page's nested view type is
// computed in this crate, and the login page with the nav inside it runs past
// the default depth of 128.
#![recursion_limit = "512"]

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use eustress_web::seo::{
    build_date, first_h1, is_doc_route, llms_full, pristine_shell, routes_from_sitemap, sitemap_with_lastmod, splice, splice_not_found,
    strip_tags, DocPage, MarkdownDoc, MARKDOWN_DOCS, NOT_FOUND_PROBE,
};
use eustress_web::App;
use hydration_context::{SharedContext, SsrSharedContext};
use leptos::prelude::*;
use leptos_router::location::RequestUrl;

// -----------------------------------------------------------------------------
// 1. Arguments
// -----------------------------------------------------------------------------

struct Args {
    dist: PathBuf,
    strict: bool,
    routes: Vec<String>,
}

fn print_usage() {
    eprintln!(
        "usage: prerender [--dist <dir>] [--strict] [/route ...]\n\
         \n\
         Renders each route into <dist>/<route>.html using <dist>/index.html as\n\
         the shell. With no routes, every <loc> in <dist>/sitemap.xml is used, and\n\
         the sitemap, llms-full.txt and 404.html are written as well."
    );
}

fn parse_args() -> Args {
    let mut dist = PathBuf::from("dist");
    let mut strict = false;
    let mut routes = Vec::new();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--dist" => {
                dist = PathBuf::from(it.next().unwrap_or_else(|| {
                    eprintln!("--dist needs a directory");
                    std::process::exit(2);
                }))
            }
            "--strict" => strict = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            route if route.starts_with('/') => routes.push(route.to_string()),
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                std::process::exit(2);
            }
        }
    }
    Args { dist, strict, routes }
}

/// The sitemap's routes, as site-relative paths.
fn sitemap_routes(dist: &Path) -> Vec<String> {
    let path = dist.join("sitemap.xml");
    let xml = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", path.display());
        std::process::exit(2);
    });
    routes_from_sitemap(&xml)
}

// -----------------------------------------------------------------------------
// 2. Rendering
// -----------------------------------------------------------------------------

thread_local! {
    /// The message of the last panic, captured by the hook installed in main
    /// so a failing route reports its cause on one line instead of dumping
    /// a backtrace into the build log.
    static LAST_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Render one route to the markup the browser would build for it.
fn render_route(route: &str) -> Result<String, String> {
    let path = route.to_string();
    LAST_PANIC.with(|p| *p.borrow_mut() = None);
    std::panic::catch_unwind(move || {
        // A root owner with a server shared context is what the official
        // integrations build per request; it is what lets Suspense and the
        // router render without a browser.
        let shared: Arc<dyn SharedContext + Send + Sync> = Arc::new(SsrSharedContext::new());
        let owner = Owner::new_root(Some(shared));
        let html = owner.with(|| {
            provide_context(RequestUrl::new(&path));
            App().to_html()
        });
        owner.cleanup();
        html
    })
    .map_err(|_| {
        LAST_PANIC
            .with(|p| p.borrow().clone())
            .unwrap_or_else(|| "panicked without a message".to_string())
    })
}

/// `/` is the shell itself (which is also the SPA fallback); any other route
/// becomes `<route>.html`, which Cloudflare Pages serves at the extensionless
/// URL without adding a trailing slash.
fn output_path(dist: &Path, route: &str) -> PathBuf {
    if route == "/" {
        dist.join("index.html")
    } else {
        dist.join(format!("{}.html", route.trim_start_matches('/')))
    }
}

// -----------------------------------------------------------------------------
// 3. Files written besides the pages
// -----------------------------------------------------------------------------

/// Write beside the target and rename over it, so a reader never sees half a
/// file and an interrupted run leaves the old one.
fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let temp = path.with_extension("tmp-prerender");
    fs::write(&temp, contents).map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
    fs::rename(&temp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

/// The Markdown docs `llms.txt` lists, as Trunk copied them into `dist/`. A
/// missing one is reported and left out, not fatal.
fn markdown_docs(dist: &Path) -> Vec<MarkdownDoc> {
    let mut docs = Vec::new();
    for path in MARKDOWN_DOCS {
        match fs::read_to_string(dist.join(path)) {
            Ok(text) => docs.push(MarkdownDoc { path: path.to_string(), text }),
            Err(e) => eprintln!("note  {path} is not in {}: {e}", dist.display()),
        }
    }
    docs
}

// -----------------------------------------------------------------------------
// 4. Main
// -----------------------------------------------------------------------------

fn main() {
    let args = parse_args();

    let shell_path = args.dist.join("index.html");
    let written = fs::read_to_string(&shell_path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", shell_path.display());
        std::process::exit(2);
    });
    let Some(shell) = pristine_shell(&written) else {
        eprintln!(
            "{} has neither an empty mount point nor a fenced render inside it, \
             so it is not a Trunk shell. Run `trunk build --release` first.",
            shell_path.display()
        );
        std::process::exit(2);
    };

    let whole_site = args.routes.is_empty();
    let routes = if whole_site { sitemap_routes(&args.dist) } else { args.routes.clone() };
    if routes.is_empty() {
        eprintln!("no routes to render");
        std::process::exit(2);
    }

    // Park spawned futures instead of aborting on them (see Cargo.toml). A
    // second init is an error only because it already happened, so ignore it.
    let _ = leptos::task::Executor::init_futures_executor();
    // Printed as well as stored: a panic inside a Drop while unwinding aborts
    // the process before the route's FAIL line, and this is then the only
    // trace of which component reached for the browser.
    // A wasm-bindgen import shim is `extern "C"`, so a browser call on the
    // native target aborts instead of unwinding; with RUST_BACKTRACE set the
    // frames inside this crate are printed, which names the component.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("      panic: {}", info.to_string().lines().collect::<Vec<_>>().join(" | "));
        if std::env::var_os("RUST_BACKTRACE").is_some() {
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            let everything = std::env::var("RUST_BACKTRACE").map(|v| v == "full").unwrap_or(false);
            // Frames are printed as `eustress_web::...` symbols followed by
            // `at .\src\...` locations relative to the crate directory.
            let ours = |l: &str| {
                l.contains("eustress_web::")
                    || (l.contains("\\src\\") || l.contains("/src/"))
                        && !l.contains(".cargo")
                        && !l.contains(".rustup")
                        && !l.contains("/rustc/")
            };
            for frame in trace.lines().filter(|l| everything || ours(l)) {
                eprintln!("      {}", frame.trim());
            }
        }
        LAST_PANIC.with(|p| *p.borrow_mut() = Some(info.to_string()));
    }));

    // Pages that failed, and every failure (pages and the files beside them).
    let mut page_failed = 0usize;
    let mut failed = 0usize;
    let mut guides: Vec<DocPage> = Vec::new();
    for route in &routes {
        match render_route(route).and_then(|body| splice(&shell, route, &body).map(|page| (body, page))) {
            Ok((body, page)) => {
                let out = output_path(&args.dist, route);
                if let Some(parent) = out.parent() {
                    fs::create_dir_all(parent).unwrap_or_else(|e| {
                        eprintln!("cannot create {}: {e}", parent.display());
                        std::process::exit(2);
                    });
                }
                fs::write(&out, &page).unwrap_or_else(|e| {
                    eprintln!("cannot write {}: {e}", out.display());
                    std::process::exit(2);
                });
                // Characters of visible text in the rendered body: the number
                // that distinguishes a real page from an empty shell.
                let text = strip_tags(&body).split_whitespace().collect::<Vec<_>>().join(" ").chars().count();
                let title = first_h1(&body).unwrap_or_else(|| "(no h1)".to_string());
                println!("ok    {route:<26} {text:>6} chars  {title}");
                if is_doc_route(route) {
                    guides.push(DocPage { route: route.clone(), body });
                }
            }
            Err(msg) => {
                page_failed += 1;
                failed += 1;
                let one_line = msg.lines().map(str::trim).collect::<Vec<_>>().join(" | ");
                eprintln!("FAIL  {route:<26} {one_line}");
            }
        }
    }

    // The page for an address nothing owns: the router's own fallback.
    match render_route(NOT_FOUND_PROBE).and_then(|body| splice_not_found(&shell, &body)) {
        Ok(page) => match write_atomic(&args.dist.join("404.html"), &page) {
            Ok(()) => println!("ok    {:<26} not-found page", "404.html"),
            Err(msg) => {
                failed += 1;
                eprintln!("FAIL  404.html                   {msg}");
            }
        },
        Err(msg) => {
            failed += 1;
            let one_line = msg.lines().map(str::trim).collect::<Vec<_>>().join(" | ");
            eprintln!("FAIL  404.html                   {one_line}");
        }
    }

    // The whole-site files describe the whole site, so a run for a few routes
    // leaves them alone.
    if whole_site {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let date = build_date(std::env::var("SOURCE_DATE_EPOCH").ok().as_deref(), now);

        let sitemap_path = args.dist.join("sitemap.xml");
        let stamped = fs::read_to_string(&sitemap_path)
            .map_err(|e| format!("cannot read {}: {e}", sitemap_path.display()))
            .and_then(|xml| write_atomic(&sitemap_path, &sitemap_with_lastmod(&xml, &date)));
        match stamped {
            Ok(()) => println!("ok    {:<26} lastmod {date} on {} URLs", "sitemap.xml", routes.len()),
            Err(msg) => {
                failed += 1;
                eprintln!("FAIL  sitemap.xml                {msg}");
            }
        }

        if guides.is_empty() {
            failed += 1;
            eprintln!("FAIL  llms-full.txt              no docs or learn page was rendered");
        } else {
            let text = llms_full(&guides, &markdown_docs(&args.dist), &date);
            match write_atomic(&args.dist.join("llms-full.txt"), &text) {
                Ok(()) => println!("ok    {:<26} {} guides, {} bytes", "llms-full.txt", guides.len(), text.len()),
                Err(msg) => {
                    failed += 1;
                    eprintln!("FAIL  llms-full.txt              {msg}");
                }
            }
        }
    }

    println!("prerendered {} of {} routes into {}", routes.len() - page_failed, routes.len(), args.dist.display());
    if failed > 0 && args.strict {
        std::process::exit(1);
    }
}
