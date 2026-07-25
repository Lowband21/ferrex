# Ferrex Player launcher for Windows

Write-Host "Starting Ferrex Player..." -ForegroundColor Green

# Use only the bundled GStreamer plugins and out-of-process scanner.
$pluginPath = "$PSScriptRoot\lib\gstreamer-1.0"
$scanner = "$PSScriptRoot\libexec\gstreamer-1.0\gst-plugin-scanner.exe"
$env:GST_PLUGIN_PATH = $pluginPath
$env:GST_PLUGIN_PATH_1_0 = $pluginPath
$env:GST_PLUGIN_SYSTEM_PATH_1_0 = $pluginPath
$env:GST_PLUGIN_SCANNER = $scanner
$env:GST_PLUGIN_SCANNER_1_0 = $scanner
$env:GIO_EXTRA_MODULES = "$PSScriptRoot\lib\gio\modules"
$env:SSL_CERT_FILE = "$PSScriptRoot\etc\ssl\certs\ca-certificates.crt"
$localData = if ($env:LOCALAPPDATA) {
    $env:LOCALAPPDATA
} else {
    [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
}
$registryDirectory = Join-Path $localData 'Ferrex\gstreamer-1.0'
New-Item -ItemType Directory -Force -Path $registryDirectory | Out-Null
$env:GST_REGISTRY_1_0 = Join-Path $registryDirectory 'registry.bin'

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
