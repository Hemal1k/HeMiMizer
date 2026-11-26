# Builds the downloads of a HeMiMizer release - the same on a PC and in the GitHub workflow (.github/workflows/release.yml):
#   build\dist\HeMiMizer-Setup.exe   the installer (Inno Setup 6) for x64: {autopf}\HeMi\HeMiMizer, a shortcut in Start
#   build\dist\HeMiMizer.exe         the portable program for x64 (one file; it runs from any folder, e.g. a USB stick) -
#                                    the name stays, older HeMiMizers update themselves by it
# HeMiMizer has no ARM64 version (the author's choice, 2026-11-18): it installs x64 drivers and programs.
# Needs: Rust (stable, MSVC) and Inno Setup 6. The version comes from Cargo.toml.
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
Set-Location $root
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { $env:Path += ";$env:USERPROFILE\.cargo\bin" }
$app = 'HeMiMizer'
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$iscc = @((Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
          "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
        Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 is not installed (https://jrsoftware.org/isdl.php).' }

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

# the installer, around the same x64 program
& $iscc '/Q' "/DAppVersion=$version" "/DAppExe=$(Join-Path $root "build\$($targets['x64'])\release\$app.exe")" "/DOutputDir=$dist" (Join-Path $root "installer\$app.iss")
if ($LASTEXITCODE) { throw 'The installer could not be made.' }

Get-ChildItem $dist | ForEach-Object { Write-Host ("  {0}  {1:N1} MB" -f $_.Name, ($_.Length / 1MB)) }