//! `eustress-agent-census`: the tool-surface conformance census (G7.01).
//!
//! Exit codes:
//! * 0: complete. Every defined descriptor has a classified valid-input probe
//!   and a classified missing-parameter probe, every valid-input probe has a
//!   p50 and p95 latency, and the repetition floor was met.
//! * 1: the artifact was written but the census is incomplete (a descriptor
//!   was skipped, the run aborted, or the repetitions were under the floor).
//! * 2: usage or setup error; no artifact.
//! * 3: refused for safety (the MCP server would act outside the fixture).
//! * 4: `--static-only` found descriptor errors.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use eustress_agent_eval::census::{run_census, run_static, CensusConfig, CensusError, RoutePreference, StaticConfig};

const USAGE: &str = "\
eustress-agent-census: invoke every agent tool twice against a live engine and classify each outcome

USAGE:
    eustress-agent-census --universe <dir> [OPTIONS]
    eustress-agent-census --static-only [--universe <dir>] [--static-out <file>]

OPTIONS:
    --universe <dir>                  The fixture Universe (scripts/agent_eval/make_census_fixture.ps1 makes one)
    --repetitions <N>                 Valid-input repetitions per tool (default 5; under 5 the run is incomplete)
    --missing-param-repetitions <N>   Missing-parameter repetitions per tool (default 1)
    --timeout-s <S>                   Per-call deadline in seconds (default 30)
    --settle-s <S>                    Longest wait for the world digest to settle (default 3)
    --engine-start-timeout-s <S>      Longest wait for the engine to answer (default 240)
    --out <file>                      Artifact path (default docs/PROMPTS/artifacts/G7.01/tool_conformance.json)
    --mcp-bin <file>                  eustress-mcp binary (default: beside this binary)
    --headless-bin <file>             Start and own eustress-headless (restarted after a crash); without it
                                      the census attaches to an engine already serving --universe
    --engine-log <file>               Attach mode: the engine's stderr file, watched for panics
    --inputs <file>                   Curated inputs (default scripts/agent_eval/census_inputs.json)
    --repo-root <dir>                 Repository root (default: from this crate's location)
    --route <mcp-first|engine-first>  Surface for names both list (default mcp-first)
    --only <a,b,...>                  Probe only these tools (the run is then incomplete by design)
    --swap-studio-last-space          Point Studio's last_space_path at the fixture for the run, restored at exit
    --static-only                     List and check descriptors; call no tool
    --static-out <file>               Static artifact path (default docs/PROMPTS/artifacts/G7.01/tool_surface_static.json)
    -h, --help                        Show this help";

fn main() {
    std::process::exit(real_main());
}

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let default_repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..");
    let mut universe: Option<PathBuf> = None;
    let mut repetitions = 5usize;
    let mut missing_reps = 1usize;
    let mut timeout_s = 30.0f64;
    let mut settle_s = 3.0f64;
    let mut start_s = 240.0f64;
    let mut out: Option<PathBuf> = None;
    let mut mcp_bin: Option<PathBuf> = None;
    let mut headless_bin: Option<PathBuf> = None;
    let mut engine_log: Option<PathBuf> = None;
    let mut inputs: Option<PathBuf> = None;
    let mut repo_root = default_repo;
    let mut route = RoutePreference::McpFirst;
    let mut only: Option<BTreeSet<String>> = None;
    let mut swap = false;
    let mut static_only = false;
    let mut static_out: Option<PathBuf> = None;

    let mut i = 0;
    macro_rules! value {
        () => {{
            i += 1;
            match args.get(i) {
                Some(v) => v.clone(),
                None => {
                    eprintln!("{} requires a value\n\n{USAGE}", args[i - 1]);
                    return 2;
                }
            }
        }};
    }
    macro_rules! number {
        ($t:ty) => {{
            let raw = value!();
            match raw.parse::<$t>() {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("{}: {e}", args[i - 1]);
                    return 2;
                }
            }
        }};
    }
    while i < args.len() {
        match args[i].as_str() {
            "--universe" => universe = Some(PathBuf::from(value!())),
            "--repetitions" => repetitions = number!(usize),
            "--missing-param-repetitions" => missing_reps = number!(usize),
            "--timeout-s" => timeout_s = number!(f64),
            "--settle-s" => settle_s = number!(f64),
            "--engine-start-timeout-s" => start_s = number!(f64),
            "--out" => out = Some(PathBuf::from(value!())),
            "--mcp-bin" => mcp_bin = Some(PathBuf::from(value!())),
            "--headless-bin" => headless_bin = Some(PathBuf::from(value!())),
            "--engine-log" => engine_log = Some(PathBuf::from(value!())),
            "--inputs" => inputs = Some(PathBuf::from(value!())),
            "--repo-root" => repo_root = PathBuf::from(value!()),
            "--route" => {
                route = match value!().as_str() {
                    "mcp-first" => RoutePreference::McpFirst,
                    "engine-first" => RoutePreference::EngineFirst,
                    other => {
                        eprintln!("--route: unknown value {other:?}\n\n{USAGE}");
                        return 2;
                    }
                }
            }
            "--only" => only = Some(value!().split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()),
            "--swap-studio-last-space" => swap = true,
            "--static-only" => static_only = true,
            "--static-out" => static_out = Some(PathBuf::from(value!())),
            "-h" | "--help" => {
                println!("{USAGE}");
                return 0;
            }
            other => {
                eprintln!("unknown argument {other:?}\n\n{USAGE}");
                return 2;
            }
        }
        i += 1;
    }
    if !(timeout_s > 0.0 && settle_s >= 0.0 && start_s > 0.0) {
        eprintln!("timeouts must be positive");
        return 2;
    }

    let repo_root = std::fs::canonicalize(&repo_root).unwrap_or(repo_root);
    let artifacts = repo_root.join("docs").join("PROMPTS").join("artifacts").join("G7.01");
    let mcp_bin = mcp_bin.unwrap_or_else(|| {
        let name = if cfg!(windows) { "eustress-mcp.exe" } else { "eustress-mcp" };
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(name)))
            .unwrap_or_else(|| PathBuf::from(name))
    });
    let timeout = Duration::from_secs_f64(timeout_s);

    if static_only {
        let cfg = StaticConfig {
            universe,
            repo_root,
            out: static_out.unwrap_or_else(|| artifacts.join("tool_surface_static.json")),
            mcp_bin,
            timeout,
        };
        return match run_static(&cfg) {
            Ok((artifact, errors)) => {
                println!(
                    "static: {} tools listed, {} errors, {} warnings -> {}",
                    artifact["tools_listed"], errors, artifact["static"]["warnings"], cfg.out.display()
                );
                if errors > 0 {
                    4
                } else {
                    0
                }
            }
            Err(e) => report_error(e),
        };
    }

    let Some(universe) = universe else {
        eprintln!("--universe is required\n\n{USAGE}");
        return 2;
    };
    let cfg = CensusConfig {
        universe,
        inputs: inputs.unwrap_or_else(|| repo_root.join("scripts").join("agent_eval").join("census_inputs.json")),
        repo_root,
        out: out.unwrap_or_else(|| artifacts.join("tool_conformance.json")),
        mcp_bin,
        headless_bin,
        engine_log,
        repetitions,
        missing_param_repetitions: missing_reps,
        timeout,
        settle: Duration::from_secs_f64(settle_s),
        engine_start_timeout: Duration::from_secs_f64(start_s),
        route,
        only,
        swap_studio_last_space: swap,
    };
    match run_census(&cfg) {
        Ok(report) => {
            let a = &report.artifact;
            println!(
                "census: defined {} (mcp {}, workshop {}), exposed {}, invoked {}, unclassified {}, totals {} -> {}",
                a["tools_defined"],
                a["tools_defined_mcp"],
                a["tools_defined_workshop"],
                a["tools_exposed_default_modes"],
                a["tools_invoked"],
                a["unclassified"],
                a["outcome_totals"],
                cfg.out.display()
            );
            if report.complete {
                0
            } else {
                for r in &report.incomplete_reasons {
                    eprintln!("incomplete: {r}");
                }
                1
            }
        }
        Err(e) => report_error(e),
    }
}

fn report_error(e: CensusError) -> i32 {
    match e {
        CensusError::Usage(m) => {
            eprintln!("usage: {m}");
            2
        }
        CensusError::Setup(m) => {
            eprintln!("setup: {m}");
            2
        }
        CensusError::Unsafe(m) => {
            eprintln!("refused: {m}");
            3
        }
    }
}
