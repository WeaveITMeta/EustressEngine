#requires -Version 7
# Exit-criterion step 1 for T3 item G6.06 — derive the wired/unwired ground truth from the
# generated tool_metadata.rs table. Prints truth_wired and truth_unwired and exits 0.
# Invoked as: pwsh -NoProfile -File docs/PROMPTS/harness/checkers/G6.06_check_1.ps1

Set-Location 'E:/Workspace/EustressEngine'

$t = (Select-String -Path 'eustress/crates/engine/src/tool_metadata.rs' -Pattern ', true\),$' | Measure-Object).Count
$f = (Select-String -Path 'eustress/crates/engine/src/tool_metadata.rs' -Pattern ', false\),$' | Measure-Object).Count

Write-Output ('truth_wired=' + $t + ' truth_unwired=' + $f)
exit 0
