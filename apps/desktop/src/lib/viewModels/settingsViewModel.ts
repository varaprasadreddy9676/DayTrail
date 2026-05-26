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
    privacyBadges: ["Metadata-first capture", "Screenshots off", "Clipboard not stored"],
  };
}
