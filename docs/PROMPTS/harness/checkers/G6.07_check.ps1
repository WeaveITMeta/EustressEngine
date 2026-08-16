#requires -Version 7
# Exit-criterion step 3 for T3 item G6.07 — run the studio with the frozen G6.02 instrument
# armed so palette keystroke latency can be derived. Exits with the studio's own exit code.
# The latency gate is docs/PROMPTS/artifacts/G6.07/palette_latency.ps1.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.07_check.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.07/palette_trace.csv'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_UI_TRACE = '1'
$env:EUSTRESS_UI_TRACE_OUT = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
