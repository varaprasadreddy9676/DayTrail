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
      source: "Idle recovery",
      reason: "DayTrail found an unlabeled gap and should not guess what it means.",
      primaryAction: "Classify",
      evidenceCount: 1,
      priority: "medium" as const,
    }));
  const loopItems = (snapshot?.unclosedLoopInbox ?? []).map((item, index) => ({
    id: item.id ?? `review-${index}`,
    title: item.title ?? "Session needs review",
    detail: item.detail ?? "Review the related activity.",
    source: item.source ?? label(item.category),
    reason: item.detail ?? "DayTrail found source evidence that needs a decision.",
    primaryAction: item.primaryAction ?? "Review",
    evidenceCount: item.evidenceIds?.length ?? 0,
    priority: (item.risk ?? "medium").toLowerCase(),
    status: item.status ?? "open",
  }));

  return {
    lowDataMessage: isLowData(snapshot)
      ? "Needs Review becomes more useful after more activity is captured."
      : undefined,
    items: [...loopItems, ...idleItems],
    emptyTitle: "Nothing needs review yet",
    emptyCopy: "DayTrail will show unclear work sessions, long idle gaps, AI activity without clear context, draft timesheet sessions, and unfinished report inputs here.",
  };
}
