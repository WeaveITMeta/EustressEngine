//! `eustress-space` — headless open / verify / export for a `.eustress`
//! space, WITHOUT the engine.
//!
//! ```text
//! eustress-space open   <path>                 # entity count, class histogram,
//!                                               #   world bounds, voxel/Fjall info
//! eustress-space verify <path>                  # rkyv CheckBytes every core;
//!                                               #   exit nonzero if any fail (§8.C)
//! eustress-space export <path> [--out <dir>]    # binary -> readable TOML tree
//! ```
//!
//! `<path>` is either a `.eustress` space root (a directory containing
//! `world.fjalldb/`) or the `world.fjalldb/` directory itself. All reads
//! go through the worlddb crate — the engine is never linked.

use std::path::PathBuf;
use std::process::ExitCode;

use eustress_space::{export, open, verify, OpenReport, VerifyReport};

const USAGE: &str = "\
eustress-space — open/inspect/verify/export a .eustress space (no engine)

USAGE:
    eustress-space open   <path>
    eustress-space verify <path>
    eustress-space export <path> [--out <dir>]

ARGS:
    <path>    A .eustress space root (holds world.fjalldb/) OR the
              world.fjalldb/ directory itself.

OPTIONS (export):
    --out <dir>   Output directory for the readable .instance.toml tree.
                  Default: <path>/export_toml

SUBCOMMANDS:
    open      Print entity count, class histogram, world bounds, voxel-chunk
              + Fjall info. The \"did it load + what's in it\" check.
    verify    rkyv::access (CheckBytes) every instance core. Prints
              \"N OK, M failed\" and a failure list. Exits non-zero if any fail.
    export    Project each binary core to a readable <class>/<entity>.instance.toml
              tree — the binary -> readable portability escape hatch.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::FAILURE
        }
    }
}

/// Parse + dispatch. Returns the process exit code on success-of-parsing
/// (a failing `verify` still parses fine but yields a non-zero code), or
/// an error string for usage / IO / open failures.
fn run(args: &[String]) -> Result<ExitCode, String> {
    let sub = match args.first() {
        Some(s) => s.as_str(),
        None => {
            println!("{USAGE}");
            return Ok(ExitCode::FAILURE);
        }
    };

    match sub {
        "open" => {
            let path = positional_path(&args[1..])?;
            let report = open(&path).map_err(|e| e.to_string())?;
            print_open(&report);
            Ok(ExitCode::SUCCESS)
        }
        "verify" => {
            let path = positional_path(&args[1..])?;
            let report = verify(&path).map_err(|e| e.to_string())?;
            print_verify(&report);
            if report.passed() {
                Ok(ExitCode::SUCCESS)
            } else {
                // The §8.C gate: any round-trip failure => non-zero exit so
                // CI / import pipelines fail loudly.
                Ok(ExitCode::FAILURE)
            }
        }
        "export" => {
            let (path, out) = parse_export(&args[1..])?;
            let out_dir = out.unwrap_or_else(|| path.join("export_toml"));
            let report = export(&path, &out_dir).map_err(|e| e.to_string())?;
            println!(
                "Exported {} core(s) to {} ({} skipped/undecodable).",
                report.written,
                report.out_dir.display(),
                report.skipped
            );
            Ok(ExitCode::SUCCESS)
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!(
            "unknown subcommand '{other}'.\n\n{USAGE}"
        )),
    }
}

/// Pull the single positional `<path>` arg, rejecting stray flags so a
/// typo doesn't silently open the wrong thing.
fn positional_path(rest: &[String]) -> Result<PathBuf, String> {
    let mut path: Option<PathBuf> = None;
    for a in rest {
        if a.starts_with('-') {
            return Err(format!("unexpected option '{a}'\n\n{USAGE}"));
        }
        if path.is_some() {
            return Err(format!("unexpected extra argument '{a}'\n\n{USAGE}"));
        }
        path = Some(PathBuf::from(a));
    }
    path.ok_or_else(|| format!("missing <path>\n\n{USAGE}"))
}

/// Parse `export`'s `<path> [--out <dir>]`.
fn parse_export(rest: &[String]) -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut path: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        match a.as_str() {
            "--out" | "-o" => {
                i += 1;
                let v = rest
                    .get(i)
                    .ok_or_else(|| format!("--out requires a directory\n\n{USAGE}"))?;
                out = Some(PathBuf::from(v));
            }
            // `--out=<dir>` form.
            s if s.starts_with("--out=") => {
                out = Some(PathBuf::from(&s["--out=".len()..]));
            }
            s if s.starts_with('-') => {
                return Err(format!("unexpected option '{s}'\n\n{USAGE}"));
            }
            s => {
                if path.is_some() {
                    return Err(format!("unexpected extra argument '{s}'\n\n{USAGE}"));
                }
                path = Some(PathBuf::from(s));
            }
        }
        i += 1;
    }
    let path = path.ok_or_else(|| format!("missing <path>\n\n{USAGE}"))?;
    Ok((path, out))
}

// ── rendering ────────────────────────────────────────────────────────

fn print_open(r: &OpenReport) {
    println!("== .eustress space ==");
    if let Some(h) = &r.header {
        println!("world id      : {}", h.world_id);
        println!("engine        : v{}", h.engine_semver);
        match &h.migrated_at {
            Some(ts) => println!("migrated_at   : {ts} (DB-authoritative)"),
            None => println!("migrated_at   : (not migrated — legacy/loose)"),
        }
    } else {
        println!("header.bin    : (none alongside world.fjalldb/)");
    }
    match r.schema_version {
        Some(v) => println!("schema ver    : v{v}"),
        None => println!("schema ver    : (unknown)"),
    }
    println!();
    println!("entity cores  : {}", r.entity_count);
    if r.undecodable > 0 {
        println!(
            "  WARNING     : {} core(s) failed to decode (run `verify` for details)",
            r.undecodable
        );
    }

    match &r.bounds {
        Some(b) => {
            println!(
                "world bounds  : min [{:.3}, {:.3}, {:.3}]  max [{:.3}, {:.3}, {:.3}]",
                b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]
            );
        }
        None => println!("world bounds  : (no decodable cores)"),
    }

    match r.voxel_chunk_count {
        Some(n) => println!("voxel chunks  : {n}"),
        None => println!(
            "voxel chunks  : (voxel-chunk store not present in this worlddb build — Wave 9.A)"
        ),
    }
    // Fjall segment/partition info is not cheaply exposed by the worlddb
    // read API today; surface what we know rather than reaching past the trait.
    println!("fjall info    : opened via FjallWorldDb (entities/tree/datastore/uuid partitions)");

    println!();
    println!("class histogram ({} classes):", r.class_histogram.len());
    if r.class_histogram.is_empty() {
        println!("  (no instance cores)");
    } else {
        let width = r
            .class_histogram
            .iter()
            .map(|c| c.class_name.len())
            .max()
            .unwrap_or(0);
        for c in &r.class_histogram {
            println!("  {:<width$}  {:>8}", c.class_name, c.count, width = width);
        }
    }
}

fn print_verify(r: &VerifyReport) {
    let total = r.ok + r.failures.len();
    println!(
        "verify: {} core(s) OK, {} failed (of {total} total)",
        r.ok,
        r.failures.len()
    );
    if r.failures.is_empty() {
        println!("PASS — every instance core round-trips (rkyv CheckBytes).");
    } else {
        println!("FAIL — the following cores did not validate:");
        for f in &r.failures {
            println!("  entity {:<20}  {}", f.entity, f.reason);
        }
    }
}
