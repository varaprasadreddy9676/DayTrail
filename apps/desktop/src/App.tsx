import { invoke as invokeTauriCore } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { FormEvent, MouseEvent, ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { buildActivityView } from "./lib/viewModels/activityViewModel";
import { buildAiImpactView } from "./lib/viewModels/aiImpactViewModel";
import { isSimpleVisibleApp } from "./lib/viewModels/appClassification";
import { buildHourTimelineView, normalizeExperienceSettings, type ExperienceSettingsLike } from "./lib/viewModels/hourTimelineViewModel";
import { buildDeterministicReportMarkdown, buildReportView } from "./lib/viewModels/reportViewModel";
import { buildReviewView } from "./lib/viewModels/reviewViewModel";
import { buildSettingsView } from "./lib/viewModels/settingsViewModel";
import { buildTodayView } from "./lib/viewModels/todayViewModel";

// ── Toast system ────────────────────────────────────────────────────────────
type ToastKind = "success" | "error" | "info" | "warning";
type Toast = { id: number; kind: ToastKind; title: string; message?: string };
let _toastSeq = 0;
function nextToastId() { return ++_toastSeq; }

type ViewKey =
  | "today"
  | "hour"
  | "apps"
  | "loops"
  | "ai"
  | "automation"
  | "restore"
  | "rituals"
  | "review"
  | "memory"
  | "settings";
type RitualKey = "daily";

type WorkSession = {
  id: string;
  time: string;
  title: string;
  project: string;
  status: string;
  tools: string;
  confidence: string;
  evidenceEventIds: string[];
};

type ActionItem = {
  id: string;
  title: string;
  source: string;
  state: "open" | "done" | "snoozed";
};

type StreamEvent = {
  id: string;
  title: string;
  timeSpan: string;
  sourceType: string;
  width: number;
};

type Stream = {
  id: string;
  title: string;
  summary: string;
  status: string;
  sessions: string;
  eventIds: string[];
  events: StreamEvent[];
};

type Note = {
  id: number | string;
  text: string;
  time: string;
  context: string;
};

type SourceFeedItem = {
  id: string;
  label: string;
  selected: boolean;
};

type AiThread = {
  id: string;
  tool: string;
  title: string;
  clue: string;
};

type MemoryFact = {
  id: string;
  kind: "quickNote" | "commitment" | "aiOutput" | "meeting" | "fieldVisit";
  rawId: string | number;
  date: string;
  title: string;
  source: string;
};

type BackendQuickNote = {
  id: number;
  body: string;
  source?: string | null;
  projectPath?: string | null;
  createdAt: string;
};

type BackendSettings = {
  browserBridgeEnabled: boolean;
  terminalBridgePath?: string | null;
  launchAtLogin?: boolean;
  excludedDomains: string[];
  aiProvider?: string;
  aiModel?: string;
  aiEndpoint?: string;
  aiRedactSecrets?: boolean;
  fullClipboardHistory?: boolean;
  experienceMode?: "simple" | "pro";
  showSystemApps?: boolean;
  showRawEvents?: boolean;
  showCaptureConfidence?: boolean;
  showAiDetails?: "summary" | "detailed";
};

type BackendStorageLocationInfo = {
  databasePath: string;
  backupDir: string;
};

type BackendTerminalBridgeInstallResult = {
  shell: string;
  profilePath: string;
  bridgeScriptPath: string;
  metadataPath: string;
  alreadyInstalled: boolean;
  message: string;
};

type BackendDatabaseTransferResult = {
  path: string;
  bytes: number;
  generatedAt: string;
  preRestoreBackupPath?: string | null;
};

type BackendCapturePermissionSummary = {
  platform: string;
  setupRequired: boolean;
  allRequiredGranted: boolean;
  appPath?: string | null;
  executablePath?: string | null;
  restartRecommended?: boolean;
  diagnostics?: string[];
  checks: BackendCapturePermissionCheck[];
};

type BackendCapturePermissionCheck = {
  id: string;
  label: string;
  required: boolean;
  status: string;
  detail: string;
  settingsLabel?: string | null;
  settingsUrl?: string | null;
  actionLabel?: string | null;
};

type BackendTodaySnapshot = {
  localDate: string;
  tasks: Array<{ id: number; title: string; source?: string | null; projectPath?: string | null }>;
  quickNotes: BackendQuickNote[];
  commitments: Array<{ id: string; title: string; source?: string | null; dueAt?: number | null }>;
  pendingReplies: Array<{ id: string; subject: string; latestSender?: string | null }>;
  aiOutputs: Array<{ id: string; title: string; outputType: string; status: string; aiAssisted: boolean }>;
  meetings: Array<{ id: string; title: string; summary?: string | null }>;
  fieldVisits: Array<{ id: string; clientLabel?: string | null; locationLabel?: string | null; status: string }>;
  idleBlocks: Array<{ id: string; durationMs: number; classified: boolean }>;
  sourceEvents?: BackendSourceEvent[];
  aiUsageSummary?: BackendAiUsageSummary;
  appUsageSummary?: BackendAppUsageSummary;
  automationCandidates?: BackendAutomationCandidate[];
  captureHealth?: BackendCaptureHealthSummary;
  unclosedLoopInbox?: BackendUnclosedLoopItem[];
  aiOutputLedger?: BackendAiOutputLedgerItem[];
  menuBarSummary?: BackendMenuBarSummary;
  loopRisks?: Array<{
    id: string;
    riskType: string;
    title: string;
    source: string;
    reason: string;
    priority: number;
  }>;
  workSessions: Array<{
    id: string;
    title: string;
    status: string;
    startedAt: number;
    endedAt: number;
    durationMs: number;
    aiUsed: boolean;
    confidencePercent: number;
    summary?: string | null;
    evidenceEventIds?: string[];
  }>;
  parallelStreams: Array<{
    id: string;
    title: string;
    status: string;
    startedAt: number;
    endedAt?: number | null;
    summary?: string | null;
    eventIds: string[];
    nextAction?: string | null;
  }>;
  nextBestAction?: {
    title: string;
    reason: string;
    sourceType: string;
    sourceId: string;
    priority: number;
  } | null;
  pauseState: { paused: boolean };
  settings: BackendSettings;
  projectContext?: { path: string; source: string } | null;
  activeWorkContext?: BackendActiveWorkContext | null;
};

type BackendActiveWorkContext = {
  client: string | null;
  project: string | null;
  task: string | null;
  ticketId: string | null;
  billable: boolean;
  updatedAt: number;
};

type BackendWorkSessionSummary = {
  id: string;
  title: string;
  status: string;
  startedAt: number;
  endedAt: number;
  durationMs: number;
  aiUsed: boolean;
  confidencePercent: number;
  summary: string | null;
  evidenceEventIds: string[];
  billingStatus: string;
  billable: boolean;
  clientLabel: string | null;
  projectLabel: string | null;
  ticketId: string | null;
  reviewNotes: string | null;
};

type BackendSourceEvent = {
  id: string;
  source: string;
  eventType: string;
  app?: string | null;
  title?: string | null;
  domain?: string | null;
  urlRedacted?: string | null;
  workspaceKey?: string | null;
  startedAt: number;
  endedAt: number;
  durationMs: number;
  sensitivity: string;
  metadataJson?: string | null;
  createdAt: number;
};

type BackendAiToolUsage = {
  tool: string;
  durationMs: number;
  events: number;
  contexts: string[];
};

type BackendAiUsageSummary = {
  totalDurationMs: number;
  tools: BackendAiToolUsage[];
  contexts: Array<{ label: string; durationMs: number; events: number }>;
  outputCount: number;
};

type BackendFileUsage = {
  name: string;
  context: string | null;
  durationMs: number;
  events: number;
};

type BackendAppUsageSummary = {
  totalDurationMs: number;
  apps: Array<{
    app: string;
    category?: string | null;
    durationMs: number;
    events: number;
    projects: Array<{
      label: string;
      contexts?: string[];
      durationMs: number;
      events: number;
      aiTools: BackendAiToolUsage[];
      examples: string[];
    }>;
    aiTools: BackendAiToolUsage[];
    files: BackendFileUsage[];
  }>;
};

type BackendAutomationCandidate = {
  id: string;
  title: string;
  signal: string;
  reason: string;
  occurrences: number;
  durationMs: number;
  exampleSources: string[];
  suggestedSteps?: string[];
  relatedAiTools?: string[];
};

type HourFileEntry = {
  name: string;
  context: string | null;
  durationMs: number;
};

type HourAppBreakdown = {
  app: string;
  durationMs: number;
  events: number;
  contexts: string[];
  aiTools: string[];
  examples: string[];
  files: HourFileEntry[];
};

type IdleGap = {
  startMs: number;
  endMs: number;
  durationMs: number;
};

type HourBucket = {
  hour: number;
  label: string;
  durationMs: number;
  events: BackendSourceEvent[];
  apps: HourAppBreakdown[];
  aiTools: string[];
  idleGaps: IdleGap[];
};

type ProjectUsageBreakdown = {
  key: string;
  label: string;
  durationMs: number;
  events: number;
  apps: Array<{ app: string; durationMs: number }>;
  aiTools: string[];
  contexts: string[];
};

type BackendCaptureHealthSummary = {
  status: string;
  headline: string;
  updatedAt: number;
  checks: Array<{
    id: string;
    label: string;
    status: string;
    detail: string;
    lastSeenAt?: number | null;
    evidenceCount: number;
    action?: string | null;
  }>;
};

type BackendUnclosedLoopItem = {
  id: string;
  category: string;
  title: string;
  detail: string;
  source: string;
  risk: string;
  status: string;
  primaryAction: string;
  evidenceIds: string[];
};

type BackendAiOutputLedgerItem = {
  id: string;
  title: string;
  tool: string;
  sourceContext: string;
  destination: string;
  status: string;
  durationMs: number;
  evidenceIds: string[];
  evidence: string;
};

type BackendMenuBarSummary = {
  currentWork: string;
  detail: string;
  captureState: string;
  aiUsage: string;
  openLoops: number;
  nextAction?: string | null;
  updatedAt: number;
};

type BackendExportPayload = {
  generatedAt: string;
  fromDate?: string | null;
  toDate?: string | null;
  timesheetRows: Array<{
    id: string;
    localDate: string;
    durationMs: number;
    title: string;
    app: string;
    projectOrClient: string;
    aiUsed: boolean;
    aiTools: string[];
  }>;
  aiContributionRows: Array<{
    id: string;
    tool: string;
    app: string;
    projectOrClient: string;
    durationMs: number;
    title: string;
    destination: string;
    status: string;
  }>;
  sourceEvents: BackendSourceEvent[];
  workSessions: BackendTodaySnapshot["workSessions"];
  idleBlocks: BackendTodaySnapshot["idleBlocks"];
  aiUsage: Array<{
    id: string;
    provider?: string | null;
    toolName?: string | null;
    threadTitle?: string | null;
    contextId?: string | null;
    promptSummary?: string | null;
    outputSummary?: string | null;
    startedAt?: number | null;
    endedAt?: number | null;
    durationMs?: number | null;
    metadataJson?: string | null;
    createdAt: number;
  }>;
  appUsageSummary: BackendAppUsageSummary;
  aiUsageSummary: BackendAiUsageSummary;
  automationCandidates: BackendAutomationCandidate[];
  unclosedLoopInbox: BackendUnclosedLoopItem[];
};

type BackendReport = {
  bodyMarkdown: string;
  usedAi?: boolean;
  fallbackReason?: string | null;
};

type BackendSearchResult = {
  entityType: string;
  entityId: string;
  title: string;
  snippet: string;
  source?: string | null;
  score: number;
};

type TauriGlobal = {
  core?: {
    invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  };
  invoke?<T>(command: string, args?: Record<string, unknown>): Promise<T>;
};

type TauriInternals = {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  transformCallback?: unknown;
};

declare global {
  interface Window {
    __TAURI__?: TauriGlobal;
    __TAURI_INTERNALS__?: TauriInternals;
  }
}

function hasTauriRuntime() {
  return Boolean(getTauriInvoke());
}

function hasTauriEventRuntime() {
  return typeof window.__TAURI_INTERNALS__?.transformCallback === "function";
}

function getTauriInvoke() {
  return (
    window.__TAURI__?.core?.invoke ??
    window.__TAURI__?.invoke ??
    window.__TAURI_INTERNALS__?.invoke ??
    (typeof window.__TAURI_INTERNALS__ === "object" ? invokeTauriCore : null)
  );
}

function errorMessage(error: unknown) {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) {
      return message;
    }
  }

  try {
    const serialized = JSON.stringify(error);
    if (serialized && serialized !== "{}") {
      return serialized;
    }
  } catch {
    // Fall through to the generic message.
  }

  return "No error detail returned by desktop bridge";
}

type AiConfig = {
  provider:
    | "Ollama Local"
    | "LM Studio"
    | "OpenAI Compatible"
    | "OpenAI"
    | "OpenRouter"
    | "Groq"
    | "Gemini"
    | "Anthropic"
    | "NVIDIA NIM"
    | "Custom API";
  model: string;
  endpoint: string;
  apiKey: string;
  redactSecrets: boolean;
  fullClipboard: boolean;
};

type WorkspaceFolder = {
  id: string;
  label: string;
  path: string;
  selected: boolean;
};

const navigation: Array<{ id: ViewKey; label: string; icon: IconName }> = [
  { id: "today", label: "Today", icon: "layout" },
  { id: "apps", label: "Activity", icon: "apps" },
  { id: "ai", label: "AI Impact", icon: "ritual" },
  { id: "review", label: "Needs Review", icon: "archive" },
  { id: "rituals", label: "Reports", icon: "ritual" },
  { id: "settings", label: "Settings", icon: "sliders" },
];

const workSessions: WorkSession[] = [];
const initialActions: ActionItem[] = [];
const streams: Stream[] = [];

const commandSuggestions = [
  "/today",
  "/activity",
  "/ai-usage",
  "/report",
  "/export",
];

const commandLabels: Record<string, string> = {
  "/today": "Open Today",
  "/activity": "Open Activity",
  "/report": "Generate daily report",
  "/what-did-i-do": "What did I work on today?",
  "/ai-usage": "Open AI Impact",
  "/export": "Export data",
  "/eod": "Generate daily report",
};

const initialFolders: WorkspaceFolder[] = [];
const defaultNotes: Note[] = [];

const providerDefaults: Record<AiConfig["provider"], { model: string; endpoint: string }> = {
  "Ollama Local": {
    model: "llama3.1",
    endpoint: "http://127.0.0.1:11434/v1/chat/completions",
  },
  "LM Studio": {
    model: "local-model",
    endpoint: "http://127.0.0.1:1234/v1/chat/completions",
  },
  "OpenAI Compatible": {
    model: "gpt-4.1-mini",
    endpoint: "http://127.0.0.1:1234/v1/chat/completions",
  },
  OpenAI: {
    model: "gpt-4.1-mini",
    endpoint: "https://api.openai.com/v1/chat/completions",
  },
  OpenRouter: {
    model: "google/gemini-2.0-flash-001",
    endpoint: "https://openrouter.ai/api/v1/chat/completions",
  },
  Groq: {
    model: "llama-3.1-8b-instant",
    endpoint: "https://api.groq.com/openai/v1/chat/completions",
  },
  Gemini: {
    model: "gemini-flash-latest",
    endpoint: "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent",
  },
  Anthropic: {
    model: "claude-sonnet-4-20250514",
    endpoint: "https://api.anthropic.com/v1/messages",
  },
  "NVIDIA NIM": {
    model: "meta/llama-3.1-8b-instruct",
    endpoint: "https://integrate.api.nvidia.com/v1/chat/completions",
  },
  "Custom API": {
    model: "custom-model",
    endpoint: "http://127.0.0.1:1234/v1/chat/completions",
  },
};

const defaultAiConfig: AiConfig = {
  provider: "Ollama Local",
  ...providerDefaults["Ollama Local"],
  apiKey: "",
  redactSecrets: true,
  fullClipboard: false,
};

function defaultEndpointForProvider(provider: AiConfig["provider"]) {
  return providerDefaults[provider].endpoint;
}

function defaultModelForProvider(provider: AiConfig["provider"]) {
  return providerDefaults[provider].model;
}

function endpointForProviderModel(provider: AiConfig["provider"], model: string) {
  if (provider !== "Gemini") {
    return defaultEndpointForProvider(provider);
  }
  const safeModel = model.trim() || defaultModelForProvider(provider);
  return `https://generativelanguage.googleapis.com/v1beta/models/${safeModel}:generateContent`;
}

function isProviderModelCompatible(provider: AiConfig["provider"], model: string) {
  const normalized = model.trim().toLowerCase();
  if (!normalized) {
    return false;
  }
  if (provider === "Gemini") {
    return normalized.startsWith("gemini");
  }
  if (provider === "Anthropic") {
    return normalized.startsWith("claude");
  }
  return true;
}

function isProviderEndpointCompatible(provider: AiConfig["provider"], endpoint: string) {
  const normalized = endpoint.trim().toLowerCase();
  if (!normalized) {
    return false;
  }
  if (provider === "Gemini") {
    return normalized.includes("generativelanguage.googleapis.com");
  }
  if (provider === "OpenRouter") {
    return normalized.includes("openrouter.ai");
  }
  if (provider === "Groq") {
    return normalized.includes("groq.com");
  }
  if (provider === "NVIDIA NIM") {
    return normalized.includes("integrate.api.nvidia.com");
  }
  if (provider === "Anthropic") {
    return normalized.includes("anthropic.com");
  }
  if (provider === "OpenAI") {
    return normalized.includes("api.openai.com");
  }
  return true;
}

const emptyStream: Stream = {
  id: "empty",
  title: "No active context",
  summary: "No captured work signals yet. Start the desktop app, browser bridge, or editor bridge to populate this view.",
  status: "Waiting",
  sessions: "0 events",
  eventIds: [],
  events: [],
};

function buildLocalReportMarkdown(_reportType: RitualKey, snapshot: BackendTodaySnapshot | null, settings?: ExperienceSettingsLike) {
  const title = "Daily Work Report";

  if (!snapshot) {
    return `# ${title}

No captured local data is available yet.

Start capture from the desktop watcher, browser bridge, editor bridge, or terminal bridge, then regenerate this report.`;
  }

  return buildDeterministicReportMarkdown(snapshot, settings);
}

async function invokeTauri<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T | null> {
  const invoke = getTauriInvoke();

  if (!invoke) {
    return null;
  }

  try {
    return await invoke<T>(command, args);
  } catch (error) {
    console.warn(`Tauri command failed: ${command}`, error);
    return null;
  }
}

async function writeClipboardText(value: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(value);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  document.body.removeChild(textarea);
}

function downloadTextFile(filename: string, contents: string, mimeType = "text/plain") {
  const blob = new Blob([contents], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  URL.revokeObjectURL(url);
}

function formatTimeRange(startedAt: number, endedAt: number) {
  const start = new Date(startedAt);
  const end = new Date(endedAt);

  if (Number.isNaN(start.getTime()) || Number.isNaN(end.getTime())) {
    return "Captured";
  }

  return `${start.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} - ${end.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
}

function formatDuration(durationMs = 0) {
  const totalSeconds = Math.max(0, Math.round(durationMs / 1_000));

  if (totalSeconds < 60) {
    return totalSeconds === 0 ? "0s" : `${totalSeconds}s`;
  }

  const totalMinutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;

  if (hours === 0) {
    return seconds === 0 ? `${totalMinutes}m` : `${totalMinutes}m ${seconds}s`;
  }

  if (minutes === 0 && seconds === 0) return `${hours}h`;
  if (seconds === 0) return `${hours}h ${minutes}m`;
  return `${hours}h ${minutes}m`;
}

function compactDisplayLabel(value?: string | null) {
  const cleaned = value?.replace(/[\u200e\u200f\u202a\u202c]/g, "").trim();

  if (!cleaned) {
    return "Captured activity";
  }

  const normalized = cleaned.toLowerCase();
  if (normalized === "code" || normalized === "visual studio code") {
    return "VS Code";
  }
  if (normalized === "/bin/zsh" || normalized === "zsh" || normalized === "iterm2") {
    return "Terminal";
  }
  if (cleaned.startsWith("/") || cleaned.startsWith("~/") || cleaned.includes("\\")) {
    const parts = cleaned.replace(/\\/g, "/").split("/").filter(Boolean);
    return parts.at(-1) ?? cleaned;
  }

  return cleaned;
}

function eventAppLabel(event: BackendSourceEvent) {
  return compactDisplayLabel(event.app ?? event.source);
}

function eventContextLabel(event: BackendSourceEvent) {
  return compactDisplayLabel(event.workspaceKey ?? event.domain ?? event.title ?? event.app);
}

function metadataStringAt(value: unknown, path: string[]) {
  let cursor = value;
  for (const segment of path) {
    if (!cursor || typeof cursor !== "object" || !(segment in cursor)) {
      return null;
    }
    cursor = (cursor as Record<string, unknown>)[segment];
  }
  return typeof cursor === "string" && cursor.trim() ? cursor : null;
}

function eventDocumentLabel(event: BackendSourceEvent) {
  if (!event.metadataJson) {
    return null;
  }

  try {
    const metadata = JSON.parse(event.metadataJson) as unknown;
    const rawDocument =
      metadataStringAt(metadata, ["document", "fileName"]) ??
      metadataStringAt(metadata, ["document", "filePath"]) ??
      metadataStringAt(metadata, ["document", "uri"]) ??
      metadataStringAt(metadata, ["activeFile"]);
    const label = compactDisplayLabel(rawDocument);
    return label === "Captured activity" ? null : label;
  } catch {
    return null;
  }
}

function eventTitle(event: BackendSourceEvent) {
  const app = eventAppLabel(event);
  const documentLabel = eventDocumentLabel(event);
  const title = documentLabel ?? compactDisplayLabel(event.title);
  const normalizedTitle = title.toLowerCase();
  const normalizedApp = app.toLowerCase();

  if (
    title &&
    normalizedTitle !== "captured activity" &&
    normalizedTitle !== normalizedApp &&
    !(normalizedApp === "vs code" && normalizedTitle === "code")
  ) {
    return title;
  }

  return compactDisplayLabel(event.domain ?? event.workspaceKey ?? event.app ?? event.source);
}

function isGenericEventDetail(value: string, event: BackendSourceEvent) {
  const normalizedValue = compactDisplayLabel(value).toLowerCase();
  const normalizedApp = eventAppLabel(event).toLowerCase();

  return (
    normalizedValue === "captured activity" ||
    normalizedValue === normalizedApp ||
    (normalizedApp === "vs code" && normalizedValue === "code")
  );
}

function eventSubtitle(event: BackendSourceEvent) {
  return [
    event.domain,
    event.urlRedacted,
    event.workspaceKey && !event.urlRedacted ? event.workspaceKey : null,
  ]
    .filter(
      (value): value is string =>
        typeof value === "string" && value.length > 0 && !isGenericEventDetail(value, event),
    )
    .join(" - ");
}

function eventFullContextLabel(event: BackendSourceEvent) {
  return (
    event.workspaceKey ||
    event.urlRedacted ||
    event.domain ||
    event.title ||
    event.app ||
    event.source ||
    "Captured activity"
  );
}

function pushUniqueTool(tools: string[], tool: string) {
  if (!tools.includes(tool)) {
    tools.push(tool);
  }
}

function aiToolLabelsForEvent(event: BackendSourceEvent) {
  const haystack = [
    event.domain,
    event.title,
    event.app,
    event.urlRedacted,
    event.workspaceKey,
  ]
    .filter(Boolean)
    .join(" ")
    .toLowerCase();
  const tools: string[] = [];

  if (event.metadataJson) {
    try {
      const metadata = JSON.parse(event.metadataJson) as unknown;
      const collect = (value: unknown) => {
        if (Array.isArray(value)) {
          value.forEach(collect);
          return;
        }
        if (value && typeof value === "object") {
          Object.entries(value as Record<string, unknown>).forEach(([key, nested]) => {
            if (["aiTools", "ai_tools", "tool", "toolName"].includes(key)) {
              collect(nested);
            } else if (typeof nested === "object") {
              collect(nested);
            }
          });
          return;
        }
        if (typeof value === "string") {
          const lower = value.toLowerCase();
          if (lower.includes("claude code")) pushUniqueTool(tools, "Claude Code");
          if (lower.includes("chatgpt")) pushUniqueTool(tools, "ChatGPT");
          if (lower.includes("claude") && !lower.includes("claude code")) pushUniqueTool(tools, "Claude");
          if (lower.includes("gemini")) pushUniqueTool(tools, "Gemini");
          if (lower.includes("copilot")) pushUniqueTool(tools, "Copilot");
          if (lower.includes("cursor")) pushUniqueTool(tools, "Cursor");
          if (lower.includes("codex")) pushUniqueTool(tools, "Codex");
          if (lower.includes("aider")) pushUniqueTool(tools, "Aider");
          if (lower.includes("cline")) pushUniqueTool(tools, "Cline");
          if (lower.includes("continue")) pushUniqueTool(tools, "Continue");
        }
      };
      collect(metadata);
    } catch {
      // Older rows can contain plain text metadata. Fall back to title/domain matching below.
    }
  }

  const matches: Array<[string, string]> = [
    ["claude code", "Claude Code"],
    ["chatgpt", "ChatGPT"],
    ["claude", "Claude"],
    ["gemini", "Gemini"],
    ["copilot", "Copilot"],
    ["cursor", "Cursor"],
    ["codex", "Codex"],
    ["aider", "Aider"],
    ["cline", "Cline"],
    ["continue", "Continue"],
  ];

  matches.forEach(([needle, label]) => {
    if (haystack.includes(needle)) {
      pushUniqueTool(tools, label);
    }
  });

  return tools;
}

function aiToolLabelForEvent(event: BackendSourceEvent) {
  return aiToolLabelsForEvent(event)[0] ?? null;
}

function uniqueValues(values: Array<string | null | undefined>) {
  return [...new Set(values.filter((value): value is string => Boolean(value)))];
}

// ── Domain → work category ────────────────────────────────────────────────────

type DomainCategory =
  | "ai-tools" | "code-review" | "project-mgmt" | "email" | "communication"
  | "documentation" | "design" | "research" | "meetings" | "web";

const DOMAIN_PATTERNS: Array<[RegExp, DomainCategory]> = [
  [/chatgpt\.com|claude\.ai|gemini\.google|aistudio\.google|perplexity\.ai|copilot\.microsoft|you\.com|poe\.com/, "ai-tools"],
  [/github\.com|gitlab|bitbucket|sourcehut|review\./, "code-review"],
  [/jira|linear\.app|trello|asana|monday\.com|notion\.so|clickup|basecamp|shortcut\.com/, "project-mgmt"],
  [/mail\.google|gmail|outlook|yahoo.*mail|protonmail|fastmail|mail\.|webmail/, "email"],
  [/slack\.com|discord|teams\.microsoft|zoom\.us|meet\.google|webex|whereby\.com|loom\.com/, "communication"],
  [/notion\.so|confluence|docs\.google|dropbox.*paper|quip|gitbook|readme\.io|docusaurus/, "documentation"],
  [/figma\.com|miro\.com|sketch\.cloud|zeplin|invision|canva\.com|framer\.com/, "design"],
  [/stackoverflow|mdn|developer\.|docs\.|w3schools|medium\.com|youtube\.com|dev\.to|hashnode/, "research"],
  [/calendar\.google|cal\.com|calendly|outlook.*calendar|fantastical/, "meetings"],
];

function classifyDomain(domain: string | null | undefined): DomainCategory | null {
  if (!domain) return null;
  const d = domain.toLowerCase();
  for (const [pattern, category] of DOMAIN_PATTERNS) {
    if (pattern.test(d)) return category;
  }
  return null;
}

const DOMAIN_CATEGORY_LABELS: Record<DomainCategory, string> = {
  "ai-tools": "AI",
  "code-review": "Code review",
  "project-mgmt": "Project mgmt",
  "email": "Email",
  "communication": "Comms",
  "documentation": "Docs",
  "design": "Design",
  "research": "Research",
  "meetings": "Meetings",
  "web": "Web",
};

const DOMAIN_CATEGORY_COLORS: Record<DomainCategory, string> = {
  "ai-tools": "#a855f7",
  "code-review": "#3b82f6",
  "project-mgmt": "#f59e0b",
  "email": "#10b981",
  "communication": "#06b6d4",
  "documentation": "#6366f1",
  "design": "#ec4899",
  "research": "#f97316",
  "meetings": "#14b8a6",
  "web": "#6b7280",
};

// ── App icon with real macOS bundle icon, falls back to colored letter ──────

const appIconCache = new Map<string, string | null>();

function AppIcon({ appName, className = "sidebar-app-icon" }: { appName: string; className?: string }) {
  const [iconSrc, setIconSrc] = useState<string | null>(() => {
    const cached = appIconCache.get(appName);
    return cached !== undefined ? cached : null;
  });
  const [loading, setLoading] = useState(() => !appIconCache.has(appName));
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (appIconCache.has(appName)) return;
    let cancelled = false;
    invokeTauri<string | null>("get_app_icon", { appName }).then((src) => {
      if (!cancelled) {
        const validatedSrc = src && src.startsWith("data:image/") ? src : null;
        appIconCache.set(appName, validatedSrc);
        setIconSrc(validatedSrc);
        setFailed(false);
        setLoading(false);
      }
    }).catch(() => {
      if (!cancelled) {
        appIconCache.set(appName, null);
        setLoading(false);
      }
    });
    return () => { cancelled = true; };
  }, [appName]);

  if (!loading && iconSrc && !failed) {
    return (
      <img
        alt={appName}
        className={`${className} app-icon-img`}
        onError={() => {
          appIconCache.set(appName, null);
          setFailed(true);
        }}
        src={iconSrc}
      />
    );
  }
  return (
    <span className={className} style={{ background: appColor(appName) }}>
      {appName.slice(0, 1).toUpperCase()}
    </span>
  );
}

function appColor(appName: string) {
  const palette = [
    "#0a84ff",
    "#30d158",
    "#ff9f0a",
    "#bf5af2",
    "#ff375f",
    "#64d2ff",
    "#ffd60a",
    "#5e5ce6",
    "#ff6b35",
    "#00c7be",
  ];
  let hash = 0;
  for (let index = 0; index < appName.length; index += 1) {
    hash = (hash * 31 + appName.charCodeAt(index)) % palette.length;
  }
  return palette[Math.abs(hash) % palette.length];
}

function formatDateTime(value?: number | null) {
  if (!value) {
    return "Not seen";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "Not seen";
  }
  return date.toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function sourceEventsForIds(sourceEvents: BackendSourceEvent[], ids: string[]) {
  const idSet = new Set(ids);
  return sourceEvents.filter((event) => idSet.has(event.id));
}

function sourceEventsForApp(
  sourceEvents: BackendSourceEvent[],
  appName?: string | null,
  projectLabel?: string | null,
  projectContexts: string[] = [],
) {
  if (!appName) {
    return [];
  }

  return sourceEvents.filter((event) => {
    const sameApp = eventAppLabel(event) === appName;
    const sameProject =
      !projectLabel ||
      eventContextLabel(event) === projectLabel ||
      projectContexts.includes(eventFullContextLabel(event));
    return sameApp && sameProject;
  });
}

function localHourLabel(hour: number) {
  const date = new Date();
  date.setHours(hour, 0, 0, 0);
  return date.toLocaleTimeString([], { hour: "numeric" });
}

// ── File-breakdown helpers (mirrors Rust parse_editor_file_from_title) ────────

const EDITOR_APPS_SET = new Set([
  "Visual Studio Code", "VS Code Insiders", "Cursor", "Sublime Text",
  "Sublime Text 2", "Sublime Text 3", "Sublime Text 4", "Xcode",
  "IntelliJ IDEA", "WebStorm", "PyCharm", "GoLand", "Rider", "CLion",
  "RubyMine", "DataGrip", "Nova", "BBEdit", "MacVim", "Neovim", "Zed",
]);

const BROWSER_APPS_SET = new Set([
  "Safari", "Firefox", "Firefox Developer Edition",
  "Google Chrome", "Google Chrome Canary", "Microsoft Edge",
  "Brave Browser", "Arc", "Chromium", "Opera", "Vivaldi",
  "ChatGPT Atlas", "Tor Browser", "Waterfox",
]);

const GENERIC_TAB_TITLES = new Set([
  "new tab", "new private tab", "new incognito window",
  "about:blank", "about:newtab", "new window", "start page", "blank page",
]);

function looksLikeFilename(s: string): boolean {
  return s.length > 0 && s.length <= 200 && s.includes(".") && !s.startsWith("/");
}

function basenameFromPath(path: string): string {
  return path.split(/[/\\]/).filter(Boolean).pop() ?? path;
}

function parseEditorFile(title: string, appName: string): HourFileEntry | null {
  if (!EDITOR_APPS_SET.has(appName)) return null;
  const t = title.replace(/^[●•]\s*/, "").trim();

  // VS Code / Cursor / Zed: "file — project — App"
  const emParts = t.split(" — ");
  if (emParts.length >= 2) {
    const name = emParts[0].trim();
    if (looksLikeFilename(name)) {
      const context =
        emParts.length >= 3 && !EDITOR_APPS_SET.has(emParts[1].trim())
          ? basenameFromPath(emParts[1].trim())
          : null;
      return { name, context, durationMs: 0 };
    }
  }

  // Sublime Text: "file (project) - App"
  const parenMatch = /^(.+?)\s+\(([^)]*)\)/.exec(t);
  if (parenMatch && looksLikeFilename(parenMatch[1].trim())) {
    return {
      name: parenMatch[1].trim(),
      context: parenMatch[2] ? basenameFromPath(parenMatch[2]) : null,
      durationMs: 0,
    };
  }

  // JetBrains: "file [/path] – IDE"
  const enParts = t.split(" – ");
  if (enParts.length >= 2 && looksLikeFilename(enParts[0].trim())) {
    const bracketMatch = /^\[([^\]]+)\]/.exec(enParts[1] ?? "");
    return {
      name: enParts[0].trim(),
      context: bracketMatch ? basenameFromPath(bracketMatch[1]) : null,
      durationMs: 0,
    };
  }

  return null;
}

function parseBrowserTab(event: BackendSourceEvent): HourFileEntry | null {
  const app = eventAppLabel(event);
  if (!BROWSER_APPS_SET.has(app)) return null;
  const title = event.title?.trim();
  if (!title || GENERIC_TAB_TITLES.has(title.toLowerCase())) return null;
  return { name: title, context: event.domain ?? null, durationMs: 0 };
}

// ── End file-breakdown helpers ─────────────────────────────────────────────

function buildHourBuckets(sourceEvents: BackendSourceEvent[]) {
  const hourMs = 60 * 60 * 1000;
  const dayStartDate = new Date();
  dayStartDate.setHours(0, 0, 0, 0);
  const dayStart = dayStartDate.getTime();
  const dayEnd = dayStart + 24 * hourMs;
  const working = Array.from({ length: 24 }, (_, hour) => ({
    hour,
    label: localHourLabel(hour),
    durationMs: 0,
    events: [] as BackendSourceEvent[],
    apps: new Map<
      string,
      {
        app: string;
        durationMs: number;
        events: Set<string>;
        contexts: Set<string>;
        aiTools: Set<string>;
        examples: Set<string>;
        files: Map<string, HourFileEntry>;
      }
    >(),
    aiTools: new Set<string>(),
  }));

  sourceEvents.forEach((event) => {
    const rawStart = Number.isFinite(event.startedAt) ? event.startedAt : event.createdAt;
    const rawEnd = Number.isFinite(event.endedAt) ? event.endedAt : rawStart + event.durationMs;
    const start = Math.max(rawStart, dayStart);
    const end = Math.min(Math.max(rawEnd, rawStart + 1), dayEnd);

    if (end <= dayStart || start >= dayEnd || end <= start) {
      return;
    }

    const firstHour = Math.max(0, Math.min(23, Math.floor((start - dayStart) / hourMs)));
    const lastHour = Math.max(0, Math.min(23, Math.floor((end - 1 - dayStart) / hourMs)));

    for (let hour = firstHour; hour <= lastHour; hour += 1) {
      const bucket = working[hour];
      const hourStart = dayStart + hour * hourMs;
      const overlap = Math.max(0, Math.min(end, hourStart + hourMs) - Math.max(start, hourStart));

      if (overlap <= 0) {
        continue;
      }

      const app = eventAppLabel(event);
      const context = eventFullContextLabel(event);
      const title = eventTitle(event);
      const document = eventDocumentLabel(event);
      const aiTools = aiToolLabelsForEvent(event);
      const appRow = bucket.apps.get(app) ?? {
        app,
        durationMs: 0,
        events: new Set<string>(),
        contexts: new Set<string>(),
        aiTools: new Set<string>(),
        examples: new Set<string>(),
        files: new Map<string, HourFileEntry>(),
      };

      appRow.durationMs += overlap;
      appRow.events.add(event.id);
      appRow.contexts.add(context);
      appRow.examples.add(title);
      if (document) {
        appRow.examples.add(document);
      }
      aiTools.forEach((tool) => {
        appRow.aiTools.add(tool);
        bucket.aiTools.add(tool);
      });

      // Per-file duration tracking for editors and browsers
      const rawTitle = event.title?.trim() ?? "";
      const parsedFile = parseEditorFile(rawTitle, app) ?? parseBrowserTab(event);
      if (parsedFile) {
        const fileKey = `${parsedFile.name}|${parsedFile.context ?? ""}`;
        const existing = appRow.files.get(fileKey);
        if (existing) {
          appRow.files.set(fileKey, { ...existing, durationMs: existing.durationMs + overlap });
        } else {
          appRow.files.set(fileKey, { ...parsedFile, durationMs: overlap });
        }
      }

      bucket.apps.set(app, appRow);
      bucket.durationMs += overlap;
      if (!bucket.events.some((existing) => existing.id === event.id)) {
        bucket.events.push(event);
      }
    }
  });

  const IDLE_THRESHOLD_MS = 5 * 60 * 1000; // 5-minute gap = "away"

  return working.map((bucket): HourBucket => {
    const sortedEvents = bucket.events.sort((left, right) => left.startedAt - right.startedAt);

    // Detect idle gaps between consecutive events
    const idleGaps: IdleGap[] = [];
    for (let i = 1; i < sortedEvents.length; i++) {
      const prev = sortedEvents[i - 1];
      const curr = sortedEvents[i];
      const prevEnd = Math.max(prev.endedAt, prev.startedAt + prev.durationMs);
      const gap = curr.startedAt - prevEnd;
      if (gap >= IDLE_THRESHOLD_MS) {
        idleGaps.push({ startMs: prevEnd, endMs: curr.startedAt, durationMs: gap });
      }
    }

    return {
      hour: bucket.hour,
      label: bucket.label,
      durationMs: bucket.durationMs,
      events: sortedEvents,
      apps: [...bucket.apps.values()]
        .map((app) => ({
          app: app.app,
          durationMs: app.durationMs,
          events: app.events.size,
          contexts: [...app.contexts],
          aiTools: [...app.aiTools],
          examples: [...app.examples],
          files: [...app.files.values()]
            .sort((a, b) => b.durationMs - a.durationMs)
            .slice(0, 5),
        }))
        .sort((left, right) => right.durationMs - left.durationMs),
      aiTools: [...bucket.aiTools],
      idleGaps,
    };
  });
}

function buildProjectUsageBreakdown(sourceEvents: BackendSourceEvent[]): ProjectUsageBreakdown[] {
  const buckets = new Map<
    string,
    {
      key: string;
      label: string;
      durationMs: number;
      eventIds: Set<string>;
      apps: Map<string, number>;
      aiTools: Set<string>;
      contexts: Set<string>;
    }
  >();

  sourceEvents.forEach((event) => {
    const rawKey = event.workspaceKey || event.domain || event.urlRedacted || event.title || event.app || event.source;
    const key = rawKey?.trim() || "Captured activity";
    const label = compactDisplayLabel(event.workspaceKey ?? event.domain ?? event.title ?? event.app ?? event.source);
    const bucket = buckets.get(key) ?? {
      key,
      label,
      durationMs: 0,
      eventIds: new Set<string>(),
      apps: new Map<string, number>(),
      aiTools: new Set<string>(),
      contexts: new Set<string>(),
    };
    const app = eventAppLabel(event);
    bucket.durationMs += event.durationMs;
    bucket.eventIds.add(event.id);
    bucket.apps.set(app, (bucket.apps.get(app) ?? 0) + event.durationMs);
    bucket.contexts.add(eventFullContextLabel(event));
    aiToolLabelsForEvent(event).forEach((tool) => bucket.aiTools.add(tool));
    buckets.set(key, bucket);
  });

  return [...buckets.values()]
    .map((bucket) => ({
      key: bucket.key,
      label: bucket.label,
      durationMs: bucket.durationMs,
      events: bucket.eventIds.size,
      apps: [...bucket.apps.entries()]
        .map(([app, durationMs]) => ({ app, durationMs }))
        .sort((left, right) => right.durationMs - left.durationMs),
      aiTools: [...bucket.aiTools],
      contexts: [...bucket.contexts],
    }))
    .filter((project) => project.label !== "Captured activity")
    .sort((left, right) => right.durationMs - left.durationMs);
}

function sessionProjectLine(session: BackendTodaySnapshot["workSessions"][number], snapshot: BackendTodaySnapshot) {
  const summary = session.summary?.trim();
  const fallbackPath = snapshot.projectContext?.path;
  if (!summary) {
    return fallbackPath ?? "Captured context";
  }
  if (fallbackPath && !summary.includes("/") && !summary.includes(fallbackPath)) {
    return `${summary} - ${fallbackPath}`;
  }
  return summary;
}

function mapSessions(snapshot: BackendTodaySnapshot | null): WorkSession[] {
  if (!snapshot?.workSessions.length) {
    return [];
  }

  return snapshot.workSessions.map((session) => ({
    id: session.id,
    time: formatTimeRange(session.startedAt, session.endedAt),
    title: compactDisplayLabel(session.title),
    project: sessionProjectLine(session, snapshot),
    status: session.status,
    tools: session.aiUsed ? "AI-assisted, local signals" : "Local signals",
    confidence: `${session.confidencePercent}%`,
    evidenceEventIds: session.evidenceEventIds ?? [],
  }));
}

function mapStreamEvents(
  eventIds: string[],
  sourceEvents: BackendSourceEvent[] | undefined,
): StreamEvent[] {
  if (!eventIds.length || !sourceEvents?.length) {
    return [];
  }

  const wanted = new Set(eventIds);
  return sourceEvents
    .filter((event) => wanted.has(event.id))
    .sort((left, right) => left.startedAt - right.startedAt)
    .slice(0, 12)
    .map((event) => ({
      id: event.id,
      title: `${eventAppLabel(event)} - ${eventTitle(event)}`,
      timeSpan: formatTimeRange(event.startedAt, event.endedAt),
      sourceType: event.source,
      width: Math.max(8, Math.min(100, Math.round(event.durationMs / 60000))),
    }));
}

function mapStreams(snapshot: BackendTodaySnapshot | null): Stream[] {
  if (!snapshot?.parallelStreams.length) {
    return [];
  }

  return snapshot.parallelStreams.map((stream) => ({
    id: stream.id,
    title: compactDisplayLabel(stream.title),
    summary: stream.summary ?? "Captured from local activity details.",
    status: stream.status,
    sessions: `${stream.eventIds.length} activity item${stream.eventIds.length === 1 ? "" : "s"}`,
    eventIds: stream.eventIds,
    events: mapStreamEvents(stream.eventIds, snapshot.sourceEvents),
  }));
}

function mapNotes(snapshot: BackendTodaySnapshot | null): Note[] {
  if (!snapshot?.quickNotes.length) {
    return [];
  }

  return snapshot.quickNotes.map((note) => ({
    id: note.id,
    text: note.body,
    time: "Saved",
    context: note.projectPath ?? note.source ?? "Quick note",
  }));
}

function mapActions(snapshot: BackendTodaySnapshot | null): ActionItem[] {
  if (!snapshot) {
    return [];
  }

  const actions: ActionItem[] = [];
  if (snapshot.nextBestAction) {
    actions.push({
      id: `nba-${snapshot.nextBestAction.sourceType}-${snapshot.nextBestAction.sourceId}`,
      title: snapshot.nextBestAction.title,
      source: snapshot.nextBestAction.reason,
      state: "open",
    });
  }
  actions.push(
    ...snapshot.pendingReplies.slice(0, 3).map((reply) => ({
      id: `reply-${reply.id}`,
      title: `Reply to ${reply.subject}`,
      source: reply.latestSender ?? "Unanswered message",
      state: "open" as const,
    })),
    ...snapshot.commitments.slice(0, 3).map((commitment) => ({
      id: `commitment-${commitment.id}`,
      title: commitment.title,
      source: commitment.source ?? "Commitment tracker",
      state: "open" as const,
    })),
    ...snapshot.aiOutputs
      .filter((output) => output.status === "drafted" || output.status === "needs_review")
      .slice(0, 2)
      .map((output) => ({
        id: `output-${output.id}`,
        title: output.title,
        source: `AI-assisted work - ${output.outputType}`,
        state: "open" as const,
      })),
    ...snapshot.idleBlocks
      .filter((block) => !block.classified)
      .slice(0, 2)
      .map((block) => ({
        id: `idle-${block.id}`,
        title: `Classify ${Math.max(1, Math.round(block.durationMs / 60_000))}m idle block`,
        source: "Smart idle recovery",
        state: "open" as const,
      })),
    ...(snapshot.loopRisks ?? [])
      .filter(
        (risk) =>
          !actions.some(
            (action) =>
              action.title === risk.title ||
              action.id === `loop-${risk.riskType}-${risk.id}`,
          ),
      )
      .slice(0, 4)
      .map((risk) => ({
        id: `loop-${risk.riskType}-${risk.id}`,
        title: risk.title,
        source: `${risk.riskType} - ${risk.reason}`,
        state: "open" as const,
      })),
  );

  const seenCategories = new Set<string>();

  return actions.filter((action) => {
    const text = `${action.title} ${action.source}`.toLowerCase();
    const category =
      text.includes("idle") || text.includes("away")
        ? "idle-away"
        : text.includes("reply")
          ? `reply-${action.title.toLowerCase()}`
          : text.includes("commitment") || text.includes("promise")
            ? `commitment-${action.title.toLowerCase()}`
            : action.title.toLowerCase();

    if (seenCategories.has(category)) {
      return false;
    }

    seenCategories.add(category);
    return true;
  });
}

function formatFactDate(createdAt?: string | number | null) {
  if (!createdAt) {
    return "Today";
  }

  const date = new Date(createdAt);

  if (Number.isNaN(date.getTime())) {
    return "Today";
  }

  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function mapSourceFeed(snapshot: BackendTodaySnapshot | null): SourceFeedItem[] {
  if (!snapshot) {
    return [];
  }

  const items: SourceFeedItem[] = [];

  snapshot.workSessions.slice(0, 3).forEach((session) => {
    items.push({
      id: `session-${session.id}`,
      label: `Session: ${session.title}`,
      selected: true,
    });
  });

  snapshot.pendingReplies.slice(0, 2).forEach((reply) => {
    items.push({
      id: `reply-${reply.id}`,
      label: `Reply risk: ${reply.subject}`,
      selected: true,
    });
  });

  snapshot.commitments.slice(0, 2).forEach((commitment) => {
    items.push({
      id: `commitment-${commitment.id}`,
      label: `Promise: ${commitment.title}`,
      selected: true,
    });
  });

  snapshot.aiOutputs.slice(0, 2).forEach((output) => {
    items.push({
      id: `ai-output-${output.id}`,
      label: `AI-assisted work: ${output.title}`,
      selected: output.status !== "discarded",
    });
  });

  snapshot.quickNotes.slice(0, 2).forEach((note) => {
    items.push({
      id: `note-${note.id}`,
      label: `Scratchpad: ${note.body}`,
      selected: true,
    });
  });

  return items.slice(0, 10);
}

function mapAiThreads(snapshot: BackendTodaySnapshot | null): AiThread[] {
  if (!snapshot) {
    return [];
  }

  const fromOutputs = snapshot.aiOutputs.slice(0, 6).map((output) => ({
    id: `output-${output.id}`,
    tool: output.outputType,
    title: output.title,
    clue: output.status,
  }));

  const fromSessions = snapshot.workSessions
    .filter((session) => session.aiUsed)
    .slice(0, 4)
    .map((session) => ({
      id: `session-${session.id}`,
      tool: "AI-assisted work",
      title: session.title,
      clue: session.summary ?? session.status,
    }));

  return [...fromOutputs, ...fromSessions].slice(0, 8);
}

function mapMemoryFacts(snapshot: BackendTodaySnapshot | null): MemoryFact[] {
  if (!snapshot) {
    return [];
  }

  const facts: MemoryFact[] = [];

  snapshot.quickNotes.slice(0, 4).forEach((note) => {
    facts.push({
      id: `note-${note.id}`,
      kind: "quickNote",
      rawId: note.id,
      date: formatFactDate(note.createdAt),
      title: note.body,
      source: note.projectPath ?? note.source ?? "Scratchpad",
    });
  });

  snapshot.commitments.slice(0, 4).forEach((commitment) => {
    facts.push({
      id: `commitment-${commitment.id}`,
      kind: "commitment",
      rawId: commitment.id,
      date: formatFactDate(commitment.dueAt),
      title: commitment.title,
      source: commitment.source ?? "Commitment tracker",
    });
  });

  snapshot.aiOutputs.slice(0, 3).forEach((output) => {
    facts.push({
      id: `ai-output-${output.id}`,
      kind: "aiOutput",
      rawId: output.id,
      date: "Today",
      title: output.title,
      source: `AI-assisted work - ${output.status}`,
    });
  });

  snapshot.meetings.slice(0, 3).forEach((meeting) => {
    facts.push({
      id: `meeting-${meeting.id}`,
      kind: "meeting",
      rawId: meeting.id,
      date: "Today",
      title: meeting.title,
      source: meeting.summary ?? "Meeting capture",
    });
  });

  snapshot.fieldVisits.slice(0, 3).forEach((visit) => {
    facts.push({
      id: `field-visit-${visit.id}`,
      kind: "fieldVisit",
      rawId: visit.id,
      date: "Today",
      title: visit.clientLabel ?? visit.locationLabel ?? "Field visit",
      source: visit.status,
    });
  });

  return facts.slice(0, 12);
}

function memoryFactKindLabel(kind: MemoryFact["kind"]) {
  switch (kind) {
    case "quickNote":
      return "Scratchpad note";
    case "commitment":
      return "Commitment";
    case "aiOutput":
      return "AI-assisted work";
    case "meeting":
      return "Meeting";
    case "fieldVisit":
      return "Field visit";
    default:
      return "Memory";
  }
}

function contextPackDisplayLabel(key: string) {
  const labels: Record<string, string> = {
    workspace: "Current workspace",
    capture: "Capture status",
    source: "Memory source",
    pending: "Open loops",
    folders: "Folders",
    ai: "AI model",
    notes: "Recent notes",
  };
  return labels[key] ?? key;
}

function mapAiConfig(settings?: BackendSettings): AiConfig {
  if (!settings) {
    return defaultAiConfig;
  }

  const provider = [
    "Ollama Local",
    "LM Studio",
    "OpenAI Compatible",
    "OpenAI",
    "OpenRouter",
    "Groq",
    "Gemini",
    "Anthropic",
    "NVIDIA NIM",
    "Custom API",
  ].includes(settings.aiProvider ?? "")
    ? (settings.aiProvider as AiConfig["provider"])
    : defaultAiConfig.provider;
  const configuredModel = settings.aiModel?.trim() || defaultModelForProvider(provider);
  const configuredEndpoint =
    settings.aiEndpoint?.trim() || defaultEndpointForProvider(provider);

  return {
    provider,
    model: isProviderModelCompatible(provider, configuredModel)
      ? configuredModel
      : defaultModelForProvider(provider),
    endpoint: isProviderEndpointCompatible(provider, configuredEndpoint)
      ? configuredEndpoint
      : defaultEndpointForProvider(provider),
    apiKey: "",
    redactSecrets: settings.aiRedactSecrets ?? defaultAiConfig.redactSecrets,
    fullClipboard: settings.fullClipboardHistory ?? defaultAiConfig.fullClipboard,
  };
}

// ── Work Context Editor ───────────────────────────────────────────────────────

interface WorkContextForm {
  client: string;
  project: string;
  task: string;
  ticketId: string;
  billable: boolean;
}

function WorkContextEditorModal({
  context,
  initialForm,
  onClose,
  onSave,
  onClear,
}: {
  context: BackendActiveWorkContext | null | undefined;
  initialForm?: Partial<WorkContextForm>;
  onClose: () => void;
  onSave: (form: WorkContextForm) => void;
  onClear: () => void;
}) {
  const [form, setForm] = useState<WorkContextForm>({
    client: initialForm?.client ?? context?.client ?? "",
    project: initialForm?.project ?? context?.project ?? "",
    task: initialForm?.task ?? context?.task ?? "",
    ticketId: initialForm?.ticketId ?? context?.ticketId ?? "",
    billable: initialForm?.billable ?? context?.billable ?? true,
  });

  const hasContext = Boolean(context?.client || context?.project || context?.task);

  function handleSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    onSave(form);
    onClose();
  }

  return (
        <div className="offline-modal-backdrop" onClick={onClose}>
          <form
            className="offline-modal work-context-modal"
            onClick={(e) => e.stopPropagation()}
            onSubmit={handleSubmit}
          >
            <h3>Set current task</h3>
            <p>Use this when you want future activity tagged to a client, project, or ticket.</p>
            <label className="offline-modal-full">
              Client / Organisation
              <input
                placeholder="e.g. GG IVF, Acme Corp"
                type="text"
                value={form.client}
                onChange={(e) => setForm((prev) => ({ ...prev, client: e.target.value }))}
              />
            </label>
            <label className="offline-modal-full">
              Project / Module
              <input
                placeholder="e.g. Medics v9, WhatsApp Follow-up"
                type="text"
                value={form.project}
                onChange={(e) => setForm((prev) => ({ ...prev, project: e.target.value }))}
              />
            </label>
            <label className="offline-modal-full">
              Task / Description
              <input
                placeholder="e.g. Bug 124238 - EMR Casesheet"
                type="text"
                value={form.task}
                onChange={(e) => setForm((prev) => ({ ...prev, task: e.target.value }))}
              />
            </label>
            <div className="offline-modal-row">
              <label className="offline-modal-full">
                Ticket / MR / Issue ID
                <input
                  placeholder="e.g. PROJ-123 or !4209"
                  type="text"
                  value={form.ticketId}
                  onChange={(e) => setForm((prev) => ({ ...prev, ticketId: e.target.value }))}
                />
              </label>
              <label className="offline-modal-full">
                Billable?
                <select
                  value={form.billable ? "yes" : "no"}
                  onChange={(e) => setForm((prev) => ({ ...prev, billable: e.target.value === "yes" }))}
                >
                  <option value="yes">Yes - billable</option>
                  <option value="no">No - internal / personal</option>
                </select>
              </label>
            </div>
            <div className="offline-modal-actions">
              <button className="button" type="submit">Save context</button>
              {hasContext && (
                <button
                  className="button ghost"
                  onClick={() => { onClear(); onClose(); }}
                  type="button"
                >
                  Clear
                </button>
              )}
              <button className="button ghost" onClick={onClose} type="button">Cancel</button>
            </div>
          </form>
        </div>
  );
}

export default function App() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const toastTimers = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  const addToast = useCallback((kind: ToastKind, title: string, message?: string) => {
    const id = nextToastId();
    setToasts((prev) => [...prev, { id, kind, title, message }]);
    const delay = kind === "error" || kind === "warning" ? 6000 : 3000;
    toastTimers.current.set(id, setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
      toastTimers.current.delete(id);
    }, delay));
  }, []);

  const dismissToast = useCallback((id: number) => {
    const timer = toastTimers.current.get(id);
    if (timer) clearTimeout(timer);
    toastTimers.current.delete(id);
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const [activeView, setActiveView] = useState<ViewKey>("today");
  const [activeStream, setActiveStream] = useState("backend");
  const [activeAppName, setActiveAppName] = useState<string | null>(null);
  const [activeHourDetail, setActiveHourDetail] = useState<number | null>(null);
  const [activeRitual, setActiveRitual] = useState<RitualKey>("daily");
  const [isPaused, setIsPaused] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);
  const [commandQuery, setCommandQuery] = useState("");
  const [quickNote, setQuickNote] = useState("");
  const [notes, setNotes] = useState<Note[]>(defaultNotes);
  const [actions, setActions] = useState<ActionItem[]>(initialActions);
  const [folders, setFolders] = useState<WorkspaceFolder[]>(initialFolders);
  const [aiConfig, setAiConfig] = useState<AiConfig>(defaultAiConfig);
  const [draftAiConfig, setDraftAiConfig] = useState<AiConfig>(defaultAiConfig);
  const [draftLaunchAtLogin, setDraftLaunchAtLogin] = useState(false);
  const [saveState, setSaveState] = useState("Local ready");
  const [todaySnapshot, setTodaySnapshot] = useState<BackendTodaySnapshot | null>(null);
  const [dismissedLoopIds, setDismissedLoopIds] = useState<Set<string>>(() => new Set());
  const [backendReady, setBackendReady] = useState(false);
  const [bridgeStatus, setBridgeStatus] = useState("Connecting to desktop bridge...");
  const [reportMarkdown, setReportMarkdown] = useState("");
  const [memoryResults, setMemoryResults] = useState<BackendSearchResult[]>([]);
  const [exportFromDate, setExportFromDate] = useState("");
  const [exportToDate, setExportToDate] = useState("");
  const [exportPreview, setExportPreview] = useState("");
  const [exportStatus, setExportStatus] = useState("Ready");
  const [storageInfo, setStorageInfo] = useState<BackendStorageLocationInfo | null>(null);
  const [storageStatus, setStorageStatus] = useState("Storage ready");
  const [terminalBridgeStatus, setTerminalBridgeStatus] = useState("Ready to install");
  const [settingsConfigJson, setSettingsConfigJson] = useState("");
  const [databaseRestorePath, setDatabaseRestorePath] = useState("");
  const [permissionSummary, setPermissionSummary] =
    useState<BackendCapturePermissionSummary | null>(null);
  const [permissionStatus, setPermissionStatus] = useState("Checking permissions...");
  const [permissionSetupDismissed, setPermissionSetupDismissed] = useState(false);
  const [logOfflineOpen, setLogOfflineOpen] = useState(false);
  const [offlineForm, setOfflineForm] = useState({ start: "", end: "", category: "Away from desk" });
  const [workContextEditor, setWorkContextEditor] = useState<Partial<WorkContextForm> | null>(null);

  // Review / timesheet state
  const [reviewFromDate, setReviewFromDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [reviewToDate, setReviewToDate] = useState(() => new Date().toISOString().slice(0, 10));
  const [reviewSessions, setReviewSessions] = useState<BackendWorkSessionSummary[]>([]);
  const [reviewLoading, setReviewLoading] = useState(false);
  const [exportingTimesheet, setExportingTimesheet] = useState(false);

  const displayStreams = useMemo(() => mapStreams(todaySnapshot), [todaySnapshot]);
  const displaySessions = useMemo(() => mapSessions(todaySnapshot), [todaySnapshot]);
  const experienceSettings = useMemo(
    () => normalizeExperienceSettings(todaySnapshot?.settings),
    [todaySnapshot?.settings],
  );
  const isSimpleMode = experienceSettings.experienceMode === "simple";
  const displayApps = useMemo(
    () => {
      const apps = todaySnapshot?.appUsageSummary?.apps ?? [];
      if (!isSimpleMode || experienceSettings.showSystemApps) {
        return apps;
      }
      return apps.filter((app) => isSimpleVisibleApp(app.app, app.category));
    },
    [experienceSettings.showSystemApps, isSimpleMode, todaySnapshot],
  );

  // Most recently captured app — always visible in sidebar even if outside top-8
  const liveAppName = useMemo(() => {
    const events = todaySnapshot?.sourceEvents ?? [];
    if (!events.length) return null;
    const latest = events.reduce((best, ev) => ev.endedAt > best.endedAt ? ev : best);
    return eventAppLabel(latest);
  }, [todaySnapshot]);

  const sidebarApps = useMemo(() => {
    const top8 = displayApps.slice(0, 8);
    if (!liveAppName || top8.some((a) => a.app === liveAppName)) return top8;
    const liveEntry = displayApps.find((a) => a.app === liveAppName);
    if (!liveEntry) return top8;
    // Pin active app at top, keep total at 8
    return [liveEntry, ...top8.filter((a) => a.app !== liveAppName).slice(0, 7)];
  }, [displayApps, liveAppName]);
  const displaySourceFeed = useMemo(() => mapSourceFeed(todaySnapshot), [todaySnapshot]);
  const displayAiThreads = useMemo(() => mapAiThreads(todaySnapshot), [todaySnapshot]);
  const displayMemoryFacts = useMemo(() => mapMemoryFacts(todaySnapshot), [todaySnapshot]);
  const displayLoopItems = useMemo(
    () =>
      (todaySnapshot?.unclosedLoopInbox ?? []).filter(
        (item) => !dismissedLoopIds.has(item.id),
      ),
    [dismissedLoopIds, todaySnapshot],
  );

  const currentView = navigation.find((item) => item.id === activeView);
  const currentViewLabel =
    currentView?.label ??
    (activeView === "automation"
      ? "Export Data"
      : activeView === "memory"
        ? "Saved Notes"
        : activeView === "hour"
          ? "Hour Breakdown"
          : activeView === "restore"
            ? "Work Context"
            : "DayTrail");
  const selectedStream =
    displayStreams.find((stream) => stream.id === activeStream) ??
    displayStreams[0] ??
    emptyStream;
  const latestStream = displayStreams[0] ?? selectedStream;
  const openActions = actions.filter((action) => action.state === "open");
  const selectedFolders = folders.filter((folder) => folder.selected);
  const pendingReplyCount = todaySnapshot?.pendingReplies.length ?? 0;

  async function applyTodaySnapshot(snapshot: BackendTodaySnapshot) {
    const mappedStreams = mapStreams(snapshot);
    const mappedApps = snapshot.appUsageSummary?.apps ?? [];
    setTodaySnapshot(snapshot);
    setBackendReady(true);
    setBridgeStatus("Local fact store connected");
    setNotes(mapNotes(snapshot));
    setActions(mapActions(snapshot));
    setIsPaused(snapshot.pauseState.paused);
    const loadedAiConfig = mapAiConfig(snapshot.settings);
    setAiConfig(loadedAiConfig);
    setDraftAiConfig(loadedAiConfig);
    setDraftLaunchAtLogin(snapshot.settings.launchAtLogin ?? false);
    setSaveState(`${loadedAiConfig.provider} ready`);
    setActiveStream((currentStream) =>
      mappedStreams.some((stream) => stream.id === currentStream)
        ? currentStream
        : (mappedStreams[0]?.id ?? currentStream),
    );
    setActiveAppName((currentApp) =>
      currentApp && mappedApps.some((app) => app.app === currentApp)
        ? currentApp
        : (mappedApps[0]?.app ?? currentApp),
    );
  }

  async function refreshTodaySnapshot() {
    const invoke = getTauriInvoke();

    if (!invoke) {
      setBridgeStatus("Desktop bridge unavailable");
      setBackendReady(false);
      return null;
    }

    let snapshot: BackendTodaySnapshot | null = null;
    try {
      snapshot = await invoke<BackendTodaySnapshot>("today", undefined);
    } catch (error) {
      setBridgeStatus(`Desktop bridge error: ${errorMessage(error)}`);
      setBackendReady(false);
      return null;
    }

    if (!snapshot) {
      setBackendReady(false);
      return null;
    }

    await applyTodaySnapshot(snapshot);
    return snapshot;
  }

  async function loadStorageLocations() {
    const info = await invokeTauri<BackendStorageLocationInfo>("get_storage_locations");

    if (!info) {
      setStorageStatus("Storage locations unavailable");
      return null;
    }

    setStorageInfo(info);
    return info;
  }

  async function loadCapturePermissions() {
    setPermissionStatus("Checking permissions...");
    const summary = await invokeTauri<BackendCapturePermissionSummary>(
      "get_capture_permissions",
    );

    if (!summary) {
      setPermissionStatus("Permission checks unavailable");
      return null;
    }

    setPermissionSummary(summary);
    setPermissionStatus(permissionStatusMessage(summary));
    if (summary.allRequiredGranted) {
      setPermissionSetupDismissed(false);
    }
    return summary;
  }

  async function openCapturePermissionSettings(permissionId: string) {
    setPermissionStatus("Requesting permission...");
    const summary = await invokeTauri<BackendCapturePermissionSummary>(
      "request_capture_permission",
      { permissionId },
    );

    if (!summary) {
      setPermissionStatus("Permission request unavailable");
      return;
    }

    setPermissionSummary(summary);
    setPermissionStatus(permissionStatusMessage(summary));
  }

  async function resetAndRequestAccessibility() {
    setPermissionStatus("Resetting accessibility grant...");
    const summary = await invokeTauri<BackendCapturePermissionSummary>(
      "reset_and_request_accessibility",
    );

    if (!summary) {
      setPermissionStatus("Reset unavailable");
      return;
    }

    setPermissionSummary(summary);
    setPermissionStatus(permissionStatusMessage(summary));
  }

  async function triggerBrowserAutomation() {
    setPermissionStatus("Requesting browser automation access...");
    const summary = await invokeTauri<BackendCapturePermissionSummary>(
      "trigger_browser_automation_prompt",
    );
    if (summary) {
      setPermissionSummary(summary);
      setPermissionStatus(permissionStatusMessage(summary));
    }
  }

  async function restartDayTrail() {
    setPermissionStatus("Restarting DayTrail...");
    const restarted = await invokeTauri<boolean>("restart_app");

    if (!restarted) {
      setPermissionStatus("Restart unavailable. Quit and reopen DayTrail.");
    }
  }

  useEffect(() => {
    let ignore = false;

    async function loadToday() {
      try {
        const invoke = getTauriInvoke();

        if (!invoke) {
          setBridgeStatus("Desktop bridge unavailable");
          setBackendReady(false);
          return;
        }

        const snapshot = await invoke<BackendTodaySnapshot>("today", undefined);

        if (!snapshot || ignore) {
          return;
        }

        await applyTodaySnapshot(snapshot);
      } catch (error) {
        if (!ignore) {
          setBridgeStatus(`Desktop bridge error: ${errorMessage(error)}`);
          setBackendReady(false);
        }
      }
    }

    loadToday();
    const refreshId = window.setInterval(loadToday, 2500);

    return () => {
      ignore = true;
      window.clearInterval(refreshId);
    };
  }, []);

  useEffect(() => {
    if (!hasTauriRuntime()) {
      return;
    }

    let ignore = false;

    async function loadPermissions() {
      const summary = await invokeTauri<BackendCapturePermissionSummary>(
        "get_capture_permissions",
      );

      if (!summary || ignore) {
        return;
      }

      setPermissionSummary(summary);
      setPermissionStatus(permissionStatusMessage(summary));
      if (summary.allRequiredGranted) {
        setPermissionSetupDismissed(false);
      }
    }

    loadPermissions();
    const refreshId = window.setInterval(loadPermissions, 5000);

    return () => {
      ignore = true;
      window.clearInterval(refreshId);
    };
  }, []);

  useEffect(() => {
    if (activeView === "today" && latestStream.id !== "empty") {
      setActiveStream(latestStream.id);
    }
  }, [activeView, latestStream.id]);

  // Tray-action events emitted by the Rust tray handler
  useEffect(() => {
    if (!hasTauriRuntime() || !hasTauriEventRuntime()) return;
    let unlisten: (() => void) | undefined;
    listen<string>("tray-navigate", (event) => {
      if (event.payload === "quick_note") {
        setActiveView("restore");
      } else if (event.payload === "eod") {
        generateRitual("daily");
      }
    })
      .then((fn) => { unlisten = fn; })
      .catch(() => undefined);
    return () => { unlisten?.(); };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement | null;
      const isTyping =
        target?.tagName === "INPUT" ||
        target?.tagName === "TEXTAREA" ||
        target?.isContentEditable;

      if (event.key === "Escape" && commandOpen) {
        setCommandOpen(false);
        setCommandQuery("");
        return;
      }

      if (isTyping) {
        return;
      }

      if ((event.metaKey && event.key.toLowerCase() === "k") || (event.altKey && event.code === "Space")) {
        event.preventDefault();
        setCommandOpen(true);
        setCommandQuery("");
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [commandOpen]);

  useEffect(() => {
    let ignore = false;
    const query = commandQuery.trim();

    if (!commandOpen || query.length < 2 || query.startsWith("/")) {
      setMemoryResults([]);
      return;
    }

    async function searchMemory() {
      const results = await invokeTauri<BackendSearchResult[]>("search_work_memory", {
        query,
        limit: 6,
      });

      if (!ignore) {
        setMemoryResults(results ?? []);
      }
    }

    searchMemory();

    return () => {
      ignore = true;
    };
  }, [commandOpen, commandQuery]);

  const commandResults = useMemo(() => {
    const query = commandQuery.trim().toLowerCase();

    if (!query) {
      return commandSuggestions;
    }

    return commandSuggestions
      .filter((item) => item.includes(query) || commandLabels[item]?.toLowerCase().includes(query))
      .slice(0, 5);
  }, [commandQuery]);

  const contextPack = useMemo(
    () => ({
      workspace: selectedStream.title,
      capture: isPaused ? "paused" : "active",
      source: backendReady ? "local fact store connected" : "Tauri bridge unavailable",
      pending: openActions.length,
      folders: selectedFolders.map((folder) => folder.path),
      ai: `${aiConfig.provider} / ${aiConfig.model}`,
      notes: notes.slice(0, 3).map((note) => note.text),
    }),
    [
      aiConfig,
      backendReady,
      isPaused,
      notes,
      openActions.length,
      selectedFolders,
      selectedStream.title,
    ],
  );

  async function addNote(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = quickNote.trim();

    if (!trimmed) {
      return;
    }

    const optimisticId = `note-${Date.now()}`;
    const optimisticNote = {
      id: optimisticId,
      text: trimmed,
      time: "Now",
      context: selectedStream.title,
    };

    setNotes((currentNotes) => [optimisticNote, ...currentNotes]);
    setQuickNote("");

    const savedNote = await invokeTauri<BackendQuickNote>("add_quick_note", {
      body: trimmed,
      source: "desktop-ui",
      projectPath: selectedStream.title,
    });

    if (!savedNote) {
      return;
    }

    setNotes((currentNotes) =>
      currentNotes.map((note) =>
        note.id === optimisticId
          ? {
              id: savedNote.id,
              text: savedNote.body,
              time: "Saved",
              context: savedNote.projectPath ?? savedNote.source ?? selectedStream.title,
            }
          : note,
      ),
    );
  }

  async function deleteMemoryFact(fact: MemoryFact) {
    if (fact.kind !== "quickNote") {
      return;
    }

    const confirmed = window.confirm(`Forget this saved memory?\n\n${fact.title}`);
    if (!confirmed) {
      return;
    }

    setNotes((currentNotes) => currentNotes.filter((note) => note.id !== fact.rawId));
    const deleted = await invokeTauri<{ deletedRows: number }>("delete_quick_note", {
      id: fact.rawId,
    });

    if (!deleted) {
      await refreshTodaySnapshot();
      return;
    }

    await refreshTodaySnapshot();
  }

  function updateAction(actionId: string, state: ActionItem["state"]) {
    setActions((currentActions) =>
      currentActions.map((action) =>
        action.id === actionId ? { ...action, state } : action,
      ),
    );
  }

  async function handleLoopAction(itemId: string, action: "closed" | "snoozed" | "ignored") {
    setDismissedLoopIds((current) => {
      const next = new Set(current);
      next.add(itemId);
      return next;
    });

    const snoozedUntil =
      action === "snoozed" ? Date.now() + 24 * 60 * 60 * 1000 : null;

    const saved = await invokeTauri("record_loop_action", {
      input: {
        id: itemId,
        action,
        snoozedUntil,
      },
    });

    if (!saved) {
      setActions((currentActions) =>
        currentActions.map((item) =>
          item.id === itemId
            ? { ...item, state: action === "snoozed" ? "snoozed" : "done" }
            : item,
        ),
      );
    }
  }

  function toggleFolder(folderId: string) {
    setFolders((currentFolders) =>
      currentFolders.map((folder) =>
        folder.id === folderId
          ? { ...folder, selected: !folder.selected }
          : folder,
      ),
    );
  }

  async function saveAiConfig(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!isProviderModelCompatible(draftAiConfig.provider, draftAiConfig.model)) {
      const corrected = {
        ...draftAiConfig,
        model: defaultModelForProvider(draftAiConfig.provider),
      };
      setDraftAiConfig(corrected);
      setSaveState(`Model reset for ${draftAiConfig.provider}`);
      return;
    }
    if (!isProviderEndpointCompatible(draftAiConfig.provider, draftAiConfig.endpoint)) {
      const corrected = {
        ...draftAiConfig,
        endpoint: defaultEndpointForProvider(draftAiConfig.provider),
      };
      setDraftAiConfig(corrected);
      setSaveState(`Endpoint reset for ${draftAiConfig.provider}`);
      return;
    }

    if (!hasTauriRuntime()) {
      setAiConfig(draftAiConfig);
      setDraftAiConfig(draftAiConfig);
      setSaveState(`${draftAiConfig.provider} ready`);
      return;
    }

    setSaveState("Saving settings...");

    const savedSettings = await invokeTauri<BackendSettings>("update_settings", {
      patch: {
        aiProvider: draftAiConfig.provider,
        aiModel: draftAiConfig.model,
        aiEndpoint: draftAiConfig.endpoint,
        launchAtLogin: draftLaunchAtLogin,
        aiRedactSecrets: draftAiConfig.redactSecrets,
        fullClipboardHistory: false,
      },
    });

    if (!savedSettings) {
      setSaveState("Save failed");
      addToast("error", "Settings not saved", "Could not write to local storage. Check disk space and try again.");
      return;
    }

    let finalSettings = savedSettings;
    if (draftAiConfig.apiKey.trim()) {
      const keySettings = await invokeTauri<BackendSettings>("set_ai_api_key", {
        provider: draftAiConfig.provider,
        apiKey: draftAiConfig.apiKey,
      });

      if (!keySettings) {
        setSaveState("Settings saved, API key save failed");
        addToast("error", "API key not saved", "Settings were saved but the API key could not be stored in the OS keychain.");
        return;
      }

      finalSettings = keySettings;
    }

    const savedConfig = mapAiConfig(finalSettings);
    setAiConfig(savedConfig);
    setDraftAiConfig(savedConfig);
    setSaveState(`${savedConfig.provider} saved`);
    addToast("success", "Settings saved");
  }

  async function saveExperienceSettings(patch: Partial<BackendSettings>) {
    const nextSettings = {
      ...(todaySnapshot?.settings ?? ({} as BackendSettings)),
      ...patch,
    };

    if (!hasTauriRuntime()) {
      setTodaySnapshot((previous) =>
        previous ? { ...previous, settings: nextSettings } : previous,
      );
      addToast("success", "Display mode updated");
      return;
    }

    setSaveState("Saving settings...");
    const savedSettings = await invokeTauri<BackendSettings>("update_settings", { patch });

    if (!savedSettings) {
      setSaveState("Save failed");
      addToast("error", "Settings not saved", "Could not update display mode.");
      return;
    }

    setTodaySnapshot((previous) =>
      previous ? { ...previous, settings: { ...previous.settings, ...savedSettings } } : previous,
    );
    setSaveState("Settings saved");
    addToast("success", "Display mode updated");
  }

  async function submitOfflineBlock(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    const now = new Date();
    const todayPrefix = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
    const startedAt = new Date(`${todayPrefix}T${offlineForm.start}:00`).getTime();
    const endedAt = new Date(`${todayPrefix}T${offlineForm.end}:00`).getTime();
    if (Number.isNaN(startedAt) || Number.isNaN(endedAt) || endedAt <= startedAt) {
      addToast("error", "Invalid time range", "End time must be after start time.");
      return;
    }
    const result = await invokeTauri("upsert_idle_block", {
      input: {
        id: null,
        startedAt,
        endedAt,
        category: offlineForm.category,
        classified: true,
        evidenceJson: null,
      },
    });
    if (result) {
      setLogOfflineOpen(false);
      setOfflineForm({ start: "", end: "", category: "Away from desk" });
      addToast("success", "Offline time logged", `${offlineForm.category} block saved.`);
      await refreshTodaySnapshot();
    } else {
      addToast("error", "Could not log offline time", "Backend unavailable.");
    }
  }

  async function saveWorkContext(form: WorkContextForm) {
    const result = await invokeTauri<BackendActiveWorkContext>("set_active_work_context", {
      input: {
        client: form.client.trim() || null,
        project: form.project.trim() || null,
        task: form.task.trim() || null,
        ticketId: form.ticketId.trim() || null,
        billable: form.billable,
      },
    });
    if (result) {
      setTodaySnapshot((prev) => prev ? { ...prev, activeWorkContext: result } : prev);
      const label = form.project || form.client || form.task || "context";
      addToast("success", "Work context set", `Tracking as: ${label}`);
    }
  }

  async function clearWorkContext() {
    await invokeTauri("clear_active_work_context");
    setTodaySnapshot((prev) => prev ? { ...prev, activeWorkContext: null } : prev);
    addToast("info", "Work context cleared", "Activity will be untagged until you set a new context.");
  }

  async function toggleTracking() {
    const nextPaused = !isPaused;

    const pauseState = nextPaused
      ? await invokeTauri<{ paused: boolean }>("pause_tracking", { reason: "manual" })
      : await invokeTauri<{ paused: boolean }>("resume_tracking");

    if (pauseState) {
      setIsPaused(pauseState.paused);
    } else {
      setSaveState("Capture control unavailable");
      addToast("error", "Tracking control unavailable", "Could not reach the desktop backend. Try restarting DayTrail.");
    }
  }

  async function loadReviewSessions() {
    setReviewLoading(true);
    const result = await invokeTauri<BackendWorkSessionSummary[]>(
      "list_sessions_for_review",
      { fromDate: reviewFromDate, toDate: reviewToDate },
    );
    setReviewSessions(result ?? []);
    setReviewLoading(false);
  }

  async function updateSessionBilling(
    id: string,
    patch: Partial<BackendWorkSessionSummary>,
  ) {
    const result = await invokeTauri<BackendWorkSessionSummary>("review_session", {
      input: {
        sessionId: id,
        billingStatus: patch.billingStatus ?? null,
        billable: patch.billable ?? null,
        clientLabel: patch.clientLabel ?? null,
        projectLabel: patch.projectLabel ?? null,
        ticketId: patch.ticketId ?? null,
        reviewNotes: patch.reviewNotes ?? null,
      },
    });
    if (result) {
      setReviewSessions((prev) =>
        prev.map((s) => (s.id === id ? result : s)),
      );
    }
  }

  async function exportTimesheetMarkdown() {
    setExportingTimesheet(true);
    const md = await invokeTauri<string>("export_timesheet_markdown", {
      fromDate: reviewFromDate,
      toDate: reviewToDate,
    });
    setExportingTimesheet(false);
    if (md) {
      await writeClipboardText(md);
      addToast("success", "Timesheet copied", "Markdown timesheet copied to clipboard.");
    } else {
      addToast("error", "Export failed", "Could not generate timesheet.");
    }
  }

  async function installTerminalBridge() {
    setTerminalBridgeStatus("Installing terminal bridge...");
    const result = await invokeTauri<BackendTerminalBridgeInstallResult>(
      "install_terminal_bridge",
    );

    if (!result) {
      setTerminalBridgeStatus("Install failed");
      addToast("error", "Terminal bridge install failed", "Could not write shell hook. Check that your shell profile is writable.");
      return;
    }

    setTerminalBridgeStatus(result.message);
    addToast("success", "Terminal bridge installed", "Open a new terminal tab for the hook to take effect.");
    await refreshTodaySnapshot();
  }

  async function generateDailyReport() {
    await generateRitual("daily");
  }

  function exportRangeArgs() {
    return {
      range: {
        fromDate: exportFromDate.trim() || null,
        toDate: exportToDate.trim() || null,
      },
    };
  }

  async function generateRawExport() {
    setExportStatus("Generating export...");
    const payload = await invokeTauri<BackendExportPayload>(
      "export_data_range",
      exportRangeArgs(),
    );

    if (!payload) {
      setExportStatus("Export unavailable");
      addToast("error", "Export failed", "Could not generate export. Make sure DayTrail has captured some activity first.");
      return;
    }

    setExportPreview(JSON.stringify(payload, null, 2));
    setExportStatus(
      `${payload.timesheetRows.length} activity row(s), ${payload.aiContributionRows.length} AI contribution(s), ${payload.sourceEvents.length} technical item(s)`,
    );
  }

  const [isAnalyzing, setIsAnalyzing] = useState(false);

  async function analyzeRawExport() {
    setIsAnalyzing(true);
    setExportStatus("Analyzing with configured AI...");
    const report = await invokeTauri<BackendReport>(
      "analyze_export_range",
      exportRangeArgs(),
    );
    setIsAnalyzing(false);

    if (!report?.bodyMarkdown) {
      setExportStatus("AI analysis unavailable");
      addToast("error", "AI analysis failed", "Check that an AI provider and API key are configured in Settings.");
      return;
    }

    setExportPreview(report.bodyMarkdown);
    setExportStatus(
      report.usedAi
        ? "AI analysis ready"
        : report.fallbackReason ?? "Source-backed analysis ready",
    );
  }

  async function exportSettingsConfig() {
    setStorageStatus("Exporting configuration...");
    const configJson = await invokeTauri<string>("export_settings_config");

    if (!configJson) {
      setStorageStatus("Configuration export unavailable");
      addToast("error", "Config export failed", "Could not read settings from the backend.");
      return;
    }

    setSettingsConfigJson(configJson);
    setStorageStatus("Configuration export ready");
    addToast("success", "Configuration exported", "Copy the JSON below to save or transfer your settings.");
  }

  async function importSettingsConfig() {
    const configJson = settingsConfigJson.trim();

    if (!configJson) {
      setStorageStatus("Paste exported configuration JSON first");
      return;
    }

    setStorageStatus("Importing configuration...");
    const imported = await invokeTauri<BackendSettings>("import_settings_config", {
      configJson,
    });

    if (!imported) {
      setStorageStatus("Configuration import failed");
      addToast("error", "Config import failed", "The JSON may be invalid or from an incompatible version.");
      return;
    }

    const importedConfig = mapAiConfig(imported);
    setAiConfig(importedConfig);
    setDraftAiConfig(importedConfig);
    setDraftLaunchAtLogin(imported.launchAtLogin ?? false);
    setSaveState(`${importedConfig.provider} imported`);
    setStorageStatus("Configuration imported");
    await refreshTodaySnapshot();
  }

  async function backupDatabase() {
    setStorageStatus("Creating database backup...");
    const backup = await invokeTauri<BackendDatabaseTransferResult>("backup_database");

    if (!backup) {
      setStorageStatus("Database backup failed");
      addToast("error", "Backup failed", "Could not write backup file. Check available disk space.");
      return;
    }

    setStorageStatus(`Backup created: ${backup.path}`);
    addToast("success", "Backup complete", backup.path);
    await loadStorageLocations();
  }

  async function restoreDatabase() {
    const path = databaseRestorePath.trim();

    if (!path) {
      setStorageStatus("Enter a database file path to restore");
      return;
    }

    const confirmed = window.confirm(
      "Restore this DayTrail database? A safety backup of the current database will be created first.",
    );
    if (!confirmed) {
      return;
    }

    setStorageStatus("Restoring database...");
    const restored = await invokeTauri<BackendDatabaseTransferResult>("restore_database", {
      path,
    });

    if (!restored) {
      setStorageStatus("Database restore failed");
      addToast("error", "Restore failed", "Could not restore the database. The file may be corrupt or from an incompatible version.");
      return;
    }

    const restoreMsg = restored.preRestoreBackupPath
      ? `Database restored. Safety backup: ${restored.preRestoreBackupPath}`
      : "Database restored";
    setStorageStatus(restoreMsg);
    addToast("success", "Restore complete", restored.preRestoreBackupPath ? `Safety backup saved to ${restored.preRestoreBackupPath}` : undefined);
    await refreshTodaySnapshot();
    await loadStorageLocations();
  }

  async function generateRitual(ritual: RitualKey = activeRitual) {
    setActiveView("rituals");
    setActiveRitual(ritual);

    const report = await invokeTauri<BackendReport>("generate_daily_report");

    setReportMarkdown(report?.bodyMarkdown || buildLocalReportMarkdown(ritual, todaySnapshot, experienceSettings));
  }

  async function regenerateContextData() {
    setSaveState("Refreshing work memory...");
    const refreshed = await invokeTauri("materialize_work_memory");
    const snapshot = await refreshTodaySnapshot();
    setSaveState(refreshed || snapshot ? "Context data refreshed" : "Context refresh unavailable");
  }

  function resumeCurrentContext() {
    setActiveView("today");
  }

  async function runCommand(command: string) {
    if (command === "/today" || command === "/what-did-i-do") {
      setActiveView("today");
    } else if (command === "/activity") {
      setActiveView("apps");
    } else if (command === "/ai-usage") {
      setActiveView("ai");
    } else if (command === "/export") {
      setActiveView("automation");
    } else if (command === "/report" || command === "/eod") {
      await generateRitual("daily");
    } else if (
      command === "/pending" ||
      command === "/commitments" ||
      command === "/reply-debt" ||
      command === "/stuck"
    ) {
      setActiveView("today");
    }

    setCommandQuery(command);
    setCommandOpen(false);
  }

  if (permissionSummary?.setupRequired && !permissionSetupDismissed) {
    return (
      <>
        <PermissionSetupView
          onContinue={() => setPermissionSetupDismissed(true)}
          onOpenSettings={openCapturePermissionSettings}
          onRefresh={loadCapturePermissions}
          onRestart={restartDayTrail}
          onResetAccessibility={resetAndRequestAccessibility}
          onTriggerBrowserAutomation={triggerBrowserAutomation}
          permissionStatus={permissionStatus}
          summary={permissionSummary}
        />
        <ToastContainer toasts={toasts} onDismiss={dismissToast} />
      </>
    );
  }

  return (
    <div className="app-shell">
      <aside className="native-sidebar" aria-label="Primary navigation">
        <div className="sidebar-brand">
          <img alt="" className="brand-mark" src="/daytrail-icon.png" />
          <span>
            <strong>DayTrail</strong>
            <em>Retrace your workday.</em>
          </span>
        </div>

        <nav className="nav-list" aria-label="Workspace views">
          {navigation.map((item) => (
            <button
              aria-label={item.label}
              aria-current={activeView === item.id ? "page" : undefined}
              className="nav-item"
              key={item.id}
              onClick={() => setActiveView(item.id)}
              title={item.label}
              type="button"
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </button>
          ))}
        </nav>

        {!isSimpleMode && sidebarApps.length > 0 && (
          <section className="sidebar-section sidebar-apps" aria-label="Apps today">
            <span className="sidebar-label">Apps Today</span>
            {sidebarApps.map((app) => (
              <button
                className={`sidebar-app-row${app.app === liveAppName ? " sidebar-app-row--live" : ""}`}
                key={app.app}
                onClick={() => {
                  setActiveAppName(app.app);
                  setActiveView("apps");
                }}
                title={`${app.app} - ${formatDuration(app.durationMs)}${app.app === liveAppName ? " · Active now" : ""}`}
                type="button"
              >
                <AppIcon appName={app.app} />
                <span>
                  <strong>{app.app}</strong>
                  <em>{formatDuration(app.durationMs)}</em>
                </span>
                {app.app === liveAppName && <span className="sidebar-live-dot" aria-label="Active now" />}
              </button>
            ))}
          </section>
        )}

        <div className="sidebar-offline-action">
          <button
            className="button compact sidebar-log-offline"
            onClick={() => {
              const now = new Date();
              const pad = (n: number) => String(n).padStart(2, "0");
              const hm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
              const oneHourAgo = new Date(now.getTime() - 60 * 60 * 1000);
              setOfflineForm({ start: hm(oneHourAgo), end: hm(now), category: "Away from desk" });
              setLogOfflineOpen(true);
            }}
            type="button"
          >
            + Log offline time
          </button>
        </div>

        <footer className="sidebar-footer">
          <button
            className="status-toggle"
            onClick={toggleTracking}
            type="button"
          >
            <span className={isPaused ? "status-light paused" : "status-light"} />
            {isPaused ? "Capture paused" : "Capturing"}
          </button>
          <span>{aiConfig.provider}</span>
        </footer>
      </aside>

      <main className="main-canvas">
        <header className="universal-toolbar" data-tauri-drag-region>
          <div>
            <h1>{currentViewLabel}</h1>
          </div>
          <div className="toolbar-actions">
            <button
              className="command-trigger"
              onClick={() => setCommandOpen(true)}
              aria-label="Search work"
              title="Search work"
              type="button"
            >
              <Icon name="search" />
              <span className="command-label">Search work</span>
              <kbd>⌥ Space</kbd>
              <kbd>⌘K</kbd>
            </button>
            <button
              className="button primary"
              onClick={() => generateRitual("daily")}
              aria-label="Generate daily report"
              title="Generate end-of-day report"
              type="button"
            >
              <Icon name="ritual" />
              <span className="button-label">Daily report</span>
            </button>
          </div>
        </header>

        <section className="content-pane" aria-live="polite">
          {activeView === "today" && (
            <TodayView
              actions={actions}
              aiUsageSummary={todaySnapshot?.aiUsageSummary}
              appUsageSummary={todaySnapshot?.appUsageSummary}
              onGenerateReport={generateDailyReport}
              onOpenHour={(hour) => {
                setActiveHourDetail(hour);
                setActiveView("hour");
              }}
              onOpenWorkContextEditor={(initialForm) => setWorkContextEditor(initialForm)}
              onClearWorkContext={clearWorkContext}
              onUpdateAction={updateAction}
              idleGapCount={todaySnapshot?.idleBlocks.filter((block) => !block.classified).length ?? 0}
              isPaused={isPaused}
              pendingReplyCount={pendingReplyCount}
              selectedStream={latestStream}
              sourceEvents={todaySnapshot?.sourceEvents ?? []}
              sessions={displaySessions}
              settings={todaySnapshot?.settings}
              snapshot={todaySnapshot}
              appCount={displayApps.length}
              bridgeStatus={bridgeStatus}
              backendReady={backendReady}
            />
          )}
          {activeView === "hour" && (
            <HourDetailView
              bucket={
                buildHourBuckets(todaySnapshot?.sourceEvents ?? [])[
                  activeHourDetail ?? new Date().getHours()
                ]
              }
              onBack={() => setActiveView("today")}
              onOpenActivity={() => setActiveView("apps")}
              settings={todaySnapshot?.settings}
            />
          )}
          {activeView === "apps" && (
            isSimpleMode ? (
              <SimpleActivityView settings={todaySnapshot?.settings} snapshot={todaySnapshot} />
            ) : (
              <AppsView
                activeAppName={activeAppName}
                setActiveAppName={setActiveAppName}
                summary={todaySnapshot?.appUsageSummary}
                sourceEvents={todaySnapshot?.sourceEvents ?? []}
              />
            )
          )}
          {activeView === "loops" && (
            <LoopsView items={displayLoopItems} onLoopAction={handleLoopAction} />
          )}
          {activeView === "ai" && (
            isSimpleMode ? (
              <SimpleAiImpactView settings={todaySnapshot?.settings} snapshot={todaySnapshot} />
            ) : (
              <AiLedgerView
                ledger={todaySnapshot?.aiOutputLedger ?? []}
                summary={todaySnapshot?.aiUsageSummary}
                appSummary={todaySnapshot?.appUsageSummary}
                sourceEvents={todaySnapshot?.sourceEvents ?? []}
              />
            )
          )}
          {activeView === "automation" && (
            <AutomationView
              aiProvider={aiConfig.provider}
              candidates={todaySnapshot?.automationCandidates ?? []}
              exportFromDate={exportFromDate}
              exportPreview={exportPreview}
              exportStatus={exportStatus}
              exportToDate={exportToDate}
              isAnalyzing={isAnalyzing}
              onAnalyze={analyzeRawExport}
              onExport={generateRawExport}
              setExportFromDate={setExportFromDate}
              setExportToDate={setExportToDate}
            />
          )}
          {activeView === "restore" && (
            <RestoreView
              addNote={addNote}
              aiThreads={displayAiThreads}
              notes={notes}
              onResume={resumeCurrentContext}
              quickNote={quickNote}
              selectedStream={selectedStream}
              setQuickNote={setQuickNote}
            />
          )}
          {activeView === "review" && (
            isSimpleMode ? (
              <NeedsReviewSimpleView settings={todaySnapshot?.settings} snapshot={todaySnapshot} />
            ) : (
              <ReviewView
                sessions={reviewSessions}
                loading={reviewLoading}
                fromDate={reviewFromDate}
                toDate={reviewToDate}
                onFromDate={setReviewFromDate}
                onToDate={setReviewToDate}
                onLoad={loadReviewSessions}
                onUpdate={updateSessionBilling}
                onExport={exportTimesheetMarkdown}
                exporting={exportingTimesheet}
              />
            )
          )}
          {activeView === "rituals" && (
            <RitualsView
              onOpenExports={() => setActiveView("automation")}
              onGenerateReport={() => generateRitual(activeRitual)}
              onRegenerateContext={regenerateContextData}
              reportMarkdown={reportMarkdown}
              settings={todaySnapshot?.settings}
              sourceFeed={displaySourceFeed}
              snapshot={todaySnapshot}
            />
          )}
          {activeView === "memory" && (
            <MemoryView
              contextPack={contextPack}
              facts={displayMemoryFacts}
              onDeleteFact={deleteMemoryFact}
              snapshot={todaySnapshot}
            />
          )}
          {activeView === "settings" && (
            <SettingsView
              aiConfig={draftAiConfig}
              captureHealth={todaySnapshot?.captureHealth}
              databaseRestorePath={databaseRestorePath}
              excludedDomainCount={todaySnapshot?.settings.excludedDomains.length ?? 0}
              folders={folders}
              launchAtLogin={draftLaunchAtLogin}
              onBackupDatabase={backupDatabase}
              onExportSettingsConfig={exportSettingsConfig}
              onImportSettingsConfig={importSettingsConfig}
              onLoadStorageInfo={loadStorageLocations}
              onInstallTerminalBridge={installTerminalBridge}
              onOpenCapturePermission={openCapturePermissionSettings}
              onOpenExports={() => setActiveView("automation")}
              onOpenSavedNotes={() => setActiveView("memory")}
              onRefreshCapturePermissions={loadCapturePermissions}
              onRestartApp={restartDayTrail}
              onTriggerBrowserAutomation={triggerBrowserAutomation}
              onRestoreDatabase={restoreDatabase}
              onSaveExperienceSettings={saveExperienceSettings}
              permissionStatus={permissionStatus}
              permissionSummary={permissionSummary}
              saveAiConfig={saveAiConfig}
              saveState={saveState}
              selectedCount={selectedFolders.length}
              setAiConfig={setDraftAiConfig}
              setDatabaseRestorePath={setDatabaseRestorePath}
              setSettingsConfigJson={setSettingsConfigJson}
              setSaveState={setSaveState}
              setLaunchAtLogin={setDraftLaunchAtLogin}
              settings={todaySnapshot?.settings}
              settingsConfigJson={settingsConfigJson}
              storageInfo={storageInfo}
              storageStatus={storageStatus}
              terminalBridgeStatus={terminalBridgeStatus}
              toggleFolder={toggleFolder}
            />
          )}
        </section>
      </main>

      {commandOpen && (
        <CommandOverlay
          commandQuery={commandQuery}
          commandResults={commandResults}
          memoryResults={memoryResults}
          onClose={() => setCommandOpen(false)}
          onRun={runCommand}
          setCommandQuery={setCommandQuery}
        />
      )}
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />

      {logOfflineOpen && (
        <div className="offline-modal-backdrop" onClick={() => setLogOfflineOpen(false)}>
          <form
            className="offline-modal"
            onClick={(e) => e.stopPropagation()}
            onSubmit={submitOfflineBlock}
          >
            <h3>Log offline time</h3>
            <p>Record time when your laptop was closed or you were away.</p>
            <div className="offline-modal-row">
              <label>
                Start
                <input
                  required
                  type="time"
                  value={offlineForm.start}
                  onChange={(e) => setOfflineForm((prev) => ({ ...prev, start: e.target.value }))}
                />
              </label>
              <label>
                End
                <input
                  required
                  type="time"
                  value={offlineForm.end}
                  onChange={(e) => setOfflineForm((prev) => ({ ...prev, end: e.target.value }))}
                />
              </label>
            </div>
            <label className="offline-modal-full">
              Category
              <select
                value={offlineForm.category}
                onChange={(e) => setOfflineForm((prev) => ({ ...prev, category: e.target.value }))}
              >
                <option>Away from desk</option>
                <option>Lunch / break</option>
                <option>Meeting</option>
                <option>Travel</option>
                <option>Exercise</option>
                <option>Focus time (offline)</option>
              </select>
            </label>
            <div className="offline-modal-actions">
              <button className="button" type="submit">Save block</button>
              <button className="button ghost" onClick={() => setLogOfflineOpen(false)} type="button">Cancel</button>
            </div>
          </form>
        </div>
      )}

      {workContextEditor && (
        <WorkContextEditorModal
          context={todaySnapshot?.activeWorkContext}
          initialForm={workContextEditor}
          onClear={clearWorkContext}
          onClose={() => setWorkContextEditor(null)}
          onSave={saveWorkContext}
        />
      )}
    </div>
  );
}

function ToastContainer({ toasts, onDismiss }: { toasts: Toast[]; onDismiss: (id: number) => void }) {
  if (toasts.length === 0) return null;
  return (
    <div className="toast-container" role="region" aria-label="Notifications" aria-live="polite">
      {toasts.map((toast) => (
        <div className="toast" data-kind={toast.kind} key={toast.id} role="alert">
          <span className="toast-icon" aria-hidden="true" />
          <div className="toast-body">
            <strong className="toast-title">{toast.title}</strong>
            {toast.message && <span className="toast-message">{toast.message}</span>}
          </div>
          <button className="toast-close" onClick={() => onDismiss(toast.id)} type="button" aria-label="Dismiss">×</button>
        </div>
      ))}
    </div>
  );
}

function PermissionSetupView({
  onContinue,
  onOpenSettings,
  onRefresh,
  onRestart,
  onResetAccessibility,
  onTriggerBrowserAutomation,
  permissionStatus,
  summary,
}: {
  onContinue: () => void;
  onOpenSettings: (permissionId: string) => void;
  onRefresh: () => void;
  onRestart: () => void;
  onResetAccessibility: () => void;
  onTriggerBrowserAutomation: () => void;
  permissionStatus: string;
  summary: BackendCapturePermissionSummary;
}) {
  const requiredCheck = summary.checks.find((check) => check.required && check.status !== "granted");
  const stillMissingAfterCheck = permissionStatus.startsWith("Still missing");

  return (
    <div className="permission-setup-shell">
      <main className="permission-setup-panel">
        <div className="permission-brand">
          <img alt="" className="brand-mark" src="/daytrail-icon.png" />
          <span>
            <strong>DayTrail</strong>
            <em>Local workday capture</em>
          </span>
        </div>
        <section className="permission-hero">
          <span>{permissionStatus}</span>
          <h1>{stillMissingAfterCheck ? "Still not detected — let's fix it" : "Allow app and window tracking"}</h1>
          <p>
            {stillMissingAfterCheck
              ? "macOS may have a stale or mismatched grant. Click \"Fix accessibility\" — it clears the old entry and opens System Settings so you can re-grant in one step."
              : "DayTrail needs Accessibility access to identify the active app and window title. It does not capture screenshots, keystrokes, clipboard text, or file contents."}
          </p>
          {stillMissingAfterCheck && (
            <ol className="permission-steps">
              <li>Click <strong>Fix accessibility</strong> below to open <strong>Privacy & Security &gt; Accessibility</strong>.</li>
              <li>Remove or toggle off any old <strong>DayTrail</strong> entry.</li>
              <li>Enable this exact app: <strong>{summary.appPath ?? "/Applications/DayTrail.app"}</strong>.</li>
              <li>Switch back to DayTrail and click <strong>Recheck</strong>.</li>
            </ol>
          )}
        </section>
        <PermissionStatusList
          onOpenSettings={onOpenSettings}
          onRefresh={onRefresh}
          onRestart={onRestart}
          onTriggerBrowserAutomation={onTriggerBrowserAutomation}
          summary={summary}
        />
        <div className="permission-actions">
          {stillMissingAfterCheck ? (
            <>
              <button className="button primary" onClick={onResetAccessibility} type="button">
                <Icon name="warning" />
                <span>Fix accessibility</span>
              </button>
              <button className="button" onClick={onRefresh} type="button">
                <Icon name="sync" />
                <span>Recheck</span>
              </button>
              <button className="button" onClick={onRestart} type="button">
                <Icon name="return" />
                <span>Restart app</span>
              </button>
            </>
          ) : (
            <>
              <button
                className="button primary"
                disabled={!requiredCheck}
                onClick={() => requiredCheck && onOpenSettings(requiredCheck.id)}
                type="button"
              >
                <Icon name="warning" />
                <span>{requiredCheck?.actionLabel ?? "Open Settings"}</span>
              </button>
              <button className="button" onClick={onRefresh} type="button">
                <Icon name="sync" />
                <span>Recheck</span>
              </button>
              <button className="button" onClick={onRestart} type="button">
                <Icon name="return" />
                <span>Restart app</span>
              </button>
            </>
          )}
          <button className="button compact" onClick={onContinue} type="button">
            Continue limited
          </button>
        </div>
      </main>
    </div>
  );
}

function PermissionStatusList({
  compact = false,
  onOpenSettings,
  onRefresh,
  onRestart,
  onTriggerBrowserAutomation,
  summary,
}: {
  compact?: boolean;
  onOpenSettings: (permissionId: string) => void;
  onRefresh: () => void;
  onRestart: () => void;
  onTriggerBrowserAutomation?: () => void;
  summary: BackendCapturePermissionSummary | null;
}) {
  const [copyStatus, setCopyStatus] = useState("Copy app path");

  async function copyAppPath() {
    if (!summary?.appPath) return;
    await writeClipboardText(summary.appPath);
    setCopyStatus("Copied");
    window.setTimeout(() => setCopyStatus("Copy app path"), 1800);
  }

  if (!summary) {
    return (
      <div className="empty-state compact-empty">
        <strong>Permission status unavailable</strong>
        <span>Open the installed desktop app to check OS permissions.</span>
      </div>
    );
  }

  return (
    <div className={compact ? "permission-list compact" : "permission-list"}>
      {summary.checks.map((check) => (
        <article className="permission-row" data-state={check.status} key={check.id}>
          <div className="permission-row-icon">
            <Icon name={check.status === "granted" ? "check" : check.required ? "warning" : "sliders"} />
          </div>
          <div className="permission-row-copy">
            <span>
              <strong>{check.label}</strong>
              <em>{check.required ? "Required" : "Optional"}</em>
            </span>
            <p>{check.detail}</p>
            {check.settingsLabel && <small>{check.settingsLabel}</small>}
          </div>
          <strong>{permissionStatusLabel(check.status)}</strong>
          {check.id === "browser-automation" && check.actionLabel ? (
            <div className="permission-row-actions">
              <button
                className="button compact primary"
                onClick={onTriggerBrowserAutomation ?? (() => onOpenSettings(check.id))}
                type="button"
              >
                <Icon name="check" />
                <span>{check.actionLabel}</span>
              </button>
              <button
                className="button compact"
                onClick={() => onOpenSettings(check.id)}
                type="button"
                title="Open Automation Settings"
              >
                <Icon name="sliders" />
              </button>
            </div>
          ) : check.actionLabel ? (
            <button
              className="button compact"
              onClick={() => onOpenSettings(check.id)}
              type="button"
            >
              <Icon name="arrow" />
              <span>{check.actionLabel}</span>
            </button>
          ) : null}
        </article>
      ))}
      <div className="permission-diagnostics">
        {summary.appPath && (
          <div className="permission-path-helper">
            <span>
              System Settings path: <strong>Privacy &amp; Security &gt; Accessibility</strong>
            </span>
            <span>
              Enable this exact app: <strong>{summary.appPath}</strong>
            </span>
            <button className="button compact" onClick={copyAppPath} type="button">
              <Icon name="copy" />
              {copyStatus}
            </button>
          </div>
        )}
        {(summary.diagnostics ?? []).map((item) => (
          <span key={item}>{item}</span>
        ))}
        {summary.appPath && (
          <span>
            App path: <strong>{summary.appPath}</strong>
          </span>
        )}
        {summary.executablePath && (
          <span>
            Executable: <strong>{summary.executablePath}</strong>
          </span>
        )}
      </div>
      <div className="permission-list-actions">
        <button className="button compact" onClick={onRefresh} type="button">
          <Icon name="sync" />
          Recheck permissions
        </button>
        {summary.restartRecommended && (
          <button className="button compact" onClick={onRestart} type="button">
            <Icon name="return" />
            Restart app
          </button>
        )}
      </div>
    </div>
  );
}

function permissionStatusMessage(summary: BackendCapturePermissionSummary) {
  if (summary.allRequiredGranted) {
    return "Permissions ready";
  }

  const suffix = new Date().toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
  const appHint = summary.appPath?.includes("/Applications/")
    ? "installed app"
    : "this app copy";
  return `Still missing for ${appHint} - checked ${suffix}`;
}

function permissionStatusLabel(status: string) {
  const normalized = status.replace(/_/g, " ");

  if (status === "granted") {
    return "Granted";
  }
  if (status === "missing") {
    return "Needs access";
  }
  if (status === "user_prompt") {
    return "Prompts when needed";
  }
  if (status === "limited") {
    return "Partial access";
  }
  if (status === "not_running") {
    return "No browser open";
  }
  if (status === "not_required") {
    return "Not required";
  }

  return normalized.charAt(0).toUpperCase() + normalized.slice(1);
}

function TodayView({
  actions,
  aiUsageSummary,
  appUsageSummary,
  idleGapCount,
  isPaused,
  onGenerateReport,
  onOpenHour,
  onOpenWorkContextEditor,
  onClearWorkContext,
  onUpdateAction,
  pendingReplyCount,
  selectedStream,
  sourceEvents,
  sessions,
  settings,
  snapshot,
  appCount,
  bridgeStatus,
  backendReady,
}: {
  actions: ActionItem[];
  aiUsageSummary?: BackendAiUsageSummary;
  appUsageSummary?: BackendAppUsageSummary;
  idleGapCount: number;
  isPaused: boolean;
  onGenerateReport: () => void;
  onOpenHour: (hour: number) => void;
  onOpenWorkContextEditor: (initialForm: Partial<WorkContextForm>) => void;
  onClearWorkContext: () => void;
  onUpdateAction: (actionId: string, state: ActionItem["state"]) => void;
  pendingReplyCount: number;
  selectedStream: Stream;
  sourceEvents: BackendSourceEvent[];
  sessions: WorkSession[];
  settings?: BackendSettings;
  snapshot: BackendTodaySnapshot | null;
  appCount: number;
  bridgeStatus: string;
  backendReady: boolean;
}) {
  const [inspectedSessionId, setInspectedSessionId] = useState<string | null>(null);
  const [selectedHour, setSelectedHour] = useState<number | null>(null);
  const todaySettings = normalizeExperienceSettings(settings);
  const visibleSourceEvents = useMemo(() => {
    if (todaySettings.experienceMode === "pro" || todaySettings.showSystemApps) {
      return sourceEvents;
    }
    return sourceEvents.filter((event) => isSimpleVisibleApp(eventAppLabel(event)));
  }, [sourceEvents, todaySettings.experienceMode, todaySettings.showSystemApps]);
  const visibleAppUsageSummary = useMemo(() => {
    if (!appUsageSummary || todaySettings.experienceMode === "pro" || todaySettings.showSystemApps) {
      return appUsageSummary;
    }
    return {
      ...appUsageSummary,
      apps: appUsageSummary.apps.filter((app) => isSimpleVisibleApp(app.app, app.category)),
    };
  }, [appUsageSummary, todaySettings.experienceMode, todaySettings.showSystemApps]);
  const openActions = actions.filter((action) => action.state === "open");
  const latestSession = sessions[0] ?? null;
  const latestEvent = [...sourceEvents].sort((left, right) => right.endedAt - left.endedAt)[0] ?? null;
  const hourBuckets = useMemo(() => buildHourBuckets(visibleSourceEvents), [visibleSourceEvents]);
  const simpleToday = useMemo(
    () => buildTodayView(snapshot, settings),
    [settings, snapshot],
  );
  const projectUsage = useMemo(() => buildProjectUsageBreakdown(visibleSourceEvents), [visibleSourceEvents]);
  const activeHour = selectedHour ?? (latestEvent ? new Date(latestEvent.endedAt).getHours() : new Date().getHours());
  const handleSelectHour = (hour: number) => {
    setSelectedHour(hour);
  };
  const selectedHourBucket = hourBuckets[activeHour] ?? hourBuckets[new Date().getHours()];
  const inspectedSession =
    sessions.find((session) => session.id === inspectedSessionId) ?? null;
  const inspectedEvents = inspectedSession
    ? sourceEventsForIds(visibleSourceEvents, inspectedSession.evidenceEventIds)
    : [];
  const hasStream = selectedStream.id !== "empty";
  const latestEventApp = latestEvent ? eventAppLabel(latestEvent) : null;
  const latestEventContext = latestEvent ? eventContextLabel(latestEvent) : null;
  const currentContext = latestEvent
    ? latestEventContext && latestEventContext !== latestEventApp
      ? `${latestEventApp} · ${latestEventContext}`
      : (latestEventApp ?? "Captured activity")
    : !backendReady
      ? "Desktop bridge not connected"
    : hasStream
      ? selectedStream.title
      : (latestSession?.title ?? "Waiting for captured activity");
  const currentSummary = latestEvent
    ? [
        eventTitle(latestEvent),
        eventSubtitle(latestEvent) || null,
        `${formatDuration(latestEvent.durationMs)} captured`,
      ]
          .filter(Boolean)
          .join(" - ")
    : hasStream
      ? selectedStream.summary
      : latestSession
        ? `${latestSession.project} - ${latestSession.tools}`
        : !backendReady
          ? bridgeStatus
          : "Open an editor, terminal, browser tab, or AI tool. DayTrail will attach the app, URL, and workspace folder when the signal is available.";
  const attentionCount = openActions.length + pendingReplyCount + idleGapCount;
  const aiToolCount = aiUsageSummary?.tools.length ?? 0;
  const aiActiveDuration = sourceEvents
    .filter((event) => aiToolLabelsForEvent(event).length > 0)
    .reduce((sum, event) => sum + event.durationMs, 0);
  const totalTrackedDuration = hourBuckets.reduce((sum, bucket) => sum + bucket.durationMs, 0);
  const stats = [
    { label: "Time tracked", value: simpleToday.totalTrackedLabel || formatDuration(totalTrackedDuration), detail: "captured today" },
    { label: "Work sessions", value: sessions.length, detail: "captured today" },
    { label: "Work apps", value: simpleToday.appCount || appCount, detail: "with activity" },
    {
      label: "AI detected",
      value: aiActiveDuration > 0 ? formatDuration(aiActiveDuration) : "0m",
      detail: aiToolCount ? `${aiToolCount} tool${aiToolCount === 1 ? "" : "s"}` : "not detected yet",
    },
    {
      label: "Top work app",
      value: simpleToday.topWorkApp?.name ?? "No work app yet",
      detail: simpleToday.topWorkApp?.durationLabel ?? "waiting",
    },
    { label: "Needs review", value: attentionCount, detail: "items" },
  ];

  return (
    <div className="view-frame today-view">
      <section className="today-live-card">
        <div className="focus-copy">
          <span className={!backendReady || isPaused ? "capture-pill paused" : "capture-pill"}>
            {!backendReady ? "Bridge offline" : isPaused ? "Paused" : "Capturing"}
          </span>
          <h2>Now: {currentContext}</h2>
          <p>{currentSummary}</p>
        </div>
        <div className="focus-actions">
          <button className="button compact" onClick={onGenerateReport} type="button">
            <Icon name="ritual" />
            Daily report
          </button>
        </div>
      </section>

      {simpleToday.lowData && (
        <section className="panel-block early-capture-panel">
          <PanelHeader eyebrow="Getting started" title="DayTrail is capturing your workday" value={simpleToday.totalTrackedLabel} />
          <p>
            Captured so far: {simpleToday.totalTrackedLabel} across {simpleToday.appCount} work app{simpleToday.appCount === 1 ? "" : "s"}.
            Keep working. Your timeline becomes useful after a few minutes.
          </p>
        </section>
      )}

      <section className="today-stat-strip" aria-label="Today stats">
        {stats.map((stat) => (
          <div className="stat-card" key={stat.label}>
            <span>{stat.label}</span>
            <strong>{stat.value}</strong>
            <em>{stat.detail}</em>
          </div>
        ))}
      </section>

      <section className="today-hero-grid" aria-label="Daily timeline and selected hour">
        <HourlyTimelinePanel
          buckets={hourBuckets}
          onClearWorkContext={onClearWorkContext}
          onOpenWorkContextEditor={onOpenWorkContextEditor}
          onSelectHour={handleSelectHour}
          selectedHour={activeHour}
        />
        <SelectedHourPanel
          bucket={selectedHourBucket}
          onOpenFullBreakdown={() => onOpenHour(activeHour)}
        />
      </section>

      <section className="today-highlights-grid" aria-label="Today highlights">
        <ProjectUsagePanel projects={projectUsage} />
        <AppUsagePanel summary={visibleAppUsageSummary} />
        <AiUsagePanel activeDurationMs={aiActiveDuration} summary={aiUsageSummary} />
        <section className="panel-block recent-panel">
          <PanelHeader
            eyebrow="Recent highlights"
            title="What you worked on"
            value={`${sessions.length} captured`}
          />
          <div className="recent-highlight-list">
            {sessions.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No work captured yet</strong>
                <span>Keep DayTrail open while you use editors, terminals, browsers, and AI tools.</span>
              </div>
            )}
            {sessions.slice(0, 6).map((session) => (
              <button
                aria-pressed={inspectedSession?.id === session.id}
                aria-label={`Open details for ${session.title}`}
                className="recent-highlight-card"
                key={session.id}
                onClick={() => setInspectedSessionId(session.id)}
                title={`${session.title} - ${session.project}`}
                type="button"
              >
                <span>{session.time}</span>
                <strong>{session.title}</strong>
                <em>{session.project}</em>
                <small>{session.tools}</small>
              </button>
            ))}
          </div>
        </section>
        <section className="panel-block attention-panel">
          <PanelHeader
            eyebrow="Next actions"
            title="Needs review"
            value={`${openActions.length} open`}
          />
          <div className="action-list">
            {openActions.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No open actions</strong>
                <span>Unclear sessions, idle gaps, AI activity without context, and unfinished report inputs appear here when detected.</span>
              </div>
            )}
            {openActions.slice(0, 6).map((action) => (
              <article className="action-row" data-state={action.state} key={action.id}>
                <label>
                  <input
                    checked={action.state === "done"}
                    onChange={() => onUpdateAction(action.id, "done")}
                    type="checkbox"
                  />
                  <span>
                    <strong>{action.title}</strong>
                    <em>{action.source}</em>
                  </span>
                </label>
                <div className="action-row-actions">
                  <button
                    className="text-button"
                    onClick={() => onUpdateAction(action.id, "snoozed")}
                    type="button"
                  >
                    Snooze
                  </button>
                  <button
                    className="text-button"
                    onClick={() => onUpdateAction(action.id, "done")}
                    type="button"
                  >
                    Done
                  </button>
                </div>
              </article>
            ))}
          </div>
        </section>
      </section>

      {inspectedSession && (
        <SessionDetailPanel
          events={inspectedEvents}
          onClose={() => setInspectedSessionId(null)}
          session={inspectedSession}
        />
      )}
    </div>
  );
}

function HourlyTimelinePanel({
  buckets,
  onClearWorkContext,
  onOpenWorkContextEditor,
  onSelectHour,
  selectedHour,
}: {
  buckets: HourBucket[];
  onClearWorkContext: () => void;
  onOpenWorkContextEditor: (initialForm: Partial<WorkContextForm>) => void;
  onSelectHour: (hour: number) => void;
  selectedHour: number;
}) {
  const [showFullDay, setShowFullDay] = useState(false);
  const [contextMenu, setContextMenu] = useState<{
    bucket: HourBucket;
    x: number;
    y: number;
  } | null>(null);
  const totalDuration = buckets.reduce((sum, bucket) => sum + bucket.durationMs, 0);
  const activeBuckets = buckets.filter((bucket) => bucket.durationMs > 0);
  const visibleBuckets = showFullDay || activeBuckets.length === 0 ? buckets : activeBuckets;
  const topApps = [...buckets.reduce((apps, bucket) => {
    bucket.apps.forEach((app) => {
      apps.set(app.app, (apps.get(app.app) ?? 0) + app.durationMs);
    });
    return apps;
  }, new Map<string, number>())]
    .sort((left, right) => right[1] - left[1])
    .slice(0, 6);
  const suggestedContextForBucket = (bucket: HourBucket): Partial<WorkContextForm> => {
    const topApp = bucket.apps[0] ?? null;
    const rawContext = topApp?.contexts.find((context) => context && context !== topApp.app);
    return {
      project: rawContext ? compactDisplayLabel(rawContext) : "",
      task: topApp ? `${bucket.label} - ${topApp.app}` : `${bucket.label} work`,
      billable: true,
    };
  };
  const openContextMenu = (event: MouseEvent, bucket: HourBucket) => {
    event.preventDefault();
    setContextMenu({
      bucket,
      x: Math.min(event.clientX, window.innerWidth - 240),
      y: Math.min(event.clientY, window.innerHeight - 120),
    });
  };

  return (
    <section className="panel-block hourly-panel">
      <PanelHeader
        eyebrow="Day tracker"
        title="24-hour timeline"
        value={totalDuration > 0 ? formatDuration(totalDuration) : "No activity"}
      />
      {topApps.length > 0 && (
        <div className="hour-legend" aria-label="Top app color legend">
          {topApps.map(([app, duration]) => (
            <span key={app}>
              <i style={{ background: appColor(app) }} />
              {app} · {formatDuration(duration)}
            </span>
          ))}
        </div>
      )}
      <div className="hour-filter-bar">
        <span>
          {activeBuckets.length === 0
            ? "No activity captured yet — use your computer for a few minutes and DayTrail will populate this view"
            : showFullDay
              ? "Showing all 24 hours"
              : `Showing ${activeBuckets.length} active hour${activeBuckets.length === 1 ? "" : "s"}`}
        </span>
        <button
          className="button compact"
          disabled={activeBuckets.length === 0}
          onClick={() => setShowFullDay((value) => !value)}
          type="button"
        >
          {showFullDay ? "Show active hours" : "Show all 24 hours"}
        </button>
      </div>
      <div className="hour-timeline-list" aria-label="Day activity by hour">
        {visibleBuckets.map((bucket) => {
          const hourFillPercent = Math.min(100, Math.round((bucket.durationMs / (60 * 60 * 1000)) * 100));

          return (
            <button
              aria-pressed={selectedHour === bucket.hour}
              className="hour-row"
              key={bucket.hour}
              onClick={() => onSelectHour(bucket.hour)}
              onContextMenu={(event) => openContextMenu(event, bucket)}
              title={`${bucket.label}: ${bucket.apps.map((app) => `${app.app} ${formatDuration(app.durationMs)}`).join(", ") || "No activity"}`}
              type="button"
            >
              <span className="hour-label">{bucket.label}</span>
              <span className="hour-stack">
                <span className="hour-row-fill" style={{ width: `${hourFillPercent}%` }}>
                  {bucket.apps.map((app) => {
                    const width = Math.max(6, Math.round((app.durationMs / Math.max(bucket.durationMs, 1)) * 100));
                    return (
                      <span
                        className="hour-segment"
                        key={app.app}
                        style={{ background: appColor(app.app), width: `${width}%` }}
                        title={`${app.app}: ${formatDuration(app.durationMs)}${app.aiTools.length ? ` · AI: ${app.aiTools.join(", ")}` : ""}`}
                      >
                        <span />
                      </span>
                    );
                  })}
                </span>
              </span>
              <strong>{bucket.durationMs > 0 ? formatDuration(bucket.durationMs) : "-"}</strong>
              {bucket.aiTools.length > 0 && (
                <em className="hour-ai-badges">{bucket.aiTools.slice(0, 3).join(", ")}</em>
              )}
            </button>
          );
        })}
      </div>
      {contextMenu && (
        <div className="hour-context-menu-backdrop" onClick={() => setContextMenu(null)}>
          <div
            className="hour-context-menu"
            onClick={(event) => event.stopPropagation()}
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              onClick={() => {
                onOpenWorkContextEditor(suggestedContextForBucket(contextMenu.bucket));
                setContextMenu(null);
              }}
              type="button"
            >
              Set current task from this hour
            </button>
            <button
              onClick={() => {
                onClearWorkContext();
                setContextMenu(null);
              }}
              type="button"
            >
              Clear current task
            </button>
          </div>
        </div>
      )}
    </section>
  );
}

function SelectedHourPanel({
  bucket,
  onOpenFullBreakdown,
}: {
  bucket?: HourBucket;
  onOpenFullBreakdown: () => void;
}) {
  const hour = bucket?.hour ?? new Date().getHours();
  const label = bucket
    ? `${bucket.label} - ${localHourLabel((bucket.hour + 1) % 24)}`
    : `${localHourLabel(hour)} - ${localHourLabel((hour + 1) % 24)}`;
  const aiByTool = bucket
    ? [...bucket.events.reduce((tools, event) => {
        aiToolLabelsForEvent(event).forEach((tool) => {
          tools.set(tool, (tools.get(tool) ?? 0) + event.durationMs);
        });
        return tools;
      }, new Map<string, number>())].sort((left, right) => right[1] - left[1])
    : [];

  return (
    <aside className="panel-block selected-hour-panel">
      <PanelHeader
        eyebrow="Selected hour"
        title={label}
        value={bucket?.durationMs ? formatDuration(bucket.durationMs) : "No activity"}
      />
      <div className="selected-hour-metrics">
        <div>
          <span>Total tracked</span>
          <strong>{formatDuration(bucket?.durationMs ?? 0)}</strong>
        </div>
        <div>
          <span>Apps</span>
          <strong>{bucket?.apps.length ?? 0}</strong>
        </div>
        <div>
          <span>AI detected</span>
          <strong>{bucket?.aiTools.length ?? 0}</strong>
        </div>
      </div>
      <div className="selected-hour-list">
        {(bucket?.apps ?? []).slice(0, 7).map((app) => (
          <article className="selected-hour-row" key={app.app}>
            <AppIcon appName={app.app} className="app-color-dot" />
            <div>
              <strong>{app.app}</strong>
              <em>{app.contexts.slice(0, 2).join(" · ") || "Activity details available"}</em>
            </div>
            <span>{formatDuration(app.durationMs)}</span>
          </article>
        ))}
        {bucket && bucket.apps.length === 0 && (
          <div className="empty-state compact-empty">
            <strong>No activity in this hour</strong>
            <span>Apps, sites, folders, and AI tools appear here when captured.</span>
          </div>
        )}
      </div>
      {aiByTool.length > 0 && (
        <div className="tool-chip-row" aria-label="AI tools detected in selected hour">
          {aiByTool.slice(0, 5).map(([tool, duration]) => (
            <span className="tool-chip" key={tool}>
              {tool} · {formatDuration(duration)}
            </span>
          ))}
        </div>
      )}
      <button className="button compact" onClick={onOpenFullBreakdown} type="button">
        View full hour breakdown
        <Icon name="arrow" />
      </button>
    </aside>
  );
}

function HourDetailView({
  bucket,
  onBack,
  onOpenActivity,
  settings,
}: {
  bucket: HourBucket;
  onBack: () => void;
  onOpenActivity: () => void;
  settings?: BackendSettings;
}) {
  const [selectedAppName, setSelectedAppName] = useState<string | null>(null);
  const detailSettings = normalizeExperienceSettings(settings);
  const showTechnicalDetails = detailSettings.experienceMode === "pro" && detailSettings.showRawEvents;
  const aiDuration = bucket.apps
    .filter((app) => app.aiTools.length > 0)
    .reduce((sum, app) => sum + app.durationMs, 0);
  const hourStart = bucket.label;
  const hourEnd = localHourLabel((bucket.hour + 1) % 24);
  const topApp = bucket.apps[0] ?? null;
  const contexts = uniqueValues(bucket.apps.flatMap((app) => app.contexts));
  const aiByTool = [...bucket.events.reduce((tools, event) => {
    aiToolLabelsForEvent(event).forEach((tool) => {
      tools.set(tool, (tools.get(tool) ?? 0) + event.durationMs);
    });
    return tools;
  }, new Map<string, number>())].sort((left, right) => right[1] - left[1]);
  const hourLabel = `${hourStart} - ${hourEnd}`;
  const eventTimeLabel = (event: BackendSourceEvent) =>
    new Date(event.startedAt).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  const selectedApp = selectedAppName
    ? bucket.apps.find((app) => app.app === selectedAppName) ?? null
    : null;
  const selectedAppEvents = selectedApp
    ? bucket.events.filter((event) => eventAppLabel(event) === selectedApp.app)
    : [];

  useEffect(() => {
    if (selectedAppName && !bucket.apps.some((app) => app.app === selectedAppName)) {
      setSelectedAppName(null);
    }
  }, [bucket.apps, selectedAppName]);

  return (
    <div className="view-frame hour-detail-view">
      <div className="hour-detail-topbar">
        <button className="button compact" onClick={onBack} type="button">
          <Icon name="return" />
          Back to Today
        </button>
        <div className="hour-detail-title">
          <span>Hour breakdown</span>
          <h2>{hourLabel}</h2>
          <p>{bucket.durationMs > 0 ? `${formatDuration(bucket.durationMs)} tracked` : "No captured activity in this hour"}</p>
        </div>
        <button className="button compact" onClick={onOpenActivity} type="button">
          Open Activity
          <Icon name="arrow" />
        </button>
      </div>

      <section className="hour-metric-strip" aria-label="Hour metrics">
        <div className="stat-card">
          <span>Time spent</span>
          <strong>{formatDuration(bucket.durationMs)}</strong>
          <em>{bucket.events.length} item{bucket.events.length === 1 ? "" : "s"}</em>
        </div>
        <div className="stat-card">
          <span>Apps used</span>
          <strong>{bucket.apps.length}</strong>
          <em>{topApp?.app ?? "No app"}</em>
        </div>
        <div className="stat-card">
          <span>AI detected</span>
          <strong>{formatDuration(aiDuration)}</strong>
          <em>{bucket.aiTools.length ? bucket.aiTools.join(", ") : "No AI detected"}</em>
        </div>
        <div className="stat-card">
          <span>Top app</span>
          <strong>{topApp?.app ?? "-"}</strong>
          <em>{topApp ? formatDuration(topApp.durationMs) : "waiting"}</em>
        </div>
      </section>

      <div className="hour-detail-layout">
        <main className="hour-detail-main">
          <section className="panel-block hour-distribution-panel">
            <PanelHeader eyebrow="Time distribution" title="Within this hour" value={hourLabel} />
            <div className="within-hour-track" aria-label="App distribution in selected hour">
              {bucket.apps.length === 0 && <span className="within-hour-empty" />}
              {bucket.apps.map((app) => (
                <span
                  className="within-hour-segment"
                  key={app.app}
                  style={{
                    background: appColor(app.app),
                    width: `${Math.max(4, Math.round((app.durationMs / Math.max(bucket.durationMs, 1)) * 100))}%`,
                  }}
                  title={`${app.app}: ${formatDuration(app.durationMs)}`}
                />
              ))}
            </div>
          </section>

          <section className="panel-block hour-app-panel">
            <PanelHeader eyebrow="Apps and context" title="What happened in this hour" value={`${bucket.apps.length} app${bucket.apps.length === 1 ? "" : "s"}`} />
            <div className="hour-app-list">
              {bucket.apps.length === 0 && (
                <div className="empty-state compact-empty">
                  <strong>No captured activity</strong>
                  <span>This hour will show apps, folders, browser domains, and AI tools when activity exists.</span>
                </div>
              )}
              {bucket.apps.map((app) => {
                const visibleContext = [...app.examples, ...app.contexts]
                  .filter((value, index, values) => value && values.indexOf(value) === index)
                  .slice(0, 4);
                const hasFiles = app.files.length > 0;
                return (
                  <button
                    aria-label={`Show ${app.app} breakdown`}
                    aria-pressed={selectedAppName === app.app}
                    className="hour-app-row"
                    key={app.app}
                    onClick={() => setSelectedAppName(app.app)}
                    type="button"
                  >
                    <AppIcon appName={app.app} className="app-color-dot" />
                    <div className="hour-app-row-body">
                      <div className="hour-app-row-header">
                        <strong>{app.app}</strong>
                        <span className="hour-app-row-meta">
                          {formatDuration(app.durationMs)} · {app.events} item{app.events === 1 ? "" : "s"} · Details
                        </span>
                      </div>
                      {hasFiles ? (
                        <ul className="hour-file-list" aria-label={`Files in ${app.app}`}>
                          {app.files.map((file) => (
                            <li className="hour-file-row" key={`${file.name}|${file.context ?? ""}`}>
                              <span className="hour-file-name">{file.name}</span>
                              {file.context && <em className="hour-file-context">{file.context}</em>}
                              <span className="hour-file-duration">{formatDuration(file.durationMs)}</span>
                            </li>
                          ))}
                        </ul>
                      ) : (
                        <em className="hour-app-row-context">{visibleContext.join(" · ") || "No file, folder, or site captured"}</em>
                      )}
                      {app.aiTools.length > 0 && (
                        <span className="tool-chip-row">
                          {app.aiTools.map((tool) => (
                            <span className="tool-chip" key={tool}>{tool}</span>
                          ))}
                        </span>
                      )}
                    </div>
                  </button>
                );
              })}
              {bucket.idleGaps.length > 0 && (
                <div className="hour-idle-gaps">
                  <span className="hour-idle-gaps-label">Away gaps detected</span>
                  {bucket.idleGaps.map((gap) => {
                    const startTime = new Date(gap.startMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
                    const endTime = new Date(gap.endMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
                    return (
                      <article className="hour-idle-gap-row" key={gap.startMs}>
                        <span className="hour-idle-dot" />
                        <div>
                          <strong>Away</strong>
                          <em>{startTime} – {endTime}</em>
                        </div>
                        <span>{formatDuration(gap.durationMs)}</span>
                      </article>
                    );
                  })}
                </div>
              )}
            </div>
          </section>

          {showTechnicalDetails && (
          <section className="panel-block hour-events-panel">
            <PanelHeader eyebrow="Technical details" title="Activity details" value={`${bucket.events.length} item${bucket.events.length === 1 ? "" : "s"}`} />
            <div className="hour-event-list">
              {bucket.events.length === 0 && (
                <div className="empty-state compact-empty">
                  <strong>No activity details</strong>
                  <span>DayTrail did not capture technical details for this hour.</span>
                </div>
              )}
              {bucket.events.slice(0, 40).map((event) => {
                const eventAiTools = aiToolLabelsForEvent(event);
                return (
                  <article className="hour-event-row" key={event.id}>
                    <span>{eventTimeLabel(event)}</span>
                    <strong>{eventAppLabel(event)}</strong>
                    <em>{eventTitle(event)}</em>
                    <small>{eventSubtitle(event) || eventContextLabel(event)}</small>
                    <b>{formatDuration(event.durationMs)}</b>
                    {eventAiTools.length > 0 && <i>{eventAiTools.join(", ")}</i>}
                  </article>
                );
              })}
            </div>
          </section>
          )}
        </main>

        <aside className="hour-detail-sidebar">
          <section className="panel-block hour-ai-panel">
            <PanelHeader eyebrow="AI in this hour" title="Tool usage" value={formatDuration(aiDuration)} />
            <div className="ai-tool-list">
              {aiByTool.length === 0 && (
                <div className="empty-state compact-empty">
                  <strong>No AI detected</strong>
                  <span>Copilot, Codex, Claude, ChatGPT, Gemini, Cursor, and terminal agents appear here when captured.</span>
                </div>
              )}
              {aiByTool.map(([tool, duration]) => (
                <article className="ai-tool-row" key={tool}>
                  <strong>{tool}</strong>
                  <span>{formatDuration(duration)}</span>
                  <div>
                    <i style={{ width: `${Math.min(100, Math.round((duration / Math.max(aiDuration, 1)) * 100))}%` }} />
                  </div>
                </article>
              ))}
            </div>
          </section>

          <section className="panel-block hour-context-panel">
            <PanelHeader eyebrow="Context" title="Folders and sites" value={`${contexts.length} places`} />
            <div className="context-stack">
              {contexts.length === 0 && <span>No captured context</span>}
              {contexts.slice(0, 12).map((context) => (
                <span key={context}>{context}</span>
              ))}
            </div>
          </section>
        </aside>
      </div>
      {selectedApp && (
        <AppBreakdownPanel
          app={selectedApp}
          events={selectedAppEvents}
          hourLabel={hourLabel}
          onClose={() => setSelectedAppName(null)}
        />
      )}
    </div>
  );
}

function AppBreakdownPanel({
  app,
  events,
  hourLabel,
  onClose,
}: {
  app: HourAppBreakdown;
  events: BackendSourceEvent[];
  hourLabel: string;
  onClose: () => void;
}) {
  const eventTimeLabel = (event: BackendSourceEvent) =>
    new Date(event.startedAt).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  const contexts = uniqueValues(
    events
      .flatMap((event) => [
        eventDocumentLabel(event),
        eventSubtitle(event),
        eventContextLabel(event),
      ])
      .filter((value): value is string => Boolean(value)),
  );
  const aiByTool = [...events.reduce((tools, event) => {
    aiToolLabelsForEvent(event).forEach((tool) => {
      tools.set(tool, (tools.get(tool) ?? 0) + event.durationMs);
    });
    return tools;
  }, new Map<string, number>())].sort((left, right) => right[1] - left[1]);
  const sortedEvents = [...events].sort((left, right) => left.startedAt - right.startedAt);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  return (
    <div className="app-breakdown-overlay" onMouseDown={onClose} role="presentation">
      <section
        aria-label={`${app.app} breakdown`}
        aria-modal="true"
        className="panel-block app-breakdown-panel"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="app-breakdown-header">
          <div>
            <span>App breakdown</span>
            <h3>{app.app}</h3>
            <p>{hourLabel} · {formatDuration(app.durationMs)} · {app.events} item{app.events === 1 ? "" : "s"}</p>
          </div>
          <button aria-label={`Close ${app.app} breakdown`} className="icon-button" onClick={onClose} type="button">
            <Icon name="x" />
          </button>
        </header>

        <div className="app-breakdown-body">
          <div className="app-breakdown-grid">
            <div className="app-breakdown-column">
              <div className="app-breakdown-section-title">
                <span>What was open</span>
                <strong>{app.files.length || contexts.length}</strong>
              </div>
              {app.files.length > 0 ? (
                <div className="app-breakdown-file-list">
                  {app.files.map((file) => (
                    <article className="app-breakdown-file-row" key={`${file.name}|${file.context ?? ""}`}>
                      <strong>{file.name}</strong>
                      <em>{file.context ?? app.app}</em>
                      <span>{formatDuration(file.durationMs)}</span>
                    </article>
                  ))}
                </div>
              ) : (
                <div className="context-stack app-breakdown-contexts">
                  {contexts.length === 0 && <span>No page, file, folder, or title captured</span>}
                  {contexts.slice(0, 10).map((context) => (
                    <span key={context}>{context}</span>
                  ))}
                </div>
              )}
            </div>

            <div className="app-breakdown-column">
              <div className="app-breakdown-section-title">
                <span>AI and context</span>
                <strong>{aiByTool.length ? `${aiByTool.length} tool${aiByTool.length === 1 ? "" : "s"}` : "None"}</strong>
              </div>
              {aiByTool.length > 0 ? (
                <div className="ai-tool-list compact-ai-tool-list">
                  {aiByTool.map(([tool, duration]) => (
                    <article className="ai-tool-row" key={tool}>
                      <strong>{tool}</strong>
                      <span>{formatDuration(duration)}</span>
                      <div>
                        <i style={{ width: `${Math.min(100, Math.round((duration / Math.max(app.durationMs, 1)) * 100))}%` }} />
                      </div>
                    </article>
                  ))}
                </div>
              ) : (
                <div className="empty-state compact-empty">
                  <strong>No AI detected in this app</strong>
                  <span>AI labels appear when DayTrail sees known AI tools in titles, URLs, or metadata.</span>
                </div>
              )}
            </div>
          </div>

          <div className="app-breakdown-section-title">
            <span>Event timeline</span>
            <strong>{sortedEvents.length} item{sortedEvents.length === 1 ? "" : "s"}</strong>
          </div>
          <div className="app-event-timeline">
            {sortedEvents.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No detailed events</strong>
                <span>This app was summarized before source-event details were available.</span>
              </div>
            )}
            {sortedEvents.map((event) => {
              const eventAiTools = aiToolLabelsForEvent(event);
              return (
                <article className="app-event-row" key={event.id}>
                  <span>{eventTimeLabel(event)}</span>
                  <strong>{eventTitle(event)}</strong>
                  <em>{eventSubtitle(event) || eventContextLabel(event)}</em>
                  <b>{formatDuration(event.durationMs)}</b>
                  {eventAiTools.length > 0 && <i>{eventAiTools.join(", ")}</i>}
                </article>
              );
            })}
          </div>
        </div>
      </section>
    </div>
  );
}

function SessionDetailPanel({
  events,
  onClose,
  session,
}: {
  events: BackendSourceEvent[];
  onClose: () => void;
  session: WorkSession;
}) {
  const apps = uniqueValues(events.map(eventAppLabel));
  const contexts = uniqueValues(events.map(eventContextLabel));
  const aiTools = uniqueValues(events.flatMap(aiToolLabelsForEvent));

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  return (
    <div className="session-inspector-overlay" onMouseDown={onClose} role="presentation">
      <section
        aria-label={`Session details for ${session.title}`}
        aria-modal="true"
        className="panel-block session-detail-panel session-detail-sheet"
        onMouseDown={(event) => event.stopPropagation()}
        role="dialog"
      >
        <header className="panel-header sheet-header">
          <div>
            <span>Session details</span>
            <h2>{session.title}</h2>
          </div>
          <div className="sheet-header-actions">
            <em>{events.length} event{events.length === 1 ? "" : "s"}</em>
            <button aria-label="Close session details" className="icon-button" onClick={onClose} type="button">
              <Icon name="x" />
            </button>
          </div>
        </header>
        {events.length > 0 && (
          <div className="detail-chip-strip">
            <span>{apps.slice(0, 3).join(", ")}</span>
            <span>{contexts.slice(0, 2).join(", ")}</span>
            <span>{aiTools.length ? `AI: ${aiTools.join(", ")}` : "No AI detected"}</span>
          </div>
        )}
        <div className="event-detail-list">
        {session && events.length === 0 && (
          <div className="empty-state compact-empty">
            <strong>No source details linked</strong>
            <span>This session was captured before detailed evidence was available.</span>
          </div>
        )}
        {events.map((event) => {
          const eventAiTools = aiToolLabelsForEvent(event);
          const domainCat = classifyDomain(event.domain);

          return (
            <article className="event-detail-row" key={event.id}>
              <span className="event-app">{eventAppLabel(event)}</span>
              <strong>{eventTitle(event)}</strong>
              <em>{eventSubtitle(event) || eventContextLabel(event)}</em>
              <small>{formatDuration(event.durationMs)}</small>
              {(eventAiTools.length > 0 || domainCat) && (
                <span className="event-ai-chip-row">
                  {eventAiTools.map((tool) => (
                    <span className="event-ai-chip" key={tool}>{tool}</span>
                  ))}
                  {domainCat && eventAiTools.length === 0 && (
                    <span
                      className="event-category-chip"
                      style={{ background: DOMAIN_CATEGORY_COLORS[domainCat] }}
                    >
                      {DOMAIN_CATEGORY_LABELS[domainCat]}
                    </span>
                  )}
                </span>
              )}
            </article>
          );
        })}
        </div>
      </section>
    </div>
  );
}

function AiUsagePanel({
  activeDurationMs,
  summary,
}: {
  activeDurationMs?: number;
  summary?: BackendAiUsageSummary;
}) {
  const tools = summary?.tools ?? [];
  const total = activeDurationMs ?? summary?.totalDurationMs ?? 0;

  return (
    <section className="panel-block ai-usage-panel">
      <PanelHeader
        eyebrow="AI tools detected"
        title={total > 0 ? formatDuration(total) : "No AI captured"}
        value={
          activeDurationMs === undefined
            ? `${summary?.outputCount ?? 0} finished item(s)`
            : `${tools.length} tool${tools.length === 1 ? "" : "s"}`
        }
      />
      <div className="insight-list">
        {tools.length === 0 && (
          <div className="empty-state compact-empty">
            <strong>No AI tools detected</strong>
            <span>ChatGPT, Claude, Copilot, Cursor, Codex, Gemini, Aider, and Cline will appear here when captured.</span>
          </div>
        )}
        {tools.slice(0, 8).map((tool) => (
          <article className="insight-row" key={tool.tool}>
            <strong>{tool.tool}</strong>
            <span>{formatDuration(tool.durationMs)}</span>
            <em>{tool.contexts.slice(0, 2).join(", ") || `${tool.events} event(s)`}</em>
          </article>
        ))}
      </div>
    </section>
  );
}

function ProjectUsagePanel({ projects }: { projects: ProjectUsageBreakdown[] }) {
  const maxDuration = Math.max(...projects.map((project) => project.durationMs), 1);

  return (
    <section className="panel-block project-usage-panel">
      <PanelHeader
        eyebrow="Projects today"
        title={projects.length ? "Where your time went" : "No projects captured"}
        value={`${projects.length} place${projects.length === 1 ? "" : "s"}`}
      />
      <div className="app-usage-summary-list">
        {projects.length === 0 && (
          <div className="empty-state compact-empty">
            <strong>No project folders yet</strong>
            <span>Editor folders, terminal cwd, browser domains, and agent workspaces appear here when captured.</span>
          </div>
        )}
        {projects.slice(0, 6).map((project) => {
          const percent = Math.max(4, Math.round((project.durationMs / maxDuration) * 100));
          const appSummary = project.apps
            .slice(0, 3)
            .map((app) => `${app.app} ${formatDuration(app.durationMs)}`)
            .join(" · ");

          return (
            <article className="app-summary-row project-summary-row" key={project.key}>
              <div>
                <strong>{project.label}</strong>
                <em>{project.contexts[0] ?? "No folder or site detail"}</em>
              </div>
              <span>{formatDuration(project.durationMs)}</span>
              <div className="usage-bar-track">
                <div className="usage-bar-fill" style={{ width: `${percent}%` }} />
              </div>
              <small>{appSummary || `${project.events} event${project.events === 1 ? "" : "s"}`}</small>
              {project.aiTools.length ? (
                <div className="tool-chip-row app-summary-tools">
                  {project.aiTools.slice(0, 4).map((tool) => (
                    <span className="tool-chip" key={tool}>
                      {tool}
                    </span>
                  ))}
                </div>
              ) : null}
            </article>
          );
        })}
      </div>
    </section>
  );
}

function AppUsagePanel({ summary }: { summary?: BackendAppUsageSummary }) {
  const apps = summary?.apps ?? [];
  const maxDuration = Math.max(...apps.map((app) => app.durationMs), 1);

  return (
    <section className="panel-block app-usage-panel">
      <PanelHeader
        eyebrow="Top apps today"
        title={apps.length ? "Top apps today" : "No work apps yet"}
        value={formatDuration(summary?.totalDurationMs ?? 0)}
      />
      <div className="app-usage-summary-list">
          {apps.length === 0 && (
            <div className="empty-state compact-empty">
              <strong>No activity yet</strong>
              <span>App totals will populate as active windows are captured.</span>
            </div>
          )}
          {apps.slice(0, 5).map((app) => {
            const topProject = app.projects[0] ?? null;
            const projectSummary =
              app.projects.length > 1
                ? `${app.projects.length} places: ${app.projects.map((project) => project.label).slice(0, 2).join(", ")}`
                : topProject?.label ?? `${app.events} event(s)`;
            const toolBadges = app.aiTools.length ? app.aiTools : topProject?.aiTools ?? [];
            const percent = Math.max(4, Math.round((app.durationMs / maxDuration) * 100));

            return (
            <article className="app-summary-row" key={app.app}>
              <div>
                <strong>{app.app}</strong>
                <em>{projectSummary}</em>
              </div>
              <span>{formatDuration(app.durationMs)}</span>
              <div className="usage-bar-track">
                <div className="usage-bar-fill" style={{ width: `${percent}%` }} />
              </div>
              {toolBadges.length ? (
                <div className="tool-chip-row app-summary-tools">
                  {toolBadges.slice(0, 3).map((tool) => (
                    <span className="tool-chip" key={tool.tool}>
                      {tool.tool}
                    </span>
                  ))}
                </div>
              ) : null}
            </article>
            );
          })}
      </div>
    </section>
  );
}

function StatusMatrix({
  loopRisks,
}: {
  loopRisks: NonNullable<BackendTodaySnapshot["loopRisks"]>;
}) {
  const countByType = (type: string) =>
    loopRisks.filter((risk) => risk.riskType === type).length;
  const rows = [
    ["Unanswered messages", `${countByType("reply_debt")} open`, countByType("reply_debt") ? "warning" : "ok"],
    ["Unfinished AI work", `${countByType("ai_output_open")} open`, countByType("ai_output_open") ? "warning" : "ok"],
    ["Ghost Agent", `${countByType("ghost_agent")} stalled`, countByType("ghost_agent") ? "danger" : "ok"],
    ["Stale Hypothesis", `${countByType("stale_hypothesis")} flags`, countByType("stale_hypothesis") ? "danger" : "ok"],
  ];

  return (
    <div className="status-matrix">
      {rows.map(([label, value, state]) => (
        <div className="status-row" data-state={state} key={label}>
          <span>{label}</span>
          <strong>{value}</strong>
        </div>
      ))}
    </div>
  );
}

function AppsView({
  activeAppName,
  setActiveAppName,
  sourceEvents,
  summary,
}: {
  activeAppName: string | null;
  setActiveAppName: (appName: string | null) => void;
  sourceEvents: BackendSourceEvent[];
  summary?: BackendAppUsageSummary;
}) {
  const apps = summary?.apps ?? [];
  const selectedApp =
    apps.find((app) => app.app === activeAppName) ?? apps[0] ?? null;
  const [selectedProjectLabel, setSelectedProjectLabel] = useState<string | null>(null);
  const [activityTab, setActivityTab] = useState<"summary" | "timeline" | "context" | "ai">("summary");
  const selectedProject =
    selectedApp?.projects.find((project) => project.label === selectedProjectLabel) ??
    selectedApp?.projects[0] ??
    null;
  const selectedEvents = selectedProject
    ? sourceEventsForApp(sourceEvents, selectedApp?.app, selectedProject.label, selectedProject.contexts ?? [])
    : sourceEventsForApp(sourceEvents, selectedApp?.app);
  const appMeter =
    apps.length === 0
      ? "Waiting for capture"
      : `${apps.length} app${apps.length === 1 ? "" : "s"} · ${formatDuration(summary?.totalDurationMs ?? 0)}`;
  const selectedAiTools = uniqueValues([
    ...(selectedProject?.aiTools.map((tool) => tool.tool) ?? []),
    ...selectedEvents.flatMap(aiToolLabelsForEvent),
  ]);
  const selectedContexts = uniqueValues([
    ...(selectedProject?.contexts ?? []),
    ...selectedEvents.map(eventFullContextLabel),
  ]).filter((value) => value !== "Captured activity");
  const selectedContextLabels = uniqueValues(selectedEvents.map(eventContextLabel)).filter(
    (value) => value !== "Captured activity",
  );
  const selectedExamples = uniqueValues([
    ...(selectedProject?.examples ?? []),
    ...selectedEvents.map(eventTitle),
  ]).filter((value) => value !== "Captured activity");
  const selectedStart = selectedEvents.length
    ? Math.min(...selectedEvents.map((event) => event.startedAt))
    : null;
  const selectedEnd = selectedEvents.length
    ? Math.max(...selectedEvents.map((event) => event.endedAt))
    : null;
  const activePeriod =
    selectedStart && selectedEnd
      ? `${new Date(selectedStart).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} - ${new Date(selectedEnd).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
      : "No period";
  const extensions = selectedExamples.reduce((map, value) => {
    const match = value.match(/\.([a-zA-Z0-9]{1,8})(?:\b|$)/);
    if (match) {
      map.set(match[1].toLowerCase(), (map.get(match[1].toLowerCase()) ?? 0) + 1);
    }
    return map;
  }, new Map<string, number>());

  useEffect(() => {
    if (selectedApp && activeAppName !== selectedApp.app) {
      setActiveAppName(selectedApp.app);
    }
  }, [activeAppName, selectedApp, setActiveAppName]);

  useEffect(() => {
    if (!selectedApp?.projects.length) {
      setSelectedProjectLabel(null);
      return;
    }

    if (!selectedApp.projects.some((project) => project.label === selectedProjectLabel)) {
      setSelectedProjectLabel(selectedApp.projects[0].label);
    }
  }, [selectedApp, selectedProjectLabel]);

  return (
    <div className="view-frame apps-view">
      <div className="screen-titlebar">
        <div>
          <h2>Activity</h2>
          <p>Explore exactly what you worked on by app, project, folder, website, and AI tool.</p>
        </div>
        <div className="screen-actions">
          <span className="mini-meter">{appMeter}</span>
        </div>
      </div>

      <section className="activity-workbench" aria-label="App usage details">
        {apps.length === 0 && (
          <div className="empty-state empty-panel">
            <strong>No activity yet</strong>
            <span>DayTrail will show editors, terminals, browsers, and AI tools here as soon as active windows are captured.</span>
          </div>
        )}
        {apps.length > 0 && (
          <>
            <div className="activity-column app-detail-list" aria-label="Captured apps">
              <header className="activity-column-header">
                <strong>1. Choose an app</strong>
                <span>All apps used today</span>
              </header>
              {apps.map((app) => {
                const isSelected = selectedApp?.app === app.app;
                return (
                  <div className="activity-app-entry" key={app.app}>
                    <button
                      aria-pressed={isSelected}
                      className="app-detail-button"
                      onClick={() => {
                        setActiveAppName(app.app);
                        setSelectedProjectLabel(null);
                      }}
                      type="button"
                    >
                      <AppIcon appName={app.app} className="activity-app-icon" />
                      <span className="activity-app-label">
                        <strong>{app.app}</strong>
                        <em>
                          {app.aiTools.length
                            ? `AI: ${app.aiTools.map((tool) => tool.tool).slice(0, 3).join(", ")}`
                            : `${app.projects.length} place${app.projects.length === 1 ? "" : "s"}`}
                        </em>
                      </span>
                      <span className="activity-app-duration">{formatDuration(app.durationMs)}</span>
                    </button>
                    {isSelected && app.files.length > 0 && (
                      <ul className="activity-file-list" aria-label={`Files in ${app.app}`}>
                        {app.files.map((file) => (
                          <li className="activity-file-row" key={`${file.name}-${file.context ?? ""}`}>
                            <span className="activity-file-name">
                              {file.name}
                              {file.context && <em> — {file.context}</em>}
                            </span>
                            <span className="activity-file-duration">{formatDuration(file.durationMs)}</span>
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                );
              })}
            </div>

            <div className="activity-column project-drilldown" aria-label="Folders, sites, and windows">
              <header className="activity-column-header">
                <strong>2. Select project / workspace</strong>
                <span>Places you worked in {selectedApp?.app ?? "this app"}</span>
              </header>
                  {selectedApp?.projects.length === 0 && (
                    <div className="empty-state compact-empty">
                      <strong>No context detail yet</strong>
                      <span>This app has captured time, but no folder, tab, or site detail was available.</span>
                    </div>
                  )}
                  {selectedApp?.projects.map((project) => (
                    <button
                      aria-pressed={selectedProject?.label === project.label}
                      className="project-detail-button"
                      key={project.label}
                      onClick={() => setSelectedProjectLabel(project.label)}
                      type="button"
                    >
                      <strong>{project.label}</strong>
                      <span>{formatDuration(project.durationMs)} · {project.events} event{project.events === 1 ? "" : "s"}</span>
                      {project.aiTools.length > 0 && (
                        <em>{project.aiTools.map((tool) => tool.tool).slice(0, 3).join(", ")}</em>
                      )}
                    </button>
                  ))}
            </div>

            <section className="panel-block activity-detail-pane" aria-label="Activity details">
              <header className="activity-detail-header">
                <div>
                  <span>3. Activity details</span>
                  <h2>{selectedApp?.app ?? "Select an app"}{selectedProject ? ` · ${selectedProject.label}` : ""}</h2>
                </div>
                <em>{selectedProject ? formatDuration(selectedProject.durationMs) : "No selection"}</em>
              </header>

              <div className="activity-tabs" role="tablist" aria-label="Activity sections">
                {[
                  ["summary", "Summary"],
                  ["timeline", "Timeline"],
                  ["context", "Files / context"],
                  ["ai", "AI usage"],
                ].map(([id, label]) => (
                  <button
                    aria-selected={activityTab === id}
                    key={id}
                    onClick={() => setActivityTab(id as typeof activityTab)}
                    role="tab"
                    type="button"
                  >
                    {label}
                  </button>
                ))}
              </div>

              {(activityTab === "summary" || activityTab === "timeline" || activityTab === "context" || activityTab === "ai") && (
                <section className="activity-summary-cards" aria-label="Selected activity summary">
                  <div>
                    <span>Time spent</span>
                    <strong>{formatDuration(selectedProject?.durationMs ?? 0)}</strong>
                    <em>{activePeriod}</em>
                  </div>
                  <div>
                    <span>Events</span>
                    <strong>{selectedProject?.events ?? selectedEvents.length}</strong>
                    <em>{selectedEvents.length} source records</em>
                  </div>
                  <div>
                    <span>Context items</span>
                    <strong>{selectedContexts.length || selectedExamples.length}</strong>
                    <em>Folders, sites, files, or windows</em>
                  </div>
                  <div>
                    <span>AI tools</span>
                    <strong>{selectedAiTools.length}</strong>
                    <em>{selectedAiTools.join(", ") || "No AI detected"}</em>
                  </div>
                  <div>
                    <span>Types</span>
                    <strong>{extensions.size || "-"}</strong>
                    <em>{[...extensions.keys()].slice(0, 4).join(", ") || "No file extensions"}</em>
                  </div>
                </section>
              )}

              {activityTab === "summary" && (
                <section className="activity-context-callout">
                  <strong>Most active period: {activePeriod}</strong>
                  <span>{selectedContexts[0] ?? selectedProject?.label ?? "No detailed context captured for this selection."}</span>
                </section>
              )}

              <div className="activity-detail-grid" data-tab={activityTab}>
                {(activityTab === "summary" || activityTab === "timeline") && (
                  <section className="activity-events-panel">
                    <div className="section-label">Key events</div>
                    {selectedEvents.length === 0 && (
                      <div className="empty-state compact-empty">
                        <strong>No detailed activity for this selection</strong>
                        <span>Select another app or folder, or keep capture running for more detail.</span>
                      </div>
                    )}
                    {selectedEvents.slice(0, activityTab === "timeline" ? 80 : 30).map((event) => {
                      const eventAiTools = aiToolLabelsForEvent(event);
                      const domainCat = classifyDomain(event.domain);
                      const gitCtx = (() => {
                        if (!event.metadataJson) return null;
                        try {
                          const meta = JSON.parse(event.metadataJson) as { gitContext?: { branch?: string; ticketId?: string } };
                          return meta.gitContext ?? null;
                        } catch { return null; }
                      })();

                      return (
                        <article className="event-detail-row" key={event.id}>
                          <span className="event-app">{eventAppLabel(event)}</span>
                          <strong>{eventTitle(event)}</strong>
                          <em>{eventSubtitle(event) || eventContextLabel(event)}</em>
                          <small>{formatDuration(event.durationMs)}</small>
                          {(eventAiTools.length > 0 || domainCat || gitCtx?.branch) && (
                            <span className="event-ai-chip-row">
                              {eventAiTools.map((tool) => (
                                <span className="event-ai-chip" key={tool}>{tool}</span>
                              ))}
                              {domainCat && eventAiTools.length === 0 && (
                                <span
                                  className="event-category-chip"
                                  style={{ background: DOMAIN_CATEGORY_COLORS[domainCat] }}
                                >
                                  {DOMAIN_CATEGORY_LABELS[domainCat]}
                                </span>
                              )}
                              {gitCtx?.ticketId && (
                                <span className="event-git-chip event-git-chip--ticket">{gitCtx.ticketId}</span>
                              )}
                              {gitCtx?.branch && !gitCtx.ticketId && (
                                <span className="event-git-chip">{gitCtx.branch}</span>
                              )}
                            </span>
                          )}
                        </article>
                      );
                    })}
                  </section>
                )}

                {(activityTab === "summary" || activityTab === "context") && (
                  <aside className="activity-context-panel">
                    <div className="section-label">Context</div>
                    <dl>
                      <div>
                        <dt>App</dt>
                        <dd>{selectedApp?.app ?? "-"}</dd>
                      </div>
                      <div>
                        <dt>Workspace</dt>
                        <dd>{selectedContextLabels[0] ?? selectedProject?.label ?? "-"}</dd>
                      </div>
                      <div>
                        <dt>AI tools</dt>
                        <dd>{selectedAiTools.join(", ") || "No AI detected"}</dd>
                      </div>
                      <div>
                        <dt>Source records</dt>
                        <dd>{selectedEvents.length}</dd>
                      </div>
                    </dl>
                    <div className="activity-context-list">
                      {selectedContexts.slice(0, activityTab === "context" ? 30 : 8).map((context) => (
                        <span key={context}>{context}</span>
                      ))}
                      {selectedExamples.slice(0, activityTab === "context" ? 30 : 8).map((example) => (
                        <span key={example}>{example}</span>
                      ))}
                      {selectedContexts.length === 0 && selectedExamples.length === 0 && (
                        <span>No folder, file, URL, or window title captured.</span>
                      )}
                    </div>
                  </aside>
                )}

                {activityTab === "ai" && (
                  <section className="activity-events-panel full-span">
                    <div className="section-label">AI usage in this selection</div>
                    {selectedAiTools.length === 0 && (
                      <div className="empty-state compact-empty">
                        <strong>No AI usage detected here</strong>
                        <span>Copilot, Codex, Claude Code, Gemini, ChatGPT, Cursor, Aider, Cline, and terminal agents appear when source-backed signals are captured.</span>
                      </div>
                    )}
                    {selectedAiTools.map((tool) => {
                      const toolDuration =
                        selectedProject?.aiTools.find((item) => item.tool === tool)?.durationMs ??
                        selectedEvents
                          .filter((event) => aiToolLabelsForEvent(event).includes(tool))
                          .reduce((total, event) => total + event.durationMs, 0);

                      return (
                        <article className="event-detail-row" key={tool}>
                          <span className="event-app">AI</span>
                          <strong>{tool}</strong>
                          <em>{selectedProject?.label ?? selectedApp?.app ?? "Captured activity"}</em>
                          <small>{formatDuration(toolDuration)}</small>
                        </article>
                      );
                    })}
                  </section>
                )}
              </div>
            </section>
          </>
        )}
      </section>
    </div>
  );
}

function SimpleActivityView({
  settings,
  snapshot,
}: {
  settings?: BackendSettings;
  snapshot: BackendTodaySnapshot | null;
}) {
  const view = useMemo(() => buildActivityView(snapshot, settings), [settings, snapshot]);
  const [activeTab, setActiveTab] = useState<"Sessions" | "Apps" | "Projects" | "Raw Activity">("Sessions");
  const [openSessionId, setOpenSessionId] = useState<string | null>(null);
  const projects = useMemo(() => {
    const buckets = new Map<string, { label: string; durationMs: number; apps: Set<string>; aiTools: Set<string> }>();
    view.apps.forEach((app) => {
      (app.projects ?? []).forEach((project) => {
        const bucket = buckets.get(project.label) ?? {
          label: project.label,
          durationMs: 0,
          apps: new Set<string>(),
          aiTools: new Set<string>(),
        };
        bucket.durationMs += project.durationMs ?? 0;
        bucket.apps.add(app.app);
        (project.aiTools ?? []).forEach((tool) => bucket.aiTools.add(tool.tool));
        buckets.set(project.label, bucket);
      });
    });
    return [...buckets.values()].sort((left, right) => right.durationMs - left.durationMs);
  }, [view.apps]);

  return (
    <div className="view-frame apps-view">
      <div className="screen-titlebar">
        <div>
          <h2>Activity</h2>
          <p>Sessions first, with app and project breakdowns when enough activity is captured.</p>
        </div>
        <div className="screen-actions">
          <span className="mini-meter">{view.lowDataMessage ? "Getting started" : `${view.sessions.length} session${view.sessions.length === 1 ? "" : "s"}`}</span>
        </div>
      </div>

      {view.lowDataMessage && (
        <section className="panel-block early-capture-panel">
          <PanelHeader eyebrow="Activity" title="More work needed for a useful breakdown" value="Simple Mode" />
          <p>{view.lowDataMessage}</p>
        </section>
      )}

      <div className="activity-tabs" role="tablist" aria-label="Activity sections">
        {view.tabs.map((tab) => (
          <button
            aria-selected={activeTab === tab}
            key={tab}
            onClick={() => setActiveTab(tab as typeof activeTab)}
            role="tab"
            type="button"
          >
            {tab}
          </button>
        ))}
      </div>

      {activeTab === "Sessions" && (
        <section className="panel-block">
          <PanelHeader eyebrow="Sessions" title="Work sessions today" value={`${view.sessions.length}`} />
          <div className="session-list">
            {view.sessions.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No sessions yet</strong>
                <span>Work sessions appear after DayTrail captures enough app and window activity.</span>
              </div>
            )}
            {view.sessions.map((session) => {
              const sessionKey = session.id ?? `${session.timeRangeLabel}-${session.title ?? session.startedAt ?? ""}`;
              return (
              <article className="session-card" key={sessionKey}>
                <div className="session-card-main">
                  <span className="session-card-time">{session.timeRangeLabel}</span>
                  <strong>{session.title ?? "Work session"}</strong>
                  <p>
                    {session.projects.length > 0
                      ? session.projects.map((project) => project.label).join(" · ")
                      : session.summary ?? "Activity details available"}
                  </p>
                  <div className="session-card-chips" aria-label="Session details">
                    {session.mainApps.map((app) => (
                      <span key={app.label}>{app.label} · {app.durationLabel}</span>
                    ))}
                    {session.aiTools.map((tool) => (
                      <span key={tool}>AI: {tool}</span>
                    ))}
                    {session.qualityWarnings.slice(0, 2).map((warning) => (
                      <span className="warning-chip" key={warning}>{warning}</span>
                    ))}
                  </div>
                </div>
                <div className="session-card-meta">
                  <span>{session.durationLabel}</span>
                  <em>{session.qualityLabel}</em>
                  <button
                    className="session-open-button"
                    onClick={() => setOpenSessionId((current) => (current === sessionKey ? null : sessionKey))}
                    type="button"
                  >
                    {openSessionId === sessionKey ? "Close details" : "Open session"}
                  </button>
                </div>
                {openSessionId === sessionKey && (
                  <div className="session-inline-detail">
                    <div>
                      <span>When</span>
                      <strong>{session.timeRangeLabel}</strong>
                    </div>
                    <div>
                      <span>Main apps</span>
                      <strong>
                        {session.mainApps.length
                          ? session.mainApps.map((app) => `${app.label} ${app.durationLabel}`).join(" · ")
                          : "App breakdown unavailable"}
                      </strong>
                    </div>
                    <div>
                      <span>Context</span>
                      <strong>
                        {session.projects.length
                          ? session.projects.map((project) => project.label).join(" · ")
                          : session.summary ?? "No project context yet"}
                      </strong>
                    </div>
                    <div>
                      <span>AI tools detected</span>
                      <strong>{session.aiTools.length ? session.aiTools.join(", ") : "None detected"}</strong>
                    </div>
                    <div>
                      <span>Review status</span>
                      <strong>
                        {session.qualityWarnings.length ? session.qualityWarnings.join(" · ") : "Enough context for Simple Mode"}
                      </strong>
                    </div>
                    <div>
                      <span>Activity items</span>
                      <strong>{session.eventCount}</strong>
                    </div>
                  </div>
                )}
              </article>
              );
            })}
          </div>
        </section>
      )}

      {activeTab === "Apps" && (
        <section className="panel-block">
          <PanelHeader eyebrow="Apps" title="Top apps today" value={`${view.apps.length}`} />
          <div className="ai-app-list">
            {view.apps.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No work apps yet</strong>
                <span>System and utility apps stay hidden in Simple Mode.</span>
              </div>
            )}
            {view.apps.map((app) => (
              <article className="ai-app-row" key={app.app}>
                <strong>{app.app}</strong>
                <span>{app.durationLabel}</span>
                <em>{app.projects?.map((project) => project.label).slice(0, 2).join(", ") || "Activity details"}</em>
                {(app.aiTools?.length ?? 0) > 0 && (
                  <div className="tool-chip-row">
                    {app.aiTools?.slice(0, 4).map((tool) => (
                      <span className="tool-chip" key={`${app.app}-${tool.tool}`}>{tool.tool}</span>
                    ))}
                  </div>
                )}
              </article>
            ))}
          </div>
        </section>
      )}

      {activeTab === "Projects" && (
        <section className="panel-block">
          <PanelHeader eyebrow="Projects" title="Folders, sites, and work areas" value={`${projects.length}`} />
          <div className="project-usage-list">
            {projects.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No project detail yet</strong>
                <span>Projects appear when DayTrail sees folder, site, or workspace context.</span>
              </div>
            )}
            {projects.map((project) => (
              <article className="project-usage-row" key={project.label}>
                <strong>{project.label}</strong>
                <span>{formatDuration(project.durationMs)}</span>
                <em>{[...project.apps].slice(0, 3).join(", ")}</em>
                {[...project.aiTools].length > 0 && (
                  <div className="tool-chip-row">
                    {[...project.aiTools].slice(0, 4).map((tool) => (
                      <span className="tool-chip" key={tool}>{tool}</span>
                    ))}
                  </div>
                )}
              </article>
            ))}
          </div>
        </section>
      )}

      {activeTab === "Raw Activity" && view.showTechnicalDetails && (
        <section className="panel-block">
          <PanelHeader eyebrow="Technical details" title="Activity details" value={`${view.technicalItems.length}`} />
          <div className="hour-event-list">
            {view.technicalItems.map((event) => (
              <article className="hour-event-row" key={event.id}>
                <span>{event.app ?? event.source}</span>
                <strong>{event.title ?? "Activity detail"}</strong>
                <em>{event.workspaceKey ?? event.domain ?? ""}</em>
                <b>{formatDuration(event.durationMs ?? 0)}</b>
              </article>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function LoopsView({
  items,
  onLoopAction,
}: {
  items: BackendUnclosedLoopItem[];
  onLoopAction: (itemId: string, action: "closed" | "snoozed" | "ignored") => void;
}) {
  const [selectedItemId, setSelectedItemId] = useState<string | null>(null);
  const [reviewFilter, setReviewFilter] = useState<"all" | "high" | "medium" | "low" | "reviewed">("all");
  const riskRank = { high: 0, medium: 1, low: 2 } as Record<string, number>;
  const normalizedItems = items
    .map((item) => ({
      ...item,
      risk: item.risk?.toLowerCase() || "low",
    }))
    .sort(
      (left, right) =>
        (riskRank[left.risk] ?? 3) - (riskRank[right.risk] ?? 3) ||
        left.title.localeCompare(right.title),
    );
  const selectedItem =
    normalizedItems.find((item) => item.id === selectedItemId) ?? normalizedItems[0] ?? null;
  const counts = {
    high: normalizedItems.filter((item) => item.risk === "high").length,
    medium: normalizedItems.filter((item) => item.risk === "medium").length,
    low: normalizedItems.filter((item) => item.risk === "low").length,
    reviewed: normalizedItems.filter((item) => item.status !== "open").length,
  };
  const visibleItems = normalizedItems.filter((item) => {
    if (reviewFilter === "all") {
      return true;
    }
    if (reviewFilter === "reviewed") {
      return item.status !== "open";
    }
    return item.risk === reviewFilter;
  });
  const groups = [
    { key: "high", label: "High priority", items: visibleItems.filter((item) => item.risk === "high") },
    { key: "medium", label: "Medium priority", items: visibleItems.filter((item) => item.risk === "medium") },
    { key: "low", label: "Low priority", items: visibleItems.filter((item) => item.risk === "low") },
  ];
  const formatLoopLabel = (value: string) =>
    value
      .replace(/[_-]+/g, " ")
      .replace(/\b\w/g, (letter) => letter.toUpperCase());

  useEffect(() => {
    if (!normalizedItems.length) {
      if (selectedItemId) {
        setSelectedItemId(null);
      }
      return;
    }

    if (!normalizedItems.some((item) => item.id === selectedItemId)) {
      setSelectedItemId(normalizedItems[0].id);
    }
  }, [normalizedItems, selectedItemId]);

  return (
    <div className="view-frame loops-view">
      <div className="screen-titlebar">
        <div>
          <h2>Needs Review</h2>
          <p>Review source-backed replies, promises, idle gaps, AI drafts, meeting actions, and agent failures.</p>
        </div>
        <div className="screen-actions">
          <button
            className="button compact"
            onClick={() => setReviewFilter((current) => {
              const order = ["all", "high", "medium", "low", "reviewed"] as const;
              return order[(order.indexOf(current) + 1) % order.length];
            })}
            type="button"
          >
            <Icon name="sliders" />
            {reviewFilter === "all" ? "All items" : `Filter: ${formatLoopLabel(reviewFilter)}`}
          </button>
          <button
            className="button compact primary"
            disabled={normalizedItems.length === 0}
            onClick={() => normalizedItems.forEach((item) => onLoopAction(item.id, "closed"))}
            type="button"
          >
            <Icon name="check" />
            Mark all done
          </button>
        </div>
      </div>

      <section className="review-summary-grid" aria-label="Review queue risk summary">
        <button
          aria-pressed={reviewFilter === "all"}
          className="review-summary-card"
          onClick={() => setReviewFilter("all")}
          type="button"
        >
          <span>All items</span>
          <strong>{normalizedItems.length}</strong>
        </button>
        <button
          aria-pressed={reviewFilter === "high"}
          className="review-summary-card danger"
          onClick={() => setReviewFilter("high")}
          type="button"
        >
          <span>High priority</span>
          <strong>{counts.high}</strong>
        </button>
        <button
          aria-pressed={reviewFilter === "medium"}
          className="review-summary-card warning"
          onClick={() => setReviewFilter("medium")}
          type="button"
        >
          <span>Medium priority</span>
          <strong>{counts.medium}</strong>
        </button>
        <button
          aria-pressed={reviewFilter === "low"}
          className="review-summary-card success"
          onClick={() => setReviewFilter("low")}
          type="button"
        >
          <span>Low priority</span>
          <strong>{counts.low}</strong>
        </button>
        <button
          aria-pressed={reviewFilter === "reviewed"}
          className="review-summary-card"
          onClick={() => setReviewFilter("reviewed")}
          type="button"
        >
          <span>Reviewed</span>
          <strong>{counts.reviewed}</strong>
        </button>
      </section>

      <section className="review-layout" aria-label="Needs review queue">
        <div className="review-list">
          {visibleItems.length === 0 && (
            <div className="panel-block review-empty-panel">
              <div className="empty-state empty-panel">
                <strong>{normalizedItems.length === 0 ? "Nothing needs review" : "No items match this filter"}</strong>
                <span>Unanswered messages, promises, idle gaps, drafted AI work, meeting actions, and agent failures appear only when DayTrail has source evidence.</span>
              </div>
            </div>
          )}
          {groups.map((group) =>
            group.items.length ? (
              <section className="review-group" key={group.key}>
                <header>
                  <span data-risk={group.key} />
                  <h3>{group.label}</h3>
                  <em>{group.items.length}</em>
                </header>
                <div className="review-row-list">
                  {group.items.map((item) => (
                    <button
                      aria-pressed={selectedItem?.id === item.id}
                      className="review-row"
                      data-risk={item.risk}
                      key={item.id}
                      onClick={() => setSelectedItemId(item.id)}
                      type="button"
                    >
                      <span className="review-source">{formatLoopLabel(item.source || item.category)}</span>
                      <strong>{item.title}</strong>
                      <em>{item.detail}</em>
                      <small>{formatLoopLabel(item.category)}</small>
                      <b>{formatLoopLabel(item.risk)}</b>
                    </button>
                  ))}
                </div>
              </section>
            ) : null,
          )}
        </div>

        <aside className="panel-block review-detail-panel">
          <PanelHeader
            eyebrow={selectedItem ? formatLoopLabel(selectedItem.source) : "Review detail"}
            title={selectedItem?.title ?? "Select an item"}
            value={selectedItem ? formatLoopLabel(selectedItem.risk) : ""}
          />
          {selectedItem ? (
            <div className="review-detail-body">
              <p>{selectedItem.detail}</p>
              <dl>
                <div>
                  <dt>Category</dt>
                  <dd>{formatLoopLabel(selectedItem.category)}</dd>
                </div>
                <div>
                  <dt>Source</dt>
                  <dd>{selectedItem.source}</dd>
                </div>
                <div>
                  <dt>Evidence</dt>
                  <dd>{selectedItem.evidenceIds.length} source record{selectedItem.evidenceIds.length === 1 ? "" : "s"}</dd>
                </div>
                <div>
                  <dt>Status</dt>
                  <dd>{formatLoopLabel(selectedItem.status)}</dd>
                </div>
              </dl>
              <section className="review-reason-card">
                <strong>Why this needs review</strong>
                <span>{selectedItem.detail}</span>
              </section>
              <div className="review-actions">
                <button
                  className="button compact primary"
                  onClick={() => onLoopAction(selectedItem.id, "closed")}
                  type="button"
                >
                  <Icon name="check" />
                  {selectedItem.primaryAction || "I've done this"}
                </button>
                <button
                  className="button compact"
                  onClick={() => onLoopAction(selectedItem.id, "snoozed")}
                  type="button"
                >
                  Snooze
                </button>
                <button
                  className="button compact"
                  onClick={() => onLoopAction(selectedItem.id, "ignored")}
                  type="button"
                >
                  <Icon name="x" />
                  Ignore
                </button>
              </div>
            </div>
          ) : (
            <div className="empty-state compact-empty">
              <strong>No item selected</strong>
              <span>Review items will appear here when detected from captured activity.</span>
            </div>
          )}
        </aside>
      </section>
    </div>
  );
}

function AiLedgerView({
  appSummary,
  ledger,
  summary,
  sourceEvents,
}: {
  appSummary?: BackendAppUsageSummary;
  ledger: BackendAiOutputLedgerItem[];
  summary?: BackendAiUsageSummary;
  sourceEvents: BackendSourceEvent[];
}) {
  const tools = summary?.tools ?? [];
  const appsWithAi =
    appSummary?.apps.filter((app) => app.aiTools.length > 0 || app.projects.some((project) => project.aiTools.length > 0)) ?? [];
  const aiEvents = sourceEvents.filter((event) => aiToolLabelsForEvent(event).length > 0);
  const interactionCount = tools.reduce((sum, tool) => sum + tool.events, 0);
  const completedOutputs = ledger.filter((item) =>
    ["sent", "shared", "completed", "accepted", "done"].includes(item.status.toLowerCase()),
  ).length;
  const agentSessions = aiEvents.filter((event) => {
    const haystack = `${event.eventType} ${event.app ?? ""} ${event.title ?? ""} ${event.metadataJson ?? ""}`.toLowerCase();
    return haystack.includes("agent") || haystack.includes("codex") || haystack.includes("claude code") || haystack.includes("gemini");
  }).length;
  const toolMax = Math.max(...tools.map((tool) => tool.durationMs), 1);
  const hourlyAi = Array.from({ length: 24 }, (_, hour) => ({
    hour,
    label: localHourLabel(hour),
    totalMs: 0,
    tools: new Map<string, number>(),
  }));

  aiEvents.forEach((event) => {
    const hour = new Date(event.startedAt).getHours();
    const bucket = hourlyAi[hour];
    const detectedTools = aiToolLabelsForEvent(event);
    detectedTools.forEach((tool) => {
      bucket.tools.set(tool, (bucket.tools.get(tool) ?? 0) + event.durationMs);
      bucket.totalMs += event.durationMs;
    });
  });

  const maxHourDuration = Math.max(...hourlyAi.map((hour) => hour.totalMs), 1);
  const impactRows = [
    ["AI source events", aiEvents.length, "Events where an AI tool was detected"],
    ["Linked outputs", ledger.length, "Drafted, shared, completed, or reviewed outputs"],
    ["Completed outputs", completedOutputs, "Outputs with a completed/shared/sent status"],
    ["Agent-like sessions", agentSessions, "Codex, Claude Code, Gemini, or agent-labeled events"],
  ];
  const recentInteractions =
    ledger.length > 0
      ? ledger.slice(0, 6).map((item) => ({
          id: item.id,
          tool: item.tool,
          title: item.title,
          context: item.sourceContext,
          status: item.status,
          time: formatDuration(item.durationMs),
        }))
      : aiEvents.slice(0, 6).map((event) => ({
          id: event.id,
          tool: aiToolLabelsForEvent(event).join(", "),
          title: eventTitle(event),
          context: eventContextLabel(event),
          status: "captured",
          time: formatDuration(event.durationMs),
        }));

  return (
    <div className="view-frame ai-ledger-view">
      <div className="screen-titlebar">
        <div>
          <h2>AI Impact</h2>
          <p>Understand which AI tools were used, where they were used, and which outputs were actually tracked.</p>
        </div>
        <div className="screen-actions">
          <span className="mini-meter">All times in your local timezone</span>
        </div>
      </div>

      <section className="ai-kpi-grid" aria-label="AI usage metrics">
        <div className="ai-kpi-card">
          <span>AI time today</span>
          <strong>{formatDuration(summary?.totalDurationMs ?? 0)}</strong>
          <em>{aiEvents.length} source event{aiEvents.length === 1 ? "" : "s"}</em>
        </div>
        <div className="ai-kpi-card">
          <span>AI tools used</span>
          <strong>{tools.length}</strong>
          <em>{tools.map((tool) => tool.tool).slice(0, 4).join(", ") || "No tools detected"}</em>
        </div>
        <div className="ai-kpi-card">
          <span>AI interactions</span>
          <strong>{interactionCount}</strong>
          <em>Captured completions, chats, commands, or window events</em>
        </div>
        <div className="ai-kpi-card">
          <span>Accepted / completed</span>
          <strong>{completedOutputs}</strong>
          <em>Source-backed output ledger entries</em>
        </div>
        <div className="ai-kpi-card">
          <span>Apps with AI</span>
          <strong>{appsWithAi.length}</strong>
          <em>{appsWithAi.map((app) => app.app).slice(0, 3).join(", ") || "No app linked yet"}</em>
        </div>
        <div className="ai-kpi-card">
          <span>Agent sessions</span>
          <strong>{agentSessions}</strong>
          <em>Codex, Claude Code, Gemini, or agent-like signals</em>
        </div>
      </section>

      <div className="ai-impact-grid">
        <section className="panel-block ai-tools-panel">
          <PanelHeader eyebrow="AI time by tool" title="Time spent using each AI tool" value={`${tools.length} tool${tools.length === 1 ? "" : "s"}`} />
          <div className="ai-tool-bars">
            {tools.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No AI usage captured</strong>
                <span>AI usage from ChatGPT, Claude, Gemini, Copilot, Cursor, Codex, Aider, and Cline will appear here.</span>
              </div>
            )}
            {tools.map((tool) => (
              <article className="ai-tool-bar-row" key={tool.tool}>
                <strong>{tool.tool}</strong>
                <div>
                  <i style={{ width: `${Math.max(4, Math.round((tool.durationMs / toolMax) * 100))}%` }} />
                </div>
                <span>{formatDuration(tool.durationMs)}</span>
                <em>{tool.contexts.join(", ") || `${tool.events} event(s)`}</em>
              </article>
            ))}
          </div>
        </section>

        <section className="panel-block ai-day-panel">
          <PanelHeader eyebrow="AI time over the day" title="When you used AI tools" value={formatDuration(summary?.totalDurationMs ?? 0)} />
          <div className="ai-hour-chart" aria-label="AI usage by hour">
            {hourlyAi.map((hour) => (
              <div className="ai-hour-column" key={hour.hour} title={`${hour.label}: ${formatDuration(hour.totalMs)}`}>
                <span>
                  {[...hour.tools.entries()].map(([tool, duration]) => (
                    <i
                      key={tool}
                      style={{
                        background: appColor(tool),
                        height: `${Math.max(3, Math.round((duration / maxHourDuration) * 100))}%`,
                      }}
                    />
                  ))}
                </span>
                <em>{hour.hour % 2 === 0 ? hour.label : ""}</em>
              </div>
            ))}
          </div>
          <div className="hour-legend compact-legend">
            {tools.slice(0, 6).map((tool) => (
              <span key={tool.tool}>
                <i style={{ background: appColor(tool.tool) }} />
                {tool.tool}
              </span>
            ))}
          </div>
        </section>
      </div>

      <div className="ai-lower-grid">
        <section className="panel-block ai-app-panel">
          <PanelHeader eyebrow="AI usage by app" title="Where AI tools appeared" value={`${appsWithAi.length} app${appsWithAi.length === 1 ? "" : "s"}`} />
          <div className="ai-app-list">
            {appsWithAi.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No app-linked AI yet</strong>
                <span>When AI tools are detected inside editors, terminals, browsers, or standalone apps, the app breakdown appears here.</span>
              </div>
            )}
            {appsWithAi.slice(0, 8).map((app) => (
              <article className="ai-app-row" key={app.app}>
                <strong>{app.app}</strong>
                <span>{formatDuration(app.durationMs)}</span>
                <em>{app.projects.map((project) => project.label).slice(0, 2).join(", ") || `${app.events} event(s)`}</em>
                <div className="tool-chip-row">
                  {(app.aiTools.length ? app.aiTools : app.projects.flatMap((project) => project.aiTools))
                    .slice(0, 4)
                    .map((tool) => (
                      <span className="tool-chip" key={`${app.app}-${tool.tool}`}>{tool.tool}</span>
                    ))}
                </div>
              </article>
            ))}
          </div>
        </section>

        <section className="panel-block ai-impact-summary-panel">
          <PanelHeader eyebrow="AI impact summary" title="What AI helped you accomplish" value="Source-backed" />
          <div className="ai-impact-list">
            {impactRows.map(([label, value, detail]) => (
              <article className="ai-impact-row" key={label}>
                <strong>{label}</strong>
                <span>{value}</span>
                <em>{detail}</em>
              </article>
            ))}
          </div>
        </section>

        <section className="panel-block ai-recent-panel">
          <PanelHeader eyebrow="Recent AI interactions" title="Latest detected AI work" value={`${recentInteractions.length} shown`} />
          <div className="ai-recent-list">
            {recentInteractions.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No AI interactions yet</strong>
                <span>Recent AI events and output ledger entries appear here once captured.</span>
              </div>
            )}
            {recentInteractions.map((item) => (
              <article className="ai-recent-row" key={item.id}>
                <span>{item.tool || "AI"}</span>
                <strong>{item.title}</strong>
                <em>{item.context}</em>
                <small data-status={item.status.toLowerCase()}>{item.status} · {item.time}</small>
              </article>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function SimpleAiImpactView({
  settings,
  snapshot,
}: {
  settings?: BackendSettings;
  snapshot: BackendTodaySnapshot | null;
}) {
  const view = useMemo(() => buildAiImpactView(snapshot, settings), [settings, snapshot]);
  const tools = view.toolSummaries;

  return (
    <div className="view-frame ai-ledger-view">
      <div className="screen-titlebar">
        <div>
          <h2>AI Impact</h2>
          <p>Lightweight AI context from detected tools, sessions, apps, and active hours.</p>
        </div>
        <div className="screen-actions">
          <span className="mini-meter">{view.confidenceLabel}</span>
        </div>
      </div>

      {view.lowDataMessage && (
        <section className="panel-block early-capture-panel">
          <PanelHeader eyebrow="AI tools detected" title="Not enough activity for a useful breakdown" value="Simple Mode" />
          <p>{view.lowDataMessage}</p>
        </section>
      )}

      {tools.length === 0 ? (
        <section className="panel-block">
          <div className="empty-state empty-panel">
            <strong>No AI activity detected yet</strong>
            <span>DayTrail will show AI tools here when it detects ChatGPT, Codex, Copilot, Gemini, Claude, Cursor, or other AI-assisted work.</span>
          </div>
        </section>
      ) : (
        <>
          <section className="ai-kpi-grid" aria-label="AI impact summary">
            <div className="ai-kpi-card">
              <span>AI tools detected</span>
              <strong>{tools.length}</strong>
              <em>{tools.map((tool) => tool.tool).slice(0, 4).join(", ")}</em>
            </div>
            <div className="ai-kpi-card">
              <span>Used mostly with</span>
              <strong>{view.usedMostlyWith}</strong>
              <em>Session or app context</em>
            </div>
            <div className="ai-kpi-card">
              <span>Most active tool</span>
              <strong>{view.mostActiveTool ?? "AI"}</strong>
              <em>{view.confidenceLabel}</em>
            </div>
            <div className="ai-kpi-card">
              <span>Evidence status</span>
              <strong>{view.evidenceStatus}</strong>
              <em>
                {view.evidenceCounts.linkedOutputs > 0
                  ? `${view.evidenceCounts.linkedOutputs} linked output${view.evidenceCounts.linkedOutputs === 1 ? "" : "s"}`
                  : "No accepted/generated claim without evidence"}
              </em>
            </div>
          </section>

          <section className="panel-block ai-tools-panel">
            <PanelHeader eyebrow="Tool summary" title="AI tools detected today" value={`${tools.length}`} />
            <div className="ai-tool-bars">
              {tools.map((tool) => (
                <article className="ai-tool-bar-row" key={tool.tool}>
                  <strong>{tool.tool}</strong>
                  <div>
                    <i style={{ width: `${Math.max(4, Math.min(100, Math.round((tool.durationMs / Math.max(snapshot?.aiUsageSummary?.totalDurationMs ?? 1, 1)) * 100)))}%` }} />
                  </div>
                  <span>{tool.durationLabel}</span>
                  <em>{tool.label}</em>
                </article>
              ))}
            </div>
          </section>
        </>
      )}
    </div>
  );
}

const AI_THINKING_MESSAGES = [
  "Reading your chaos…",
  "Judging your 47 open tabs…",
  "Counting every ⌘+Tab…",
  "Translating meetings into billable time…",
  "Finding deep meaning in loginwindow…",
  "Untangling your focus sessions…",
  "Converting procrastination into insights…",
  "Consulting the tab oracle…",
  "Weighing your VS Code existentialism…",
  "Normalising your browser rabbit holes…",
];

// ─── Review / Timesheet ──────────────────────────────────────────────────────

interface ReviewViewProps {
  sessions: BackendWorkSessionSummary[];
  loading: boolean;
  fromDate: string;
  toDate: string;
  onFromDate: (v: string) => void;
  onToDate: (v: string) => void;
  onLoad: () => void;
  onUpdate: (id: string, patch: Partial<BackendWorkSessionSummary>) => Promise<void>;
  onExport: () => void;
  exporting: boolean;
}

function formatReviewDuration(ms: number): string {
  const s = Math.round(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

function formatReviewTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function ReviewSessionCard({
  session,
  onUpdate,
}: {
  session: BackendWorkSessionSummary;
  onUpdate: (patch: Partial<BackendWorkSessionSummary>) => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [draftClient, setDraftClient] = useState(session.clientLabel ?? "");
  const [draftProject, setDraftProject] = useState(session.projectLabel ?? "");
  const [draftTicket, setDraftTicket] = useState(session.ticketId ?? "");
  const [draftNotes, setDraftNotes] = useState(session.reviewNotes ?? "");
  const [saving, setSaving] = useState(false);

  const statusColors: Record<string, string> = {
    draft: "var(--text-muted)",
    confirmed: "var(--success)",
    excluded: "var(--danger)",
  };
  const statusBg: Record<string, string> = {
    draft: "var(--bg-control)",
    confirmed: "var(--success-bg)",
    excluded: "var(--danger-bg)",
  };

  async function cycleStatus() {
    const next = session.billingStatus === "draft"
      ? "confirmed"
      : session.billingStatus === "confirmed"
      ? "excluded"
      : "draft";
    await onUpdate({ billingStatus: next });
  }

  async function saveEdits() {
    setSaving(true);
    await onUpdate({
      clientLabel: draftClient.trim() || null,
      projectLabel: draftProject.trim() || null,
      ticketId: draftTicket.trim() || null,
      reviewNotes: draftNotes.trim() || null,
    });
    setSaving(false);
    setEditing(false);
  }

  const excluded = session.billingStatus === "excluded";

  return (
    <div
      className="review-session-card"
      style={{ opacity: excluded ? 0.45 : 1 }}
    >
      <div className="review-card-left">
        <span className="review-card-time">
          {formatReviewTime(session.startedAt)} – {formatReviewTime(session.endedAt)}
        </span>
        <span className="review-card-duration">{formatReviewDuration(session.durationMs)}</span>
        {session.aiUsed && (
          <span className="review-card-ai-badge">AI</span>
        )}
      </div>

      <div className="review-card-body">
        <strong className="review-card-title">{session.title}</strong>
        {session.summary && (
          <p className="review-card-summary">{session.summary}</p>
        )}
        <div className="review-card-meta">
          {(session.clientLabel || session.projectLabel) && (
            <span className="review-meta-chip review-meta-client">
              {session.clientLabel ?? session.projectLabel}
            </span>
          )}
          {session.ticketId && (
            <span className="review-meta-chip review-meta-ticket">#{session.ticketId}</span>
          )}
          {session.reviewNotes && (
            <span className="review-meta-chip review-meta-note">{session.reviewNotes}</span>
          )}
        </div>

        {editing && (
          <div className="review-edit-form">
            <div className="review-edit-row">
              <label>
                Client
                <input
                  type="text"
                  value={draftClient}
                  placeholder="e.g. Acme Corp"
                  onChange={(e) => setDraftClient(e.target.value)}
                />
              </label>
              <label>
                Project
                <input
                  type="text"
                  value={draftProject}
                  placeholder="e.g. Website redesign"
                  onChange={(e) => setDraftProject(e.target.value)}
                />
              </label>
              <label>
                Ticket
                <input
                  type="text"
                  value={draftTicket}
                  placeholder="e.g. PROJ-123"
                  onChange={(e) => setDraftTicket(e.target.value)}
                />
              </label>
            </div>
            <label className="review-edit-notes">
              Notes
              <input
                type="text"
                value={draftNotes}
                placeholder="Optional note for timesheet"
                onChange={(e) => setDraftNotes(e.target.value)}
              />
            </label>
            <div className="review-edit-actions">
              <button
                type="button"
                className="review-btn review-btn--primary"
                disabled={saving}
                onClick={saveEdits}
              >
                {saving ? "Saving…" : "Save"}
              </button>
              <button
                type="button"
                className="review-btn"
                onClick={() => setEditing(false)}
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="review-card-actions">
        <button
          type="button"
          className="review-billable-toggle"
          title={session.billable ? "Mark non-billable" : "Mark billable"}
          onClick={() => onUpdate({ billable: !session.billable })}
          aria-pressed={session.billable}
        >
          {session.billable ? "💰" : "–"}
        </button>
        <button
          type="button"
          className="review-status-btn"
          style={{
            color: statusColors[session.billingStatus] ?? "var(--text-muted)",
            background: statusBg[session.billingStatus] ?? "var(--bg-control)",
          }}
          onClick={cycleStatus}
          title="Cycle: draft → confirmed → excluded"
        >
          {session.billingStatus === "confirmed" ? "✓ Confirmed"
            : session.billingStatus === "excluded" ? "✗ Excluded"
            : "Draft"}
        </button>
        <button
          type="button"
          className="review-edit-btn"
          title="Edit client / project / ticket"
          onClick={() => setEditing(!editing)}
        >
          Edit
        </button>
      </div>
    </div>
  );
}

function ReviewView({
  sessions,
  loading,
  fromDate,
  toDate,
  onFromDate,
  onToDate,
  onLoad,
  onUpdate,
  onExport,
  exporting,
}: ReviewViewProps) {
  const billableMs = sessions
    .filter((s) => s.billingStatus === "confirmed" && s.billable)
    .reduce((sum, s) => sum + s.durationMs, 0);
  const confirmedCount = sessions.filter((s) => s.billingStatus === "confirmed").length;
  const excludedCount = sessions.filter((s) => s.billingStatus === "excluded").length;
  const draftCount = sessions.filter((s) => s.billingStatus === "draft").length;

  return (
    <div className="review-view">
      <div className="review-toolbar">
        <div className="review-dates">
          <label>
            From
            <input
              type="date"
              value={fromDate}
              onChange={(e) => onFromDate(e.target.value)}
            />
          </label>
          <label>
            To
            <input
              type="date"
              value={toDate}
              onChange={(e) => onToDate(e.target.value)}
            />
          </label>
          <button
            type="button"
            className="review-btn review-btn--primary"
            onClick={onLoad}
            disabled={loading}
          >
            {loading ? "Loading…" : "Load sessions"}
          </button>
        </div>
        <button
          type="button"
          className="review-btn review-btn--export"
          onClick={onExport}
          disabled={exporting || sessions.length === 0}
          title="Export confirmed sessions as Markdown"
        >
          <Icon name="save" />
          {exporting ? "Exporting…" : "Export timesheet"}
        </button>
      </div>

      {sessions.length > 0 && (
        <div className="review-stats-bar">
          <span className="review-stat">
            <em>{sessions.length}</em> sessions
          </span>
          <span className="review-stat review-stat--draft">
            <em>{draftCount}</em> draft
          </span>
          <span className="review-stat review-stat--confirmed">
            <em>{confirmedCount}</em> confirmed
          </span>
          {excludedCount > 0 && (
            <span className="review-stat review-stat--excluded">
              <em>{excludedCount}</em> excluded
            </span>
          )}
          <span className="review-stat review-stat--billable">
            <em>{formatReviewDuration(billableMs)}</em> billable
          </span>
        </div>
      )}

      {loading && (
        <div className="review-loading">Loading sessions…</div>
      )}

      {!loading && sessions.length === 0 && (
        <div className="review-empty">
          <p>No sessions found for this date range.</p>
          <p className="review-empty-hint">Choose a date range and press "Load sessions".</p>
        </div>
      )}

      <div className="review-session-list">
        {sessions.map((s) => (
          <ReviewSessionCard
            key={s.id}
            session={s}
            onUpdate={(patch) => onUpdate(s.id, patch)}
          />
        ))}
      </div>
    </div>
  );
}

function NeedsReviewSimpleView({
  settings,
  snapshot,
}: {
  settings?: BackendSettings;
  snapshot: BackendTodaySnapshot | null;
}) {
  const view = useMemo(() => buildReviewView(snapshot, settings), [settings, snapshot]);

  return (
    <div className="view-frame loops-view">
      <div className="screen-titlebar">
        <div>
          <h2>Needs Review</h2>
          <p>Review unclear sessions, idle gaps, draft timesheet sessions, and unfinished report inputs.</p>
        </div>
        <div className="screen-actions">
          <span className="mini-meter">{view.items.length} item{view.items.length === 1 ? "" : "s"}</span>
        </div>
      </div>

      {view.lowDataMessage && (
        <section className="panel-block early-capture-panel">
          <PanelHeader eyebrow="Needs Review" title="Review queue will grow with more activity" value="Simple Mode" />
          <p>{view.lowDataMessage}</p>
        </section>
      )}

      <section className="panel-block review-list">
        {view.items.length === 0 ? (
          <div className="empty-state empty-panel">
            <strong>{view.emptyTitle}</strong>
            <span>{view.emptyCopy}</span>
          </div>
        ) : (
          <div className="review-row-list">
            {view.items.map((item) => (
              <article className="review-row" data-risk={item.priority} key={item.id}>
                <span className="review-source">{item.priority}</span>
                <strong>{item.title}</strong>
                <em>{item.detail}</em>
              </article>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

// ─── AI Loading ──────────────────────────────────────────────────────────────

function AiThinkingLoader() {
  const [msgIdx, setMsgIdx] = useState(0);

  useEffect(() => {
    const id = setInterval(() => setMsgIdx((i) => (i + 1) % AI_THINKING_MESSAGES.length), 2800);
    return () => clearInterval(id);
  }, []);

  return (
    <div className="ai-thinking-loader" aria-label="AI analysis in progress">
      <svg className="ai-thinking-svg" viewBox="0 0 160 160" aria-hidden="true">
        {/* Pulsing outward rings */}
        <circle className="ai-ring ai-ring-1" cx="80" cy="80" r="22" />
        <circle className="ai-ring ai-ring-2" cx="80" cy="80" r="40" />
        <circle className="ai-ring ai-ring-3" cx="80" cy="80" r="58" />
        {/* Connection lines from center to orbit nodes */}
        <line className="ai-spoke" x1="80" y1="80" x2="80" y2="52" />
        <line className="ai-spoke" x1="80" y1="80" x2="80" y2="38" />
        <line className="ai-spoke" x1="80" y1="80" x2="80" y2="108" />
        <line className="ai-spoke ai-spoke-delay" x1="80" y1="80" x2="80" y2="22" />
        <line className="ai-spoke ai-spoke-delay" x1="80" y1="80" x2="130" y2="109" />
        <line className="ai-spoke ai-spoke-delay" x1="80" y1="80" x2="30" y2="109" />
        {/* Orbit group 1: 1 dot, fast, inner ring (r=28) */}
        <g className="ai-orbit ai-orbit-1">
          <circle cx="80" cy="52" r="5" className="ai-orb" />
        </g>
        {/* Orbit group 2: 2 dots, medium, mid ring (r=42) */}
        <g className="ai-orbit ai-orbit-2">
          <circle cx="80" cy="38" r="4" className="ai-orb ai-orb-2" />
          <circle cx="80" cy="122" r="4" className="ai-orb ai-orb-2" />
        </g>
        {/* Orbit group 3: 3 dots, slow, outer ring (r=58) */}
        <g className="ai-orbit ai-orbit-3">
          <circle cx="80" cy="22" r="3.5" className="ai-orb ai-orb-3" />
          <circle cx="130" cy="109" r="3.5" className="ai-orb ai-orb-3" />
          <circle cx="30" cy="109" r="3.5" className="ai-orb ai-orb-3" />
        </g>
        {/* Core node */}
        <circle className="ai-core" cx="80" cy="80" r="10" />
        <circle className="ai-core-inner" cx="80" cy="80" r="5" />
      </svg>
      <p className="ai-thinking-message" key={msgIdx}>{AI_THINKING_MESSAGES[msgIdx]}</p>
    </div>
  );
}

function AutomationView({
  aiProvider,
  candidates,
  exportFromDate,
  exportPreview,
  exportStatus,
  exportToDate,
  isAnalyzing,
  onAnalyze,
  onExport,
  setExportFromDate,
  setExportToDate,
}: {
  aiProvider: string;
  candidates: BackendAutomationCandidate[];
  exportFromDate: string;
  exportPreview: string;
  exportStatus: string;
  exportToDate: string;
  isAnalyzing: boolean;
  onAnalyze: () => void;
  onExport: () => void;
  setExportFromDate: (value: string) => void;
  setExportToDate: (value: string) => void;
}) {
  const hasRoutines = candidates.length > 0;

  return (
    <div className="view-frame automation-view">
      <div className="view-intro">
        <div>
          <h2>Export data</h2>
          <p>Download activity data for a date range, or ask your configured AI to find repeated work.</p>
        </div>
        <span className="mini-meter">{hasRoutines ? `${candidates.length} routine${candidates.length === 1 ? "" : "s"}` : "Ready"}</span>
      </div>

      <div className={hasRoutines ? "automation-grid" : "automation-grid single-panel"}>
        {hasRoutines && (
        <section className="panel-block">
          <PanelHeader eyebrow="Routine analysis" title="Repeated work" value="Today" />
          <div className="automation-list">
            {candidates.map((candidate) => (
              <article className="automation-card" key={candidate.id}>
                <span>{candidate.signal}</span>
                <strong>{candidate.title}</strong>
                <em>{candidate.occurrences}x · {formatDuration(candidate.durationMs)} · {candidate.exampleSources.join(", ")}</em>
                <p>{candidate.reason}</p>
                {(candidate.suggestedSteps ?? []).map((step) => (
                  <small key={step}>{step}</small>
                ))}
                {(candidate.relatedAiTools ?? []).length > 0 && (
                  <div className="tool-chip-row">
                    {(candidate.relatedAiTools ?? []).map((tool) => (
                      <span className="tool-chip" key={tool}>{tool}</span>
                    ))}
                  </div>
                )}
              </article>
            ))}
          </div>
        </section>
        )}

        <section className="panel-block export-panel">
          <PanelHeader eyebrow="Export" title="Activity data and AI analysis" value={exportStatus} />
          <div className="export-controls">
            <label htmlFor="export-from">From</label>
            <input
              id="export-from"
              onChange={(event) => setExportFromDate(event.target.value)}
              type="date"
              value={exportFromDate}
            />
            <label htmlFor="export-to">To</label>
            <input
              id="export-to"
              onChange={(event) => setExportToDate(event.target.value)}
              type="date"
              value={exportToDate}
            />
            <button className="button compact" onClick={onExport} type="button">
              <Icon name="archive" />
              Preview export
            </button>
            <button className="button compact primary" disabled={isAnalyzing} onClick={onAnalyze} type="button">
              <Icon name="ritual" />
              {isAnalyzing ? "Analyzing…" : `Analyze with ${aiProvider}`}
            </button>
          </div>
          {isAnalyzing ? (
            <AiThinkingLoader />
          ) : (
            <textarea
              aria-label="Activity export or AI analysis"
              className="export-preview"
              readOnly
              value={exportPreview || "Preview the selected date range or run AI analysis."}
            />
          )}
        </section>
      </div>
    </div>
  );
}

function RestoreView({
  addNote,
  aiThreads,
  notes,
  onResume,
  quickNote,
  selectedStream,
  setQuickNote,
}: {
  addNote: (event: FormEvent<HTMLFormElement>) => void;
  aiThreads: AiThread[];
  notes: Note[];
  onResume: () => void;
  quickNote: string;
  selectedStream: Stream;
  setQuickNote: (value: string) => void;
}) {
  return (
    <div className="view-frame restore-grid">
      <section className="panel-block restore-marker">
        <PanelHeader
          eyebrow="Return-to-work marker"
          title={selectedStream.id === "empty" ? "No return marker yet" : selectedStream.title}
          value={selectedStream.id === "empty" ? "Waiting" : "Resume ready"}
        />
        <div className="marker-copy">
          <p>{selectedStream.summary}</p>
          <button
            className="button primary compact"
            disabled={selectedStream.id === "empty"}
            onClick={onResume}
            type="button"
          >
            <Icon name="return" />
            Resume context
          </button>
        </div>
      </section>

      <section className="panel-block clue-ledger">
        <PanelHeader
          eyebrow="Related clues"
          title="Terminal, git, browser"
          value={`${selectedStream.events.length} anchors`}
        />
        <div className="clue-list">
          {selectedStream.events.length === 0 && (
            <div className="empty-state compact-empty">
              <strong>No related clues</strong>
              <span>Editor snapshots, browser events, terminal bridge data, and AI threads will appear after capture.</span>
            </div>
          )}
          {selectedStream.events.map((item) => (
            <article className="clue-row" key={item.id}>
              <span>[-]</span>
              {item.title}
            </article>
          ))}
        </div>
      </section>

      <section className="panel-block ai-thread-panel">
        <PanelHeader eyebrow="Scattered AI finder" title="Related AI threads" value={`${aiThreads.length} found`} />
        <div className="thread-list">
          {aiThreads.length === 0 && (
            <div className="empty-state compact-empty">
              <strong>No AI threads linked</strong>
              <span>ChatGPT, Claude, Cursor, Copilot, Codex, Aider, and Cline usage is linked when detected.</span>
            </div>
          )}
          {aiThreads.map((thread) => (
            <article className="thread-row" key={thread.id}>
              <span>{thread.tool}</span>
              <strong>{thread.title}</strong>
              <p>{thread.clue}</p>
            </article>
          ))}
        </div>
      </section>

      <section className="panel-block scratchpad-panel">
        <PanelHeader eyebrow="Saved notes" title={selectedStream.title} value="Attached to this work" />
        <form className="stack-form" onSubmit={addNote}>
          <label htmlFor="restore-note">Quick bullet</label>
          <textarea
            id="restore-note"
            onChange={(event) => setQuickNote(event.target.value)}
            placeholder="Add what to verify when you resume..."
            value={quickNote}
          />
          <button className="button compact" type="submit">
            <Icon name="plus" />
            Add note
          </button>
        </form>
        <div className="note-stack">
          {notes.map((note) => (
            <article className="compact-note" key={note.id}>
              <span>{compactDisplayLabel(note.context)}</span>
              <p>{note.text}</p>
            </article>
          ))}
          {notes.length === 0 && (
            <div className="empty-state compact-empty">
              <strong>No scratchpad notes</strong>
              <span>Add a note to pin the next step to this context.</span>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}

function RitualsView({
  onGenerateReport,
  onOpenExports,
  onRegenerateContext,
  reportMarkdown,
  settings,
  sourceFeed,
  snapshot,
}: {
  onGenerateReport: () => void;
  onOpenExports: () => void;
  onRegenerateContext: () => void;
  reportMarkdown: string;
  settings?: BackendSettings;
  sourceFeed: SourceFeedItem[];
  snapshot: BackendTodaySnapshot | null;
}) {
  const [copyStatus, setCopyStatus] = useState("Copy markdown");
  const [reportSection, setReportSection] = useState<"summary" | "timeline" | "ai" | "raw">("summary");
  const reportView = useMemo(
    () => buildReportView(snapshot, settings, reportMarkdown),
    [reportMarkdown, settings, snapshot],
  );
  const reportSettings = normalizeExperienceSettings(settings);
  if (reportSettings.experienceMode === "simple") {
    const simpleReportMarkdown = reportView.markdown;

    return (
      <div className="view-frame rituals-view">
        <div className="screen-titlebar">
          <div>
            <h2>Reports</h2>
            <p>Generate a readable daily summary from sessions, apps, AI tools, and review items.</p>
          </div>
          <div className="screen-actions">
            <button className="button compact primary" onClick={onGenerateReport} type="button">
              <Icon name="plus" />
              Generate
            </button>
          </div>
        </div>

        {reportView.lowDataMessage && (
          <section className="panel-block early-capture-panel">
            <PanelHeader eyebrow="Reports" title="Reports need at least one work session" value="Simple Mode" />
            <p>{reportView.lowDataMessage}</p>
          </section>
        )}

        <div className="reports-workspace">
          <section className="panel-block report-input-panel">
            <PanelHeader eyebrow="1. Included work" title="What will be summarized" value={`${reportView.includedWork.sessions} session${reportView.includedWork.sessions === 1 ? "" : "s"}`} />
            <div className="report-settings-list">
              <div><span>Work sessions</span><strong>{reportView.includedWork.sessions}</strong></div>
              <div><span>Apps used</span><strong>{reportView.includedWork.apps}</strong></div>
              <div><span>AI tools detected</span><strong>{reportView.includedWork.aiTools}</strong></div>
              <div><span>Needs review</span><strong>{(snapshot?.unclosedLoopInbox?.length ?? 0) + (snapshot?.idleBlocks?.filter((block) => !block.classified).length ?? 0)}</strong></div>
            </div>
            <div className="report-input-actions">
              <button className="button compact" onClick={onRegenerateContext} type="button">
                <Icon name="sync" />
                Refresh included work
              </button>
            </div>
          </section>

          <section className="panel-block report-output-panel">
            <PanelHeader eyebrow="2. Daily report" title="Daily Work Report" value="Markdown" />
            <pre className="report-preview" aria-label="Generated report markdown">
              {simpleReportMarkdown || "Generate today’s report after DayTrail captures a work session."}
            </pre>
            <div className="output-actions">
              <button className="button compact primary" onClick={onGenerateReport} type="button">
                <Icon name="ritual" />
                Generate
              </button>
              <button
                className="button compact"
                disabled={!simpleReportMarkdown}
                onClick={async () => {
                  await writeClipboardText(simpleReportMarkdown);
                  setCopyStatus("Copied");
                  window.setTimeout(() => setCopyStatus("Copy markdown"), 1600);
                }}
                type="button"
              >
                <Icon name="copy" />
                {copyStatus}
              </button>
              <button
                className="button compact"
                disabled={!simpleReportMarkdown}
                onClick={() => downloadTextFile("daytrail-daily-report.md", simpleReportMarkdown, "text/markdown")}
                type="button"
              >
                <Icon name="archive" />
                Export Markdown
              </button>
            </div>
          </section>
        </div>
      </div>
    );
  }
  const markdownTitle = "Daily Work Report";
  const sourceSummary = sourceFeed.length
    ? sourceFeed.map((item) => `- ${item.label}`).join("\n")
    : "No source inputs captured yet.";
  const aiSourceSummary = sourceFeed.filter((item) => item.label.toLowerCase().includes("ai"));
  const reportContent =
    reportSection === "summary"
      ? reportMarkdown || "No generated report yet. Generate a report to create a source-backed summary from captured data."
      : reportSection === "timeline"
        ? sourceSummary
        : reportSection === "ai"
          ? (aiSourceSummary.length ? aiSourceSummary.map((item) => `- ${item.label}`).join("\n") : "No AI-specific report inputs captured yet.")
          : JSON.stringify(sourceFeed, null, 2);

  return (
    <div className="view-frame rituals-view">
      <div className="screen-titlebar">
        <div>
          <h2>Reports</h2>
          <p>Generate source-backed summaries from captured work, app usage, AI usage, and review items.</p>
        </div>
        <div className="screen-actions">
          <button className="button compact" onClick={onOpenExports} type="button">
            <Icon name="archive" />
            Raw export
          </button>
          <button className="button compact primary" onClick={onGenerateReport} type="button">
            <Icon name="plus" />
            Generate
          </button>
        </div>
      </div>

      <div className="report-type-tabs" aria-label="Report sections">
        {[
          ["summary", "Summary"],
          ["timeline", "Timeline"],
          ["ai", "AI usage"],
          ["raw", "Raw facts"],
        ].map(([id, label]) => (
          <button
            aria-pressed={reportSection === id}
            key={id}
            onClick={() => setReportSection(id as typeof reportSection)}
            type="button"
          >
            <Icon name={id === "raw" ? "archive" : id === "ai" ? "ritual" : "copy"} />
            {label}
          </button>
        ))}
      </div>

      <div className="reports-workspace">
        <section className="panel-block report-input-panel">
          <PanelHeader eyebrow="1. Report inputs" title="Review available data" value={`${sourceFeed.length} item(s)`} />
          <div className="report-input-list">
            {sourceFeed.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No verified inputs</strong>
                <span>Reports use captured sessions, app records, AI usage, notes, reply debt, and commitments when available.</span>
              </div>
            )}
            {sourceFeed.map((item) => (
              <article className="report-input-row" data-selected={item.selected} key={item.id}>
                <span className="source-indicator" />
                <div>
                  <strong>{item.label}</strong>
                  <em>{item.selected ? "Used by report generator" : "Available source fact"}</em>
                </div>
              </article>
            ))}
          </div>
          <div className="report-input-actions">
            <button className="button compact" onClick={onRegenerateContext} type="button">
              <Icon name="sync" />
              Refresh inputs
            </button>
          </div>
        </section>

        <section className="panel-block report-output-panel">
          <PanelHeader eyebrow="2. Generated report" title={markdownTitle} value="Markdown" />
          <pre className="report-preview" aria-label="Generated report markdown">{reportContent}</pre>
          <div className="output-actions">
            <button className="button compact primary" onClick={onGenerateReport} type="button">
              <Icon name="ritual" />
              Regenerate
            </button>
            <button
              className="button compact"
              disabled={!reportMarkdown}
              onClick={async () => {
                await writeClipboardText(reportMarkdown);
                setCopyStatus("Copied");
                window.setTimeout(() => setCopyStatus("Copy markdown"), 1600);
              }}
              type="button"
            >
              <Icon name="copy" />
              {copyStatus}
            </button>
          </div>
        </section>

        <aside className="panel-block report-export-panel">
          <PanelHeader eyebrow="3. Export & share" title="Use this report" value="Local" />
          <div className="report-export-actions">
            <button
              className="button compact"
              disabled={!reportMarkdown}
              onClick={async () => {
                await writeClipboardText(reportMarkdown);
                setCopyStatus("Copied");
                window.setTimeout(() => setCopyStatus("Copy markdown"), 1600);
              }}
              type="button"
            >
              <Icon name="copy" />
              Copy Markdown
            </button>
            <button
              className="button compact"
              disabled={!reportMarkdown}
              onClick={() => downloadTextFile("daytrail-daily-report.md", reportMarkdown, "text/markdown")}
              type="button"
            >
              <Icon name="archive" />
              Export Markdown
            </button>
            <button className="button compact" onClick={onOpenExports} type="button">
              <Icon name="archive" />
              Export Raw JSON
            </button>
          </div>
          <div className="report-settings-list">
            <div><span>Screenshots</span><strong>Off</strong></div>
            <div><span>Full URLs</span><strong>Off</strong></div>
            <div><span>Idle time</span><strong>Manual</strong></div>
            <div><span>AI details</span><strong>On</strong></div>
          </div>
        </aside>
      </div>
    </div>
  );
}

function MemoryView({
  contextPack,
  facts,
  onDeleteFact,
  snapshot,
}: {
  contextPack: Record<string, string | number | string[]>;
  facts: MemoryFact[];
  onDeleteFact: (fact: MemoryFact) => void;
  snapshot: BackendTodaySnapshot | null;
}) {
  const [copyStatus, setCopyStatus] = useState("Copy report briefing");
  const [selectedFactId, setSelectedFactId] = useState<string | null>(null);
  const selectedFact = facts.find((fact) => fact.id === selectedFactId) ?? facts[0] ?? null;
  const contextPackText = JSON.stringify(contextPack, null, 2);

  return (
    <div className="view-frame memory-view">
      <section className="kpi-strip" aria-label="Project memory metrics">
        <Metric label="Saved items" value={`${facts.length}`} />
        <Metric label="Unanswered messages" value={`${snapshot?.pendingReplies.length ?? 0}`} />
        <Metric label="Open promises" value={`${snapshot?.commitments.length ?? 0}`} />
        <Metric label="AI-assisted work" value={`${snapshot?.aiOutputs.length ?? 0}`} />
      </section>

      <div className="memory-grid">
        <section className="panel-block decision-ledger">
          <PanelHeader
            eyebrow="Saved items"
            title="Notes and commitments"
            value="Used in reports"
          />
          <div className="decision-table">
            {facts.length === 0 && (
              <div className="empty-state compact-empty">
                <strong>No saved items yet</strong>
                <span>Scratchpad notes, promises, AI-assisted work, meetings, and field visits will appear after capture.</span>
              </div>
            )}
            {facts.map((fact) => (
              <button
                aria-pressed={selectedFact?.id === fact.id}
                className="decision-row"
                key={fact.id}
                onClick={() => setSelectedFactId(fact.id)}
                type="button"
              >
                <span>{fact.date}</span>
                <strong>{fact.title}</strong>
                <em>{fact.source}</em>
              </button>
            ))}
          </div>
        </section>

        <section className="panel-block memory-detail-panel">
          <PanelHeader
            eyebrow="Saved item"
            title={selectedFact?.title ?? "Select a memory"}
            value={selectedFact?.date ?? ""}
          />
          {selectedFact ? (
            <div className="memory-detail-body">
              <dl>
                <div>
                  <dt>Type</dt>
                  <dd>{memoryFactKindLabel(selectedFact.kind)}</dd>
                </div>
                <div>
                  <dt>Source</dt>
                  <dd>{selectedFact.source}</dd>
                </div>
                <div>
                  <dt>Captured</dt>
                  <dd>{selectedFact.date}</dd>
                </div>
              </dl>
              <div className="memory-detail-actions">
                <button
                  className="button compact"
                  onClick={async () => {
                    await writeClipboardText(contextPackText);
                    setCopyStatus("Copied");
                    window.setTimeout(() => setCopyStatus("Copy report briefing"), 1600);
                  }}
                  type="button"
                >
                  <Icon name="copy" />
                  {copyStatus}
                </button>
                {selectedFact.kind === "quickNote" && (
                  <button
                    className="button compact danger-button"
                    onClick={() => onDeleteFact(selectedFact)}
                    type="button"
                  >
                    <Icon name="x" />
                    Delete note
                  </button>
                )}
              </div>
              <details className="context-pack-details">
                <summary>Report briefing preview</summary>
                <div className="terminal-output">
                  {Object.entries(contextPack).map(([key, value]) => (
                    <div key={key}>
                      <span>{contextPackDisplayLabel(key)}</span>
                      <code>{Array.isArray(value) ? value.join(", ") || "none" : value}</code>
                    </div>
                  ))}
                </div>
              </details>
            </div>
          ) : (
            <div className="empty-state compact-empty">
              <strong>No saved item selected</strong>
              <span>Saved notes and source-backed facts will appear here.</span>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function SettingsView({
  aiConfig,
  captureHealth,
  databaseRestorePath,
  excludedDomainCount,
  folders,
  launchAtLogin,
  onBackupDatabase,
  onExportSettingsConfig,
  onImportSettingsConfig,
  onInstallTerminalBridge,
  onLoadStorageInfo,
  onOpenCapturePermission,
  onOpenExports,
  onOpenSavedNotes,
  onRefreshCapturePermissions,
  onRestartApp,
  onRestoreDatabase,
  onSaveExperienceSettings,
  onTriggerBrowserAutomation,
  permissionStatus,
  permissionSummary,
  saveAiConfig,
  saveState,
  selectedCount,
  setAiConfig,
  setDatabaseRestorePath,
  setSettingsConfigJson,
  setSaveState,
  setLaunchAtLogin,
  settings,
  settingsConfigJson,
  storageInfo,
  storageStatus,
  terminalBridgeStatus,
  toggleFolder,
}: {
  aiConfig: AiConfig;
  captureHealth?: BackendCaptureHealthSummary;
  databaseRestorePath: string;
  excludedDomainCount: number;
  folders: WorkspaceFolder[];
  launchAtLogin: boolean;
  onBackupDatabase: () => void;
  onExportSettingsConfig: () => void;
  onImportSettingsConfig: () => void;
  onInstallTerminalBridge: () => void;
  onLoadStorageInfo: () => void;
  onOpenCapturePermission: (permissionId: string) => void;
  onOpenExports: () => void;
  onOpenSavedNotes: () => void;
  onRefreshCapturePermissions: () => void;
  onRestartApp: () => void;
  onRestoreDatabase: () => void;
  onSaveExperienceSettings: (patch: Partial<BackendSettings>) => Promise<void>;
  onTriggerBrowserAutomation: () => void;
  permissionStatus: string;
  permissionSummary: BackendCapturePermissionSummary | null;
  saveAiConfig: (event: FormEvent<HTMLFormElement>) => void;
  saveState: string;
  selectedCount: number;
  setAiConfig: (config: AiConfig) => void;
  setDatabaseRestorePath: (value: string) => void;
  setSettingsConfigJson: (value: string) => void;
  setSaveState: (value: string) => void;
  setLaunchAtLogin: (value: boolean) => void;
  settings?: BackendSettings;
  settingsConfigJson: string;
  storageInfo: BackendStorageLocationInfo | null;
  storageStatus: string;
  terminalBridgeStatus: string;
  toggleFolder: (folderId: string) => void;
}) {
  const [activeSettings, setActiveSettings] = useState<
    "mode" | "capture" | "ai" | "privacy" | "integrations" | "storage" | "shortcuts" | "advanced" | "about"
  >("mode");
  const settingsView = useMemo(() => buildSettingsView(settings), [settings]);
  const settingSections: Array<{
    id: typeof activeSettings;
    label: string;
    detail: string;
    icon: IconName;
  }> = [
    { id: "mode", label: "Mode", detail: "Simple or Pro experience", icon: "layout" },
    { id: "capture", label: "Capture Health", detail: "Permissions and capture status", icon: "sync" },
    { id: "privacy", label: "Privacy", detail: "What is stored and analyzed", icon: "warning" },
    { id: "ai", label: "AI Provider", detail: "Analysis model and routing", icon: "ritual" },
    { id: "storage", label: "Data Storage", detail: "Local data and exports", icon: "archive" },
    { id: "advanced", label: "Advanced", detail: "Storage, exports, bridges, and endpoint", icon: "sliders" },
  ];
  const checkForSource = (source: "desktop" | "browser" | "editor" | "terminal" | "ai") => {
    const checks = captureHealth?.checks ?? [];
    if (source === "desktop") {
      const permissions = checks.find((item) => item.id === "os-permissions");
      if (permissions && permissions.status !== "ok") {
        return permissions;
      }
      return checks.find((item) => item.id === "active-window") ?? permissions;
    }
    const idBySource = {
      browser: "browser-bridge",
      editor: "editor-bridge",
      terminal: "terminal-bridge",
      ai: "ai-tools",
    } satisfies Record<Exclude<typeof source, "desktop">, string>;
    return checks.find((item) => item.id === idBySource[source]);
  };
  const checkStatus = (source: "desktop" | "browser" | "editor" | "terminal" | "ai") => {
    const check = checkForSource(source);
    return check ? check.status.replace("_", " ") : "waiting";
  };
  const checkState = (source: "desktop" | "browser" | "editor" | "terminal" | "ai") => {
    return checkForSource(source)?.status ?? "waiting";
  };

  return (
    <div className="view-frame settings-view">
      <div className="screen-titlebar">
        <div>
          <h2>Settings</h2>
          <p>Configure mode, capture health, privacy, AI provider, and advanced data controls.</p>
        </div>
        <div className="screen-actions">
          <span className="capture-pill">{captureHealth?.status?.replace("_", " ") ?? "Waiting"}</span>
          <button className="button compact" onClick={onOpenExports} type="button">
            <Icon name="archive" />
            Open exports
          </button>
        </div>
      </div>

      <div className="settings-pro-shell">
        <aside className="settings-pro-nav" aria-label="Settings sections">
          {settingSections.map((section) => (
            <button
              aria-pressed={activeSettings === section.id}
              key={section.id}
              onClick={() => {
                setActiveSettings(section.id);
                if (section.id === "storage") {
                  onLoadStorageInfo();
                }
              }}
              type="button"
            >
              <Icon name={section.icon} />
              <span>
                <strong>{section.label}</strong>
                <em>{section.detail}</em>
              </span>
              <Icon name="arrow" />
            </button>
          ))}
          <div className="settings-help-card">
            <strong>Need help?</strong>
            <span>DayTrail stores metadata locally and keeps screenshots and clipboard text off by default.</span>
          </div>
        </aside>

        <section className="settings-pro-content">
          {activeSettings === "mode" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>Experience mode</span>
                  <h2>Choose how much detail DayTrail shows</h2>
                </div>
                <strong>{settingsView.mode === "pro" ? "Pro Mode" : "Simple Mode"}</strong>
              </div>
              <div className="settings-card-grid">
                <button
                  aria-pressed={settingsView.mode === "simple"}
                  className="settings-action-row"
                  onClick={() =>
                    onSaveExperienceSettings({
                      experienceMode: "simple",
                      showSystemApps: false,
                      showRawEvents: false,
                      showCaptureConfidence: false,
                      showAiDetails: "summary",
                    })
                  }
                  type="button"
                >
                  <Icon name="layout" />
                  <span>
                    <strong>Simple Mode</strong>
                    <em>Timeline, sessions, reports, and review items without technical detail.</em>
                  </span>
                  <Icon name="check" />
                </button>
                <button
                  aria-pressed={settingsView.mode === "pro"}
                  className="settings-action-row"
                  onClick={() =>
                    onSaveExperienceSettings({
                      experienceMode: "pro",
                      showRawEvents: true,
                      showCaptureConfidence: true,
                      showAiDetails: "detailed",
                    })
                  }
                  type="button"
                >
                  <Icon name="sliders" />
                  <span>
                    <strong>Pro Mode</strong>
                    <em>Detailed activity rows, confidence, technical exports, and evidence inspection.</em>
                  </span>
                  <Icon name="arrow" />
                </button>
              </div>
              <div className="status-matrix">
                <label className="settings-toggle-row">
                  <span>
                    <strong>Show system apps</strong>
                    <em>Include Finder, System Settings, and utility apps in summaries.</em>
                  </span>
                  <input
                    checked={settingsView.showSystemApps}
                    onChange={(event) => onSaveExperienceSettings({ showSystemApps: event.target.checked })}
                    type="checkbox"
                  />
                </label>
                <label className="settings-toggle-row">
                  <span>
                    <strong>Show technical details</strong>
                    <em>Expose technical activity details in Pro Mode.</em>
                  </span>
                  <input
                    checked={settingsView.showRawEvents}
                    onChange={(event) => onSaveExperienceSettings({ showRawEvents: event.target.checked })}
                    type="checkbox"
                  />
                </label>
                <label className="settings-toggle-row">
                  <span>
                    <strong>Show capture confidence</strong>
                    <em>Display confidence labels for inferred details.</em>
                  </span>
                  <input
                    checked={settingsView.showCaptureConfidence}
                    onChange={(event) => onSaveExperienceSettings({ showCaptureConfidence: event.target.checked })}
                    type="checkbox"
                  />
                </label>
              </div>
            </section>
          )}

          {activeSettings === "capture" && (
            <>
              <section className="settings-section">
                <div className="settings-section-header">
                  <div>
                    <span>First-run permissions</span>
                    <h2>OS capture access</h2>
                  </div>
                  <strong>{permissionStatus}</strong>
                </div>
                <PermissionStatusList
                  compact
                  onOpenSettings={onOpenCapturePermission}
                  onRefresh={onRefreshCapturePermissions}
                  onRestart={onRestartApp}
                  onTriggerBrowserAutomation={onTriggerBrowserAutomation}
                  summary={permissionSummary}
                />
              </section>
              <section className="settings-section">
                <div className="settings-section-header">
                  <div>
                    <span>Capture health</span>
                    <h2>{captureHealth?.headline ?? "Waiting for capture"}</h2>
                  </div>
                  <strong>{captureHealth?.status?.replace("_", " ") ?? "Waiting"}</strong>
                </div>
                <CaptureHealthPanel
                  onInstallTerminalBridge={onInstallTerminalBridge}
                  summary={captureHealth}
                  terminalBridgeStatus={terminalBridgeStatus}
                />
              </section>
              <div className="settings-card-grid">
                <section className="settings-section">
                  <div className="settings-section-header">
                    <div>
                      <span>Data sources</span>
                      <h2>What DayTrail captures</h2>
                    </div>
                  </div>
                  <div className="status-matrix">
                    <div className="status-row" data-state={checkState("desktop")}><span>Apps and windows</span><strong>{checkStatus("desktop")}</strong></div>
                    <div className="status-row" data-state={checkState("browser")}><span>Browsers</span><strong>{checkStatus("browser")}</strong></div>
                    <div className="status-row" data-state={checkState("editor")}><span>Editor projects</span><strong>{checkStatus("editor")}</strong></div>
                    <div className="status-row" data-state={checkState("terminal")}><span>Terminal folders</span><strong>{checkStatus("terminal")}</strong></div>
                    <div className="status-row" data-state={checkState("ai")}><span>AI tools</span><strong>{checkStatus("ai")}</strong></div>
                  </div>
                </section>
                <section className="settings-section">
                  <div className="settings-section-header">
                    <div>
                      <span>Data controls</span>
                      <h2>Manage your data</h2>
                    </div>
                  </div>
                  <div className="settings-action-list">
                    <button className="settings-action-row" onClick={onOpenExports} type="button">
                      <Icon name="archive" />
                      <span><strong>Export activity data</strong><em>Export captured data as JSON for any date range.</em></span>
                      <Icon name="arrow" />
                    </button>
                    <button className="settings-action-row" onClick={onOpenSavedNotes} type="button">
                      <Icon name="copy" />
                      <span><strong>Manage saved notes</strong><em>Review and delete saved scratchpad notes.</em></span>
                      <Icon name="arrow" />
                    </button>
                  </div>
                </section>
              </div>
            </>
          )}

          {activeSettings === "ai" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>AI analysis provider</span>
                  <h2>Local/cloud model routing</h2>
                </div>
                <strong>{saveState}</strong>
              </div>
              <form className="settings-form" onSubmit={saveAiConfig}>
                <label className="settings-toggle-row">
                  <span>
                    <strong>Launch at login</strong>
                    <em>Start DayTrail automatically when you sign in.</em>
                  </span>
                  <input
                    checked={launchAtLogin}
                    onChange={(event) => setLaunchAtLogin(event.target.checked)}
                    type="checkbox"
                  />
                </label>
                <label className="settings-field" htmlFor="provider">
                  <span>Provider</span>
                  <select
                    id="provider"
                    onChange={(event) => {
                      const provider = event.target.value as AiConfig["provider"];
                      setAiConfig({
                        ...aiConfig,
                        provider,
                        model: defaultModelForProvider(provider),
                        endpoint: defaultEndpointForProvider(provider),
                        apiKey: "",
                      });
                      setSaveState("Unsaved provider change");
                    }}
                    value={aiConfig.provider}
                  >
                    <option>Ollama Local</option>
                    <option>LM Studio</option>
                    <option>OpenAI Compatible</option>
                    <option>OpenAI</option>
                    <option>OpenRouter</option>
                    <option>Groq</option>
                    <option>Gemini</option>
                    <option>Anthropic</option>
                    <option>NVIDIA NIM</option>
                    <option>Custom API</option>
                  </select>
                </label>
                <label className="settings-field" htmlFor="model">
                  <span>Model</span>
                  <input
                    id="model"
                    onChange={(event) => {
                      const model = event.target.value;
                      setAiConfig({
                        ...aiConfig,
                        model,
                        endpoint:
                          aiConfig.provider === "Gemini"
                            ? endpointForProviderModel(aiConfig.provider, model)
                            : aiConfig.endpoint,
                      });
                    }}
                    value={aiConfig.model}
                  />
                </label>
                <details className="context-pack-details">
                  <summary>Advanced endpoint</summary>
                  <label className="settings-field" htmlFor="endpoint">
                    <span>Endpoint</span>
                    <input
                      id="endpoint"
                      onChange={(event) => setAiConfig({ ...aiConfig, endpoint: event.target.value })}
                      value={aiConfig.endpoint}
                    />
                  </label>
                </details>
                <label className="settings-field" htmlFor="api-key">
                  <span>API key</span>
                  <input
                    autoComplete="off"
                    id="api-key"
                    onChange={(event) => setAiConfig({ ...aiConfig, apiKey: event.target.value })}
                    placeholder="Stored in OS keychain"
                    type="password"
                    value={aiConfig.apiKey}
                  />
                </label>
                <label className="settings-toggle-row">
                  <span>
                    <strong>Redact secrets before DayTrail analysis</strong>
                    <em>Prompts sent for analysis are scrubbed before leaving the device.</em>
                  </span>
                  <input
                    checked={aiConfig.redactSecrets}
                    onChange={(event) =>
                      setAiConfig({ ...aiConfig, redactSecrets: event.target.checked })
                    }
                    type="checkbox"
                  />
                </label>
                <div className="settings-actions">
                  <button className="button compact primary" type="submit">
                    <Icon name="save" />
                    Save settings
                  </button>
                </div>
              </form>
            </section>
          )}

          {activeSettings === "advanced" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>Advanced</span>
                  <h2>Storage, exports, bridges, and technical settings</h2>
                </div>
                <strong>{storageStatus}</strong>
              </div>
              <div className="settings-action-list">
                <button className="settings-action-row" onClick={onOpenExports} type="button">
                  <Icon name="archive" />
                  <span><strong>Export activity data</strong><em>Open date-range JSON export and AI routine analysis.</em></span>
                  <Icon name="arrow" />
                </button>
                <button className="settings-action-row" onClick={onOpenSavedNotes} type="button">
                  <Icon name="copy" />
                  <span><strong>Manage saved notes</strong><em>Review and delete saved scratchpad notes.</em></span>
                  <Icon name="arrow" />
                </button>
                <button className="settings-action-row" onClick={onLoadStorageInfo} type="button">
                  <Icon name="sync" />
                  <span><strong>Load storage locations</strong><em>Show database and backup paths.</em></span>
                  <Icon name="arrow" />
                </button>
                <button className="settings-action-row" onClick={onInstallTerminalBridge} type="button">
                  <Icon name="apps" />
                  <span><strong>Install terminal bridge</strong><em>Improve terminal folder detection.</em></span>
                  <Icon name="arrow" />
                </button>
              </div>
              <div className="status-matrix storage-location-list">
                <div className="status-row" data-state={storageInfo ? "ok" : "muted"}>
                  <span>Current database</span>
                  <strong>{storageInfo?.databasePath ?? "Load storage locations"}</strong>
                </div>
                <div className="status-row" data-state={storageInfo ? "ok" : "muted"}>
                  <span>Backup folder</span>
                  <strong>{storageInfo?.backupDir ?? "Load storage locations"}</strong>
                </div>
                <div className="status-row" data-state="ok">
                  <span>AI endpoint</span>
                  <strong>{aiConfig.endpoint}</strong>
                </div>
              </div>
            </section>
          )}

          {activeSettings === "privacy" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>Privacy controls</span>
                  <h2>Metadata-first capture policy</h2>
                </div>
                <strong>Metadata only</strong>
              </div>
              <div className="tool-chip-row privacy-badge-row">
                {settingsView.privacyBadges.map((badge) => (
                  <span className="tool-chip" key={badge}>{badge}</span>
                ))}
              </div>
              <div className="status-matrix privacy-matrix">
                <div className="status-row" data-state="ok"><span>Apps and windows</span><strong>Active metadata only</strong></div>
                <div className="status-row" data-state="ok"><span>Browsers</span><strong>Domain + redacted URL</strong></div>
                <div className="status-row" data-state="ok"><span>Editor and terminal</span><strong>Project/folder path</strong></div>
                <div className="status-row" data-state="ok"><span>AI prompts</span><strong>Redacted before analysis</strong></div>
                <div className="status-row" data-state="muted"><span>Screenshots</span><strong>Not captured</strong></div>
                <div className="status-row" data-state="muted"><span>Clipboard content</span><strong>Not captured</strong></div>
                <div className="status-row" data-state="muted"><span>File contents</span><strong>Not captured by default</strong></div>
                <div className="status-row" data-state={excludedDomainCount > 0 ? "warning" : "ok"}><span>Excluded browser domains</span><strong>{excludedDomainCount}</strong></div>
              </div>
            </section>
          )}

          {activeSettings === "integrations" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>Integrations</span>
                  <h2>Connected capture bridges</h2>
                </div>
                <strong>{captureHealth?.status?.replace("_", " ") ?? "Waiting"}</strong>
              </div>
              <div className="health-check-list">
                {(captureHealth?.checks ?? []).map((check) => (
                  <article className="health-check-row" data-state={check.status} key={check.id}>
                    <span>{check.label}</span>
                    <strong>{check.status.replace("_", " ")}</strong>
                    <em>{check.detail}</em>
                    <small>{check.lastSeenAt ? formatDateTime(check.lastSeenAt) : check.action || "Waiting"}</small>
                  </article>
                ))}
                {(captureHealth?.checks ?? []).length === 0 && (
                  <div className="empty-state compact-empty">
                    <strong>No bridge status yet</strong>
                    <span>Bridge status appears after the installed desktop app sees app, browser, editor, terminal, or AI signals.</span>
                  </div>
                )}
              </div>
            </section>
          )}

          {activeSettings === "storage" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>Data Storage</span>
                  <h2>Local database and portable setup</h2>
                </div>
                <strong>{storageStatus}</strong>
              </div>
              <div className="status-matrix storage-location-list">
                <div className="status-row" data-state={storageInfo ? "ok" : "muted"}>
                  <span>Current database</span>
                  <strong>{storageInfo?.databasePath ?? "Waiting for desktop app"}</strong>
                </div>
                <div className="status-row" data-state={storageInfo ? "ok" : "muted"}>
                  <span>Backup folder</span>
                  <strong>{storageInfo?.backupDir ?? "Waiting for desktop app"}</strong>
                </div>
              </div>
              <div className="settings-action-list">
                <button className="settings-action-row" onClick={onOpenExports} type="button">
                  <Icon name="archive" />
                  <span><strong>Export activity data</strong><em>Open date-range JSON export and AI routine analysis.</em></span>
                  <Icon name="arrow" />
                </button>
                <button className="settings-action-row" onClick={onExportSettingsConfig} type="button">
                  <Icon name="copy" />
                  <span><strong>Export configuration</strong><em>Prepare portable settings JSON without API keys.</em></span>
                  <Icon name="arrow" />
                </button>
                <button className="settings-action-row" onClick={onImportSettingsConfig} type="button">
                  <Icon name="return" />
                  <span><strong>Import configuration</strong><em>Apply the JSON below on this machine.</em></span>
                  <Icon name="arrow" />
                </button>
                <button className="settings-action-row" onClick={onBackupDatabase} type="button">
                  <Icon name="save" />
                  <span><strong>Backup database</strong><em>Create a verified SQLite backup in the backup folder.</em></span>
                  <Icon name="arrow" />
                </button>
              </div>
              <label className="settings-field" htmlFor="settings-config-json">
                <span>Settings configuration JSON</span>
                <textarea
                  className="export-preview settings-config-editor"
                  id="settings-config-json"
                  onChange={(event) => setSettingsConfigJson(event.target.value)}
                  value={settingsConfigJson}
                />
              </label>
              <div className="settings-form">
                <label className="settings-field" htmlFor="database-restore-path">
                  <span>Database file path to restore</span>
                  <input
                    id="database-restore-path"
                    onChange={(event) => setDatabaseRestorePath(event.target.value)}
                    placeholder="/path/to/daytrail-backup.sqlite3"
                    value={databaseRestorePath}
                  />
                </label>
                <div className="settings-actions">
                  <button className="button compact" onClick={onRestoreDatabase} type="button">
                    <Icon name="return" />
                    Restore database
                  </button>
                </div>
              </div>
              <div className="folder-list">
                <div className="settings-section-header inline-section-header">
                  <div>
                    <span>Workspace folders</span>
                    <h2>Captured project folders</h2>
                  </div>
                  <strong>{selectedCount} selected</strong>
                </div>
                {folders.length === 0 && (
                  <div className="empty-state compact-empty">
                    <strong>No workspace folders selected manually</strong>
                    <span>Folders are discovered from captured editor and terminal context.</span>
                  </div>
                )}
                {folders.map((folder) => (
                  <label className="folder-row" key={folder.id}>
                    <input
                      checked={folder.selected}
                      onChange={() => toggleFolder(folder.id)}
                      type="checkbox"
                    />
                    <span>
                      <strong>{folder.label}</strong>
                      <em>{folder.path}</em>
                    </span>
                  </label>
                ))}
              </div>
            </section>
          )}

          {activeSettings === "shortcuts" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>Shortcuts</span>
                  <h2>Keyboard shortcuts</h2>
                </div>
              </div>
              <div className="status-matrix">
                <div className="status-row" data-state="ok"><span>Search work</span><strong>⌥ Space / ⌘K</strong></div>
                <div className="status-row" data-state="ok"><span>Daily report</span><strong>Toolbar button</strong></div>
                <div className="status-row" data-state="ok"><span>Tray capture controls</span><strong>Menu bar</strong></div>
              </div>
            </section>
          )}

          {activeSettings === "about" && (
            <section className="settings-section">
              <div className="settings-section-header">
                <div>
                  <span>About</span>
                  <h2>DayTrail</h2>
                </div>
                <strong>Retrace your workday.</strong>
              </div>
              <div className="about-card">
                <img alt="" src="/daytrail-icon.png" />
                <div>
                  <strong>DayTrail</strong>
                  <span>Local-first work memory, AI usage tracking, and timesheet-grade daily reporting.</span>
                </div>
              </div>
            </section>
          )}
        </section>
      </div>
    </div>
  );
}

function CaptureHealthPanel({
  onInstallTerminalBridge,
  summary,
  terminalBridgeStatus,
}: {
  onInstallTerminalBridge?: () => void;
  summary?: BackendCaptureHealthSummary;
  terminalBridgeStatus?: string;
}) {
  const checks = summary?.checks ?? [];
  const status = summary?.status?.replace("_", " ") ?? "Waiting";

  return (
    <div className="capture-health-panel">
      <div className="capture-health-summary">
        <span>{status}</span>
        <strong>{summary?.headline ?? "No capture signals yet"}</strong>
      </div>
      <div className="health-check-list">
        {checks.length === 0 && (
          <div className="empty-state compact-empty">
            <strong>Waiting for first signal</strong>
            <span>Switch to another app, browser tab, editor, or terminal. If this stays empty, macOS Accessibility permission is likely missing.</span>
          </div>
        )}
        {checks.map((check) => {
          const isTerminalBridge = check.id === "terminal-bridge";
          const canInstallTerminalBridge =
            isTerminalBridge
            && check.status !== "ok"
            && Boolean(onInstallTerminalBridge)
            && (
              Boolean(check.action?.toLowerCase().includes("install"))
              || terminalBridgeStatus === "Install failed"
            );
          return (
            <article className="health-check-row" data-state={check.status} key={check.id}>
              <span>{check.label}</span>
              <strong>{check.status.replace("_", " ")}</strong>
              <em>{check.detail}</em>
              <small>
                {isTerminalBridge && terminalBridgeStatus && terminalBridgeStatus !== "Ready to install"
                  ? terminalBridgeStatus
                  : check.lastSeenAt
                    ? formatDateTime(check.lastSeenAt)
                    : check.action || "Waiting for activity"}
              </small>
              {canInstallTerminalBridge && (
                <button className="button compact" onClick={onInstallTerminalBridge} type="button">
                  <Icon name="sync" />
                  Install terminal bridge
                </button>
              )}
            </article>
          );
        })}
      </div>
    </div>
  );
}

function CommandOverlay({
  commandQuery,
  commandResults,
  memoryResults,
  onClose,
  onRun,
  setCommandQuery,
}: {
  commandQuery: string;
  commandResults: string[];
  memoryResults: BackendSearchResult[];
  onClose: () => void;
  onRun: (command: string) => void | Promise<void>;
  setCommandQuery: (query: string) => void;
}) {
  const activeCommand = commandQuery.trim().startsWith("/")
    ? commandQuery.trim()
    : "/today";
  const answerByCommand: Record<string, string> = {
    "/today":
      "Open Today to review the hour-by-hour timeline and recent work.",
    "/activity":
      "Open Activity to inspect apps, sites, folders, and activity details.",
    "/report":
      "Generate the daily report from captured activity.",
    "/what-did-i-do":
      "Open Today to review the hour-by-hour timeline and recent work.",
    "/ai-usage":
      "Open Activity to see AI usage in context.",
    "/export":
      "Open Export Data to preview or export local activity data.",
    "/eod":
      "Generate the daily report from captured activity.",
  };

  return (
    <div className="overlay-backdrop" onClick={onClose} role="presentation">
      <section
        aria-label="Command bar"
        aria-modal="true"
        className="command-overlay"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
      >
        <div className="command-input-row">
          <Icon name="search" />
          <input
            autoFocus
            onChange={(event) => setCommandQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                onClose();
              }
            }}
            placeholder="Search work, apps, AI tools, folders..."
            value={commandQuery}
          />
          <button
            aria-label="Close command bar"
            className="icon-button"
            onClick={onClose}
            type="button"
          >
            <Icon name="x" />
          </button>
        </div>

        <div className="command-body">
          <div className="command-results">
            {commandResults.map((command) => (
              <button
                className="command-result"
                key={command}
                onClick={() => onRun(command)}
                type="button"
              >
                <strong>{commandLabels[command] ?? command}</strong>
                <span>{command === "/report" || command === "/eod" ? "Generate" : "Open"}</span>
              </button>
            ))}
            {memoryResults.map((result) => (
              <button
                className="command-result"
                key={`${result.entityType}-${result.entityId}`}
                onClick={() => setCommandQuery(result.title)}
                type="button"
              >
                <code>{result.entityType}</code>
                <span>{result.title}</span>
              </button>
            ))}
          </div>
          <article className="ai-answer-block">
            <span>{memoryResults.length ? "Search result" : "Action"}</span>
            <p>
              {memoryResults[0]
                ? memoryResults[0].snippet || memoryResults[0].title
                : answerByCommand[activeCommand] ?? answerByCommand["/today"]}
            </p>
            <span className="source-anchor" aria-label="Selected source">
              {memoryResults[0]
                ? `${memoryResults[0].entityType} #${memoryResults[0].entityId}`
                : "No result selected"}
            </span>
          </article>
        </div>
      </section>
    </div>
  );
}

function PanelHeader({
  eyebrow,
  title,
  value,
}: {
  eyebrow: string;
  title: string;
  value: string;
}) {
  return (
    <header className="panel-header">
      <div>
        <span>{eyebrow}</span>
        <h2>{title}</h2>
      </div>
      <em>{value}</em>
    </header>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric-cell">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

type IconName =
  | "apps"
  | "archive"
  | "arrow"
  | "check"
  | "copy"
  | "layout"
  | "plus"
  | "return"
  | "ritual"
  | "save"
  | "search"
  | "sliders"
  | "sync"
  | "warning"
  | "x";

function Icon({ name }: { name: IconName }) {
  const pathByName: Record<IconName, ReactNode> = {
    apps: (
      <>
        <rect height="6" rx="1.5" width="6" x="4" y="4" />
        <rect height="6" rx="1.5" width="6" x="14" y="4" />
        <rect height="6" rx="1.5" width="6" x="4" y="14" />
        <rect height="6" rx="1.5" width="6" x="14" y="14" />
      </>
    ),
    archive: (
      <>
        <path d="M4 7h16v13H4z" />
        <path d="M3 4h18v3H3zM9 11h6" />
      </>
    ),
    arrow: <path d="M5 12h13M13 6l6 6-6 6" />,
    check: <path d="m5 12 4 4L19 6" />,
    copy: (
      <>
        <rect height="12" rx="2" width="12" x="8" y="8" />
        <path d="M5 15H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
      </>
    ),
    layout: (
      <>
        <rect height="14" rx="2" width="16" x="4" y="5" />
        <path d="M9 5v14M4 10h16" />
      </>
    ),
    plus: <path d="M12 5v14M5 12h14" />,
    return: <path d="M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3" />,
    ritual: (
      <>
        <path d="M12 3v5M12 16v5M5.6 5.6l3.5 3.5M14.9 14.9l3.5 3.5M3 12h5M16 12h5M5.6 18.4l3.5-3.5M14.9 9.1l3.5-3.5" />
        <circle cx="12" cy="12" r="3" />
      </>
    ),
    save: (
      <>
        <path d="M5 3h12l2 2v16H5z" />
        <path d="M8 3v6h8V3M8 21v-7h8v7" />
      </>
    ),
    search: (
      <>
        <circle cx="11" cy="11" r="7" />
        <path d="m16.5 16.5 4 4" />
      </>
    ),
    sliders: (
      <>
        <path d="M4 7h16M4 17h16" />
        <circle cx="9" cy="7" r="2" />
        <circle cx="15" cy="17" r="2" />
      </>
    ),
    sync: (
      <>
        <path d="M20 11a8 8 0 0 0-14.8-4M4 5v5h5" />
        <path d="M4 13a8 8 0 0 0 14.8 4M20 19v-5h-5" />
      </>
    ),
    warning: (
      <>
        <path d="m12 3 10 18H2z" />
        <path d="M12 9v5M12 17h.01" />
      </>
    ),
    x: <path d="M6 6l12 12M18 6 6 18" />,
  };

  return (
    <svg
      aria-hidden="true"
      className="icon"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
    >
      {pathByName[name]}
    </svg>
  );
}
