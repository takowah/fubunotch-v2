# Keeps Kanmuri up to date on a laptop that doesn't build it (Daffa's home laptop).
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\Codingers\kanmuri\tools\update.ps1 -Setup
# -Setup: installs the latest release now and registers the task "Kanmuri update" (at logon and every
# 3 hours) that runs this script again. Each run: git pull this repo (so the script itself stays
# current), ask GitHub for the latest release (gh, the repo is private), and only when it is newer
# than %LOCALAPPDATA%\Kanmuri\version.txt download it and install it with switch-from-mini-island.ps1,
# which keeps the owner's settings. Nothing is downloaded when there is nothing new.
param([switch]$Setup, [switch]$Force)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $env:LOCALAPPDATA 'Kanmuri'
$verFile = Join-Path $dir 'version.txt'
$log = Join-Path $env:TEMP 'kanmuri-update.log'
function Log($t) { $l = "$(Get-Date -Format s) $t"; $l; Add-Content $log $l }

if ($Setup) {
    $vbs = Join-Path $PSScriptRoot 'update.vbs'
    $action = New-ScheduledTaskAction -Execute (Join-Path $env:WINDIR 'System32\wscript.exe') -Argument ('"' + $vbs + '"')
    $logon = New-ScheduledTaskTrigger -AtLogOn -User "$env:USERDOMAIN\$env:USERNAME"
    $every = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(5) -RepetitionInterval (New-TimeSpan -Hours 3)
    $set = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 15)
    Register-ScheduledTask -TaskName 'Kanmuri update' -Action $action -Trigger $logon, $every -Settings $set -Force | Out-Null
    Log 'task "Kanmuri update" registered (at logon, every 3 hours)'
}

cmd /c "gh auth status >nul 2>&1"
if ($LASTEXITCODE -ne 0) { Log 'gh is not logged in: run gh auth login (Decka-tan)'; exit 1 }
if (Test-Path (Join-Path $repo '.git')) { git -C $repo pull --ff-only --quiet 2>&1 | Out-Null }

$tag = (gh release view --repo Decka-tan/kanmuri --json tagName -q .tagName).Trim()
if (-not $tag) { Log 'no release found'; exit 1 }
$have = if (Test-Path $verFile) { (Get-Content $verFile -Raw).Trim() } else { '' }
if ($tag -eq $have -and -not $Force -and (Test-Path (Join-Path $dir 'kanmuri.exe'))) { Log "up to date ($tag)"; exit 0 }

$tmp = Join-Path $env:TEMP "kanmuri-update-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
    gh release download $tag --repo Decka-tan/kanmuri --pattern 'kanmuri.zip' --dir $tmp
    Expand-Archive (Join-Path $tmp 'kanmuri.zip') -DestinationPath $tmp -Force
    & (Join-Path $PSScriptRoot 'switch-from-mini-island.ps1') -From $tmp
    [IO.File]::WriteAllText($verFile, $tag)
    Log "installed $tag (was '$have')"
} finally { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
