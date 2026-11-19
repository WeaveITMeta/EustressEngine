# Runs the spike's measurement matrix, one process per run, and appends one
# JSON line per run to $Results. Screenshots go to $OutDir.
#
#   pwsh -File measure.ps1 -Exe C:\EustressBuild\target\release\ui-gpu-spike.exe -OutDir <dir>
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Size = '3456x929',
    [int]$Frames = 600,
    [int]$Warmup = 180,
    [int]$Repeat = 3
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $OutDir | Out-Null
$Results = Join-Path $OutDir 'results.jsonl'

function Run([string]$Label, [string[]]$SpikeArgs) {
    $all = @('--size', $Size, '--frames', $Frames, '--warmup', $Warmup, '--results', $Results, '--label', $Label) + $SpikeArgs
    Write-Host "== $Label :: $($SpikeArgs -join ' ')"
    & $Exe @all 2>> (Join-Path $OutDir 'stderr.log')
    if ($LASTEXITCODE -ne 0) { Write-Host "   exit $LASTEXITCODE" }
}

for ($r = 1; $r -le $Repeat; $r++) {
    foreach ($scenario in 'idle', 'sweep', 'full') {
        # Studio's path as Studio runs it (pipelined rendering on).
        Run "studio-sw-$scenario-pipe-r$r" @('--renderer', 'software', '--ui', 'studio', '--scenario', $scenario, '--pipelined', 'on')
        # The same with pipelining off, the setting the GPU path needs on Vulkan.
        Run "studio-sw-$scenario-nopipe-r$r" @('--renderer', 'software', '--ui', 'studio', '--scenario', $scenario, '--pipelined', 'off')
        Run "studio-gpu-$scenario-nopipe-r$r" @('--renderer', 'gpu', '--ui', 'studio', '--scenario', $scenario, '--pipelined', 'off')
    }
    # CPU + GPU time of one full UI draw, each renderer waited on.
    Run "studio-gpu-full-sync-r$r" @('--renderer', 'gpu', '--ui', 'studio', '--scenario', 'full', '--pipelined', 'off', '--sync')
    # Glass: blur on and off over the live scene, then the overlay layout.
    Run "glass-gpu-under-blur24-r$r" @('--renderer', 'gpu', '--ui', 'glass', '--scene-under', '--scenario', 'full', '--blur', '24')
    Run "glass-gpu-under-blur0-r$r" @('--renderer', 'gpu', '--ui', 'glass', '--scene-under', '--scenario', 'full', '--blur', '0')
    Run "glass-gpu-under-blur24-sync-r$r" @('--renderer', 'gpu', '--ui', 'glass', '--scene-under', '--scenario', 'full', '--blur', '24', '--sync')
    Run "glass-gpu-under-blur0-sync-r$r" @('--renderer', 'gpu', '--ui', 'glass', '--scene-under', '--scenario', 'full', '--blur', '0', '--sync')
    Run "glass-sw-r$r" @('--renderer', 'software', '--ui', 'glass', '--scenario', 'idle')
    Run "glass-gpu-overlay-r$r" @('--renderer', 'gpu', '--ui', 'glass', '--scenario', 'idle')
}

# Once each: the oversized-text shapes, the D3D12 backend, and Vulkan with
# pipelining left on (not a safe configuration; see spike.rs).
Run 'textprobe-sw-full' @('--renderer', 'software', '--ui', 'textprobe', '--scenario', 'full', '--frames', '120')
Run 'textprobe-gpu-full' @('--renderer', 'gpu', '--ui', 'textprobe', '--scenario', 'full', '--frames', '120')
Run 'studio-gpu-full-dx12-pipe' @('--renderer', 'gpu', '--ui', 'studio', '--scenario', 'full', '--backend', 'dx12', '--pipelined', 'on')
Run 'studio-gpu-full-dx12-nopipe' @('--renderer', 'gpu', '--ui', 'studio', '--scenario', 'full', '--backend', 'dx12', '--pipelined', 'off')
Run 'studio-sw-full-dx12-pipe' @('--renderer', 'software', '--ui', 'studio', '--scenario', 'full', '--backend', 'dx12', '--pipelined', 'on')
Run 'studio-gpu-full-vulkan-pipe-UNSAFE' @('--renderer', 'gpu', '--ui', 'studio', '--scenario', 'full', '--pipelined', 'on')
'DONE' | Set-Content (Join-Path $OutDir 'measure.done')
