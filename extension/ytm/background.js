// Talks to FubuNotch v2 on this PC: posts YouTube Music's shuffle/repeat to
// http://127.0.0.1:47821/ytm/state and the "Watch in island" window's state to
// /ytm/watch, and waits on /ytm/command (a long poll) for the island's clicks,
// which it hands to the tab they are for: "watch-…" to the YouTube tab whose
// video is in the island, the others to the YouTube Music tab.
const BASE = "http://127.0.0.1:47821";
let musicTab = null;
let watchTab = null;
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
  if (!msg || !sender.tab) return;
  if (msg.type === "state") {
    musicTab = sender.tab.id;
    post("state", msg.state);
  } else if (msg.type === "watch") {
    if (msg.state && msg.state.open) watchTab = sender.tab.id;
    else if (watchTab === sender.tab.id) watchTab = null;
    post("watch", msg.state);
  } else {
    return;
  }
  if (!polling) poll();
});

chrome.tabs.onRemoved.addListener((id) => {
  if (id === musicTab) musicTab = null;
  if (id === watchTab) {
    watchTab = null;
    post("watch", { open: false });
  }
});

async function poll() {
  polling = true;
  try {
    while (musicTab !== null || watchTab !== null) {
      let r;
      try {
        r = await fetch(`${BASE}/ytm/command`, { method: "POST", cache: "no-store" }); // a GET would go without the Origin the island checks
      } catch {
        await sleep(5000); // FubuNotch isn't running
        continue;
      }
      if (r.status === 200) {
        const { command } = await r.json();
        const tab = command.startsWith("watch-") ? watchTab : musicTab;
        if (tab !== null) chrome.tabs.sendMessage(tab, { type: "command", command }).catch(() => {});
      } else if (r.status !== 204) {
        await sleep(3000);
      }
    }
  } finally {
    polling = false;
  }
}
