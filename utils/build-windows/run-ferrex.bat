@echo off
echo Starting Ferrex Player...

REM Set GStreamer plugin path
set GST_PLUGIN_PATH=%~dp0lib\gstreamer-1.0

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
