# Proof Standard

Status: normative for every validation report produced by items G2.30 through G2.37.

A claim about Eustress is re-runnable when an engineer who has never spoken to anyone at Eustress can
pin the environment, run one exact command, compare against a declared tolerance, name where the
uncertainty comes from, and verify a content hash of the result. A claim whose command cannot be run
by a stranger is not a claim, it is an assertion. Assertions do not appear in validation reports.

Eustress is an AI-native simulation substrate and world engine. It is source-available under
PolyForm Shield 1.0.0.

## 1. Required fields

A report carries one fenced block whose info string is `proof`. Each line of that block is
`dotted.field: value`. Every field below is required. Each has a machine check that runs with no
network access; `docs/PROMPTS/harness/checkers/report_lint.py` implements every check named here.

| Field | Meaning | Machine check | Example value |
|---|---|---|---|
| `claim.text` | The claim in one sentence, carrying its label inline as `[MEASURED]`, `[TARGET]` or `[CONFIG_DEFAULT]` | `check:inline_label` | `[MEASURED] Two runs from one seed end in identical state.` |
| `environment.toolchain` | Rust toolchain version, equal to the channel in `eustress/rust-toolchain.toml` | `check:toolchain` | `1.100.0` |
| `environment.os` | Operating system name and version, including a digit | `check:os` | `Windows 11 Pro 10.0.26200` |
| `environment.cpu` | CPU model and core count | `check:nonempty` | `AMD Ryzen 9 7950X, 16 cores` |
| `environment.gpu` | GPU and driver where the measurement touches the GPU, otherwise the literal `not_applicable` | `check:nonempty` | `not_applicable` |
| `environment.cargo_features` | Cargo features enabled for the command, comma separated, or the literal `none` | `check:features` | `physics` |
| `measurement.command` | One exact command, run from the repository root, starting `cd eustress &&`, using `cargo run` or `cargo test`, never `cargo check`, and naming every feature in `environment.cargo_features` after `--features` | `check:command` | `cd eustress && cargo test -p eustress-common --features physics --test determinism` |
| `measurement.source_path` | Repository path of the test or benchmark the command runs; the file must exist | `check:path_exists` | `eustress/crates/common/tests/determinism.rs` |
| `result.label` | `MEASURED`, `TARGET` or `CONFIG_DEFAULT`; must equal the label tag in `claim.text` | `check:label_enum` | `MEASURED` |
| `result.value` | The result as a literal string | `check:nonempty` | `hash_a == hash_b` |
| `result.tolerance` | `exact`, `abs <number> <unit>`, or `rel <number>%` | `check:tolerance` | `exact` |
| `result.uncertainty_source` | At least three words naming where the uncertainty comes from | `check:uncertainty` | `fixed seed and fixed step count remove sampling noise` |
| `result.content_hash` | `sha256:` plus 64 hex digits, equal to the SHA-256 of the UTF-8 bytes of `result.value` | `check:content_hash` | `sha256:` and 64 hex digits |

The linter derives the field list by reading the table above, so the document and the check count
cannot drift apart. `--list-required-fields` prints one field name per line. A check id in the table
that the linter does not implement is an error, never a silent pass.

## 2. Labels

Every number a report states is labelled `MEASURED`, `TARGET` or `CONFIG_DEFAULT` inline.
`MEASURED` means produced by running the named command on the named environment. `TARGET` means the
project intends to reach it. `CONFIG_DEFAULT` means it is a shipped default, not an observation. A
`TARGET` is never presented as a measurement.

## 3. Environment pinning

- Toolchain: the version in `eustress/rust-toolchain.toml`. The linter compares when that file is
  present.
- OS and version, CPU, and GPU with driver version when the measurement touches the GPU.
- Cargo features. Some tests compile to nothing without their feature. `determinism.rs` is
  `#![cfg(feature = "physics")]`; run without `--features physics` it reports zero tests and looks
  like success. A report that omits its feature list is not reproducible.
- Run from the `eustress/` workspace. The workspace shares one `target/` directory, so run one cargo
  build at a time. Do not kill a build mid-compile.
- Validate with `cargo run` or `cargo test`. `cargo check` does not catch plugin-registration
  failures and is rejected.

## 4. Tolerance and uncertainty

The tolerance is declared before the run, in the report, in one of three forms: `exact`,
`abs <number> <unit>`, or `rel <number>%`. The uncertainty source names the physical or procedural
origin of any spread: seed, step count, wall clock, thermal state, scheduler, driver. "Unknown" is
not a source.

## 5. Content hash

`result.content_hash` is the SHA-256 of the UTF-8 bytes of the `result.value` string, after
stripping surrounding whitespace. A reader recomputes it with no network. A result artifact larger
than a line (a log, a CSV) is referenced from `result.value` by repository path and its own hash.

## 6. What the standard does not assume

No hosted dashboard, no metrics stack, no signed binary, and no green CI badge is required or
consulted. CI does not run `cargo test`; the engineer runs the command. The linter checks form and
internal consistency. Re-running the command is the engineer's job, and a result that fails to
reproduce falsifies the claim.

## 7. Worked reference

`docs/validation/reports/EXAMPLE_conforming.md` is built against
`eustress/crates/common/tests/determinism.rs`. Report G2.33 follows its shape.
