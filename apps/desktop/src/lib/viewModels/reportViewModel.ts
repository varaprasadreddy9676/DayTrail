import { formatDuration, isLowData, totalTrackedMs } from "./duration";
import { isSimpleVisibleApp } from "./appClassification";
import { ExperienceSettingsLike, normalizeExperienceSettings } from "./hourTimelineViewModel";
import { TodaySnapshotLike } from "./todayViewModel";

function formatTime(value?: number | null): string {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

function sessionEventIds(session: NonNullable<TodaySnapshotLike["workSessions"]>[number]) {
  return new Set(session.evidenceEventIds ?? []);
}

function sessionEvents(snapshot: TodaySnapshotLike | null | undefined, session: NonNullable<TodaySnapshotLike["workSessions"]>[number]) {
  const ids = sessionEventIds(session);
  if (ids.size > 0) {
    return (snapshot?.sourceEvents ?? []).filter((event) => event.id && ids.has(event.id));
  }
  const startedAt = session.startedAt ?? 0;
  const endedAt = session.endedAt ?? startedAt;
  return (snapshot?.sourceEvents ?? []).filter((event) => {
    const eventStart = event.startedAt ?? 0;
    const eventEnd = event.endedAt ?? eventStart;
    return eventStart < endedAt && eventEnd > startedAt;
  });
}

function summarizeSessionLine(snapshot: TodaySnapshotLike | null | undefined, session: NonNullable<TodaySnapshotLike["workSessions"]>[number]): string {
  const events = sessionEvents(snapshot, session);
  const apps = [...new Set(events.map((event) => event.app ?? event.source).filter(Boolean))].slice(0, 3);
  const contexts = [...new Set(events.map((event) => event.workspaceKey ?? event.domain).filter(Boolean))].slice(0, 2);
  const timeRange = [formatTime(session.startedAt), formatTime(session.endedAt)].filter(Boolean).join(" - ");
  const parts = [
    `${session.title ?? "Work session"} - ${formatDuration(session.durationMs ?? 0)}`,
    timeRange,
    apps.length ? `mostly in ${apps.join(", ")}` : null,
    contexts.length ? `context: ${contexts.join(", ")}` : null,
  ].filter(Boolean);
  return `- ${parts.join("; ")}`;
}

export function buildDeterministicReportMarkdown(
  snapshot: TodaySnapshotLike | null | undefined,
  settings?: ExperienceSettingsLike | null,
): string {
  const normalizedSettings = normalizeExperienceSettings(settings);
  const sessions = snapshot?.workSessions ?? [];
  const includeSystemApps = normalizedSettings.experienceMode === "pro" || normalizedSettings.showSystemApps;
  const apps = includeSystemApps
    ? snapshot?.appUsageSummary?.apps ?? []
    : (snapshot?.appUsageSummary?.apps ?? []).filter((app) => isSimpleVisibleApp(app.app, app.category));
  const tools = snapshot?.aiUsageSummary?.tools ?? [];
  const reviewCount = (snapshot?.unclosedLoopInbox?.length ?? 0) + (snapshot?.idleBlocks?.filter((block) => !block.classified).length ?? 0);

  return [
    "# Daily Work Report",
    "",
    "## Summary",
    `You worked for ${formatDuration(totalTrackedMs(snapshot))} across ${sessions.length} session${sessions.length === 1 ? "" : "s"}.`,
    sessions[0]
      ? `Main thread: ${sessions[0].title ?? "Work session"} (${formatDuration(sessions[0].durationMs ?? 0)}).`
      : "No complete work session has enough detail yet.",
    tools.length
      ? `AI tools detected: ${tools.map((tool) => tool.tool).slice(0, 5).join(", ")}.`
      : "No AI tools were detected in the current activity.",
    "",
    "## What happened",
    ...(sessions.length
      ? sessions.slice(0, 5).map((session) => summarizeSessionLine(snapshot, session))
      : ["- Keep working for a few minutes and DayTrail will summarize the main work threads."]),
    "",
    "## Work sessions",
    ...(sessions.length
      ? sessions.map((session) => `- ${session.title ?? "Work session"} - ${formatDuration(session.durationMs ?? 0)}${session.summary ? ` - ${session.summary}` : ""}`)
      : ["- No work sessions captured yet."]),
    "",
    "## Apps used",
    ...(apps.length
      ? apps.slice(0, 8).map((app) => `- ${app.app} - ${formatDuration(app.durationMs)}`)
      : ["- No app activity captured yet."]),
    "",
    "## AI detected",
    ...(tools.length
      ? tools.slice(0, 8).map((tool) => `- ${tool.tool} - ${formatDuration(tool.durationMs ?? 0)}`)
      : ["- No AI activity detected yet."]),
    "",
    "## Needs review",
    reviewCount ? `- ${reviewCount} item${reviewCount === 1 ? "" : "s"} need review.` : "- No review items detected from current sessions.",
  ].join("\n");
}

export function buildReportView(
  snapshot: TodaySnapshotLike | null | undefined,
  settings?: ExperienceSettingsLike | null,
  reportMarkdown = "",
) {
  const normalizedSettings = normalizeExperienceSettings(settings);
  const includeSystemApps = normalizedSettings.experienceMode === "pro" || normalizedSettings.showSystemApps;
  const sessions = snapshot?.workSessions ?? [];
  const apps = includeSystemApps
    ? snapshot?.appUsageSummary?.apps ?? []
    : (snapshot?.appUsageSummary?.apps ?? []).filter((app) => isSimpleVisibleApp(app.app, app.category));
  const markdown = reportMarkdown.trim() || (sessions.length ? buildDeterministicReportMarkdown(snapshot, settings) : "");

  return {
    markdown,
    hasUsableReport: markdown.trim().length > 0,
    lowDataMessage: isLowData(snapshot) && sessions.length === 0
      ? "Reports become useful after at least one work session."
      : undefined,
    includedWork: {
      sessions: sessions.length,
      apps: apps.length,
      aiTools: snapshot?.aiUsageSummary?.tools?.length ?? 0,
    },
  };
}
