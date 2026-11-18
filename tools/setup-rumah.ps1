# One-time setup of Kanmuri on another Windows laptop (Daffa's home laptop).
# Moved here from the notchdeck repo (19 Nov 2026): Kanmuri switches accounts itself now, so
# NotchDeck is no longer installed, and one left by an earlier run is taken off.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\Codingers\kanmuri\tools\setup-rumah.ps1
#   (docs/SETUP-RUMAH.md has the first-time steps)
#
# What it does, each step safe to run again:
#   1. checks git, node and the GitHub CLI login (the repos are private)
#   2. clones or updates C:\Codingers\kanmuri
#   3. NotchDeck from an earlier setup, if any: stopped, uninstalled, off Windows' startup
#   4. installs Kanmuri from its latest GitHub release and registers its auto-update task
#      (at logon and every 3 hours; also stops Mini Island if this laptop still has it)
#   5. makes the second Claude Code account folder (shares skills/memory/hooks/settings)
#   5b. updates the sopmod workspace sync (Claude sessions travel through it)
#   6. writes the account list Kanmuri switches between (only if this laptop has none yet)
#   7. opens the one login that has to be done by hand in a terminal (Claude Code, 2nd account)
# Logins are never copied between laptops: Claude Desktop's is locked to this Windows account,
# and Claude/Codex tokens are renewed on use, so one token on two laptops logs one of them out.
param([switch]$DryRun)
$ErrorActionPreference = 'Stop'

$root = 'C:\Codingers'
$kanmuri = Join-Path $root 'kanmuri'
$home2 = Join-Path $env:USERPROFILE '.accountdeck\claude\claude-daffa-kyou-team'
function Step($t) { Write-Host ''; Write-Host "== $t" -ForegroundColor Cyan }
function Run($what, [scriptblock]$do) { if ($DryRun) { Write-Host "   (dry run) $what" } else { & $do } }

Step '1. Tools'
foreach ($c in 'git', 'node', 'gh') {
    if (-not (Get-Command $c -ErrorAction SilentlyContinue)) { throw "$c is not installed. Install it first (git, Node.js LTS, GitHub CLI), then run this again." }
    Write-Host "   $c ok"
}
# Through cmd: in Windows PowerShell 5.1 a redirected native stderr becomes an error under 'Stop'
cmd /c "gh auth status >nul 2>&1"
if ($LASTEXITCODE -ne 0) { throw "GitHub CLI is not logged in. Run: gh auth login   (account Decka-tan), then run this again." }
Write-Host '   GitHub login ok'

Step '2. Code'
New-Item -ItemType Directory -Force $root | Out-Null
if (Test-Path (Join-Path $kanmuri '.git')) {
    Run "git pull $kanmuri" { git -C $kanmuri pull --ff-only }
} else {
    Run 'gh repo clone Decka-tan/kanmuri' { gh repo clone Decka-tan/kanmuri $kanmuri }
}

Step '3. NotchDeck (no longer needed)'
$deckExe = Join-Path $env:LOCALAPPDATA 'Programs\NotchDeck\NotchDeck.exe'
$deckUninstall = Join-Path $env:LOCALAPPDATA 'Programs\NotchDeck\Uninstall NotchDeck.exe'
$rk = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if (Test-Path $deckExe) {
    Run 'stop and uninstall NotchDeck' {
        Get-Process NotchDeck -ErrorAction SilentlyContinue | Stop-Process -Force
        if (Test-Path $deckUninstall) { Start-Process $deckUninstall -ArgumentList '/S' -Wait }
    }
} else { Write-Host '   not installed here' }
Run 'NotchDeck off Windows startup' {
    foreach ($n in 'com.decka.notchdeck', 'com.nadimsheikh.accountdeck') { Remove-ItemProperty $rk -Name $n -ErrorAction SilentlyContinue }
}

Step '4. Kanmuri'
# Latest release (built on the office laptop by tools\release.ps1); keeps settings already made here.
Run 'install Kanmuri + auto-update task' { powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $kanmuri 'tools\update.ps1') -Setup }

Step '5. Second Claude Code account (Startup)'
$sync = Join-Path $root '.claude\hooks\sync-akun-claude.js'
if (-not (Test-Path $sync)) { throw "$sync is missing: the workspace sync (R2) has not brought C:\Codingers\.claude\hooks to this laptop yet. Wait for it (or run it), then run this again." }
Run 'make the account folder' { node $sync --setup $home2 }

Step '5b. Claude sessions sync (sopmod workspace sync)'
# Sessions travel through the same R2 workspace sync (sopmod services/workspaceSync.js, 17 Nov 2026).
# This laptop runs it either inside the Sopmod bot (C:\Codingers\sopmod) or, while the office bot is
# up, as the small copy C:\Codingers\sopmod-sync: both get the new code.
$sop = Join-Path $root 'sopmod'
$sopSync = Join-Path $root 'sopmod-sync'
if (Test-Path (Join-Path $sop '.git')) {
    Run 'git pull sopmod' { git -C $sop pull --ff-only }
} else {
    Run 'gh repo clone Decka-tan/m4-sopmod-ii' { gh repo clone Decka-tan/m4-sopmod-ii $sop }
}
if (Test-Path $sopSync) {
    Run 'update sopmod-sync and restart it' {
        foreach ($f in 'scripts\workspace-sync.js', 'services\workspaceSync.js', 'services\vault.js', 'utils\cloudSync.js') {
            Copy-Item (Join-Path $sop $f) (Join-Path $sopSync $f) -Force
        }
        # Only the sync-only process (node ... workspace-sync.js --watch); the bot is left alone
        Get-CimInstance Win32_Process -Filter "Name='node.exe'" | Where-Object { $_.CommandLine -match 'workspace-sync\.js.*--watch' } |
            ForEach-Object { Stop-Process -Id $_.ProcessId -Force }
        Start-Process -WindowStyle Minimized cmd -ArgumentList '/c', ('"' + (Join-Path $sopSync 'start-workspace-sync.bat') + '"')
    }
}
Write-Host '   If this laptop is running the Sopmod bot itself, restart it once so it takes the new sync.'

Step '6. Account list'
# The file NotchDeck kept and Kanmuri reads and switches: %USERPROFILE%\.accountdeck\state.json.
$ad = Join-Path $env:USERPROFILE '.accountdeck'
New-Item -ItemType Directory -Force $ad | Out-Null
$st = Join-Path $ad 'state.json'
if (Test-Path $st) {
    Write-Host '   this laptop already has one: left as it is'
} else {
    Run 'write state.json' {
        $acc = @(
            @{ id = 'desktop-default'; service = 'desktop'; name = 'Startup'; isDefault = $true; dir = (Join-Path $ad 'desktop\desktop-default') }
            @{ id = 'claude-default'; service = 'claude'; name = 'Partner'; isDefault = $true; dir = (Join-Path $env:USERPROFILE '.claude') }
            @{ id = 'codex-default'; service = 'codex'; name = 'Main'; isDefault = $true; dir = (Join-Path $env:USERPROFILE '.codex') }
            @{ id = 'desktop-partner-kyou-ultra'; service = 'desktop'; name = 'Partner'; isDefault = $false; dir = (Join-Path $ad 'desktop\desktop-partner-kyou-ultra') }
            @{ id = 'claude-daffa-kyou-team'; service = 'claude'; name = 'Startup'; isDefault = $false; dir = $home2 }
        )
        foreach ($a in $acc) { if ($a.service -eq 'desktop') { New-Item -ItemType Directory -Force $a.dir | Out-Null } }
        $doc = @{ accounts = $acc; active = @{ desktop = 'desktop-default'; claude = 'claude-default'; codex = 'codex-default' } }
        [IO.File]::WriteAllText($st, ($doc | ConvertTo-Json -Depth 5))
    }
}

Step '7. Logins (once on this laptop)'
$cj = Join-Path $home2 '.credentials.json'
if (-not (Test-Path $cj)) {
    Write-Host '   A terminal opens for Claude Code, 2nd account: pick "Yes, I trust this folder", then log in as daffa@kyou.id.'
    Run 'open the login terminal' {
        Start-Process powershell -ArgumentList '-NoExit', '-Command', "`$env:CLAUDE_CONFIG_DIR = '$home2'; Set-Location '$root'; claude /login"
    }
} else { Write-Host '   Claude Code 2nd account: already logged in' }
Write-Host ''
Write-Host 'Still by hand, once:' -ForegroundColor Yellow
Write-Host '  - Claude Code main folder (~\.claude): logged in as partnerareakyou@gmail.com? If not: claude /login'
Write-Host '  - Claude Desktop: log in as Startup (daffa@kyou.id); then Kanmuri > Accounts > Partner, Claude restarts, log in as Partner'
Write-Host '  - Codex: codex login'
Write-Host '  - Kanmuri > Settings > Accounts: each slot shows who it is signed in as (a warning when two are the same account)'
Write-Host '  - Apple Music web: log in once in Chrome'
Write-Host '  - Kanmuri on the other monitor: Settings > General > Screen'
Write-Host ''
Write-Host 'Done.' -ForegroundColor Green
