//! Gallery listings.
//!
//! The requests are the ones the web gallery makes
//! (`eustress/crates/web/src/api/gallery.rs`): `GET /api/gallery` with `q`,
//! `sort`, repeated `tag`, and `limit`, and `GET /api/gallery/featured`.
//! The JSON names are the Worker's (`simulations`, `author_name`, `play_count`).

use serde::Deserialize;

/// One card. Field names follow the web `GalleryExperience`, including the
/// aliases the Worker actually sends.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Card {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(alias = "author_name", default)]
    pub creator_name: String,
    #[serde(alias = "play_count", default)]
    pub visit_count: u64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tags_source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GalleryResponse {
    #[serde(alias = "simulations")]
    experiences: Vec<Card>,
}

#[derive(Debug, Deserialize)]
struct FeaturedResponse {
    featured: Vec<Card>,
}

/// The path `gallery_endpoint` builds, for a search box and a limit.
/// Tags are repeated `tag=` keys, as the Worker expects.
pub fn gallery_path(query: &str, limit: u32) -> String {
    let mut endpoint = "/api/gallery?".to_string();
    if !query.is_empty() {
        endpoint.push_str(&format!("q={}&", encode(query)));
    }
    endpoint.push_str(&format!("limit={limit}&"));
    endpoint
}

/// Featured row, then the search page. A failed featured call still returns
/// the search page: the gallery is usable when that route is down.
pub fn fetch(api_base: &str, query: &str) -> Result<Vec<Card>, String> {
    let base = api_base.trim().trim_end_matches('/');
    let agent = agent();
    let mut cards = Vec::new();
    if query.is_empty() {
        let url = format!("{base}/api/gallery/featured");
        match agent.get(&url).call() {
            Ok(response) => {
                let body: FeaturedResponse = response.into_json().map_err(|e| format!("featured: {e}"))?;
                cards.extend(body.featured);
            }
            Err(ureq::Error::Status(status, _)) => {
                eprintln!("gallery: featured returned {status}");
            }
            Err(ureq::Error::Transport(err)) => return Err(format!("could not reach the gallery: {err}")),
        }
    }
    let url = format!("{base}{}", gallery_path(query, 40));
    let response = agent.get(&url).call().map_err(http_err)?;
    let body: GalleryResponse = response.into_json().map_err(|e| format!("gallery: {e}"))?;
    for card in body.experiences {
        if !cards.iter().any(|have| have.id == card.id) {
            cards.push(card);
        }
    }
    Ok(cards)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(4))
        .timeout(std::time::Duration::from_secs(20))
        .redirects(0)
        .build()
}

fn http_err(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, _) => format!("the gallery returned {code}"),
        ureq::Error::Transport(err) => format!("could not reach the gallery: {err}"),
    }
}

/// Gallery name as a `space/<slug>` path: lower case, words joined by `-`.
pub fn slug_of(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The published Space a `space/<slug>` link names.
///
/// An exact gallery id wins. Otherwise the listing whose name slug equals
/// the link, or starts with it at a word boundary (`box-head` matches
/// "Box Head Zombie Defense Tycoon"). Two listings at the same closeness
/// are not a guess.
pub fn pick_published<'a>(slug: &str, cards: &'a [Card]) -> Result<&'a Card, String> {
    let slug = slug.trim().trim_matches('/').to_ascii_lowercase();
    if slug.is_empty() {
        return Err("the link names no space".into());
    }
    if let Some(exact) = cards.iter().find(|card| card.id.eq_ignore_ascii_case(&slug)) {
        return Ok(exact);
    }
    let mut hits: Vec<&Card> = cards
        .iter()
        .filter(|card| {
            let name = slug_of(&card.name);
            name == slug || name.starts_with(&format!("{slug}-"))
        })
        .collect();
    if hits.is_empty() {
        return Err(format!("no published Space matches {slug}"));
    }
    hits.sort_by_key(|card| slug_of(&card.name).len());
    let best = slug_of(&hits[0].name).len();
    if hits.iter().filter(|card| slug_of(&card.name).len() == best).count() > 1 {
        return Err(format!("several published Spaces match {slug}"));
    }
    Ok(hits[0])
}

fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gallery_path_matches_the_web_client_for_a_plain_search() {
        assert_eq!(gallery_path("", 40), "/api/gallery?limit=40&");
        assert_eq!(gallery_path("harbor", 40), "/api/gallery?q=harbor&limit=40&");
    }

    #[test]
    fn a_worker_payload_reads_with_its_own_field_names() {
        let body = r#"{"simulations":[{"id":"abc","name":"Harbor","author_name":"Ada","play_count":3,"tags":["sailing"],"tags_source":"author"}]}"#;
        let parsed: GalleryResponse = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.experiences[0].creator_name, "Ada");
        assert_eq!(parsed.experiences[0].visit_count, 3);
        let featured: FeaturedResponse =
            serde_json::from_str(r#"{"featured":[{"id":"abc","name":"Harbor"}]}"#).unwrap();
        assert_eq!(featured.featured[0].name, "Harbor");
    }

    #[test]
    fn a_space_slug_picks_the_published_listing() {
        let cards = vec![
            Card {
                id: "abc".into(),
                name: "Box Head Zombie Defense Tycoon".into(),
                description: String::new(),
                creator_name: String::new(),
                visit_count: 0,
                tags: Vec::new(),
                tags_source: None,
            },
            Card {
                id: "def".into(),
                name: "Harbor".into(),
                description: String::new(),
                creator_name: String::new(),
                visit_count: 0,
                tags: Vec::new(),
                tags_source: None,
            },
        ];
        assert_eq!(slug_of(&cards[0].name), "box-head-zombie-defense-tycoon");
        let picked = pick_published("box-head", &cards).unwrap();
        assert_eq!(picked.id, "abc");
        assert_eq!(pick_published("ABC", &cards).unwrap().id, "abc");
        assert!(pick_published("missing", &cards).is_err());
        let tied = vec![
            Card { id: "1".into(), name: "Box Head Alpha".into(), ..cards[1].clone() },
            Card { id: "2".into(), name: "Box Head Bravo".into(), ..cards[1].clone() },
        ];
        assert!(pick_published("box-head", &tied).is_err());
    }
}
