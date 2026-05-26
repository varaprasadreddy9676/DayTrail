import { formatDuration, isLowData } from "./duration";
import {
  detectAiToolsFromEvent,
  ExperienceSettingsLike,
  normalizeExperienceSettings,
  SourceEventLike,
} from "./hourTimelineViewModel";
import { TodaySnapshotLike } from "./todayViewModel";

export type SimpleAiImpactLabel = "Detected" | "Observed" | "Active app" | "Used with session";
export type AiEvidenceStatus = "Completed" | "Needs review" | "Draft" | "Detected only";

function eventContainsTool(event: SourceEventLike, tool: string): boolean {
  return detectAiToolsFromEvent(event).includes(tool);
}

function labelForTool(
  tool: string,
  events: SourceEventLike[],
  sessions: Array<{ evidenceEventIds?: string[]; aiUsed?: boolean }> = [],
): SimpleAiImpactLabel {
  const eventIdsForTool = new Set(events.filter((event) => eventContainsTool(event, tool)).map((event) => event.id).filter(Boolean));
  if (
    eventIdsForTool.size > 0 &&
    sessions.some((session) => (session.evidenceEventIds ?? []).some((id) => eventIdsForTool.has(id)))
  ) {
    return "Used with session";
  }

  if (events.some((event) => (event.app ?? "").toLowerCase().includes(tool.toLowerCase()))) {
    return "Active app";
  }

  return events.some((event) => eventContainsTool(event, tool)) ? "Observed" : "Detected";
}

export function buildAiImpactView(snapshot: TodaySnapshotLike | null | undefined, settings?: ExperienceSettingsLike | null) {
  const normalizedSettings = normalizeExperienceSettings(settings);
  const events = snapshot?.sourceEvents ?? [];
  const sessions = snapshot?.workSessions ?? [];
  const tools = snapshot?.aiUsageSummary?.tools ?? [];
  const ledger = snapshot?.aiOutputLedger ?? [];
  const completedCount = ledger.filter((item) =>
    ["sent", "shared", "completed", "accepted", "done"].includes((item.status ?? "").toLowerCase()),
  ).length;
  const needsReviewCount = ledger.filter((item) =>
    ["needs_review", "review", "failed", "blocked"].includes((item.status ?? "").toLowerCase()),
  ).length;
  const draftCount = ledger.filter((item) =>
    ["draft", "created", "captured"].includes((item.status ?? "").toLowerCase()),
  ).length;
  const toolSummaries = tools.map((tool) => ({
    tool: tool.tool,
    durationMs: tool.durationMs ?? 0,
    durationLabel: formatDuration(tool.durationMs ?? 0),
    label: normalizedSettings.experienceMode === "pro" && normalizedSettings.showAiDetails === "detailed"
      ? labelForTool(tool.tool, events, sessions)
      : labelForTool(tool.tool, events, sessions),
  }));
  const mostActive = toolSummaries.slice().sort((left, right) => right.durationMs - left.durationMs)[0];

  return {
    lowDataMessage: isLowData(snapshot)
      ? "AI tools detected, but not enough activity for a useful breakdown."
      : undefined,
    toolsDetected: toolSummaries.map((tool) => tool.tool),
    usedMostlyWith: snapshot?.workSessions?.find((session) => session.aiUsed)?.title ?? snapshot?.appUsageSummary?.apps?.[0]?.app ?? "No session yet",
    mostActiveTool: mostActive?.tool ?? null,
    confidenceLabel: normalizedSettings.experienceMode === "pro" ? "Detailed detection" : "Basic detection",
    evidenceStatus: ledger.length === 0
      ? "Detected only" as AiEvidenceStatus
      : needsReviewCount > 0
        ? "Needs review" as AiEvidenceStatus
        : completedCount > 0
          ? "Completed" as AiEvidenceStatus
          : "Draft" as AiEvidenceStatus,
    evidenceCounts: {
      linkedOutputs: ledger.length,
      completed: completedCount,
      needsReview: needsReviewCount,
      drafts: draftCount,
    },
    toolSummaries,
  };
}
