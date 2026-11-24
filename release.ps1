# Builds the download of a HeMiMizer release - the same on a PC and in the GitHub workflow (.github/workflows/release.yml):
#   build\dist\HeMiMizer.exe         the program for x64 (one file; it runs from any folder, e.g. a USB stick)
# HeMiMizer has no ARM64 version (the author's choice, 2026-11-18): it installs x64 drivers and programs.
# Needs: Rust (stable, MSVC). The version comes from Cargo.toml.
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
Set-Location $root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:Path += ";$env:USERPROFILE\.cargo\bin" }
$app = 'HeMiMizer'
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

$dist = Join-Path $root 'build\dist'
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $dist | Out-Null
$targets = [ordered]@{ x64 = 'x86_64-pc-windows-msvc' }
foreach ($arch in $targets.Keys) {
    $triple = $targets[$arch]
    $ErrorActionPreference = 'Continue'
    rustup target add $triple *> $null
    cargo build --release --locked --target $triple
    $ok = $LASTEXITCODE -eq 0
    $ErrorActionPreference = 'Stop'
    if (-not $ok) {
        throw "The build failed ($arch)."
    }
    $name = if ($arch -eq 'x64') { "$app.exe" } else { "$app-$arch.exe" }
    Copy-Item (Join-Path $root "build\$triple\release\$app.exe") (Join-Path $dist $name)
}
Write-Host "$app $version" -ForegroundColor Cyan
Get-ChildItem $dist | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }