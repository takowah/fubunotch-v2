// The extension's side of the page: passes the page's reports to the
// background worker, and the worker's commands to the page.
window.addEventListener("message", (e) => {
  if (e.source !== window || !e.data) return;
  if (e.data.fubunotch !== "state") return;
  // After the extension is reloaded, this copy in an open tab is cut off from
  // it ("Extension context invalidated", thrown at once, not as a rejection):
  // stay quiet until the tab is reloaded.
  if (!chrome.runtime || !chrome.runtime.id) return;
  try {
    chrome.runtime.sendMessage({ type: "state", state: e.data.state }).catch(() => {});
  } catch (_) {}
});

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === "command") window.postMessage({ fubunotch: "command", command: msg.command }, location.origin);
});
