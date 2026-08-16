#requires -Version 7
# Exit-criterion step 2 for T3 item G6.13 — the frozen drain-contract regression test.
# Prints EXITCODE=<n> and exits with the cargo test exit code.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.13_check_2.ps1

Set-Location 'E:/Workspace/EustressEngine/eustress'

cargo test -p eustress-engine --lib ui::drain_contract -- --test-threads=1
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
