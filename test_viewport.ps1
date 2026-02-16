# Test script to diagnose viewport issues
Write-Host "Starting Eustress Engine with diagnostic output..." -ForegroundColor Cyan

# Run and capture specific diagnostic lines
cargo run --bin eustress-engine 2>&1 | ForEach-Object {
    $line = $_
    
    # Highlight important setup messages
    if ($line -match "(Camera|Baseplate|Welcome|viewport|scene|EustressCamera|Window resized)") {
        Write-Host $line -ForegroundColor Yellow
    }
    # Highlight errors
    elseif ($line -match "error|Error|ERROR") {
        Write-Host $line -ForegroundColor Red
    }
    # Show all other output in gray
    else {
        Write-Host $line -ForegroundColor Gray
    }
}
