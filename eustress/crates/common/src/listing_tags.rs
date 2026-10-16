//! A gallery listing's hashtags (`docs/networking/LISTING_TAGS.md`): the one
//! rule Studio's Publish dialog and the MCP publish tool share with the
//! Worker (`src/tags.mjs`), so a chip shows exactly what will be stored.
//!
//! A tag is normalized in this order: Unicode NFKC, trim, lower-case, one
//! leading `#` removed. What is left must be 2 to 24 characters of letters a
//! to z and digits, with single hyphens between them. A tag is never personal
//! information: a phone number, an email address or a handle
//! ([`personal_info_in`]). A listing holds at most [`MAX_TAGS`], counted after
//! duplicates are dropped; order is kept. A bad tag is never repaired or
//! dropped: it refuses the whole list.

use unicode_normalization::UnicodeNormalization;

/// The most tags a listing holds.
pub const MAX_TAGS: usize = 8;
/// The shortest a tag is, without its `#`.
pub const TAG_MIN: usize = 2;
/// The longest a tag is, without its `#`.
pub const TAG_MAX: usize = 24;

/// The rule, in the words a refusal ends with.
pub const TAG_RULE: &str = "A tag is 2 to 24 characters: lowercase letters a to z and digits, with single hyphens \
     between them. A tag is never a phone number, an email address or a handle.";

/// Digits in a row that make a phone number (the Worker's `PHONE_DIGITS`).
const PHONE_DIGITS: usize = 7;
/// The endings a spelled-out email address has, as `ann-at-mail-com` spells
/// it (the Worker's `SPELLED_EMAIL`).
const SPELLED_EMAIL_ENDINGS: &[&str] =
    &["com", "net", "org", "edu", "gov", "io", "co", "me", "us", "uk", "de", "fr", "ca", "au", "info", "biz"];

/// The personal information `raw` carries, if any, by its kind: "a phone
/// number", "an email address" or "a handle". It is the Worker's own check
/// (`personalInfoIn`), and narrow on purpose, not a word list:
/// - a phone number is 7 or more digits in a row, where single hyphens between
///   digit groups do not break the row (`555-1234`), but two groups of 4 digits
///   (a year range, `1939-1945`) are not;
/// - an email address is an `@` with text on both sides and a dot after it, or
///   one spelled with words (`ann-at-mail-dot-com`);
/// - a handle starts with `@`.
///
/// The email and handle checks read the text as typed, since the tag shape
/// refuses `@` and `.` anyway; the rest read the tag it normalizes to.
pub fn personal_info_in(raw: &str) -> Option<&'static str> {
    let text: String = raw.nfkc().collect();
    let text = text.trim();
    if email_shaped(text) {
        return Some("an email address");
    }
    if handle_shaped(text.strip_prefix('#').unwrap_or(text)) {
        return Some("a handle");
    }
    let lowered = text.to_lowercase();
    let tag = lowered.strip_prefix('#').unwrap_or(&lowered);
    if !tag_shaped(tag) {
        return None;
    }
    if spelled_email(tag) {
        return Some("an email address");
    }
    phone_shaped(tag).then_some("a phone number")
}

/// Letters a to z and digits, one hyphen at a time between them.
fn tag_shaped(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

/// Text, `@`, text, a dot, text: `[^\s@]@[^\s@]+\.[^\s@]` somewhere in it.
fn email_shaped(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let word = |c: char| !c.is_whitespace() && c != '@';
    (1..chars.len()).filter(|&at| chars[at] == '@' && word(chars[at - 1])).any(|at| {
        (at + 2..chars.len().saturating_sub(1))
            .take_while(|&k| word(chars[k - 1]))
            .any(|k| chars[k] == '.' && word(chars[k + 1]))
    })
}

/// `@` and then a character that is neither a space nor another `@`.
fn handle_shaped(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next() == Some('@') && chars.next().is_some_and(|c| !c.is_whitespace() && c != '@')
}

/// `at`, at least one word, then a known ending: `ann-at-mail-com`.
fn spelled_email(tag: &str) -> bool {
    let parts: Vec<&str> = tag.split('-').collect();
    let Some(last) = parts.last() else { return false };
    SPELLED_EMAIL_ENDINGS.contains(last) && parts.len() >= 3 && parts[..parts.len() - 2].contains(&"at")
}

/// 7 or more digits in a row, single hyphens between digit groups included,
/// unless the row is two groups of 4 (a year range).
fn phone_shaped(tag: &str) -> bool {
    let mut run: Vec<&str> = Vec::new();
    let ends = |run: &mut Vec<&str>| {
        let digits: usize = run.iter().map(|g| g.len()).sum();
        let year_range = run.len() == 2 && run.iter().all(|g| g.len() == 4);
        run.clear();
        digits >= PHONE_DIGITS && !year_range
    };
    let longest_digits = |part: &str| {
        part.split(|c: char| !c.is_ascii_digit()).map(str::len).max().unwrap_or(0)
    };
    for part in tag.split('-') {
        if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) {
            run.push(part);
            continue;
        }
        if ends(&mut run) || longest_digits(part) >= PHONE_DIGITS {
            return true;
        }
    }
    ends(&mut run)
}

/// A tag as the Worker stores it, or why it cannot be one. Personal
/// information is refused by its kind, and the refusal never repeats it.
pub fn normalize_tag(raw: &str) -> Result<String, String> {
    if let Some(kind) = personal_info_in(raw) {
        return Err(format!("That looks like {kind}, and a tag never carries personal information. {TAG_RULE}"));
    }
    let folded: String = raw.nfkc().collect();
    let lowered = folded.trim().to_lowercase();
    let tag = lowered.strip_prefix('#').unwrap_or(&lowered);
    let shown = if tag.is_empty() { raw.trim().to_string() } else { tag.to_string() };
    if !tag_shaped(tag) {
        return Err(format!("\"{shown}\" can't be a tag. {TAG_RULE}"));
    }
    let len = tag.chars().count();
    if len < TAG_MIN {
        return Err(format!("\"{shown}\" is too short: a tag is at least {TAG_MIN} characters."));
    }
    if len > TAG_MAX {
        return Err(format!("\"{shown}\" is too long: a tag is at most {TAG_MAX} characters."));
    }
    Ok(tag.to_string())
}

/// A whole list as the Worker stores it: each tag normalized, duplicates
/// dropped in order, at most [`MAX_TAGS`]. The first bad tag refuses it all.
pub fn check_tags<S: AsRef<str>>(tags: &[S]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        let tag = normalize_tag(raw.as_ref())?;
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    if out.len() > MAX_TAGS {
        return Err(format!("A listing holds at most {MAX_TAGS} tags, and this names {}.", out.len()));
    }
    Ok(out)
}

/// Add one tag to a list as an author types it: normalized, a duplicate
/// ignored, refused past [`MAX_TAGS`]. The list is left as it was on a
/// refusal.
pub fn add_tag(tags: &mut Vec<String>, raw: &str) -> Result<(), String> {
    let tag = normalize_tag(raw)?;
    if tags.contains(&tag) {
        return Ok(());
    }
    if tags.len() >= MAX_TAGS {
        return Err(format!("A listing holds at most {MAX_TAGS} tags."));
    }
    tags.push(tag);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(raw: &str) -> String {
        normalize_tag(raw).unwrap_or_else(|e| panic!("{raw:?} refused: {e}"))
    }

    #[test]
    fn tags_normalize_as_the_worker_stores_them() {
        assert_eq!(ok("Zombies"), "zombies");
        assert_eq!(ok("#Zombies"), "zombies");
        assert_eq!(ok("\u{FF5A}\u{FF4F}\u{FF4D}\u{FF42}\u{FF49}\u{FF45}\u{FF53}"), "zombies", "full-width letters fold");
        assert_eq!(ok("  coop  "), "coop");
        assert_eq!(ok("co-op"), "co-op");
        assert_eq!(ok("3d"), "3d");
        assert_eq!(ok("ab"), "ab");
        assert_eq!(ok(&"a".repeat(24)), "a".repeat(24));
    }

    #[test]
    fn bad_tags_are_refused_never_repaired() {
        for raw in [
            "sci_fi", "sci fi", "co--op", "-coop", "coop-", "a", "#", "##coop", "caf\u{E9}", "\u{0437}\u{043E}\u{043C}\u{0431}\u{0438}",
            "\u{1F680}", "",
        ] {
            assert!(normalize_tag(raw).is_err(), "{raw:?} should be refused");
        }
        assert!(normalize_tag(&"a".repeat(25)).is_err());
        let why = normalize_tag("sci_fi").unwrap_err();
        assert!(why.contains("\"sci_fi\"") && why.contains(TAG_RULE), "{why}");
    }

    #[test]
    fn a_list_drops_duplicates_in_order_and_holds_eight() {
        assert_eq!(check_tags(&["#Zombies", "coop", "ZOMBIES"]).unwrap(), ["zombies", "coop"]);
        assert_eq!(check_tags::<&str>(&[]).unwrap(), Vec::<String>::new(), "an empty list clears");
        let eight: Vec<String> = (0..8).map(|i| format!("tag{i}")).collect();
        assert_eq!(check_tags(&eight).unwrap().len(), 8);
        let nine: Vec<String> = (0..9).map(|i| format!("tag{i}")).collect();
        assert!(check_tags(&nine).is_err());
        assert!(check_tags(&["coop", "sci_fi"]).unwrap_err().contains("sci_fi"));
    }

    #[test]
    fn adding_as_an_author_types() {
        let mut tags = Vec::new();
        add_tag(&mut tags, "#Tycoon").unwrap();
        add_tag(&mut tags, "TYCOON").unwrap();
        assert_eq!(tags, ["tycoon"], "a duplicate is ignored");
        assert!(add_tag(&mut tags, "no way").is_err());
        assert_eq!(tags, ["tycoon"], "a refusal leaves the list alone");
        for i in 0..7 {
            add_tag(&mut tags, &format!("t{i}x")).unwrap();
        }
        assert_eq!(tags.len(), 8);
        assert!(add_tag(&mut tags, "ninth").is_err());
    }

    /// The Worker's own cases (`tests/listing_tags_judge.test.mjs`): a phone
    /// number, an email address or a handle is never a tag, and the refusal
    /// names the kind without repeating it.
    #[test]
    fn personal_information_is_never_a_tag() {
        for phone in ["5551234", "555-1234", "1-800-555-1234", "call-555-1234", "call5551234", "#5551234", "555-12-34"] {
            assert_eq!(personal_info_in(phone), Some("a phone number"), "{phone}");
            let why = normalize_tag(phone).unwrap_err();
            assert!(why.contains("a phone number") && !why.contains(phone.trim_start_matches('#')), "{why}");
        }
        for fine in ["2026", "007", "1939-1945", "ww2-1944", "top10", "24h", "3d", "1v1", "123456"] {
            assert_eq!(ok(fine), fine);
        }
        for (raw, kind) in [
            ("ann@mail.com", "an email address"),
            ("ann-at-mail-com", "an email address"),
            ("ann-at-mail-dot-com", "an email address"),
            ("@ann_b", "a handle"),
            ("#@ann", "a handle"),
        ] {
            assert_eq!(personal_info_in(raw), Some(kind), "{raw}");
            let why = normalize_tag(raw).unwrap_err();
            assert!(why.contains(kind) && !why.contains("ann"), "{why}");
        }
        for fine in ["chat-room", "at-home", "cat", "meet-at-noon", "b@d"] {
            assert_eq!(personal_info_in(fine), None, "{fine}");
        }
        assert!(TAG_RULE.contains("never a phone number, an email address or a handle"));
    }
}
