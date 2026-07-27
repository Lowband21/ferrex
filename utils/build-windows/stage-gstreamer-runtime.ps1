param(
    [Parameter(Mandatory = $true)]
    [string]$SourceRoot,

    [Parameter(Mandatory = $true)]
    [string]$StageRoot,

    [Parameter(Mandatory = $true)]
    [string]$PluginManifest
)

$ErrorActionPreference = 'Stop'

$sourceRoot = (Resolve-Path $SourceRoot).Path
$stageRoot = (Resolve-Path $StageRoot).Path
$pluginManifest = (Resolve-Path $PluginManifest).Path
$sourceBin = Join-Path $sourceRoot 'bin'
$sourcePlugins = Join-Path $sourceRoot 'lib\gstreamer-1.0'
$sourceGioModules = Join-Path $sourceRoot 'lib\gio\modules'
$sourceLibproxy = Join-Path $sourceRoot 'lib\libproxy'
$stageBin = Join-Path $stageRoot 'bin'
$stagePlugins = Join-Path $stageRoot 'lib\gstreamer-1.0'
$stageGioModules = Join-Path $stageRoot 'lib\gio\modules'
$stageLibexec = Join-Path $stageRoot 'libexec\gstreamer-1.0'
$stageCerts = Join-Path $stageRoot 'etc\ssl\certs'
$noticeRoot = Join-Path $stageRoot 'share\licenses\gstreamer'
$system32 = Join-Path $env:SystemRoot 'System32'
$dumpbin = (Get-Command dumpbin.exe -ErrorAction Stop).Source

foreach ($directory in @($sourceBin, $sourcePlugins, $sourceGioModules, $sourceLibproxy)) {
    if (-not (Test-Path $directory -PathType Container)) {
        throw "GStreamer source directory is missing: $directory"
    }
}
New-Item -ItemType Directory -Force -Path $stageBin, $stagePlugins, $stageGioModules, $stageLibexec, $stageCerts, $noticeRoot | Out-Null

$pluginRoots = @(
    Get-Content $pluginManifest |
        ForEach-Object { ($_ -replace '#.*$', '').Trim() } |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
)
if ($pluginRoots.Count -eq 0) {
    throw "GStreamer plugin allowlist is empty: $pluginManifest"
}
$pluginRootNames = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
foreach ($pluginRoot in $pluginRoots) {
    if ([System.IO.Path]::GetFileName($pluginRoot) -ne $pluginRoot -or
        $pluginRoot -notmatch '^[A-Za-z0-9_.+\-]+$') {
        throw "Invalid GStreamer plugin root entry: $pluginRoot"
    }
    if (-not $pluginRootNames.Add($pluginRoot)) {
        throw "Duplicate GStreamer plugin root entry: $pluginRoot"
    }
}

# Cerbero normally omits the Unix `lib` prefix for PE plugins, but accepting
# exactly one of the two documented forms keeps the policy robust across
# official installer layout changes without using a wildcard allowlist.
$plugins = [System.Collections.Generic.List[string]]::new()
foreach ($pluginRoot in $pluginRoots) {
    $matches = @(
        @(
            (Join-Path $sourcePlugins "$pluginRoot.dll"),
            (Join-Path $sourcePlugins "lib$pluginRoot.dll")
        ) | Where-Object { Test-Path $_ -PathType Leaf }
    )
    if ($matches.Count -ne 1) {
        throw "Expected exactly one official plugin for '$pluginRoot'; found $($matches.Count)"
    }
    $plugins.Add([System.IO.Path]::GetFileName($matches[0]))
}

$copied = [System.Collections.Generic.List[string]]::new()
function Copy-FromGStreamer([string]$Source, [string]$Destination) {
    if (-not (Test-Path $Source -PathType Leaf)) {
        throw "Required GStreamer runtime file is missing: $Source"
    }
    Copy-Item -Force $Source $Destination
    $copied.Add((Resolve-Path $Destination).Path)
}

foreach ($plugin in $plugins) {
    Copy-FromGStreamer (Join-Path $sourcePlugins $plugin) (Join-Path $stagePlugins $plugin)
}

$inspect = Join-Path $sourceBin 'gst-inspect-1.0.exe'
Copy-FromGStreamer $inspect (Join-Path $stageBin 'gst-inspect-1.0.exe')
$launch = Join-Path $sourceBin 'gst-launch-1.0.exe'
Copy-FromGStreamer $launch (Join-Path $stageBin 'gst-launch-1.0.exe')
$scanner = @(
    (Join-Path $sourceRoot 'libexec\gstreamer-1.0\gst-plugin-scanner.exe'),
    (Join-Path $sourceBin 'gst-plugin-scanner.exe')
) | Where-Object { Test-Path $_ -PathType Leaf } | Select-Object -First 1
if (-not $scanner) {
    throw 'GStreamer plugin scanner is missing'
}
Copy-FromGStreamer $scanner (Join-Path $stageLibexec 'gst-plugin-scanner.exe')

foreach ($gioModuleRoot in @('gioopenssl', 'giolibproxy')) {
    $matches = @(
        @(
            (Join-Path $sourceGioModules "$gioModuleRoot.dll"),
            (Join-Path $sourceGioModules "lib$gioModuleRoot.dll")
        ) | Where-Object { Test-Path $_ -PathType Leaf }
    )
    if ($matches.Count -ne 1) {
        throw "Expected exactly one official GIO module for '$gioModuleRoot'; found $($matches.Count)"
    }
    $gioModule = [System.IO.Path]::GetFileName($matches[0])
    Copy-FromGStreamer $matches[0] (Join-Path $stageGioModules $gioModule)
}
$gioModuleCache = Join-Path $sourceGioModules 'giomodule.cache'
if (Test-Path $gioModuleCache -PathType Leaf) {
    Copy-FromGStreamer $gioModuleCache (Join-Path $stageGioModules 'giomodule.cache')
}
$certificateBundle = Join-Path $sourceRoot 'etc\ssl\certs\ca-certificates.crt'
Copy-FromGStreamer $certificateBundle (Join-Path $stageCerts 'ca-certificates.crt')

function Get-Imports([string]$Path) {
    & $dumpbin /nologo /dependents $Path |
        ForEach-Object { $_.Trim() } |
        Where-Object { $_ -match '^[A-Za-z0-9_.+\-]+\.dll$' }
}

$availableByName = @{}
foreach ($directory in @($sourceBin, $sourceLibproxy)) {
    foreach ($file in Get-ChildItem $directory -File -Filter '*.dll') {
        if ($availableByName.ContainsKey($file.Name)) {
            throw "Duplicate GStreamer runtime DLL basename: $($file.Name)"
        }
        $availableByName[$file.Name] = $file.FullName
    }
}

$localByName = @{}
foreach ($directory in @($stageRoot, $stageBin, $stagePlugins, $stageGioModules)) {
    foreach ($file in Get-ChildItem $directory -File -Filter '*.dll' -ErrorAction SilentlyContinue) {
        if ($localByName.ContainsKey($file.Name) -and
            (Get-FileHash $localByName[$file.Name] -Algorithm SHA256).Hash -ne
                (Get-FileHash $file.FullName -Algorithm SHA256).Hash) {
            throw "Conflicting staged DLLs share the name $($file.Name)"
        }
        $localByName[$file.Name] = $file.FullName
    }
}

$queue = [System.Collections.Generic.Queue[string]]::new()
foreach ($binary in @(
    (Join-Path $stageRoot 'ferrex-player.exe'),
    (Join-Path $stageBin 'gst-inspect-1.0.exe'),
    (Join-Path $stageBin 'gst-launch-1.0.exe'),
    (Join-Path $stageLibexec 'gst-plugin-scanner.exe')
)) {
    if (Test-Path $binary -PathType Leaf) {
        $queue.Enqueue((Resolve-Path $binary).Path)
    }
}
foreach ($directory in @($stageBin, $stagePlugins, $stageGioModules)) {
    foreach ($binary in Get-ChildItem $directory -File -Filter '*.dll') {
        $queue.Enqueue($binary.FullName)
    }
}

$seen = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::OrdinalIgnoreCase
)
while ($queue.Count -gt 0) {
    $binary = $queue.Dequeue()
    if (-not $seen.Add($binary)) { continue }

    foreach ($dependency in Get-Imports $binary) {
        if ($dependency.StartsWith('api-ms-win-', [System.StringComparison]::OrdinalIgnoreCase) -or
            $dependency.StartsWith('ext-ms-', [System.StringComparison]::OrdinalIgnoreCase)) {
            continue
        }
        if ($localByName.ContainsKey($dependency)) {
            $queue.Enqueue($localByName[$dependency])
            continue
        }
        if ($availableByName.ContainsKey($dependency)) {
            $destination = Join-Path $stageBin $dependency
            Copy-FromGStreamer $availableByName[$dependency] $destination
            $localByName[$dependency] = (Resolve-Path $destination).Path
            $queue.Enqueue($localByName[$dependency])
            continue
        }
        if (Test-Path (Join-Path $system32 $dependency) -PathType Leaf) {
            continue
        }
        throw "Unresolved DLL import while staging GStreamer: $dependency required by $binary"
    }
}

Copy-Item -Force $pluginManifest (Join-Path $noticeRoot 'PLUGIN_ROOTS')
$plugins | Set-Content (Join-Path $noticeRoot 'PLUGIN_ALLOWLIST')
$runtimeHashes = [System.Collections.Generic.List[string]]::new()
$runtimeProvenance = [System.Collections.Generic.List[string]]::new()
foreach ($path in @($copied | Sort-Object -Unique)) {
    $relative = [System.IO.Path]::GetRelativePath($stageRoot, $path).Replace('\', '/')
    $hash = (Get-FileHash $path -Algorithm SHA256).Hash.ToLowerInvariant()
    $runtimeHashes.Add("$hash *$relative")
    $runtimeProvenance.Add("$relative`tGStreamer 1.28.4 official MSVC x86_64 installer")
}
$runtimeHashes | Set-Content (Join-Path $noticeRoot 'RUNTIME_FILES.sha256')
$runtimeProvenance | Set-Content (Join-Path $noticeRoot 'RUNTIME_PROVENANCE.tsv')

Write-Host "Staged reviewed GStreamer runtime closure ($($plugins.Count) plugins, $($copied.Count) files)."
