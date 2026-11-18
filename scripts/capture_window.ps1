# DPI-aware window capture: P/Invoke SetProcessDPIAware before GDI+ so
# CopyFromScreen records true physical pixels, then save the target window
# rect (also queried un-virtualized) to a PNG.
# Usage: powershell -NoProfile -File capture_window.ps1 -ProcessName rustytube -Out path.png
param(
    [string]$ProcessName = "rustytube",
    [string]$Out = "E:\dev\bzvibs\dist\capture.png"
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class DpiAwareShot {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L; public int T; public int R; public int B; }
}
"@

[DpiAwareShot]::SetProcessDPIAware() | Out-Null
Add-Type -AssemblyName System.Drawing

$p = Get-Process $ProcessName -ErrorAction Stop | Select-Object -First 1
[DpiAwareShot]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 600

$r = New-Object DpiAwareShot+RECT
[DpiAwareShot]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
$w = $r.R - $r.L
$h = $r.B - $r.T
Write-Output "window_physical: ${w}x${h} at $($r.L),$($r.T)"

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $h))
$bmp.Save($Out)
$g.Dispose()
$bmp.Dispose()
Write-Output "saved: $Out"
