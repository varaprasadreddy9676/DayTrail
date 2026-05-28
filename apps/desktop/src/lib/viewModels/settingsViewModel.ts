import { ExperienceSettingsLike, normalizeExperienceSettings } from "./hourTimelineViewModel";

export function buildSettingsView(settings?: ExperienceSettingsLike | null) {
  const normalized = normalizeExperienceSettings(settings);

  return {
    mode: normalized.experienceMode,
    showSystemApps: normalized.showSystemApps,
    showRawEvents: normalized.showRawEvents,
    showCaptureConfidence: normalized.showCaptureConfidence,
    showAiDetails: normalized.showAiDetails,
    sections: ["Mode", "Capture Health", "Privacy", "AI Provider", "Advanced"],
    privacyBadges: ["Metadata-first capture", "Clipboard not stored", "File contents not read"],
  };
}
