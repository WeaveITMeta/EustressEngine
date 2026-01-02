# Eustress Engine File Association Registration Script
# Run as Administrator to register .eustress file extension
#
# Usage:
#   .\register_file_association.ps1           # Register
#   .\register_file_association.ps1 -Remove   # Unregister

param(
    [switch]$Remove
)

$ErrorActionPreference = "Stop"

# Get the path to the engine executable
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$EnginePath = Join-Path (Split-Path -Parent $ScriptDir) "target\release\eustress-engine.exe"

# Check if running as admin
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    Write-Host "This script requires Administrator privileges." -ForegroundColor Red
    Write-Host "Please run PowerShell as Administrator and try again." -ForegroundColor Yellow
    exit 1
}

if ($Remove) {
    Write-Host "Removing Eustress file associations..." -ForegroundColor Yellow
    
    # Remove registry keys
    Remove-Item -Path "HKCR:\.eustress" -Recurse -ErrorAction SilentlyContinue
    Remove-Item -Path "HKCR:\EustressScene" -Recurse -ErrorAction SilentlyContinue
    Remove-Item -Path "HKCR:\.escene" -Recurse -ErrorAction SilentlyContinue
    
    Write-Host "File associations removed." -ForegroundColor Green
    exit 0
}

# Check if engine exists
if (-not (Test-Path $EnginePath)) {
    Write-Host "Engine not found at: $EnginePath" -ForegroundColor Red
    Write-Host "Please build the engine first with: cargo build --release" -ForegroundColor Yellow
    
    # Try debug build
    $DebugPath = Join-Path (Split-Path -Parent $ScriptDir) "target\debug\eustress-engine.exe"
    if (Test-Path $DebugPath) {
        Write-Host "Found debug build, using that instead." -ForegroundColor Yellow
        $EnginePath = $DebugPath
    } else {
        exit 1
    }
}

Write-Host "Registering Eustress file associations..." -ForegroundColor Cyan
Write-Host "Engine path: $EnginePath" -ForegroundColor Gray

# Create HKCR: PSDrive if it doesn't exist
if (-not (Test-Path "HKCR:")) {
    New-PSDrive -Name HKCR -PSProvider Registry -Root HKEY_CLASSES_ROOT | Out-Null
}

# Register .eustress extension
Write-Host "  Registering .eustress extension..." -ForegroundColor Gray
New-Item -Path "HKCR:\.eustress" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\.eustress" -Name "(Default)" -Value "EustressScene"
Set-ItemProperty -Path "HKCR:\.eustress" -Name "Content Type" -Value "application/x-eustress-scene"

# Register .escene extension (alternative)
Write-Host "  Registering .escene extension..." -ForegroundColor Gray
New-Item -Path "HKCR:\.escene" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\.escene" -Name "(Default)" -Value "EustressScene"

# Create EustressScene class
Write-Host "  Creating EustressScene class..." -ForegroundColor Gray
New-Item -Path "HKCR:\EustressScene" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene" -Name "(Default)" -Value "Eustress Scene File"

# Set icon
Write-Host "  Setting file icon..." -ForegroundColor Gray
New-Item -Path "HKCR:\EustressScene\DefaultIcon" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\DefaultIcon" -Name "(Default)" -Value "`"$EnginePath`",0"

# Create shell commands
Write-Host "  Creating shell commands..." -ForegroundColor Gray

# Open command (default double-click)
New-Item -Path "HKCR:\EustressScene\shell\open" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\shell\open" -Name "(Default)" -Value "Open with Eustress Engine"
New-Item -Path "HKCR:\EustressScene\shell\open\command" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\shell\open\command" -Name "(Default)" -Value "`"$EnginePath`" `"%1`""

# Edit command
New-Item -Path "HKCR:\EustressScene\shell\edit" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\shell\edit" -Name "(Default)" -Value "Edit with Eustress Engine"
New-Item -Path "HKCR:\EustressScene\shell\edit\command" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\shell\edit\command" -Name "(Default)" -Value "`"$EnginePath`" `"%1`""

# Play command
New-Item -Path "HKCR:\EustressScene\shell\play" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\shell\play" -Name "(Default)" -Value "Play in Eustress"
Set-ItemProperty -Path "HKCR:\EustressScene\shell\play" -Name "Icon" -Value "`"$EnginePath`",0"
New-Item -Path "HKCR:\EustressScene\shell\play\command" -Force | Out-Null
Set-ItemProperty -Path "HKCR:\EustressScene\shell\play\command" -Name "(Default)" -Value "`"$EnginePath`" --play `"%1`""

# Notify shell of changes
Write-Host "  Notifying Windows Explorer..." -ForegroundColor Gray
$code = @"
[DllImport("shell32.dll")]
public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
"@
$shell = Add-Type -MemberDefinition $code -Name "Shell32" -Namespace "Win32" -PassThru
$shell::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)

Write-Host ""
Write-Host "File associations registered successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "You can now:" -ForegroundColor Cyan
Write-Host "  - Double-click .eustress files to open them in Eustress Engine"
Write-Host "  - Right-click for 'Edit' or 'Play' options"
Write-Host ""
Write-Host "To remove associations, run: .\register_file_association.ps1 -Remove" -ForegroundColor Gray
