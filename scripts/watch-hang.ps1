# Watches the running island: hangs (the window not answering), every window
# the process owns (new ones, shown/hidden, style), and focus changes, to
# catch what makes a title bar appear over the island.
#   Start-Process powershell -WindowStyle Hidden -ArgumentList '-File', '.\scripts\watch-hang.ps1'
# Log: %APPDATA%\FubuNotch v2\hang-watch.log
Add-Type @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class HW {
  public delegate bool P(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(P p, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumThreadWindows(uint t, P p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint f, uint t, out IntPtr r);
  [StructLayout(LayoutKind.Sequential)] public struct R { public int l,t,r,b; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  public static string Describe(IntPtr h) {
    var t = new StringBuilder(256); GetWindowText(h, t, 256); var c = new StringBuilder(256); GetClassName(h, c, 256); R r; GetWindowRect(h, out r);
    return string.Format("'{0}' [{1}] vis={2} style={3:X8}/{4:X8} rect={5},{6} {7}x{8}", t, c, IsWindowVisible(h) ? 1 : 0, GetLong(h,-16).ToInt64(), GetLong(h,-20).ToInt64(), r.l, r.t, r.r-r.l, r.b-r.t);
  }
  public static Dictionary<long,string> Windows(uint pid) {
    var d = new Dictionary<long,string>();
    EnumWindows((h,l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) d[h.ToInt64()] = Describe(h); return true; }, IntPtr.Zero);
    return d;
  }
}
'@
$log = Join-Path $env:APPDATA 'FubuNotch v2\hang-watch.log'
function Log($m) { Add-Content $log ("{0:HH:mm:ss.fff} {1}" -f (Get-Date), $m) }
Log 'watch started'
$seen = @{}; $lastFg = ''; $tick = 0
while ($true) {
  $p = Get-Process fubunotch-v2 -ErrorAction SilentlyContinue | Select-Object -First 1
  if (-not $p) { Start-Sleep -Milliseconds 1000; continue }
  $now = [HW]::Windows([uint32]$p.Id)
  foreach ($k in $now.Keys) { if ($seen[$k] -ne $now[$k]) { Log ("win {0:X} {1}" -f $k, $now[$k]); $seen[$k] = $now[$k] } }
  foreach ($k in @($seen.Keys)) { if (-not $now.ContainsKey($k)) { Log ("gone {0:X}" -f $k); $seen.Remove($k) } }
  $fg = [HW]::GetForegroundWindow()
  $fgs = '{0:X}' -f $fg.ToInt64()
  if ($fgs -ne $lastFg) { Log ("foreground {0} {1}" -f $fgs, [HW]::Describe($fg)); $lastFg = $fgs }
  if (($tick++ % 5) -eq 0) {
    foreach ($k in $now.Keys) {
      if ($now[$k] -like "*'FubuNotch v2'*") {
        $sw = [Diagnostics.Stopwatch]::StartNew(); $r = [IntPtr]::Zero
        $ok = [HW]::SendMessageTimeout([IntPtr]$k, 0, [IntPtr]::Zero, [IntPtr]::Zero, 0x2, 3000, [ref]$r)
        if ($ok -eq [IntPtr]::Zero -or $sw.ElapsedMilliseconds -gt 300) { Log ("slow reply {0} ms" -f $sw.ElapsedMilliseconds) }
      }
    }
  }
  Start-Sleep -Milliseconds 50
}
