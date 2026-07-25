Ferrex Player for Windows
========================

Requirements:
- Windows 10 or later (64-bit)
- DirectX 11 compatible graphics

Quick Start:
1. Double-click "run-ferrex.bat" to start the player
2. Set your server URL when prompted, or:
   - Set FERREX_SERVER_URL environment variable
   - Run: run-ferrex.bat http://your-server:3000

Troubleshooting:
- If you see DLL errors, ensure all files in the 'bin' folder are present
- For video playback issues, update your graphics drivers
- Check that your antivirus isn't blocking the executable

Advanced Usage:
- Use run-ferrex.ps1 for better error messages
- Set GST_DEBUG=3 for GStreamer debugging
- Check logs in %APPDATA%\ferrex-player\

Native mpv presenter status:
- The package's presenter mode is recorded in
  share\ferrex-player\PRESENTER_BUILD_MODE
- "spike" contains the developer-only Win32 owned-overlay presenter; "disabled"
  is the control/release build and falls back without attempting attachment
- The production/Auto gate remains disabled until the Windows hardware, HDR,
  focus, fullscreen, taskbar, and lifecycle test matrix passes
- Developers rebuilding the package must set
  FERREX_MPV_WINDOWS_PRESENTER=spike before cargo build
- Presenter failure falls back deterministically to mpv's native window

Support:
Visit https://github.com/Lowband21/ferrex for issues and discussions
