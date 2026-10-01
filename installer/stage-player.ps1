#Requires -Version 7.0
<#
.SYNOPSIS
Stages the Eustress Player layout that every Player package ships, on Windows, macOS or Linux.

.DESCRIPTION
    <stage>/eustress-client[.exe]   the Player
    <stage>/common/assets/          crates/common/assets: materials, characters, class schemas, service templates

The Player looks for its assets beside its executable first:
`eustress_common::assets_dir`, the avatar `bundled://` root and the Player's
`AssetPlugin` root all prefer `<exe dir>/common/assets`.

Assets come from `git ls-files`, so the stage holds exactly what is committed
and none of the untracked files a developer checkout carries. Authoring
sources the Player never reads (FBX, Python, Blender files, `scripts/`
folders) are left out.

Each platform wraps this layout in its own package: the Windows zip and
installer (installer/windows/eustress-player.iss), the macOS app bundle and the
Linux archive with its desktop entry (.github/workflows/player-release.yml).

.EXAMPLE
pwsh installer/stage-player.ps1 -BinDir eustress/target/release
#>
param(
    # Directory holding the built eustress-client executable.
    [Parameter(Mandatory)] [string] $BinDir,
    # Where to build the layout. Emptied first.
    [string] $OutDir = (Join-Path $PSScriptRoot '../dist/player-stage')
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$BinDir = (Resolve-Path $BinDir).Path

if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

$exe = if ($IsWindows) { 'eustress-client.exe' } else { 'eustress-client' }
$src = Join-Path $BinDir $exe
if (-not (Test-Path $src)) { throw "$exe is not in $BinDir" }
Copy-Item $src $OutDir

# Copies the tracked files under `$From` (a repo-relative path) into `$To`,
# skipping any whose path inside `$From` matches `$Skip`.
function Copy-Tracked([string] $From, [string] $To, [string] $Skip) {
    # `-z` and `core.quotepath=off` pass names with spaces through verbatim.
    $list = git -C $repo -c core.quotepath=off ls-files -z -- $From
    if ($LASTEXITCODE -ne 0) { throw "git ls-files failed for $From" }
    $copied = 0
    foreach ($rel in (($list -join "`n") -split "`0")) {
        if (-not $rel) { continue }
        $inner = $rel.Substring($From.Length).TrimStart('/')
        if ($inner -match $Skip) { continue }
        $dest = Join-Path $To $inner
        New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
        Copy-Item -LiteralPath (Join-Path $repo $rel) -Destination $dest
        $copied++
    }
    if ($copied -eq 0) { throw "nothing staged from $From" }
    "{0,6} files  {1}" -f $copied, $From
}

Copy-Tracked 'eustress/crates/common/assets' (Join-Path $OutDir 'common/assets') '(^|/)scripts/|\.(fbx|py|pyc|blend\d*)$'

# The Player only treats these as its install layout when they are present, so
# a stage missing one would quietly fall back to the build machine's paths.
foreach ($must in 'common/assets/class_schema', 'common/assets/characters/y_bot.glb') {
    if (-not (Test-Path (Join-Path $OutDir $must))) { throw "stage is missing $must" }
}

$files = Get-ChildItem $OutDir -Recurse -File
"{0,6} files  {1:N1} MB  staged in {2}" -f $files.Count, (($files | Measure-Object Length -Sum).Sum / 1MB), $OutDir
