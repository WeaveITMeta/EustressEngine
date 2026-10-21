//! Local Forge control plane.
//!
//! Runs forge-orchestration 0.6 (`ForgeBuilder::run`) on 127.0.0.1:8080 with
//! no Nomad client, and the Studio compatibility server on 127.0.0.1:8090.
//! A deployment starts `eustress-engine --server` on this computer. The
//! control plane records the job; it does not start the process.
//!
//! ```text
//! eustress-forge
//! ```
//!
//! `FORGE_LOCAL_API_KEY` overrides the key `local-dev`. `EUSTRESS_ENGINE_BIN`
//! names the game binary when it is not `eustress-engine` on `PATH`.

use std::sync::Arc;

use eustress_forge::studio_compat::{self, CompatState, CONTROL_ADDR};
use eustress_forge::ForgeBuilder;
use parking_lot::Mutex;

fn main() {
    let http = std::env::var("EUSTRESS_FORGE_HTTP").unwrap_or_else(|_| CONTROL_ADDR.to_string());
    let compat = std::env::var("EUSTRESS_FORGE_COMPAT").unwrap_or_else(|_| studio_compat::COMPAT_ADDR.to_string());
    if studio_compat::loopback(&http).is_err() || studio_compat::loopback(&compat).is_err() {
        eprintln!("eustress-forge binds to loopback only ({http}, {compat}).");
        std::process::exit(1);
    }

    let forge = match ForgeBuilder::new().with_http_addr(&http) {
        Ok(builder) => builder.with_metrics(false).build(),
        Err(error) => {
            eprintln!("control plane address: {error}");
            std::process::exit(1);
        }
    };
    let forge = match forge {
        Ok(forge) => forge,
        Err(error) => {
            eprintln!("control plane: {error}");
            std::process::exit(1);
        }
    };
    if forge.has_nomad() {
        eprintln!("eustress-forge does not use Nomad. Refusing to start because a Nomad client is configured.");
        std::process::exit(1);
    }

    eprintln!("Forge control plane on http://{http} (no Nomad)");
    std::thread::Builder::new()
        .name("eustress-forge-control".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("control plane runtime: {error}");
                    return;
                }
            };
            if let Err(error) = runtime.block_on(forge.run()) {
                eprintln!("control plane stopped: {error}");
            }
        })
        .expect("control plane thread");

    let state = Arc::new(Mutex::new(CompatState::from_env()));
    let stopping = state.clone();
    let _ = std::thread::Builder::new().name("eustress-forge-stop".into()).spawn(move || {
        // The compatibility thread blocks in accept. A signal asks the
        // children to leave; the process then exits.
        arm_stop();
        while !STOP.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        stopping.lock().stop_children();
        std::process::exit(0);
    });

    if let Err(error) = studio_compat::serve(&compat, state) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn arm_stop() {
    #[cfg(unix)]
    {
        unsafe extern "C" fn handle(_: i32) {
            STOP.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        unsafe extern "C" {
            fn signal(sig: i32, handler: unsafe extern "C" fn(i32)) -> usize;
        }
        unsafe {
            signal(2, handle);
            signal(15, handle);
        }
    }
}
