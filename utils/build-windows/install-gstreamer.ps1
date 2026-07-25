param(
    [Parameter(Mandatory = $true)]
    [string]$Destination
)

$ErrorActionPreference = 'Stop'

$version = '1.28.4'
$expectedSha256 = '1a745d67225e43394a4a5db929c97397cb56e74b1c38bb77c6ded4b037d3c040'
$architecture = 'x86_64'
$abi = 'msvc'
$installerName = "gstreamer-1.0-$abi-$architecture-$version.exe"
$downloadRoot = if ($env:RUNNER_TEMP) {
    $env:RUNNER_TEMP
} else {
    [System.IO.Path]::GetTempPath()
}
$installer = Join-Path $downloadRoot $installerName
$pkgConfig = Join-Path $Destination 'lib\pkgconfig\gstreamer-1.0.pc'

if (-not (Test-Path $pkgConfig -PathType Leaf)) {
    $url = "https://gstreamer.freedesktop.org/pkg/windows/$version/$abi/$installerName"
    Invoke-WebRequest -Uri $url -OutFile $installer

    $actualSha256 = (Get-FileHash $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "GStreamer installer hash mismatch: expected $expectedSha256, got $actualSha256"
    }

    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $arguments = @(
        '/VERYSILENT',
        '/SUPPRESSMSGBOXES',
        '/NORESTART',
        '/CURRENTUSER',
        '/TYPE=devel',
        "/DIR=$Destination"
    )
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "GStreamer installer exited with code $($process.ExitCode)"
    }
}

if (-not (Test-Path $pkgConfig -PathType Leaf)) {
    throw "GStreamer development package is incomplete: $pkgConfig is missing"
}

$pkgConfigExe = Join-Path $Destination 'bin\pkg-config.exe'
if (-not (Test-Path $pkgConfigExe -PathType Leaf)) {
    throw "GStreamer development package is incomplete: $pkgConfigExe is missing"
}

$reportedVersion = (& $pkgConfigExe --modversion gstreamer-1.0).Trim()
if ($LASTEXITCODE -ne 0 -or $reportedVersion -ne $version) {
    throw "Expected GStreamer $version, found '$reportedVersion'"
}

Remove-Item $installer -Force -ErrorAction SilentlyContinue
Write-Host "Validated pinned GStreamer $version MSVC SDK at $Destination"
