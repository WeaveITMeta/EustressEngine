#requires -Version 7
# Exit-criterion step 1 for T3 item G6.13 — line ceiling and drain uniqueness.
# Pass: prints SPLIT_OK and exits 0. Fail: prints SPLIT_FAIL and exits 1.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.13_check_1.ps1

Set-Location 'E:/Workspace/EustressEngine'

$files = Get-ChildItem -Recurse -Filter *.rs -Path 'eustress/crates/engine/src/ui'
$max = ($files | ForEach-Object { [pscustomobject]@{ N = $_.FullName; L = @(Get-Content $_.FullName).Count } } | Sort-Object L -Descending | Select-Object -First 1)
$drains = (Get-ChildItem -Recurse -Filter *.rs -Path 'eustress/crates/engine/src' | Select-String -Pattern '\.in_set\(SlintSystems::Drain\)' | Measure-Object).Count
Write-Output ('max_file=' + $max.N + ' lines=' + $max.L)
Write-Output ('drain_registrations=' + $drains)

if (($max.L -le 3000) -and ($drains -eq 1)) { Write-Output 'SPLIT_OK'; exit 0 } else { Write-Output 'SPLIT_FAIL'; exit 1 }
