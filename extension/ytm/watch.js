// Runs in YouTube's own page (the "main world"): adds a "Watch in island"
// button to the player. One click moves the video into a small
// picture-in-picture window (Chrome's Document Picture-in-Picture), sized for
// FubuNotch's island; FubuNotch then docks that window under the island and
// shows or hides it with the island. Chrome opens such a window only from a
// click in the page, so the button lives here. Closing the window (its own ✕
// or "back to tab", or the island's command) puts the video back exactly
// where it was.
(() => {
  const SIZE = { width: 400, height: 225 };
  // The window's title, for FubuNotch to find it among Chrome's windows.
  const MARKER = "FubuNotch watch";
  let pip = null;
  let home = null; // { video, parent, next, style }

  const videoTitle = () =>
    (document.querySelector("h1.ytd-watch-metadata yt-formatted-string")?.textContent || document.title.replace(/^\(\d+\)\s*/, "").replace(/\s*-\s*YouTube$/, "")).trim();

  function report(open) {
    window.postMessage({ fubunotch: "watch", state: { open, window: open ? MARKER : "", title: open ? videoTitle() : "" } }, location.origin);
  }

  function putBack() {
    if (!home) return;
    const { video, parent, next, style } = home;
    home = null;
    if (next && next.parentNode === parent) parent.insertBefore(video, next);
    else parent.appendChild(video);
    if (style === null) video.removeAttribute("style");
    else video.setAttribute("style", style);
  }

  async function open() {
    if (pip || !("documentPictureInPicture" in window)) return;
    const video = document.querySelector("#movie_player video.html5-main-video") || document.querySelector("#movie_player video");
    if (!video) return;
    const playing = !video.paused;
    pip = await documentPictureInPicture.requestWindow(SIZE);
    try {
      pip.resizeTo(SIZE.width, SIZE.height);
    } catch {}
    pip.document.title = MARKER;
    const body = pip.document.body;
    body.style.cssText = "margin:0;background:#000;overflow:hidden;height:100vh";
    home = { video, parent: video.parentNode, next: video.nextSibling, style: video.getAttribute("style") };
    video.setAttribute("style", "width:100%;height:100%;object-fit:contain;position:static;left:0;top:0");
    body.appendChild(video);
    if (playing && video.paused) video.play().catch(() => {});
    pip.addEventListener("pagehide", () => {
      putBack();
      pip = null;
      report(false);
    });
    report(true);
  }

  // The island's "back to tab".
  window.addEventListener("message", (e) => {
    if (e.source !== window || !e.data || e.data.fubunotch !== "command") return;
    if (e.data.command === "watch-back" && pip) pip.close();
  });

  // Built node by node: YouTube's pages refuse innerHTML (Trusted Types).
  function icon() {
    const NS = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(NS, "svg");
    svg.setAttribute("viewBox", "0 0 36 36");
    svg.setAttribute("width", "100%");
    svg.setAttribute("height", "100%");
    const path = document.createElementNS(NS, "path");
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", "#fff");
    path.setAttribute("stroke-width", "2");
    path.setAttribute("d", "M8 12h20v13H8z M14 8l4 4 4-4 M12 29h12");
    svg.appendChild(path);
    return svg;
  }

  function addButton() {
    const bar = document.querySelector("#movie_player .ytp-right-controls");
    if (!bar || bar.querySelector(".fubunotch-watch")) return;
    const b = document.createElement("button");
    b.className = "ytp-button fubunotch-watch";
    b.title = "Watch in FubuNotch's island";
    b.setAttribute("aria-label", b.title);
    b.appendChild(icon());
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      if (pip) pip.close();
      else open().catch(() => {});
    });
    bar.insertBefore(b, bar.firstChild);
  }

  // YouTube builds the player late and again between videos: a cheap check.
  setInterval(addButton, 2000);
  addButton();
})();
