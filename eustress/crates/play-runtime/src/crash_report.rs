//! # A panic the Player can show
//!
//! A release Player has no console (`windows_subsystem = "windows"`), so a
//! panic used to end it with nothing on screen and nothing in its log: the log
//! simply stopped mid-run. [`install`] puts a hook in front of the existing one.
//! On a panic it writes one `PANIC` line to the Player's log file and, the first
//! time only, shows one message in plain words. Then the previous hook runs, so
//! a debug build still prints its backtrace.
//!
//! The log file is the one the Player's tracing layer writes for this run; the
//! Player passes the same path function to both, so the line lands with the
//! run's other lines. Studio's engine writes its own `PANIC` line the same way
//! (engine/src/main.rs).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The line the log gets for one panic: where it happened, then what it said.
/// One line, so the log keeps its shape: control characters (a newline in the
/// message) become spaces.
pub fn panic_line(location: &str, payload: &str) -> String {
    let payload: String = payload.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    format!("PANIC at {location}: {}", payload.trim())
}

/// A panic's message as text: a `panic!` with arguments gives a `String`, one
/// without gives a `&str`. Anything else says so rather than hiding the panic.
pub fn payload_text(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("<non-text panic payload>")
}

/// Append `line` to the log at `path`, creating the file if it is missing. It
/// uses a handle of its own, not the logger's, so a panic inside the logger
/// cannot wait on the logger's own lock.
pub fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

/// The message box for a panic, as `(title, body)`: plain words, and where the
/// details are. It stays true for a panic on a worker thread too, where the
/// window may carry on.
pub fn message(log: Option<&Path>) -> (String, String) {
    let title = "Eustress Player hit an error".to_string();
    let body = match log {
        Some(log) => format!(
            "Eustress Player hit an unexpected error. The details are in its log file:\n{}\n\n\
             If the window has stopped responding, close it and start the Player again.",
            log.display()
        ),
        None => "Eustress Player hit an unexpected error, and its log file could not be opened to record \
                 the details. If the window has stopped responding, close it and start the Player again."
            .to_string(),
    };
    (title, body)
}

/// Install the panic hook. `log_path` names the log for this run (it is called
/// at each panic, so it may return `None` if the home folder is unavailable);
/// `show` displays `(title, body)` in a message box. Call once, before the app
/// runs.
pub fn install(log_path: fn() -> Option<PathBuf>, show: fn(&str, &str)) {
    // The message is shown for the first panic only: a panic that repeats must
    // not stack message boxes.
    static SHOWN: AtomicBool = AtomicBool::new(false);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown location>".to_string());
        let log = log_path();
        if let Some(log) = &log {
            let _ = append_line(log, &panic_line(&location, payload_text(info.payload())));
        }
        if !SHOWN.swap(true, Ordering::SeqCst) {
            let (title, body) = message(log.as_deref());
            show(&title, &body);
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, Once};

    /// Titles of the messages the hook showed in this test process.
    static SHOWN_TITLES: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn test_log() -> Option<PathBuf> {
        Some(std::env::temp_dir().join(format!("eustress_crash_report_{}.log", std::process::id())))
    }

    fn record_shown(title: &str, _body: &str) {
        SHOWN_TITLES.lock().unwrap_or_else(|p| p.into_inner()).push(title.to_string());
    }

    #[test]
    fn a_panic_line_is_one_line_with_where_and_what() {
        assert_eq!(panic_line("a.rs:1:2", "boom"), "PANIC at a.rs:1:2: boom");
        let line = panic_line("a.rs:1:2", "boom\nsecond line\r\n");
        assert_eq!(line, "PANIC at a.rs:1:2: boom second line");
        assert!(!line.contains('\n') && !line.contains('\r'));
    }

    #[test]
    fn a_payload_is_read_as_text_in_both_panic_forms() {
        let formatted = std::panic::catch_unwind(|| panic!("value {}", 7)).unwrap_err();
        assert_eq!(payload_text(&*formatted), "value 7");
        let literal = std::panic::catch_unwind(|| panic!("plain")).unwrap_err();
        assert_eq!(payload_text(&*literal), "plain");
        let other = std::panic::catch_unwind(|| std::panic::panic_any(42_u32)).unwrap_err();
        assert_eq!(payload_text(&*other), "<non-text panic payload>");
    }

    #[test]
    fn the_message_names_the_log_in_plain_words() {
        let (title, body) = message(Some(Path::new("C:/logs/player-1.log")));
        assert_eq!(title, "Eustress Player hit an error");
        assert!(body.contains("unexpected error") && body.contains("C:/logs/player-1.log"), "{body}");
        let (_, body) = message(None);
        assert!(body.contains("could not be opened"), "{body}");
    }

    /// A panic reaches the log file and the message box, through the hook the
    /// Player installs. Before the hook existed, a panic left the log without a
    /// line, and the window showed nothing.
    #[test]
    fn a_panic_is_written_to_the_log_and_shown_once() {
        static INSTALLED: Once = Once::new();
        INSTALLED.call_once(|| install(test_log, record_shown));
        let log = test_log().expect("a log path");
        let _ = std::fs::remove_file(&log);

        let _ = std::panic::catch_unwind(|| panic!("crash-report-test-one"));
        let _ = std::panic::catch_unwind(|| panic!("crash-report-test-two"));

        let text = std::fs::read_to_string(&log).expect("the panic wrote a log line");
        let _ = std::fs::remove_file(&log);
        assert!(text.contains("PANIC at ") && text.contains("crash-report-test-one"), "{text}");
        assert!(text.contains("crash-report-test-two"), "{text}");
        // Other panics in this test process may share the hook, so the
        // invariant is the process-wide one: at most one message ever shows.
        let shown = SHOWN_TITLES.lock().unwrap_or_else(|p| p.into_inner());
        assert!(shown.len() <= 1, "messages shown: {shown:?}");
    }
}
