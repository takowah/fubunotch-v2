# FubuNotch v2: spec draft

**Draft, 23 Oct 2026.** Written at the office without `SPEC.html` (it is on the personal laptop), from the four ideas HANDOFF names for v2: activity contract, priority arbiter, local webhook, modules. When `SPEC.html` lands in `docs/`, reconcile: SPEC.html wins, and this file records what changed.

The parts are split so that a different decision in SPEC.html costs as little as possible: the contract and the arbiter (`crates/island-core`) and the webhook (`crates/island-webhook`) know nothing about the UI. Only `app/` knows Slint.

## Stack

Native Rust, no web engine (see `reference/nusa.md`: a native island runs in ~30 MB RAM). UI in **Slint** (winit backend, **software renderer**: 17 MB RAM measured, against 71 MB with FemtoVG's OpenGL context): declarative markup, transparent frameless windows, redraws only when something changes. Fallback if Slint can't draw the island's look: Win32 + Direct2D through the `windows` crate, replacing `app/` only.

## Activity contract

Everything the island can show is an **activity**, whoever sends it (a module inside the app, or anything on the PC through the webhook).

| Field | Type | Meaning |
|---|---|---|
| `id` | string, ≤ 128 | Stable key chosen by the sender. Sending the same id again updates that activity in place. |
| `source` | string, ≤ 64 | Who: `claude-code`, `music`, `discord`, `script`… Shown small, and used for the source's colour. |
| `kind` | `alert` · `ask` · `progress` · `status` · `ambient` | What sort of thing it is (below). |
| `priority` | `critical` · `high` · `normal` · `low` | Decides who gets the island (arbiter). Defaults by kind. |
| `title` | string, ≤ 120 | One line. |
| `body` | string, ≤ 500, optional | Up to three lines. |
| `progress` | number 0..1, optional | A bar, for `progress`. |
| `ttl_ms` | number, optional | Gone this long after its last update. None: stays until ended. Default by kind. |
| `actions` | list of `{id, label}`, ≤ 3, optional | Buttons; a click is reported back to the sender (later: callback URL). |

Kinds and their defaults:

| Kind | For | Default priority | Default ttl |
|---|---|---|---|
| `alert` | Something happened: a message, a finished build | normal | 6 s |
| `ask` | Someone waits for you: an approval, a question | high | none (until ended) |
| `progress` | Something running: a session, a download | low | none |
| `status` | A short state change: charger, headphones | normal | 3 s |
| `ambient` | Background: now playing, the clock | low | none |

Ending: a sender removes its activity (`DELETE /activity/{id}`) or lets the ttl run out.

## Priority arbiter

One activity is **in front** (the island shows it); the rest wait. Pure function of the live activities and the time:

1. Expired activities are dropped.
2. Highest priority wins. Among equals, `ask` beats `alert` beats `status` beats `progress` beats `ambient`; then the most recently updated.
3. **No flicker:** a front activity keeps the island for at least `MIN_SHOW` (2.5 s) against a newcomer of the same or lower priority. A higher priority takes over at once.
4. `ambient` never takes the island from anything else; it is what shows when nothing else is live.
5. The island knows how many others are waiting (a small counter).

Later (needs SPEC.html): quiet hours / focus mode lowering everything below `high`; full-screen apps hiding all but `critical`.

## Local webhook

- `http://127.0.0.1:47821` only (never another interface).
- `POST /activity` with a JSON activity: create or update. `DELETE /activity/{id}`: end. `GET /health`: `ok`.
- Every request but `/health` carries `Authorization: Bearer <token>`. The token is random, made at first start, kept in `%APPDATA%\FubuNotch v2\token` (readable by the user only).
- `Content-Type: application/json` is required: a web page can't send that to localhost without a CORS preflight, which the server never answers, so browsers can't post to it.
- Bodies over 64 KB are refused. Fields over their limits are cut, not refused.

Example (PowerShell):

```powershell
$t = Get-Content "$env:APPDATA\FubuNotch v2\token"
Invoke-RestMethod http://127.0.0.1:47821/activity -Method Post -ContentType application/json `
  -Headers @{ Authorization = "Bearer $t" } `
  -Body '{"id":"build-1","source":"script","kind":"progress","title":"Building","progress":0.4}'
```

## Modules

A module is code inside the app that produces activities through the same contract (no shortcut around the arbiter). First ones, in order: clock (ambient), now playing (GSMTC, ambient), Claude Code (hooks → `ask` / `progress` / `alert`), Windows notifications (`alert`). Each is a separate source the user can turn off.

## Modules and tabs (added 23 Oct, after building)

Most of v1 is not short activities but pages that stay: a player, a list. So next to activities there are **modules**: each one may send activities *and* may have a page, shown as a tab of the expanded island (Home first, then the enabled modules in the user's order). Settings → Modules turns them on and off and orders them; each module has its own switches below. Built so far: Music (tab), Notifications (tab: history), System (no tab: pop-ups only).

## Island

- Top centre of the primary monitor, always on top, no taskbar button.
- The window is exactly the island's size and follows it: no click-through tricks, nothing to poll.
- States: **compact** (one line: the front activity), **expanded** on hover (title, body, progress, actions, the waiting list).
- Look: "Fubuki icy". Deep navy body, ice-white text, cyan glow; the source colour as a small dot. No Fubuki logo, art or voice.
- 0 % CPU when nothing changes: redraws only on a new activity, an expiry (one timer for the next one), hover, or the clock's minute.

## Not decided here (waits for SPEC.html)

Persistence of activities across restarts, action callbacks, settings UI, module list beyond the first four, multi-monitor, sounds.
