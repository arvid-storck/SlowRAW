param(
    [string]$Target = "",
    [string]$OutputDirectory = "artifacts"
)

$ErrorActionPreference = "Stop"
$workspace = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$tauriDirectory = Join-Path $workspace "src-tauri"
$targetRoot = if ($Target) { Join-Path $tauriDirectory "target\$Target\release" } else { Join-Path $tauriDirectory "target\release" }
$executable = Join-Path $targetRoot "SlowRAW.exe"
$portableRoot = Join-Path $workspace $OutputDirectory
$portableDirectory = Join-Path $portableRoot "SlowRAW-portable"

Push-Location $workspace
try {
    if (-not (Test-Path -LiteralPath (Join-Path $workspace "node_modules"))) {
        npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
    }
    # Tauri CLI sets the production build environment and embeds frontendDist.
    # A plain `cargo build --release` leaves the webview pointed at devUrl.
    $tauriArgs = @("run", "tauri", "--", "build", "--no-bundle")
    if ($Target) { $tauriArgs += @("--target", $Target) }
    npm @tauriArgs
    if ($LASTEXITCODE -ne 0) { throw "Tauri build failed; refusing to package a stale executable" }
} finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $executable)) {
    throw "Portable executable was not produced at $executable"
}

if (Test-Path -LiteralPath $portableDirectory) {
    throw "Output already exists; choose a new OutputDirectory to preserve portable user data: $portableDirectory"
}
New-Item -ItemType Directory -Force -Path $portableDirectory | Out-Null
Set-Content -LiteralPath (Join-Path $portableDirectory "portable.flag") -Value "SlowRAW portable build" -Encoding ASCII
Copy-Item -LiteralPath $executable -Destination (Join-Path $portableDirectory "SlowRAW.exe") -Force
$resourceDestination = Join-Path $portableDirectory "resources"
New-Item -ItemType Directory -Path $resourceDestination | Out-Null
Get-ChildItem -LiteralPath (Join-Path $tauriDirectory "resources") |
    ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $resourceDestination -Recurse -Force }
Copy-Item -LiteralPath (Join-Path $tauriDirectory "lensfun_db") -Destination $portableDirectory -Recurse -Force
$readme = @"
SlowRAW portable

Start SlowRAW.exe directly. Settings and albums are stored in the data folder beside the executable.
The rebuildable image index is rapidraw.db beside the executable.
Original images and SlowRAW sidecars remain in their original locations.

Move or copy this entire folder to relocate the portable installation.
The folder must be writable. Keep portable.flag so SlowRAW recognizes it as
portable; installed builds continue to use the normal system data directory.
Windows 10/11 normally includes the Microsoft Edge WebView2 runtime used by Tauri.
"@
Set-Content -LiteralPath (Join-Path $portableDirectory "README.txt") -Value $readme -Encoding UTF8

$archive = Join-Path $portableRoot "SlowRAW-portable.zip"
if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
Compress-Archive -LiteralPath $portableDirectory -DestinationPath $archive -CompressionLevel Optimal
$programBytes = (Get-Item -LiteralPath (Join-Path $portableDirectory "SlowRAW.exe")).Length
$portableBytes = (Get-ChildItem -LiteralPath $portableDirectory -Recurse -File | Measure-Object -Property Length -Sum).Sum
$archiveBytes = (Get-Item -LiteralPath $archive).Length
Write-Output ("EXE: {0:N2} MiB; portable folder: {1:N2} MiB; ZIP: {2:N2} MiB" -f ($programBytes / 1MB), ($portableBytes / 1MB), ($archiveBytes / 1MB))
Write-Output $archive
