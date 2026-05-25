import { createEventBatcher } from "./batching.js";
import { toBridgePayload } from "./privacy.js";

const NATIVE_HOST = "ai.daytrail.desktop";
const RECENT_EVENTS_KEY = "worktraceRecentEvents";

async function getActiveTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab ?? null;
}

async function rememberEvent(payload) {
  const stored = await chrome.storage.local.get(RECENT_EVENTS_KEY);
  const recent = Array.isArray(stored[RECENT_EVENTS_KEY])
    ? stored[RECENT_EVENTS_KEY]
    : [];
  recent.unshift(payload);
  await chrome.storage.local.set({
    [RECENT_EVENTS_KEY]: recent.slice(0, 25),
  });
}

function sendNative(payload) {
  return new Promise((resolve) => {
    if (!chrome.runtime.sendNativeMessage) {
      resolve({ ok: false, error: "native messaging unavailable" });
      return;
    }

    chrome.runtime.sendNativeMessage(NATIVE_HOST, payload, (response) => {
      const error = chrome.runtime.lastError?.message;
      resolve(error ? { ok: false, error } : { ok: true, response });
    });
  });
}

const batcher = createEventBatcher({
  send: sendNative,
});

const lastCaptureByTab = new Map();
const DUPLICATE_WINDOW_MS = 1500;

async function captureAndForward(tab, source) {
  if (tab?.incognito) {
    return { ok: true, skipped: true, ignoredReason: "incognito" };
  }

  const payload = toBridgePayload(tab, source);
  const dedupeKey = `${payload.url ?? ""}|${payload.title ?? ""}`;
  const previous = lastCaptureByTab.get(payload.tabId);
  const now = Date.now();

  if (
    previous?.key === dedupeKey &&
    now - previous.capturedAt < DUPLICATE_WINDOW_MS
  ) {
    return { ok: true, skipped: true, ignoredReason: "duplicate" };
  }

  if (payload.tabId != null) {
    lastCaptureByTab.set(payload.tabId, { key: dedupeKey, capturedAt: now });
  }

  await rememberEvent(payload);
  return batcher.enqueue(payload);
}

chrome.action.onClicked.addListener((tab) => {
  captureAndForward(tab, "action").catch(() => {});
});

chrome.tabs.onActivated.addListener(async () => {
  const tab = await getActiveTab();
  if (tab) {
    await captureAndForward(tab, "tab-activated");
  }
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (!tab.active || tab.incognito) {
    return;
  }

  if (!changeInfo.url && !changeInfo.title && changeInfo.status !== "complete") {
    return;
  }

  captureAndForward({ ...tab, id: tab.id ?? tabId }, "tab-updated").catch(() => {});
});

chrome.windows.onFocusChanged.addListener(async (windowId) => {
  if (windowId === chrome.windows.WINDOW_ID_NONE) {
    return;
  }

  const tab = await getActiveTab();
  if (tab) {
    await captureAndForward(tab, "window-focused");
  }
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type !== "WORKTRACE_CAPTURE_ACTIVE_TAB") {
    return false;
  }

  getActiveTab()
    .then((tab) => captureAndForward(tab, "runtime-message"))
    .then(sendResponse)
    .catch((error) => sendResponse({ ok: false, error: String(error) }));
  return true;
});
