import { isLowData } from "./duration";
import { ExperienceSettingsLike } from "./hourTimelineViewModel";
import { TodaySnapshotLike } from "./todayViewModel";

export function buildReviewView(snapshot: TodaySnapshotLike | null | undefined, _settings?: ExperienceSettingsLike | null) {
  const idleItems = (snapshot?.idleBlocks ?? [])
    .filter((block) => !block.classified)
    .map((block, index) => ({
      id: `idle-${index}`,
      title: "Long idle gap needs classification",
      detail: "Confirm whether this was break time, meeting time, or offline work.",
      priority: "medium",
    }));
  const loopItems = (snapshot?.unclosedLoopInbox ?? []).map((item, index) => ({
    id: `review-${index}`,
    title: typeof item === "object" && item && "title" in item ? String((item as { title: unknown }).title) : "Session needs review",
    detail: typeof item === "object" && item && "detail" in item ? String((item as { detail: unknown }).detail) : "Review the related activity.",
    priority: "medium",
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
