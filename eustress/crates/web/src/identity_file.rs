// =============================================================================
// Eustress Web - Reading an identity file
// =============================================================================
// A member picks an identity file (`eustress-<name>.toml`) to prove they hold
// their key. This reads the things the page needs from it, the account's public
// key, its private key, its name and the account's id, and nothing else. It is
// pure so it can be tested without a browser; the file is read into memory,
// never sent. Every refusal is a sentence the page can show as it is.
// =============================================================================

/// The most a person's identity file can plausibly hold; anything bigger is some
/// other file.
pub const MAX_IDENTITY_FILE_BYTES: usize = 64 * 1024;

/// Why a file bigger than that is refused.
pub const TOO_LARGE: &str = "it is too large to be an identity file. Pick the file called eustress-<your name>.toml.";

/// The sentence for a file that could not be used, from the reason a reader or the parser gave.
pub fn unreadable(why: &str) -> String {
    format!("We couldn't read this file: {why}")
}

/// What the page takes from an identity file.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityFile {
    /// 64 lower-case hex digits.
    pub public_key: String,
    /// 64 lower-case hex digits. Held in memory only, for as long as the page
    /// needs it to sign.
    pub private_key: String,
    /// The name written in the file; may be empty (an older file).
    pub username: String,
    /// The account's id written in the file, lower-case; `None` in a file that has none.
    pub account_id: Option<String>,
}

fn is_hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The value of a `key = value` line: a quoted string (anything after the closing quote, such
/// as a comment, is ignored), or the bare text up to a `#`.
fn value_of(raw: &str) -> String {
    let raw = raw.trim();
    for quote in ['"', '\''] {
        if let Some(rest) = raw.strip_prefix(quote) {
            return rest.split(quote).next().unwrap_or("").trim().to_string();
        }
    }
    raw.split('#').next().unwrap_or("").trim().to_string()
}

/// The keys and the name of an identity file, from its `[identity]` table (or
/// the top level, an older file). A file with a `[lock]` table is locked with a
/// passphrase and cannot be read here yet.
pub fn parse_identity_file(text: &str) -> Result<IdentityFile, String> {
    if text.len() > MAX_IDENTITY_FILE_BYTES {
        return Err(TOO_LARGE.to_string());
    }
    // A byte-order mark (Notepad and older PowerShell write one) is not part of the first line.
    let text = text.trim_start_matches('\u{feff}');
    let mut section = String::new();
    let mut locked = false;
    let (mut public, mut private, mut username, mut account_id) = (None, None, None, None);
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            // A table header, with whatever follows its closing bracket (a comment) ignored.
            if let Some((name, _)) = header.split_once(']') {
                section = name.trim().trim_matches(|c| c == '[' || c == ']').trim().to_string();
                if section == "lock" {
                    locked = true;
                }
                continue;
            }
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if !section.is_empty() && section != "identity" {
            continue;
        }
        let value = value_of(value);
        match key.trim().trim_matches('"') {
            "public_key" => public = Some(value),
            "private_key" => private = Some(value),
            "username" => username = Some(value),
            "user_id" => account_id = Some(value.to_ascii_lowercase()).filter(|v| !v.is_empty()),
            _ => {}
        }
    }
    if locked {
        return Err("it is locked with a passphrase. Locked files cannot be used here yet.".to_string());
    }
    if public.is_none() && private.is_none() {
        return Err("it does not look like an identity file: it has no keys. Pick the file called eustress-<your name>.toml.".to_string());
    }
    let public_key = public.ok_or("it has no public key.")?.to_ascii_lowercase();
    let private_key = private.ok_or("it has no private key.")?.to_ascii_lowercase();
    if !is_hex64(&public_key) {
        return Err("its public key is not in the form this page reads (64 hex digits). It may be a very old file.".to_string());
    }
    if !is_hex64(&private_key) {
        return Err("its private key is not in the form this page reads (64 hex digits). It may be a very old file.".to_string());
    }
    Ok(IdentityFile { public_key, private_key, username: username.unwrap_or_default(), account_id })
}

/// A key short enough to recognise on a screen, not to copy: `d75a9801...f707511a`.
pub fn fingerprint(public_key: &str) -> String {
    if public_key.len() < 16 || !public_key.is_ascii() {
        return public_key.to_string();
    }
    format!("{}...{}", &public_key[..8], &public_key[public_key.len() - 8..])
}

/// Whether the API this page talks to is production's.
pub fn is_production(origin: &str) -> bool {
    origin.trim_end_matches('/') == "https://api.eustress.dev"
}

/// The file the member picked, against the account they are signed in to: `Ok` when it is the
/// key the account signs in with now, otherwise the sentence that says which way it is wrong.
/// `account_key` and `account_id` come from the Worker's answer, `origin` is the API in use.
pub fn check_against_account(file: &IdentityFile, account_key: &str, account_id: &str, origin: &str) -> Result<(), String> {
    if file.public_key == account_key {
        return Ok(());
    }
    // Which environment this is matters for the advice: a file from one is never a key on the other.
    let environment = if is_production(origin) {
        ""
    } else {
        " You are on the Eustress test site (staging): an identity file from the real site, eustress.dev, does not work here, and one from here does not work there."
    };
    if let Some(id) = &file.account_id {
        if !id.eq_ignore_ascii_case(account_id) {
            return Err(format!("This file is for a different account than the one you are signed in to. Pick the identity file for this account.{environment}"));
        }
    }
    Err(format!(
        "This is not the identity file your account signs in with now. If you replaced your key before, pick the newest file.{environment}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    const PRIVATE: &str = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
    const ID: &str = "3f2b8a52-9c1e-4d7a-8b0f-6a5e4c3d2b1a";

    fn slim() -> String {
        format!(
            "# Eustress Identity - generated now\n# This file IS your account. Keep it safe. Do not share your private key.\n\n[identity]\nversion = \"1.0\"\nuser_id = \"{ID}\"\nusername = \"someone\"\npublic_key = \"{PUBLIC}\"\nprivate_key = \"{PRIVATE}\"\nissued_at = \"now\"\n\n[verification]\nverified = false\n"
        )
    }

    fn expected() -> IdentityFile {
        IdentityFile { public_key: PUBLIC.into(), private_key: PRIVATE.into(), username: "someone".into(), account_id: Some(ID.into()) }
    }

    #[test]
    fn reads_the_file_the_sign_up_page_builds() {
        assert_eq!(parse_identity_file(&slim()).unwrap(), expected());
    }

    #[test]
    fn reads_a_file_from_before_the_id_data_was_removed() {
        let old = slim().replace("[verification]\nverified = false\n", "[profile]\nbirthday = \"1990-01-01\"\n\n[verification]\nid_type = \"passport\"\nid_hash = \"abc\"\nverified = false\n");
        assert_eq!(parse_identity_file(&old).unwrap(), expected());
    }

    #[test]
    fn reads_the_keys_at_the_top_level_of_an_older_file_and_in_either_case() {
        let text = format!("public_key = \"{}\"\nprivate_key = \"{}\"\n", PUBLIC.to_uppercase(), PRIVATE.to_uppercase());
        let f = parse_identity_file(&text).unwrap();
        assert_eq!((f.public_key.as_str(), f.private_key.as_str(), f.username.as_str(), f.account_id), (PUBLIC, PRIVATE, "", None));
    }

    #[test]
    fn reads_a_file_with_windows_line_endings_a_byte_order_mark_and_comments_on_the_lines() {
        let crlf = slim().replace('\n', "\r\n");
        assert_eq!(parse_identity_file(&crlf).unwrap(), expected());
        assert_eq!(parse_identity_file(&format!("\u{feff}{}", slim())).unwrap(), expected(), "a mark before a comment");
        assert_eq!(parse_identity_file(&format!("\u{feff}{}", slim().replace("# Eustress Identity - generated now\n", ""))).unwrap().public_key, PUBLIC, "a mark before the table");
        let commented = slim()
            .replace("[identity]", "[identity] # the account")
            .replace(&format!("public_key = \"{PUBLIC}\""), &format!("public_key = \"{PUBLIC}\" # shown to everyone"))
            .replace(&format!("private_key = \"{PRIVATE}\""), &format!("private_key = '{PRIVATE}'   # never shown"));
        assert_eq!(parse_identity_file(&commented).unwrap(), expected());
        let bare = format!("[identity]\npublic_key = {PUBLIC}\nprivate_key = {PRIVATE} # bare values\n");
        assert_eq!(parse_identity_file(&bare).unwrap().private_key, PRIVATE);
    }

    #[test]
    fn a_key_in_another_table_is_not_the_account_key() {
        let text = format!("[identity]\nversion = \"1.0\"\n\n[other]\npublic_key = \"{PUBLIC}\"\nprivate_key = \"{PRIVATE}\"\n");
        assert!(parse_identity_file(&text).is_err());
    }

    #[test]
    fn refuses_what_it_cannot_use_and_says_why() {
        let locked = format!("[identity]\nversion = \"1.0\"\nusername = \"x\"\npublic_key = \"{PUBLIC}\"\n\n[lock]\nversion = 1\nkdf = \"argon2id\"\n");
        assert!(parse_identity_file(&locked).unwrap_err().contains("locked"));
        assert!(parse_identity_file("").unwrap_err().contains("does not look like an identity file"));
        assert!(parse_identity_file("name = \"a recipe\"\nsteps = 3\n").unwrap_err().contains("no keys"));
        assert!(parse_identity_file(&format!("[identity]\npublic_key = \"{PUBLIC}\"\n")).unwrap_err().contains("no private key"));
        assert!(parse_identity_file(&format!("[identity]\nprivate_key = \"{PRIVATE}\"\n")).unwrap_err().contains("no public key"));
        assert!(parse_identity_file(&format!("[identity]\npublic_key = \"{}\"\nprivate_key = \"{PRIVATE}\"\n", "z".repeat(64))).unwrap_err().contains("public key"));
        assert!(parse_identity_file(&format!("[identity]\npublic_key = \"{PUBLIC}\"\nprivate_key = \"{}\"\n", "ab".repeat(31))).unwrap_err().contains("private key"));
        // The base64 form some very old files used is named as a form this page does not read.
        let b64 = "[identity]\npublic_key = \"11qYAYKxCrfVS/7TyWQHOg7hcvPdqmIlrwIaaPcHUR8=\"\nprivate_key = \"nWGxne/9WmC6hEr0kuwsxERJxWl7MmkZcDusAxyuf2A=\"\n";
        assert!(parse_identity_file(b64).unwrap_err().contains("very old file"));
        assert!(parse_identity_file(&"x".repeat(MAX_IDENTITY_FILE_BYTES + 1)).unwrap_err().contains("too large"));
    }

    #[test]
    fn a_refusal_reads_as_one_sentence_the_page_can_show() {
        assert_eq!(unreadable("it has no private key."), "We couldn't read this file: it has no private key.");
        for why in [parse_identity_file("").unwrap_err(), parse_identity_file("[lock]\n").unwrap_err(), TOO_LARGE.to_string()] {
            assert!(why.starts_with("it "), "{why}");
            assert!(why.ends_with('.'), "{why}");
        }
    }

    #[test]
    fn a_key_is_shortened_to_what_a_person_can_recognise() {
        assert_eq!(fingerprint(PUBLIC), "d75a9801...f707511a");
        assert_eq!(fingerprint("abc"), "abc");
        assert_eq!(fingerprint(""), "");
    }

    const STAGING: &str = "https://api-staging.eustress.dev";
    const PRODUCTION: &str = "https://api.eustress.dev";

    #[test]
    fn the_file_for_the_account_passes_and_every_other_file_says_how_it_is_wrong() {
        let file = parse_identity_file(&slim()).unwrap();
        assert_eq!(check_against_account(&file, PUBLIC, ID, STAGING), Ok(()));
        assert_eq!(check_against_account(&file, PUBLIC, "someone-else", STAGING), Ok(()), "the key decides, not the name written in the file");

        // Another account's file.
        let other = "a".repeat(64);
        let different = check_against_account(&file, &other, "99999999-9999-4999-8999-999999999999", PRODUCTION).unwrap_err();
        assert!(different.starts_with("This file is for a different account"), "{different}");
        assert!(!different.contains("staging"), "on production the sentence says nothing of staging: {different}");
        let different_on_staging = check_against_account(&file, &other, "99999999-9999-4999-8999-999999999999", STAGING).unwrap_err();
        assert!(different_on_staging.contains("staging") && different_on_staging.contains("eustress.dev"), "{different_on_staging}");

        // The account's own file, but not the key it signs in with now (an older file).
        let older = check_against_account(&file, &other, &ID.to_uppercase(), PRODUCTION).unwrap_err();
        assert!(older.starts_with("This is not the identity file your account signs in with now"), "{older}");
        assert!(older.contains("newest file"));

        // A file with no account id in it can only be judged by its key.
        let top = parse_identity_file(&format!("public_key = \"{PUBLIC}\"\nprivate_key = \"{PRIVATE}\"\n")).unwrap();
        let unknown = check_against_account(&top, &other, "99999999-9999-4999-8999-999999999999", STAGING).unwrap_err();
        assert!(unknown.starts_with("This is not the identity file your account signs in with now"), "{unknown}");
        assert!(unknown.contains("test site (staging)"), "{unknown}");
    }

    #[test]
    fn production_is_the_one_api_address_and_nothing_like_it() {
        assert!(is_production("https://api.eustress.dev"));
        assert!(is_production("https://api.eustress.dev/"));
        for not in [STAGING, "http://127.0.0.1:8787", "https://api.eustress.dev.evil.example", ""] {
            assert!(!is_production(not), "{not}");
        }
    }
}
