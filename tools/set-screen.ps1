# Puts the island on one screen (Settings > General > Screen does the same).
# Usage: set-screen.ps1 <Windows screen name, see [Windows.Forms.Screen]::AllScreens>   ('' = the main screen). Run outside Claude's container.
param([string]$Screen = ('\' + '\.\DISPLAY1'))
$sf = Join-Path $env:APPDATA 'FubuNotch v2\settings.json'
$j = Get-Content $sf -Raw | ConvertFrom-Json
if (-not $j.general) { $j | Add-Member -NotePropertyName general -NotePropertyValue ([pscustomobject]@{}) -Force }
$j.general | Add-Member -NotePropertyName screen -NotePropertyValue $Screen -Force
[IO.File]::WriteAllText($sf, ($j | ConvertTo-Json -Depth 10), (New-Object Text.UTF8Encoding($false)))
"screen: $Screen"
