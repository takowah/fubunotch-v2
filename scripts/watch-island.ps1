# Records the island for a while: a picture of the top of its screen every
# $Every seconds, for $Seconds, into scripts\watch\<time>.png, plus the app log's
# new lines. For checking real behaviour (song changes, notices) without
# asking the owner to report each glitch. Run from a normal PowerShell:
#   .\scripts\watch-island.ps1 -Seconds 120
param([int]$Seconds = 240, [double]$Every = 2, [string]$Screen = '')
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
$out = Join-Path $PSScriptRoot 'watch'
New-Item -ItemType Directory -Force $out | Out-Null
Get-ChildItem $out -Filter *.png -ErrorAction SilentlyContinue | Remove-Item -Force
if (-not $Screen) {
    $sf = Join-Path $env:APPDATA 'FubuNotch v2\settings.json'
    try { $Screen = (Get-Content $sf -Raw | ConvertFrom-Json).general.screen } catch {}
}
$s = [System.Windows.Forms.Screen]::AllScreens | Where-Object { $_.DeviceName -eq $Screen } | Select-Object -First 1
if (-not $s) { $s = [System.Windows.Forms.Screen]::PrimaryScreen }
$w = 900; $h = 340
$x = $s.Bounds.X + [int](($s.Bounds.Width - $w) / 2); $y = $s.Bounds.Y
$log = Join-Path $env:APPDATA 'FubuNotch v2\fubunotch-v2.log'
$start = (Get-Item $log -ErrorAction SilentlyContinue).Length
$end = (Get-Date).AddSeconds($Seconds)
$prev = $null
while ((Get-Date) -lt $end) {
    $b = New-Object Drawing.Bitmap $w, $h
    $g = [Drawing.Graphics]::FromImage($b)
    $g.CopyFromScreen($x, $y, 0, 0, $b.Size)
    $g.Dispose()
    $ms = New-Object IO.MemoryStream
    $b.Save($ms, [Drawing.Imaging.ImageFormat]::Png)
    $bytes = $ms.ToArray()
    $hash = [BitConverter]::ToString((New-Object Security.Cryptography.SHA1Managed).ComputeHash($bytes))
    if ($hash -ne $prev) { [IO.File]::WriteAllBytes((Join-Path $out ((Get-Date).ToString('HHmmss') + '.png')), $bytes) }
    $prev = $hash
    $b.Dispose(); $ms.Dispose()
    Start-Sleep -Milliseconds ([int]($Every * 1000))
}
if (Test-Path $log) {
    $fs = [IO.File]::Open($log, 'Open', 'Read', 'ReadWrite')
    $fs.Seek([long]$start, 'Begin') | Out-Null
    $r = New-Object IO.StreamReader($fs)
    [IO.File]::WriteAllText((Join-Path $out 'log.txt'), $r.ReadToEnd())
    $r.Close()
}
"done: " + @(Get-ChildItem $out -Filter *.png).Count + " pictures"
