import { isSimpleVisibleApp } from "./appClassification";
import { formatDuration, isLowData } from "./duration";
import {
  detectAiToolsFromEvent,
  ExperienceSettingsLike,
  normalizeExperienceSettings,
  SourceEventLike,
} from "./hourTimelineViewModel";
import { TodaySnapshotLike } from "./todayViewModel";

function formatTime(value?: number | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function formatSessionTimeRange(startedAt?: number, endedAt?: number): string {
  const start = formatTime(startedAt);
  const end = formatTime(endedAt);
  if (!start && !end) return "Time unavailable";
  if (!end || start === end) return start;
  return `${start} - ${end}`;
}

function eventApp(event: SourceEventLike): string {
  return event.app?.trim() || event.source?.trim() || "Activity";
}

function sessionEvents(
  session: NonNullable<TodaySnapshotLike["workSessions"]>[number],
  events: SourceEventLike[],
): SourceEventLike[] {
  const evidenceIds = new Set(session.evidenceEventIds ?? []);
  if (evidenceIds.size > 0) {
    return events.filter((event) => event.id && evidenceIds.has(event.id));
  }
  const startedAt = session.startedAt ?? 0;
  const endedAt = session.endedAt ?? startedAt;
  if (!startedAt || !endedAt) return [];
  return events.filter((event) => {
    const eventStart = event.startedAt ?? 0;
    const eventEnd = event.endedAt ?? eventStart;
    return eventStart < endedAt && eventEnd > startedAt;
  });
}

function topValues(values: Map<string, number>, limit: number): Array<{ label: string; durationMs: number; durationLabel: string }> {
  return [...values.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, limit)
    .map(([label, durationMs]) => ({ label, durationMs, durationLabel: formatDuration(durationMs) }));
}

function contextLabelForEvent(event: SourceEventLike): string | null {
  return event.workspaceKey ?? event.domain ?? null;
}

export function buildActivityView(snapshot: TodaySnapshotLike | null | undefined, settings?: ExperienceSettingsLike | null) {
  const normalizedSettings = normalizeExperienceSettings(settings);
  const isPro = normalizedSettings.experienceMode === "pro";
  const showTechnicalDetails = isPro && normalizedSettings.showRawEvents;
  const tabs = showTechnicalDetails
    ? ["Sessions", "Apps", "Projects", "Raw Activity"]
    : ["Sessions", "Apps", "Projects"];
  const apps = snapshot?.appUsageSummary?.apps ?? [];
  const visibleApps = isPro || normalizedSettings.showSystemApps
    ? apps
    : apps.filter((app) => isSimpleVisibleApp(app.app, app.category));
  const sessions = snapshot?.workSessions ?? [];

  return {
    snapshot,
    defaultTab: "Sessions",
    tabs,
    showTechnicalDetails,
    technicalItems: showTechnicalDetails ? snapshot?.sourceEvents ?? [] : [],
    lowDataMessage: isLowData(snapshot)
      ? "Activity details will appear after more work is captured."
      : undefined,
    sessions: sessions.map((session) => {
      const events = sessionEvents(session, snapshot?.sourceEvents ?? []);
      const appDurations = new Map<string, number>();
      const projectDurations = new Map<string, number>();
      const aiTools = new Set<string>();
      events.forEach((event) => {
        const durationMs = event.durationMs ?? 0;
        appDurations.set(eventApp(event), (appDurations.get(eventApp(event)) ?? 0) + durationMs);
        const context = contextLabelForEvent(event);
        if (context) {
          projectDurations.set(context, (projectDurations.get(context) ?? 0) + durationMs);
        }
        detectAiToolsFromEvent(event).forEach((tool) => aiTools.add(tool));
      });
      const qualityWarnings = [
        events.length === 0 ? "No linked activity details" : null,
        appDurations.size === 0 ? "No app breakdown" : null,
        projectDurations.size === 0 ? "No project context" : null,
        (session.confidencePercent ?? 100) < 70 ? "Low confidence" : null,
      ].filter((item): item is string => Boolean(item));

      return {
        ...session,
        timeRangeLabel: formatSessionTimeRange(session.startedAt, session.endedAt),
        durationLabel: formatDuration(session.durationMs),
        mainApps: topValues(appDurations, 4),
        projects: topValues(projectDurations, 3),
        aiTools: [...aiTools].slice(0, 5),
        qualityLabel: qualityWarnings.length ? "Needs context" : "Clear session",
        qualityWarnings,
        eventCount: events.length,
      };
    }),
    apps: visibleApps.map((app) => ({
      ...app,
      durationLabel: formatDuration(app.durationMs),
    })),
  };
}
