#requires -Version 7
# Exit-criterion step 2 for T3 item G6.06 — run the studio with the ribbon-sweep dump armed.
# Prints EXITCODE=<n> and exits with the studio's own exit code. The honesty gate is step 3
# (docs/PROMPTS/artifacts/G6.06/check_honesty.ps1).
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.06_check_2.ps1

param(
    [string]$Out = 'docs/PROMPTS/artifacts/G6.06/ribbon_rows.jsonl'
)

Set-Location 'E:/Workspace/EustressEngine'

$env:EUSTRESS_RIBBON_DUMP = $Out

cargo run --release -p eustress-engine --bin eustress-engine
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
