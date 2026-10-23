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
    /// `play/<hex id>` or `space/<hex id>`: a published Space, no host or port.
    Published { sim_id: String },
    /// `space/<slug>`: a published Space named in the gallery, no host or port.
    Space { slug: String },
    Join { link: JoinLink, name: String, ticket: Option<String> },
}

impl std::fmt::Debug for LaunchKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchKind::Gallery => f.write_str("Gallery"),
            LaunchKind::Published { sim_id } => write!(f, "Published({sim_id})"),
            LaunchKind::Space { slug } => write!(f, "Space({slug})"),
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
            LaunchKind::Space { slug } => write!(f, "Space({slug})"),
            LaunchKind::Join { link, name, ticket } => {
                write!(f, "Join({link:?}, {name}, ticket {})", if ticket.is_some() { "set" } else { "absent" })
            }
        }
    }
}

/// Arguments after the program name, the same shapes the desktop Player accepts.
pub fn parse_args(args: &[String], player_api: &str) -> Result<Launch, String> {
    let mut link: Option<(JoinLink, LinkExtras)> = None;
    let mut sim: Option<(String, LinkExtras, PublishedPath)> = None;
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
            "--sim" => sim = Some((value("--sim")?, LinkExtras::default(), PublishedPath::Play)),
            "--api-url" | "--env" => drop(value(arg)?),
            // `play/<id>` and `space/<name>` are published Spaces. They have
            // no host and no port. Sending them through `parse_join_link`
            // reports "the link names no port".
            other if let Some(id) = published_path(other, "eustress://play/") => {
                sim = Some((id, extras, PublishedPath::Play));
            }
            other if let Some(id) = published_path(other, "eustress://space/") => {
                sim = Some((id, extras, PublishedPath::Space));
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

/// Which published-Space path a link used. `Play` is desktop's
/// `eustress-player://play/<hex id>`. `Space` is `eustress-player://space/<name>`,
/// a gallery name when it is not itself a hex id.
#[derive(Clone, Copy)]
enum PublishedPath {
    Play,
    Space,
}

fn finish(
    sim: Option<(String, LinkExtras, PublishedPath)>,
    link: Option<(JoinLink, LinkExtras)>,
    key: Option<String>,
    pin: Option<String>,
    name: Option<String>,
    player_api: &str,
) -> Result<Launch, String> {
    if let Some((sim_id, extras, path)) = sim {
        extras.check_api(player_api)?;
        if sim_id.is_empty() {
            return Err("the link names no simulation".into());
        }
        if is_sim_id(&sim_id) {
            return Ok(Launch { kind: LaunchKind::Published { sim_id } });
        }
        return match path {
            PublishedPath::Play => Err("the simulation id must be hex digits and dashes".into()),
            PublishedPath::Space if is_space_slug(&sim_id) => Ok(Launch { kind: LaunchKind::Space { slug: sim_id } }),
            PublishedPath::Space => Err("the space name must be letters, digits and dashes".into()),
        };
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

fn published_path(arg: &str, prefix: &str) -> Option<String> {
    let rest = arg.strip_prefix(prefix)?;
    let id = rest.split(['?', '#']).next().unwrap_or("");
    Some(id.trim_matches('/').to_string())
}

fn is_sim_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn is_space_slug(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

/// A link safe to print. `ticket` and `key` are secrets; the rest of the
/// URI is what the log is for.
pub fn redact_link(uri: &str) -> String {
    let mut out = String::with_capacity(uri.len());
    for (i, part) in uri.split('&').enumerate() {
        if i > 0 {
            out.push('&');
        }
        let Some((head, _)) = part.split_once('=') else {
            out.push_str(part);
            continue;
        };
        let name = head.rsplit(['?', '/']).next().unwrap_or(head);
        if name == "ticket" || name == "key" {
            let name_at = head.rfind(name).unwrap_or(0);
            out.push_str(&head[..name_at]);
            out.push_str(name);
            out.push_str("=<redacted>");
        } else {
            out.push_str(part);
        }
    }
    out
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

    #[test]
    fn a_space_link_opens_a_published_space_without_a_port() {
        for uri in ["eustress-player://space/box-head", "eustress://space/box-head", "EUSTRESS-PLAYER://space/box-head/"] {
            let launch = parse_uri(uri, STAGING_API).unwrap();
            match launch.kind {
                LaunchKind::Space { slug } => assert_eq!(slug, "box-head"),
                other => panic!("{uri} -> {other:?}"),
            }
        }
        let shown = format!("{:?}", parse_uri("eustress-player://space/box-head", STAGING_API));
        assert!(!shown.contains("port"), "{shown}");
        let by_id = parse_uri(&format!("eustress-player://space/{SIM}"), STAGING_API).unwrap();
        assert!(matches!(by_id.kind, LaunchKind::Published { .. }));
        let join = parse_uri("eustress-player://join/127.0.0.1", STAGING_API).unwrap_err();
        assert!(join.contains("port"), "{join}");
        let redacted = redact_link("eustress-player://join/h:1?key=secret&pin=ab&ticket=abc.def");
        assert!(!redacted.contains("secret") && !redacted.contains("abc.def"), "{redacted}");
        assert!(redacted.contains("h:1") && redacted.contains("pin=ab"), "{redacted}");
        assert_eq!(redact_link("eustress-player://space/box-head"), "eustress-player://space/box-head");
    }
}
