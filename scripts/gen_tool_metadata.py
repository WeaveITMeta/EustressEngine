# -*- coding: utf-8 -*-
"""Generates eustress/crates/engine/src/tool_metadata.rs from the mode
manifests (eustress/crates/engine/modes/*.toml).

For every unique tool id referenced by any mode/submode ribbon section this
emits a display label, a tooltip (matching the built-in ribbon's short-phrase
format), and an archetype icon id (resolved to an SVG by `tool-icon()` in
ribbon.slint). Run from the repo root after editing any mode manifest:

    python scripts/gen_tool_metadata.py

A `studio_modes` test asserts every manifest tool id has an entry here, so
forgetting to regenerate fails CI rather than shipping a bare button."""
import glob
import re
import sys

try:
    import tomllib
except ImportError:  # Python < 3.11
    import tomli as tomllib

MODES_GLOB = "eustress/crates/engine/modes/*.toml"
OUT = "eustress/crates/engine/src/tool_metadata.rs"

# ── Wired ids: exact label/tooltip/icon matching the built-in ribbon ─────────
WIRED = {
    "data:chart": ("Chart", "Open a Chart tab", "viewport"),
    "data:import": ("Import", "Import CSV / JSON / Parquet", "import"),
    "data:record": ("Record", "Start the timeseries Recorder", "play"),
    "data:connect": ("Connect", "Connect a live source", "link"),
    "data:stats": ("Stats", "Descriptive statistics", "summary"),
    "data:fit": ("Fit", "Curve fit (linear / polynomial)", "brain"),
    "data:fft": ("FFT", "Spectral analysis", "sparkle"),
    "data:cluster": ("Cluster", "k-means clustering", "array-radial"),
    "data:anomaly": ("Anomaly", "Anomaly detection", "search"),
    "data:grid": ("Grid", "Open the Data Grid", "array-grid"),
    "data:timeline": ("Timeline", "Open the Recorder timeline", "history"),
    "insert:script": ("Script", "Create a Script", "new-file"),
    "insert:localscript": ("LocalScript", "Create a LocalScript", "new-file"),
    "insert:modulescript": ("ModuleScript", "Create a ModuleScript", "new-file"),
    "insert:part": ("Part", "Insert a Part", "block"),
    "insert:cad_plate": ("Plate", "Insert a parametric plate", "frame"),
    "insert:cad_box": ("Box", "Insert a parametric box", "block"),
    "insert:cad_cylinder": ("Cylinder", "Insert a parametric cylinder", "cylinder"),
    "insert:cad_plate_hole": ("Plate + Hole", "Insert a plate with hole", "frame"),
    "insert:cad_lbracket": ("L-Bracket", "Insert an L-bracket", "corner"),
    "insert:cad_frame": ("Frame", "Insert a constrained frame", "cad-frame"),
    "insert:cad_shell": ("Shell", "Insert a shelled box", "package"),
    "cad:export_glb": ("Export GLB", "Export the selected CadPart to .glb", "export"),
    "cad:solve_sketch": ("Solve Sketch", "Solve the parametric sketch", "cad-solve"),
    "cad:mate_coincident": ("Coincident", "Add a coincident mate", "link"),
    "cad:mate_revolute": ("Revolute", "Add a revolute (hinge) mate", "hinge"),
    "cad:mate_prismatic": ("Prismatic", "Add a prismatic (slider) mate", "cad-slide"),
    "cad:mate_ball": ("Ball", "Add a ball mate", "cad-ball"),
    "cad:mate_distance": ("Distance", "Add a distance mate", "ruler"),
    "csg:union": ("Union", "Boolean union", "csg-union"),
    "csg:negate": ("Negate", "Boolean subtract", "csg-subtract"),
    "csg:intersect": ("Intersect", "Boolean intersect", "csg-intersect"),
    "csg:separate": ("Separate", "Split a union", "csg-separate"),
    "terrain:toggle-visibility": ("Terrain", "Toggle terrain visibility", "terrain"),
    "terrain:clear": ("Clear Terrain", "Clear all terrain voxels", "clear"),
}

# ── Acronyms / proper nouns for label + tooltip casing ───────────────────────
ACR = {
    "gis": "GIS", "gdt": "GD&T", "bom": "BOM", "vle": "VLE", "pfd": "PFD",
    "pid": "P&ID", "hazop": "HAZOP", "fmea": "FMEA", "capa": "CAPA",
    "icf": "ICF", "soap": "SOAP", "sbar": "SBAR", "isbar": "ISBAR",
    "emar": "eMAR", "moa": "MoA", "pk": "PK", "mtm": "MTM", "sir": "SIR",
    "irac": "IRAC", "mbe": "MBE", "mpt": "MPT", "nda": "NDA", "coa": "COA",
    "opord": "OPORD", "warno": "WARNO", "cic": "CIC", "unrep": "UNREP",
    "ato": "ATO", "aco": "ACO", "jfacc": "JFACC", "mog": "MOG", "ifr": "IFR",
    "ics": "ICS", "vts": "VTS", "sar": "SAR", "fpcon": "FPCON", "fob": "FOB",
    "magtf": "MAGTF", "sead": "SEAD", "notam": "NOTAM", "tpfdd": "TPFDD",
    "colregs": "COLREGS", "npv": "NPV", "irr": "IRR", "dcf": "DCF",
    "gaap": "GAAP", "eoq": "EOQ", "otif": "OTIF", "abc": "ABC", "tam": "TAM",
    "sam": "SAM", "som": "SOM", "ltv": "LTV", "cac": "CAC", "okr": "OKR",
    "kpi": "KPI", "pestel": "PESTEL", "swot": "SWOT", "erc": "ERC",
    "pcb": "PCB", "dof": "DOF", "sn": "S-N", "nmr": "NMR", "ir": "IR",
    "sds": "SDS", "ph": "pH", "dna": "DNA", "osce": "OSCE", "ngn": "NGN",
    "iv": "IV", "sysml": "SysML", "n2": "N²", "vn": "V-n", "ppf": "PPF",
    "zopa": "ZOPA", "batna": "BATNA", "watna": "WATNA", "cle": "CLE",
    "sol": "SOL", "rfq": "RFQ", "dbq": "DBQ", "opvl": "OPVL", "rula": "RULA",
    "reba": "REBA", "pmts": "PMTS", "spc": "SPC", "rfp": "RFP", "rfa": "RFA",
    "esi": "ESI", "ab": "A/B", "cpm": "CPM", "rfi": "RFI", "npdes": "NPDES",
    "swppp": "SWPPP", "bmp": "BMP", "gnss": "GNSS", "ls": "LS", "idf": "IDF",
    "esal": "ESAL", "los": "LOS", "pt": "PT", "pe": "PE", "spt": "SPT",
    "kirchhoff": "Kirchhoff", "thevenin": "Thévenin", "bode": "Bode",
    "nash": "Nash", "phillips": "Phillips", "ashby": "Ashby",
    "redfern": "Redfern", "pareto": "Pareto", "maxwell": "Maxwell",
    "lagrangian": "Lagrangian", "turing": "Turing", "punnett": "Punnett",
    "socratic": "Socratic", "cornell": "Cornell", "bluebook": "Bluebook",
    "brady": "Brady", "atterberg": "Atterberg", "efiling": "E-Filing",
    "esign": "E-Sign", "asbuilt": "As-Built", "mvp": "MVP",
    # ── Government / municipal ──────────────────────────────────────────────
    "adu": "ADU", "cafr": "CAFR", "cip": "CIP", "sla": "SLA", "pit": "PIT",
    "hmis": "HMIS", "cdbg": "CDBG", "tif": "TIF", "ada": "ADA", "fte": "FTE",
    "rif": "RIF", "gasb": "GASB", "ems": "EMS", "pra": "PRA", "foia": "FOIA",
    "gtfs": "GTFS", "ucr": "UCR", "nibrs": "NIBRS", "roi": "ROI",
    "cpi": "CPI", "oig": "OIG", "zba": "ZBA", "ceqa": "CEQA", "nepa": "NEPA",
    "colorado": "Colorado",
}

# ── Curated tooltips for jargon-heavy / non-derivable ids ────────────────────
OVERRIDE_TIP = {
    # ── AI mode (2026-08-20) ──
    "ai:run_launcher": "Start a training run from the selected corpus",
    "ai:sweep_planner": "Plan a hyperparameter sweep across runs",
    "ai:run_compare": "Compare metrics across two or more runs",
    "ai:leaderboard": "Rank runs by the metric that matters",
    "acq:sim_recorder": "Record simulation state as training rows",
    "acq:episode_capture": "Capture a full episode as one record",
    "acq:viewport_frames": "Sample rendered frames as image data",
    "acq:state_sampler": "Sample entity state on a fixed interval",
    "acq:domain_randomize": "Randomize scene parameters for robustness",
    "acq:coverage_map": "Show which regions of the space are sampled",
    "acq:gap_finder": "Find conditions the corpus never captured",
    "cur:near_duplicate": "Find rows that are near-duplicates of each other",
    "cur:leakage_scan": "Detect rows shared between train and eval splits",
    "cur:inter_rater": "Measure agreement between annotators",
    "cur:label_propagation": "Spread labels from a labelled subset",
    "cur:stratified_split": "Split while preserving class balance",
    "prov:source_register": "Declare a source and who holds its rights",
    "prov:record_lineage": "Trace a row back through every transform",
    "prov:manifest_build": "Build the verifiable provenance manifest",
    "prov:manifest_verify": "Verify the manifest hashes end to end",
    "prov:undeclared_scan": "Find records citing an undeclared source",
    "prov:merkle_root": "Show the corpus Merkle root",
    "prov:volume_by_source": "How much of the corpus each source supplied",
    "prov:payout_preview": "Preview contributor payouts by volume share",
    "trn:objective_picker": "Choose the training objective",
    "trn:curriculum_order": "Order the corpus from easy to hard",
    "trn:learning_rate_schedule": "Shape the learning rate over training",
    "trn:overfit_detector": "Flag when validation loss diverges",
    "trn:grad_accumulation": "Trade steps for effective batch size",
    "rl:env_builder": "Turn a Space into a training environment",
    "rl:reward_designer": "Compose the reward function",
    "rl:reward_hacking_probe": "Search for reward the agent can game",
    "rl:determinism_check": "Verify the environment replays identically",
    "rl:self_play": "Train the policy against copies of itself",
    "rl:policy_export": "Export the trained policy",
    "evl:contamination_scan": "Check whether eval data leaked into training",
    "evl:slice_analysis": "Compare performance across data slices",
    "evl:regression_gate": "Block a release that regresses a benchmark",
    "evl:model_card": "Write the model card",
    "evl:reproduce_script": "Emit a script that reproduces this evaluation",
    "hub:corpus_assemble": "Assemble the corpus into an exportable set",
    "hub:shard_size": "Set rows per parquet shard",
    "hub:split_manifest": "Define train / validation / test splits",
    "hub:data_card": "Write the HuggingFace dataset card",
    "hub:task_categories": "Tag the Hub task categories",
    "hub:license_select": "Choose the dataset license",
    "hub:rights_gate": "Refuse export while any source is undeclared",
    "hub:export_dataset": "Export a HuggingFace-loadable dataset directory",
    "hub:hub_push": "Push the dataset to the Hub",
    "hub:revision_tag": "Tag this revision of the dataset",
    "hub:load_test": "Load the published dataset back and verify it",
    "jus:sol_clock": "Statute-of-limitations countdown per charge",
    "jus:brady_alerts": "Brady disclosure deadline alerts",
    "jus:hash_verifier": "Verify evidence file hashes against the custody log",
    "jus:ledger_viewer": "Browse the tamper-evident integrity ledger",
    "jus:integrity_verify": "Recompute the hash chain to verify integrity",
    "jus:proof_export": "Export a court-ready integrity proof package",
    "jus:algorithm_registry": "Registry of algorithms used in decisions",
    "jus:bias_stress_test": "Stress-test an algorithm for demographic bias",
    "jus:quasi_experiment": "Quasi-experimental reform study template",
    "jus:desistance_curves": "Plot desistance-from-crime curves over time",
    "crim:second_look_petition": "Draft a second-look sentence review petition",
    "crim:clean_slate_generator": "Generate record-clearing (clean slate) filings",
    "crim:collateral_checker": "Check collateral consequences of a conviction",
    "crim:voir_dire_tool": "Structure voir dire juror questioning",
    "jciv:meet_confer_tracker": "Track meet-and-confer obligations",
    "jciv:tentative_rulings": "Review tentative rulings before hearing",
    "jdg:departure_log": "Log sentencing guideline departures",
    "leg:bates_stamper": "Apply Bates numbers to a production set",
    "leg:esi_protocol": "Manage the ESI discovery protocol",
    "leg:good_law_checker": "Check whether cited authority is still good law",
    "leg:bluebook_checker": "Check citations against Bluebook format",
    "leg:hot_docs": "Surface the highest-relevance documents",
    "leg:predictive_coding": "Rank documents by ML-assisted relevance",
    "leg:circuit_split_detector": "Detect circuit splits on an issue",
    "leg:table_of_authorities": "Build the table of authorities",
    "leg:trust_accounting": "Client trust (IOLTA) accounting",
    "leg:disparate_impact": "Screen outcomes for disparate impact",
    "stu:cornell_notes": "Cornell-format note taking",
    "gam:level_blockout": "Grey-box a level layout quickly",
    "gam:loot_table_editor": "Edit drop rates and loot tables",
    "gam:player_heatmap": "Heatmap of player movement and deaths",
    "cstr:pt_layout": "Lay out post-tensioning tendons",
    "cgeo:spt_analyzer": "Interpret standard penetration test blow counts",
    "ctrn:esal_calculator": "Compute equivalent single-axle loads",
    "ctrn:los_analyzer": "Analyze intersection level of service",
    "cwtr:idf_curves": "Intensity-duration-frequency rainfall curves",
    "ccem:cpm_scheduler": "Critical-path-method schedule",
    "cenv:swppp_builder": "Build the stormwater pollution prevention plan",
    "csrv:least_squares_report": "Least-squares network adjustment report",
    "cutl:one_call_tracker": "Track 811 one-call locate tickets",
    "ccst:runup_calculator": "Compute wave run-up on structures",
    "mil:warno_builder": "Draft a warning order (WARNO)",
    "mil:op_overlay": "Draw an operations overlay",
    "cs:big_o_analyzer": "Analyze algorithmic Big-O complexity",
    "nurs:emar_scanner": "Scan the electronic medication administration record",
    "army:opord_builder": "Build an operations order (OPORD)",
    "phys:lagrangian_derivor": "Derive Lagrangian equations of motion",
    "econ:is_lm_model": "Interactive IS-LM macroeconomic model",
    "acct:coa_designer": "Design the chart of accounts",
    "ussf:delta_v_calculator": "Compute orbital delta-v budgets",
    "uscg:datum_calculator": "Compute the SAR search datum from drift",
}

# ── Suffix templates: (suffix, label-tail?, tooltip-template) ────────────────
SUFFIX_TIP = [
    ("visualizer", "Visualize {b}"), ("diagrammer", "Diagram {b}"),
    ("calculator", "Calculate {b}"), ("comparator", "Compare {b}"),
    ("annotator", "Annotate {b}"), ("simulator", "Simulate {b}"),
    ("navigator", "Navigate {b}"), ("generator", "Generate {b}"),
    ("optimizer", "Optimize {b}"), ("estimator", "Estimate {b}"),
    ("scheduler", "Schedule {b}"), ("collator", "Collate {b}"),
    ("composer", "Compose {b}"), ("organizer", "Organize {b}"),
    ("digitizer", "Digitize {b}"), ("predictor", "Predict {b}"),
    ("interpreter", "Interpret {b}"), ("classifier", "Classify {b}"),
    ("designer", "Design {b}"), ("analyzer", "Analyze {b}"),
    ("explorer", "Explore {b}"), ("inspector", "Inspect {b}"),
    ("selector", "Select {b}"), ("detector", "Detect {b}"),
    ("recorder", "Record {b}"), ("reviewer", "Review {b}"),
    ("verifier", "Verify {b}"), ("reducer", "Reduce {b}"),
    ("checker", "Check {b}"), ("builder", "Build {b}"),
    ("plotter", "Plot {b}"), ("grapher", "Graph {b}"),
    ("scorer", "Score {b}"), ("tracker", "Track {b}"),
    ("mapper", "Map {b}"), ("solver", "Solve {b}"),
    ("viewer", "View {b}"), ("editor", "Edit {b}"),
    ("planner", "Plan {b}"), ("monitor", "Monitor {b}"),
    ("router", "Route {b}"), ("tagger", "Tag {b}"),
    ("tester", "Test {b}"), ("trainer", "Train on {b}"),
    ("screener", "Screen {b}"), ("finder", "Find {b}"),
    ("tuner", "Tune {b}"), ("sizer", "Size {b}"),
    ("logger", "Log {b}"), ("stamper", "Stamp {b}"),
    ("adjuster", "Adjust {b}"), ("assessor", "Assess {b}"),
    ("evaluator", "Evaluate {b}"), ("delineator", "Delineate {b}"),
    ("transformer", "Transform {b}"), ("forecaster", "Forecast {b}"),
    ("assigner", "Assign {b}"), ("manager", "Manage {b}"),
    ("packager", "Package {b}"), ("publisher", "Publish {b}"),
    ("resolver", "Resolve {b}"), ("writer", "Write {b}"),
    ("processor", "Process {b}"), ("converter", "Convert {b}"),
    ("separator", "Separate {b}"), ("compiler", "Compile {b}"),
    ("leveler", "Level {b}"), ("loader", "Load {b}"),
    ("painter", "Paint {b}"), ("queue", "Work queue for {b}"),
    ("console", "Console for {b}"), ("dashboard", "Dashboard of {b}"),
    ("board", "Board for {b}"), ("matrix", "{B} matrix"),
    ("model", "Model {b}"), ("wizard", "Guided {b} workflow"),
    ("primer", "Primer on {b}"), ("reference", "{B} reference"),
    ("library", "Library of {b}"), ("drill", "Practice {b} drills"),
    ("drills", "Practice {b} drills"), ("lab", "Interactive {b} lab"),
    ("canvas", "{B} working canvas"), ("guide", "Guide to {b}"),
    ("index", "Index of {b}"), ("prep", "Prepare for {b}"),
    ("kit", "{B} toolkit"), ("chart", "{B} chart"),
    ("charts", "{B} charts"), ("curve", "{B} curve"),
    ("curves", "{B} curves"), ("diagram", "{B} diagram"),
    ("worksheet", "{B} worksheet"), ("checklist", "{B} checklist"),
    ("template", "{B} template"), ("timeline", "{B} timeline"),
    ("timer", "Time {b}"), ("feed", "Live {b} feed"),
    ("bank", "{B} bank"), ("gallery", "{B} gallery"),
    ("portal", "{B} portal"), ("inbox", "{B} inbox"),
    ("log", "Log {b}"), ("notebook", "{B} notebook"),
    ("search", "Search {b}"), ("lookup", "Look up {b}"),
    ("register", "Register of {b}"), ("registry", "Registry of {b}"),
    ("catalog", "Catalog of {b}"), ("archive", "Archive {b}"),
    ("report", "Report on {b}"), ("survey", "Survey {b}"),
    ("study", "Study {b}"), ("audit", "Audit {b}"),
    ("review", "Review {b}"), ("plan", "Plan {b}"),
    ("tool", "{B} tool"), ("tools", "{B} tools"),
]

# ── Icon assignment: keyword rules (ordered) then prefix defaults ────────────
ICON_RULES = [
    # ── AI mode (2026-08-20). First, so ML vocabulary is not absorbed by the
    # generic checklist/chart/document catch-alls further down.
    (r"reward|preference_pairs|human_feedback", "trophy"),
    (r"policy|rollout|self_play|behavior_clone|advantage", "robot"),
    (r"episode|env_builder|observation_space|action_space|step_budget", "gamepad"),
    (r"checkpoint|resume_run|snapshot_tag", "save"),
    (r"shard|parquet|corpus_assemble|split_manifest|stream_check", "package"),
    (r"data_card|model_card|pretty_name|description_editor|citation", "document"),
    (r"hub_push|export_dataset|payout_export|policy_export", "export"),
    (r"license|usage_terms|rights_gate|rights_holder|license_terms", "certificate"),
    (r"merkle|content_hash|hash_dedupe|tamper|chain_audit", "fingerprint"),
    (r"lineage|transform_log|record_lineage|provenance", "route"),
    (r"tokenizer|token_|embedding|vector_", "sigma"),
    (r"learning_rate|warmup|schedule|curriculum|early_stop", "clock"),
    (r"grad_|weight_decay|dropout|activation_stats|precision_mode", "gears"),
    (r"loss_curve|lr_trace|metric_overlay|calibration_plot", "chart-line"),
    (r"benchmark|task_battery|leaderboard|baseline_compare", "trophy"),
    (r"holdout|contamination|leakage|temporal_guard|ood_probe", "shield"),
    (r"annotat|bbox|span_marker|label_workbench|gold_set", "tag"),
    (r"randomiz|noise_inject|augment|variant_sweep|time_warp", "sparkle"),
    (r"sensor|telemetry_tap|socket_source|stream_health", "antenna"),
    (r"harvest|api_puller|bulk_loader", "import"),
    (r"distributed_plan|memory_profile|dataloader|batch_planner", "server"),
    (r"base_model|adapter_config|model_", "brain"),
    # ── Wave 2 (2026-07-23): specific archetypes FIRST, so the generic
    # catch-alls below stop absorbing most ids into checklist/chart/document.
    (r"stamp|seal|notari|bates", "stamp"),
    (r"alert|notification|warning|escalat", "bell"),
    (r"milestone|phase_|goal|checkpoint", "flag"),
    (r"score($|r)|rating|readiness|_index|capability|maturity", "gauge"),
    (r"kanban|backlog|board$", "kanban"),
    (r"pareto|histogram|distribution|frequency_analysis|bar_chart", "chart-bar"),
    (r"allocation|breakdown|_mix|share_", "chart-pie"),
    (r"manuscript|scansion|codex|ancient|epigraph", "scroll"),
    (r"mail|envelope|correspond|service_of_process|notice_", "envelope"),
    (r"label|_tag($|g)|classif", "tag"),
    (r"funnel|conversion|triage|pipeline_", "funnel"),
    (r"process_map|flow($|chart|_diagram)|pathway|value_stream", "flowchart"),
    (r"credential|permission|access_|_key$", "key"),
    (r"identity|biometric|forensic|provenance", "fingerprint"),
    (r"intake|enrollment|registration|onboard", "id-card"),
    (r"exam($|_)|certification|licensure|diploma|bar_prep|qualifying", "certificate"),
    (r"constitutional|precedent|doctrine|jurisprudence", "column"),
    (r"observator|astronom|horizon_scan", "telescope"),
    (r"magnet", "magnet"),
    (r"oscill|waveform|spectr|resonance|vibration", "wave"),
    (r"thermal|temperature|thermo_|heat_", "thermometer"),
    (r"titration|solution|solvent|reagent|_ph_", "drop"),
    (r"sustain|ecolog|green_|environment|habitat|wetland", "leaf"),
    (r"solar|weather|climate|irradiance", "sun"),
    (r"battery|energy_storage", "battery"),
    (r"utility_|power_supply|outlet", "plug"),
    (r"comms|radio|broadcast|transmit|link_budget", "antenna"),
    (r"ppe|hardhat|osha|toolbox_talk|safety_inspection", "hardhat"),
    (r"erection|lifting|rigging|heavy_lift|crane", "crane"),
    (r"pavement|highway|road_|lane|intersection", "road"),
    (r"warehouse|depot|stockpile", "warehouse"),
    (r"sku|barcode|serial|scan($|ner)", "barcode"),
    (r"automation|auto_|_bot$|ml_|ai_|predictive_coding", "robot"),
    (r"integration|orchestr|coordination|interop|mechanism", "gears"),
    (r"injection|vaccine|immuniz|infusion", "syringe"),
    (r"screening|checkup|auscult|physical_exam", "stethoscope"),
    (r"recon|scout|observation_post|lookout", "binoculars"),
    (r"routing|_route($|_)|waypoint|itinerary|deconflict", "route"),
    (r"banking|treasury|capital_|lending|escrow|trust_account", "bank"),
    (r"invoice|billing|receipt|payable|receivable|reimburs", "receipt"),
    (r"presentation|pitch_deck|briefing|showcase|demo_", "presentation"),
    (r"requirement|traceability|decomposition|trade_stud", "puzzle"),
    (r"tracker|_log($|ger)|journal|inventory", "clipboard"),
    (r"_list$|roster|directory|line_list|watch_bill", "list"),
    (r"monitor|_watch|oversight|observab", "eye"),
    # ── generic tier (fallbacks) ──
    (r"calculator|_calc$|_calc_", "calc"),
    (r"chart|graph$|grapher|plot|curve|heatmap|dashboard|kpi|trend", "chart-line"),
    (r"timeline|chronolog|era_|history|historio", "history"),
    (r"schedule|calendar|planner|lookahead|window", "calendar"),
    (r"clock|deadline|timer|speedy|sol_clock", "clock"),
    (r"registry|catalog|library|database|archive|repository|ledger|formulary", "database"),
    (r"checklist|check$|checker|rights", "checklist"),
    (r"audit|inspect|review|search|find|lookup|query|browser|screen", "search"),
    (r"map$|mapper|terrain|zone|overlay|gis|choropleth|delineator|corridor", "map"),
    (r"network|topology", "network"),
    (r"tree|hierarchy|stemma|cladogram|phylo|taxonomy", "tree"),
    (r"simulator|_sim$|wargame|monte_carlo|scenario", "atom"),
    (r"memo|note|report$|draft|letter|template|brief|worksheet|form$|document|pleading|petition|filing", "document"),
    (r"budget|cost|price|fee|financ|revenue|fund|valuation|cap_table|payment|money|economics", "coin"),
    (r"mastery|challenge|kata|competition|portfolio|medal|trophy", "trophy"),
    (r"team|collab|conference|circle|panel|jury|stakeholder|crew|users|handoff", "users"),
    (r"persona|profile|client_|patient_", "user"),
    (r"radar|detect|surveillance", "radar"),
    (r"shield|protect|security|defense|force_protection|hazard|safety|risk", "shield"),
    (r"wave|water|hydro|flood|tide|marine|coastal|drainage", "water"),
    (r"experiment|hypothesis|test_builder|ab_test|reaction|lab_", "flask"),
    (r"wizard|guide$|primer|reference$|glossary|statute|handbook|doctrine", "book"),
    (r"drafter|editor$|composer|writer|composition|essay|translation", "pen"),
    (r"satellite|orbit|space|launch|conjunction|telemetry", "satellite"),
    (r"aircraft|flight|sortie|airspace|airlift|aero", "plane"),
    (r"vessel|fleet|maritime|harbor|ship|nav_|navigation", "anchor"),
    (r"target|fires|weapon|strike|mission", "crosshair"),
    (r"med(ication)?_|dose|drug|pharma|pill|prescri", "pill"),
    (r"vital|cardiac|pulse|diagnos|clinical|therapy|care_plan", "heart-pulse"),
    (r"gene|cell|specimen|organism|dissect|biolog", "dna"),
    (r"microscope|assay|biomed|research_design", "microscope"),
    (r"campaign|brand|promo|announce|megaphone|outreach", "megaphone"),
    (r"startup|pitch|mvp|launch_|venture", "rocket"),
    (r"logistics|shipment|delivery|fleet_|truck|supply", "truck"),
    (r"facility|office|org_chart|governance|building|entity", "building"),
    (r"settle|negotiat|mediat|agreement|handshake", "handshake"),
    (r"measure|dimension|tolerance|survey|level_run|traverse", "ruler"),
    (r"electric|circuit|power|voltage|signal_|energy", "lightning"),
    (r"material|layer|soil|stratum|composite|grain", "layers"),
    (r"compass|bearing|azimuth|orienteer", "compass"),
]

PREFIX_ICON = {
    # AI mode (2026-08-20): data platform for ML/RL.
    "ai": "brain", "acq": "radar", "cur": "funnel", "prov": "fingerprint",
    "trn": "brain", "rl": "robot", "evl": "gauge", "hub": "rocket",
    "math": "sigma", "phys": "atom", "chem": "flask", "chme": "flask",
    "bio": "dna", "cs": "terminal", "hist": "book", "clas": "pen",
    "phil": "brain", "econ": "trend-up", "mech": "gears", "elec": "lightning",
    "aero": "plane", "mat": "layers", "sys": "network", "ind": "factory",
    "strat": "crosshair", "fin": "coin", "acct": "calc", "sc": "truck",
    "mktg": "megaphone", "ent": "rocket", "biz": "briefcase",
    "clin": "heart-pulse", "ph": "globe", "bmed": "microscope",
    "nurs": "checklist", "rx": "pill", "ah": "users", "hlth": "heart-pulse",
    "usmc": "crosshair", "army": "shield", "navy": "anchor",
    "uscg": "lifebuoy", "usaf": "plane", "ussf": "satellite",
    "mil": "military", "pl": "book", "sb": "briefcase", "corp": "building",
    "lit": "gavel", "med": "handshake", "arb": "scales", "leg": "scales",
    "jus": "scales", "crim": "shield", "jciv": "scales", "jdg": "gavel",
    "gam": "gamepad", "stu": "mortarboard", "cstr": "truss", "cgeo": "layers",
    "ctrn": "map", "cwtr": "water", "ccem": "build", "cenv": "globe",
    "csrv": "compass", "cutl": "lightning", "ccst": "anchor",
    # ── Government mode: shared `gov:` plus one prefix per discipline ───────
    "gov": "bank", "gexe": "flag", "gcnl": "users", "gbud": "coin",
    "gadm": "flowchart", "gclk": "stamp", "gprc": "handshake",
    "gaud": "search", "gpln": "map", "gpwk": "hardhat", "gpsa": "shield",
    "ghhs": "heart-pulse", "gcon": "lifebuoy",
}


def humanize(tokens, capitalize_first=False):
    out = []
    for i, t in enumerate(tokens):
        if t in ACR:
            out.append(ACR[t])
        elif capitalize_first or i > -1 and False:
            out.append(t)
        else:
            out.append(t)
    s = " ".join(out)
    return s


def label_for(tool_id):
    rest = tool_id.split(":", 1)[1] if ":" in tool_id else tool_id
    toks = rest.split("_")
    out = []
    for t in toks:
        if t in ACR:
            out.append(ACR[t])
        else:
            out.append(t[:1].upper() + t[1:])
    return " ".join(out)


def tooltip_for(tool_id):
    if tool_id in OVERRIDE_TIP:
        return OVERRIDE_TIP[tool_id]
    rest = tool_id.split(":", 1)[1] if ":" in tool_id else tool_id
    toks = rest.split("_")
    last = toks[-1]
    for suf, tmpl in SUFFIX_TIP:
        if last == suf and len(toks) > 1:
            base = humanize(toks[:-1])
            return tmpl.format(b=base, B=base[:1].upper() + base[1:])
    base = humanize(toks)
    return base[:1].upper() + base[1:]


def icon_for(tool_id):
    rest = tool_id.split(":", 1)[1] if ":" in tool_id else tool_id
    for pat, icon in ICON_RULES:
        if re.search(pat, rest):
            return icon
    prefix = tool_id.split(":", 1)[0] if ":" in tool_id else ""
    return PREFIX_ICON.get(prefix, "settings")


def main():
    ids = set()
    submode_icons = set()
    for path in sorted(glob.glob(MODES_GLOB)):
        with open(path, "rb") as f:
            d = tomllib.load(f)
        for sm in d.get("submodes", []):
            if sm.get("icon"):
                submode_icons.add(sm["icon"])
        for key in ("tabs", "submode_tabs"):
            for tab in d.get(key, []):
                for sec in tab.get("sections", []):
                    for t in sec.get("tools", []):
                        ids.add(t)

    rows = {}
    for tid in sorted(ids):
        if tid in WIRED:
            rows[tid] = WIRED[tid]
        else:
            rows[tid] = (label_for(tid), tooltip_for(tid), icon_for(tid))

    icons_used = sorted({r[2] for r in rows.values()} | submode_icons | {"settings"})

    def esc(s):
        return s.replace("\\", "\\\\").replace('"', '\\"')

    lines = []
    lines.append("//! GENERATED by scripts/gen_tool_metadata.py — do not edit by hand.")
    lines.append("//! Regenerate after changing any mode manifest:")
    lines.append("//!     python scripts/gen_tool_metadata.py")
    lines.append("//!")
    lines.append("//! Label + tooltip + archetype icon for every ribbon tool id referenced")
    lines.append("//! by the mode manifests (crates/engine/modes/*.toml). Wired ids mirror")
    lines.append("//! the built-in ribbon's exact labels/tooltips; aspirational (\"dream\")")
    lines.append("//! ids get derived metadata. `tool-icon()` in ribbon.slint resolves the")
    lines.append("//! icon id to an SVG; a studio_modes test asserts full coverage.")
    lines.append("")
    lines.append("/// Display metadata for one ribbon tool button.")
    lines.append("pub struct ToolMeta {")
    lines.append("    pub label: &'static str,")
    lines.append("    pub tooltip: &'static str,")
    lines.append("    pub icon: &'static str,")
    lines.append("    /// True when clicking this actually does something today.")
    lines.append("    /// False = a deliberate \"dream\" button: it renders fully but")
    lines.append("    /// has no dispatch arm, so the UI must say so honestly and the")
    lines.append("    /// click is counted as demand (see `usage_telemetry.rs`).")
    lines.append("    pub wired: bool,")
    lines.append("}")
    lines.append("")
    lines.append("/// Every icon id referenced by the table below or by a submode `icon`")
    lines.append("/// field — the allowlist `tool-icon()` in ribbon.slint must cover.")
    lines.append("pub const TOOL_ICON_IDS: &[&str] = &[")
    for ic in icons_used:
        lines.append(f'    "{ic}",')
    lines.append("];")
    lines.append("")
    lines.append(f"/// Metadata for all {len(rows)} tool ids across the mode manifests.")
    lines.append("pub fn tool_meta(id: &str) -> Option<ToolMeta> {")
    lines.append("    let (label, tooltip, icon, wired): (&'static str, &'static str, &'static str, bool) = match id {")
    for tid in sorted(rows):
        lb, tip, ic = rows[tid]
        w = "true" if tid in WIRED else "false"
        lines.append(f'        "{esc(tid)}" => ("{esc(lb)}", "{esc(tip)}", "{esc(ic)}", {w}),')
    lines.append("        _ => return None,")
    lines.append("    };")
    lines.append("    Some(ToolMeta { label, tooltip, icon, wired })")
    lines.append("}")
    lines.append("")

    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    print(f"wrote {OUT}: {len(rows)} tool ids, {len(icons_used)} icon ids")
    print("icons used:", ", ".join(icons_used))


if __name__ == "__main__":
    sys.exit(main())
