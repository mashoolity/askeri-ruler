<#
.SYNOPSIS
  End-to-end smoke test of the release build on the real desktop.

.DESCRIPTION
  Launches askeri-ruller.exe, then:
    1. opens Settings via the tray double-click message and presses OK
       (checks settings.ini is written and the window closes)
    2. presses the hotkey and checks the cursor's screen row turns red
    3. locks the line with a left click and checks it stays
    4. simulates WM_DISPLAYCHANGE and checks the locked line survives
    5. presses Esc and checks all lines are gone
  Your settings.ini is backed up and restored. Any running instance is stopped.
  Don't touch the mouse/keyboard while it runs (~5 s).

.EXAMPLE
  cargo build --release; powershell -ExecutionPolicy Bypass -File scripts\smoke-test.ps1
#>
param([string]$Exe = (Join-Path $PSScriptRoot '..\target\release\askeri-ruller.exe'))

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class W {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  public struct POINT { public int X; public int Y; }
}
"@
[W]::SetProcessDPIAware() | Out-Null

function Key([byte]$vk, [bool]$up) { [W]::keybd_event($vk, 0, $(if ($up) { 2 } else { 0 }), [UIntPtr]::Zero) }
function Find($cls) { [W]::FindWindowW($cls, [NullString]::Value) }
function RedInRow([int]$y) {
    $x0 = [W]::GetSystemMetrics(76); $w = [W]::GetSystemMetrics(78)   # SM_XVIRTUALSCREEN, SM_CXVIRTUALSCREEN
    $bmp = New-Object System.Drawing.Bitmap $w, 1
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x0, $y, 0, 0, (New-Object System.Drawing.Size $w, 1))
    $n = 0
    for ($x = 0; $x -lt $w; $x++) { $c = $bmp.GetPixel($x, 0); if ($c.R -gt 200 -and $c.G -lt 60 -and $c.B -lt 60) { $n++ } }
    $g.Dispose(); $bmp.Dispose(); $n
}
$failures = 0
function Check([string]$name, [bool]$ok) {
    if ($ok) { Write-Host "PASS  $name" -ForegroundColor Green }
    else { Write-Host "FAIL  $name" -ForegroundColor Red; $script:failures++ }
}

$ini = "$env:APPDATA\AskeriRuller\settings.ini"
$backup = "$ini.smoke-backup"
if (Test-Path $ini) { Copy-Item $ini $backup -Force; Remove-Item $ini }
Get-Process askeri-ruller -ErrorAction SilentlyContinue | Stop-Process
Start-Sleep -Milliseconds 300
$p = Start-Process -FilePath (Resolve-Path $Exe) -PassThru
try {
    Start-Sleep -Milliseconds 1000
    $main = Find 'AskeriRullerMain'
    Check 'app started' ($main -ne [IntPtr]::Zero)

    # 1. Settings window: open via tray double-click, press OK.
    [W]::PostMessageW($main, 0x8001, [IntPtr]::Zero, [IntPtr]0x203) | Out-Null   # WM_TRAY, WM_LBUTTONDBLCLK
    Start-Sleep -Milliseconds 700
    $s = Find 'AskeriRullerSettings'
    Check 'settings window opens' ($s -ne [IntPtr]::Zero)
    [W]::PostMessageW($s, 0x0111, [IntPtr]1, [IntPtr]::Zero) | Out-Null           # WM_COMMAND IDOK
    Start-Sleep -Milliseconds 700
    Check 'settings window closes on OK' ((Find 'AskeriRullerSettings') -eq [IntPtr]::Zero)
    Check 'settings.ini written' (Test-Path $ini)

    # 2. Hotkey shows a line across the monitor at the cursor row.
    # Red pixels already on screen in that row (other apps) are the baseline.
    $pt = New-Object W+POINT; [W]::GetCursorPos([ref]$pt) | Out-Null
    $base = RedInRow $pt.Y
    Key 0x11 $false; Key 0x10 $false; Key 0x58 $false; Key 0x58 $true; Key 0x10 $true; Key 0x11 $true
    Start-Sleep -Milliseconds 400
    $placing = (RedInRow $pt.Y) - $base
    Check "line visible while placing (+$placing px)" ($placing -gt 500)

    # 3. Left click locks the line.
    [W]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [W]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 400
    Check 'line stays after lock' (((RedInRow $pt.Y) - $base) -gt 500)

    # 4. Simulated display change keeps the locked line (600 ms debounce).
    [W]::PostMessageW($main, 0x007E, [IntPtr]32, [IntPtr]::Zero) | Out-Null       # WM_DISPLAYCHANGE
    Start-Sleep -Milliseconds 1200
    Check 'line survives display change' (((RedInRow $pt.Y) - $base) -gt 500)

    # 5. Esc clears everything (back to the baseline, allowing a little noise).
    Key 0x1B $false; Key 0x1B $true
    Start-Sleep -Milliseconds 400
    $after = (RedInRow $pt.Y) - $base
    Check "Esc removes all lines (+$after px)" ($after -lt 20)
    Check 'app still running' (-not $p.HasExited)
}
finally {
    if (-not $p.HasExited) { Stop-Process -Id $p.Id }
    if (Test-Path $backup) { Move-Item $backup $ini -Force }
}

if ($failures) { Write-Host "$failures check(s) failed" -ForegroundColor Red; exit 1 }
Write-Host 'All checks passed' -ForegroundColor Green
