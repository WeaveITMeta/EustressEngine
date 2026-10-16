// =============================================================================
// Eustress Web - Economics, start from my projects (/economics)
// =============================================================================
// Beside the Small, Medium and Large presets, a signed-in creator can start the creator's sliders from their own projects: they pick
// some of their Spaces and the page reads what the Worker has on them.
//
//   GET /api/projects                                   each Space's visits (play_count) and when it was published
//   GET /api/commerce/purchases?sim_id=...&limit=100    the Space's own sales, newest first (live mode)
//
// Players a month are the visits since publishing spread over the months since, summed over the picked Spaces; buyers, conversion
// and Tickets per buyer come from the sales of the last 30 days. Nothing else is read, and nothing leaves the page: the sliders are set
// as if the creator had moved them.
//
// Table of Contents:
// 1. Attribution
// 2. Tests
// =============================================================================

use std::collections::BTreeSet;

use serde::Deserialize;

use super::economics_model::{control, Key};

// -----------------------------------------------------------------------------
// 1. Attribution
// -----------------------------------------------------------------------------

/// One of the creator's Spaces as `/api/projects` lists it.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct MyProject {
    pub id: String,
    pub name: String,
    pub play_count: u64,
    pub created_at: String,
    pub published_at: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct MyProjects {
    pub projects: Vec<MyProject>,
}

/// One sale of a picked Space: who bought, how many Tickets, when (Unix seconds), and whether it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct MySale {
    pub buyer: String,
    pub tickets: u64,
    pub created: i64,
    pub refunded: bool,
}

/// What the picked Spaces set the creator's sliders to, and the words that say where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribution {
    pub players: f64,
    pub conversion: f64,
    pub average: f64,
    pub note: String,
}

const DAY_SECS: f64 = 86_400.0;
const MONTH_SECS: f64 = 30.0 * DAY_SECS;

/// Seconds since the epoch for an RFC 3339 time, or None.
fn secs(rfc3339: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339).ok().map(|t| t.timestamp() as f64)
}

/// The creator's sliders from the picked Spaces and their sales, at `now` (Unix seconds).
pub fn attribute(picked: &[MyProject], sales: &[MySale], now: f64) -> Attribution {
    let mut players = 0.0;
    for p in picked {
        let since = p.published_at.as_deref().and_then(secs).or_else(|| secs(&p.created_at)).unwrap_or(now);
        let months = ((now - since) / MONTH_SECS).max(1.0);
        players += p.play_count as f64 / months;
    }
    let recent: Vec<&MySale> = sales.iter().filter(|s| !s.refunded && (now - s.created as f64) <= MONTH_SECS).collect();
    let buyers: BTreeSet<&str> = recent.iter().map(|s| s.buyer.as_str()).collect();
    let tickets: u64 = recent.iter().map(|s| s.tickets).sum();
    let players = control(Key::Players).from_position(control(Key::Players).position(players.round()));
    let conv_max = control(Key::Conversion).max;
    let conversion = if players > 0.0 { (100.0 * buyers.len() as f64 / players).min(conv_max) } else { 0.0 };
    // the sliders' own steps, so the page shows what the creator could have set
    let step = |key: Key, v: f64| {
        let c = control(key);
        ((v / c.step).round() * c.step).clamp(c.min, c.max)
    };
    let average = if buyers.is_empty() { 0.0 } else { step(Key::Average, tickets as f64 / buyers.len() as f64) };
    let names = picked.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ");
    let note = format!(
        "From {}: visits since publishing spread over the months since, and {} sales to {} buyers in the last 30 days.",
        if names.is_empty() { "no Spaces".to_string() } else { names },
        recent.len(),
        buyers.len()
    );
    Attribution { players, conversion: step(Key::Conversion, conversion), average, note }
}

// -----------------------------------------------------------------------------
// 2. Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: f64 = 1_791_400_000.0; // 2026-10-07

    fn space(name: &str, plays: u64, published_days_ago: f64) -> MyProject {
        let at = chrono::DateTime::from_timestamp((NOW - published_days_ago * DAY_SECS) as i64, 0).unwrap().to_rfc3339();
        MyProject { id: name.to_lowercase(), name: name.into(), play_count: plays, created_at: at.clone(), published_at: Some(at), status: "approved".into() }
    }

    fn sale(buyer: &str, tickets: u64, days_ago: f64) -> MySale {
        MySale { buyer: buyer.into(), tickets, created: (NOW - days_ago * DAY_SECS) as i64, refunded: false }
    }

    #[test]
    fn visits_spread_over_the_months_since_publishing_and_sum_over_the_picked_spaces() {
        // 60,000 visits over 6 months is 10,000 a month; 2,000 visits in 10 days counts as one month
        let a = attribute(&[space("Box Head", 60_000, 180.0), space("Pong", 2_000, 10.0)], &[], NOW);
        assert_eq!(a.players, 12_000.0);
        assert_eq!((a.conversion, a.average), (0.0, 0.0), "no sales: nobody converts");
        assert!(a.note.starts_with("From Box Head, Pong:") && a.note.contains("0 sales to 0 buyers"), "{}", a.note);
    }

    #[test]
    fn buyers_and_tickets_come_from_the_last_30_days_and_refunds_do_not_count() {
        let sales = vec![
            sale("a", 300, 1.0),
            sale("a", 200, 2.0),
            sale("b", 100, 29.0),
            sale("c", 900, 31.0), // too old
            MySale { refunded: true, ..sale("d", 5_000, 3.0) },
        ];
        let a = attribute(&[space("Box Head", 30_000, 90.0)], &sales, NOW);
        assert_eq!(a.players, 10_000.0);
        assert_eq!(a.conversion, 0.0, "2 buyers of 10,000 is 0.02%, which the 0.5% slider shows as 0");
        assert_eq!(a.average, 300.0, "600 Tickets over 2 buyers is 300, on the slider's 25-Ticket steps 300");
        assert!(a.note.contains("3 sales to 2 buyers"), "{}", a.note);
        // a small Space with real buyers
        let many: Vec<MySale> = (0..40).map(|i| sale(&format!("u{i}"), 150, 5.0)).collect();
        let b = attribute(&[space("Pong", 2_000, 60.0)], &many, NOW);
        assert_eq!(b.players, 1_000.0);
        assert_eq!(b.conversion, 4.0, "40 buyers of 1,000 players");
        assert_eq!(b.average, 150.0);
    }

    #[test]
    fn the_numbers_land_on_what_the_sliders_can_show() {
        // players land on the log slider's two figures; conversion is held to its maximum
        let a = attribute(&[space("Hit", 1_234_567, 30.0)], &[], NOW);
        assert_eq!(a.players, 1_200_000.0);
        let crowd: Vec<MySale> = (0..500).map(|i| sale(&format!("u{i}"), 25, 1.0)).collect();
        let b = attribute(&[space("Tiny", 100, 30.0)], &crowd, NOW);
        assert_eq!(b.conversion, control(Key::Conversion).max, "more buyers than players is held to the slider's top");
        // a Space never published counts from its creation; one with no dates counts as one month
        let mut draft = space("Draft", 3_000, 0.0);
        draft.published_at = None;
        draft.created_at = chrono::DateTime::from_timestamp((NOW - 90.0 * DAY_SECS) as i64, 0).unwrap().to_rfc3339();
        assert_eq!(attribute(&[draft], &[], NOW).players, 1_000.0);
        let undated = MyProject { name: "X".into(), play_count: 700, ..Default::default() };
        assert_eq!(attribute(&[undated], &[], NOW).players, 700.0);
    }

    #[test]
    fn the_projects_list_parses_as_the_worker_sends_it() {
        let json = r#"{"projects":[{"id":"07d0","name":"Box Head","play_count":3,"favorite_count":1,"created_at":"2026-09-01T00:00:00Z","published_at":null,"status":"approved","genre":"Horror"}],"total":1,"page":1,"limit":100}"#;
        let p: MyProjects = serde_json::from_str(json).unwrap();
        assert_eq!((p.projects[0].name.as_str(), p.projects[0].play_count, p.projects[0].published_at.clone()), ("Box Head", 3, None));
    }
}
