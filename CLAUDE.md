# FubuNotch v2: read this first

A rewrite of FubuNotch from scratch, native Rust. `crates/island-core`: the activity contract and the arbiter (pure, no UI, no I/O). `crates/island-webhook`: the local webhook. `app/`: the Slint island (`app/ui/island.slint`). Status and plan: `README.md`.

## The spec decides

- The architecture comes from `SPEC.html`. Until it is in `docs/`, `docs/SPEC-draft.md` stands in for it (written 23 Oct 2026 at the owner's request to build without waiting). When SPEC.html arrives, reconcile: SPEC.html wins, and the draft records what changed.
- Keep the split: the contract and the arbiter know nothing about the UI, so a different UI decision only replaces `app/`.
- Slint's software renderer is deliberate (RAM). Its limits, learnt the hard way: no per-corner radii, no drop shadows, and **a gradient background is not clipped by border-radius** (rounded shapes come out square): rounded things get solid colours; gradients only on paths and bars. Stacked rounded layers leave halos at the edges: one shape with its own border instead. Don't switch renderer for looks without measuring RAM.
- Check UI changes with a picture, even with the PC locked: `FUBUNOTCH_V2_SNAPSHOT=out.png` draws the island into a PNG and exits (no screen needed). Pair it with `FUBUNOTCH_V2_EXPANDED=1|compact`, `FUBUNOTCH_V2_TAB=<tab>`, `FUBUNOTCH_V2_SETTINGS=<page>` (snapshots Settings instead), `FUBUNOTCH_V2_THEME`, `FUBUNOTCH_V2_BG=<picture>` and the in-memory demo data `FUBUNOTCH_V2_DEMO_MUSIC|NOTIFS|TODO|WATER|FOCUS` (`DEMO_MUSIC=nolyrics` for a song without lyrics; `FUBUNOTCH_V2_LINE=1` starts with Line on, in snapshots and live). Never moves the user's mouse.
- The island is a popup tool window: `popup_style` + a subclass that refuses frame styles (winit puts WS_CAPTION back now and then, and Windows then paints a title bar over the island on a click).
- Smoothness: the window never resizes (it is clipped to the island with a window region, `clip_window`); the island springs inside it with Slint `animate`, and pages fade/slide (`Page`). Resizing the OS window per frame is what looked choppy.

## Clean room

- The mascots (`app/assets/sprites/`, written by `scripts/sprites.py` from `pixel-art/holo-tiny/build.py`, which also writes `app/ui/sprites.slint`) are our own pixel art of Shirakami Fubuki, Ookami Mio, Tokoyami Towa, AZKi and Hoshimachi Suisei (Cover / hololive): tiny SD chibis (24 x 26 on a 32 x 32 canvas) drawn with the pixel-art-studio skill after the owner's references, 24 Oct 2026, for personal use. Show them at whole multiples only. Before any team or public release, check hololive's fan-work guidelines with the owner. Settings → Appearance → Mascot turns them off.
- FubuNotch v1 (`../fubunotch-v1`) and Coucou are references for behaviour only. Never copy their code, the Mochi character, sounds or icons: those are Coucou's assets and not ours to ship.
- Read v1 to learn what worked and what bit (click-through, notification mirroring, GSMTC, lyrics), then write it again.

## Identity

- Product "FubuNotch v2", data in `%APPDATA%\FubuNotch v2`, webhook port 47821. Never touch v1's identifier, data folders, pipe (`\\.\pipe\fubunotch-<sid>`) or the `fubunotch-hook` marker in `~/.claude/settings.json`: v1 runs on the same PC.

## Rules carried over from v1

- Secrets live in the OS credential store, never on disk or in git. One exception, on purpose: the webhook token is a file in `%APPDATA%\FubuNotch v2\token` so local scripts can read it; it only guards 127.0.0.1.
- No telemetry. Network calls only to services the user switched on.
- Never block Claude Code: if the app doesn't answer, a hook exits at once.
- Never overwrite `~/.claude/settings.json`: dated backup, merge, show the diff, write only after the user confirms.
- 0 % CPU while the island is hidden; no polling where Windows offers an event.
- Every change keeps `cargo test --workspace` green; check RAM and idle CPU after UI changes (README has the numbers).
