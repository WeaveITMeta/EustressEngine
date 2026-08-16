#requires -Version 7
# Exit-criterion step 2 for T3 item G6.04 — run the four interaction sequences with the
# frozen G6.02 instrument armed. Exits with the studio's own exit code. The drain gate is
# step 3 (docs/PROMPTS/artifacts/G6.04/gate_drain.ps1).
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.04_check_1.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.04/ui_trace_after.csv'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_UI_TRACE = '1'
$env:EUSTRESS_UI_TRACE_OUT = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
