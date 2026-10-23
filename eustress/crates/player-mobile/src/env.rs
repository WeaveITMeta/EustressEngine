//! Which Eustress API this Player calls.
//!
//! Desktop Studio and the desktop Player default to production
//! ([`eustress_common::api_base::PRODUCTION_API`]) unless `EUSTRESS_API_URL`
//! or `--api-url` says otherwise. The mobile Player defaults to staging.
//! A link's `api=` parameter does not change this: it only checks that the
//! link belongs to the API already chosen, the same rule as the desktop
//! Player (`Launch::check_api`).
//!
//! [`install`] must run before anything calls [`eustress_common::api_base::api_base`].
//! That function reads the variable once and keeps the result.

use eustress_common::api_base::{api_for_name, API_URL_VAR, STAGING_API};

/// The base to use, before it is written into the environment.
///
/// `--api-url` wins, then `--env production|staging`, then a value already
/// in `EUSTRESS_API_URL`, then staging.
pub fn choose_api(args: &[String], env_value: Option<&str>) -> Result<String, String> {
    if let Some(url) = flag(args, "--api-url") {
        return Ok(url);
    }
    if let Some(name) = flag(args, "--env") {
        return api_for_name(&name)
            .map(|url| url.to_string())
            .ok_or_else(|| format!("--env {name:?} is not production or staging"));
    }
    if let Some(raw) = env_value.map(str::trim).filter(|value| !value.is_empty()) {
        return Ok(raw.to_string());
    }
    Ok(STAGING_API.to_string())
}

/// Point [`eustress_common::api_base::api_base`] at [`choose_api`].
pub fn install(args: &[String]) -> Result<(), String> {
    let chosen = choose_api(args, std::env::var(API_URL_VAR).ok().as_deref())?;
    // rustc on this workspace still treats set_var as safe. The call is once,
    // before api_base's OnceLock.
    std::env::set_var(API_URL_VAR, chosen);
    Ok(())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|arg| arg == name).and_then(|index| args.get(index + 1)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_defaults_to_staging_and_flags_override_it() {
        assert_eq!(choose_api(&["player".into()], None).unwrap(), STAGING_API);
        assert_eq!(
            choose_api(&["player".into(), "--env".into(), "production".into()], None).unwrap(),
            eustress_common::api_base::PRODUCTION_API
        );
        assert_eq!(
            choose_api(
                &["player".into(), "--api-url".into(), "http://127.0.0.1:8787".into(), "--env".into(), "staging".into()],
                Some("https://api.eustress.dev")
            )
            .unwrap(),
            "http://127.0.0.1:8787"
        );
        assert!(choose_api(&["player".into(), "--env".into(), "nightly".into()], None).is_err());
        assert_eq!(
            choose_api(&["player".into()], Some("http://localhost:8787")).unwrap(),
            "http://localhost:8787"
        );
    }
}
