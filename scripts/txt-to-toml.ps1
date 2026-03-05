# Convert .txt files to .toml files
# Usage: .\scripts\txt-to-toml.ps1 <file.txt>
# Example: .\scripts\txt-to-toml.ps1 Workspace\Baseplate.part.txt

param(
    [Parameter(Mandatory=$true)]
    [string]$InputFile
)

# Check if file exists
if (-not (Test-Path $InputFile)) {
    Write-Error "File not found: $InputFile"
    exit 1
}

# Get the file info
$file = Get-Item $InputFile

# Check if it's a .txt file
if ($file.Extension -ne ".txt") {
    Write-Error "Input file must be a .txt file"
    exit 1
}

# Create the new filename by replacing .txt with .toml
$newName = $file.FullName -replace '\.txt$', '.toml'

# Read content from .txt file
$content = Get-Content $InputFile -Raw

# Write content to .toml file
Set-Content -Path $newName -Value $content -NoNewline

# Delete the original .txt file
Remove-Item $InputFile

Write-Host "✅ Converted: $($file.Name) → $([System.IO.Path]::GetFileName($newName))" -ForegroundColor Green
Write-Host "📄 Location: $newName" -ForegroundColor Cyan
