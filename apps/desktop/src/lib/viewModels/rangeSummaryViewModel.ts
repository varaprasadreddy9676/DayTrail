import { formatDuration } from "./duration";

type RangeSourceEventLike = {
  id?: string;
  app?: string | null;
  title?: string | null;
  domain?: string | null;
  durationMs?: number | null;
};

type RangeExportLike = {
  sourceEvents?: RangeSourceEventLike[] | null;
  workSessions?: Array<{ id?: string; aiUsed?: boolean; durationMs?: number | null }> | null;
  timesheetRows?: Array<{ id?: string; durationMs?: number | null; app?: string | null }> | null;
  aiContributionRows?: Array<{ id?: string; status?: string | null; durationMs?: number | null; tool?: string | null }> | null;
  unclosedLoopInbox?: unknown[] | null;
  inferredWorkBlocks?: unknown[] | null;
};

function topByDuration<T extends { label: string; durationMs: number }>(items: T[], limit: number) {
  return items
    .slice()
    .sort((left, right) => right.durationMs - left.durationMs)
    .slice(0, limit);
}

export function buildRangeSummaryView(payload: RangeExportLike | null | undefined) {
  const events = payload?.sourceEvents ?? [];
  const timesheetRows = payload?.timesheetRows ?? [];
  const aiRows = payload?.aiContributionRows ?? [];
  const sessions = payload?.workSessions ?? [];
  const trackedMs =
    timesheetRows.reduce((sum, row) => sum + (row.durationMs ?? 0), 0) ||
    events.reduce((sum, event) => sum + (event.durationMs ?? 0), 0);
  const apps = new Map<string, number>();
  const tools = new Map<string, number>();

  timesheetRows.forEach((row) => {
    const label = row.app || "Captured activity";
    apps.set(label, (apps.get(label) ?? 0) + (row.durationMs ?? 0));
  });
  events.forEach((event) => {
    const label = event.app || event.domain || "Captured activity";
    apps.set(label, (apps.get(label) ?? 0) + (event.durationMs ?? 0));
  });
  aiRows.forEach((row) => {
    const label = row.tool || "AI";
    tools.set(label, (tools.get(label) ?? 0) + (row.durationMs ?? 0));
  });

  const needsReviewCount =
    (payload?.unclosedLoopInbox?.length ?? 0) +
    (payload?.inferredWorkBlocks?.length ?? 0) +
    aiRows.filter((row) => ["needs_review", "review", "blocked", "failed"].includes((row.status ?? "").toLowerCase())).length;

  return {
    empty: events.length === 0 && timesheetRows.length === 0,
    trackedMs,
    trackedLabel: formatDuration(trackedMs),
    sessionCount: sessions.length || timesheetRows.length,
    sourceEventCount: events.length,
    aiOutputCount: aiRows.length,
    needsReviewCount,
    topApps: topByDuration(
      [...apps.entries()].map(([label, durationMs]) => ({ label, durationMs, durationLabel: formatDuration(durationMs) })),
      4,
    ),
    topAiTools: topByDuration(
      [...tools.entries()].map(([label, durationMs]) => ({ label, durationMs, durationLabel: formatDuration(durationMs) })),
      4,
    ),
  };
}
