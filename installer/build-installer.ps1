<#
.SYNOPSIS
  Builds the standalone installer and portable zip into dist\.

.DESCRIPTION
  1. cargo build --release (unless -SkipBuild) and generates the icon
  2. optionally signs askeri-ruller.exe
  3. compiles installer\askeri-ruler.iss with Inno Setup (ISCC.exe)
  4. optionally signs the setup .exe
  5. writes a portable zip and SHA-256 checksums

  Outputs:
    dist\AskeriRuler-Setup-<version>.exe
    dist\AskeriRuler-<version>-portable.zip
    dist\SHA256SUMS.txt

  Signing (optional): pass -SignCommand with a command line containing {file},
  e.g. for a .pfx certificate:
    -SignCommand 'signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /f C:\certs\code.pfx /p PASSWORD "{file}"'
  or set the SIGN_COMMAND environment variable. Without it the files are unsigned.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1
#>
param(
    [switch]$SkipBuild,
    [string]$SignCommand = $env:SIGN_COMMAND
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path += ";$env:USERPROFILE\.cargo\bin"
}

function Invoke-Native([string]$exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "$exe failed with exit code $LASTEXITCODE" }
}

function Find-Iscc {
    $cmd = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    foreach ($p in @(
            "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
            "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
            "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe")) {
        if (Test-Path $p) { return $p }
    }
    throw 'Inno Setup 6 (ISCC.exe) not found. Install it: winget install JRSoftware.InnoSetup'
}

function Invoke-Sign([string]$file) {
    if (-not $SignCommand) { return }
    if ($SignCommand -notmatch '\{file\}') { throw 'SignCommand must contain {file}' }
    if ($env:Path -notmatch 'Windows Kits') {
        # Make signtool.exe resolvable.
        $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
        $bin = Get-ChildItem $kits -Directory -ErrorAction SilentlyContinue | Where-Object Name -match '^10\.' |
            Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1
        if ($bin) { $env:Path += ";$(Join-Path $bin.FullName 'x64')" }
    }
    Write-Host "== signing $([IO.Path]::GetFileName($file))"
    $cmd = $SignCommand.Replace('{file}', $file)
    cmd /c $cmd
    if ($LASTEXITCODE -ne 0) { throw "Signing failed for $file" }
}

$m = Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $m) { throw 'Could not read version from Cargo.toml' }
$version = $m.Matches[0].Groups[1].Value

$dist = Join-Path $root 'dist'
$work = Join-Path $root 'target\installer'
New-Item -ItemType Directory -Force $dist, $work | Out-Null

if (-not $SkipBuild) {
    Write-Host '== cargo build --release'
    Invoke-Native 'cargo' @('build', '--release')
}
Write-Host '== generating icon'
Invoke-Native 'cargo' @('run', '--release', '--quiet', '--example', 'gen_assets', '--', (Join-Path $work 'assets')) | Out-Null

# Sign a copy so target\release stays untouched.
$exe = Join-Path $work 'askeri-ruller.exe'
Copy-Item target\release\askeri-ruller.exe $exe -Force
Invoke-Sign $exe

Write-Host "== Inno Setup $version"
$iscc = Find-Iscc
Invoke-Native $iscc @('/Q',
    "/DAppVersion=$version",
    "/DSourceExe=$exe",
    "/DIconFile=$(Join-Path $work 'assets\app.ico')",
    "/DOutputDir=$dist",
    'installer\askeri-ruler.iss')
$setup = Join-Path $dist "AskeriRuler-Setup-$version.exe"
Invoke-Sign $setup

Write-Host '== portable zip'
$zip = Join-Path $dist "AskeriRuler-$version-portable.zip"
$zipDir = Join-Path $work 'portable'
if (Test-Path $zipDir) { Remove-Item $zipDir -Recurse -Force }
New-Item -ItemType Directory $zipDir | Out-Null
Copy-Item $exe $zipDir
Copy-Item README.md, PRIVACY.md $zipDir
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path (Join-Path $zipDir '*') -DestinationPath $zip

$sums = Get-ChildItem $setup, $zip | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
}
[IO.File]::WriteAllLines((Join-Path $dist 'SHA256SUMS.txt'), $sums)

Write-Host "Done:"
Get-ChildItem $dist | ForEach-Object { Write-Host "  $($_.Name)  ($([math]::Round($_.Length / 1KB)) KB)" }
if (-not $SignCommand) { Write-Host '(unsigned: pass -SignCommand or set SIGN_COMMAND to sign)' }
