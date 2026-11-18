# Sets Settings > General > Move automatically on this laptop (off | fullscreen | unused | follow),
# restarting Kanmuri so it reads it. Run outside Claude's container (set-auto-screen.bat via explorer.exe).
param([string]$Mode = 'follow')
$ErrorActionPreference = 'Stop'
$sf = Join-Path $env:APPDATA 'Kanmuri\settings.json'
Get-Process kanmuri -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 800
$j = Get-Content $sf -Raw | ConvertFrom-Json
if (-not $j.general) { $j | Add-Member -NotePropertyName general -NotePropertyValue ([pscustomobject]@{}) }
$j.general | Add-Member -NotePropertyName auto_screen -NotePropertyValue $Mode -Force
[IO.File]::WriteAllText($sf, ($j | ConvertTo-Json -Depth 10), (New-Object Text.UTF8Encoding($false)))
Start-Process (Join-Path $env:LOCALAPPDATA 'Kanmuri\kanmuri.exe')
Add-Type -AssemblyName System.Windows.Forms
"auto_screen: $($j.general.auto_screen); screens: " + (([System.Windows.Forms.Screen]::AllScreens | ForEach-Object { "$($_.DeviceName)$(if ($_.Primary) {' (main)'}) $($_.Bounds)" }) -join ' | ')
