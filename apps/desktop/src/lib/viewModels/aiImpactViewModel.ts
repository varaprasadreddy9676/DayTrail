import { formatDuration, isLowData } from "./duration";
import {
  detectAiToolsFromEvent,
  ExperienceSettingsLike,
  normalizeExperienceSettings,
  SourceEventLike,
} from "./hourTimelineViewModel";
import { TodaySnapshotLike } from "./todayViewModel";

export type SimpleAiImpactLabel =
  | "Observed"
  | "Linked to work"
  | "Generated output"
  | "Accepted/completed"
  | "Needs review";
export type AiEvidenceStatus = SimpleAiImpactLabel;

function eventContainsTool(event: SourceEventLike, tool: string): boolean {
  return detectAiToolsFromEvent(event).includes(tool);
}

function labelForTool(
  tool: string,
  events: SourceEventLike[],
  sessions: Array<{ evidenceEventIds?: string[]; aiUsed?: boolean }> = [],
  ledger: Array<{ status?: string | null; tool?: string | null }> = [],
): SimpleAiImpactLabel {
  const eventIdsForTool = new Set(events.filter((event) => eventContainsTool(event, tool)).map((event) => event.id).filter(Boolean));
  const matchingLedger = ledger.filter((item) => (item.tool ?? "").toLowerCase() === tool.toLowerCase());
  if (
    matchingLedger.some((item) => ["needs_review", "review", "failed", "blocked"].includes((item.status ?? "").toLowerCase()))
  ) {
    return "Needs review";
  }

  if (
    matchingLedger.some((item) => ["sent", "shared", "completed", "accepted", "done"].includes((item.status ?? "").toLowerCase()))
  ) {
    return "Accepted/completed";
  }

  if (matchingLedger.length > 0) {
    return "Generated output";
  }

  if (
    eventIdsForTool.size > 0 &&
    sessions.some((session) => (session.evidenceEventIds ?? []).some((id) => eventIdsForTool.has(id)))
  ) {
    return "Linked to work";
  }

  return "Observed";
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
  const aiEventIds = new Set(
    events
      .filter((event) => detectAiToolsFromEvent(event).length > 0)
      .map((event) => event.id)
      .filter(Boolean),
  );
  const linkedSessionCount = sessions.filter(
    (session) => session.aiUsed || (session.evidenceEventIds ?? []).some((id) => aiEventIds.has(id)),
  ).length;
  const toolSummaries = tools.map((tool) => ({
    tool: tool.tool,
    durationMs: tool.durationMs ?? 0,
    durationLabel: formatDuration(tool.durationMs ?? 0),
    label: labelForTool(tool.tool, events, sessions, ledger),
  }));
  const mostActive = toolSummaries.slice().sort((left, right) => right.durationMs - left.durationMs)[0];
  const observedCount = events.filter((event) => detectAiToolsFromEvent(event).length > 0).length;

  return {
    lowDataMessage: isLowData(snapshot)
      ? "AI tools detected, but not enough activity for a useful breakdown."
      : undefined,
    toolsDetected: toolSummaries.map((tool) => tool.tool),
    usedMostlyWith: snapshot?.workSessions?.find((session) => session.aiUsed)?.title ?? snapshot?.appUsageSummary?.apps?.[0]?.app ?? "No session yet",
    mostActiveTool: mostActive?.tool ?? null,
    confidenceLabel: normalizedSettings.experienceMode === "pro" ? "Detailed evidence" : "Simple evidence",
    evidenceStatus: needsReviewCount > 0
      ? "Needs review" as AiEvidenceStatus
      : completedCount > 0
        ? "Accepted/completed" as AiEvidenceStatus
        : ledger.length > 0
          ? "Generated output" as AiEvidenceStatus
          : linkedSessionCount > 0
            ? "Linked to work" as AiEvidenceStatus
            : "Observed" as AiEvidenceStatus,
    evidenceCounts: {
      observed: observedCount,
      linkedToWork: linkedSessionCount,
      linkedOutputs: ledger.length,
      completed: completedCount,
      needsReview: needsReviewCount,
      drafts: draftCount,
    },
    toolSummaries,
  };
}
