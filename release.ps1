# Builds the downloads of a HeMimizer release - the same on a PC and in the GitHub workflow (.github/workflows/release.yml):
#   build\dist\HeMimizer.exe         the program for x64 (one file; it runs from any folder, e.g. a USB stick)
#   build\dist\HeMimizer-arm64.exe   the same for Windows on ARM
# Needs: Rust (stable, MSVC), and for ARM64 the MSVC ARM64 build tools (on a PC without them ARM64 is left out with a
# warning; the GitHub workflow always builds both). The version comes from Cargo.toml.
param([switch]$NoArm)
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
Set-Location $root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:Path += ";$env:USERPROFILE\.cargo\bin" }
$app = 'HeMimizer'
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

$dist = Join-Path $root 'build\dist'
Remove-Item $dist -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $dist | Out-Null
$targets = [ordered]@{ x64 = 'x86_64-pc-windows-msvc'; arm64 = 'aarch64-pc-windows-msvc' }
foreach ($arch in $targets.Keys) {
    if ($arch -eq 'arm64' -and $NoArm) { continue }
    $triple = $targets[$arch]
    $ErrorActionPreference = 'Continue'
    rustup target add $triple *> $null
    cargo build --release --target $triple
    $ok = $LASTEXITCODE -eq 0
    $ErrorActionPreference = 'Stop'
    if (-not $ok) {
        if ($arch -eq 'arm64' -and -not $env:GITHUB_ACTIONS) { Write-Warning 'ARM64 could not be built on this PC (the MSVC ARM64 build tools are missing?) - x64 only.'; continue }
        throw "The build failed ($arch)."
    }
    $name = if ($arch -eq 'x64') { "$app.exe" } else { "$app-$arch.exe" }
    Copy-Item (Join-Path $root "build\$triple\release\$app.exe") (Join-Path $dist $name)
}
Write-Host "$app $version" -ForegroundColor Cyan
Get-ChildItem $dist | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }