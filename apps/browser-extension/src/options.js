const RECENT_EVENTS_KEY = "worktraceRecentEvents";

const list = document.querySelector("#recent-events");
const button = document.querySelector("#capture-active-tab");

function render(events) {
  list.replaceChildren(
    ...events.map((event) => {
      const item = document.createElement("li");
      const title = event.title || event.url || "Untitled tab";
      item.textContent = `${title} (${event.source})`;
      return item;
    }),
  );
}

async function loadEvents() {
  const stored = await chrome.storage.local.get(RECENT_EVENTS_KEY);
  render(Array.isArray(stored[RECENT_EVENTS_KEY]) ? stored[RECENT_EVENTS_KEY] : []);
}

button.addEventListener("click", async () => {
  await chrome.runtime.sendMessage({ type: "WORKTRACE_CAPTURE_ACTIVE_TAB" });
  await loadEvents();
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area === "local" && changes[RECENT_EVENTS_KEY]) {
    render(changes[RECENT_EVENTS_KEY].newValue ?? []);
  }
});

loadEvents().catch(() => render([]));
