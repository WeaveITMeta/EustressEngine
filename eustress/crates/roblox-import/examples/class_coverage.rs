//! Roblox class-coverage verdict.
//!
//! Enumerates EVERY class in Roblox's own reflection dump
//! (`rbx_reflection_database` — the canonical catalog Roblox Studio
//! ships) and runs each one through the importer's
//! `class_map::roblox_to_eustress_class`. Produces a bucketed verdict so
//! we know exactly what fraction of the real Roblox class surface the
//! importer covers — and, crucially, the GAP list: creatable,
//! browsable, non-service instance classes that a real place could
//! contain but the importer does NOT yet map.
//!
//! Run:
//!   cargo run -p eustress-roblox-import --example class_coverage

use std::collections::BTreeMap;

use eustress_roblox_import::class_map::roblox_to_eustress_class;
use rbx_reflection::ClassTag;

fn main() {
    let db = rbx_reflection_database::get();

    // Buckets.
    let mut mapped: Vec<(String, String)> = Vec::new();   // (roblox, eustress)
    let mut gap: Vec<String> = Vec::new();                // creatable instance, NOT mapped — real coverage gap
    let mut services: Vec<(String, bool)> = Vec::new();   // (name, mapped?) — handled as folders
    let mut not_creatable: Vec<(String, bool)> = Vec::new();
    let mut deprecated: Vec<(String, bool)> = Vec::new();
    let mut settings: Vec<String> = Vec::new();

    // Does this class (or any ancestor) descend from `target`?
    let descends_from = |start: &str, target: &str| -> bool {
        let mut cur = Some(start.to_string());
        let mut hops = 0;
        while let Some(name) = cur {
            if name == target {
                return true;
            }
            hops += 1;
            if hops > 50 {
                break; // cycle guard
            }
            cur = db
                .classes
                .get(name.as_str())
                .and_then(|c| c.superclass.as_ref().map(|s| s.to_string()));
        }
        false
    };

    for (name, desc) in &db.classes {
        let name = name.to_string();
        let mapped_to = roblox_to_eustress_class(&name).map(|c| format!("{:?}", c));
        let is_mapped = mapped_to.is_some();

        let tags = &desc.tags;
        if tags.contains(&ClassTag::Settings)
            || tags.contains(&ClassTag::UserSettings)
        {
            settings.push(name);
            continue;
        }
        if tags.contains(&ClassTag::Service) {
            services.push((name, is_mapped));
            continue;
        }
        if tags.contains(&ClassTag::Deprecated) {
            deprecated.push((name, is_mapped));
            continue;
        }
        if tags.contains(&ClassTag::NotCreatable) {
            not_creatable.push((name, is_mapped));
            continue;
        }

        // Creatable, non-deprecated, non-service, browsable instance —
        // THIS is the population that matters for place import.
        match mapped_to {
            Some(eustress) => mapped.push((name, eustress)),
            None => gap.push(name),
        }
    }

    mapped.sort();
    gap.sort();
    services.sort();
    not_creatable.sort();
    deprecated.sort();

    let total = db.classes.len();
    let creatable_pop = mapped.len() + gap.len();
    let pct = if creatable_pop > 0 {
        (mapped.len() as f64 / creatable_pop as f64) * 100.0
    } else {
        0.0
    };

    // Categorize the mapped classes by their Eustress target family for a
    // readable breakdown.
    let mut by_family: BTreeMap<String, usize> = BTreeMap::new();
    for (_, eustress) in &mapped {
        *by_family.entry(eustress.clone()).or_default() += 1;
    }

    println!("══════════════════════════════════════════════════════════════");
    println!(" ROBLOX → EUSTRESS CLASS COVERAGE VERDICT");
    println!(" (source: rbx_reflection_database — Roblox's own class catalog)");
    println!("══════════════════════════════════════════════════════════════");
    println!(" Total Roblox classes in catalog : {}", total);
    println!("──────────────────────────────────────────────────────────────");
    println!(" CREATABLE INSTANCE CLASSES (what a place file contains):");
    println!("   mapped   : {:>4}", mapped.len());
    println!("   gap      : {:>4}  (creatable, browsable, NOT yet mapped)", gap.len());
    println!("   ───────────────");
    println!("   COVERAGE : {:.1}%  ({}/{})", pct, mapped.len(), creatable_pop);
    println!("──────────────────────────────────────────────────────────────");
    println!(" CORRECTLY-NOT-INSTANCE (handled differently / skipped):");
    let svc_mapped = services.iter().filter(|(_, m)| *m).count();
    println!("   services       : {:>4}  (become folders; {} also class-mapped)", services.len(), svc_mapped);
    println!("   not-creatable  : {:>4}  (abstract bases / internal — never in a place)", not_creatable.len());
    println!("   deprecated     : {:>4}  (legacy — importer may still map some)", deprecated.len());
    println!("   settings       : {:>4}  (Studio config — not world content)", settings.len());
    println!("──────────────────────────────────────────────────────────────");
    println!(" MAPPED — by Eustress target ({} distinct targets):", by_family.len());
    for (eustress, count) in &by_family {
        println!("   {:<22} ← {} Roblox class(es)", eustress, count);
    }
    println!("──────────────────────────────────────────────────────────────");
    if gap.is_empty() {
        println!(" GAP LIST: none — every creatable instance class maps. ✅");
    } else {
        println!(" GAP LIST ({} creatable classes not yet mapped):", gap.len());
        println!(" (each is a real place could contain it but import would skip it)");
        // Annotate each gap with whether it descends from a family we DO
        // handle — those are the highest-value gaps to close next.
        for name in &gap {
            let hint = if descends_from(name, "BasePart") {
                "  [BasePart descendant — high value]"
            } else if descends_from(name, "GuiObject") {
                "  [GuiObject descendant — high value]"
            } else if descends_from(name, "Constraint") {
                "  [Constraint descendant]"
            } else if descends_from(name, "PostEffect") {
                "  [post-FX]"
            } else {
                ""
            };
            println!("   {}{}", name, hint);
        }
    }
    println!("══════════════════════════════════════════════════════════════");
    println!(" VERDICT: {:.1}% of creatable Roblox instance classes import.", pct);
    println!(" The {} services route to folders; the {} not-creatable +", services.len(), not_creatable.len());
    println!(" {} settings are correctly excluded from world import.", settings.len());
    println!("══════════════════════════════════════════════════════════════");
}
