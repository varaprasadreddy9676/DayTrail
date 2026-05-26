export const LOW_DATA_THRESHOLD_MS = 5 * 60 * 1000;

export function clampDurationMs(durationMs: unknown): number {
  return typeof durationMs === "number" && Number.isFinite(durationMs)
    ? Math.max(0, durationMs)
    : 0;
}

export function formatDuration(durationMs = 0): string {
  const totalSeconds = Math.max(0, Math.round(durationMs / 1_000));

  if (totalSeconds < 60) {
    return totalSeconds === 0 ? "0s" : `${totalSeconds}s`;
  }

  const totalMinutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;

  if (hours === 0) {
    return seconds === 0 ? `${totalMinutes}m` : `${totalMinutes}m ${seconds}s`;
  }

  if (minutes === 0 && seconds === 0) return `${hours}h`;
  return `${hours}h ${minutes}m`;
}

export function totalTrackedMs(snapshot: {
  appUsageSummary?: { totalDurationMs?: number } | null;
  sourceEvents?: Array<{ durationMs?: number }> | null;
  workSessions?: Array<{ durationMs?: number }> | null;
} | null | undefined): number {
  if (!snapshot) return 0;

  const appTotal = clampDurationMs(snapshot.appUsageSummary?.totalDurationMs);
  if (appTotal > 0) return appTotal;

  const sessionTotal = (snapshot.workSessions ?? []).reduce(
    (sum, session) => sum + clampDurationMs(session.durationMs),
    0,
  );
  if (sessionTotal > 0) return sessionTotal;

  return (snapshot.sourceEvents ?? []).reduce(
    (sum, event) => sum + clampDurationMs(event.durationMs),
    0,
  );
}

export function isLowData(snapshot: Parameters<typeof totalTrackedMs>[0]): boolean {
  return totalTrackedMs(snapshot) < LOW_DATA_THRESHOLD_MS;
}
