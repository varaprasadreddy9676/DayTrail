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
  idleBlocks?: Array<{ classified?: boolean }> | null;
  unclosedLoopInbox?: unknown[] | null;
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
    hourTimeline,
  };
}
