# builds release exe and installer when inno setup is available.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root
Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue

if (-not (Test-Path (Join-Path $Root "target\release\sfxrecord.exe"))) {
    Write-Host "Building release executable..."
    cargo build --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
$ExeSource = Join-Path $Root "target\release\sfxrecord.exe"
$Dist = Join-Path $Root "dist"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Copy-Item -Force $ExeSource (Join-Path $Dist "sfxrecord.exe")
Write-Host "Copied: $Dist\sfxrecord.exe"

$IsccCandidates = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
)
$Iscc = $IsccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $Iscc) {
    Write-Host ""
    Write-Host "Inno Setup not found. Install it, then re-run this script:"
    Write-Host "  winget install --id JRSoftware.InnoSetup -e"
    Write-Host ""
    Write-Host "Release exe is ready in dist\sfxrecord.exe"
    exit 0
}

Write-Host "Compiling installer with Inno Setup..."
& $Iscc (Join-Path $Root "installer\SFXRecord.iss")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host ""
Write-Host "Done."
Write-Host "  dist\sfxrecord.exe"
Write-Host "  dist\SFXRecord-Setup-0.1.0.exe"
