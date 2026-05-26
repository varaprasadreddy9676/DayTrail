import { isSimpleVisibleApp } from "./appClassification";
import { clampDurationMs, formatDuration } from "./duration";

export type ExperienceMode = "simple" | "pro";
export type ShowAiDetails = "summary" | "detailed";

export type ExperienceSettingsLike = {
  experienceMode?: ExperienceMode;
  showSystemApps?: boolean;
  showRawEvents?: boolean;
  showCaptureConfidence?: boolean;
  showAiDetails?: ShowAiDetails;
};

export type SourceEventLike = {
  id?: string;
  source?: string | null;
  eventType?: string | null;
  app?: string | null;
  title?: string | null;
  domain?: string | null;
  urlRedacted?: string | null;
  workspaceKey?: string | null;
  startedAt?: number;
  endedAt?: number;
  durationMs?: number;
  metadataJson?: string | null;
  createdAt?: number;
};

export type HourTimelineSegment = {
  appName: string;
  durationMs: number;
  durationLabel: string;
  percent: number;
  aiTools: string[];
};

export type HourTimelineHour = {
  hour: number;
  label: string;
  durationMs: number;
  durationLabel: string;
  segments: HourTimelineSegment[];
  aiTools: string[];
  contexts: string[];
  rawItems: SourceEventLike[];
};

export type HourTimelineView = {
  hours: HourTimelineHour[];
  totalDurationMs: number;
};

const aiToolPatterns: Array<[string, RegExp]> = [
  ["ChatGPT", /\bchatgpt\b|chat\.openai\.com|openai\.com\/chat/i],
  ["Codex", /\bcodex\b/i],
  ["Copilot", /\bcopilot\b|github copilot/i],
  ["Claude", /\bclaude\b|anthropic/i],
  ["Gemini", /\bgemini\b/i],
  ["Cursor", /\bcursor\b/i],
  ["Aider", /\baider\b/i],
  ["Cline", /\bcline\b/i],
];

export function normalizeExperienceSettings(settings?: ExperienceSettingsLike | null): Required<ExperienceSettingsLike> {
  return {
    experienceMode: settings?.experienceMode === "pro" ? "pro" : "simple",
    showSystemApps: Boolean(settings?.showSystemApps),
    showRawEvents: Boolean(settings?.showRawEvents),
    showCaptureConfidence: Boolean(settings?.showCaptureConfidence),
    showAiDetails: settings?.showAiDetails === "detailed" ? "detailed" : "summary",
  };
}

export function hourLabel(hour: number): string {
  if (hour === 0) return "12 AM";
  if (hour < 12) return `${hour} AM`;
  if (hour === 12) return "12 PM";
  return `${hour - 12} PM`;
}

export function detectAiToolsFromEvent(event: SourceEventLike): string[] {
  const haystack = [
    event.app,
    event.title,
    event.domain,
    event.urlRedacted,
    event.workspaceKey,
    event.eventType,
    event.source,
    event.metadataJson,
  ]
    .filter(Boolean)
    .join(" ");
  return aiToolPatterns
    .filter(([, pattern]) => pattern.test(haystack))
    .map(([tool]) => tool);
}

function eventApp(event: SourceEventLike): string {
  return event.app?.trim() || event.source?.trim() || "Activity";
}

function eventContext(event: SourceEventLike): string | null {
  return event.workspaceKey?.trim() || event.domain?.trim() || event.title?.trim() || null;
}

function eventStart(event: SourceEventLike): number {
  return typeof event.startedAt === "number" && Number.isFinite(event.startedAt)
    ? event.startedAt
    : typeof event.createdAt === "number" && Number.isFinite(event.createdAt)
      ? event.createdAt
      : Date.now();
}

function eventEnd(event: SourceEventLike): number {
  const start = eventStart(event);
  if (typeof event.endedAt === "number" && Number.isFinite(event.endedAt) && event.endedAt > start) {
    return event.endedAt;
  }
  return start + Math.max(1, clampDurationMs(event.durationMs));
}

export function buildHourTimelineView(
  sourceEvents: SourceEventLike[] = [],
  settings?: ExperienceSettingsLike | null,
): HourTimelineView {
  const normalizedSettings = normalizeExperienceSettings(settings);
  const isPro = normalizedSettings.experienceMode === "pro";
  const includeTechnicalItems = isPro && normalizedSettings.showRawEvents;
  const visibleEvents = sourceEvents.filter((event) => {
    if (isPro || normalizedSettings.showSystemApps) return true;
    return isSimpleVisibleApp(eventApp(event));
  });
  const hours = Array.from({ length: 24 }, (_, hour) => ({
    hour,
    label: hourLabel(hour),
    durationMs: 0,
    segmentsByApp: new Map<string, { durationMs: number; aiTools: Set<string> }>(),
    aiTools: new Set<string>(),
    contexts: new Set<string>(),
    rawItems: [] as SourceEventLike[],
  }));

  visibleEvents.forEach((event) => {
    const start = eventStart(event);
    const end = eventEnd(event);
    const durationMs = Math.max(1, end - start);
    const hour = new Date(start).getHours();
    const bucket = hours[hour];
    const appName = eventApp(event);
    const aiTools = detectAiToolsFromEvent(event);
    const segment = bucket.segmentsByApp.get(appName) ?? { durationMs: 0, aiTools: new Set<string>() };
    segment.durationMs += durationMs;
    aiTools.forEach((tool) => {
      segment.aiTools.add(tool);
      bucket.aiTools.add(tool);
    });
    const context = eventContext(event);
    if (context) bucket.contexts.add(context);
    bucket.durationMs += durationMs;
    bucket.segmentsByApp.set(appName, segment);
    if (includeTechnicalItems) bucket.rawItems.push(event);
  });

  const viewHours = hours.map((bucket): HourTimelineHour => {
    const segments = [...bucket.segmentsByApp.entries()]
      .map(([appName, segment]) => ({
        appName,
        durationMs: segment.durationMs,
        durationLabel: formatDuration(segment.durationMs),
        percent: Math.max(3, Math.round((segment.durationMs / Math.max(bucket.durationMs, 1)) * 100)),
        aiTools: [...segment.aiTools],
      }))
      .sort((left, right) => right.durationMs - left.durationMs);

    return {
      hour: bucket.hour,
      label: bucket.label,
      durationMs: bucket.durationMs,
      durationLabel: formatDuration(bucket.durationMs),
      segments,
      aiTools: [...bucket.aiTools],
      contexts: [...bucket.contexts],
      rawItems: includeTechnicalItems ? bucket.rawItems : [],
    };
  });

  return {
    hours: viewHours,
    totalDurationMs: viewHours.reduce((sum, hour) => sum + hour.durationMs, 0),
  };
}
