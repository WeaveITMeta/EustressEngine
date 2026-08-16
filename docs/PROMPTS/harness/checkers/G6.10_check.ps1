#requires -Version 7
# Exit-criterion step 1 for T3 item G6.10 — run the studio with the panel-state dump armed.
# Exits with the studio's own exit code. The coverage gate is step 2
# (docs/PROMPTS/artifacts/G6.10/check_states.ps1).
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.10_check.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.10/panel_states.jsonl'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_PANEL_STATE_DUMP = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
