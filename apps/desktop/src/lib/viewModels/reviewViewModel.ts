import { isLowData } from "./duration";
import { ExperienceSettingsLike } from "./hourTimelineViewModel";
import { TodaySnapshotLike } from "./todayViewModel";

function label(value: string | null | undefined, fallback = "Review") {
  return (value || fallback)
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

export function buildReviewView(snapshot: TodaySnapshotLike | null | undefined, _settings?: ExperienceSettingsLike | null) {
  const idleItems = (snapshot?.idleBlocks ?? [])
    .filter((block) => !block.classified)
    .map((block, index) => ({
      id: `idle-${block.id ?? index}`,
      title: "Long idle gap needs classification",
      detail: "Confirm whether this was break time, meeting time, or offline work.",
      source: "Away time",
      reason: "DayTrail found an unlabeled gap and should not guess what it means.",
      primaryAction: "Classify",
      evidenceCount: 1,
      priority: "medium" as const,
    }));
  const loopItems = (snapshot?.unclosedLoopInbox ?? []).map((item, index) => ({
    id: item.id ?? `review-${index}`,
    title: item.title ?? "Session needs a decision",
    detail: item.detail ?? "Review the related activity.",
    source: item.source ?? label(item.category),
    reason: item.detail ?? "A local source record needs a decision.",
    primaryAction: item.primaryAction ?? "Review",
    evidenceCount: item.evidenceIds?.length ?? 0,
    priority: (item.risk ?? "medium").toLowerCase(),
    status: item.status ?? "open",
  }));

  return {
    lowDataMessage: isLowData(snapshot)
      ? "Review Queue becomes more useful after more activity is captured."
      : undefined,
    items: [...loopItems, ...idleItems],
    emptyTitle: "No decisions waiting",
    emptyCopy: "Tasks, AI drafts, saved promises, source-marked replies, meeting actions, and away-time gaps appear here only when DayTrail has a local record to review.",
  };
}
