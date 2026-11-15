<#
.SYNOPSIS
  Builds the MSIX package for Askeri Ruler.

.DESCRIPTION
  1. cargo build --release and generates the logo PNGs
  2. stages exe + assets + manifest (identity from packaging\package.json,
     version from Cargo.toml) in target\msix\stage
  3. runs makepri (resource index for the scaled logos) and makeappx

  The resulting .msix in target\msix is what you upload to Partner Center
  (the Store signs it). For a local test install, either register the staged
  folder with -Register (needs Windows Developer Mode) or sign the package
  with -CertPfx and install it.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1 -Register
.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\build-msix.ps1 -CertPfx dev.pfx -CertPassword secret
#>
param(
    [switch]$SkipBuild,
    [switch]$Register,
    [string]$CertPfx,
    [string]$CertPassword
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

function Find-SdkTool([string]$name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $dirs = Get-ChildItem $kits -Directory | Where-Object Name -match '^10\.' |
        Sort-Object { [version]$_.Name } -Descending
    foreach ($d in $dirs) {
        $p = Join-Path $d.FullName "x64\$name"
        if (Test-Path $p) { return $p }
    }
    throw "$name not found. Install the Windows 10/11 SDK."
}

function Invoke-Native([string]$exe, [string[]]$arguments) {
    & $exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "$([IO.Path]::GetFileName($exe)) failed with exit code $LASTEXITCODE" }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path += ";$env:USERPROFILE\.cargo\bin"
}

$makeappx = Find-SdkTool 'makeappx.exe'
$makepri = Find-SdkTool 'makepri.exe'

# Version: Cargo.toml x.y.z -> x.y.z.0 (the Store requires the 4th part to be 0).
$m = Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(\d+)\.(\d+)\.(\d+)' | Select-Object -First 1
if (-not $m) { throw 'Could not read version from Cargo.toml' }
$g = $m.Matches[0].Groups
$version = "$($g[1].Value).$($g[2].Value).$($g[3].Value).0"

$pkg = Get-Content packaging\package.json -Raw | ConvertFrom-Json
$out = Join-Path $root 'target\msix'
$stage = Join-Path $out 'stage'
$assets = Join-Path $out 'assets'

if (-not $SkipBuild) {
    Write-Host '== cargo build --release'
    Invoke-Native 'cargo' @('build', '--release')
}
# Always (re)generate the logos: -SkipBuild only skips compiling the app, and a
# fresh checkout (e.g. CI) has no assets yet.
Write-Host '== generating logos'
Invoke-Native 'cargo' @('run', '--release', '--quiet', '--example', 'gen_assets', '--', $assets) | Out-Null

Write-Host "== staging $version"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage 'Assets') | Out-Null
Copy-Item target\release\askeri-ruller.exe $stage
Copy-Item (Join-Path $assets '*.png') (Join-Path $stage 'Assets')

$manifest = Get-Content packaging\AppxManifest.xml -Raw
$tokens = @{
    'IdentityName'         = $pkg.IdentityName
    'Publisher'            = $pkg.Publisher
    'PublisherDisplayName' = $pkg.PublisherDisplayName
    'DisplayName'          = $pkg.DisplayName
    'Description'          = $pkg.Description
    'Version'              = $version
}
foreach ($k in $tokens.Keys) {
    $v = [System.Security.SecurityElement]::Escape([string]$tokens[$k])
    $manifest = $manifest.Replace("`$$k`$", $v)
}
if ($manifest -match '\$[A-Za-z]+\$') { throw "Unreplaced token in manifest: $($Matches[0])" }
[IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding($false)))

Write-Host '== makepri'
$priconfig = Join-Path $out 'priconfig.xml'
Invoke-Native $makepri @('createconfig', '/cf', $priconfig, '/dq', 'en-US', '/pv', '10.0.0', '/o') | Out-Null
# Single package (not a bundle): keep every scale in one resources.pri instead
# of splitting them out for resource packages.
[xml]$cfg = Get-Content $priconfig
$packaging = $cfg.SelectSingleNode('//packaging')
if ($packaging) { [void]$packaging.ParentNode.RemoveChild($packaging) }
$cfg.Save($priconfig)
Invoke-Native $makepri @('new', '/pr', $stage, '/cf', $priconfig, '/mn', (Join-Path $stage 'AppxManifest.xml'),
    '/of', (Join-Path $stage 'resources.pri'), '/o') | Out-Null

if ($Register) {
    Write-Host '== registering staged folder (Developer Mode)'
    Get-AppxPackage -Name $pkg.IdentityName | Remove-AppxPackage
    Add-AppxPackage -Register (Join-Path $stage 'AppxManifest.xml')
    Write-Host "Registered. Start '$($pkg.DisplayName)' from the Start menu."
    return
}

$msix = Join-Path $out ("AskeriRuller_{0}_x64.msix" -f $version)
Write-Host '== makeappx'
Invoke-Native $makeappx @('pack', '/d', $stage, '/p', $msix, '/o') | Out-Null

if ($CertPfx) {
    $signtool = Find-SdkTool 'signtool.exe'
    Write-Host '== signtool'
    $sign = @('sign', '/fd', 'SHA256', '/f', $CertPfx)
    if ($CertPassword) { $sign += @('/p', $CertPassword) }
    Invoke-Native $signtool ($sign + @($msix))
}

Write-Host "Package: $msix"
