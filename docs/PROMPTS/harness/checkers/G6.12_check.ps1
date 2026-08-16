#requires -Version 7
# Exit-criterion steps 1 and 2 for T3 item G6.12 — run the studio with the layout dump armed.
# Step 1 uses the default -Out (layout_before.jsonl); step 2 reruns the identical script with
# -Out docs/PROMPTS/artifacts/G6.12/layout_after.jsonl. Using one script for both halves is
# deliberate: the before and after numbers must not come from different measurement code.
# Exits with the studio's own exit code. The density gate is step 3
# (docs/PROMPTS/artifacts/G6.12/check_density.ps1).
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.12_check.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.12/layout_before.jsonl'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_LAYOUT_DUMP = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
