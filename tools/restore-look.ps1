# One-off (18 Nov 2026): puts back the picture and theme that the old install script wiped.
$sf = Join-Path $env:APPDATA 'Kanmuri\settings.json'
$j = Get-Content $sf -Raw | ConvertFrom-Json
$pic = Get-ChildItem (Join-Path $env:APPDATA 'Kanmuri') -Filter 'background.*' | Select-Object -First 1
if ($pic) { $j.look | Add-Member -NotePropertyName background -NotePropertyValue $pic.FullName -Force }
$j.look | Add-Member -NotePropertyName theme -NotePropertyValue 'azki' -Force
[IO.File]::WriteAllText($sf, ($j | ConvertTo-Json -Depth 10), (New-Object Text.UTF8Encoding($false)))
"restored: theme $($j.look.theme), picture $($j.look.background)"
