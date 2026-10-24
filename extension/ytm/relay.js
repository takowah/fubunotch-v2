// The extension's side of the page: passes the page's reports to the
// background worker, and the worker's commands to the page.
window.addEventListener("message", (e) => {
  if (e.source !== window || !e.data || e.data.fubunotch !== "state") return;
  chrome.runtime.sendMessage({ type: "state", state: e.data.state }).catch(() => {});
});

chrome.runtime.onMessage.addListener((msg) => {
  if (msg && msg.type === "command") window.postMessage({ fubunotch: "command", command: msg.command }, location.origin);
});
