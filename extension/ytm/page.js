// Runs in YouTube Music's own page (the "main world"): the player bar keeps
// shuffle as a property (shuffleOn) that an ordinary content script can't
// read. Reports shuffle/repeat when they change (and every 15 s, so the
// island knows the tab is still there), and clicks the buttons on command.
(() => {
  const bar = () => document.querySelector("ytmusic-player-bar");
  let last = "";

  function read() {
    const b = bar();
    if (!b) return null;
    return {
      shuffle: !!b.shuffleOn,
      repeat: (b.getAttribute("repeat-mode") || "NONE").toLowerCase(), // none | all | one
      title: (b.querySelector(".title")?.textContent || "").trim(),
    };
  }

  function report(force) {
    const state = read();
    if (!state) return;
    const key = JSON.stringify(state);
    if (force || key !== last) {
      last = key;
      window.postMessage({ fubunotch: "state", state }, location.origin);
    }
  }

  window.addEventListener("message", (e) => {
    if (e.source !== window || !e.data || e.data.fubunotch !== "command") return;
    const b = bar();
    const button = b && b.querySelector(e.data.command === "shuffle" ? ".shuffle button" : ".repeat button");
    if (button) {
      button.click();
      setTimeout(() => report(true), 250);
    }
  });

  setInterval(() => report(false), 1000); // a cheap compare; sends only on change
  setInterval(() => report(true), 15000); // still here
  report(true);
})();
