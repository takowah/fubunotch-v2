// Talks to FubuNotch v2 on this PC: posts YouTube Music's shuffle/repeat to
// http://127.0.0.1:47821/ytm/state, and waits on /ytm/command (a long poll)
// for the island's clicks, which it hands to the YouTube Music tab.
const BASE = "http://127.0.0.1:47821";
let tabId = null;
let polling = false;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (!msg || msg.type !== "state" || !sender.tab) return;
  tabId = sender.tab.id;
  fetch(`${BASE}/ytm/state`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(msg.state),
  }).catch(() => {}); // FubuNotch isn't running: nothing to do
  if (!polling) poll();
});

chrome.tabs.onRemoved.addListener((id) => {
  if (id === tabId) tabId = null;
});

async function poll() {
  polling = true;
  try {
    while (tabId !== null) {
      let r;
      try {
        r = await fetch(`${BASE}/ytm/command`, { cache: "no-store" });
      } catch {
        await sleep(5000); // FubuNotch isn't running
        continue;
      }
      if (r.status === 200) {
        const { command } = await r.json();
        chrome.tabs.sendMessage(tabId, { type: "command", command }).catch(() => (tabId = null));
      } else if (r.status !== 204) {
        await sleep(3000);
      }
    }
  } finally {
    polling = false;
  }
}
