# 触发翻译并在流式进行中连拍，验证增量输出逐字增长。
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
}
"@
$text = -join @([char]0x4EBA,[char]0x5DE5,[char]0x667A,[char]0x80FD,[char]0x6B63,[char]0x5728,[char]0x6DF1,[char]0x523B,
                 [char]0x6539,[char]0x53D8,[char]0x8F6F,[char]0x4EF6,[char]0x5F00,[char]0x53D1,[char]0x7684,[char]0x65B9,[char]0x5F0F,[char]0x3002)
Set-Clipboard -Value $text

$p = Get-Process -Name 'hy-mt2-translator' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Host 'app not running'; exit 1 }
$h = $p.MainWindowHandle
[U]::ShowWindow($h, 9) | Out-Null
[U]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 900

$r = New-Object U+R
[U]::GetWindowRect($h, [ref]$r) | Out-Null
Write-Host "rect $($r.L),$($r.T) - $($r.Rt),$($r.B)"
[U]::SetCursorPos($r.L + 340, $r.T + 260) | Out-Null
Start-Sleep -Milliseconds 250
[U]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
[U]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
Start-Sleep -Milliseconds 500
[System.Windows.Forms.SendKeys]::SendWait('^a')
Start-Sleep -Milliseconds 150
[System.Windows.Forms.SendKeys]::SendWait('^v')
Start-Sleep -Milliseconds 800

$w = $r.Rt - $r.L; $ht = $r.B - $r.T
function Snap($path) {
  $bmp = New-Object System.Drawing.Bitmap($w, $ht)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $hdc = $g.GetHdc()
  [U]::PrintWindow($h, $hdc, 2) | Out-Null
  $g.ReleaseHdc($hdc)
  $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
Snap (Join-Path $PSScriptRoot 'stream-0-input.png')

[System.Windows.Forms.SendKeys]::SendWait('^{ENTER}')
Start-Sleep -Milliseconds 450
Snap (Join-Path $PSScriptRoot 'stream-1.png')
Start-Sleep -Milliseconds 450
Snap (Join-Path $PSScriptRoot 'stream-2.png')
Start-Sleep -Seconds 4
Snap (Join-Path $PSScriptRoot 'stream-3-final.png')
Write-Host 'saved stream-0-input / 1 / 2 / 3-final'