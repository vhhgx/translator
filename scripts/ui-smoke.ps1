# 聚焦窗口 -> 粘贴中文 -> Ctrl+Enter 翻译 -> 截图
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class U {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool c);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, IntPtr e);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
}
"@
$text = -join @([char]0x4ECA,[char]0x5929,[char]0x5929,[char]0x6C14,[char]0x771F,[char]0x597D,
                 [char]0xFF0C,[char]0x6211,[char]0x4EEC,[char]0x4E00,[char]0x8D77,[char]0x53BB,
                 [char]0x516C,[char]0x56ED,[char]0x6563,[char]0x6B65,[char]0x5427,[char]0x3002)
Set-Clipboard -Value $text

$p = Get-Process -Name 'hy-mt2-translator' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { Write-Host 'app not running'; exit 1 }
$h = $p.MainWindowHandle

# 强制聚焦：把当前前台线程与本窗口线程绑定后再 SetForegroundWindow
$fg = [U]::GetForegroundWindow()
$fgThread = [U]::GetWindowThreadProcessId($fg, [IntPtr]::Zero)
$myThread = [U]::GetWindowThreadProcessId($h, [IntPtr]::Zero)
[U]::AttachThreadInput($myThread, $fgThread, $true) | Out-Null
[U]::ShowWindow($h, 9) | Out-Null   # SW_RESTORE
[U]::BringWindowToTop($h) | Out-Null
[U]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 700
$nowFg = [U]::GetForegroundWindow()
Write-Host "foreground match: $($nowFg -eq $h)"

$r = New-Object U+R
[U]::GetWindowRect($h, [ref]$r) | Out-Null
[U]::SetCursorPos($r.L + 340, $r.T + 220) | Out-Null
Start-Sleep -Milliseconds 200
[U]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
[U]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
Start-Sleep -Milliseconds 500

[System.Windows.Forms.SendKeys]::SendWait('^a')
Start-Sleep -Milliseconds 200
[System.Windows.Forms.SendKeys]::SendWait('^v')
Start-Sleep -Milliseconds 800
[System.Windows.Forms.SendKeys]::SendWait('^{ENTER}')
Write-Host 'pasted + Ctrl+Enter, waiting 15s...'
Start-Sleep -Seconds 15

$w = $r.Rt - $r.L; $ht = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap($w, $ht)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[U]::PrintWindow($h, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc)
$out = Join-Path $PSScriptRoot 'app-translate.png'
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $bmp.Dispose()
Write-Host "saved $out"