//! Files and permissions: the startup check on the TLS private key,
//! `parties.toml` and its folder, the reload check, and the identity file
//! `party token` reads; the startup log of the certificate's SPKI pin;
//! `party revoke` while a reload holds `parties.toml` open (Windows); and the
//! ratchet that keeps every key file read through the handle its owner-only
//! check returned.
//!
//! "Another account" is BUILTIN\Users on Windows (an access entry added with
//! icacls) and group or other permission bits on Unix.

mod common;

use eustress_identity::private_file;
use eustress_stream_node::auth::{ReloadOutcome, TokenClass};
use eustress_stream_node::client::{self, TokenRequest};
use eustress_stream_node::protocol::ServerFrame;
use eustress_stream_node::{NodeError, SpkiPin, StreamNode};

use common::{peer, DuplexNode, Fixture, RawSession, TestParty};

/// Start a node on a fixed free port; return the refusal and the port.
async fn refused_startup(fixture: &Fixture) -> (NodeError, u16) {
    let port = common::free_port();
    let mut config = fixture.config().with_port(port);
    config.auto_increment = false;
    let error = StreamNode::start(config).await.err().expect("startup must be refused");
    (error, port)
}

async fn assert_nothing_listens(port: u16) {
    tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("no listener may hold the node's port after a refused startup");
}

/// A `parties.toml` with an access entry for another account fails the
/// startup check, and no listener starts.
#[tokio::test]
async fn parties_file_with_an_extra_access_entry_fails_startup_and_no_listener_starts() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    common::grant_foreign_access(&fixture.parties_path);

    let (error, port) = refused_startup(&fixture).await;
    let text = error.to_string();
    assert!(matches!(error, NodeError::Startup(_)), "{text}");
    assert!(text.contains("parties file failed the owner-only check"), "{text}");
    assert_nothing_listens(port).await;
}

/// The folder that holds `parties.toml` is checked too.
#[tokio::test]
async fn parties_folder_with_an_extra_access_entry_fails_startup() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    common::grant_foreign_access(&fixture.private_dir);

    let (error, port) = refused_startup(&fixture).await;
    let text = error.to_string();
    assert!(text.contains("folder that holds the parties file"), "{text}");
    assert_nothing_listens(port).await;
}

/// A private key file with an access entry for another account fails the
/// startup check.
#[tokio::test]
async fn private_key_with_an_extra_access_entry_fails_startup() {
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);
    common::grant_foreign_access(&fixture.key_path);

    let (error, port) = refused_startup(&fixture).await;
    let text = error.to_string();
    assert!(text.contains("TLS private key failed the owner-only check"), "{text}");
    assert_nothing_listens(port).await;

    // Control: the same fixture with an owner-only key starts.
    private_file::write_private(&fixture.key_path, fixture.cert.key_pem.as_bytes()).unwrap();
    let node = StreamNode::start(fixture.config()).await.expect("an owner-only key starts");
    node.shutdown();
}

/// A `parties.toml` that becomes unprotected after startup keeps the last good
/// set at reload, even when its new contents would enroll another party.
#[tokio::test(start_paused = true)]
async fn parties_file_that_becomes_unprotected_after_startup_keeps_the_last_good_set() {
    let fixture = Fixture::new();
    let plant = TestParty::new("north-plant").publish(&["log/output"]);
    fixture.write_parties(&[&plant]);
    let node = DuplexNode::start(&fixture);

    // An edit another account could have made: a new admin party, in a file
    // that is no longer owner-only.
    let intruder = TestParty::new("intruder").publish(&["scene_deltas"]).admin();
    fixture.write_parties(&[&plant, &intruder]);
    common::grant_foreign_access(&fixture.parties_path);

    let report = node.auth.revocation_tick().await;
    assert!(matches!(report.reload, ReloadOutcome::KeptLastGood(_)), "{:?}", report.reload);
    assert!(node.auth.party_set().get("intruder").is_none(), "the unprotected file's new party must not be enrolled");

    let mut kept = RawSession::open(node.connect(peer(1)), fixture.cert.pin).await.expect("TLS");
    assert!(matches!(kept.login(&plant).await.unwrap(), ServerFrame::Welcome { .. }), "the last good set still logs in");
    let mut refused = RawSession::open(node.connect(peer(2)), fixture.cert.pin).await.expect("TLS");
    assert!(matches!(refused.login(&intruder).await.unwrap(), ServerFrame::AuthFailed { .. }));

    // Control: the same contents written owner-only are taken.
    fixture.write_parties(&[&plant, &intruder]);
    let report = node.auth.revocation_tick().await;
    assert_eq!(report.reload, ReloadOutcome::Replaced);
    assert!(node.auth.party_set().get("intruder").is_some());
}

/// `party token` refuses to read an identity file that fails the permission
/// check. The file is not TOML, so had it been read the error would be a
/// parse error instead.
#[tokio::test]
async fn party_token_refuses_to_read_an_identity_file_that_fails_the_permission_check() {
    let dir = tempfile::tempdir().expect("temp dir");
    let unprotected = dir.path().join("eustress-north-plant.toml");
    std::fs::write(&unprotected, "this is not an identity file [").expect("write");
    common::grant_foreign_access(&unprotected);

    let request = TokenRequest {
        party_id: "north-plant".to_string(),
        // Never reached: the identity file is refused first.
        node: "127.0.0.1:9".to_string(),
        pin: SpkiPin([0; 32]),
        identity: unprotected,
        class: TokenClass::Browser,
    };
    match client::request_token(&request).await {
        Err(NodeError::Identity(message)) => {
            assert!(message.contains("refusing to read the identity file"), "{message}")
        }
        other => panic!("expected the permission refusal, got {other:?}"),
    }

    // Control: the same bytes in an owner-only file are read, and fail as text.
    let private = dir.path().join("private");
    private_file::private_dir(&private).expect("owner-only folder");
    let protected = private.join("eustress-north-plant.toml");
    private_file::write_private(&protected, b"this is not an identity file [").expect("write owner-only");
    match client::request_token(&TokenRequest { identity: protected, ..request }).await {
        Err(NodeError::Identity(message)) => assert!(message.contains("does not parse"), "{message}"),
        other => panic!("expected a parse error from the owner-only file, got {other:?}"),
    }
}

/// This thread's log output, from INFO up. A `#[tokio::test]` runs on a
/// current-thread runtime, so the node's startup logs on this thread.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Captured {
    /// Send this thread's log lines here until the guard drops.
    fn install(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .finish();
        tracing::subscriber::set_default(subscriber)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// When the node starts its listeners, the log names the loaded
/// certificate's SPKI pin at info: the hash clients pin, the one
/// `StreamNode::spki_pin` returns. The log holds the pin only, never the key.
#[tokio::test]
async fn startup_logs_the_loaded_certificates_spki_pin_at_info() {
    let logs = Captured::default();
    let _logging = logs.install();
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant")]);

    let node = StreamNode::start(fixture.config()).await.expect("the node starts");
    assert_eq!(node.spki_pin(), fixture.cert.pin, "the node's pin is the one clients hold");
    let pin = node.spki_pin().to_string();
    node.shutdown();

    let log = logs.text();
    let pin_lines: Vec<&str> = log.lines().filter(|line| line.contains("Clients pin SPKI SHA-256")).collect();
    assert!(
        pin_lines.iter().any(|line| line.contains("INFO") && line.contains(&pin)),
        "an info line at listener start names the pin {pin}: {pin_lines:?}"
    );
    assert!(!log.contains("PRIVATE KEY"), "the key never reaches the log");
    for key_line in fixture.cert.key_pem.lines().filter(|line| !line.is_empty() && !line.starts_with("-----")) {
        assert!(!log.contains(key_line), "the key never reaches the log");
    }
}

/// `party revoke` succeeds while a reload holds `parties.toml` open, and the
/// reload's handle still reads the file it checked. The test holds the file
/// open as a reload does, through `verify_open`, and runs the real
/// `party revoke`. Windows only: elsewhere an open file never blocks a rename
/// over it.
// `verify_open` opens the file with read and delete sharing and without write
// sharing (FILE_SHARE_READ | FILE_SHARE_DELETE), so `party revoke` can rename
// over the file while a reload holds it.
#[cfg(windows)]
#[test]
fn party_revoke_succeeds_while_a_reload_holds_the_parties_file_open() {
    use std::io::Read as _;
    let fixture = Fixture::new();
    fixture.write_parties(&[&TestParty::new("north-plant").publish(&["log/output"])]);
    let parse = |text: &str| eustress_stream_node::auth::PartiesFile::parse(text).expect("parties.toml parses");

    // The handle a reload holds while it reads: `read_parties` opens the file
    // with `verify_open`.
    let mut held = private_file::verify_open(&fixture.parties_path).expect("the parties file passes the check");

    let revoke = std::process::Command::new(env!("CARGO_BIN_EXE_eustress-stream-node"))
        .args(["party", "revoke", "north-plant", "--parties"])
        .arg(&fixture.parties_path)
        .output()
        .expect("run party revoke");
    assert!(
        revoke.status.success(),
        "party revoke failed while a reload held the file open: {}",
        String::from_utf8_lossy(&revoke.stderr)
    );

    // The held handle still reads the file it checked: the party enabled.
    let mut checked = String::new();
    held.read_to_string(&mut checked).expect("read through the held handle");
    drop(held);
    assert!(parse(&checked).parties.iter().any(|p| p.id == "north-plant" && p.enabled), "{checked}");

    // A new read, as the next reload makes, sees the party revoked.
    let mut now = String::new();
    private_file::verify_open(&fixture.parties_path)
        .expect("the rewritten file passes the check")
        .read_to_string(&mut now)
        .expect("read the rewritten file");
    assert!(parse(&now).parties.iter().any(|p| p.id == "north-plant" && !p.enabled), "{now}");
}

// ─────────────────────────────────────────────────────────────────────────────
// Ratchet: key material is read through the handle its check returned
// ─────────────────────────────────────────────────────────────────────────────

/// The loaders: the source files that read a key file, `parties.toml` or an
/// identity file, or build the TLS configuration from the key.
const LOADERS: [&str; 4] = ["auth.rs", "client.rs", "transport.rs", "bin/eustress-stream-node.rs"];

/// Calls that read a file by its path.
const PATH_READS: [&str; 5] = ["fs::read(", "fs::read_to_string(", "File::open(", "OpenOptions::new(", "fs::copy("];

/// A source file's shipped code, line by line so the numbers match the file:
/// `//` comments removed, and nothing from its unit-test module on.
fn shipped_code(text: &str) -> Vec<String> {
    let mut code = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]" && lines.peek().is_some_and(|next| next.trim() == "mod tests {") {
            break;
        }
        code.push(match line.find("//") {
            Some(at) => line[..at].to_string(),
            None => line.to_string(),
        });
    }
    code
}

/// Key material is read only through the handle `verify_open` returned. No
/// loader reads a file by path except the certificate, which is public; each
/// key file's loader opens the file with `verify_open` and reads through that
/// handle; startup's helper is `private_file` itself; and every source file
/// that calls the helper is a loader, so a new loader cannot slip past.
#[test]
fn loaders_read_key_material_only_through_the_handle_verify_open_returned() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let shipped = |file: &str| shipped_code(&std::fs::read_to_string(src.join(file)).expect("read a source file"));

    // No path read in a loader but the certificate's.
    for loader in LOADERS {
        for (i, line) in shipped(loader).iter().enumerate() {
            if PATH_READS.iter().any(|read| line.contains(read)) {
                assert!(
                    line.contains("std::fs::read(cert_path)"),
                    "{loader}:{}: a path read in a loader; key material is read only through the handle verify_open returned: {}",
                    i + 1,
                    line.trim()
                );
            }
        }
    }

    // Each key file is opened with verify_open and read through its handle.
    for (loader, function, opened) in [
        ("auth.rs", "fn read_private(", "(owner_only.verify_open)(path)"),
        ("auth.rs", "fn read_parties(", "(owner_only.verify_open)(path)"),
        ("client.rs", "pub fn read_identity_keypair(", "private_file::verify_open(path)"),
        ("bin/eustress-stream-node.rs", "fn read_parties_for_edit(", "private_file::verify_open(path)"),
    ] {
        let code = shipped(loader).join("\n");
        let body = code
            .split(function)
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap_or_else(|| panic!("{loader}: `{function}` not found"));
        assert!(body.contains(opened), "{loader}: `{function}` opens its file with `{opened}`");
        assert!(
            body.contains("checked") && (body.contains(".read_to_end(&mut bytes)") || body.contains(".read_to_string(&mut text)")),
            "{loader}: `{function}` reads through the handle its check returned"
        );
    }

    // Startup's helper is private_file itself.
    let auth = shipped("auth.rs").join("\n");
    assert!(auth.contains("verify: eustress_identity::private_file::verify,"), "the folder check is the helper's verify");
    assert!(auth.contains("verify_open: eustress_identity::private_file::verify_open,"), "the file check is the helper's verify_open");

    // Every source file that calls the helper is a loader.
    let mut folders = vec![src.clone()];
    let mut callers = Vec::new();
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("read the source folder") {
            let path = entry.expect("a folder entry").path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let file = path.strip_prefix(&src).expect("under src").to_string_lossy().replace('\\', "/");
                if shipped(&file).iter().any(|line| line.contains("private_file")) {
                    callers.push(file);
                }
            }
        }
    }
    callers.sort();
    for caller in &callers {
        assert!(LOADERS.contains(&caller.as_str()), "{caller} calls the owner-only helper, so the ratchet must scan it as a loader");
    }
    assert!(callers.len() >= 3, "the ratchet must see the helper's callers: {callers:?}");
}
