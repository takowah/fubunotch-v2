# FubuNotch v2 · YouTube extension

Chrome (and Edge) don't pass YouTube Music's **shuffle** and **repeat** to Windows, so the island can't show or change them on its own. This small extension does: it reads them from the YouTube Music tab, tells FubuNotch v2 on this PC, and clicks YouTube Music's own buttons when you press them on the island.

FubuNotch docks any picture-in-picture window Chrome opens by itself (YouTube's own button, a right-click, Chrome's media controls); the extension adds two things to that. It tells FubuNotch about such a window, so the island's "back to tab" leaves picture-in-picture with the video still playing. And it adds **Watch in island** to YouTube's player (a cyan-topped screen icon, first in the right-hand group of the controls; it says "Watch in FubuNotch island" on hover): one click moves the video into a small picture-in-picture window (Chrome's Document Picture-in-Picture), which FubuNotch docks under the island and shows or hides with it, still playing. Chrome opens such a window only from a click in the page, so the button is on the player, not on the island. Close it with the window's own ✕ or back-to-tab, or right-click the island's screen button: the video goes back into the page as it was.

It talks only to `127.0.0.1:47821` (FubuNotch v2 on your PC) and only runs on `music.youtube.com` and `www.youtube.com`. Nothing else is read or sent.

## Install (unpacked)

1. Get the `extension/ytm` folder (download the repository, or the release's `FubuNotch-v2-ytm-extension.zip`, unzipped).
2. Open `chrome://extensions` (Edge: `edge://extensions`), turn on **Developer mode**.
3. **Load unpacked** → choose the `ytm` folder.
4. Reload your YouTube Music and YouTube tabs. After an update of the folder, press the extension's reload button in `chrome://extensions` and reload the tabs.

With FubuNotch v2 running and music playing in YouTube Music, the island's shuffle and repeat buttons light up and follow YouTube Music.
