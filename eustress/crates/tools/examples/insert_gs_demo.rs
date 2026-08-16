//! One-shot harness that drives `InsertGaussianSplatsTool::execute` against a
//! real Universe, so the new MCP tool can be verified end-to-end WITHOUT an
//! MCP-client reconnect (the running stdio server still holds the pre-build
//! tool list). This calls the exact same code path the MCP server exposes.
//!
//! Usage:
//!   cargo run -p eustress-tools --example insert_gs_demo -- \
//!       <universe_root> <space_root> <ply_path> <name> [x y z]

use eustress_tools::{entity_tools::InsertGaussianSplatsTool, ToolContext, ToolHandler};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: insert_gs_demo <universe_root> <space_root> <ply_path> <name> [x y z]"
        );
        std::process::exit(2);
    }

    let universe_root = PathBuf::from(&args[1]);
    let space_root = PathBuf::from(&args[2]);
    let ply = args[3].clone();
    let name = args[4].clone();
    let pos = if args.len() >= 8 {
        [
            args[5].parse().unwrap_or(0.0),
            args[6].parse().unwrap_or(2.0),
            args[7].parse().unwrap_or(0.0),
        ]
    } else {
        [0.0, 2.0, 0.0]
    };

    let ctx = ToolContext {
        space_root,
        universe_root,
        user_id: None,
        username: Some("mcp-verify".to_string()),
        luau_executor: None,
        display_unit: None,
        cancelled: None,
        // Local demo binary: insert_gaussian_splats is a Write, so the
        // standard set covers it.
        permissions: eustress_tools::Permissions::standard().for_principal("insert-gs-demo"),
    };

    let input = serde_json::json!({
        "path": ply,
        "name": name,
        "position": pos,
        "cull_floaters": true,
        "ppisp": true,
    });

    let result = InsertGaussianSplatsTool.execute(input, &ctx);
    println!("success = {}", result.success);
    println!("content = {}", result.content);
    if let Some(d) = result.structured_data {
        println!(
            "structured =\n{}",
            serde_json::to_string_pretty(&d).unwrap_or_default()
        );
    }
    std::process::exit(if result.success { 0 } else { 1 });
}
