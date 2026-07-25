param(
    [Parameter(Mandatory = $true)]
    [string]$SdkRoot
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path $SdkRoot).Path
$dll = Get-ChildItem (Join-Path $root 'bin') -File |
    Where-Object { $_.Name -in @('libmpv-2.dll', 'mpv-2.dll', 'mpv.dll') } |
    Select-Object -First 1
if (-not $dll) { throw "libmpv runtime DLL not found under $root\bin" }

$dumpbin = Get-Command dumpbin.exe -ErrorAction Stop
$lib = Get-Command lib.exe -ErrorAction Stop
$exports = & $dumpbin.Source /nologo /exports $dll.FullName |
    ForEach-Object {
        if ($_ -match '^\s+\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)') {
            $Matches[1]
        }
    } |
    Sort-Object -Unique
if (-not $exports -or $exports.Count -lt 20) {
    throw "Could not read a plausible libmpv export table from $($dll.FullName)"
}
foreach ($required in @('mpv_client_api_version', 'mpv_create', 'mpv_initialize', 'mpv_terminate_destroy')) {
    if ($required -notin $exports) { throw "libmpv DLL is missing export $required" }
}

$libDir = Join-Path $root 'lib'
New-Item -ItemType Directory -Force -Path $libDir | Out-Null
$def = Join-Path $libDir 'mpv.def'
$lines = @("LIBRARY `"$($dll.Name)`"", 'EXPORTS') + ($exports | ForEach-Object { "    $_" })
Set-Content -Path $def -Value $lines -Encoding ascii

& $lib.Source "/def:$def" '/machine:x64' "/out:$(Join-Path $libDir 'mpv.lib')" /nologo
if ($LASTEXITCODE -ne 0 -or -not (Test-Path (Join-Path $libDir 'mpv.lib'))) {
    throw 'MSVC libmpv import-library generation failed'
}
Write-Host "Created $libDir\mpv.lib from $($dll.Name)"
