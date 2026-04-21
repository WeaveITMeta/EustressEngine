//! # eustress-lsp — LSP server for the Rune scripting language
//!
//! Two transports, same protocol shell:
//!
//! * **stdio** (default) — `eustress-lsp` reads JSON-RPC on stdin and
//!   writes to stdout. Matches how every IDE spawns a language server.
//! * **TCP** — `eustress-lsp --tcp [--port-file <path>]` binds on a
//!   loopback port (OS-assigned by default), writes the port to the
//!   given file, and serves every incoming connection. Used when the
//!   Eustress engine launches the LSP as a child process so external
//!   IDEs can connect without spawning their own.
//!
//! All language intelligence lives in
//! `eustress_engine::script_editor::analyzer`. This file is only the
//! transport shell.
//!
//! ## Running
//!
//! ```bash
//! cargo build --bin eustress-lsp --features lsp --release
//!
//! # Stdio — spawned by an IDE extension.
//! eustress-lsp
//!
//! # TCP — launched by the engine. Port written to the file.
//! eustress-lsp --tcp --port-file {universe}/.eustress/lsp.port
//! ```
#![cfg(feature = "lsp")]

use eustress_engine::script_editor::lsp::EustressLsp;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tower_lsp::{LspService, Server};

/// CLI flags parsed out of `env::args()`. Kept deliberately simple —
/// `clap` would be overkill for three optional flags that are only ever
/// set by the engine launcher.
struct Args {
    tcp: bool,
    /// Explicit port for `--tcp`. `None` = OS-assigned (port 0).
    port: Option<u16>,
    /// Where to write the bound port after `--tcp` + OS assignment.
    port_file: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut a = Args { tcp: false, port: None, port_file: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--tcp" => a.tcp = true,
            "--port" => {
                a.port = it.next().and_then(|s| s.parse().ok());
            }
            "--port-file" => {
                a.port_file = it.next().map(PathBuf::from);
            }
            "-h" | "--help" | "--version" => {
                println!("eustress-lsp {}", env!("CARGO_PKG_VERSION"));
                println!("Usage: eustress-lsp [--tcp [--port <n>] [--port-file <path>]]");
                std::process::exit(0);
            }
            _ => {
                eprintln!("[eustress-lsp] ignoring unknown arg: {}", arg);
            }
        }
    }
    a
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    if args.tcp {
        run_tcp(args).await;
    } else {
        run_stdio().await;
    }
}

async fn run_stdio() {
    let stdin  = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(EustressLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

/// Serve LSP over TCP. We loop on `accept()` so multiple IDEs can connect
/// to the same engine-launched server simultaneously — each connection
/// gets its own `LspService` instance, its own document buffer map, its
/// own subscription set.
async fn run_tcp(args: Args) {
    let bind_addr = format!("127.0.0.1:{}", args.port.unwrap_or(0));
    let listener = match TcpListener::bind(&bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[eustress-lsp] bind {} failed: {}", bind_addr, e);
            std::process::exit(1);
        }
    };

    let actual_port = listener.local_addr()
        .map(|a| a.port())
        .unwrap_or(0);
    eprintln!("[eustress-lsp] TCP listening on 127.0.0.1:{}", actual_port);

    // Advertise the port to whoever spawned us (the engine) via both
    // stdout (one-shot line, machine-parseable) and the optional
    // port-file (file-system sentinel, readable by IDE extensions).
    println!("port={}", actual_port);
    if let Some(path) = &args.port_file {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(path, actual_port.to_string()) {
            eprintln!("[eustress-lsp] failed to write port file {}: {}", path.display(), e);
        }
    }

    // Cleanup the port file on Ctrl-C / SIGTERM. The engine launcher
    // kills us on its own shutdown, so this mostly matters for standalone
    // operators.
    let port_file = args.port_file.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        if let Some(p) = port_file {
            let _ = std::fs::remove_file(&p);
        }
        std::process::exit(0);
    });

    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                eprintln!("[eustress-lsp] accept failed: {}", e);
                continue;
            }
        };
        eprintln!("[eustress-lsp] connection from {}", peer);
        tokio::spawn(async move {
            let (read, write) = stream.into_split();
            let (service, socket) = LspService::new(EustressLsp::new);
            Server::new(read, write, socket).serve(service).await;
            eprintln!("[eustress-lsp] connection from {} closed", peer);
        });
    }
}
