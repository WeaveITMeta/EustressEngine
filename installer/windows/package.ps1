<#
.SYNOPSIS
Packages a built Windows engine for release: install layout, zip, installer, checksums, latest.json.

.DESCRIPTION
The manual counterpart of the Windows and publish jobs in .github/workflows/release.yml,
for releases made by hand (PC only, no GitHub Actions). Run from a checkout of the
release commit, so the staged assets are that commit's tracked files.

Everything lands in dist\v<Version>\:
    eustress-engine-v<Version>-windows-x64.zip   what the in-app updater unpacks over an install
    EustressEngine-Setup.exe                     what a visitor downloads
    checksums.txt, latest.json                   the manifest, same shape CI writes
    stage\                                       the layout both of them hold

The whole manual release, in order:

  1. Build (static C runtime, thin LTO; the flag goes through --config because a
     RUSTFLAGS variable would replace .cargo/config.toml's lld-link flavour):

       $env:CARGO_TARGET_DIR = '<somewhere>\target-dist'; $env:CARGO_INCREMENTAL = '0'
       cargo build --locked --offline --profile dist -p eustress-engine `
         --bin eustress-engine --bin eustress-lsp --target x86_64-pc-windows-msvc `
         --config 'target.x86_64-pc-windows-msvc.rustflags=["-C","target-feature=+crt-static"]'

  2. This script, with -BinDir <target dir>\x86_64-pc-windows-msvc\dist.
  3. Upload each file to the eustress-downloads R2 bucket under v<Version>/ with
     `wrangler r2 object put eustress-downloads/v<Version>/<file> --file <file> --remote`,
     then latest.json LAST, with `--content-type application/json
     --cache-control "public, max-age=60"`. Fetch each back and compare checksums.
  4. downloads.eustress.dev serves /latest.json only once infrastructure/cloudflare's
     download worker is deployed.

wrangler's single upload tops out near 300 MiB; the zip is the larger file.

.EXAMPLE
pwsh installer/windows/package.ps1 -BinDir E:\build\target-dist\x86_64-pc-windows-msvc\dist
#>
param(
    # Directory holding the built eustress-engine.exe and eustress-lsp.exe.
    [Parameter(Mandatory)] [string] $BinDir,
    # Defaults to the engine crate's version, which the updater compares against.
    [string] $Version,
    [string] $Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
)
$ErrorActionPreference = 'Stop'

if (-not $Version) {
    $Version = (Select-String -Path "$Repo\eustress\crates\engine\Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"' |
        Select-Object -First 1).Matches[0].Groups[1].Value
}
$out    = Join-Path $Repo "dist\v$Version"
$stage  = "$out\stage"
$zip    = "eustress-engine-v$Version-windows-x64.zip"
$setup  = 'EustressEngine-Setup.exe'
$commit = (git -C $Repo rev-parse HEAD).Trim()
if (git -C $Repo status --porcelain -- eustress/crates installer) {
    Write-Warning 'Uncommitted changes under eustress/crates or installer: the staged assets are the committed files, not these.'
}

if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Force $out | Out-Null

& pwsh -NoProfile -File "$PSScriptRoot\stage.ps1" -BinDir $BinDir -OutDir $stage
if ($LASTEXITCODE -ne 0) { throw "stage.ps1 failed ($LASTEXITCODE)" }

Push-Location $stage
& 7z a -tzip -mx=9 "$out\$zip" * | Select-Object -Last 2
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "7z failed ($code)" }

& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" "/DMyAppVersion=$Version" "/DStageDir=$stage" "/O$out" "$PSScriptRoot\eustress-engine.iss" *> "$out\iscc.log"
if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE); see $out\iscc.log" }

# No BOM: the updater parses latest.json with serde_json.
$base = "https://downloads.eustress.dev/v$Version"
function Entry([string] $file, [bool] $installer) {
    $p = Join-Path $out $file
    [ordered]@{
        file       = $file
        url        = "$base/$file"
        sha256     = (Get-FileHash $p -Algorithm SHA256).Hash.ToLower()
        size_bytes = (Get-Item $p).Length
        installer  = $installer
    }
}
$win  = Entry $zip $false
$inst = Entry $setup $true
"$($win.sha256)  $zip`n$($inst.sha256)  $setup`n" | Set-Content "$out\checksums.txt" -Encoding utf8NoBOM -NoNewline
[ordered]@{
    version   = $Version
    date      = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
    channel   = 'stable'
    commit    = $commit
    platforms = [ordered]@{
        'windows-x64'           = $win
        'windows-x64-installer' = $inst
    }
} | ConvertTo-Json -Depth 5 | Set-Content "$out\latest.json" -Encoding utf8NoBOM

Get-ChildItem $out -File | Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,1)}} | Format-Table -AutoSize
