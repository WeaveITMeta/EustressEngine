# Verifies that a pinned reference machine in docs/PROMPTS/harness/reference_machines.json
# still describes the live machine this script is running on.
#
# Run from the repository root:
#   pwsh -NoProfile -File docs/PROMPTS/harness/verify_reference_machine.ps1 -Id RM-1
#
# Exits 0 only when all eleven required fields are non-empty AND the live GPU name and
# live GPU driver version match the registry entry exactly, string for string.

param(
    [Parameter(Mandatory = $true)]
    [string]$Id
)

$reg  = Get-Content docs/PROMPTS/harness/reference_machines.json -Raw | ConvertFrom-Json
$m    = $reg.machines | Where-Object { $_.id -eq $Id }
$req  = 'id','cpu','gpu','gpu_driver','ram_gb','os','os_build','wgpu_backend','rustc_version','recorded_by','recorded_utc'
$missing = $req | Where-Object { -not $m.$_ }
$live = Get-CimInstance Win32_VideoController | Select-Object -First 1
Write-Output ("missing="      + ($missing -join ','))
Write-Output ("gpu_match="    + ($m.gpu        -eq $live.Name))
Write-Output ("driver_match=" + ($m.gpu_driver -eq $live.DriverVersion))
if ($missing.Count -eq 0 -and $m.gpu -eq $live.Name -and $m.gpu_driver -eq $live.DriverVersion) { exit 0 } else { exit 1 }
