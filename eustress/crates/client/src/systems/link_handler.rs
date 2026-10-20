//! # eustress-player:// opens this Player
//!
//! The gallery's Play button is an `eustress-player://` link, and Windows
//! hands such a link to whichever program is registered for the scheme. A
//! Player installer would register it, but a Player that was only built or
//! unpacked would never be found. So the Player registers itself each time it
//! starts, for the current user only (`HKEY_CURRENT_USER\Software\Classes`, no
//! administrator rights), pointing at the exe that is running: the Player you
//! ran last is the one a link opens. When that Player's API is not production
//! (a staging build, `EUSTRESS_API_URL`, or `--env`), the registered command
//! carries `--api-url`, so a link opened afterwards stays on that server.
//!
//! Each value is read first and written only when it differs, so a second
//! start changes nothing. `EUSTRESS_PLAYER_REGISTER=0` skips it. Linux
//! registers through `install.sh` (xdg-mime), so this is Windows only.
#![cfg_attr(not(windows), allow(dead_code))]

/// The scheme's key under the current user.
pub const SCHEME_KEY: &str = r"Software\Classes\eustress-player";
/// Set to `0` (or `false`, `no`) to leave the registration alone.
pub const REGISTER_VAR: &str = "EUSTRESS_PLAYER_REGISTER";

/// One registry value: its key under the current user, its name (empty for
/// the key's default value) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub name: &'static str,
    pub value: String,
}

/// The command Windows runs for a link: the exe, the API when it is not
/// production, then the link itself.
pub fn command(exe: &str, api_url: Option<&str>) -> String {
    match api_url.filter(|u| !u.is_empty() && !u.contains('"') && !u.chars().any(char::is_whitespace)) {
        Some(api) => format!("\"{exe}\" --api-url \"{api}\" \"%1\""),
        None => format!("\"{exe}\" \"%1\""),
    }
}

/// Every value that makes `eustress-player://` open `exe`.
pub fn entries(exe: &str, api_url: Option<&str>) -> Vec<Entry> {
    let entry = |key: String, name, value: String| Entry { key, name, value };
    vec![
        entry(SCHEME_KEY.to_string(), "", "URL:Eustress Player".to_string()),
        // Its presence, empty, is what marks the key as a URL scheme.
        entry(SCHEME_KEY.to_string(), "URL Protocol", String::new()),
        entry(format!(r"{SCHEME_KEY}\DefaultIcon"), "", format!("\"{exe}\",0")),
        entry(format!(r"{SCHEME_KEY}\shell\open\command"), "", command(exe, api_url)),
    ]
}

/// Where the values live: the registry, or a map in a test.
pub trait Store {
    fn get(&self, key: &str, name: &str) -> Option<String>;
    fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String>;
}

/// Write each entry the store does not already hold; returns how many were
/// written (0 when everything was already in place).
pub fn apply(store: &mut dyn Store, entries: &[Entry]) -> Result<usize, String> {
    let mut written = 0;
    for e in entries {
        if store.get(&e.key, e.name).as_deref() != Some(e.value.as_str()) {
            store.set(&e.key, e.name, &e.value)?;
            written += 1;
        }
    }
    Ok(written)
}

/// Whether `EUSTRESS_PLAYER_REGISTER` leaves registration on (unset is on).
pub fn enabled(var: Option<&str>) -> bool {
    !matches!(var.map(|v| v.trim().to_ascii_lowercase()).as_deref(), Some("0" | "false" | "no" | "off"))
}

/// The current user's registry, under `HKEY_CURRENT_USER`.
#[cfg(windows)]
pub struct CurrentUser;

#[cfg(windows)]
impl Store for CurrentUser {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        windows_registry::CURRENT_USER.open(key).ok()?.get_string(name).ok()
    }

    fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
        let k = windows_registry::CURRENT_USER.create(key).map_err(|e| format!("could not open {key}: {e}"))?;
        k.set_string(name, value).map_err(|e| format!("could not write {key}\\{name}: {e}"))
    }
}

/// Register this exe for `eustress-player://` links, for the current user.
/// `Ok(0)`: already registered as it should be.
#[cfg(windows)]
pub fn register(exe: &std::path::Path, api_url: Option<&str>) -> Result<usize, String> {
    let exe = exe.to_str().ok_or("the Player's path is not valid Unicode")?;
    apply(&mut CurrentUser, &entries(exe, api_url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Map {
        values: HashMap<(String, String), String>,
        writes: usize,
    }

    impl Store for Map {
        fn get(&self, key: &str, name: &str) -> Option<String> {
            self.values.get(&(key.to_string(), name.to_string())).cloned()
        }
        fn set(&mut self, key: &str, name: &str, value: &str) -> Result<(), String> {
            self.writes += 1;
            self.values.insert((key.to_string(), name.to_string()), value.to_string());
            Ok(())
        }
    }

    const EXE: &str = r"C:\Games\Eustress\eustress-client.exe";

    /// The keys a browser looks for: a URL scheme whose open command runs the
    /// Player with the link, quoted.
    #[test]
    fn the_scheme_opens_this_exe_with_the_link() {
        let e = entries(EXE, None);
        let find = |key: &str, name: &str| e.iter().find(|x| x.key == key && x.name == name).map(|x| x.value.clone());
        assert_eq!(find(SCHEME_KEY, "").as_deref(), Some("URL:Eustress Player"));
        assert_eq!(find(SCHEME_KEY, "URL Protocol").as_deref(), Some(""));
        assert_eq!(
            find(r"Software\Classes\eustress-player\shell\open\command", "").as_deref(),
            Some(r#""C:\Games\Eustress\eustress-client.exe" "%1""#)
        );
        assert_eq!(
            find(r"Software\Classes\eustress-player\DefaultIcon", "").as_deref(),
            Some(r#""C:\Games\Eustress\eustress-client.exe",0"#)
        );
    }

    /// A Player on staging registers a link that stays on staging; a bad API
    /// value is left out rather than breaking the command's quoting.
    #[test]
    fn a_non_production_api_rides_along() {
        assert_eq!(
            command(EXE, Some("https://api-staging.eustress.dev")),
            r#""C:\Games\Eustress\eustress-client.exe" --api-url "https://api-staging.eustress.dev" "%1""#
        );
        for bad in ["", "https://x\" --evil", "https://a b"] {
            assert_eq!(command(EXE, Some(bad)), command(EXE, None), "{bad:?}");
        }
    }

    /// The first start writes every value; the next writes none; a moved exe
    /// rewrites only what names it.
    #[test]
    fn registering_twice_changes_nothing() {
        let mut store = Map::default();
        assert_eq!(apply(&mut store, &entries(EXE, None)), Ok(4));
        assert_eq!(apply(&mut store, &entries(EXE, None)), Ok(0));
        assert_eq!(store.writes, 4);
        let moved = r"D:\Eustress\eustress-client.exe";
        assert_eq!(apply(&mut store, &entries(moved, None)), Ok(2), "the icon and the command");
        assert_eq!(apply(&mut store, &entries(moved, Some("https://api-staging.eustress.dev"))), Ok(1), "the command");
    }

    #[test]
    fn registration_can_be_turned_off() {
        assert!(enabled(None) && enabled(Some("1")) && enabled(Some("")));
        for off in ["0", "false", "No", " off "] {
            assert!(!enabled(Some(off)), "{off}");
        }
    }

    /// The real registry, under a throwaway key of this test's own (never
    /// the scheme's): written, read back, unchanged on the second pass, then
    /// removed. Writes to HKEY_CURRENT_USER, so it runs only when asked:
    /// `cargo test -p eustress-client --bin eustress-client link_handler -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore = "writes a throwaway key under HKEY_CURRENT_USER"]
    fn the_registry_round_trip() {
        let root = format!(r"Software\Eustress\Tests\link_handler-{}", std::process::id());
        let mine: Vec<Entry> = entries(EXE, None)
            .into_iter()
            .map(|e| Entry { key: e.key.replacen(SCHEME_KEY, &root, 1), ..e })
            .collect();
        let result = (|| {
            assert_eq!(apply(&mut CurrentUser, &mine)?, 4);
            assert_eq!(apply(&mut CurrentUser, &mine)?, 0);
            assert_eq!(CurrentUser.get(&format!(r"{root}\shell\open\command"), "").as_deref(), Some(command(EXE, None).as_str()));
            Ok::<_, String>(())
        })();
        let _ = windows_registry::CURRENT_USER.remove_tree(&root);
        result.unwrap();
    }
}
