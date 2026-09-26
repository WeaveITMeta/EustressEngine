<#
.SYNOPSIS
Stages the Windows install layout that the release zip and the installer both ship.

.DESCRIPTION
    <stage>\eustress-engine.exe            the editor
    <stage>\eustress-lsp.exe               the Rune language server the editor starts
    <stage>\assets\                        crates/engine/assets: icons, lighting templates, shaders
    <stage>\common\assets\                 crates/common/assets: materials, characters, class schemas, service templates, the part meshes
    <stage>\docs\eustress_constitution.pdf opened by Help > Constitution

The engine looks for each of these beside its executable first:
`eustress_engine::resource_root`, `eustress_common::assets_dir`, the avatar
`bundled://` root and the `AssetPlugin` root all prefer the exe-adjacent copy.

Assets come from `git ls-files`, so the stage holds exactly what is committed
and none of the untracked files a developer checkout carries. Authoring
sources the engine never reads (FBX, Python, `scripts/` folders, the Linux
install scripts) are left out.

.EXAMPLE
pwsh installer/windows/stage.ps1 -BinDir eustress/target/release
#>
param(
    # Directory holding the built eustress-engine.exe and eustress-lsp.exe.
    [Parameter(Mandatory)] [string] $BinDir,
    # Where to build the layout. Emptied first.
    [string] $OutDir = (Join-Path $PSScriptRoot '..\..\dist\windows\stage')
)

$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$BinDir = (Resolve-Path $BinDir).Path

if (Test-Path $OutDir) { Remove-Item $OutDir -Recurse -Force }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path

foreach ($exe in 'eustress-engine.exe', 'eustress-lsp.exe') {
    $src = Join-Path $BinDir $exe
    if (-not (Test-Path $src)) { throw "$exe is not in $BinDir" }
    Copy-Item $src $OutDir
}

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

$authoring = '(^|/)scripts/|\.(fbx|py|pyc|blend\d*)$'
Copy-Tracked 'eustress/crates/engine/assets' (Join-Path $OutDir 'assets') "^linux/|$authoring"
Copy-Tracked 'eustress/crates/common/assets' (Join-Path $OutDir 'common\assets') $authoring

New-Item -ItemType Directory -Force (Join-Path $OutDir 'docs') | Out-Null
Copy-Item (Join-Path $repo 'docs\documents\eustress_constitution.pdf') (Join-Path $OutDir 'docs')

# The engine only treats these as its install layout when they are present,
# so a stage missing one would quietly fall back to the build machine's paths.
foreach ($must in 'assets\icons', 'common\assets\parts', 'assets\lighting_templates', 'assets\shaders',
                  'common\assets\class_schema', 'common\assets\characters\y_bot.glb') {
    if (-not (Test-Path (Join-Path $OutDir $must))) { throw "stage is missing $must" }
}

$files = Get-ChildItem $OutDir -Recurse -File
"{0,6} files  {1:N1} MB  staged in {2}" -f $files.Count, (($files | Measure-Object Length -Sum).Sum / 1MB), $OutDir
