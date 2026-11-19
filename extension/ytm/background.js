// Talks to FubuNotch v2 on this PC: posts YouTube Music's shuffle/repeat to
// http://127.0.0.1:47821/ytm/state, and waits on /ytm/command (a long poll)
// for the island's clicks, which it hands to the YouTube Music tab.
const BASE = "http://127.0.0.1:47821";
let musicTab = null;
let polling = false;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function post(path, body) {
  fetch(`${BASE}/ytm/${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  }).catch(() => {}); // FubuNotch isn't running: nothing to do
}

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (!msg || !sender.tab || msg.type !== "state") return;
  musicTab = sender.tab.id;
  post("state", msg.state);
  if (!polling) poll();
});

chrome.tabs.onRemoved.addListener((id) => {
  if (id === musicTab) musicTab = null;
});

async function poll() {
  polling = true;
  try {
    while (musicTab !== null) {
      let r;
      try {
        r = await fetch(`${BASE}/ytm/command`, { method: "POST", cache: "no-store" }); // a GET would go without the Origin the island checks
      } catch {
        await sleep(5000); // FubuNotch isn't running
        continue;
      }
      if (r.status === 200) {
        const { command } = await r.json();
        if (musicTab !== null) chrome.tabs.sendMessage(musicTab, { type: "command", command }).catch(() => {});
      } else if (r.status !== 204) {
        await sleep(3000);
      }
    }
  } finally {
    polling = false;
  }
}
