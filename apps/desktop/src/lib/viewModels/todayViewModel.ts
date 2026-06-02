import { isSimpleVisibleApp, normalizeAppCategory } from "./appClassification";
import { formatDuration, isLowData, totalTrackedMs } from "./duration";
import { buildHourTimelineView, ExperienceSettingsLike, SourceEventLike } from "./hourTimelineViewModel";

type AppUsageLike = {
  app: string;
  category?: string | null;
  durationMs: number;
  events?: number;
  aiTools?: Array<{ tool: string; durationMs?: number }>;
  projects?: Array<{ label: string; durationMs?: number; events?: number; aiTools?: Array<{ tool: string; durationMs?: number }> }>;
};

type RecoverySummaryLike = {
  score?: number | null;
  totalScreenMs?: number | null;
  longestUninterruptedMs?: number | null;
  currentStreakMs?: number | null;
  takenCount?: number | null;
  skippedCount?: number | null;
  snoozedCount?: number | null;
  promptedCount?: number | null;
  nextPrompt?: {
    action?: string | null;
    reason?: string | null;
    streakMs?: number | null;
    suggestedMinutes?: number | null;
  } | null;
} | null;

export type TodaySnapshotLike = {
  sourceEvents?: SourceEventLike[] | null;
  workSessions?: Array<{
    id?: string;
    title?: string;
    status?: string;
    startedAt?: number;
    endedAt?: number;
    durationMs?: number;
    aiUsed?: boolean;
    confidencePercent?: number;
    summary?: string | null;
    evidenceEventIds?: string[];
  }> | null;
  appUsageSummary?: { totalDurationMs?: number; apps?: AppUsageLike[] } | null;
  aiUsageSummary?: { totalDurationMs?: number; tools?: Array<{ tool: string; durationMs?: number }> } | null;
  aiOutputLedger?: Array<{ status?: string | null; tool?: string | null; title?: string | null; durationMs?: number | null }> | null;
  recoverySummary?: RecoverySummaryLike;
  idleBlocks?: Array<{ id?: string; classified?: boolean; durationMs?: number | null }> | null;
  unclosedLoopInbox?: Array<{
    id?: string;
    category?: string | null;
    title?: string | null;
    detail?: string | null;
    source?: string | null;
    risk?: string | null;
    status?: string | null;
    primaryAction?: string | null;
    evidenceIds?: string[];
  }> | null;
};

export function buildTodayView(snapshot: TodaySnapshotLike | null | undefined, settings?: ExperienceSettingsLike | null) {
  const apps = snapshot?.appUsageSummary?.apps ?? [];
  const simpleMode = settings?.experienceMode !== "pro";
  const topWorkApp = apps
    .filter((app) => (simpleMode && !settings?.showSystemApps ? isSimpleVisibleApp(app.app, app.category) : true))
    .sort((left, right) => right.durationMs - left.durationMs)[0];
  const trackedMs = totalTrackedMs(snapshot);
  const hourTimeline = buildHourTimelineView(snapshot?.sourceEvents ?? [], settings);
  const aiTools = snapshot?.aiUsageSummary?.tools ?? [];
  const reviewCount =
    (snapshot?.unclosedLoopInbox?.length ?? 0) +
    (snapshot?.idleBlocks?.filter((block) => !block.classified).length ?? 0);
  const recovery = buildRecoveryView(snapshot?.recoverySummary);

  return {
    lowData: isLowData(snapshot),
    totalTrackedMs: trackedMs,
    totalTrackedLabel: formatDuration(trackedMs),
    topWorkApp: topWorkApp
      ? {
          name: topWorkApp.app,
          category: normalizeAppCategory(topWorkApp.category, topWorkApp.app),
          durationMs: topWorkApp.durationMs,
          durationLabel: formatDuration(topWorkApp.durationMs),
        }
      : undefined,
    topWorkAppFallback: topWorkApp ? undefined : "No work app yet",
    sessionCount: snapshot?.workSessions?.length ?? 0,
    appCount: simpleMode
      ? apps.filter((app) => settings?.showSystemApps || isSimpleVisibleApp(app.app, app.category)).length
      : apps.length,
    aiToolCount: aiTools.length,
    aiDetectedLabel: aiTools.length
      ? aiTools.map((tool) => tool.tool).slice(0, 4).join(", ")
      : "No AI activity detected yet",
    reviewCount,
    recovery,
    hourTimeline,
  };
}

function buildRecoveryView(summary: RecoverySummaryLike | undefined) {
  const score = clampNumber(summary?.score, 0);
  const totalScreenMs = clampNumber(summary?.totalScreenMs, 0);
  const longestRunMs = clampNumber(summary?.longestUninterruptedMs, 0);
  const currentStreakMs = clampNumber(summary?.currentStreakMs, 0);
  const takenCount = clampNumber(summary?.takenCount, 0);
  const skippedCount = clampNumber(summary?.skippedCount, 0);
  const snoozedCount = clampNumber(summary?.snoozedCount, 0);
  const promptAction = summary?.nextPrompt?.action ?? null;
  const promptDue = promptAction === "due";
  const hasData = totalScreenMs > 0 || longestRunMs > 0 || takenCount > 0 || skippedCount > 0;

  return {
    score,
    scoreLabel: hasData ? String(score) : "Ready",
    statusLabel: promptDue ? "Recovery due" : hasData ? "On rhythm" : "Ready",
    longestRunMs,
    longestRunLabel: formatDuration(longestRunMs),
    currentStreakMs,
    currentStreakLabel: formatDuration(currentStreakMs),
    takenCount,
    takenLabel: `${takenCount} taken`,
    skippedCount,
    skippedLabel: `${skippedCount} skipped`,
    snoozedCount,
    snoozedLabel: `${snoozedCount} snoozed`,
    promptDue,
    promptReason: summary?.nextPrompt?.reason ?? "Recovery rhythm is available",
    suggestedMinutes: clampNumber(summary?.nextPrompt?.suggestedMinutes, 3) || 3,
  };
}

function clampNumber(value: unknown, fallback: number) {
  return typeof value === "number" && Number.isFinite(value) ? Math.max(0, Math.floor(value)) : fallback;
}
