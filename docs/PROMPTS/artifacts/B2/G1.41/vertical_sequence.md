# G1.41: one lead vertical, every vertical advancing

No vertical is killed or frozen. Engineering / CAD leads because it is closest to revenue today (weighted total 63; the next is research at 30; margin 33). Leading means it gets the first demo, the first outreach and the first sale. Every other vertical keeps moving, and most of what the lead needs moves them too. Scores, evidence and the measured tool counts are in `vertical_selection.json` beside this file.

| Vertical | Role | Next milestone | At full speed when |
|---|---|---|---|
| Engineering / CAD | Lead | The wired demo path (G6.40) and the SKU sheet (G1.42) | Now: first in line for every build slot |
| Manufacturing | Advancing, on the lead's tools | One part from CAD to STEP to a named vendor | A manufacturer and an investor sign a first deal |
| Research / V-Cell | Advancing | Thermal and reaction states coupled | A university signs a priced statement of work |
| Government / civic (Tucson) | Advancing | 12 wired tools and one section 5 simulation running | A paying Engineering licence funds it; WCAG AA is green |
| Creator platform | Advancing on its own track | Hosted sessions at 60 Hz, packed to 80% | Not scored here; see the last section |

## Engineering / CAD: lead

What it gets first: the fully wired demo path (G6.40), the chargeable SKU sheet (G1.42) and the first commercial licence conversation. Why it leads: 22 of 174 tool ids wired, the most of the four; 20 CAD tools registered on the MCP server (`eustress/crates/tools/src/lib.rs:149-151`); 42 tests in `eustress/crates/cad/tests/`.

## Manufacturing: advancing, on the lead's tools

Its ribbon tab runs on the CAD tools: 8 of 8 are wired (`insert:cad_*`, `csg:*`, `cad:mate_*`), so every CAD improvement lands in Manufacturing the same day. Next milestone (proposed in this record; the founder may swap it): one part carried end to end, designed with the CAD tools, exported to STEP, and recorded against a named vendor in the Space registry (`eustress/crates/engine/src/manufacturing/`). Full speed: a named manufacturer and a named investor agree in writing to a first deal, `business_banking` in the G1.40 ledger is live, and counsel signs off on the equity distribution at `docs/development/MANUFACTURING_DEAL_STRUCTURE.md:43`.

## Research / university pilot (V-Cell): advancing

The free pilot asset under `docs/marketing/` stays the door-opener, and `docs/architecture/VCELL_CASE_STUDY.md` is the walkthrough behind it. Next milestone: the V-Cell model couples its thermal and reaction states, so `docs/AUDIT/19_REALISM_PHYSICS.md:11` no longer records `ElectrochemicalState` and `ThermodynamicState` as decoupled (gauntlet G2.08 validates the cell model against a public dataset). Full speed: a university counterparty agrees to a priced statement of work, moving the G1.40 `paid_pilot` row off `spec_only`.

## Government / civic (Tucson): advancing

The largest declared surface in the tree: 563 tool ids in `government.toml`, 1 wired, and sixteen simulation designs in `docs/architecture/GOVERNMENT_MODE.md` section 5. Each simulation is shown as working software once it runs (`GOVERNMENT_MODE.md:716`). Next milestone: 12 wired ids by the G6.40 join and one section 5 simulation implemented. Full speed: a first paying Engineering licence funds it, and `docs/AUDIT/06_WEBSITE.md` Feature 15 (WCAG AA) is green.

## Creator platform (Spaces, Studio, Player, Gallery): advancing on its own track

The August brief scored four verticals. The platform most of the tree serves today (published Spaces, the desktop and browser Player, hosted multiplayer, listings, moderation) is a fifth line that this record does not score. It advances on its own track; scoring it beside the four is the founder's call.
