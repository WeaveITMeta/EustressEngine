# Validation report (format reference): same seed, same world

This report is the worked reference for `docs/validation/PROOF_STANDARD.md`. It is built against the
existing test `eustress/crates/common/tests/determinism.rs`. The result below is a format
illustration labelled `TARGET`: the item that wrote it compiles nothing, so the test was not run to
produce it. A real report labels its result `MEASURED` only after the command has been run and its
output captured.

```proof
claim.text: [TARGET] Two runs of the Avian scene from one GlobalRngSeed end in byte-identical quantized state after 120 fixed steps at 60 Hz.
environment.toolchain: 1.100.0
environment.os: Windows 11 Pro 10.0.26200
environment.cpu: reference CPU, to be filled by the engineer who runs it
environment.gpu: not_applicable
environment.cargo_features: physics
measurement.command: cd eustress && cargo test -p eustress-common --features physics --test determinism
measurement.source_path: eustress/crates/common/tests/determinism.rs
result.label: TARGET
result.value: same_seed_same_world passes: hash_a == hash_b
result.tolerance: exact
result.uncertainty_source: fixed seed and fixed step count remove sampling noise
result.content_hash: sha256:8d6e0f1b546c5d753e4c877151134e0ef0022c6e629da30284821a5d26b18466
```

Without `--features physics` the test file compiles to nothing and reports zero tests. That is why the
feature list is a required field.
