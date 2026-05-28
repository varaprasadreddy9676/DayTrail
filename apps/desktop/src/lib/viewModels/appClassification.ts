export type AppCategory =
  | "work"
  | "communication"
  | "browser"
  | "ai"
  | "system"
  | "utility"
  | "idle"
  | "unknown";

const systemAppNames = [
  "system settings",
  "system preferences",
  "activity monitor",
  "notification center",
  "usernotificationcenter",
  "windowserver",
  "control center",
  "dock",
  "problem reporter",
];

const idleSystemAppNames = [
  "loginwindow",
  "com.apple.loginwindow",
  "screensaver",
  "com.apple.screensaver",
  "lockscreen",
  "com.apple.lockscreen",
];

const utilityAppNames = [
  "finder",
  "preview",
  "textedit",
  "quicktime player",
  "archive utility",
  "font book",
];

const browserAppNames = [
  "safari",
  "firefox",
  "firefox developer edition",
  "google chrome",
  "google chrome canary",
  "chrome",
  "brave browser",
  "brave",
  "microsoft edge",
  "edge",
  "arc",
  "chromium",
  "opera",
  "vivaldi",
  "chatgpt atlas",
];

const aiAppNames = [
  "chatgpt",
  "claude",
  "claude code",
  "gemini",
  "copilot",
  "github copilot",
  "codex",
  "aider",
  "cline",
];

const communicationAppNames = [
  "slack",
  "microsoft teams",
  "teams",
  "mail",
  "outlook",
  "messages",
  "discord",
  "zoom",
  "google meet",
];

const workAppNames = [
  "code",
  "visual studio code",
  "vs code",
  "vs code insiders",
  "cursor",
  "terminal",
  "iterm",
  "iterm2",
  "warp",
  "xcode",
  "intellij idea",
  "webstorm",
  "pycharm",
  "goland",
  "datagrip",
  "zed",
  "sublime text",
  "mysql workbench",
  "postman",
  "docker",
  "figma",
  "notion",
  "obsidian",
];

function normalizeAppName(appName?: string | null): string {
  return (appName ?? "").replace(/[\u200e\u200f\u202a-\u202e]/g, "").trim().toLowerCase();
}

function matchesAny(value: string, names: string[]) {
  return names.some((name) => value === name || value.includes(name));
}

export function classifyApp(appName?: string | null): AppCategory {
  const value = normalizeAppName(appName);
  if (!value) return "unknown";
  if (value === "idle" || value === "away" || value.includes("idle")) return "idle";
  if (isIdleSystemApp(value)) return "idle";
  if (matchesAny(value, systemAppNames)) return "system";
  if (matchesAny(value, utilityAppNames)) return "utility";
  if (matchesAny(value, aiAppNames)) return "ai";
  if (matchesAny(value, communicationAppNames)) return "communication";
  if (matchesAny(value, browserAppNames)) return "browser";
  if (matchesAny(value, workAppNames)) return "work";
  return "unknown";
}

export function isIdleSystemApp(appName?: string | null): boolean {
  const value = normalizeAppName(appName);
  if (!value) return false;
  return idleSystemAppNames.some((name) => value === name || value.includes(name));
}

export function normalizeAppCategory(category?: string | null, appName?: string | null): AppCategory {
  const normalized = (category ?? "").trim().toLowerCase();
  if (
    normalized === "work" ||
    normalized === "communication" ||
    normalized === "browser" ||
    normalized === "ai" ||
    normalized === "system" ||
    normalized === "utility" ||
    normalized === "idle" ||
    normalized === "unknown"
  ) {
    return normalized;
  }
  return classifyApp(appName);
}

export function isWorkAppCategory(category: AppCategory): boolean {
  return !["system", "utility", "idle", "unknown"].includes(category);
}

export function isSimpleVisibleApp(appName?: string | null, category?: string | null): boolean {
  return isWorkAppCategory(normalizeAppCategory(category, appName));
}
