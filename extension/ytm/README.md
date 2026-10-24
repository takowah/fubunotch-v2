# FubuNotch v2 · YouTube Music extension

Chrome (and Edge) don't pass YouTube Music's **shuffle** and **repeat** to Windows, so the island can't show or change them on its own. This small extension does: it reads them from the YouTube Music tab, tells FubuNotch v2 on this PC, and clicks YouTube Music's own buttons when you press them on the island.

It talks only to `127.0.0.1:47821` (FubuNotch v2 on your PC) and only runs on `music.youtube.com`. Nothing else is read or sent.

## Install (unpacked)

1. Get the `extension/ytm` folder (download the repository, or the release's `FubuNotch-v2-ytm-extension.zip`, unzipped).
2. Open `chrome://extensions` (Edge: `edge://extensions`), turn on **Developer mode**.
3. **Load unpacked** → choose the `ytm` folder.
4. Reload your YouTube Music tab.

With FubuNotch v2 running and music playing in YouTube Music, the island's shuffle and repeat buttons light up and follow YouTube Music.
