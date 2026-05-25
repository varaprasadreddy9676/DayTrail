export function redactUrl(value) {
  if (!value || typeof value !== "string") {
    return null;
  }

  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed = new URL(trimmed);
    parsed.search = "";
    parsed.hash = "";
    return parsed.toString();
  } catch {
    return trimmed.split("#")[0].split("?")[0] || null;
  }
}

export function domainFromUrl(value) {
  if (!value || typeof value !== "string") {
    return null;
  }

  try {
    return new URL(value).hostname.toLowerCase();
  } catch {
    return null;
  }
}

export function toBridgePayload(tab, source) {
  const redactedUrl = redactUrl(tab?.url ?? null);

  return {
    type: "worktrace.browser_tab",
    schemaVersion: 1,
    source,
    capturedAt: new Date().toISOString(),
    title: tab?.title ?? null,
    url: redactedUrl,
    domain: domainFromUrl(redactedUrl),
    tabId: tab?.id ?? null,
    windowId: tab?.windowId ?? null,
    incognito: tab?.incognito ?? false,
  };
}
