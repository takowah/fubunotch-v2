# Kanmuri on another laptop

Kanmuri is the island at the top of the screen (music and lyrics, notifications, Control, the
Accounts tab). It updates itself from its GitHub releases (task "Kanmuri update": at logon and
every 3 hours), and switches the Claude Desktop, Claude Code and Codex accounts itself. NotchDeck
is no longer needed: the setup takes it off a laptop that still has it.

## Run once

In PowerShell, not as admin:

```
gh auth login
gh repo clone Decka-tan/kanmuri C:\Codingers\kanmuri
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Codingers\kanmuri\tools\setup-rumah.ps1
```

Needs git, Node.js LTS and the GitHub CLI. The workspace sync (R2) has to have run once on the
laptop too, so `C:\Codingers\.claude\hooks\sync-akun-claude.js` is there.

Running it again is safe: it pulls the latest code and reinstalls, and it leaves an existing
account list alone. `-DryRun` shows what it would do.

## Logins: once per laptop, never copied

Claude Desktop's login is locked to the Windows account it was made on, and Claude / Codex
tokens are renewed on use, so the same token on two laptops logs one of them out.

- Claude Code, second account (Startup): the script opens a terminal for it (log in as daffa@kyou.id).
- Claude Code, main folder `~\.claude` (Partner): partnerareakyou@gmail.com (`claude /login` if not).
- Claude Desktop: log in as Startup, then Kanmuri > Accounts > Partner; Claude restarts, log in as
  Partner. From then on the switch keeps both.
- Codex: `codex login`.
- Kanmuri > Settings > Accounts shows who each slot is signed in as, and warns when two slots are
  the same account; Sign in… there redoes one.
- Apple Music web: log in once in Chrome.

## Also once

- Kanmuri on the other monitor: Settings > General > Screen.
