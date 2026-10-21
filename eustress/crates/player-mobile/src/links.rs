//! `eustress-player://` and `eustress://` links.
//!
//! Parsing goes through [`eustress_networking::join_link::split_link_extras`]
//! and [`eustress_networking::join_link::parse_join_link`], the same functions
//! the desktop Player uses in `client/src/main.rs`. A link's `api=` is checked
//! against the API this process was started with. It does not change that API.

use eustress_networking::join_link::{parse_join_link, split_link_extras, JoinLink, LinkExtras};

const PLAYER_SCHEME: &str = "eustress-player://";
const STUDIO_SCHEME: &str = "eustress://";

/// What a link or a command line asked the Player to open.
///
/// `ticket` is the gallery's one-time identity, when the link carried one.
/// It is not printed.
pub struct Launch {
    pub kind: LaunchKind,
}

pub enum LaunchKind {
    Gallery,
    Published { sim_id: String },
    Join { link: JoinLink, name: String, ticket: Option<String> },
}

impl std::fmt::Debug for LaunchKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchKind::Gallery => f.write_str("Gallery"),
            LaunchKind::Published { sim_id } => write!(f, "Published({sim_id})"),
            LaunchKind::Join { link, name, ticket } => {
                write!(f, "Join({link:?}, {name}, ticket {})", if ticket.is_some() { "set" } else { "absent" })
            }
        }
    }
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            LaunchKind::Gallery => f.write_str("Gallery"),
            LaunchKind::Published { sim_id } => write!(f, "Published({sim_id})"),
            LaunchKind::Join { link, name, ticket } => {
                write!(f, "Join({link:?}, {name}, ticket {})", if ticket.is_some() { "set" } else { "absent" })
            }
        }
    }
}

/// Arguments after the program name, the same shapes the desktop Player accepts.
pub fn parse_args(args: &[String], player_api: &str) -> Result<Launch, String> {
    let mut link: Option<(JoinLink, LinkExtras)> = None;
    let mut sim: Option<(String, LinkExtras)> = None;
    let (mut key, mut pin, mut name) = (None, None, None);
    let mut it = args.iter().skip(1);
    while let Some(raw) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        let without_ticket;
        let (arg, extras): (&str, LinkExtras) = match player_link(raw) {
            Some(rest) => {
                let (link_text, found) = split_link_extras(&format!("{STUDIO_SCHEME}{rest}"))?;
                without_ticket = link_text;
                (without_ticket.as_str(), found)
            }
            None => (raw.as_str(), LinkExtras::default()),
        };
        match arg {
            "--connect" => link = Some((parse_join_link(&value("--connect")?)?, LinkExtras::default())),
            "--key" => key = Some(value("--key")?),
            "--pin" => pin = Some(value("--pin")?),
            "--name" => name = Some(value("--name")?),
            "--sim" => sim = Some((value("--sim")?, LinkExtras::default())),
            "--api-url" | "--env" => drop(value(arg)?),
            other if other.starts_with("eustress://play/") => {
                let id = other["eustress://play/".len()..].split('?').next().unwrap_or("");
                sim = Some((id.trim_end_matches('/').to_string(), extras));
            }
            other if other.starts_with(STUDIO_SCHEME) || other.starts_with("https://") => {
                link = Some((parse_join_link(other)?, extras));
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option {}", option_name(other)));
            }
            _ => {}
        }
    }
    finish(sim, link, key, pin, name, player_api)
}

/// One URI from an Android view intent or an iOS open-URL call.
pub fn parse_uri(uri: &str, player_api: &str) -> Result<Launch, String> {
    parse_args(&["eustress-player".to_string(), uri.to_string()], player_api)
}

fn finish(
    sim: Option<(String, LinkExtras)>,
    link: Option<(JoinLink, LinkExtras)>,
    key: Option<String>,
    pin: Option<String>,
    name: Option<String>,
    player_api: &str,
) -> Result<Launch, String> {
    if let Some((sim_id, extras)) = sim {
        let valid = !sim_id.is_empty() && sim_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if !valid {
            return Err("the simulation id must be hex digits and dashes".into());
        }
        extras.check_api(player_api)?;
        return Ok(Launch { kind: LaunchKind::Published { sim_id } });
    }
    if let Some((mut link, extras)) = link {
        extras.check_api(player_api)?;
        if key.is_some() {
            link.key = key;
        }
        if let Some(pin) = pin {
            link.pin = Some(
                eustress_networking::join_link::parse_hex32(&pin).ok_or("--pin must be 64 hex characters")?,
            );
        }
        let name = name
            .or_else(|| std::env::var("EUSTRESS_PLAYER_NAME").ok())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "Player".into());
        let ticket = extras.ticket.map(|ticket| ticket.into_secret());
        return Ok(Launch { kind: LaunchKind::Join { link, name, ticket } });
    }
    Ok(Launch { kind: LaunchKind::Gallery })
}

fn player_link(arg: &str) -> Option<&str> {
    let arg = arg.trim();
    for scheme in [PLAYER_SCHEME, STUDIO_SCHEME] {
        if arg.get(..scheme.len()).is_some_and(|head| head.eq_ignore_ascii_case(scheme)) {
            return Some(&arg[scheme.len()..]);
        }
    }
    None
}

fn option_name(arg: &str) -> String {
    arg.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eustress_common::api_base::{PRODUCTION_API, STAGING_API};

    const SIM: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn play_links_keep_the_id_and_check_the_named_api() {
        let staging = parse_uri(&format!("eustress-player://play/{SIM}?api=staging"), STAGING_API).unwrap();
        match staging.kind {
            LaunchKind::Published { sim_id } => assert_eq!(sim_id, SIM),
            other => panic!("{other:?}"),
        }
        let slash = parse_uri(&format!("eustress-player://play/{SIM}/?api=production"), PRODUCTION_API).unwrap();
        assert!(matches!(slash.kind, LaunchKind::Published { .. }));
        assert!(parse_uri(&format!("eustress-player://play/{SIM}?api=staging"), PRODUCTION_API).is_err());
        assert!(parse_uri(&format!("eustress-player://play/{SIM}?api=nightly"), STAGING_API).is_err());
        assert!(parse_uri("eustress-player://play/not-an-id", STAGING_API).is_err());
        assert!(parse_uri("EUSTRESS-PLAYER://play/", STAGING_API).is_err());
    }

    #[test]
    fn a_join_link_uses_the_shared_parser_and_hides_the_ticket() {
        let uri = "eustress-player://join/127.0.0.1:7777?key=abcd&pin=00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff&api=staging&ticket=abc.def";
        let launch = parse_uri(uri, STAGING_API).unwrap();
        match launch.kind {
            LaunchKind::Join { ref link, ref ticket, .. } => {
                assert_eq!(link.port, 7777);
                assert_eq!(ticket.as_deref(), Some("abc.def"));
                let shown = format!("{launch:?}");
                assert!(!shown.contains("abc.def"));
            }
            other => panic!("{other:?}"),
        }
    }
}
