# Poni parity

The owner, 18 Nov 2026: "kanmuri nyamain poni". Poni (github.com/liimalim/Poni, closed source,
WebView2, most likely Tauri) is the reference for what the island does; Kanmuri keeps its own
engine (Slint, software renderer, ~40 MB RAM: the owner's PC sits at 92 % of 16 GB, so no web
engine). Features below are from Poni's README; copy what it does, not its code or assets.

Order (the owner picked all four, in this order of work): 1 Notch & daily panels, 2 Productivity,
3 AI Chat & Translate, 4 Style & animation. Each item: built, tested, snapshot checked, released.

Status: done · doing · todo. "Have" = Kanmuri already did it before this list.

## 1. Notch & daily panels
- [~] Closed notch: pick what its left and right show (weather, battery, date, next prayer, event,
      task, temperature, CPU, memory, disk, Wi-Fi), a clock in the middle that stays while music
      plays. Done 18 Nov 2026: next prayer, CPU, RAM, disk (stats.rs, read every 2 s only while
      shown). Left: weather (with the Dashboard), event (Calendar), task (To-do), temperature,
      the clock staying while music plays.
- [ ] Auto-hide: always, or while chosen apps are in front; comes down while idle; a hide button /
      double right-click tucks it to a handle. Have: hide when idle, slides away for fullscreen.
- [~] Dashboard tab: weather (Open-Meteo, a switch: network only when on), quick toggles (Wi-Fi,
      Bluetooth, keep awake, dark mode, mute mic, lock), app launcher, day progress, prayer times.
      Done 18 Nov 2026: weather (weather.rs, module "weather" off until turned on, from the
      Prayer place, every 30 min while the page is open), Today (day progress + next prayer), the
      six switches. Tested live: Open-Meteo, dark mode round trip. Left: the app launcher.
- [~] Control tab (like Windows Quick Settings): Wi-Fi and Bluetooth lists (join / connect),
      toggles, brightness and volume sliders, sound output and microphone pickers, device battery.
      Done 18 Nov 2026 (control.rs): volume + mute, brightness (WMI, built-in screen), output and
      microphone pickers (IPolicyConfig), Wi-Fi / Bluetooth radio switches, microphone mute; tested
      live and put back (volume, mic mute, output, brightness). Left: the Wi-Fi network list and
      joining, Bluetooth devices (connect / disconnect, battery).
- [ ] System stats: CPU / GPU load and temperatures (when Windows shares them), memory, disk.

## 2. Productivity
- [ ] Calendar: Google / Outlook / any .ics link, meeting reminders in the notch, month view,
      event details with Join.
- [ ] To-do: due times in everyday words (Indonesian or English), repeating tasks, snooze,
      countdown in the closed notch, tick off from there. Have: a to-do module with no tab now.
- [ ] Notes (with `question :: answer` flashcards).
- [ ] Timer of any length and a stopwatch. Have: Pomodoro.
- [ ] World clock: up to 8 places, meeting planner.
- [ ] Health: breaks, breathing, 20-20-20 eye breaks. Have: water reminders.

## 3. AI Chat & Translate
- [ ] Chat with Gemini / Claude / OpenAI-compatible / Ollama (the user's key, in the OS
      credential store), knows the day and the PC, can add to-dos / reminders / notes / events,
      start a timer, play / pause music, set volume, open apps, with Undo.
- [ ] Translate.

## 4. Style & animation
- [ ] Little animations, each with a switch: a glowing outline for alerts, confetti for a done
      day, karaoke lyrics.
- [ ] Music: live visualizer, colour glow from the cover. Have: cover colour wash, bars.
- [ ] Buddy (3D cat / dog / owl that watches the mouse). Cost check first: 3D in the software
      renderer may cost CPU; measure before shipping.
- [ ] Cute cursors; moving wallpaper.

Not picked (the owner didn't choose them, 18 Nov 2026): Dock / taskbar, Snap zones, Shelf drop
actions and clipboard history, phone transfer, FPS counter, Screen Time, Radio, Bookmarks,
Teleprompter, colour picker, speed test, GitHub PRs, backup, more languages.
