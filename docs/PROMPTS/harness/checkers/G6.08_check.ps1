#requires -Version 7
# Exit-criterion step 1 for T3 item G6.08 — the existing default-binding test must still pass.
# Prints EXITCODE=<n> and exits with the cargo test exit code.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.08_check.ps1

Set-Location 'E:/Workspace/EustressEngine/eustress'

cargo test -p eustress-engine --lib keybindings -- --test-threads=1
$code = $LASTEXITCODE

Write-Output ('EXITCODE=' + $code)
exit $code
