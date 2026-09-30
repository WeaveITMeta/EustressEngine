//! # Model discovery with the user's own keys
//!
//! Asks each provider the user holds a key for which models that key can use,
//! and resolves the Workshop picker against the answer. The requests go
//! straight from this machine to `api.anthropic.com`, `api.x.ai` and
//! `api.openai.com` with the user's own key: nothing passes through Eustress,
//! and listing models bills no tokens, so neither Eustress nor the user pays
//! anything for it.
//!
//! ## Why not a compiled catalog from a server
//!
//! The list used to be compiled nightly on `api.eustress.dev` by Grok reading
//! live web search, on Eustress's xAI bill. Grok held no Anthropic or OpenAI
//! key, so nothing it published was ever confirmed by the provider. It aliased
//! everyone on Claude Opus 5 to Opus 5.5 while Opus 5 was still served, and a
//! page planting `claude-opus-6` at a plausible price would have passed every
//! shape and range check it had. A provider's authenticated model list cannot
//! be planted: the only models offered here are ones their provider lists for
//! the user's own key.
//!
//! ## The rules ([`resolve`])
//!
//! * A provider with no key, or one that did not answer, offers its curated
//!   models as compiled in. Nothing to confirm them against, nothing invented.
//! * A provider that answered offers its curated models that it listed, and
//!   drops the rest: its own API says they do not exist for this key.
//! * A listed model the table does not know appears only if it is a chat
//!   model and newer than every curated model the provider confirmed, at most
//!   [`NEW_MODELS_PER_PROVIDER`] per provider. Its name comes from the provider
//!   (or its id), and its price from the provider's list if the list carries
//!   one (xAI's does), otherwise it is shown as not published.
//!
//! The list is resolved at launch and again whenever a key changes. A turn
//! already running keeps its model; see `workshop_model` for why that is safe.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use serde_json::Value;

use super::workshop_model::{install_catalog, Catalog, ModelOrigin, ModelSpec, Provider};

/// At most this many models the table does not know are surfaced per provider,
/// newest first. The job is currency, not breadth: a picker with thirty
/// entries is worse than one with six.
pub const NEW_MODELS_PER_PROVIDER: usize = 2;

/// Output cap for a surfaced model: the provider's stated maximum, held to the
/// same ceiling the curated reasoning models use.
const DISCOVERED_MAX_TOKENS_CEILING: u32 = 32000;
const DISCOVERED_MAX_TOKENS_DEFAULT: u32 = 16384;
const DISCOVERED_TIMEOUT_SECS: u64 = 300;

/// One model as a provider's own list describes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListedModel {
    pub id: String,
    /// The provider's own name for it, when the list carries one.
    pub display_name: Option<String>,
    /// Release time, Unix seconds. Orders "newer than".
    pub created: i64,
    pub max_tokens: Option<u32>,
    /// USD per million tokens, only when the provider's list states it.
    pub input_price_per_mtok: Option<f64>,
    pub output_price_per_mtok: Option<f64>,
    pub vision: Option<bool>,
    /// Other ids the provider accepts for this same model.
    pub aliases: Vec<String>,
    /// Takes text and returns text, the only kind the Workshop can drive.
    pub chat: bool,
}

/// Each answering provider's list. A provider absent from the map had no key
/// or did not answer.
pub type Listings = HashMap<Provider, Vec<ListedModel>>;

// ── Reading the providers' answers ──────────────────────────────────────────

/// One page of Anthropic's `GET /v1/models`, plus the id to continue after
/// when there is another page. Every model on it is a chat model.
pub fn parse_anthropic_page(body: &Value) -> (Vec<ListedModel>, Option<String>) {
    let models = body["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    Some(ListedModel {
                        // "Claude Opus 5.5" reads as "Opus 5.5" in a picker
                        // whose section header already says Anthropic.
                        display_name: m["display_name"]
                            .as_str()
                            .map(|n| n.strip_prefix("Claude ").unwrap_or(n).to_string()),
                        created: m["created_at"]
                            .as_str()
                            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                            .map(|t| t.timestamp())
                            .unwrap_or(0),
                        max_tokens: m["max_tokens"].as_u64().map(|t| t.min(u32::MAX as u64) as u32),
                        vision: m["capabilities"]["image_input"]["supported"].as_bool(),
                        chat: true,
                        id,
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let next = match body["has_more"].as_bool() {
        Some(true) => body["last_id"].as_str().map(str::to_string),
        _ => None,
    };
    (models, next)
}

/// OpenAI's `GET /v1/models`: ids and release times only, no names or prices,
/// and every kind of model mixed together.
pub fn parse_openai(body: &Value) -> Vec<ListedModel> {
    body["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    Some(ListedModel {
                        created: m["created"].as_i64().unwrap_or(0),
                        chat: openai_is_chat(&id),
                        id,
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// xAI's `GET /v1/language-models`: ids, aliases, modalities and prices.
pub fn parse_xai(body: &Value) -> Vec<ListedModel> {
    body["models"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let inputs = str_list(&m["input_modalities"]);
                    let outputs = str_list(&m["output_modalities"]);
                    Some(ListedModel {
                        created: m["created"].as_i64().unwrap_or(0),
                        input_price_per_mtok: xai_price(&m["prompt_text_token_price"]),
                        output_price_per_mtok: xai_price(&m["completion_text_token_price"]),
                        vision: Some(inputs.contains(&"image")),
                        aliases: str_list(&m["aliases"]).into_iter().map(str::to_string).collect(),
                        chat: inputs.contains(&"text") && outputs.contains(&"text"),
                        id,
                        ..Default::default()
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn str_list(value: &Value) -> Vec<&str> {
    value
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// xAI states prices in US cents per 100 million tokens, so one dollar per
/// million tokens is 10,000 of its units. Zero or absent reads as unknown,
/// never as free.
fn xai_price(value: &Value) -> Option<f64> {
    let cents_per_100m = value.as_f64()?;
    (cents_per_100m > 0.0).then(|| cents_per_100m / 10_000.0)
}

/// Whether an id from OpenAI's mixed list is a chat model the Workshop can
/// drive over chat completions. Only used to decide which models the table
/// does not know are worth surfacing; it never admits an id OpenAI did not
/// list, so a wrong guess here costs a picker entry, not safety.
fn openai_is_chat(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let family = id.starts_with("gpt-")
        || (id.starts_with('o') && id[1..].starts_with(|c: char| c.is_ascii_digit()));
    const NOT_CHAT: &[&str] = &[
        "audio", "realtime", "transcribe", "tts", "image", "embedding", "moderation",
        "search", "instruct", "codex", "computer-use", "deep-research", "-pro",
    ];
    family && !NOT_CHAT.iter().any(|w| id.contains(w)) && !is_dated_snapshot(&id)
}

/// `gpt-4o-2024-08-06` or `gpt-4-0613`: a frozen snapshot of a model the list
/// also carries under its plain id.
fn is_dated_snapshot(id: &str) -> bool {
    fn digits(s: &str, n: usize) -> bool {
        s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
    }
    let parts: Vec<&str> = id.split('-').collect();
    match parts.as_slice() {
        [.., y, m, d] if digits(y, 4) && digits(m, 2) && digits(d, 2) => true,
        [.., tail] => digits(tail, 4),
        [] => false,
    }
}

/// A picker name for a model the table does not know: the provider's own name
/// when it gives one, otherwise one made from the id in the provider's house
/// style ("gpt-6-sol" reads "GPT-6 Sol", "grok-4.7" reads "Grok 4.7").
fn picker_name(provider: Provider, listed: &ListedModel) -> String {
    if let Some(name) = &listed.display_name {
        return name.clone();
    }
    fn title(word: &str) -> String {
        let mut chars = word.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    }
    let id = listed.id.as_str();
    let mut words = id.split('-');
    match (provider, words.next()) {
        (Provider::OpenAi, Some("gpt")) => {
            let version = words.next().unwrap_or_default();
            let rest: Vec<String> = words.map(title).collect();
            if rest.is_empty() {
                format!("GPT-{version}")
            } else {
                format!("GPT-{version} {}", rest.join(" "))
            }
        }
        // o-series ids are already their names.
        (Provider::OpenAi, _) => id.to_string(),
        (Provider::Anthropic, Some("claude")) => words.map(title).collect::<Vec<_>>().join(" "),
        _ => id.split('-').map(title).collect::<Vec<_>>().join(" "),
    }
}

// ── Resolving the picker ────────────────────────────────────────────────────

/// Resolve the curated table against the providers' own lists. Pure, so every
/// rule in the module docs is pinned by a test below.
pub fn resolve(curated: &Catalog, listings: &Listings) -> Catalog {
    let mut models: Vec<ModelSpec> = Vec::new();

    for provider in Provider::ALL {
        let table = curated.models.iter().filter(|m| m.provider == provider);
        let Some(listed) = listings.get(&provider) else {
            // No key to ask with, or no answer: the table as compiled in.
            models.extend(table.cloned());
            continue;
        };

        let lookup = |id: &str| listed.iter().find(|l| l.id == id || l.aliases.iter().any(|a| a == id));

        let mut newest_confirmed: Option<i64> = None;
        for spec in table {
            // Not listed: the provider says this key cannot use it.
            let Some(entry) = lookup(&spec.id) else { continue };
            let mut spec = spec.clone();
            // The provider's own price beats the compiled one when it states it.
            if let (Some(input), Some(output)) = (entry.input_price_per_mtok, entry.output_price_per_mtok) {
                spec.input_price_per_mtok = Some(input);
                spec.output_price_per_mtok = Some(output);
            }
            newest_confirmed = Some(newest_confirmed.map_or(entry.created, |n| n.max(entry.created)));
            models.push(spec);
        }

        let in_table = |l: &ListedModel| {
            curated
                .models
                .iter()
                .any(|m| m.provider == provider && (m.id == l.id || l.aliases.contains(&m.id)))
        };
        let mut fresh: Vec<&ListedModel> = listed
            .iter()
            .filter(|l| l.chat && !in_table(l) && newest_confirmed.map_or(true, |n| l.created > n))
            .collect();
        fresh.sort_by(|a, b| b.created.cmp(&a.created));
        for entry in fresh.into_iter().take(NEW_MODELS_PER_PROVIDER) {
            models.push(ModelSpec {
                id: entry.id.clone(),
                display_name: picker_name(provider, entry),
                provider,
                tagline: format!("New from {}, listed for your key.", provider.label()),
                input_price_per_mtok: entry.input_price_per_mtok,
                output_price_per_mtok: entry.output_price_per_mtok,
                max_tokens: entry
                    .max_tokens
                    .map(|t| t.min(DISCOVERED_MAX_TOKENS_CEILING))
                    .unwrap_or(DISCOVERED_MAX_TOKENS_DEFAULT),
                timeout_secs: DISCOVERED_TIMEOUT_SECS,
                vision: entry.vision.unwrap_or(false),
                origin: ModelOrigin::Discovered,
            });
        }
    }

    // A key that can reach nothing: keep the table rather than an empty menu.
    // Sending will then fail with the provider's own explanation.
    if models.is_empty() {
        return curated.clone();
    }

    // Grouped by provider, cheapest first, unknown prices last. Stable, so the
    // table's own order breaks ties.
    let group = |m: &ModelSpec| Provider::ALL.iter().position(|p| *p == m.provider).unwrap_or(usize::MAX);
    let price = |m: &ModelSpec| m.input_price_per_mtok.unwrap_or(f64::INFINITY);
    models.sort_by(|a, b| group(a).cmp(&group(b)).then(price(a).total_cmp(&price(b))));

    let present = |id: &str| models.iter().any(|m| m.id == id);

    // Aliases: the table's own, plus every alternate id a provider declared
    // for a model on offer. Never an alias that shadows a model on offer, and
    // never one pointing at a model that is not.
    let mut aliases = curated.aliases.clone();
    for (provider, listed) in listings {
        for entry in listed {
            let Some(offered) = models.iter().find(|m| {
                m.provider == *provider && (m.id == entry.id || entry.aliases.contains(&m.id))
            }) else {
                continue;
            };
            for name in std::iter::once(&entry.id).chain(&entry.aliases) {
                if *name != offered.id {
                    aliases.entry(name.clone()).or_insert_with(|| offered.id.clone());
                }
            }
        }
    }
    aliases.retain(|from, to| present(to) && !present(from));

    let default_model = if present(&curated.default_model) {
        curated.default_model.clone()
    } else {
        models[0].id.clone()
    };
    // No stand-in advisor: an advisor the key cannot reach withholds the tool.
    let advisor_model = if present(&curated.advisor_model) {
        curated.advisor_model.clone()
    } else {
        String::new()
    };

    Catalog { models, default_model, advisor_model, aliases }
}

// ── Asking the providers ────────────────────────────────────────────────────

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// GET a provider endpoint and read its JSON. Errors carry the status or the
/// kind of transport failure only, never the request, so a key can never end
/// up in a log line.
fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, String> {
    let mut request = ureq::get(url).timeout(FETCH_TIMEOUT);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    match request.call() {
        Ok(response) => response
            .into_json::<Value>()
            .map_err(|e| format!("unreadable response: {e}")),
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(ureq::Error::Transport(e)) => Err(e.kind().to_string()),
    }
}

fn fetch_anthropic(key: &str) -> Result<Vec<ListedModel>, String> {
    const PAGE: &str = "https://api.anthropic.com/v1/models?limit=1000";
    let headers = [("x-api-key", key), ("anthropic-version", "2023-06-01")];
    let mut all = Vec::new();
    let mut url = PAGE.to_string();
    // Ten pages of a thousand is far beyond any real list; the cap only stops
    // a misbehaving `has_more` from looping forever.
    for _ in 0..10 {
        let (page, next) = parse_anthropic_page(&get_json(&url, &headers)?);
        all.extend(page);
        match next {
            Some(after) if after.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) => {
                url = format!("{PAGE}&after_id={after}");
            }
            _ => break,
        }
    }
    Ok(all)
}

fn fetch_xai(key: &str) -> Result<Vec<ListedModel>, String> {
    let bearer = format!("Bearer {key}");
    get_json("https://api.x.ai/v1/language-models", &[("Authorization", bearer.as_str())]).map(|b| parse_xai(&b))
}

fn fetch_openai(key: &str) -> Result<Vec<ListedModel>, String> {
    let bearer = format!("Bearer {key}");
    get_json("https://api.openai.com/v1/models", &[("Authorization", bearer.as_str())]).map(|b| parse_openai(&b))
}

// ── Running discovery in the engine ─────────────────────────────────────────

/// The user's keys, one per provider. Held only for the length of one
/// discovery; its hash (in memory, never written anywhere) is how a key
/// change is noticed.
#[derive(Clone, Default, Hash)]
pub struct ProviderKeys {
    pub anthropic: Option<String>,
    pub xai: Option<String>,
    pub openai: Option<String>,
}

/// Bumped each time a resolved list is installed, so the Workshop picker
/// re-reads the models.
#[derive(Resource, Default)]
pub struct ModelCatalogRevision(pub u64);

/// The newest discovery started. An older one that finishes late is ignored,
/// so a key removed a moment ago can never have its list installed.
static GENERATION: AtomicU64 = AtomicU64::new(0);
static FINISHED: Mutex<Option<(u64, Listings)>> = Mutex::new(None);

/// Ask every provider the user holds a key for, on a background thread.
pub fn begin_discovery(keys: ProviderKeys) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let spawned = std::thread::Builder::new()
        .name("model-discovery".into())
        .spawn(move || {
            let mut listings = Listings::new();
            let providers: [(Provider, Option<String>, fn(&str) -> Result<Vec<ListedModel>, String>); 3] = [
                (Provider::Anthropic, keys.anthropic, fetch_anthropic),
                (Provider::Xai, keys.xai, fetch_xai),
                (Provider::OpenAi, keys.openai, fetch_openai),
            ];
            for (provider, key, fetch) in providers {
                let Some(key) = key else { continue };
                match fetch(&key) {
                    Ok(models) => {
                        info!("Workshop: {} listed {} models for your key", provider.label(), models.len());
                        listings.insert(provider, models);
                    }
                    Err(why) => warn!(
                        "Workshop: could not list {} models ({why}); offering the built-in {} models unconfirmed",
                        provider.label(),
                        provider.label()
                    ),
                }
            }
            *FINISHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((generation, listings));
        });
    if let Err(e) = spawned {
        warn!("Workshop: could not start model discovery: {e}");
    }
}

/// Start a discovery at launch and whenever a key changes.
pub fn rediscover_on_key_change(
    global: Option<Res<super::GlobalSoulSettings>>,
    space: Option<Res<super::SoulServiceSettings>>,
    mut last: Local<Option<u64>>,
) {
    use std::hash::{Hash, Hasher};

    let Some(global) = global else { return };
    let changed = global.is_changed() || space.as_ref().is_some_and(|s| s.is_changed());
    if last.is_some() && !changed {
        return;
    }

    let usable = |k: String| (!k.trim().is_empty()).then(|| k.trim().to_string());
    let anthropic = match space.as_ref() {
        Some(space) => usable(space.effective_api_key(&global)),
        None => global.key_for_provider(Provider::Anthropic),
    };
    let keys = ProviderKeys {
        anthropic,
        xai: global.key_for_provider(Provider::Xai),
        openai: global.key_for_provider(Provider::OpenAi),
    };

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    keys.hash(&mut hasher);
    let fingerprint = hasher.finish();
    if *last == Some(fingerprint) {
        return;
    }
    *last = Some(fingerprint);
    // Also runs with no keys at all: that resolves to the curated table, so a
    // list confirmed with a key the user just removed does not linger.
    begin_discovery(keys);
}

/// Install a finished discovery's list.
pub fn apply_discovered_models(mut revision: ResMut<ModelCatalogRevision>) {
    let finished = FINISHED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    let Some((generation, listings)) = finished else { return };
    if generation != GENERATION.load(Ordering::SeqCst) {
        return;
    }
    if install_catalog(resolve(&Catalog::curated(), &listings)) {
        revision.0 += 1;
    }
}

/// Remove the Grok-compiled catalog earlier builds cached. Nothing reads it any
/// more, and it holds aliases no provider confirmed, so it goes rather than
/// sitting there looking current.
pub fn remove_retired_catalog_cache() {
    if let Some(home) = dirs::home_dir() {
        let _ = std::fs::remove_file(home.join(".eustress_engine").join("model_catalog.json"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn listed(id: &str, created: i64) -> ListedModel {
        ListedModel { id: id.into(), created, chat: true, ..Default::default() }
    }

    fn ids(catalog: &Catalog) -> Vec<&str> {
        catalog.models.iter().map(|m| m.id.as_str()).collect()
    }

    /// Anthropic's list as it stands: every curated Claude model, plus an
    /// older one the table deliberately leaves out.
    fn anthropic_today() -> Vec<ListedModel> {
        vec![
            listed("claude-haiku-4-5", 100),
            listed("claude-sonnet-5", 200),
            listed("claude-opus-5", 210),
            listed("claude-fable-5-1", 220),
            listed("claude-opus-5-5", 230),
        ]
    }

    #[test]
    fn a_model_its_provider_does_not_list_is_never_offered() {
        // The imposter case: a convincing id at a plausible price, planted in
        // whatever a list is built from. Here it is planted in the table
        // itself, the worst place; the provider's own list still keeps it out.
        let mut poisoned = Catalog::curated();
        let mut fake = poisoned.models[0].clone();
        fake.id = "claude-opus-6".into();
        fake.display_name = "Opus 6".into();
        poisoned.models.push(fake);

        let listings = Listings::from([(Provider::Anthropic, anthropic_today())]);
        let resolved = resolve(&poisoned, &listings);
        assert!(!ids(&resolved).contains(&"claude-opus-6"), "{:?}", ids(&resolved));
    }

    #[test]
    fn a_still_served_model_stays_on_offer() {
        // The Grok list aliased Opus 5 to Opus 5.5 while Opus 5 was served.
        let listings = Listings::from([(Provider::Anthropic, anthropic_today())]);
        let resolved = resolve(&Catalog::curated(), &listings);
        assert!(ids(&resolved).contains(&"claude-opus-5"));
        assert!(!resolved.aliases.contains_key("claude-opus-5"));
    }

    #[test]
    fn a_provider_without_a_key_offers_its_table_unchanged() {
        let listings = Listings::from([(Provider::Anthropic, anthropic_today())]);
        let resolved = resolve(&Catalog::curated(), &listings);
        assert!(ids(&resolved).contains(&"grok-4.6"));
        assert!(ids(&resolved).contains(&"gpt-6-astra"));
    }

    #[test]
    fn a_model_the_key_cannot_reach_is_dropped() {
        let mut anthropic = anthropic_today();
        anthropic.retain(|m| m.id != "claude-fable-5-1");
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Anthropic, anthropic)]));
        assert!(!ids(&resolved).contains(&"claude-fable-5-1"));
        // The advisor role goes with it rather than falling to a stand-in.
        assert_eq!(resolved.advisor_model, "");
        // And its alias with it, so a stored claude-fable-5 cannot point at nothing.
        assert!(!resolved.aliases.contains_key("claude-fable-5"));
    }

    #[test]
    fn a_newer_listed_model_appears_with_the_providers_own_name() {
        let mut anthropic = anthropic_today();
        anthropic.push(ListedModel {
            display_name: Some("Opus 6".into()),
            max_tokens: Some(128_000),
            ..listed("claude-opus-6", 400)
        });
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Anthropic, anthropic)]));
        let new = resolved.models.iter().find(|m| m.id == "claude-opus-6").expect("surfaced");
        assert_eq!(new.display_name, "Opus 6");
        assert_eq!(new.origin, ModelOrigin::Discovered);
        assert_eq!(new.input_price_per_mtok, None, "Anthropic's list carries no prices");
        assert_eq!(new.max_tokens, DISCOVERED_MAX_TOKENS_CEILING);
    }

    #[test]
    fn an_older_uncurated_model_stays_out() {
        // claude-haiku-4-5 is listed but older than everything curated.
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Anthropic, anthropic_today())]));
        assert!(!ids(&resolved).contains(&"claude-haiku-4-5"));
    }

    #[test]
    fn surfaced_models_are_capped_newest_first() {
        let mut anthropic = anthropic_today();
        for (i, id) in ["claude-a", "claude-b", "claude-c"].iter().enumerate() {
            anthropic.push(listed(id, 500 + i as i64));
        }
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Anthropic, anthropic)]));
        let surfaced: Vec<&str> = resolved
            .models
            .iter()
            .filter(|m| m.origin == ModelOrigin::Discovered)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(surfaced.len(), NEW_MODELS_PER_PROVIDER);
        assert!(surfaced.contains(&"claude-c") && surfaced.contains(&"claude-b"), "{surfaced:?}");
    }

    #[test]
    fn unknown_prices_sort_after_known_ones() {
        let mut anthropic = anthropic_today();
        anthropic.push(listed("claude-opus-6", 400));
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Anthropic, anthropic)]));
        let claude: Vec<&ModelSpec> = resolved.models.iter().filter(|m| m.provider == Provider::Anthropic).collect();
        assert_eq!(claude.last().map(|m| m.id.as_str()), Some("claude-opus-6"));
        assert_eq!(claude.first().map(|m| m.id.as_str()), Some("claude-sonnet-5"));
    }

    #[test]
    fn xai_prices_replace_the_table_and_read_in_the_right_units() {
        // $3 in, $15 out per million, stated in cents per 100 million tokens.
        let body = json!({ "models": [{
            "id": "grok-4.6", "created": 300, "aliases": ["grok-4.6-latest"],
            "input_modalities": ["text", "image"], "output_modalities": ["text"],
            "prompt_text_token_price": 30000, "completion_text_token_price": 150000
        }]});
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Xai, parse_xai(&body))]));
        let grok = resolved.models.iter().find(|m| m.id == "grok-4.6").expect("confirmed");
        assert_eq!(grok.input_price_per_mtok, Some(3.0));
        assert_eq!(grok.output_price_per_mtok, Some(15.0));
        // The provider's declared alias resolves to the model on offer.
        assert_eq!(resolved.aliases.get("grok-4.6-latest").map(String::as_str), Some("grok-4.6"));
    }

    #[test]
    fn a_curated_id_listed_only_as_an_alias_is_still_confirmed() {
        let body = json!({ "models": [{
            "id": "grok-4.6-0911", "created": 300, "aliases": ["grok-4.6"],
            "input_modalities": ["text"], "output_modalities": ["text"]
        }]});
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::Xai, parse_xai(&body))]));
        assert!(ids(&resolved).contains(&"grok-4.6"));
        assert!(!ids(&resolved).contains(&"grok-4.6-0911"), "the same model must not appear twice");
    }

    #[test]
    fn openai_surfaces_only_new_chat_models() {
        let body = json!({ "data": [
            { "id": "gpt-6-astra", "created": 1000 },
            { "id": "gpt-6-sol", "created": 1100 },
            { "id": "gpt-6-astra-2026-09-03", "created": 1200 },
            { "id": "text-embedding-4", "created": 1300 },
            { "id": "gpt-6-realtime", "created": 1400 },
            { "id": "gpt-6-mini-tts", "created": 1500 },
            { "id": "gpt-5.6-terra", "created": 900 }
        ]});
        let resolved = resolve(&Catalog::curated(), &Listings::from([(Provider::OpenAi, parse_openai(&body))]));
        let openai: Vec<&str> = resolved
            .models
            .iter()
            .filter(|m| m.provider == Provider::OpenAi)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(openai, vec!["gpt-6-astra", "gpt-6-sol"], "priced first, then the new one");
        let sol = resolved.models.iter().find(|m| m.id == "gpt-6-sol").unwrap();
        assert_eq!(sol.display_name, "GPT-6 Sol");
    }

    #[test]
    fn a_key_that_reaches_nothing_keeps_the_table() {
        let listings = Listings::from([
            (Provider::Anthropic, vec![]),
            (Provider::Xai, vec![]),
            (Provider::OpenAi, vec![]),
        ]);
        let resolved = resolve(&Catalog::curated(), &listings);
        assert_eq!(ids(&resolved), ids(&Catalog::curated()));
    }

    #[test]
    fn every_resolution_is_installable() {
        for listings in [
            Listings::new(),
            Listings::from([(Provider::Anthropic, anthropic_today())]),
            Listings::from([(Provider::Anthropic, vec![listed("claude-opus-6", 1)])]),
        ] {
            resolve(&Catalog::curated(), &listings).is_usable().expect("usable");
        }
    }

    #[test]
    fn anthropic_pages_parse_and_continue() {
        let body = json!({
            "data": [{
                "type": "model", "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5",
                "created_at": "2026-09-10T00:00:00Z", "max_input_tokens": 1000000, "max_tokens": 128000,
                "capabilities": { "image_input": { "supported": true } }
            }],
            "has_more": true, "last_id": "claude-opus-5-5"
        });
        let (models, next) = parse_anthropic_page(&body);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].display_name.as_deref(), Some("Opus 5.5"));
        assert_eq!(models[0].max_tokens, Some(128_000));
        assert_eq!(models[0].vision, Some(true));
        assert!(models[0].created > 0);
        assert_eq!(next.as_deref(), Some("claude-opus-5-5"));

        let (_, last) = parse_anthropic_page(&json!({ "data": [], "has_more": false }));
        assert_eq!(last, None);
    }

    #[test]
    fn dated_snapshots_are_recognised() {
        assert!(is_dated_snapshot("gpt-4o-2024-08-06"));
        assert!(is_dated_snapshot("gpt-4-0613"));
        assert!(!is_dated_snapshot("gpt-6-astra"));
        assert!(!is_dated_snapshot("o4-mini"));
    }

    #[test]
    fn made_up_names_follow_each_house_style() {
        assert_eq!(picker_name(Provider::Xai, &listed("grok-4.7", 0)), "Grok 4.7");
        assert_eq!(picker_name(Provider::OpenAi, &listed("gpt-5.6-terra", 0)), "GPT-5.6 Terra");
        assert_eq!(picker_name(Provider::OpenAi, &listed("o5-mini", 0)), "o5-mini");
        assert_eq!(picker_name(Provider::Anthropic, &listed("claude-opus-6", 0)), "Opus 6");
    }
}
