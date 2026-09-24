//! # `eustress commerce`: microtransactions in published simulations
//!
//! A client for the Commerce API (`infrastructure/cloudflare/api/src/commerce.mjs`),
//! shaped like the Stripe CLI so its habits carry over:
//!
//! ```text
//! eustress commerce login                      # a test key from your Studio sign-in
//! eustress commerce products create --name "100 Coins" --price 50 --space Lobby
//! eustress commerce listen --forward-to localhost:4242/hooks
//! eustress commerce trigger purchase.succeeded
//! ```
//!
//! Everything runs in test mode unless `--live` is given. Test purchases move
//! no Tickets, and test mode can shape draft products but never change what
//! players see or pay. A product belongs to a published Universe and is sold
//! in one of its published Spaces.
//!
//! ## Credentials
//! `login` mints a commerce API key (`ek_test_...`, and `ek_live_...` with
//! `--live`) from the Studio sign-in at `<local data>/EustressEngine/auth_token`
//! (or `EUSTRESS_TOKEN`) and stores it in `<config>/eustress/cli.toml`.
//! `--api-key` (or `EUSTRESS_API_KEY`) uses a key for one command.
//! `EUSTRESS_API_URL` points at another API, such as `wrangler dev` on
//! `http://127.0.0.1:8787`. Live mode always needs a live key: the Studio
//! sign-in is used only for test mode and for managing keys.
//!
//! ## Which simulation
//! `--sim <id>`, else `EUSTRESS_SIM_ID`, else the published id of the Universe
//! the current folder is in (`<Universe>/.eustress/sync.toml`,
//! `remote.experience_id`). Inside `<Universe>/Spaces/<Name>/`, `<Name>` is
//! the default `--space`.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use hmac::{Hmac, Mac};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;

const DEFAULT_API_URL: &str = "https://api.eustress.dev";

/// Event types the API publishes (`EVENT_TYPES` in commerce.mjs).
const EVENT_TYPES: [&str; 7] = [
    "product.created",
    "product.updated",
    "product.archived",
    "purchase.succeeded",
    "purchase.failed",
    "purchase.fulfilled",
    "purchase.refunded",
];

/// Event types `trigger` can fire (`TRIGGERS` in commerce.mjs).
const TRIGGERS: [&str; 4] = [
    "purchase.succeeded",
    "purchase.failed",
    "purchase.fulfilled",
    "purchase.refunded",
];

/// How long `listen` asks the API to hold each poll open: the API's maximum.
const LISTEN_WAIT_S: u32 = 25;

const USER_AGENT: &str = concat!("eustress-cli/", env!("CARGO_PKG_VERSION"));

const NO_SESSION: &str = "Not signed in: sign in to Eustress Studio first (or set EUSTRESS_TOKEN)";

// ─────────────────────────────────────────────────────────────────────────────
// Command tree
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Args, Debug)]
pub struct CommerceArgs {
    /// Act on live data and real Tickets. Without it, everything runs in test mode.
    #[arg(long, global = true)]
    live: bool,

    /// Print the API's JSON instead of a summary.
    #[arg(long, global = true)]
    json: bool,

    /// Use this API key (ek_test_... or ek_live_...) instead of the stored one.
    #[arg(long, global = true, env = "EUSTRESS_API_KEY", hide_env_values = true)]
    api_key: Option<String>,

    /// The Commerce API to talk to (default https://api.eustress.dev).
    #[arg(long, global = true, env = "EUSTRESS_API_URL")]
    api_url: Option<String>,

    #[command(subcommand)]
    command: CommerceCommand,
}

#[derive(Subcommand, Debug)]
enum CommerceCommand {
    /// Connect the CLI to your account: mints a test API key from your Studio
    /// sign-in (and a live one with --live), or stores the key given with --api-key.
    Login {
        /// The name the key carries in your account's key list.
        #[arg(long)]
        name: Option<String>,
    },
    /// Remove the stored keys, revoking them when you are signed in to Studio.
    Logout,
    /// Show the account, the mode, and your published simulations with their Spaces.
    Whoami,
    /// Your own Ticket balance.
    Balance,
    /// What players see: a simulation's products on sale (drafts too, for its creator).
    Catalog {
        #[arg(long)]
        sim: Option<String>,
    },
    /// Products sold in your published simulations.
    Products {
        #[command(subcommand)]
        action: ProductsCommand,
    },
    /// Purchases of your products.
    Purchases {
        #[command(subcommand)]
        action: PurchasesCommand,
    },
    /// Everything that happened, newest first.
    Events {
        #[command(subcommand)]
        action: EventsCommand,
    },
    /// Stream events as they happen, and forward them, signed, to a local endpoint.
    Listen(ListenArgs),
    /// Fire an event with a made-up buyer (test mode only; nothing moves).
    Trigger(TriggerArgs),
    /// Endpoints the API sends events to.
    Webhooks {
        #[command(subcommand)]
        action: WebhooksCommand,
    },
    /// Your account's API keys (needs your Studio sign-in).
    Keys {
        #[command(subcommand)]
        action: KeysCommand,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ProductType {
    /// Granted each time it is bought, like coins or a boost.
    Consumable,
    /// Owned once, like a VIP pass.
    Pass,
}

impl ProductType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Consumable => "consumable",
            Self::Pass => "pass",
        }
    }
}

#[derive(Subcommand, Debug)]
enum ProductsCommand {
    /// List products, newest first.
    List {
        /// Only this simulation's products.
        #[arg(long)]
        sim: Option<String>,
        /// Only products on sale (true) or drafts (false).
        #[arg(long)]
        active: Option<bool>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// The last id of the previous page.
        #[arg(long)]
        starting_after: Option<String>,
    },
    /// Create a product. It starts as a draft you can test in Studio.
    Create {
        #[arg(long)]
        name: String,
        /// Price in whole Tickets.
        #[arg(long)]
        price: u64,
        #[arg(long = "type", value_enum, default_value_t = ProductType::Consumable)]
        kind: ProductType,
        /// The published Space it is sold in (default: the Space the current folder is in).
        #[arg(long)]
        space: Option<String>,
        #[arg(long)]
        sim: Option<String>,
        #[arg(long)]
        description: Option<String>,
        /// Icon URL served by Eustress.
        #[arg(long)]
        icon: Option<String>,
        /// Put it on sale at once (a live change: needs --live).
        #[arg(long)]
        active: bool,
        /// Metadata as key=value; repeatable.
        #[arg(long = "metadata", short = 'm', value_parser = parse_key_value)]
        metadata: Vec<(String, String)>,
    },
    /// Show a product: its prod_ id, or its number with --sim.
    Get {
        product: String,
        #[arg(long)]
        sim: Option<String>,
    },
    /// Change a product. Changing one on sale is a live change (needs --live).
    Update {
        product: String,
        #[arg(long)]
        sim: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        price: Option<u64>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        icon: Option<String>,
        #[arg(long)]
        space: Option<String>,
        /// true puts it on sale, false takes it off.
        #[arg(long)]
        active: Option<bool>,
        /// Metadata as key=value; `key=` removes the key. Repeatable.
        #[arg(long = "metadata", short = 'm', value_parser = parse_key_value)]
        metadata: Vec<(String, String)>,
    },
    /// Take a product off sale. Passes already bought stay owned.
    Archive {
        product: String,
        #[arg(long)]
        sim: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum PurchasesCommand {
    /// Purchases of your products, newest first.
    List {
        #[arg(long)]
        sim: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long)]
        starting_after: Option<String>,
    },
    /// Show a purchase.
    Get { id: String },
    /// Give the buyer their Tickets back and take back your share (within 72 hours).
    Refund {
        id: String,
        /// Skip the question a live refund asks first.
        #[arg(long)]
        yes: bool,
    },
    /// Mark a purchase granted, as your simulation's ProcessReceipt does.
    Fulfill {
        id: String,
        #[arg(long)]
        sim: Option<String>,
    },
    /// Buy one of your own products in test mode. Studio hands the receipt
    /// to your ProcessReceipt the next time you Play.
    Create {
        /// A prod_ id or product number (default: the simulation's first product).
        #[arg(long)]
        product: Option<String>,
        #[arg(long)]
        sim: Option<String>,
        #[arg(long)]
        space: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum EventsCommand {
    /// Events, newest first.
    List {
        /// Only events of this type.
        #[arg(long = "type")]
        kind: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long)]
        starting_after: Option<String>,
    },
    /// Show an event in full.
    Get { id: String },
    /// Send an event to your webhook endpoints again.
    Resend { id: String },
}

#[derive(Args, Debug)]
struct ListenArgs {
    /// Forward each event, signed, to this URL (e.g. localhost:4242/hooks).
    #[arg(long, short = 'f')]
    forward_to: Option<String>,
    /// Only these event types (comma-separated).
    #[arg(long, value_delimiter = ',')]
    events: Vec<String>,
    /// Print the signing secret and exit.
    #[arg(long)]
    print_secret: bool,
    /// Accept an invalid TLS certificate on the forward target (a self-signed local server).
    #[arg(long)]
    skip_verify: bool,
}

#[derive(Args, Debug)]
struct TriggerArgs {
    /// purchase.succeeded, purchase.failed, purchase.fulfilled or purchase.refunded.
    event: String,
    /// A prod_ id or product number (default: the simulation's first product).
    #[arg(long)]
    product: Option<String>,
    #[arg(long)]
    sim: Option<String>,
}

#[derive(Subcommand, Debug)]
enum WebhooksCommand {
    /// Endpoints in the current mode.
    List,
    /// Add an endpoint: a public https URL. Its signing secret is shown once.
    Create {
        #[arg(long)]
        url: String,
        /// Event types to send (comma-separated; default all).
        #[arg(long, value_delimiter = ',')]
        events: Vec<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Remove an endpoint.
    Delete { id: String },
}

#[derive(Subcommand, Debug)]
enum KeysCommand {
    /// Your account's API keys.
    List,
    /// Create a key for the current mode (--live for a live key). The key is shown once.
    Create {
        #[arg(long)]
        name: Option<String>,
        /// commerce, datastore, http or ai.
        #[arg(long = "type", default_value = "commerce")]
        kind: String,
    },
    /// Revoke a key by its key_ id.
    Revoke { id: String },
}

fn parse_key_value(raw: &str) -> Result<(String, String), String> {
    match raw.split_once('=') {
        Some((key, value)) if !key.trim().is_empty() => {
            Ok((key.trim().to_string(), value.to_string()))
        }
        _ => Err(format!("expected key=value, got '{raw}'")),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

pub async fn run(args: CommerceArgs) -> Result<()> {
    let CommerceArgs {
        live,
        json,
        api_key,
        api_url,
        command,
    } = args;
    let opts = Opts {
        live,
        json,
        api_key,
        api_url,
    };
    match command {
        CommerceCommand::Login { name } => login(&opts, name).await,
        CommerceCommand::Logout => logout(&opts).await,
        CommerceCommand::Whoami => whoami(&opts).await,
        CommerceCommand::Balance => balance(&opts).await,
        CommerceCommand::Catalog { sim } => catalog(&opts, sim).await,
        CommerceCommand::Products { action } => products(&opts, action).await,
        CommerceCommand::Purchases { action } => purchases(&opts, action).await,
        CommerceCommand::Events { action } => events(&opts, action).await,
        CommerceCommand::Listen(listen_args) => listen(&opts, listen_args).await,
        CommerceCommand::Trigger(trigger_args) => trigger(&opts, trigger_args).await,
        CommerceCommand::Webhooks { action } => webhooks(&opts, action).await,
        CommerceCommand::Keys { action } => keys(&opts, action).await,
    }
}

struct Opts {
    live: bool,
    json: bool,
    api_key: Option<String>,
    api_url: Option<String>,
}

impl Opts {
    fn mode(&self) -> &'static str {
        mode_name(self.live)
    }

    fn base_url(&self, config: Option<&CommerceConfig>) -> String {
        self.api_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .or_else(|| config.and_then(|c| c.api_url.clone()))
            .unwrap_or_else(|| DEFAULT_API_URL.to_string())
            .trim()
            .trim_end_matches('/')
            .to_string()
    }

    /// The client for commands that act as the account.
    fn api(&self) -> Result<Api> {
        let config = load_config()?;
        let auth = self.auth(config.as_ref())?;
        Api::new(self.base_url(config.as_ref()), auth, self.live)
    }

    /// The client for what only a signed-in person may do: manage API keys.
    fn session_api(&self) -> Result<Api> {
        let config = load_config()?;
        let token = studio_token().ok_or_else(|| anyhow!(NO_SESSION))?;
        Api::new(
            self.base_url(config.as_ref()),
            Auth::Session(token),
            self.live,
        )
    }

    /// A key given for this command, else the stored key for the mode, else
    /// (test mode only) the Studio sign-in.
    fn auth(&self, config: Option<&CommerceConfig>) -> Result<Auth> {
        if let Some(key) = self
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            return match key_is_live(key) {
                None => bail!("--api-key must be a commerce key: ek_test_... or ek_live_..."),
                Some(true) if !self.live => {
                    bail!("That is a live key; add --live to act on live data")
                }
                Some(false) if self.live => {
                    bail!("That is a test key; drop --live, or pass a live key")
                }
                Some(_) => Ok(Auth::Key(key.to_string())),
            };
        }
        let stored = config.and_then(|c| {
            if self.live {
                c.live_key.clone()
            } else {
                c.test_key.clone()
            }
        });
        if let Some(key) = stored {
            return Ok(Auth::Key(key));
        }
        if self.live {
            bail!("Live mode needs a live key: run `eustress commerce login --live`, or pass --api-key ek_live_...");
        }
        match studio_token() {
            Some(token) => Ok(Auth::Session(token)),
            None => bail!("Not logged in: sign in to Eustress Studio and run `eustress commerce login`, or pass --api-key"),
        }
    }

    /// Print `value` as JSON when --json was given. True when it printed.
    fn print_json(&self, value: &Value) -> bool {
        if self.json {
            println!(
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            );
        }
        self.json
    }
}

fn mode_name(live: bool) -> &'static str {
    if live {
        "live"
    } else {
        "test"
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP
// ─────────────────────────────────────────────────────────────────────────────

enum Auth {
    /// A commerce API key; its prefix decides the mode.
    Key(String),
    /// The Studio sign-in. Every request names its mode in `Eustress-Mode`,
    /// so none falls into a mode by leaving the header out.
    Session(String),
}

struct Api {
    base: String,
    http: reqwest::Client,
    auth: Auth,
    live: bool,
}

impl Api {
    fn new(base: String, auth: Auth, live: bool) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(10))
            // Above the longest the API holds a `listen` poll open.
            .timeout(Duration::from_secs(60))
            .build()
            .context("could not build the HTTP client")?;
        Ok(Self {
            base,
            http,
            auth,
            live,
        })
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let builder = self.http.request(method, format!("{}{}", self.base, path));
        match &self.auth {
            Auth::Key(key) => builder.bearer_auth(key),
            Auth::Session(token) => builder
                .bearer_auth(token)
                .header("Eustress-Mode", mode_name(self.live)),
        }
    }

    async fn send(&self, builder: reqwest::RequestBuilder) -> Result<Value> {
        let response = builder
            .send()
            .await
            .with_context(|| format!("could not reach {}", self.base))?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let body = serde_json::from_str::<Value>(&text)
            .unwrap_or_else(|_| json!({ "error": text.trim() }));
        if status.is_success() {
            Ok(body)
        } else {
            Err(ApiError::new(status.as_u16(), &body, &self.base).into())
        }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.send(self.request(Method::GET, path).query(query))
            .await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        self.send(self.request(Method::POST, path).json(body)).await
    }

    async fn delete(&self, path: &str) -> Result<Value> {
        self.send(self.request(Method::DELETE, path)).await
    }
}

/// An error the API answered with: `{error, code, param?}`.
#[derive(Debug)]
struct ApiError {
    status: u16,
    code: String,
    message: String,
    hint: Option<String>,
}

impl ApiError {
    fn new(status: u16, body: &Value, base: &str) -> Self {
        let code = s(body, "code").to_string();
        let message = match s(body, "error") {
            "" => format!("request failed with HTTP {status}"),
            text => text.to_string(),
        };
        let hint = match code.as_str() {
            "commerce_unavailable" => Some(format!("the Commerce API is not live at {base} yet")),
            "invalid_api_key" => Some("run `eustress commerce login` again".to_string()),
            "invalid_session" | "unauthenticated" => {
                Some("sign in to Eustress Studio again, then run `eustress commerce login`".to_string())
            }
            "livemode_required" => Some("add --live (with a key from `eustress commerce login --live`)".to_string()),
            "simulation_not_found" => Some(
                "publish the Universe from Studio first; `eustress commerce whoami` lists your published simulations"
                    .to_string(),
            ),
            _ => None,
        };
        Self {
            status,
            code,
            message,
            hint,
        }
    }

    fn is_auth(&self) -> bool {
        self.status == 401 || self.status == 403
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if self.code.is_empty() {
            write!(f, " [{}]", self.status)?;
        } else {
            write!(f, " [{} {}]", self.status, self.code)?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n  hint: {hint}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

// ─────────────────────────────────────────────────────────────────────────────
// Stored keys, the Studio sign-in, and the Universe under the current folder
// ─────────────────────────────────────────────────────────────────────────────

/// The `[commerce]` section of `<config>/eustress/cli.toml`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct CommerceConfig {
    account_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    test_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    test_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    live_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    live_key_id: Option<String>,
}

fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| anyhow!("this system has no config directory"))?;
    Ok(dir.join("eustress").join("cli.toml"))
}

fn load_table() -> Result<toml::Table> {
    let path = config_path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("{} is not valid TOML", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Table::new()),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

fn load_config() -> Result<Option<CommerceConfig>> {
    match load_table()?.remove("commerce") {
        Some(section) => {
            Ok(Some(section.try_into().context(
                "the [commerce] section of cli.toml is not valid",
            )?))
        }
        None => Ok(None),
    }
}

/// Write the `[commerce]` section, or remove it for `None`, keeping every
/// other section of the file.
fn save_config(config: Option<&CommerceConfig>) -> Result<PathBuf> {
    let path = config_path()?;
    let mut table = load_table()?;
    match config {
        Some(c) => {
            table.insert("commerce".to_string(), toml::Value::try_from(c)?);
        }
        None => {
            table.remove("commerce");
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    std::fs::write(&path, toml::to_string_pretty(&table)?)
        .with_context(|| format!("could not write {}", path.display()))?;
    // The file holds API keys.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(path)
}

/// The Studio sign-in: `EUSTRESS_TOKEN`, else the token Studio saved.
fn studio_token() -> Option<String> {
    if let Ok(token) = std::env::var("EUSTRESS_TOKEN") {
        if !token.trim().is_empty() {
            return Some(token.trim().to_string());
        }
    }
    let path = dirs::data_local_dir()?
        .join("EustressEngine")
        .join("auth_token");
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// The published simulation and Space of the folder the command runs in.
#[derive(Default)]
struct Here {
    sim_id: Option<String>,
    space: Option<String>,
}

impl Here {
    fn detect() -> Self {
        let mut here = Here::default();
        let Ok(cwd) = std::env::current_dir() else {
            return here;
        };
        for dir in cwd.ancestors() {
            if here.space.is_none()
                && dir
                    .parent()
                    .and_then(Path::file_name)
                    .map_or(false, |p| p == "Spaces")
            {
                here.space = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            }
            // A Space's own .eustress/sync.toml holds no published id; the
            // walk goes on up to the Universe's.
            if let Some(id) = read_experience_id(&dir.join(".eustress").join("sync.toml")) {
                here.sim_id = Some(id);
                break;
            }
        }
        here
    }
}

fn read_experience_id(sync_toml: &Path) -> Option<String> {
    let text = std::fs::read_to_string(sync_toml).ok()?;
    let table: toml::Table = toml::from_str(&text).ok()?;
    let id = table.get("remote")?.get("experience_id")?.as_str()?.trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn resolve_sim(explicit: Option<String>, here: Option<&Here>) -> Result<String> {
    if let Some(id) = explicit
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        return Ok(id);
    }
    if let Ok(id) = std::env::var("EUSTRESS_SIM_ID") {
        if !id.trim().is_empty() {
            return Ok(id.trim().to_string());
        }
    }
    let detected = match here {
        Some(h) => h.sim_id.clone(),
        None => Here::detect().sim_id,
    };
    detected.ok_or_else(|| {
        anyhow!(
            "Which simulation? Pass --sim <id> (`eustress commerce whoami` lists yours), set EUSTRESS_SIM_ID, \
             or run this inside a published Universe's folder"
        )
    })
}

/// A product reference as the API takes it: a prod_ id, or a product number.
fn product_ref_json(reference: &str) -> Value {
    let reference = reference.trim();
    match reference.parse::<u64>() {
        Ok(number) => json!(number),
        Err(_) => json!(reference),
    }
}

/// The prod_ id `reference` names: itself, or the product with that number in the simulation.
async fn resolve_product_id(api: &Api, reference: &str, sim: Option<String>) -> Result<String> {
    let reference = reference.trim();
    if reference.starts_with("prod_") {
        return Ok(reference.to_string());
    }
    let number: u64 = reference
        .parse()
        .map_err(|_| anyhow!("'{reference}' is neither a prod_ id nor a product number"))?;
    let sim = resolve_sim(sim, None)?;
    let catalog = api
        .get(&format!("/api/commerce/catalog/{sim}"), &[])
        .await?;
    items(&catalog)
        .iter()
        .find(|p| p["number"].as_u64() == Some(number))
        .map(|p| s(p, "id").to_string())
        .ok_or_else(|| anyhow!("simulation {sim} has no product #{number}"))
}

/// The number of the simulation's first product, for commands that default to one.
async fn first_product(api: &Api, sim: &str) -> Result<String> {
    let catalog = api
        .get(&format!("/api/commerce/catalog/{sim}"), &[])
        .await?;
    items(&catalog)
        .iter()
        .filter_map(|p| p["number"].as_u64())
        .min()
        .map(|n| n.to_string())
        .ok_or_else(|| anyhow!("simulation {sim} has no products yet: create one with `eustress commerce products create`"))
}

// ─────────────────────────────────────────────────────────────────────────────
// login / logout / whoami / balance / catalog
// ─────────────────────────────────────────────────────────────────────────────

async fn login(opts: &Opts, name: Option<String>) -> Result<()> {
    let existing = load_config()?;
    let base = opts.base_url(existing.as_ref());
    let api_url = (base != DEFAULT_API_URL).then(|| base.clone());

    // A key made elsewhere (the web dashboard): check it, then store it.
    if let Some(key) = opts
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty())
    {
        let live = key_is_live(key).ok_or_else(|| {
            anyhow!("--api-key must be a commerce key: ek_test_... or ek_live_...")
        })?;
        let api = Api::new(base.clone(), Auth::Key(key.to_string()), live)?;
        let account = api.get("/api/commerce/account", &[]).await?;
        let account_id = s(&account, "id").to_string();
        let mut config = existing
            .filter(|c| c.account_id == account_id)
            .unwrap_or_default();
        config.account_id = account_id;
        config.api_url = api_url;
        let key_id = account
            .get("key_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        if live {
            config.live_key = Some(key.to_string());
            config.live_key_id = key_id;
        } else {
            config.test_key = Some(key.to_string());
            config.test_key_id = key_id;
        }
        let path = save_config(Some(&config))?;
        println!(
            "{} Stored {} key {} for account {} in {}",
            "✓".green(),
            mode_name(live),
            mask_key(key),
            config.account_id.cyan(),
            path.display()
        );
        return print_simulations(&account);
    }

    let token = studio_token().ok_or_else(|| anyhow!(NO_SESSION))?;
    let session = Api::new(base.clone(), Auth::Session(token), false)?;
    let account = session.get("/api/commerce/account", &[]).await?;
    let account_id = s(&account, "id").to_string();
    if account_id.is_empty() {
        bail!("the API did not say which account this is");
    }
    // Keys stored for another account are that account's; start over.
    let mut config = existing
        .filter(|c| c.account_id == account_id)
        .unwrap_or_default();
    config.account_id = account_id.clone();
    config.api_url = api_url;

    let label = name.unwrap_or_else(|| format!("Eustress CLI ({})", machine_name()));
    let modes: &[bool] = if opts.live { &[false, true] } else { &[false] };
    for &live in modes {
        let created = session
            .post(
                "/api/keys",
                &json!({ "name": label, "mode": mode_name(live), "key_type": "commerce" }),
            )
            .await?;
        let key = s(&created, "key").to_string();
        let id = s(&created, "id").to_string();
        if key.is_empty() || id.is_empty() {
            bail!("the API did not return the new key");
        }
        let (slot, slot_id) = if live {
            (&mut config.live_key, &mut config.live_key_id)
        } else {
            (&mut config.test_key, &mut config.test_key_id)
        };
        let previous = slot_id.replace(id);
        *slot = Some(key.clone());
        // Saved before anything else can fail, so a key the API made is never lost.
        save_config(Some(&config))?;
        println!(
            "{} Created {} key {} ({})",
            "✓".green(),
            mode_name(live),
            mask_key(&key),
            label
        );
        // The key this machine held for the mode is replaced; revoke it so keys do not pile up.
        if let Some(old) = previous {
            if let Err(e) = session.delete(&format!("/api/keys/{old}")).await {
                eprintln!(
                    "{} Could not revoke the previous {} key {old}: {e}",
                    "⚠".yellow(),
                    mode_name(live)
                );
            }
        }
    }
    let path = config_path()?;
    println!(
        "{} Logged in to account {}. Keys stored in {}",
        "✓".green(),
        account_id.cyan(),
        path.display()
    );
    if !opts.live {
        println!("  Test mode only. `eustress commerce login --live` adds a live key.");
    }
    print_simulations(&account)
}

async fn logout(opts: &Opts) -> Result<()> {
    let Some(config) = load_config()? else {
        println!("Not logged in.");
        return Ok(());
    };
    let ids: Vec<String> = [config.test_key_id.clone(), config.live_key_id.clone()]
        .into_iter()
        .flatten()
        .collect();
    if !ids.is_empty() {
        match studio_token() {
            Some(token) => {
                let session = Api::new(opts.base_url(Some(&config)), Auth::Session(token), false)?;
                for id in &ids {
                    match session.delete(&format!("/api/keys/{id}")).await {
                        Ok(_) => println!("{} Revoked {id}", "✓".green()),
                        Err(e) => eprintln!("{} Could not revoke {id}: {e}", "⚠".yellow()),
                    }
                }
            }
            None => eprintln!(
                "{} Not signed in to Studio, so the keys stay valid: revoke {} from your account's API keys.",
                "⚠".yellow(),
                ids.join(", ")
            ),
        }
    }
    let path = save_config(None)?;
    println!(
        "{} Logged out; the keys are gone from {}",
        "✓".green(),
        path.display()
    );
    Ok(())
}

async fn whoami(opts: &Opts) -> Result<()> {
    let api = opts.api()?;
    let account = api.get("/api/commerce/account", &[]).await?;
    if opts.print_json(&account) {
        return Ok(());
    }
    let via = match s(&account, "via") {
        "key" => format!("API key {}", s(&account, "key_id")),
        _ => "Studio sign-in".to_string(),
    };
    println!(
        "Account {}  |  {}  |  {}  |  {}",
        s(&account, "id").cyan(),
        mode_badge(account["livemode"].as_bool() == Some(true)),
        via,
        api.base
    );
    print_simulations(&account)
}

fn print_simulations(account: &Value) -> Result<()> {
    let sims = items_at(account, "simulations");
    if sims.is_empty() {
        println!("  No published simulations yet. Publish a Universe from Studio, then add products with `eustress commerce products create`.");
        return Ok(());
    }
    println!();
    println!("{}", "Your published simulations".bold());
    for sim in &sims {
        let state = if sim["can_sell"].as_bool() == Some(true) {
            if sim["listed"].as_bool() == Some(true) {
                "selling".green().to_string()
            } else {
                "testable; players can buy once it is approved"
                    .yellow()
                    .to_string()
            }
        } else {
            format!("cannot sell: {}", s(sim, "reason"))
                .red()
                .to_string()
        };
        let spaces = strings_at(sim, "spaces");
        println!(
            "  {}  {}  {} product(s)  {}",
            s(sim, "id").dimmed(),
            s(sim, "name").bold(),
            sim["products"].as_i64().unwrap_or(0),
            state
        );
        println!(
            "      Spaces: {}",
            if spaces.is_empty() {
                "none recorded yet (republish the Universe)".to_string()
            } else {
                spaces.join(", ")
            }
        );
    }
    Ok(())
}

async fn balance(opts: &Opts) -> Result<()> {
    let api = opts.api()?;
    let reply = api.get("/api/commerce/me/balance", &[]).await?;
    if opts.print_json(&reply) {
        return Ok(());
    }
    println!(
        "{} Tickets",
        reply["tickets"].as_i64().unwrap_or(0).to_string().bold()
    );
    Ok(())
}

async fn catalog(opts: &Opts, sim: Option<String>) -> Result<()> {
    let api = opts.api()?;
    let sim = resolve_sim(sim, None)?;
    let list = api
        .get(&format!("/api/commerce/catalog/{sim}"), &[])
        .await?;
    if opts.print_json(&list) {
        return Ok(());
    }
    print_list(&list, product_line, "Nothing on sale in this simulation.");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// products
// ─────────────────────────────────────────────────────────────────────────────

async fn products(opts: &Opts, action: ProductsCommand) -> Result<()> {
    let api = opts.api()?;
    match action {
        ProductsCommand::List {
            sim,
            active,
            limit,
            starting_after,
        } => {
            let mut query = vec![("limit", limit.to_string())];
            if let Some(sim) = sim {
                query.push(("sim_id", sim));
            }
            if let Some(active) = active {
                query.push(("active", active.to_string()));
            }
            if let Some(after) = starting_after {
                query.push(("starting_after", after));
            }
            let list = api.get("/api/commerce/products", &query).await?;
            if !opts.print_json(&list) {
                print_list(
                    &list,
                    product_line,
                    "No products yet: `eustress commerce products create --name ... --price ...`",
                );
            }
        }
        ProductsCommand::Create {
            name,
            price,
            kind,
            space,
            sim,
            description,
            icon,
            active,
            metadata,
        } => {
            let here = Here::detect();
            let sim = resolve_sim(sim, Some(&here))?;
            let space = space.or(here.space).ok_or_else(|| {
                anyhow!("Which Space is it sold in? Pass --space <name> (`eustress commerce whoami` lists your published Spaces)")
            })?;
            let mut body = json!({
                "sim_id": sim, "space": space, "name": name, "price": price, "type": kind.as_str(), "active": active,
            });
            if let Some(description) = description {
                body["description"] = json!(description);
            }
            if let Some(icon) = icon {
                body["icon"] = json!(icon);
            }
            if !metadata.is_empty() {
                body["metadata"] = metadata_json(metadata);
            }
            let product = api.post("/api/commerce/products", &body).await?;
            if opts.print_json(&product) {
                return Ok(());
            }
            println!("{} Created", "✓".green());
            println!("{}", product_line(&product));
            let call = if s(&product, "type") == "pass" {
                "PromptGamePassPurchase"
            } else {
                "PromptProductPurchase"
            };
            println!(
                "  In a script: MarketplaceService:{call}(player, {})",
                product["number"]
            );
            if product["active"].as_bool() != Some(true) {
                println!(
                    "  It is a draft: test it in Studio, then put it on sale with `eustress commerce products update {} --active true --live`.",
                    s(&product, "id")
                );
            }
        }
        ProductsCommand::Get { product, sim } => {
            let id = resolve_product_id(&api, &product, sim).await?;
            let product = api
                .get(&format!("/api/commerce/products/{id}"), &[])
                .await?;
            if !opts.print_json(&product) {
                print_product(&product);
            }
        }
        ProductsCommand::Update {
            product,
            sim,
            name,
            price,
            description,
            icon,
            space,
            active,
            metadata,
        } => {
            let id = resolve_product_id(&api, &product, sim).await?;
            let mut patch = serde_json::Map::new();
            if let Some(name) = name {
                patch.insert("name".into(), json!(name));
            }
            if let Some(price) = price {
                patch.insert("price".into(), json!(price));
            }
            if let Some(description) = description {
                patch.insert("description".into(), json!(description));
            }
            if let Some(icon) = icon {
                patch.insert("icon".into(), json!(icon));
            }
            if let Some(space) = space {
                patch.insert("space".into(), json!(space));
            }
            if let Some(active) = active {
                patch.insert("active".into(), json!(active));
            }
            if !metadata.is_empty() {
                patch.insert("metadata".into(), metadata_json(metadata));
            }
            if patch.is_empty() {
                bail!("Nothing to change: pass --name, --price, --description, --icon, --space, --active or --metadata");
            }
            let product = api
                .post(
                    &format!("/api/commerce/products/{id}"),
                    &Value::Object(patch),
                )
                .await?;
            if !opts.print_json(&product) {
                println!("{} Updated", "✓".green());
                print_product(&product);
            }
        }
        ProductsCommand::Archive { product, sim } => {
            let id = resolve_product_id(&api, &product, sim).await?;
            let product = api.delete(&format!("/api/commerce/products/{id}")).await?;
            if !opts.print_json(&product) {
                println!(
                    "{} Archived: it is off sale, and passes already bought stay owned",
                    "✓".green()
                );
                println!("{}", product_line(&product));
            }
        }
    }
    Ok(())
}

fn metadata_json(pairs: Vec<(String, String)>) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in pairs {
        // An empty value removes the key, as in the API.
        map.insert(
            key,
            if value.is_empty() {
                Value::Null
            } else {
                Value::String(value)
            },
        );
    }
    Value::Object(map)
}

// ─────────────────────────────────────────────────────────────────────────────
// purchases
// ─────────────────────────────────────────────────────────────────────────────

async fn purchases(opts: &Opts, action: PurchasesCommand) -> Result<()> {
    let api = opts.api()?;
    match action {
        PurchasesCommand::List {
            sim,
            limit,
            starting_after,
        } => {
            let mut query = vec![("limit", limit.to_string())];
            if let Some(sim) = sim {
                query.push(("sim_id", sim));
            }
            if let Some(after) = starting_after {
                query.push(("starting_after", after));
            }
            let list = api.get("/api/commerce/purchases", &query).await?;
            if !opts.print_json(&list) {
                print_list(
                    &list,
                    purchase_line,
                    &format!("No {} purchases yet.", opts.mode()),
                );
            }
        }
        PurchasesCommand::Get { id } => {
            let purchase = api
                .get(&format!("/api/commerce/purchases/{id}"), &[])
                .await?;
            if !opts.print_json(&purchase) {
                print_purchase(&purchase);
            }
        }
        PurchasesCommand::Refund { id, yes } => {
            if opts.live && !yes {
                let purchase = api
                    .get(&format!("/api/commerce/purchases/{id}"), &[])
                    .await?;
                let question = format!(
                    "Refund {}: give {} their {} Tickets back and take back your {}?",
                    id,
                    s(&purchase, "buyer_id"),
                    purchase["amount"].as_u64().unwrap_or(0),
                    purchase["creator_amount"].as_u64().unwrap_or(0)
                );
                if !confirm(&question)? {
                    println!("Nothing refunded.");
                    return Ok(());
                }
            }
            let purchase = api
                .post(&format!("/api/commerce/purchases/{id}/refund"), &json!({}))
                .await?;
            if !opts.print_json(&purchase) {
                println!("{} Refunded", "✓".green());
                println!("{}", purchase_line(&purchase));
            }
        }
        PurchasesCommand::Fulfill { id, sim } => {
            let sim = resolve_sim(sim, None)?;
            let purchase = api
                .post(
                    &format!("/api/commerce/purchases/{id}/fulfill"),
                    &json!({ "sim_id": sim }),
                )
                .await?;
            if !opts.print_json(&purchase) {
                println!("{} Fulfilled", "✓".green());
                println!("{}", purchase_line(&purchase));
            }
        }
        PurchasesCommand::Create {
            product,
            sim,
            space,
        } => {
            if opts.live {
                bail!("Live purchases are made by players inside the simulation. Test your storefront in test mode.");
            }
            let here = Here::detect();
            let sim = resolve_sim(sim, Some(&here))?;
            let product = match product {
                Some(p) => p,
                None => first_product(&api, &sim).await?,
            };
            let mut body = json!({
                "sim_id": sim,
                "product": product_ref_json(&product),
                "idempotency_key": format!("cli_{}", uuid::Uuid::new_v4().simple()),
            });
            if let Some(space) = space.or(here.space) {
                body["space"] = json!(space);
            }
            let reply = api.post("/api/commerce/purchases", &body).await?;
            if opts.print_json(&reply) {
                return Ok(());
            }
            println!("{} Test purchase made; no Tickets moved", "✓".green());
            println!("{}", purchase_line(&reply["purchase"]));
            println!("  Studio hands this receipt to your ProcessReceipt the next time you Play the simulation.");
        }
    }
    Ok(())
}

fn confirm(question: &str) -> Result<bool> {
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// events / listen / trigger
// ─────────────────────────────────────────────────────────────────────────────

fn check_event_type(kind: &str) -> Result<()> {
    if EVENT_TYPES.contains(&kind) {
        Ok(())
    } else {
        bail!(
            "unknown event type '{kind}': one of {}",
            EVENT_TYPES.join(", ")
        )
    }
}

async fn events(opts: &Opts, action: EventsCommand) -> Result<()> {
    let api = opts.api()?;
    match action {
        EventsCommand::List {
            kind,
            limit,
            starting_after,
        } => {
            let mut query = vec![("limit", limit.to_string())];
            if let Some(kind) = kind {
                check_event_type(&kind)?;
                query.push(("type", kind));
            }
            if let Some(after) = starting_after {
                query.push(("starting_after", after));
            }
            let list = api.get("/api/commerce/events", &query).await?;
            if !opts.print_json(&list) {
                print_list(&list, event_line, "No events yet.");
            }
        }
        EventsCommand::Get { id } => {
            let event = api.get(&format!("/api/commerce/events/{id}"), &[]).await?;
            println!("{}", serde_json::to_string_pretty(&event)?);
        }
        EventsCommand::Resend { id } => {
            let reply = api
                .post(&format!("/api/commerce/events/{id}/resend"), &json!({}))
                .await?;
            if !opts.print_json(&reply) {
                let n = reply["deliveries"].as_u64().unwrap_or(0);
                if n == 0 {
                    println!("No webhook endpoint takes this event; nothing was sent.");
                } else {
                    println!("{} Queued {n} delivery(ies) of {id}", "✓".green());
                }
            }
        }
    }
    Ok(())
}

async fn listen(opts: &Opts, args: ListenArgs) -> Result<()> {
    let api = opts.api()?;
    let secret_reply = api.get("/api/commerce/listen/secret", &[]).await?;
    let secret = s(&secret_reply, "secret").to_string();
    if secret.is_empty() {
        bail!("the API did not return a signing secret");
    }
    if args.print_secret {
        println!("{secret}");
        return Ok(());
    }
    let types: Vec<String> = args
        .events
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    for kind in &types {
        check_event_type(kind)?;
    }
    let forward = args.forward_to.as_deref().map(forward_url).transpose()?;
    let forwarder = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(args.skip_verify)
        .build()
        .context("could not build the forwarding client")?;

    if !opts.json {
        let target = forward
            .as_deref()
            .map(|u| format!(", forwarding to {u}"))
            .unwrap_or_default();
        println!(
            "{} Ready! Listening for {} events{target}. Your webhook signing secret is {} (^C to quit)",
            "✓".green(),
            opts.mode(),
            secret.bold()
        );
    }

    let mut cursor = "now".to_string();
    let mut delay = Duration::from_secs(1);
    loop {
        let mut query = vec![
            ("after", cursor.clone()),
            ("wait", LISTEN_WAIT_S.to_string()),
        ];
        if !types.is_empty() {
            query.push(("types", types.join(",")));
        }
        let page = tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            page = api.get("/api/commerce/events/stream", &query) => page,
        };
        match page {
            Ok(page) => {
                delay = Duration::from_secs(1);
                if let Some(next) = page.get("cursor").and_then(Value::as_str) {
                    cursor = next.to_string();
                }
                for event in items(&page) {
                    if opts.json {
                        println!("{}", serde_json::to_string(&event)?);
                    } else {
                        println!(
                            "{}   --> {} [{}]",
                            now_stamp(),
                            s(&event, "type").bold(),
                            s(&event, "id")
                        );
                    }
                    if let Some(url) = &forward {
                        forward_event(&forwarder, url, &secret, &event).await;
                    }
                }
            }
            Err(e) => {
                // A key that stopped working will not start again on retry.
                if e.downcast_ref::<ApiError>()
                    .map_or(false, ApiError::is_auth)
                {
                    return Err(e);
                }
                eprintln!("{} {e}; retrying in {}s", "⚠".yellow(), delay.as_secs());
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => return Ok(()),
                    _ = tokio::time::sleep(delay) => {}
                }
                delay = (delay * 2).min(Duration::from_secs(30));
            }
        }
    }
}

/// `localhost:4242/hooks` means `http://localhost:4242/hooks`, as in the Stripe CLI.
fn forward_url(raw: &str) -> Result<String> {
    let raw = raw.trim();
    let url = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    reqwest::Url::parse(&url).with_context(|| format!("--forward-to '{raw}' is not a URL"))?;
    Ok(url)
}

/// POST one event to the local endpoint, signed with the listen secret the
/// same way the API signs webhooks, so the endpoint's verification code is
/// the code that will verify real deliveries.
async fn forward_event(client: &reqwest::Client, url: &str, secret: &str, event: &Value) {
    let Ok(payload) = serde_json::to_string(event) else {
        return;
    };
    let signature = signature_header(secret, &payload, chrono::Utc::now().timestamp());
    let id = s(event, "id");
    let sent = client
        .post(url)
        .header("content-type", "application/json")
        .header("eustress-signature", signature)
        .body(payload)
        .send()
        .await;
    match sent {
        Ok(response) => {
            let status = response.status();
            let code = if status.is_success() {
                status.as_u16().to_string().green()
            } else {
                status.as_u16().to_string().red()
            };
            println!("{}  <--  [{code}] POST {url} [{id}]", now_stamp());
        }
        Err(e) => println!(
            "{}  <--  [{}] POST {url} [{id}] failed: {e}",
            now_stamp(),
            "ERR".red()
        ),
    }
}

/// `Eustress-Signature`: `t={unix seconds},v1={hex HMAC-SHA256 of "{t}.{payload}"}`.
fn signature_header(secret: &str, payload: &str, t: i64) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC takes a key of any length");
    mac.update(format!("{t}.{payload}").as_bytes());
    format!("t={t},v1={}", hex::encode(mac.finalize().into_bytes()))
}

async fn trigger(opts: &Opts, args: TriggerArgs) -> Result<()> {
    if opts.live {
        bail!("Triggers run in test mode only: they fire events with a made-up buyer and move nothing");
    }
    if !TRIGGERS.contains(&args.event.as_str()) {
        bail!("event must be one of {}", TRIGGERS.join(", "));
    }
    let api = opts.api()?;
    let sim = resolve_sim(args.sim, None)?;
    let product = match args.product {
        Some(p) => p,
        None => first_product(&api, &sim).await?,
    };
    let reply = api
        .post(
            "/api/commerce/test_helpers/trigger",
            &json!({ "event": args.event, "sim_id": sim, "product": product_ref_json(&product) }),
        )
        .await?;
    if opts.print_json(&reply) {
        return Ok(());
    }
    println!("{} Triggered {}", "✓".green(), args.event.bold());
    if reply.get("purchase").is_some() {
        println!("{}", purchase_line(&reply["purchase"]));
    } else if reply.get("event").is_some() {
        println!("{}", event_line(&reply["event"]));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// webhooks / keys
// ─────────────────────────────────────────────────────────────────────────────

async fn webhooks(opts: &Opts, action: WebhooksCommand) -> Result<()> {
    let api = opts.api()?;
    match action {
        WebhooksCommand::List => {
            let list = api.get("/api/commerce/webhook_endpoints", &[]).await?;
            if !opts.print_json(&list) {
                print_list(
                    &list,
                    endpoint_line,
                    &format!("No {} webhook endpoints.", opts.mode()),
                );
            }
        }
        WebhooksCommand::Create {
            url,
            events,
            description,
        } => {
            let events: Vec<String> = events
                .into_iter()
                .map(|e| e.trim().to_string())
                .filter(|e| !e.is_empty())
                .collect();
            for kind in &events {
                check_event_type(kind)?;
            }
            let mut body = json!({ "url": url });
            if !events.is_empty() {
                body["enabled_events"] = json!(events);
            }
            if let Some(description) = description {
                body["description"] = json!(description);
            }
            let endpoint = api.post("/api/commerce/webhook_endpoints", &body).await?;
            if opts.print_json(&endpoint) {
                return Ok(());
            }
            println!("{} Created", "✓".green());
            println!("{}", endpoint_line(&endpoint));
            println!(
                "  Signing secret (shown once): {}",
                s(&endpoint, "secret").bold()
            );
        }
        WebhooksCommand::Delete { id } => {
            let reply = api
                .delete(&format!("/api/commerce/webhook_endpoints/{id}"))
                .await?;
            if !opts.print_json(&reply) {
                println!("{} Deleted {id}", "✓".green());
            }
        }
    }
    Ok(())
}

async fn keys(opts: &Opts, action: KeysCommand) -> Result<()> {
    let api = opts.session_api()?;
    match action {
        KeysCommand::List => {
            let reply = api.get("/api/keys", &[]).await?;
            if opts.print_json(&reply) {
                return Ok(());
            }
            let keys = items_at(&reply, "keys");
            if keys.is_empty() {
                println!("No API keys.");
            }
            for key in &keys {
                println!(
                    "{}  {:<20} {:<9} {:<4}  {:<28} last used {}",
                    s(key, "id").dimmed(),
                    s(key, "key_prefix"),
                    s(key, "key_type"),
                    s(key, "mode"),
                    clip(s(key, "name"), 28),
                    key.get("last_used")
                        .and_then(Value::as_str)
                        .unwrap_or("never")
                );
            }
        }
        KeysCommand::Create { name, kind } => {
            let label = name.unwrap_or_else(|| format!("Eustress CLI ({})", machine_name()));
            let created = api
                .post(
                    "/api/keys",
                    &json!({ "name": label, "mode": opts.mode(), "key_type": kind }),
                )
                .await?;
            if opts.print_json(&created) {
                return Ok(());
            }
            println!(
                "{} Created {} {} key {}",
                "✓".green(),
                s(&created, "mode"),
                s(&created, "key_type"),
                s(&created, "id")
            );
            println!("  Key (shown once): {}", s(&created, "key").bold());
        }
        KeysCommand::Revoke { id } => {
            let reply = api.delete(&format!("/api/keys/{id}")).await?;
            if !opts.print_json(&reply) {
                println!("{} Revoked {id}", "✓".green());
            }
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Output
// ─────────────────────────────────────────────────────────────────────────────

/// A string field, or "".
fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A list reply's `data`.
fn items(list: &Value) -> Vec<Value> {
    items_at(list, "data")
}

fn items_at(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn strings_at(value: &Value, key: &str) -> Vec<String> {
    items_at(value, key)
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

fn print_list(list: &Value, line: fn(&Value) -> String, empty: &str) {
    let data = items(list);
    if data.is_empty() {
        println!("{empty}");
        return;
    }
    for item in &data {
        println!("{}", line(item));
    }
    if list["has_more"].as_bool() == Some(true) {
        if let Some(last) = data.last() {
            println!(
                "{}",
                format!("more: --starting-after {}", s(last, "id")).dimmed()
            );
        }
    }
}

fn product_line(p: &Value) -> String {
    let state = if p["active"].as_bool() == Some(true) {
        "on sale".green()
    } else {
        "draft".yellow()
    };
    format!(
        "{}  #{:<4} {:<24} {:>9}  {:<10} {:<16} {}",
        s(p, "id").dimmed(),
        p["number"].as_u64().unwrap_or(0),
        clip(s(p, "name"), 24),
        format!("{} TKT", p["price"].as_u64().unwrap_or(0)),
        s(p, "type"),
        clip(s(p, "space"), 16),
        state
    )
}

fn print_product(p: &Value) {
    println!("{}", product_line(p));
    if !s(p, "description").is_empty() {
        println!("  {}", s(p, "description"));
    }
    if let Some(metadata) = p.get("metadata").and_then(Value::as_object) {
        for (key, value) in metadata {
            println!("  {key} = {}", value.as_str().unwrap_or_default());
        }
    }
}

fn purchase_line(p: &Value) -> String {
    let status = match s(p, "status") {
        "refunded" => "refunded".red(),
        other => other.normal(),
    };
    let fulfilled = if p["fulfilled"].as_bool() == Some(true) {
        "fulfilled".green()
    } else {
        "awaiting fulfillment".yellow()
    };
    let synthetic = if p["synthetic"].as_bool() == Some(true) {
        " (synthetic)"
    } else {
        ""
    };
    format!(
        "{}  {}  {:<24} {:>9}  buyer {}{}  {} {}",
        s(p, "id").dimmed(),
        when(p["created"].as_i64().unwrap_or(0)),
        clip(s(&p["product"], "name"), 24),
        format!("{} TKT", p["amount"].as_u64().unwrap_or(0)),
        s(p, "buyer_id"),
        synthetic,
        status,
        fulfilled
    )
}

fn print_purchase(p: &Value) {
    println!("{}", purchase_line(p));
    println!(
        "  {} mode  |  simulation {} ({})  |  Space {}",
        if p["livemode"].as_bool() == Some(true) {
            "live"
        } else {
            "test"
        },
        s(p, "sim_name"),
        s(p, "sim_id"),
        p["space"].as_str().unwrap_or("-")
    );
    println!(
        "  {} TKT: {} to the creator, {} to the platform",
        p["amount"].as_u64().unwrap_or(0),
        p["creator_amount"].as_u64().unwrap_or(0),
        p["platform_amount"].as_u64().unwrap_or(0)
    );
    if let Some(at) = p["refunded_at"].as_i64() {
        println!("  refunded {}", when(at));
    }
}

fn event_line(e: &Value) -> String {
    format!(
        "{}  {}  {:<20} {}",
        s(e, "id").dimmed(),
        when(e["created"].as_i64().unwrap_or(0)),
        s(e, "type"),
        event_summary(e)
    )
}

fn event_summary(e: &Value) -> String {
    let object = &e["data"]["object"];
    match s(e, "type") {
        kind if kind.starts_with("product.") => format!(
            "#{} {} ({} TKT)",
            object["number"].as_u64().unwrap_or(0),
            s(object, "name"),
            object["price"].as_u64().unwrap_or(0)
        ),
        "purchase.failed" => format!(
            "{} for {}: {}",
            s(&object["product"], "name"),
            s(object, "buyer_id"),
            s(object, "failure_code")
        ),
        _ => format!(
            "{} {} ({} TKT) by {}",
            s(object, "id"),
            s(&object["product"], "name"),
            object["amount"].as_u64().unwrap_or(0),
            s(object, "buyer_id")
        ),
    }
}

fn endpoint_line(we: &Value) -> String {
    let events = strings_at(we, "enabled_events").join(",");
    let last = match we.get("last_delivery") {
        Some(d) if d.is_object() => {
            if d["ok"].as_bool() == Some(true) {
                format!("last delivery {} ok", when(d["at"].as_i64().unwrap_or(0)))
                    .green()
                    .to_string()
            } else {
                format!(
                    "last delivery {} failed ({})",
                    when(d["at"].as_i64().unwrap_or(0)),
                    d["status"]
                )
                .red()
                .to_string()
            }
        }
        _ => "no deliveries yet".dimmed().to_string(),
    };
    format!(
        "{}  {}  [{}]  {}",
        s(we, "id").dimmed(),
        s(we, "url"),
        events,
        last
    )
}

fn mode_badge(live: bool) -> String {
    if live {
        "LIVE mode".red().bold().to_string()
    } else {
        "test mode".yellow().to_string()
    }
}

fn when(unix_seconds: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(unix_seconds, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| unix_seconds.to_string())
}

fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

/// `ek_test_AbCd...wxyz`: enough to recognise a key, never enough to use it.
fn mask_key(key: &str) -> String {
    if key.len() <= 16 || !key.is_ascii() {
        return "ek_...".to_string();
    }
    format!("{}...{}", &key[..12], &key[key.len() - 4..])
}

/// Whether `key` is a live commerce key, or `None` when it is not a commerce key.
fn key_is_live(key: &str) -> Option<bool> {
    let (live, rest) = if let Some(rest) = key.strip_prefix("ek_live_") {
        (true, rest)
    } else if let Some(rest) = key.strip_prefix("ek_test_") {
        (false, rest)
    } else {
        return None;
    };
    (rest.len() == 40 && rest.bytes().all(|b| b.is_ascii_alphanumeric())).then_some(live)
}

fn machine_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| "this machine".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_match_the_api() {
        // The Worker's signatureHeader('whsec_TestSecret0123456789', payload, 1790000000).
        let payload = r#"{"id":"evt_test","type":"purchase.succeeded"}"#;
        assert_eq!(
            signature_header("whsec_TestSecret0123456789", payload, 1_790_000_000),
            "t=1790000000,v1=33b11ad9ebc4011a2558b1e942b173deb18151985e6b1736c345ee843168979a"
        );
    }

    #[test]
    fn keys_carry_their_mode() {
        let body = "A".repeat(40);
        assert_eq!(key_is_live(&format!("ek_test_{body}")), Some(false));
        assert_eq!(key_is_live(&format!("ek_live_{body}")), Some(true));
        assert_eq!(key_is_live(&format!("ek_live_{}", "A".repeat(39))), None);
        assert_eq!(key_is_live("sk_test_abc"), None);
        assert_eq!(mask_key(&format!("ek_test_{body}")), "ek_test_AAAA...AAAA");
    }

    #[test]
    fn forward_targets_default_to_http() {
        assert_eq!(
            forward_url("localhost:4242/hooks").unwrap(),
            "http://localhost:4242/hooks"
        );
        assert_eq!(
            forward_url("https://example.com/x").unwrap(),
            "https://example.com/x"
        );
    }

    #[test]
    fn metadata_pairs_parse_and_an_empty_value_removes() {
        assert_eq!(
            parse_key_value("tier=gold").unwrap(),
            ("tier".to_string(), "gold".to_string())
        );
        assert!(parse_key_value("=gold").is_err());
        assert!(parse_key_value("tier").is_err());
        let md = metadata_json(vec![("a".into(), "1".into()), ("b".into(), String::new())]);
        assert_eq!(md, json!({ "a": "1", "b": null }));
    }

    #[test]
    fn product_references_are_numbers_or_ids() {
        assert_eq!(product_ref_json(" 3 "), json!(3));
        assert_eq!(product_ref_json("prod_abc"), json!("prod_abc"));
    }

    #[test]
    fn the_published_id_is_read_from_sync_toml() {
        let dir = std::env::temp_dir().join(format!("eustress-cli-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sync.toml");
        std::fs::write(&path, "[sync]\nmode = \"local_first\"\n\n[remote]\nprovider = \"cloudflare_r2\"\nexperience_id = \"abc-123\"\n").unwrap();
        assert_eq!(read_experience_id(&path).as_deref(), Some("abc-123"));
        std::fs::write(&path, "[remote]\nprovider = \"cloudflare_r2\"\n").unwrap();
        assert_eq!(read_experience_id(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
