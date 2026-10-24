# Checkpoint: 23 Oct 2026, 23:35

Where FubuNotch v2 stands after the first evening. Every module from v1's list is in; much of it is not hand-tested yet. Read this before continuing.

## Modules

| Module | What it does | Checked how |
|---|---|---|
| Core | Activity contract, priority arbiter, settings (`crates/island-core`) | 22 unit tests |
| Webhook | 127.0.0.1:47821, token, `/activity`, `/claude/<Event>` (`crates/island-webhook`) | 10 tests incl. a real server and an Allow that comes back |
| Island | Compact line, expanded on hover, tabs (Home + modules in the user's order), × to dismiss | screenshots (`FUBUNOTCH_V2_EXPANDED`) |
| Settings / tray / autostart | Modules on/off and order, each module's switches, Claude Code connect | Settings screenshot; tray and autostart not clicked |
| Music | Now playing (GSMTC), artwork, progress, controls; lyrics LRCLIB → YouTube Music → romaji, split view | real song on screen; live lyrics tests; split view with `FUBUNOTCH_V2_DEMO_MUSIC` |
| Notifications | Windows notifications mirrored, pop-up, history tab, click opens the app (WhatsApp: the chat) | two real toasts in the history; click not tested |
| System | Volume (with bar), Caps Lock, keyboard layout, charger, network, audio output | volume nudged by API; the rest not triggered |
| Focus | Pomodoro: start/pause/skip/reset, rounds, "until 15:40" line, alert at phase end | screenshot; unit tests |
| Water | Today's total vs goal, + glass, undo, reminders inside 09:00–21:00 | screenshot; unit tests (a reminder fired at 23:20: fixed) |
| To-do | Add (Enter), tick, remove; saved in data.json | screenshot with sample tasks; typing not tested |
| Shelf | Files dropped on the island, kept by path; open, Copy (clipboard), remove | screenshot with sample files; dropping not tested |
| Claude Code | Sessions as progress lines; permission requests with Allow / Deny answered for real; hooks installed from Settings with backup + confirm | real relay → island "Allow Bash?" with buttons; Allow round-trip in a test; connect not run |

Release build, all modules on: **37 MB RAM (8 MB private), ~16 ms CPU per 10 s idle**, exe 55 MB, hook relay 142 KB.

## Hand-test first

Hover; tray menu; music buttons; a click on a notification; Caps Lock, layout switch, charger, Wi-Fi, headphones; typing a to-do; dropping a file; Settings → Claude Code → Connect hooks… (then answer a real permission from the island). Run: `cargo run --release -p fubunotch-v2`, or `target\release\fubunotch-v2.exe`.

**Claude Code and v1:** connecting adds v2's hooks next to v1's (v1's stay). With both apps running, both islands may ask for the same permission: run one at a time.

## Decisions waiting for the owner

- **Exe size (55 MB)**: almost all of it is the romaji dictionary (IPADIC, 48 MB). RAM doesn't feel it (Windows loads exe pages on use: 37 MB). Ways out, none free:
  1. keep it embedded (now);
  2. download it once from Lindera.dev (~13 MB) and build it on this PC when romaji is switched on: a small exe, but a third-party download and a one-time wait;
  3. ship it as a folder next to the exe: a small exe, the same total size.
  Recommendation: 1 for personal use, 3 when it is handed to the team.
- **Idle CPU**: 0 ms before the Notifications module, ~16 ms per 10 s after (its 2 s look at the notification database, as in v1). Fine, but measurable.
- **Drag out of the shelf**: not built (Copy covers it); v1 had real OLE drag-out.

## Not in v2 yet

Lyrics holding the island open while their tab is visible; question cards for Claude Code's AskUserQuestion; Water/Focus settings in the Settings window (fixed defaults for now: 2000 ml, 250 ml glass, hourly; 25/5/15 min, 4 rounds); `SPEC.html` reconciliation.

## Test switches (no mouse needed)

| Variable | Effect |
|---|---|
| `FUBUNOTCH_V2_EXPANDED=1` | island stays expanded |
| `FUBUNOTCH_V2_TAB=<id>` | opens on a tab: music, notifications, focus, water, todo, shelf |
| `FUBUNOTCH_V2_SETTINGS=1` | opens Settings at start |
| `FUBUNOTCH_V2_DEMO_MUSIC=1` | a fake song with lyrics instead of the real player |

`scripts\try.ps1` sends three sample activities (`-Clear` ends them). Log: `%APPDATA%\FubuNotch v2\fubunotch-v2.log`. Data: `settings.json`, `data.json`, `token` in the same folder.
