#requires -Version 7
# Exit-criterion step for T3 item G6.02 — arm EUSTRESS_UI_TRACE and run the studio.
# Exits with the studio's own exit code. The gate on the emitted CSV is step 3
# (docs/PROMPTS/artifacts/G6.02/analyze_trace.ps1).
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.02_check.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.02/ui_trace.csv'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_UI_TRACE = '1'
$env:EUSTRESS_UI_TRACE_OUT = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
