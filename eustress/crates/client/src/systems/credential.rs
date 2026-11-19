//! # A credential goes only to the API origin that issued it
//!
//! Every request the Player makes with a credential is built here
//! ([`request`]), or takes its credential from here ([`Credential::secret_for`])
//! when another crate sends it. A [`Credential`] carries the web origin of the
//! API that issued it (scheme, host and port, as the API computes an origin),
//! and it is attached only to a URL on that same origin: another Eustress
//! service, plain http, another port, or a look-alike host such as
//! `api.eustress.dev.evil.com` gets the request without it. A credential is
//! never put in a URL, and its `Debug` never prints it.
//!
//! Two kinds exist: the sign-in the Player borrows from Studio
//! ([`studio_login`]), sent as `Authorization: Bearer`, and a pass for one
//! simulation ([`CredentialKind::PlayPass`]), sent as
//! `Authorization: PlayPass`, bound to the API that issued it with
//! [`Credential::issued_by`].
//!
//! ## Where Studio's sign-in comes from
//!
//! Studio saves one login per API under `<local data>/EustressEngine/`
//! (`auth_token` for production, `auth_token.<scope>` for any other;
//! `eustress_common::api_base::login_file_for`), and beside it a record of the
//! origin that issued it (`<login file>.origin`;
//! `eustress_common::api_base::login_origin_file_for`). A login with a record
//! belongs to the origin the record names. A login with no record (saved by a
//! Studio older than the record) belongs to production only when it is
//! production's file: Studio has only ever saved production's login there
//! (before per-API files it signed in to production alone). Any other login
//! without a record belongs nowhere, and is sent nowhere until Studio saves
//! it again, which it does at its next start.

use std::path::Path;

use eustress_common::api_base;

/// How a credential is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    /// The sign-in borrowed from Studio: `Authorization: Bearer <token>`.
    StudioToken,
    /// A pass for one simulation: `Authorization: PlayPass <pass>`.
    PlayPass,
}

impl CredentialKind {
    fn scheme(self) -> &'static str {
        match self {
            CredentialKind::StudioToken => "Bearer",
            CredentialKind::PlayPass => "PlayPass",
        }
    }
}

/// A credential, bound to the web origin of the API that issued it.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    kind: CredentialKind,
    secret: String,
    origin: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("kind", &self.kind)
            .field("origin", &self.origin)
            .field("secret", &format_args!("<redacted>"))
            .finish()
    }
}

impl Credential {
    /// A credential of `kind` that the API at `issuer` issued: the API's base
    /// (`https://api-staging.eustress.dev`) or any URL on it, such as the one
    /// the credential was fetched from. `None` when `issuer` is not an http or
    /// https URL with a host, or when `secret` is empty or holds whitespace or
    /// a control character (it could not be one header value).
    pub fn issued_by(kind: CredentialKind, secret: impl Into<String>, issuer: &str) -> Option<Self> {
        let secret = secret.into();
        let secret = secret.trim();
        if secret.is_empty() || secret.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return None;
        }
        Some(Self { kind, secret: secret.to_string(), origin: web_origin(issuer)? })
    }

    pub fn kind(&self) -> CredentialKind {
        self.kind
    }

    /// The origin of the API that issued it, such as `https://api.eustress.dev`.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The secret, for a request to `url`: only when `url` is on the origin
    /// that issued it. For a crate that sends it itself (the avatar fetch);
    /// a request built here goes through [`request`] instead.
    pub fn secret_for(&self, url: &str) -> Option<&str> {
        (web_origin(url).as_deref() == Some(self.origin.as_str())).then_some(self.secret.as_str())
    }

    /// The `Authorization` value for a request to `url`, under the same rule
    /// as [`Self::secret_for`].
    fn header_for(&self, url: &str) -> Option<reqwest::header::HeaderValue> {
        let secret = self.secret_for(url)?;
        let mut value = reqwest::header::HeaderValue::from_str(&format!("{} {secret}", self.kind.scheme())).ok()?;
        // Kept out of the header map's `Debug`.
        value.set_sensitive(true);
        Some(value)
    }
}

/// `url`'s web origin, as the API computes one (`https://host`, the default
/// port dropped, the host in lower case), or `None` for anything that is not
/// an http or https URL with a host.
pub fn web_origin(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url.trim()).ok()?;
    if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

/// A request for `url` on `client`, carrying `credential` only when `url` is
/// on the origin that issued it; otherwise the same request without one.
/// Every Player request that may carry a credential is built here. The URL
/// checked is the URL sent. reqwest drops the `Authorization` header on a
/// redirect to another host or port.
pub fn request(
    client: &reqwest::blocking::Client,
    method: reqwest::Method,
    url: &str,
    credential: Option<&Credential>,
) -> reqwest::blocking::RequestBuilder {
    let mut builder = client.request(method, url);
    if let Some(value) = credential.and_then(|c| c.header_for(url)) {
        builder = builder.header(reqwest::header::AUTHORIZATION, value);
    }
    builder
}

/// The folder Studio saves its logins in.
fn login_dir() -> Option<std::path::PathBuf> {
    Some(dirs::data_local_dir()?.join("EustressEngine"))
}

/// Studio's saved sign-in for the API in use, when that API issued it.
/// `None` when signed out, and when the saved login belongs to another API or
/// to none ([module docs](self)).
pub fn studio_login() -> Option<Credential> {
    studio_login_in(&login_dir()?, api_base::api_base())
}

/// Studio's saved sign-in for the API at `base` (not necessarily the one in
/// use), when that API issued it. For a fetch that falls back to another API:
/// the login it gets is only ever sent to the origin that issued it.
pub fn studio_login_for(base: &str) -> Option<Credential> {
    studio_login_in(&login_dir()?, base)
}

/// Studio's sign-in for a request to `url`: its token, only when `url` is on
/// the origin that issued it. For a crate that takes a bare token (the
/// avatar fetch, `SpawnSavedAvatar`).
pub fn studio_token_for(url: &str) -> Option<String> {
    studio_login()?.secret_for(url).map(str::to_string)
}

/// [`studio_login`] for the API at `base`, from the logins in `dir`.
fn studio_login_in(dir: &Path, base: &str) -> Option<Credential> {
    read_studio_login(dir, base).filter(|login| login.secret_for(base).is_some())
}

/// The login Studio saved for the API at `base`, bound to the origin its
/// record names (or, without one, to production for production's file).
fn read_studio_login(dir: &Path, base: &str) -> Option<Credential> {
    let file = api_base::login_file_for(base);
    let token = std::fs::read_to_string(dir.join(&file)).ok()?;
    let record = std::fs::read_to_string(dir.join(api_base::login_origin_file_for(base))).ok();
    let issuer = login_issuer(&file, record.as_deref())?;
    Credential::issued_by(CredentialKind::StudioToken, token, &issuer)
}

/// The origin a saved login belongs to, from its file's name and the text of
/// its origin record (`None`: no record). A record must hold exactly one
/// origin, as Studio writes it; anything else names no origin. Without a
/// record only production's own file belongs anywhere: to production.
fn login_issuer(file_name: &str, record: Option<&str>) -> Option<String> {
    match record.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => web_origin(text).filter(|origin| origin == text),
        None if file_name == api_base::login_file_for(api_base::PRODUCTION_API) => web_origin(api_base::PRODUCTION_API),
        None => None,
    }
}

/// The Eustress service Studio is signed in to on this computer when it is
/// not the API in use, as people call each: `(signed in to, this one)`, such
/// as `("production", "staging")`. `None` when signed in to the API in use,
/// or to no other known service.
pub fn signed_in_elsewhere() -> Option<(String, String)> {
    signed_in_elsewhere_in(&login_dir()?, api_base::api_base())
}

fn signed_in_elsewhere_in(dir: &Path, base: &str) -> Option<(String, String)> {
    if studio_login_in(dir, base).is_some() {
        return None;
    }
    let here = web_origin(base);
    [api_base::PRODUCTION_API, api_base::STAGING_API]
        .into_iter()
        .filter(|other| web_origin(other) != here)
        .find(|other| studio_login_in(dir, other).is_some())
        .map(|other| (api_base::server_name(other), api_base::server_name(base)))
}

/// What a Player signed in to another service is told to do.
pub fn signed_in_elsewhere_hint(signed_in_to: &str, this_one: &str) -> String {
    format!(
        "This Player is signed in to {signed_in_to} but this simulation is on {this_one}. \
         Sign in to {this_one} with Eustress Studio, then press Play again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_base::{PRODUCTION_API, STAGING_API};

    fn sent(credential: Option<&Credential>, url: &str) -> Option<String> {
        let client = reqwest::blocking::Client::new();
        let built = request(&client, reqwest::Method::GET, url, credential).build().expect("the request builds");
        built.headers().get(reqwest::header::AUTHORIZATION).map(|v| v.to_str().unwrap().to_string())
    }

    fn production(secret: &str) -> Credential {
        Credential::issued_by(CredentialKind::StudioToken, secret, PRODUCTION_API).unwrap()
    }

    /// A folder of its own for one test's login files.
    fn login_folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eustress-player-token-origin-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A credential goes with a request to the origin that issued it, however
    /// that origin's URL is written, and the same credential stays off the
    /// same request to another service.
    #[test]
    fn player_token_origin_sent_to_its_own_origin() {
        let login = production("eit1.session");
        for url in [
            "https://api.eustress.dev/api/identity/ticket",
            "https://api.eustress.dev",
            "https://API.Eustress.dev:443/api/avatar",
        ] {
            assert_eq!(sent(Some(&login), url).as_deref(), Some("Bearer eit1.session"), "{url}");
        }
        assert_eq!(login.secret_for("https://api.eustress.dev/api/avatar"), Some("eit1.session"));
        assert_eq!(sent(Some(&login), "https://api-staging.eustress.dev/api/identity/ticket"), None, "the control");
        let pass = Credential::issued_by(CredentialKind::PlayPass, "pp1.abc", "https://api-staging.eustress.dev/api/simulations/x/play-pass").unwrap();
        assert_eq!(pass.origin(), STAGING_API);
        assert_eq!(sent(Some(&pass), "https://api-staging.eustress.dev/api/simulations/x/world/manifest").as_deref(), Some("PlayPass pp1.abc"));
        assert_eq!(sent(Some(&pass), "https://api.eustress.dev/api/simulations/x/world/manifest"), None, "the control");
        assert_eq!(sent(None, "https://api.eustress.dev/api/avatar"), None, "a guest sends nothing");
    }

    /// Another Eustress service, plain http, another port, and every
    /// look-alike host get the request without the credential.
    #[test]
    fn player_token_origin_not_sent_to_another_origin() {
        let login = production("eit1.session");
        let staging = Credential::issued_by(CredentialKind::StudioToken, "eit1.staging", STAGING_API).unwrap();
        assert_eq!(sent(Some(&login), "https://api-staging.eustress.dev/api/identity/ticket"), None, "production's to staging");
        assert_eq!(sent(Some(&staging), "https://api.eustress.dev/api/identity/ticket"), None, "staging's to production");
        for url in [
            "https://api.eustress.dev.evil.com/api/avatar",
            "https://evilapi.eustress.dev/api/avatar",
            "https://api.eustress.dev@evil.com/api/avatar",
            "https://evil.com/?next=https://api.eustress.dev",
            "https://evil.com/https://api.eustress.dev",
            "http://api.eustress.dev/api/avatar",
            "https://api.eustress.dev:8443/api/avatar",
            "https://eustress.dev/api/avatar",
            "api.eustress.dev/api/avatar",
            "file:///C:/api.eustress.dev",
        ] {
            assert_eq!(login.secret_for(url), None, "{url}");
            if url.starts_with("http") {
                // reqwest turns a URL's own user part into Basic auth; the
                // credential itself is never in the header.
                let header = sent(Some(&login), url);
                assert!(header.as_deref().map_or(true, |h| !h.contains("eit1.session")), "{url}: {header:?}");
            }
        }
        let local = Credential::issued_by(CredentialKind::StudioToken, "eit1.local", "http://localhost:8787").unwrap();
        assert_eq!(local.secret_for("http://localhost:8788/x"), None, "another port");
        assert_eq!(local.secret_for("http://127.0.0.1:8787/x"), None, "another host for the same machine");
        assert_eq!(local.secret_for("http://localhost:8787/x"), Some("eit1.local"));
        // Nothing that is not one header value, and no issuer that is not an origin.
        assert!(Credential::issued_by(CredentialKind::StudioToken, "a\r\nX-Evil: 1", PRODUCTION_API).is_none());
        assert!(Credential::issued_by(CredentialKind::StudioToken, "", PRODUCTION_API).is_none());
        assert!(Credential::issued_by(CredentialKind::StudioToken, "t", "api.eustress.dev").is_none());
        assert!(!format!("{login:?}").contains("eit1.session"), "Debug never prints the secret");
    }

    /// A login saved before Studio recorded origins is production's only in
    /// production's own file; any other login without a record is sent
    /// nowhere, and a record must name the origin of the API asking.
    #[test]
    fn player_token_origin_old_file_without_an_origin() {
        let staging_file = api_base::login_file_for(STAGING_API);
        assert_eq!(login_issuer("auth_token", None).as_deref(), Some(PRODUCTION_API));
        assert_eq!(login_issuer(&staging_file, None), None, "no record, not production's file: no origin");
        assert_eq!(login_issuer(&staging_file, Some("https://api-staging.eustress.dev\n")).as_deref(), Some(STAGING_API));
        assert_eq!(login_issuer(&staging_file, Some("https://api-staging.eustress.dev/x")), None, "not an origin alone");
        assert_eq!(login_issuer(&staging_file, Some("staging")), None);

        let dir = login_folder("old");
        std::fs::write(dir.join("auth_token"), "eit1.production\n").unwrap();
        let login = studio_login_in(&dir, PRODUCTION_API).expect("production's old login is production's");
        assert_eq!(login.origin(), PRODUCTION_API);
        assert_eq!(login.secret_for("https://api.eustress.dev/api/avatar"), Some("eit1.production"));
        assert!(studio_login_in(&dir, STAGING_API).is_none(), "production's login is not staging's");

        std::fs::write(dir.join(&staging_file), "eit1.staging").unwrap();
        assert!(studio_login_in(&dir, STAGING_API).is_none(), "a staging login without a record is sent nowhere");
        std::fs::write(dir.join(api_base::login_origin_file_for(STAGING_API)), "https://api.eustress.dev\n").unwrap();
        assert!(studio_login_in(&dir, STAGING_API).is_none(), "a record naming another API never sends it to this one");
        std::fs::write(dir.join(api_base::login_origin_file_for(STAGING_API)), "https://api-staging.eustress.dev\n").unwrap();
        let staging = studio_login_in(&dir, STAGING_API).expect("recorded for staging");
        assert_eq!(staging.secret_for("https://api-staging.eustress.dev/api/avatar"), Some("eit1.staging"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Signed in to production only, a Player on staging is told so in
    /// these words, and one signed in to its own service is told nothing.
    #[test]
    fn player_token_origin_mismatch_notice_wording() {
        let dir = login_folder("mismatch");
        std::fs::write(dir.join("auth_token"), "eit1.production").unwrap();
        let (signed_in_to, this_one) = signed_in_elsewhere_in(&dir, STAGING_API).expect("signed in to production only");
        assert_eq!((signed_in_to.as_str(), this_one.as_str()), ("production", "staging"));
        assert_eq!(
            signed_in_elsewhere_hint(&signed_in_to, &this_one),
            "This Player is signed in to production but this simulation is on staging. \
             Sign in to staging with Eustress Studio, then press Play again."
        );
        assert_eq!(signed_in_elsewhere_in(&dir, PRODUCTION_API), None, "signed in to the service in use");
        let empty = login_folder("mismatch-empty");
        assert_eq!(signed_in_elsewhere_in(&empty, STAGING_API), None, "signed in nowhere");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }
}
