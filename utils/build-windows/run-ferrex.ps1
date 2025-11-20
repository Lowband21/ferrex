# Ferrex Player launcher for Windows

Write-Host "Starting Ferrex Player..." -ForegroundColor Green

# Set GStreamer plugin path
$env:GST_PLUGIN_PATH = "$PSScriptRoot\lib\gstreamer-1.0"

# Add bin directory to PATH
$env:PATH = "$PSScriptRoot\bin;$env:PATH"

# Handle server URL
if ($args.Count -gt 0) {
    $env:FERREX_SERVER_URL = $args[0]
    Write-Host "Connecting to server: $($args[0])" -ForegroundColor Yellow
} elseif (-not $env:FERREX_SERVER_URL) {
    Write-Host "`nNOTE: No server URL specified." -ForegroundColor Yellow
    Write-Host "To connect to a server, either:"
    Write-Host "  1. Set FERREX_SERVER_URL environment variable"
    Write-Host "  2. Run: .\run-ferrex.ps1 http://your-server:3000`n"
}

# Launch the application
try {
    & "$PSScriptRoot\ferrex-player.exe"
} catch {
    Write-Host "Error launching Ferrex Player: $_" -ForegroundColor Red
    Read-Host "Press Enter to exit"
}
