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
// Table of Contents:
// 1. Arguments
// 2. Rendering
// 3. Splicing into the shell
// 4. Main
// =============================================================================

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use eustress_web::App;
use hydration_context::{SharedContext, SsrSharedContext};
use leptos::prelude::*;
use leptos_router::location::RequestUrl;

const SITE: &str = "https://eustress.dev";

/// The mount point exactly as index.html writes it.
const APP_MOUNT: &str = r#"<div id="app"></div>"#;
const APP_OPEN: &str = r#"<div id="app">"#;

/// The rendered markup is fenced inside `#app` so a later run can lift it
/// back out. `/` is written over `index.html`, which is also the shell every
/// other route is spliced into, so without this the bin could run once per
/// `trunk build`.
const RENDER_START: &str = "<!--prerender-->";
const RENDER_END: &str = "<!--/prerender-->";

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
         the shell. With no routes, every <loc> in <dist>/sitemap.xml is used."
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

/// Every `<loc>` in the sitemap, as a site-relative path.
fn routes_from_sitemap(dist: &Path) -> Vec<String> {
    let path = dist.join("sitemap.xml");
    let xml = fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", path.display());
        std::process::exit(2);
    });
    let mut routes = Vec::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find("<loc>") {
        let after = &rest[start + "<loc>".len()..];
        let Some(end) = after.find("</loc>") else { break };
        if let Some(path) = after[..end].trim().strip_prefix(SITE) {
            routes.push(if path.is_empty() { "/".to_string() } else { path.to_string() });
        }
        rest = &after[end..];
    }
    routes
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

// -----------------------------------------------------------------------------
// 3. Splicing into the shell
// -----------------------------------------------------------------------------

/// Visible text of a fragment. Each tag becomes a space so `<span>of</span>
/// <br>Creation` reads "of Creation", not "ofCreation"; callers fold runs.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Text of the first `<h1>` in the page, tags stripped and whitespace folded.
fn first_h1(html: &str) -> Option<String> {
    let start = html.find("<h1")?;
    let open_end = start + html[start..].find('>')? + 1;
    let close = open_end + html[open_end..].find("</h1>")?;
    let text = strip_tags(&html[open_end..close]);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// Replace the text between the first `open` and the `close` that follows it,
/// keeping both markers. Returns false when the shell has no such field.
fn replace_between(page: &mut String, open: &str, close: &str, value: &str) -> bool {
    let Some(start) = page.find(open) else { return false };
    let value_start = start + open.len();
    let Some(len) = page[value_start..].find(close) else { return false };
    page.replace_range(value_start..value_start + len, value);
    true
}

/// The shell with this route's markup inside `#app` and its own URL and
/// title in the head.
fn splice(shell: &str, route: &str, body: &str) -> Result<String, String> {
    let Some(mount) = shell.find(APP_MOUNT) else {
        return Err(format!("shell has no `{APP_MOUNT}`"));
    };
    let mut page = String::with_capacity(shell.len() + body.len());
    page.push_str(&shell[..mount]);
    page.push_str(APP_OPEN);
    page.push_str(RENDER_START);
    page.push_str(body);
    page.push_str(RENDER_END);
    page.push_str("</div>");
    page.push_str(&shell[mount + APP_MOUNT.len()..]);

    // Every prerendered page is canonical for its own URL. The shell says `/`
    // everywhere, which is right for the SPA fallback and wrong for a page.
    let url = format!("{SITE}{route}");
    replace_between(&mut page, r#"<link rel="canonical" href=""#, "\"", &url);
    replace_between(&mut page, r#"<meta property="og:url" content=""#, "\"", &url);
    replace_between(&mut page, r#"<meta name="twitter:url" content=""#, "\"", &url);

    // The home page keeps the hand-written title. Every other route is named
    // after its own heading, so a search result or a link preview says what
    // the page is rather than what the site is.
    if route != "/" {
        if let Some(h1) = first_h1(body) {
            let title = format!("{h1} | Eustress Engine");
            let attr = title.replace('"', "&quot;");
            replace_between(&mut page, "<title>", "</title>", &title);
            replace_between(&mut page, r#"<meta name="title" content=""#, "\"", &attr);
            replace_between(&mut page, r#"<meta property="og:title" content=""#, "\"", &attr);
            replace_between(&mut page, r#"<meta name="twitter:title" content=""#, "\"", &attr);
        }
    }
    Ok(page)
}

/// `index.html` as Trunk wrote it. When a previous run has already put the
/// home page inside `#app`, the fenced render is lifted out again; the head
/// needs no undoing because `/` keeps the shell's own title and URLs.
fn pristine_shell(html: &str) -> Option<String> {
    if html.contains(APP_MOUNT) {
        return Some(html.to_string());
    }
    let start = html.find(APP_OPEN)?;
    let after_open = start + APP_OPEN.len();
    if !html[after_open..].starts_with(RENDER_START) {
        return None;
    }
    let end = after_open + html[after_open..].find(RENDER_END)? + RENDER_END.len();
    let rest = html[end..].strip_prefix("</div>")?;
    let mut shell = String::with_capacity(html.len());
    shell.push_str(&html[..start]);
    shell.push_str(APP_MOUNT);
    shell.push_str(rest);
    Some(shell)
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
            "{} has neither an empty `{APP_MOUNT}` nor a fenced render inside it, \
             so it is not a Trunk shell. Run `trunk build --release` first.",
            shell_path.display()
        );
        std::process::exit(2);
    };

    let routes = if args.routes.is_empty() {
        routes_from_sitemap(&args.dist)
    } else {
        args.routes.clone()
    };
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

    let mut failed = 0usize;
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
            }
            Err(msg) => {
                failed += 1;
                let one_line = msg.lines().map(str::trim).collect::<Vec<_>>().join(" | ");
                eprintln!("FAIL  {route:<26} {one_line}");
            }
        }
    }

    println!("prerendered {} of {} routes into {}", routes.len() - failed, routes.len(), args.dist.display());
    if failed > 0 && args.strict {
        std::process::exit(1);
    }
}
