//! AWS S3 source — the Data menu's "S3" provider (also MinIO, R2 and Spaces,
//! which speak the same API).
//!
//! ## Why this module links no client
//!
//! `aws-sdk-s3` is an async, Tokio-based SDK with a large transitive graph;
//! `eustress-data` is a dependency-free leaf (`DATA_PLATFORM_PLAN.md` invariant
//! **D2**) consumed by a synchronous ECS, so it cannot host one. Nor can this
//! module reach an object over plain HTTPS the way [`super::azure`] does: every
//! authenticated S3 request needs an AWS SigV4 signature, which needs HMAC-
//! SHA256 — a cryptographic dependency, not a formatting trick.
//!
//! So this module does the honest thing. It resolves the bucket, key, region
//! and object format completely and offline, validates every one of them
//! against the real S3 naming rules, exposes the exact URL the object lives at,
//! and then returns an error that names the missing feature. Nothing here
//! pretends to have fetched anything.
//!
//! If the object is public, the resolved [`S3Object::https_url`] is an ordinary
//! HTTPS URL that the REST provider can read today — the error message says so.
//!
//! ## Secrets
//!
//! [`SourceConfig::secret_ref`] names the environment variable holding the
//! secret access key. The key id is not a secret and rides in the `access_key_id`
//! option; the secret itself is read at call time and never stored.

use std::fmt;

use super::{ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// Cargo feature that would compile a real S3 client in.
const FEATURE: &str = "s3";

/// Region assumed for a custom (MinIO / R2 / Spaces) endpoint that does not
/// name one. AWS itself always requires an explicit region.
const DEFAULT_CUSTOM_REGION: &str = "us-east-1";

/// The longest key S3 accepts, in bytes.
const MAX_KEY_BYTES: usize = 1024;

// ── Resolved target ──────────────────────────────────────────────────────────

/// How the object's bytes are to be decoded once a client exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectFormat {
    /// Comma-separated values with a header row.
    Csv,
    /// A JSON document.
    Json,
    /// Newline-delimited JSON objects.
    Jsonl,
    /// Apache Parquet.
    Parquet,
}

impl ObjectFormat {
    /// Stable wire token, as written in the `format` option.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Jsonl => "jsonl",
            Self::Parquet => "parquet",
        }
    }

    /// Parse the `format` option.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "json" => Some(Self::Json),
            "jsonl" | "ndjson" => Some(Self::Jsonl),
            "parquet" => Some(Self::Parquet),
            _ => None,
        }
    }

    /// Infer from an object key's extension.
    pub fn from_key(key: &str) -> Option<Self> {
        let ext = key.rsplit('.').next()?;
        if ext == key {
            return None; // no dot at all
        }
        Self::parse(ext)
    }

    /// Every token the `format` option accepts, for error messages.
    const TOKENS: &'static str = "csv, json, jsonl, parquet";
}

/// A fully resolved S3 object — everything a client would need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Object {
    /// Bucket name.
    pub bucket: String,
    /// Object key (no leading slash).
    pub key: String,
    /// Region the bucket lives in.
    pub region: String,
    /// Custom service endpoint (MinIO / R2 / Spaces), if the config named one.
    /// `None` means AWS S3 proper.
    pub endpoint: Option<String>,
    /// How to decode the object's bytes.
    pub format: ObjectFormat,
    /// Refuse to read more than this many bytes, if the config capped it.
    pub max_bytes: Option<u64>,
    /// Whether the object is to be read without credentials.
    pub anonymous: bool,
}

impl S3Object {
    /// The HTTPS URL this object lives at.
    ///
    /// Virtual-hosted style for AWS, path style for a custom endpoint — the
    /// same choice `force_path_style` makes in an SDK. Pure, so the UI can show
    /// it and a test can assert on it.
    pub fn https_url(&self) -> String {
        match &self.endpoint {
            Some(e) => format!("{}/{}/{}", e.trim_end_matches('/'), self.bucket, self.key),
            None => {
                format!("https://{}.s3.{}.amazonaws.com/{}", self.bucket, self.region, self.key)
            }
        }
    }
}

impl fmt::Display for S3Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s3://{}/{} ({}, {})", self.bucket, self.key, self.region, self.format.as_str())
    }
}

// ── Source ───────────────────────────────────────────────────────────────────

/// The S3 provider.
///
/// Construction validates; an `S3Source` that exists is a config the platform
/// has fully understood.
#[derive(Debug, Clone)]
pub struct S3Source {
    config: SourceConfig,
    object: S3Object,
}

impl S3Source {
    /// Validate `config` and resolve its object.
    pub fn new(config: SourceConfig) -> Result<Self> {
        validate(&config)?;
        let object = resolve_object(&config)?;
        Ok(Self { config, object })
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The resolved object.
    pub fn object(&self) -> &S3Object {
        &self.object
    }

    /// The secret access key, read from the environment variable named by
    /// [`SourceConfig::secret_ref`], at call time. Never cached or logged.
    pub fn secret_access_key(&self) -> Option<String> {
        self.config.resolve_secret()
    }
}

impl DataSource for S3Source {
    fn kind(&self) -> SourceKind {
        SourceKind::S3
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        Err(unsupported(&self.object, "HEAD"))
    }

    fn fetch(&self) -> Result<Frame> {
        Err(unsupported(&self.object, "GET"))
    }
}

/// The one place this module admits it cannot reach a bucket.
///
/// [`std::io::ErrorKind::Unsupported`] is deliberate: it lets a caller tell
/// "this build cannot do it" apart from [`DataError::Schema`], which always
/// means "your configuration is wrong and you can fix it".
fn unsupported(object: &S3Object, verb: &str) -> DataError {
    DataError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!(
            "S3 support requires the '{FEATURE}' feature, which is not compiled in — \
             eustress-data links no S3 client, and an authenticated S3 request needs an AWS \
             SigV4 signature (HMAC-SHA256), which this leaf carries no crypto for. The config \
             is valid: it would {verb} {object} at {}. If that object is public, a REST source \
             pointed at the same URL reads it today.",
            object.https_url()
        ),
    ))
}

// ── Validation (pure, offline) ───────────────────────────────────────────────

/// Validate an S3 config completely, without touching the network.
///
/// Runs the shared [`super::validate_config`] checks first, then everything
/// specific to this provider: URI shape, S3 bucket naming rules, key limits,
/// region shape, format resolution and option coherence.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::S3 {
        return Err(DataError::Schema(format!(
            "S3 validation was handed a {} config",
            config.kind.as_str()
        )));
    }
    super::validate_config(config)?;
    resolve_object(config).map(|_| ())
}

fn schema_err(msg: impl Into<String>) -> DataError {
    DataError::Schema(msg.into())
}

fn non_empty(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

/// Parse the config into an [`S3Object`], surfacing every malformed shape as a
/// [`DataError::Schema`].
fn resolve_object(config: &SourceConfig) -> Result<S3Object> {
    let endpoint = config.endpoint.trim();

    let (bucket, key, service_endpoint) = if let Some(rest) = endpoint.strip_prefix("s3://") {
        // `s3://bucket/key/parts`. The key may instead come from the option.
        let (b, path) = match rest.split_once('/') {
            Some((b, p)) => (b, p),
            None => (rest, ""),
        };
        let key = match (non_empty(Some(path)), non_empty(config.option("key"))) {
            (Some(a), Some(b)) if a != b => {
                return Err(schema_err(format!(
                    "S3 endpoint names key '{a}' but the 'key' option says '{b}' — set exactly one"
                )))
            }
            (Some(a), _) => a.to_string(),
            (None, Some(b)) => b.to_string(),
            (None, None) => {
                return Err(schema_err(
                    "S3 source names no object — write s3://bucket/path/to/object.csv, or set \
                     the 'key' option",
                ))
            }
        };
        (b.to_string(), key, None)
    } else if endpoint.starts_with("https://") || endpoint.starts_with("http://") {
        // A custom service endpoint (MinIO / R2 / Spaces). The endpoint names
        // the *service*, so bucket and key must be explicit; guessing which
        // path segment is the bucket would silently read the wrong object.
        let b = non_empty(config.option("bucket")).ok_or_else(|| {
            schema_err(format!(
                "S3 endpoint '{endpoint}' is a service endpoint, so it requires the 'bucket' \
                 option (or use the s3://bucket/key form)"
            ))
        })?;
        let k = non_empty(config.option("key")).ok_or_else(|| {
            schema_err(format!(
                "S3 endpoint '{endpoint}' is a service endpoint, so it requires the 'key' option"
            ))
        })?;
        if endpoint.trim_end_matches('/').matches('/').count() > 2 {
            return Err(schema_err(format!(
                "S3 service endpoint '{endpoint}' must be a bare scheme://host[:port], with the \
                 bucket and key in their own options"
            )));
        }
        (b.to_string(), k.to_string(), Some(endpoint.trim_end_matches('/').to_string()))
    } else {
        return Err(schema_err(format!(
            "S3 endpoint '{endpoint}' must be s3://bucket/key or a https:// service endpoint"
        )));
    };

    validate_bucket(&bucket)?;
    let key = validate_key(&key)?;
    let region = resolve_region(config, service_endpoint.is_some())?;
    let format = resolve_format(config, &key)?;
    let max_bytes = resolve_max_bytes(config)?;
    let anonymous = resolve_bool(config, "anonymous")?.unwrap_or(false);

    if !anonymous && config.secret_ref.is_none() && non_empty(config.option("access_key_id")).is_some()
    {
        return Err(schema_err(
            "S3 source sets 'access_key_id' but no secret_ref — name the environment variable \
             holding the secret access key, so the secret never reaches disk",
        ));
    }

    Ok(S3Object { bucket, key, region, endpoint: service_endpoint, format, max_bytes, anonymous })
}

/// The AWS general-purpose bucket naming rules, in full.
fn validate_bucket(bucket: &str) -> Result<()> {
    let bad = |why: &str| schema_err(format!("S3 bucket '{bucket}' {why}"));

    if bucket.len() < 3 || bucket.len() > 63 {
        return Err(bad("must be 3 to 63 characters long"));
    }
    if !bucket
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
    {
        return Err(bad("may contain only lowercase letters, digits, hyphens and dots"));
    }
    let first = bucket.chars().next().unwrap_or('-');
    let last = bucket.chars().next_back().unwrap_or('-');
    if !(first.is_ascii_lowercase() || first.is_ascii_digit())
        || !(last.is_ascii_lowercase() || last.is_ascii_digit())
    {
        return Err(bad("must begin and end with a letter or a digit"));
    }
    if bucket.contains("..") {
        return Err(bad("must not contain two adjacent dots"));
    }
    if is_ipv4_literal(bucket) {
        return Err(bad("must not be formatted as an IP address"));
    }
    for prefix in ["xn--", "sthree-", "amzn-s3-demo-"] {
        if bucket.starts_with(prefix) {
            return Err(bad(&format!("must not start with '{prefix}' (reserved by AWS)")));
        }
    }
    for suffix in ["-s3alias", "--ol-s3", "--x-s3"] {
        if bucket.ends_with(suffix) {
            return Err(bad(&format!("must not end with '{suffix}' (reserved by AWS)")));
        }
    }
    Ok(())
}

fn is_ipv4_literal(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// Normalize and check an object key.
fn validate_key(key: &str) -> Result<String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(schema_err("S3 object key is empty"));
    }
    if key.starts_with('/') {
        return Err(schema_err(format!(
            "S3 object key '{key}' must not start with '/' — a key is not a filesystem path"
        )));
    }
    if key.ends_with('/') {
        return Err(schema_err(format!(
            "S3 object key '{key}' names a prefix, not an object"
        )));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(schema_err(format!(
            "S3 object key is {} bytes; S3 allows at most {MAX_KEY_BYTES}",
            key.len()
        )));
    }
    if key.chars().any(|c| c.is_control()) {
        return Err(schema_err("S3 object key contains a control character"));
    }
    if key.contains('?') || key.contains('#') {
        return Err(schema_err(format!(
            "S3 object key '{key}' contains '?' or '#', which would be read as a URL query or \
             fragment"
        )));
    }
    Ok(key.to_string())
}

fn resolve_region(config: &SourceConfig, has_custom_endpoint: bool) -> Result<String> {
    match non_empty(config.option("region")) {
        Some(r) => {
            let lowered = r.to_ascii_lowercase();
            let shaped = lowered.len() >= 2
                && lowered.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                && !lowered.starts_with('-')
                && !lowered.ends_with('-');
            if !shaped {
                return Err(schema_err(format!(
                    "S3 region '{r}' is not a region code (expected e.g. us-east-1)"
                )));
            }
            Ok(lowered)
        }
        // AWS proper needs an explicit region: there is no runtime credential
        // chain here to fall back on, and guessing would sign for the wrong host.
        None if !has_custom_endpoint => Err(schema_err(
            "S3 source requires the 'region' option (e.g. us-east-1) — this build has no AWS \
             credential chain to infer one from",
        )),
        None => Ok(DEFAULT_CUSTOM_REGION.to_string()),
    }
}

fn resolve_format(config: &SourceConfig, key: &str) -> Result<ObjectFormat> {
    match non_empty(config.option("format")) {
        Some(f) => ObjectFormat::parse(f).ok_or_else(|| {
            schema_err(format!(
                "S3 'format' option '{f}' is not one of {}",
                ObjectFormat::TOKENS
            ))
        }),
        None => ObjectFormat::from_key(key).ok_or_else(|| {
            schema_err(format!(
                "S3 cannot infer a format from key '{key}' — set the 'format' option to one of {}",
                ObjectFormat::TOKENS
            ))
        }),
    }
}

fn resolve_max_bytes(config: &SourceConfig) -> Result<Option<u64>> {
    let Some(raw) = non_empty(config.option("max_bytes")) else {
        return Ok(None);
    };
    let n: u64 = raw
        .parse()
        .map_err(|_| schema_err(format!("S3 'max_bytes' option '{raw}' is not a whole number")))?;
    if n == 0 {
        return Err(schema_err("S3 'max_bytes' option must be greater than zero"));
    }
    Ok(Some(n))
}

fn resolve_bool(config: &SourceConfig, name: &str) -> Result<Option<bool>> {
    let Some(raw) = non_empty(config.option(name)) else {
        return Ok(None);
    };
    match raw.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Ok(Some(true)),
        "false" | "no" | "0" => Ok(Some(false)),
        _ => Err(schema_err(format!(
            "S3 '{name}' option '{raw}' is not a boolean (true or false)"
        ))),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> SourceConfig {
        SourceConfig::new(SourceKind::S3, "s3://telemetry-archive/2026/08/readings.csv")
            .with_option("region", "us-west-2")
    }

    fn err_of(config: &SourceConfig) -> String {
        validate(config).expect_err("expected a validation error").to_string()
    }

    // ── Happy paths ──────────────────────────────────────────────────────────

    #[test]
    fn an_s3_uri_resolves_bucket_key_region_and_format() {
        let src = S3Source::new(base()).unwrap();
        let o = src.object();
        assert_eq!(o.bucket, "telemetry-archive");
        assert_eq!(o.key, "2026/08/readings.csv");
        assert_eq!(o.region, "us-west-2");
        assert_eq!(o.format, ObjectFormat::Csv);
        assert_eq!(o.endpoint, None);
        assert!(!o.anonymous);
        assert_eq!(
            o.https_url(),
            "https://telemetry-archive.s3.us-west-2.amazonaws.com/2026/08/readings.csv"
        );
        assert_eq!(src.kind(), SourceKind::S3);
    }

    #[test]
    fn the_key_may_come_from_the_option_instead_of_the_uri() {
        let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive")
            .with_option("region", "eu-central-1")
            .with_option("key", "rollups/daily.jsonl");
        let o = S3Source::new(c).unwrap().object().clone();
        assert_eq!(o.key, "rollups/daily.jsonl");
        assert_eq!(o.format, ObjectFormat::Jsonl);
    }

    #[test]
    fn a_custom_service_endpoint_uses_path_style_and_defaults_its_region() {
        let c = SourceConfig::new(SourceKind::S3, "https://minio.internal:9000")
            .with_option("bucket", "game-assets")
            .with_option("key", "runs/latest.parquet");
        let o = S3Source::new(c).unwrap().object().clone();
        assert_eq!(o.region, DEFAULT_CUSTOM_REGION, "a custom endpoint may omit the region");
        assert_eq!(o.format, ObjectFormat::Parquet);
        assert_eq!(o.https_url(), "https://minio.internal:9000/game-assets/runs/latest.parquet");
    }

    #[test]
    fn every_format_token_round_trips() {
        for f in
            [ObjectFormat::Csv, ObjectFormat::Json, ObjectFormat::Jsonl, ObjectFormat::Parquet]
        {
            assert_eq!(ObjectFormat::parse(f.as_str()), Some(f), "{f:?}");
        }
        assert_eq!(ObjectFormat::parse("NDJSON"), Some(ObjectFormat::Jsonl));
        assert_eq!(ObjectFormat::parse("avro"), None);
    }

    #[test]
    fn an_explicit_format_overrides_the_extension() {
        let c = base().with_option("format", "jsonl");
        assert_eq!(S3Source::new(c).unwrap().object().format, ObjectFormat::Jsonl);
    }

    #[test]
    fn an_extensionless_key_is_fine_when_the_format_is_explicit() {
        let c = SourceConfig::new(SourceKind::S3, "s3://bucket-name/exports/latest")
            .with_option("region", "us-east-1")
            .with_option("format", "csv");
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn max_bytes_and_anonymous_are_parsed() {
        let c = base().with_option("max_bytes", "1048576").with_option("anonymous", "TRUE");
        let o = S3Source::new(c).unwrap().object().clone();
        assert_eq!(o.max_bytes, Some(1_048_576));
        assert!(o.anonymous);
    }

    #[test]
    fn a_dotted_bucket_that_is_not_an_ip_is_accepted() {
        let c = SourceConfig::new(SourceKind::S3, "s3://logs.example.com/a.csv")
            .with_option("region", "us-east-1");
        assert!(validate(&c).is_ok());
    }

    // ── Rejected shapes ──────────────────────────────────────────────────────

    #[test]
    fn a_non_s3_config_is_refused_by_name() {
        let c = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x");
        assert!(err_of(&c).contains("REST"));
    }

    #[test]
    fn an_unrecognized_scheme_is_rejected() {
        let c = SourceConfig::new(SourceKind::S3, "gs://bucket/key.csv")
            .with_option("region", "us-east-1");
        assert!(validate(&c).is_err());
    }

    #[test]
    fn an_s3_uri_with_no_object_is_rejected() {
        let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive")
            .with_option("region", "us-east-1");
        assert!(err_of(&c).contains("names no object"));
    }

    #[test]
    fn a_uri_key_and_option_key_that_disagree_are_rejected() {
        let c = base().with_option("key", "somewhere/else.csv");
        assert!(err_of(&c).contains("exactly one"));
    }

    #[test]
    fn a_service_endpoint_without_bucket_or_key_is_rejected() {
        let bare = SourceConfig::new(SourceKind::S3, "https://minio.internal:9000");
        assert!(err_of(&bare).contains("'bucket' option"));
        let with_bucket = bare.with_option("bucket", "assets");
        assert!(err_of(&with_bucket).contains("'key' option"));
    }

    #[test]
    fn a_service_endpoint_carrying_a_path_is_rejected() {
        let c = SourceConfig::new(SourceKind::S3, "https://minio.internal:9000/assets/x.csv")
            .with_option("bucket", "assets")
            .with_option("key", "x.csv");
        assert!(err_of(&c).contains("bare scheme://host"));
    }

    #[test]
    fn every_bucket_naming_rule_is_enforced() {
        let too_long = "a".repeat(64);
        let cases: [(&str, &str); 9] = [
            ("ab", "too short"),
            (too_long.as_str(), "too long"),
            ("Telemetry", "uppercase"),
            ("bucket_name", "underscore"),
            ("-bucket", "leading hyphen"),
            ("bucket-", "trailing hyphen"),
            ("a..b", "adjacent dots"),
            ("192.168.0.1", "ip literal"),
            ("xn--bucket", "reserved prefix"),
        ];
        for (bucket, why) in cases {
            let c = SourceConfig::new(SourceKind::S3, format!("s3://{bucket}/a.csv"))
                .with_option("region", "us-east-1");
            assert!(validate(&c).is_err(), "bucket '{bucket}' ({why}) accepted");
        }
    }

    #[test]
    fn a_reserved_bucket_suffix_is_rejected() {
        for bucket in ["my-bucket-s3alias", "my-bucket--ol-s3", "my-bucket--x-s3"] {
            let c = SourceConfig::new(SourceKind::S3, format!("s3://{bucket}/a.csv"))
                .with_option("region", "us-east-1");
            assert!(validate(&c).is_err(), "bucket '{bucket}' accepted");
        }
    }

    #[test]
    fn a_key_that_is_a_prefix_or_path_is_rejected() {
        for key in ["/leading/slash.csv", "trailing/prefix/", "with?query.csv", "with#frag.csv"] {
            let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive")
                .with_option("region", "us-east-1")
                .with_option("key", key);
            assert!(validate(&c).is_err(), "key '{key}' accepted");
        }
    }

    #[test]
    fn an_over_long_key_is_rejected() {
        let key = format!("{}.csv", "k".repeat(MAX_KEY_BYTES));
        let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive")
            .with_option("region", "us-east-1")
            .with_option("key", key);
        assert!(err_of(&c).contains("at most 1024"));
    }

    #[test]
    fn aws_proper_requires_an_explicit_region() {
        let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive/a.csv");
        assert!(err_of(&c).contains("'region' option"));
    }

    #[test]
    fn a_malformed_region_is_rejected() {
        for region in ["US East 1", "-us-east-1", "us-east-1-", "u", "us_east_1"] {
            let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive/a.csv")
                .with_option("region", region);
            assert!(validate(&c).is_err(), "region '{region}' accepted");
        }
    }

    #[test]
    fn an_unknown_format_is_rejected_and_lists_the_known_ones() {
        let c = base().with_option("format", "avro");
        let msg = err_of(&c);
        assert!(msg.contains("csv, json, jsonl, parquet"), "{msg}");
    }

    #[test]
    fn a_format_that_cannot_be_inferred_is_rejected() {
        let c = SourceConfig::new(SourceKind::S3, "s3://telemetry-archive/exports/latest")
            .with_option("region", "us-east-1");
        assert!(err_of(&c).contains("cannot infer a format"));
    }

    #[test]
    fn a_bad_max_bytes_is_rejected() {
        for v in ["0", "-1", "some"] {
            let c = base().with_option("max_bytes", v);
            assert!(validate(&c).is_err(), "max_bytes '{v}' accepted");
        }
    }

    #[test]
    fn a_non_boolean_anonymous_is_rejected() {
        let c = base().with_option("anonymous", "maybe");
        assert!(err_of(&c).contains("not a boolean"));
    }

    #[test]
    fn a_key_id_without_a_secret_ref_is_rejected() {
        let c = base().with_option("access_key_id", "AKIAIOSFODNN7EXAMPLE");
        assert!(err_of(&c).contains("secret_ref"));
    }

    #[test]
    fn a_key_id_with_a_secret_ref_is_accepted() {
        let c = base()
            .with_option("access_key_id", "AKIAIOSFODNN7EXAMPLE")
            .with_secret_ref("AWS_SECRET_ACCESS_KEY");
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn an_anonymous_source_needs_no_credentials() {
        let c = base()
            .with_option("anonymous", "true")
            .with_option("access_key_id", "AKIAIOSFODNN7EXAMPLE");
        assert!(validate(&c).is_ok());
    }

    #[test]
    fn the_shared_checks_still_apply() {
        let mut c = base();
        c.poll_seconds = Some(0);
        assert!(validate(&c).is_err());

        let c = base().with_secret_ref("AKIA REAL SECRET");
        assert!(validate(&c).is_err());
    }

    // ── The not-compiled-in surface ──────────────────────────────────────────

    #[test]
    fn test_connection_names_the_provider_and_the_missing_feature() {
        let src = S3Source::new(base()).unwrap();
        let err = src.test_connection().expect_err("no client is linked");
        let msg = err.to_string();
        assert!(msg.contains("S3"), "must name the provider: {msg}");
        assert!(msg.contains("'s3' feature"), "must name the feature: {msg}");
        assert!(msg.contains("not compiled in"), "must say it is absent: {msg}");
        assert!(
            matches!(&err, DataError::Io(e) if e.kind() == std::io::ErrorKind::Unsupported),
            "a missing capability must not masquerade as a config error"
        );
    }

    #[test]
    fn fetch_explains_sigv4_and_offers_the_public_url_route() {
        let src = S3Source::new(base()).unwrap();
        let msg = src.fetch().expect_err("no client is linked").to_string();
        assert!(msg.contains("SigV4"), "the real blocker must be named: {msg}");
        assert!(msg.contains("REST source"), "the workable alternative must be offered: {msg}");
        assert!(msg.contains(&src.object().https_url()), "{msg}");
    }

    #[test]
    fn the_secret_key_is_read_from_the_environment_at_call_time() {
        let key = "EUSTRESS_TEST_S3_SECRET_UNSET";
        std::env::remove_var(key);
        let src = S3Source::new(base().with_secret_ref(key)).unwrap();
        assert_eq!(src.secret_access_key(), None);
    }
}
