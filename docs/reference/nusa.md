# Reference: Nusa 1.0.0

Nusa (https://rama2555.github.io/nusa/) is a "living notch" for Windows by an independent developer, released 22 Oct 2026. Closed source; studied from its site and its interactive preview only (screenshots in `nusa/`). Reference for behaviour and layout ideas: nothing here is to be copied as code, art or wording.

## What Nusa is

- Native Rust, drawn on the GPU, no web engine. About 3 MB download, ~30 MB RAM, 0 % CPU idle (it draws only while something moves, and waits for Windows events instead of polling).
- Modules: music from any player (GSMTC), synced lyrics (LRCLIB), Pomodoro, water reminder with an hourly chart, file shelf (keeps paths, not copies), to-do with reminders, volume / brightness / Caps Lock / keyboard-language overlays, charger / Wi-Fi / headphones peeks.
- Hover opens, leaving closes. Steps aside for full-screen apps. Tray menu: start with Windows, mascot on/off, minimal notch, about, exit.

FubuNotch v1 already has almost the same module list (v1's `p0-nusa` branch took Nusa as its model). Nusa has nothing about AI agents or notifications.

## 1. What could go into v2

| Idea | From Nusa | Note for v2 |
|---|---|---|
| Resting state | When idle the clock fades and a pair of eyes appears, dozes off, and waves when you come back | Our own idle character, Fubuki-icy style. No Fubuki art: a generic fox-ish shape or snow motif |
| Minimal notch | Closed notch becomes a thin line, for laptops with a thick bezel | Cheap to add, a setting |
| Lyrics split view | The notch widens: player left, three lyric lines right, the current one bright; click a line to seek | Better than v1's lyrics page (separate page + hold-open + ^). Keep v1's romaji + YouTube Music fallback |
| Pages with arrows and dots | Each module (Pomodoro, Water) has pages; chevrons on the sides, dots at the bottom | Fits a module contract: a module declares its pages |
| Header row | Module name left, date + time centre, a small equaliser right, a ring showing activity | A shared header owned by the shell, not each module |
| Full-screen aware | Steps aside for full-screen apps | The arbiter should know "full screen" as a state |
| Event-driven, draw only on change | No polling | Same rule as v1's CLAUDE.md, made a hard requirement |
| Water chart | Hourly bars of today's drinks | Nice-to-have inside the water module |
| Shelf by reference | Remembers file paths | v1 already does drag-out; keep paths, not copies |

What v2 has and Nusa doesn't (the reason v2 exists): AI agent sessions and approvals (Claude Code hooks), Windows notifications mirrored into the island (with WhatsApp chat deep links), and whatever the activity contract / priority arbiter / local webhook in SPEC.html define. Nusa shows one thing at a time with no rule for who wins: the arbiter is v2's edge.

## 2. UI: Nusa's layout, FubuNotch's look

Nusa's look: pure black notch, inverted "shoulder" corners joining the screen's top edge, white text, blue / violet gradient glow, chunky rounded pills for buttons.

v2 keeps the layout ideas and changes the look to "Fubuki icy" (HANDOFF): white / ice blue, soft rounded corners, cyan glow, no Fubuki logo, art or voice.

| Element | Nusa | FubuNotch v2 |
|---|---|---|
| Notch body | Black, hard top edge with concave shoulders | Deep navy (#0b1220 or near) or frosted ice; keep the concave shoulders, they read as "part of the screen" |
| Accent | Blue → violet gradient | Cyan glow (#5fd4ff) with ice white (#e8f6ff) |
| Primary button | Red / blue filled pill | Ice-white pill with navy text, cyan focus glow |
| Lyrics | Bright current line on a blue gradient half | Current line ice white, others 35 % opacity; a faint snow-drift gradient behind |
| Resting | Two glowing eyes | Own idle character (to design) |
| Typography | Rounded geometric sans | Pick our own (Segoe UI Variable is free on Windows 11 and sharp at every scale) |

## For SPEC.html: the stack question

Nusa's numbers (3 MB, ~30 MB RAM, native GPU drawing) show what a web-engine-free island costs. v1 (Tauri + WebView2) is 55 MB on disk with the romaji dictionary and runs WebView2 processes. If "light" is a goal of v2, SPEC.html should decide between Tauri (fast to build, HTML/CSS UI) and native Rust drawing (lighter, more work per screen) before modules are written.
