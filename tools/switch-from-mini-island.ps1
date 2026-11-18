# Daffa's laptops: Kanmuri (this fork of FubuNotch v2, drawn as Mini Island) takes over from Mini Island.
# Run it through switch-from-mini-island.bat via explorer.exe (Claude's own shell lives in the
# app's MSIX container, whose %LOCALAPPDATA% / %APPDATA% / HKCU writes nobody else sees).
#   1. copies the release exe to %LOCALAPPDATA%\FubuNotch (so a rebuild never fights a running copy)
#   2. settings: lyrics + YouTube Music lyrics + romaji on, Graphite look, no mascot, start with Windows
#   3. Mini Island: stopped, its Startup shortcut moved aside (Mini Island itself stays installed;
#      put the shortcut back from %LOCALAPPDATA%\MiniIsland\startup-off to undo)
#   4. starts Kanmuri
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$src = Join-Path $repo 'target\release\fubunotch-v2.exe'
$dir = Join-Path $env:LOCALAPPDATA 'Kanmuri'
$exe = Join-Path $dir 'kanmuri.exe'
Remove-Item (Join-Path $env:LOCALAPPDATA 'FubuNotch') -Recurse -Force -ErrorAction SilentlyContinue

Get-Process fubunotch-v2, kanmuri -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force $dir | Out-Null
# A killed copy lets go of its exe a moment later: try for up to 10 s.
for ($i = 0; ; $i++) {
    try { Copy-Item $src $exe -Force -ErrorAction Stop; break }
    catch { if ($i -ge 20) { throw }; Start-Sleep -Milliseconds 500 }
}
"exe: $exe"

$data = Join-Path $env:APPDATA 'Kanmuri'
$old = Join-Path $env:APPDATA 'FubuNotch v2'
# Renamed from FubuNotch v2: carry its folder over once (the app does the same).
if (-not (Test-Path $data) -and (Test-Path $old)) { New-Item -ItemType Directory -Force $data | Out-Null; Get-ChildItem $old -File | Copy-Item -Destination $data }
$sf = Join-Path $data 'settings.json'
New-Item -ItemType Directory -Force $data | Out-Null
$j = if (Test-Path $sf) { Get-Content $sf -Raw | ConvertFrom-Json } else { [pscustomobject]@{} }
foreach ($sec in 'music', 'look') { if (-not $j.$sec) { $j | Add-Member -NotePropertyName $sec -NotePropertyValue ([pscustomobject]@{}) -Force } }
$j.music | Add-Member -NotePropertyName lyrics -NotePropertyValue $true -Force
$j.music | Add-Member -NotePropertyName lyrics_youtube -NotePropertyValue $true -Force
$j.music | Add-Member -NotePropertyName romaji -NotePropertyValue $true -Force
$j.look | Add-Member -NotePropertyName theme -NotePropertyValue 'graphite' -Force
$j.look | Add-Member -NotePropertyName mascot -NotePropertyValue $false -Force
$j.look | Add-Member -NotePropertyName background -NotePropertyValue '' -Force
$j | Add-Member -NotePropertyName autostart -NotePropertyValue $true -Force
Copy-Item $sf "$sf.bak-before-mini-look" -ErrorAction SilentlyContinue
[IO.File]::WriteAllText($sf, ($j | ConvertTo-Json -Depth 10), (New-Object Text.UTF8Encoding($false)))
"settings: lyrics on, YouTube Music on, romaji on, graphite, no mascot, autostart"

$rk = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
Remove-ItemProperty $rk -Name 'FubuNotch v2' -ErrorAction SilentlyContinue
Set-ItemProperty $rk -Name 'Kanmuri' -Value "`"$exe`""
"autostart: $((Get-ItemProperty $rk).'Kanmuri')"

Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='wscript.exe'" |
    Where-Object { $_.CommandLine -match 'MiniIsland' -and $_.ProcessId -ne $PID } |
    ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
$lnk = Join-Path ([Environment]::GetFolderPath('Startup')) 'Mini Island.lnk'
if (Test-Path $lnk) {
    $off = Join-Path $env:LOCALAPPDATA 'MiniIsland\startup-off'
    New-Item -ItemType Directory -Force $off | Out-Null
    Move-Item $lnk (Join-Path $off 'Mini Island.lnk') -Force
    "Mini Island: stopped, Startup shortcut moved to $off"
} else { 'Mini Island: stopped (no Startup shortcut)' }

Start-Process $exe
Start-Sleep 4
"running: " + @(Get-Process kanmuri -ErrorAction SilentlyContinue).Count
