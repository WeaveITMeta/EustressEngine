<#
.SYNOPSIS
  Run the G7.01 tool-surface census end to end with an engine the census owns.

.DESCRIPTION
  Builds nothing. Expects these release binaries in eustress\target\release:
    eustress-headless.exe      cargo build --release -p eustress-engine --bin eustress-headless
    eustress-mcp.exe           cargo build --release -p eustress-mcp-server --bin eustress-mcp
    eustress-agent-census.exe  cargo build --release -p eustress-agent-eval --bin eustress-agent-census

  1. Makes the fixture under -Workspace (unless it exists).
  2. Runs the census with --headless-bin, so the engine's stderr lands in
     docs\PROMPTS\artifacts\G7.01\census_stderr.log (the panic evidence) and
     a crashed engine is restarted rather than ending the run.
  3. Prints EXIT=<code>.

  -SwapStudioLastSpace passes --swap-studio-last-space: needed when Studio's
  settings name a real Space (the census refuses otherwise, exit 3). Close
  Studio first; the settings file is restored byte for byte at exit.
#>
param(
    [string]$Workspace = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\..')) '.eustress-fixture'),
    [int]$Repetitions = 5,
    [int]$TimeoutS = 30,
    [switch]$SwapStudioLastSpace
)

$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$bin = Join-Path $repo 'eustress\target\release'
$universe = Join-Path $Workspace 'CensusUniverse'

if (-not (Test-Path -LiteralPath (Join-Path $universe 'Spaces'))) {
    & (Join-Path $PSScriptRoot 'make_census_fixture.ps1') -Out $Workspace -McpBin (Join-Path $bin 'eustress-mcp.exe')
}

$env:EUSTRESS_WORKSPACE = $Workspace
$censusArgs = @(
    '--universe', $universe,
    '--repetitions', "$Repetitions",
    '--timeout-s', "$TimeoutS",
    '--headless-bin', (Join-Path $bin 'eustress-headless.exe'),
    '--mcp-bin', (Join-Path $bin 'eustress-mcp.exe'),
    '--repo-root', $repo,
    '--out', (Join-Path $repo 'docs\PROMPTS\artifacts\G7.01\tool_conformance.json')
)
if ($SwapStudioLastSpace) { $censusArgs += '--swap-studio-last-space' }

& (Join-Path $bin 'eustress-agent-census.exe') @censusArgs
$code = $LASTEXITCODE
Write-Host "EXIT=$code"
exit $code
