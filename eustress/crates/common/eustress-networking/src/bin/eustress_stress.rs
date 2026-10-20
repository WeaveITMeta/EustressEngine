//! `eustress-stress` — headless clients against a live host.
//!
//! ```text
//! eustress-stress --connect 127.0.0.1:7777 --clients 8 --duration 30 --ramp 5
//! eustress-stress --connect "eustress-player://join/203.0.113.9:4433?key=…&pin=…"
//! ```
//!
//! A loopback address with no key or pin reads
//! `<workspace>/.eustress/hosts/<port>.link` when that file is fresh. That is
//! the file Studio and `eustress-headless` (`EUSTRESS_HOST_ON_PLAY=1`) write.
//! A remote host needs the full link: the key and the certificate pin.
//!
//! Exit 0 when every client joined and none was kicked or dropped. Exit 2
//! when the arguments are wrong.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use eustress_networking::stress::{resolve_link, run_blocking, StressConfig};

fn main() -> ExitCode {
    let mut connect = None;
    let mut clients = 4u32;
    let mut duration = 30u64;
    let mut ramp_ms = 5_000u64;
    let mut avatar_hz = 20u32;
    let mut movement = true;
    let mut interactions = false;
    let mut download = true;
    let mut workspaces: Vec<PathBuf> = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = |args: &mut std::iter::Skip<env::Args>, name: &str| -> Result<String, String> {
            args.next().ok_or_else(|| format!("{name} needs a value"))
        };
        let parsed = match arg.as_str() {
            "--connect" | "-c" => value(&mut args, "--connect").map(|v| connect = Some(v)),
            "--clients" | "-n" => value(&mut args, "--clients").and_then(|v| {
                clients = v
                    .parse()
                    .map_err(|_| format!("--clients: {v} is not a number"))?;
                Ok(())
            }),
            "--duration" => value(&mut args, "--duration").and_then(|v| {
                duration = v
                    .parse()
                    .map_err(|_| format!("--duration: {v} is not a number"))?;
                Ok(())
            }),
            "--ramp" => value(&mut args, "--ramp").and_then(|v| {
                let secs: f64 = v
                    .parse()
                    .map_err(|_| format!("--ramp: {v} is not a number"))?;
                ramp_ms = (secs * 1000.0).max(0.0) as u64;
                Ok(())
            }),
            "--avatar-hz" => value(&mut args, "--avatar-hz").and_then(|v| {
                avatar_hz = v
                    .parse()
                    .map_err(|_| format!("--avatar-hz: {v} is not a number"))?;
                Ok(())
            }),
            "--workspace" => {
                value(&mut args, "--workspace").map(|v| workspaces.push(PathBuf::from(v)))
            }
            "--no-move" => Ok(movement = false),
            "--interact" => Ok(interactions = true),
            "--no-download" => Ok(download = false),
            "--help" | "-h" => {
                print_help();
                return ExitCode::SUCCESS;
            }
            other => Err(format!("unknown argument {other}")),
        };
        if let Err(reason) = parsed {
            eprintln!("{reason}\n");
            print_help();
            return ExitCode::from(2);
        }
    }
    let Some(connect) = connect else {
        eprintln!("pass --connect with a join link or host:port\n");
        print_help();
        return ExitCode::from(2);
    };
    if let Ok(cwd) = env::current_dir() {
        workspaces.push(cwd);
    }
    if let Some(from_env) = env::var_os("EUSTRESS_WORKSPACE") {
        workspaces.push(PathBuf::from(from_env));
    }
    let link = match resolve_link(&connect, &workspaces) {
        Ok(link) => link,
        Err(reason) => {
            eprintln!("{reason}");
            return ExitCode::from(2);
        }
    };
    let config = StressConfig {
        clients,
        duration: Duration::from_secs(duration),
        ramp: Duration::from_millis(ramp_ms),
        avatar_hz,
        movement,
        interactions,
        download_world: download,
    };
    println!(
        "stress: {clients} clients for {duration}s, ramp {ramp_ms} ms, target {}",
        link_label(&link)
    );
    match run_blocking(link, config) {
        Ok(snap) => {
            println!("{}", snap.report);
            if snap.failed == 0
                && snap.kicks == 0
                && snap.disconnects == 0
                && snap.joined == clients
            {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(reason) => {
            eprintln!("{reason}");
            ExitCode::from(2)
        }
    }
}

fn link_label(link: &eustress_networking::JoinLink) -> String {
    format!("{}:{}", link.host, link.port)
}

fn print_help() {
    eprintln!(
        "\
eustress-stress — headless clients on the real session protocol

  eustress-stress --connect <link-or-host:port> [options]

Options:
  --clients N        bots to join (default 4)
  --duration SEC     how long the run lasts, from the start (default 30)
  --ramp SEC         spread the joins across this many seconds (default 5)
  --avatar-hz N      avatar samples each bot sends per second (default 20)
  --workspace DIR    where to look for .eustress/hosts/<port>.link
  --no-move          join and stay still; no avatar or input frames
  --interact         also chat and jump
  --no-download      skip the world download and send WorldReady after Welcome

A loopback host:port reads a fresh host file when one is there, so a local
dedicated server (eustress-headless with EUSTRESS_HOST_ON_PLAY=1, or Studio
F9) needs no copied link. A remote host, including a Forge allocation, needs
the full join link with its key and pin. eustress-server does not host this
protocol."
    );
}
