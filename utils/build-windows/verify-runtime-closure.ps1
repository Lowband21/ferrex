param(
    [Parameter(Mandatory = $true)]
    [string]$StageRoot
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path $StageRoot).Path
$bin = Join-Path $root 'bin'
$pluginDir = Join-Path $root 'lib\gstreamer-1.0'
$gioModuleDir = Join-Path $root 'lib\gio\modules'
$scanner = Join-Path $root 'libexec\gstreamer-1.0\gst-plugin-scanner.exe'
$gstInspect = Join-Path $bin 'gst-inspect-1.0.exe'
$gstLaunch = Join-Path $bin 'gst-launch-1.0.exe'
$certificateBundle = Join-Path $root 'etc\ssl\certs\ca-certificates.crt'
$system32 = Join-Path $env:SystemRoot 'System32'
$dumpbin = (Get-Command dumpbin.exe -ErrorAction Stop).Source

$presenterModePath = Join-Path $root 'share\ferrex-player\PRESENTER_BUILD_MODE'
if (-not (Test-Path $presenterModePath -PathType Leaf)) {
    throw 'Presenter build-mode metadata is missing from the staged artifact'
}
$presenterMode = (Get-Content $presenterModePath -Raw).Trim()
if ($presenterMode -notin @('spike', 'disabled')) {
    throw "Invalid presenter build mode in staged artifact: $presenterMode"
}
if ($env:FERREX_MPV_WINDOWS_PRESENTER -and
    $presenterMode -ne $env:FERREX_MPV_WINDOWS_PRESENTER) {
    throw "Staged presenter mode '$presenterMode' does not match build mode '$env:FERREX_MPV_WINDOWS_PRESENTER'"
}

if (-not (Test-Path (Join-Path $root 'ferrex-player.exe'))) {
    throw "ferrex-player.exe is missing from $root"
}
if (-not (Get-ChildItem $bin -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -in @('libmpv-2.dll', 'mpv-2.dll', 'mpv.dll') })) {
    throw "libmpv runtime DLL is missing from $bin"
}
if (-not (Test-Path $pluginDir -PathType Container) -or
    -not (Get-ChildItem $pluginDir -File -Filter '*.dll' -ErrorAction SilentlyContinue)) {
    throw "Bundled GStreamer plugins are missing from $pluginDir"
}
if (-not (Test-Path $scanner -PathType Leaf)) {
    throw "Bundled GStreamer plugin scanner is missing: $scanner"
}
if (-not (Test-Path $gstInspect -PathType Leaf)) {
    throw "Bundled GStreamer inspection tool is missing: $gstInspect"
}
if (-not (Test-Path $gstLaunch -PathType Leaf)) {
    throw "Bundled GStreamer launch smoke tool is missing: $gstLaunch"
}
foreach ($gioModuleRoot in @('gioopenssl', 'giolibproxy')) {
    $matches = @(
        @(
            (Join-Path $gioModuleDir "$gioModuleRoot.dll"),
            (Join-Path $gioModuleDir "lib$gioModuleRoot.dll")
        ) | Where-Object { Test-Path $_ -PathType Leaf }
    )
    if ($matches.Count -ne 1) {
        throw "Expected exactly one bundled GIO network module for '$gioModuleRoot'; found $($matches.Count)"
    }
}
if (-not (Test-Path $certificateBundle -PathType Leaf)) {
    throw "Bundled CA certificate bundle is missing: $certificateBundle"
}
$gstProfilePath = Join-Path $root 'share\licenses\gstreamer\FERREX_BUILD_PROFILE'
if (-not (Test-Path $gstProfilePath -PathType Leaf)) {
    throw 'Pinned GStreamer build profile/notices are missing from the artifact'
}
$gstPluginRootsPath = Join-Path $root 'share\licenses\gstreamer\PLUGIN_ROOTS'
$gstPluginAllowlistPath = Join-Path $root 'share\licenses\gstreamer\PLUGIN_ALLOWLIST'
$gstRuntimeHashesPath = Join-Path $root 'share\licenses\gstreamer\RUNTIME_FILES.sha256'
$gstRuntimeProvenancePath = Join-Path $root 'share\licenses\gstreamer\RUNTIME_PROVENANCE.tsv'
foreach ($requiredPath in @(
    $gstPluginRootsPath,
    $gstPluginAllowlistPath,
    $gstRuntimeHashesPath,
    $gstRuntimeProvenancePath
)) {
    if (-not (Test-Path $requiredPath -PathType Leaf)) {
        throw "GStreamer staged-closure evidence is missing: $requiredPath"
    }
}
$gstPluginRootsHash = (Get-FileHash $gstPluginRootsPath -Algorithm SHA256).Hash.ToLowerInvariant()
$gstProfile = Get-Content $gstProfilePath
foreach ($required in @(
    'gstreamer=1.28.4',
    'abi=msvc-x86_64',
    'installer_sha256=1a745d67225e43394a4a5db929c97397cb56e74b1c38bb77c6ded4b037d3c040',
    'plugin_policy=explicit-recursive-pe-v1',
    'codec_policy=openh264-mediafoundation-v1',
    "plugin_roots_sha256=$gstPluginRootsHash"
)) {
    if ($required -notin $gstProfile) {
        throw "GStreamer build profile is missing required assertion: $required"
    }
}

$allowedPluginNames = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($name in Get-Content $gstPluginAllowlistPath) {
    $name = $name.Trim()
    if ([string]::IsNullOrWhiteSpace($name)) { continue }
    if ([System.IO.Path]::GetFileName($name) -ne $name -or
        $name -notmatch '^[A-Za-z0-9_.+\-]+\.dll$' -or
        -not $allowedPluginNames.Add($name)) {
        throw "Invalid or duplicate staged GStreamer plugin allowlist entry: $name"
    }
}
$actualPluginNames = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($plugin in Get-ChildItem $pluginDir -File -Filter '*.dll') {
    [void]$actualPluginNames.Add($plugin.Name)
}
if (-not $actualPluginNames.SetEquals($allowedPluginNames)) {
    $missing = @($allowedPluginNames | Where-Object { -not $actualPluginNames.Contains($_) })
    $extra = @($actualPluginNames | Where-Object { -not $allowedPluginNames.Contains($_) })
    throw "Staged GStreamer plugins differ from the reviewed allowlist (missing=$missing, extra=$extra)"
}

$forbiddenRuntimePattern = '(?i)(x264|x265|a52dec|dtsdec|libdca|dvdcss|dvdnav|dvdread|faad|fdkaac)'
foreach ($file in Get-ChildItem $root -Recurse -File) {
    if ($file.Name -match $forbiddenRuntimePattern) {
        throw "Restricted/GPL runtime component was staged: $($file.FullName)"
    }
}
$licenseRoot = Join-Path $root 'share\licenses\ferrex-libmpv'
$profilePath = Join-Path $licenseRoot 'BUILD_PROFILE'
if (-not (Test-Path $profilePath)) {
    throw 'Ferrex libmpv BUILD_PROFILE is missing from the staged artifact'
}
$profile = Get-Content $profilePath
foreach ($required in @(
    'mpv=0.41.0',
    'mpv_commit=41f6a645068483470267271e1d09966ca3b9f413',
    'client_api=2.5', 'gpl=false', 'libmpv=true',
    'gpu_next=true', 'd3d11=true', 'direct3d=false',
    'd3d_hwaccel=true', 'd3d9_hwaccel=true', 'wasapi=true',
    'gl=false', 'vulkan=false', 'lua=luajit', 'ffmpeg=8.1.2',
    'ffmpeg_commit=38b88335f99e76ed89ff3c93f877fdefce736c13',
    'ffmpeg_gpl=false', 'ffmpeg_nonfree=false', 'ffmpeg_version3=false',
    'libass=0.17.4',
    'libass_commit=bbb3c7f1570a4a021e52683f3fbdf74fe492ae84',
    'libplacebo=7.360.1',
    'libplacebo_commit=cee9b076f2c63104ccfd497fa79c39a867293ec4',
    'luajit_commit=b411bec3ce550ef9968fc83bca094455cf812c1f',
    'toolchain=msys2-ucrt64'
)) {
    if ($required -notin $profile) {
        throw "Ferrex libmpv BUILD_PROFILE is missing required assertion: $required"
    }
}
foreach ($notice in @(
    'mpv\LICENSE.LGPL', 'ffmpeg\COPYING.LGPLv2.1',
    'ffmpeg\LICENSE.md', 'libass\COPYING', 'libplacebo\LICENSE',
    'luajit\COPYRIGHT', 'runtime-packages\MANIFEST'
)) {
    if (-not (Test-Path (Join-Path $licenseRoot $notice) -PathType Leaf)) {
        throw "Required libmpv closure notice is missing: $notice"
    }
}
$manifest = Join-Path $licenseRoot 'RUNTIME_DLLS.sha256'
if (-not (Test-Path $manifest)) {
    throw 'Ferrex libmpv runtime-DLL hash manifest is missing from the staged artifact'
}
$manifestedNames = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($line in Get-Content $manifest) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    if ($line -notmatch '^([0-9A-Fa-f]{64})\s+\*?(?:\./)?([^\\/]+\.dll)$') {
        throw "Invalid libmpv runtime-DLL manifest entry: $line"
    }
    $expected = $Matches[1]
    $name = $Matches[2]
    if (-not $manifestedNames.Add($name)) {
        throw "Duplicate libmpv runtime-DLL manifest entry: $name"
    }
    $staged = Join-Path $bin $name
    if (-not (Test-Path $staged -PathType Leaf)) {
        throw "Manifested libmpv dependency is missing from the stage: $name"
    }
    $actual = (Get-FileHash $staged -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        throw "Manifested libmpv dependency was replaced or modified: $name"
    }
}

$runtimePackageManifest = Join-Path $licenseRoot 'runtime-packages\MANIFEST'
$runtimeEvidenceNames = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
$sourceAssertions = @{
    mpv = @('0.41.0@41f6a645068483470267271e1d09966ca3b9f413', 'LGPL-2.1-or-later')
    ffmpeg = @('8.1.2@38b88335f99e76ed89ff3c93f877fdefce736c13', 'LGPL-2.1-or-later')
    libass = @('0.17.4@bbb3c7f1570a4a021e52683f3fbdf74fe492ae84', 'ISC')
    libplacebo = @('7.360.1@cee9b076f2c63104ccfd497fa79c39a867293ec4', 'LGPL-2.1-or-later')
    luajit = @('b411bec3ce550ef9968fc83bca094455cf812c1f', 'MIT')
}
foreach ($line in Get-Content $runtimePackageManifest) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $fields = @($line -split ([char]9), 5)
    if ($fields.Count -ne 5) {
        throw "Invalid libmpv runtime provenance row: $line"
    }
    $name, $originKind, $component, $version, $licenses = $fields
    if (-not $manifestedNames.Contains($name) -or
        -not $runtimeEvidenceNames.Add($name) -or
        [string]::IsNullOrWhiteSpace($version) -or
        [string]::IsNullOrWhiteSpace($licenses)) {
        throw "Incomplete, duplicate, or unexpected libmpv runtime provenance row: $line"
    }
    if ($originKind -eq 'source') {
        if (-not $sourceAssertions.ContainsKey($component)) {
            throw "Unknown source-built runtime component: $component"
        }
        $assertion = $sourceAssertions[$component]
        if ($version -ne $assertion[0] -or $licenses -ne $assertion[1]) {
            throw "Source-built runtime evidence disagrees with the pinned profile: $line"
        }
    } elseif ($originKind -eq 'msys2') {
        if ($component -notmatch '^mingw-w64-ucrt-x86_64-' -or
            $licenses -eq 'None') {
            throw "Runtime package is not an explicitly licensed UCRT64 package: $line"
        }
        $packageEvidence = Join-Path $licenseRoot "runtime-packages\$component"
        if (-not (Test-Path (Join-Path $packageEvidence 'PACKAGE_INFO') -PathType Leaf)) {
            throw "Runtime package metadata is missing for $component"
        }
        $noticeFiles = @(
            Get-ChildItem $packageEvidence -Recurse -File |
                Where-Object { $_.Name -ne 'PACKAGE_INFO' }
        )
        if ($noticeFiles.Count -eq 0) {
            throw "Runtime package license notices are missing for $component"
        }
    } else {
        throw "Unknown libmpv runtime provenance kind '$originKind'"
    }
}
if (-not $runtimeEvidenceNames.SetEquals($manifestedNames)) {
    throw 'Libmpv runtime hash and license/provenance manifests cover different DLLs'
}

$mpvDll = Get-ChildItem $bin -File |
    Where-Object { $_.Name -in @('libmpv-2.dll', 'mpv-2.dll', 'mpv.dll') } |
    Select-Object -First 1
if (-not $mpvDll -or -not $manifestedNames.Contains($mpvDll.Name)) {
    throw 'The staged libmpv runtime is not covered by its hash manifest'
}

$gstRuntimePaths = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($line in Get-Content $gstRuntimeHashesPath) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    if ($line -notmatch '^([0-9A-Fa-f]{64})\s+\*?(.+)$') {
        throw "Invalid staged GStreamer runtime hash entry: $line"
    }
    $expected = $Matches[1]
    $relative = $Matches[2].Replace('\', '/')
    if ([System.IO.Path]::IsPathRooted($relative) -or
        @($relative -split '/').Contains('..') -or
        -not $gstRuntimePaths.Add($relative)) {
        throw "Unsafe or duplicate staged GStreamer runtime path: $relative"
    }
    $staged = Join-Path $root $relative
    if (-not (Test-Path $staged -PathType Leaf)) {
        throw "Manifested GStreamer runtime file is missing: $relative"
    }
    $actual = (Get-FileHash $staged -Algorithm SHA256).Hash
    if ($actual -ne $expected) {
        throw "Manifested GStreamer runtime file was replaced or modified: $relative"
    }
}

$gstProvenancePaths = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($line in Get-Content $gstRuntimeProvenancePath) {
    if ([string]::IsNullOrWhiteSpace($line)) { continue }
    $fields = @($line -split ([char]9), 2)
    if ($fields.Count -ne 2 -or
        $fields[1] -ne 'GStreamer 1.28.4 official MSVC x86_64 installer' -or
        -not $gstProvenancePaths.Add($fields[0])) {
        throw "Invalid or duplicate GStreamer runtime provenance entry: $line"
    }
}
if (-not $gstProvenancePaths.SetEquals($gstRuntimePaths)) {
    throw 'GStreamer hash and provenance manifests cover different files'
}

foreach ($plugin in Get-ChildItem $pluginDir -File -Filter '*.dll') {
    $relative = [System.IO.Path]::GetRelativePath($root, $plugin.FullName).Replace('\', '/')
    if (-not $gstRuntimePaths.Contains($relative)) {
        throw "Staged GStreamer plugin lacks hash/provenance coverage: $relative"
    }
}
foreach ($tool in @($scanner, $gstInspect, $gstLaunch, $certificateBundle)) {
    $relative = [System.IO.Path]::GetRelativePath($root, $tool).Replace('\', '/')
    if (-not $gstRuntimePaths.Contains($relative)) {
        throw "Staged GStreamer tool lacks hash/provenance coverage: $relative"
    }
}
foreach ($module in Get-ChildItem $gioModuleDir -File -Filter '*.dll') {
    $relative = [System.IO.Path]::GetRelativePath($root, $module.FullName).Replace('\', '/')
    if (-not $gstRuntimePaths.Contains($relative)) {
        throw "Staged GIO module lacks hash/provenance coverage: $relative"
    }
}
foreach ($dll in Get-ChildItem $bin -File -Filter '*.dll') {
    if ($manifestedNames.Contains($dll.Name)) { continue }
    $relative = [System.IO.Path]::GetRelativePath($root, $dll.FullName).Replace('\', '/')
    if (-not $gstRuntimePaths.Contains($relative)) {
        throw "Staged DLL has neither libmpv nor GStreamer provenance: $relative"
    }
}

function Get-Imports([string]$Path) {
    & $dumpbin /nologo /dependents $Path |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ -match '^[A-Za-z0-9_.+\-]+\.dll$' }
}

$searchDirs = @($root, $bin, $pluginDir, $gioModuleDir) | Where-Object { Test-Path $_ }
$queue = [System.Collections.Generic.Queue[string]]::new()
$seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$localByName = @{}
foreach ($file in Get-ChildItem -Path $searchDirs -File -Filter '*.dll') {
    if (-not $localByName.ContainsKey($file.Name)) {
        $localByName[$file.Name] = $file.FullName
    } elseif ((Get-FileHash $localByName[$file.Name] -Algorithm SHA256).Hash -ne
        (Get-FileHash $file.FullName -Algorithm SHA256).Hash) {
        throw "Conflicting staged DLLs share the name $($file.Name)"
    }
    $queue.Enqueue($file.FullName)
}
$queue.Enqueue((Join-Path $root 'ferrex-player.exe'))
$queue.Enqueue($scanner)
$queue.Enqueue($gstInspect)
$queue.Enqueue($gstLaunch)

$resolved = 0
while ($queue.Count -gt 0) {
    $binary = $queue.Dequeue()
    if (-not $seen.Add($binary)) { continue }
    foreach ($dependency in Get-Imports $binary) {
        if ($dependency -match $forbiddenRuntimePattern) {
            throw "Restricted/GPL DLL import: $dependency required by $binary"
        }
        if ($dependency.StartsWith('api-ms-win-', [System.StringComparison]::OrdinalIgnoreCase) -or
            $dependency.StartsWith('ext-ms-', [System.StringComparison]::OrdinalIgnoreCase)) {
            continue
        }
        if ($localByName.ContainsKey($dependency)) {
            $queue.Enqueue($localByName[$dependency])
            $resolved++
            continue
        }
        if (Test-Path (Join-Path $system32 $dependency)) { continue }
        throw "Unresolved DLL import: $dependency required by $binary"
    }
}

$oldPath = $env:PATH
$oldPluginPath = $env:GST_PLUGIN_PATH
$oldPluginPath10 = $env:GST_PLUGIN_PATH_1_0
$oldSystemPath10 = $env:GST_PLUGIN_SYSTEM_PATH_1_0
$oldScanner = $env:GST_PLUGIN_SCANNER
$oldScanner10 = $env:GST_PLUGIN_SCANNER_1_0
$oldRegistry = $env:GST_REGISTRY_1_0
$oldGioModules = $env:GIO_EXTRA_MODULES
$oldSslCertFile = $env:SSL_CERT_FILE
$registry = Join-Path $env:TEMP "ferrex-gst-registry-$PID.bin"
$hlsFixtureDirectory = Join-Path $env:TEMP "ferrex-gst-hls-$PID"
try {
    # Do not let the installer SDK exported by the build job mask a missing
    # staged dependency. Windows' normal system DLL directories remain in the
    # loader search order without inheriting the build machine's PATH.
    $env:PATH = $bin
    $env:GST_PLUGIN_PATH = $pluginDir
    $env:GST_PLUGIN_PATH_1_0 = $pluginDir
    $env:GST_PLUGIN_SYSTEM_PATH_1_0 = $pluginDir
    $env:GST_PLUGIN_SCANNER = $scanner
    $env:GST_PLUGIN_SCANNER_1_0 = $scanner
    $env:GST_REGISTRY_1_0 = $registry
    $env:GIO_EXTRA_MODULES = $gioModuleDir
    $env:SSL_CERT_FILE = $certificateBundle

    $versionOutput = (& $gstInspect --version 2>&1) -join "`n"
    if ($LASTEXITCODE -ne 0 -or $versionOutput -notmatch 'GStreamer\s+1\.28\.4') {
        throw "Bundled gst-inspect version check failed: $versionOutput"
    }
    foreach ($factory in @(
        'playbin3', 'decodebin3', 'uridecodebin3', 'appsink',
        'videoconvertscale', 'audioconvert', 'audioresample',
        'volume', 'scaletempo', 'qtdemux', 'matroskademux',
        'tsdemux', 'hlsdemux2', 'souphttpsrc', 'aacparse',
        'h264parse', 'mfaacdec', 'openh264dec', 'assrender',
        'wasapi2sink', 'directsoundsink', 'audiotestsrc',
        'videotestsrc', 'mfaacenc', 'openh264enc', 'mpegtsmux'
    )) {
        $output = (& $gstInspect $factory 2>&1) -join "`n"
        if ($LASTEXITCODE -ne 0) {
            throw "Bundled GStreamer factory '$factory' is unavailable: $output"
        }
    }

    # Generate and then play a tiny HLS/AAC fixture using only the stage. This
    # exercises adaptive demux, MPEG-TS, the reviewed system/BSD codec path,
    # and playbin selection without depending on a mutable public media URL.
    New-Item -ItemType Directory -Force -Path $hlsFixtureDirectory | Out-Null
    $segment = Join-Path $hlsFixtureDirectory 'segment.ts'
    $generateArgs = @(
        '-q', 'mpegtsmux', 'name=mux', '!', 'filesink', "location=$segment",
        'audiotestsrc', 'wave=sine', 'num-buffers=96', '!',
        'audioconvert', '!', 'audioresample', '!',
        'audio/x-raw,rate=48000,channels=2', '!', 'mfaacenc', '!',
        'aacparse', '!', 'mux.',
        'videotestsrc', 'num-buffers=48', '!',
        'video/x-raw,width=320,height=180,framerate=24/1', '!',
        'videoconvertscale', '!', 'openh264enc', '!', 'h264parse', '!', 'mux.'
    )
    $generateOutput = (& $gstLaunch @generateArgs 2>&1) -join [Environment]::NewLine
    if ($LASTEXITCODE -ne 0) {
        throw "Bundled GStreamer HLS fixture generation failed: $generateOutput"
    }
    $playlist = Join-Path $hlsFixtureDirectory 'stream.m3u8'
    @(
        '#EXTM3U',
        '#EXT-X-VERSION:3',
        '#EXT-X-TARGETDURATION:3',
        '#EXT-X-MEDIA-SEQUENCE:0',
        '#EXTINF:2.048,',
        'segment.ts',
        '#EXT-X-ENDLIST'
    ) | Set-Content -Encoding ascii $playlist
    $playlistUri = ([System.Uri]::new($playlist)).AbsoluteUri
    $playArgs = @(
        '-q', 'playbin3', "uri=$playlistUri",
        'audio-sink=fakesink', 'video-sink=fakesink'
    )
    $playOutput = (& $gstLaunch @playArgs 2>&1) -join [Environment]::NewLine
    if ($LASTEXITCODE -ne 0) {
        throw "Bundled GStreamer HLS playback smoke failed: $playOutput"
    }

    # Exercise the dynamically loaded GIO TLS backend and CA bundle from the
    # clean stage; factory discovery alone does not prove HTTPS can connect.
    $httpsUrl = 'https://gstreamer.freedesktop.org/data/pkg/windows/1.28.4/msvc/gstreamer-1.0-msvc-x86_64-1.28.4.exe.sha256sum'
    $httpsArgs = @('-q', 'souphttpsrc', "location=$httpsUrl", '!', 'fakesink')
    $httpsOutput = (& $gstLaunch @httpsArgs 2>&1) -join [Environment]::NewLine
    if ($LASTEXITCODE -ne 0) {
        throw "Bundled GStreamer HTTPS smoke failed: $httpsOutput"
    }
} finally {
    $env:PATH = $oldPath
    $env:GST_PLUGIN_PATH = $oldPluginPath
    $env:GST_PLUGIN_PATH_1_0 = $oldPluginPath10
    $env:GST_PLUGIN_SYSTEM_PATH_1_0 = $oldSystemPath10
    $env:GST_PLUGIN_SCANNER = $oldScanner
    $env:GST_PLUGIN_SCANNER_1_0 = $oldScanner10
    $env:GST_REGISTRY_1_0 = $oldRegistry
    $env:GIO_EXTRA_MODULES = $oldGioModules
    $env:SSL_CERT_FILE = $oldSslCertFile
    Remove-Item $registry -Force -ErrorAction SilentlyContinue
    Remove-Item $hlsFixtureDirectory -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "Verified Windows runtime closure, HLS playback, and HTTPS fallback ($($seen.Count) binaries, $resolved local imports)."
