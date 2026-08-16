#requires -Version 7
# Exit-criterion step 1 for T3 item G6.03 — the positive drain-contract run.
# Tees the full test output to the artifact directory, prints EXITCODE=<n>, and
# exits with the cargo test exit code.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.03_check_1.ps1

param(
    [string]$Out = '../docs/PROMPTS/artifacts/G6.03/positive_run.txt'
)

Set-Location 'E:/Workspace/EustressEngine/eustress'

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null

cargo test -p eustress-engine --lib ui::drain_contract -- --test-threads=1 --nocapture 2>&1 | Tee-Object -FilePath $Out
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
