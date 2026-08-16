#requires -Version 7
# Exit-criterion checker for T3 item G6.01 — Studio UX baseline census.
# Pass: prints CENSUS_OK and exits 0. Fail: prints CENSUS_FAIL and exits 1.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.01_check.ps1

Set-Location 'E:/Workspace/EustressEngine'

$c = Get-Content 'docs/PROMPTS/artifacts/G6.01/ux_baseline_census.json' -Raw -ErrorAction SilentlyContinue | ConvertFrom-Json -ErrorAction SilentlyContinue
if ($null -eq $c) {
    Write-Output 'missing=<census file absent or unparseable>'
    Write-Output 'CENSUS_FAIL'
    exit 1
}

$req = @(
    'slint_file_count',
    'slint_total_lines',
    'ui_rs_total_lines',
    'largest_ui_file',
    'drain_required_params',
    'ribbon_tools_total',
    'ribbon_tools_wired',
    'slint_files_with_accessible_role',
    'top20_command_click_depth',
    'theme_selection_tokens',
    'panel_inventory'
)
$missing = @($req | Where-Object { -not ($c.PSObject.Properties.Name -contains $_) })

$a = (@(Get-ChildItem 'eustress/crates/engine/ui/slint/*.slint')).Count
$b = (Select-String -Path 'eustress/crates/engine/src/tool_metadata.rs' -Pattern ', (true|false)\),$' | Measure-Object).Count
$d = (Select-String -Path 'eustress/crates/engine/src/tool_metadata.rs' -Pattern ', true\),$' | Measure-Object).Count
$e = (Get-ChildItem 'eustress/crates/engine/ui/slint/*.slint' | Select-String -Pattern 'accessible-role' | Select-Object -ExpandProperty Path -Unique | Measure-Object).Count

$ok = ($missing.Count -eq 0) -and
      ($c.slint_file_count -eq $a) -and
      ($c.ribbon_tools_total -eq $b) -and
      ($c.ribbon_tools_wired -eq $d) -and
      ($c.slint_files_with_accessible_role -eq $e)

Write-Output ('missing=' + ($missing -join ','))
Write-Output ('slint_file_count ' + $c.slint_file_count + ' vs ' + $a)
Write-Output ('ribbon_tools_total ' + $c.ribbon_tools_total + ' vs ' + $b)
Write-Output ('ribbon_tools_wired ' + $c.ribbon_tools_wired + ' vs ' + $d)
Write-Output ('accessible_role_files ' + $c.slint_files_with_accessible_role + ' vs ' + $e)

if ($ok) { Write-Output 'CENSUS_OK'; exit 0 } else { Write-Output 'CENSUS_FAIL'; exit 1 }
