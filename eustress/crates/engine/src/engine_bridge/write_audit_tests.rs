//! Every file the engine bridge writes or removes, source by source, and why
//! each path is safe (an audit of 2026-10-01). A bridge method's caller holds
//! the bridge token (the MCP server, the CLI), and a method's parameters are
//! not tool inputs, so they never pass `eustress_tools::sandbox` on their own:
//! every path here is fixed by the engine or built from a checked name. A
//! write call or process launch added anywhere in this folder fails
//! [`every_bridge_write_is_accounted_for`] until it is reviewed and
//! [`WRITES`] is updated.
//!
//! What the count does NOT do, so a reviewer knows: it counts calls per
//! file, so a change to the PATH an existing call writes passes unnoticed,
//! and any diff in a listed file still needs a person to read it; it sees a
//! call only by its spellings in [`WRITE_CALLS`] (imports that rename file
//! or process calls are refused); and it finds a test module's end by
//! counting braces, so a test module must come last in its file.
//!
//! What a method writes INTO a file is not checked here. In particular
//! `properties.set` stores asset references (SoundId, MeshId, TextureId,
//! Texture) in an instance's TOML as given: untrusted text, like anything in
//! a Space from someone else. Nothing here makes such a value safe; the
//! readers contain it (`eustress_common::space_path`, the space:// reader
//! and the play-runtime builders), and must.
//!
//! The same count as `eustress_tools`' `every_file_write_is_accounted_for`,
//! kept here so neither crate's tests depend on the other's.

/// Each source in `engine_bridge/` that writes, with its count and why.
/// A source with no write call need not be listed.
const WRITES: &[(&str, usize, &str)] = &[
    ("instance_file.rs", 7, "this engine's record and folder under <workspace>/.eustress/instances/, named by its own process id"),
    ("port_file.rs", 7, "engine.port in the Universe's and the workspace's .eustress, removed only while it still names this engine"),
    ("protocol.rs", 6, "hil.ingest's <Space>/.eustress/hil/<run_id>.ndjson (run_id: ASCII letters, digits, - and _, and a plain name); the fixed .eustress/capture.png and .eustress/ai_camera.png"),
    ("server.rs", 2, "the unix socket at the path unix_socket_file chooses"),
    ("terrain.rs", 8, "Workspace/Terrain, cleared through a staging folder beside it; a new layer's folder, named by create_instance (sanitize_entity_name)"),
    ("token_file.rs", 2, "this launch's ~/.eustress_engine/bridge/<port>.token, written privately and removed only while it holds this launch's token"),
    ("unix_socket_file.rs", 7, "the engine.sock pointer files and the socket's own folder under the system temp folder"),
];

const WRITE_CALLS: &[&str] = &[
    "fs::write(", "File::create(", "OpenOptions::new(", "tokio::fs::", "fs::rename(", "fs::remove_file(",
    "fs::remove_dir(", "fs::remove_dir_all(", "fs::copy(", "fs::create_dir(", "fs::create_dir_all(",
    "save_toml_file(", "create_instance(", "write_glb(", "append_json_line(", "write_private(",
    "File::options(", "Command::new(", "git_command(",
    "fs::set_permissions(", "fs::hard_link(", "symlink_file(", "symlink_dir(", "create_new(",
];

/// `text` without its `#[cfg(test)]` items. Braces are counted without
/// reading strings or comments, so a block item must be followed only by
/// other `#[cfg(test)]` items: a lone brace in a test's string could
/// otherwise swallow the code after it.
fn without_tests(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("#[cfg(test)]") {
            let (mut depth, mut opened) = (0i32, false);
            while i < lines.len() {
                for c in lines[i].chars() {
                    match c {
                        '{' => {
                            depth += 1;
                            opened = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                i += 1;
                if opened && depth <= 0 {
                    let next = lines[i..].iter().map(|l| l.trim()).find(|l| !l.is_empty() && !l.starts_with("//"));
                    assert!(
                        next.map_or(true, |l| l.starts_with("#[cfg(test)]")),
                        "a #[cfg(test)] block is followed by other code ({}): move the tests to the end of the file, \
                         so the write count knows where they stop",
                        next.unwrap_or("")
                    );
                    break;
                }
                if !opened && lines[i - 1].trim_end().ends_with(';') {
                    break;
                }
            }
            continue;
        }
        out.push(lines[i]);
        i += 1;
    }
    out.join("\n")
}

fn write_calls(text: &str) -> usize {
    let body = without_tests(&text.replace("\r\n", "\n"));
    WRITE_CALLS.iter().map(|p| body.matches(p).count()).sum()
}

/// Each `use` statement in `text`, its lines joined, so an import spread over
/// several lines is read whole.
fn use_statements(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if current.is_none() {
            let l = l.strip_prefix("pub ").unwrap_or(l);
            let l = l.strip_prefix("pub(crate) ").unwrap_or(l);
            if l.starts_with("use ") {
                current = Some(String::new());
            }
        }
        if let Some(statement) = current.as_mut() {
            statement.push_str(l);
            statement.push(' ');
            if l.contains(';') {
                out.push(current.take().unwrap());
            }
        }
    }
    out
}

#[test]
fn every_bridge_write_is_accounted_for() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("engine_bridge");
    let mut seen = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") || name == "write_audit_tests.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let count = write_calls(&text);
        let listed = WRITES.iter().find(|(file, _, _)| *file == name.as_str()).map_or(0, |(_, n, _)| *n);
        assert_eq!(
            count, listed,
            "engine_bridge/{name} makes {count} write call(s) or process launch(es) outside its tests and WRITES lists \
             {listed}: review each new one (a path fixed by the engine, or built from a name that passed \
             eustress_tools::sandbox::plain_name or resolve), then update WRITES"
        );
        for statement in use_statements(&text) {
            let fs = statement.contains("std::fs")
                || statement.contains("tokio::fs")
                || (statement.contains("std::{") && statement.contains("fs"))
                || (statement.contains("tokio::{") && statement.contains("fs"));
            assert!(
                !fs && !(statement.contains("process") && statement.contains(" as ")),
                "engine_bridge/{name}: `{statement}`: spell file calls out in full"
            );
        }
        seen.push(name);
    }
    for (file, _, _) in WRITES {
        assert!(seen.iter().any(|s| s.as_str() == *file), "{file} is listed in WRITES but is not in engine_bridge/");
    }
}

#[test]
fn the_write_count_skips_tests_and_counts_helpers() {
    let text = "fn a() { std::fs::write(p, b); crate::private_file::write_private(&p, b); }\nfn b() { std::fs::remove_file(d); }\n#[cfg(test)]\nmod tests {\n    fn t() { std::fs::write(q, b); }\n}\n";
    assert_eq!(write_calls(text), 3);
}

#[test]
#[should_panic(expected = "followed by other code")]
fn a_test_block_before_other_code_is_refused() {
    write_calls("#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn after() { std::fs::write(p, b); }\n");
}
