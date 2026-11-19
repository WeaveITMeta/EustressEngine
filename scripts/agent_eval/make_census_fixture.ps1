<#
.SYNOPSIS
  Generate the fixture world the G7.01 tool-surface census runs against.

.DESCRIPTION
  No Space is committed to this repository, so the census builds its own:

    <Out>\CensusUniverse\Spaces\CensusSpace     the Space the engine opens (first by name)
    <Out>\CensusUniverse\Spaces\CensusSpace2    a second Space, for open_space
    <Out>\CensusUniverseB\Spaces\CensusSpaceB   a second Universe, for set_active_universe
    <Out>\census_fixture.json                   what was generated, and when

  Each Space is scaffolded by the MCP server's own `new_space` tool (the
  structure Studio's New Space makes: service folders, manifests, Baseplate,
  WelcomeCube, Lighting children), called with the Universe's absolute path so
  it writes nowhere else. The fixture Universe is then made a git repository
  with one commit (repository-local identity) so the git tools have history.

  Needs a built eustress-mcp binary:
    cargo build --release -p eustress-mcp-server --bin eustress-mcp

.PARAMETER Out
  The fixture workspace root (use it as EUSTRESS_WORKSPACE for the engine).

.PARAMETER McpBin
  The eustress-mcp binary. Default: eustress\target\release\eustress-mcp.exe.

.PARAMETER Force
  Delete and regenerate the three fixture Universes if they exist. Nothing
  else under -Out is touched.
#>
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$McpBin = (Join-Path $PSScriptRoot '..\..\eustress\target\release\eustress-mcp.exe'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$Out = [System.IO.Path]::GetFullPath($Out)
$McpBin = [System.IO.Path]::GetFullPath($McpBin)
if (-not (Test-Path -LiteralPath $McpBin -PathType Leaf)) {
    throw "eustress-mcp not found at $McpBin. Build it: cargo build --release -p eustress-mcp-server --bin eustress-mcp"
}

$universeA = Join-Path $Out 'CensusUniverse'
$universeB = Join-Path $Out 'CensusUniverseB'
foreach ($u in @($universeA, $universeB)) {
    if (Test-Path -LiteralPath $u) {
        if (-not $Force) { throw "$u exists. Pass -Force to regenerate the fixture." }
        Remove-Item -LiteralPath $u -Recurse -Force
    }
}

# The Universe skeleton new_universe makes.
foreach ($u in @($universeA, $universeB)) {
    foreach ($sub in @('.eustress\assets\meshes', '.eustress\assets\parts', '.eustress\knowledge', 'Spaces')) {
        New-Item -ItemType Directory -Force -Path (Join-Path $u $sub) | Out-Null
    }
}

# ---- Drive eustress-mcp over stdio -------------------------------------
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $McpBin
$psi.ArgumentList.Add('--universe')
$psi.ArgumentList.Add($universeA)
$psi.UseShellExecute = $false
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.Environment['EUSTRESS_UNIVERSE'] = $universeA
$psi.Environment['EUSTRESS_UNIVERSES_PATH'] = $Out
$psi.Environment['EUSTRESS_WORKSPACE'] = $Out
$psi.Environment['EUSTRESS_ROOT'] = $Out
$psi.Environment['RUST_LOG'] = 'warn'

$proc = [System.Diagnostics.Process]::Start($psi)
# Drain stderr so the pipe never fills.
$stderrTask = $proc.StandardError.ReadToEndAsync()

function Send-Frame($obj) {
    $line = $obj | ConvertTo-Json -Compress -Depth 20
    $proc.StandardInput.WriteLine($line)
    $proc.StandardInput.Flush()
}

function Read-Reply([int]$id, [int]$timeoutMs = 60000) {
    $deadline = [DateTime]::UtcNow.AddMilliseconds($timeoutMs)
    while ([DateTime]::UtcNow -lt $deadline) {
        $task = $proc.StandardOutput.ReadLineAsync()
        $left = [int]([Math]::Max(1, ($deadline - [DateTime]::UtcNow).TotalMilliseconds))
        if (-not $task.Wait($left)) { break }
        $line = $task.Result
        if ($null -eq $line) { throw "eustress-mcp exited before answering request $id" }
        try { $msg = $line | ConvertFrom-Json } catch { continue }
        if ($msg.PSObject.Properties.Name -contains 'id' -and $msg.id -eq $id) { return $msg }
    }
    throw "eustress-mcp did not answer request $id within $timeoutMs ms"
}

function Invoke-NewSpace([int]$id, [string]$name, [string]$universe) {
    Send-Frame @{ jsonrpc = '2.0'; id = $id; method = 'tools/call'; params = @{ name = 'new_space'; arguments = @{ name = $name; universe = $universe } } }
    $reply = Read-Reply $id
    if ($reply.PSObject.Properties.Name -contains 'error') { throw "new_space $name failed: $($reply.error.message)" }
    if ($reply.result.isError) { throw "new_space $name failed: $(($reply.result.content | ForEach-Object { $_.text }) -join ' ')" }
    $space = Join-Path (Join-Path $universe 'Spaces') $name
    if (-not (Test-Path -LiteralPath (Join-Path $space 'Workspace'))) { throw "new_space $name reported success but $space\Workspace does not exist" }
    Write-Host "created $space"
}

try {
    Send-Frame @{ jsonrpc = '2.0'; id = 1; method = 'initialize'; params = @{ protocolVersion = '2025-06-18'; capabilities = @{}; clientInfo = @{ name = 'make_census_fixture'; version = '1' } } }
    Read-Reply 1 | Out-Null
    Send-Frame @{ jsonrpc = '2.0'; method = 'notifications/initialized'; params = @{} }
    Invoke-NewSpace 2 'CensusSpace' $universeA
    Invoke-NewSpace 3 'CensusSpace2' $universeA
    Invoke-NewSpace 4 'CensusSpaceB' $universeB
}
finally {
    # The server exits on stdin EOF.
    $proc.StandardInput.Close()
    if (-not $proc.WaitForExit(10000)) {
        Write-Warning "eustress-mcp (pid $($proc.Id), started by this script) did not exit; stopping it"
        $proc.Kill()
    }
    $null = $stderrTask.Wait(2000)
}

# ---- Fixture content the synthesized inputs point at --------------------
$space = Join-Path $universeA 'Spaces\CensusSpace'
Set-Content -LiteralPath (Join-Path $space 'census_fixture.txt') -Value 'Census fixture file. read_file and list_directory probes read this.' -Encoding utf8

# ---- git history for the git tools --------------------------------------
$git = Get-Command git -ErrorAction SilentlyContinue
if ($git) {
    & git -C $universeA init -q
    & git -C $universeA -c user.name=census -c user.email=census@localhost add -A
    & git -C $universeA -c user.name=census -c user.email=census@localhost commit -q -m 'census fixture'
} else {
    Write-Warning 'git not found: the fixture Universe has no repository, so the git tools will report that'
}

$manifest = [ordered]@{
    schema_version = 1
    generator      = 'scripts/agent_eval/make_census_fixture.ps1'
    created_at     = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    mcp_bin        = $McpBin
    universe       = $universeA
    universe_b     = $universeB
    spaces         = @('CensusUniverse/Spaces/CensusSpace', 'CensusUniverse/Spaces/CensusSpace2', 'CensusUniverseB/Spaces/CensusSpaceB')
    entities       = @('Workspace/Baseplate', 'Workspace/WelcomeCube', 'Lighting/Atmosphere', 'Lighting/Clouds', 'Lighting/Moon', 'Lighting/Sky', 'Lighting/Sun')
    git            = [bool]$git
}
$manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $Out 'census_fixture.json') -Encoding utf8
Write-Host "fixture ready: $universeA"
