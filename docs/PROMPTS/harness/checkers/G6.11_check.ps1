#requires -Version 7
# Exit-criterion step 2 for T3 item G6.11 — run the studio with the focus trace armed so the
# Tab / Shift+Tab traversal of the ten named panels is recorded. Exits with the studio's own
# exit code. The a11y gate is step 3 (docs/PROMPTS/artifacts/G6.11/check_a11y.ps1).
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.11_check.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.11/focus_trace.jsonl'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_FOCUS_TRACE = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
