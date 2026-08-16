#requires -Version 7
# Exit-criterion step 3 for T3 item G6.13 — run the studio with the frozen G6.02 instrument
# armed on the same scene and seed G6.04 used. Exits with the studio's own exit code. The
# latency band is gated by docs/PROMPTS/artifacts/G6.04/gate_drain.ps1.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.13_check_3.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.13/ui_trace_postsplit.csv'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_UI_TRACE = '1'
$env:EUSTRESS_UI_TRACE_OUT = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
