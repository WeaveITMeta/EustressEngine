#requires -Version 7
# Exit-criterion step 2 for T3 item G6.03 — the dead-module check on slint_main.rs.
# Pass: prints DEADCODE_OK and exits 0. Fail: prints DEADCODE_FAIL and exits 1.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.03_check_2.ps1

Set-Location 'E:/Workspace/EustressEngine'

$hits = @(Get-ChildItem -Recurse -Filter *.rs -Path 'eustress/crates' | Select-String -Pattern 'slint_main')
$exists = Test-Path 'eustress/crates/engine/src/ui/slint_main.rs'

Write-Output ('refs=' + $hits.Count + ' file_exists=' + $exists)

if (($hits.Count -eq 0) -and (-not $exists)) { Write-Output 'DEADCODE_OK'; exit 0 } else { Write-Output 'DEADCODE_FAIL'; exit 1 }
