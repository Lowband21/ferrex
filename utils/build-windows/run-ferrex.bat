@echo off
echo Starting Ferrex Player...

REM Use only the bundled GStreamer plugins and scanner
set GST_PLUGIN_PATH=%~dp0lib\gstreamer-1.0
set GST_PLUGIN_PATH_1_0=%~dp0lib\gstreamer-1.0
set GST_PLUGIN_SYSTEM_PATH_1_0=%~dp0lib\gstreamer-1.0
set GST_PLUGIN_SCANNER=%~dp0libexec\gstreamer-1.0\gst-plugin-scanner.exe
set GST_PLUGIN_SCANNER_1_0=%~dp0libexec\gstreamer-1.0\gst-plugin-scanner.exe
set GIO_EXTRA_MODULES=%~dp0lib\gio\modules
set SSL_CERT_FILE=%~dp0etc\ssl\certs\ca-certificates.crt
if not defined LOCALAPPDATA set LOCALAPPDATA=%TEMP%
if not exist "%LOCALAPPDATA%\Ferrex\gstreamer-1.0" mkdir "%LOCALAPPDATA%\Ferrex\gstreamer-1.0"
set GST_REGISTRY_1_0=%LOCALAPPDATA%\Ferrex\gstreamer-1.0\registry.bin

REM Add bin directory to PATH for DLLs
set PATH=%~dp0bin;%PATH%

REM Set server URL if provided as argument
if not "%1"=="" (
    set FERREX_SERVER_URL=%1
    echo Connecting to server: %1
) else (
    if not defined FERREX_SERVER_URL (
        echo.
        echo NOTE: No server URL specified.
        echo To connect to a server, either:
        echo   1. Set FERREX_SERVER_URL environment variable
        echo   2. Run: run-ferrex.bat http://your-server:3000
        echo.
    )
)

REM Launch the application
"%~dp0ferrex-player.exe"

pause
