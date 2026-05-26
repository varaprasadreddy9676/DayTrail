import { describe, expect, it } from "vitest";

import { classifyApp, isSimpleVisibleApp, normalizeAppCategory } from "../src/lib/viewModels/appClassification";
import { buildActivityView } from "../src/lib/viewModels/activityViewModel";
import { buildAiImpactView } from "../src/lib/viewModels/aiImpactViewModel";
import { buildHourTimelineView } from "../src/lib/viewModels/hourTimelineViewModel";
import { buildReportView } from "../src/lib/viewModels/reportViewModel";
import { buildReviewView } from "../src/lib/viewModels/reviewViewModel";
import { buildTodayView } from "../src/lib/viewModels/todayViewModel";

const baseSettings = {
  experienceMode: "simple" as const,
  showSystemApps: false,
  showRawEvents: false,
  showCaptureConfidence: false,
  showAiDetails: "summary" as const,
};

const now = Date.UTC(2026, 4, 26, 7, 30);

function snapshot(overrides: Record<string, unknown> = {}) {
  return {
    localDate: "2026-05-26",
    workSessions: [],
    sourceEvents: [],
    idleBlocks: [],
    unclosedLoopInbox: [],
    aiUsageSummary: {
      totalDurationMs: 0,
      tools: [],
      contexts: [],
      outputCount: 0,
    },
    appUsageSummary: {
      totalDurationMs: 0,
      apps: [],
    },
    captureHealth: {
      status: "healthy",
      headline: "Capture active",
      updatedAt: now,
      checks: [],
    },
    settings: baseSettings,
    ...overrides,
  };
}

describe("Simple Mode view models", () => {
  it("classifies system apps and excludes them from simple top work app", () => {
    expect(classifyApp("System Settings")).toBe("system");
    expect(isSimpleVisibleApp("System Settings")).toBe(false);

    const view = buildTodayView(
      snapshot({
        sourceEvents: [
          {
            id: "system-settings",
            app: "System Settings",
            title: "Accessibility",
            startedAt: now,
            endedAt: now + 60_000,
            durationMs: 60_000,
          },
        ],
        appUsageSummary: {
          totalDurationMs: 60_000,
          apps: [
            {
              app: "System Settings",
              durationMs: 60_000,
              events: 1,
              projects: [],
              aiTools: [],
              files: [],
            },
          ],
        },
      }),
      baseSettings,
    );

    expect(view.topWorkApp?.name).toBeUndefined();
    expect(view.topWorkAppFallback).toBe("No work app yet");
  });

  it("returns low-data states across simple screens", () => {
    const lowDataSnapshot = snapshot({
      sourceEvents: [
        {
          id: "short-vscode",
          app: "VS Code",
          title: "DayTrail",
          startedAt: now,
          endedAt: now + 38_000,
          durationMs: 38_000,
        },
      ],
      appUsageSummary: {
        totalDurationMs: 38_000,
        apps: [
          {
            app: "VS Code",
            durationMs: 38_000,
            events: 1,
            projects: [],
            aiTools: [],
            files: [],
          },
        ],
      },
    });

    expect(buildTodayView(lowDataSnapshot, baseSettings).lowData).toBe(true);
    expect(buildActivityView(lowDataSnapshot, baseSettings).lowDataMessage).toMatch(/after more work/i);
    expect(buildAiImpactView(lowDataSnapshot, baseSettings).lowDataMessage).toMatch(/not enough activity/i);
    expect(buildReportView(lowDataSnapshot, baseSettings, "").lowDataMessage).toMatch(/at least one work session/i);
    expect(buildReviewView(lowDataSnapshot, baseSettings).lowDataMessage).toMatch(/more activity/i);
  });

  it("builds multi-segment hour rows and hides raw details in simple mode", () => {
    const view = buildHourTimelineView(
      [
        {
          id: "vscode",
          app: "VS Code",
          title: "App.tsx",
          workspaceKey: "/repo/daytrail",
          startedAt: new Date(2026, 4, 26, 9, 0).getTime(),
          endedAt: new Date(2026, 4, 26, 9, 20).getTime(),
          durationMs: 1_200_000,
        },
        {
          id: "chatgpt",
          app: "ChatGPT",
          title: "ChatGPT",
          workspaceKey: "chatgpt.com",
          startedAt: new Date(2026, 4, 26, 9, 20).getTime(),
          endedAt: new Date(2026, 4, 26, 9, 30).getTime(),
          durationMs: 600_000,
        },
      ],
      baseSettings,
    );

    const nineAm = view.hours.find((hour) => hour.hour === 9);
    expect(nineAm?.segments.map((segment) => segment.appName)).toEqual(["VS Code", "ChatGPT"]);
    expect(nineAm?.rawItems).toEqual([]);
    expect(nineAm?.aiTools).toContain("ChatGPT");
  });

  it("uses simple AI impact labels and does not imply generated or accepted output", () => {
    const view = buildAiImpactView(
      snapshot({
        sourceEvents: [
          {
            id: "codex",
            app: "VS Code",
            title: "DayTrail Codex",
            workspaceKey: "/repo/daytrail",
            startedAt: now,
            endedAt: now + 120_000,
            durationMs: 120_000,
          },
        ],
        workSessions: [
          {
            id: "session-1",
            title: "DayTrail development",
            durationMs: 120_000,
            evidenceEventIds: ["codex"],
          },
        ],
        aiUsageSummary: {
          totalDurationMs: 120_000,
          tools: [{ tool: "Codex", durationMs: 120_000, events: 1, contexts: ["/repo/daytrail"] }],
          contexts: [],
          outputCount: 0,
        },
      }),
      baseSettings,
    );

    expect(view.toolSummaries[0]).toMatchObject({ tool: "Codex", label: "Used with session" });
    expect(view.toolSummaries.map((tool) => tool.label).join(" ")).not.toMatch(/accepted|generated|agent completed/i);
  });

  it("exposes technical details only in pro mode", () => {
    const simple = buildActivityView(
      snapshot({
        sourceEvents: [
          {
            id: "raw-1",
            app: "VS Code",
            title: "App.tsx",
            source: "active-window",
            eventType: "active_window",
            startedAt: now,
            endedAt: now + 600_000,
            durationMs: 600_000,
          },
        ],
      }),
      baseSettings,
    );
    const pro = buildActivityView(simple.snapshot, {
      ...baseSettings,
      experienceMode: "pro",
      showRawEvents: true,
    });

    expect(simple.showTechnicalDetails).toBe(false);
    expect(simple.technicalItems).toHaveLength(0);
    expect(pro.showTechnicalDetails).toBe(true);
    expect(pro.technicalItems).toHaveLength(1);
  });

  it("uses backend app categories when available", () => {
    expect(normalizeAppCategory("system", "VS Code")).toBe("system");
    expect(isSimpleVisibleApp("VS Code", "system")).toBe(false);
    expect(isSimpleVisibleApp("Unknown Internal Helper", "work")).toBe(true);
  });

  it("enriches simple activity sessions with time, apps, projects, and quality", () => {
    const view = buildActivityView(
      snapshot({
        sourceEvents: [
          {
            id: "event-1",
            app: "VS Code",
            title: "App.tsx",
            workspaceKey: "/repo/daytrail",
            startedAt: now,
            endedAt: now + 300_000,
            durationMs: 300_000,
          },
          {
            id: "event-2",
            app: "ChatGPT",
            title: "ChatGPT",
            domain: "chatgpt.com",
            startedAt: now + 300_000,
            endedAt: now + 420_000,
            durationMs: 120_000,
          },
        ],
        workSessions: [
          {
            id: "session-1",
            title: "DayTrail development",
            startedAt: now,
            endedAt: now + 420_000,
            durationMs: 420_000,
            confidencePercent: 92,
            evidenceEventIds: ["event-1", "event-2"],
          },
        ],
      }),
      baseSettings,
    );

    expect(view.sessions[0].timeRangeLabel).toContain("-");
    expect(view.sessions[0].mainApps.map((app) => app.label)).toEqual(["VS Code", "ChatGPT"]);
    expect(view.sessions[0].projects.map((project) => project.label)).toEqual(["/repo/daytrail", "chatgpt.com"]);
    expect(view.sessions[0].qualityLabel).toBe("Clear session");
  });

  it("keeps system and utility apps out of simple reports", () => {
    const view = buildReportView(
      snapshot({
        workSessions: [
          {
            id: "session-1",
            title: "DayTrail development",
            durationMs: 600_000,
          },
        ],
        appUsageSummary: {
          totalDurationMs: 780_000,
          apps: [
            { app: "System Settings", durationMs: 60_000, events: 1, projects: [], aiTools: [], files: [] },
            { app: "Problem Reporter", durationMs: 20_000, events: 1, projects: [], aiTools: [], files: [] },
            { app: "Finder", durationMs: 100_000, events: 1, projects: [], aiTools: [], files: [] },
            { app: "VS Code", durationMs: 600_000, events: 4, projects: [], aiTools: [], files: [] },
          ],
        },
      }),
      baseSettings,
      "",
    );

    expect(view.includedWork.apps).toBe(1);
    expect(view.markdown).toContain("## What happened");
    expect(view.markdown).toContain("- VS Code");
    expect(view.markdown).not.toMatch(/System Settings|Problem Reporter|Finder/);
  });

  it("keeps AI impact honest about evidence status", () => {
    const detectedOnly = buildAiImpactView(
      snapshot({
        aiUsageSummary: {
          totalDurationMs: 60_000,
          tools: [{ tool: "Codex", durationMs: 60_000, events: 1, contexts: [] }],
          contexts: [],
          outputCount: 0,
        },
      }),
      baseSettings,
    );
    const completed = buildAiImpactView(
      snapshot({
        aiUsageSummary: {
          totalDurationMs: 60_000,
          tools: [{ tool: "Codex", durationMs: 60_000, events: 1, contexts: [] }],
          contexts: [],
          outputCount: 1,
        },
        aiOutputLedger: [{ tool: "Codex", status: "completed", title: "Patch", durationMs: 60_000 }],
      }),
      baseSettings,
    );

    expect(detectedOnly.evidenceStatus).toBe("Detected only");
    expect(completed.evidenceStatus).toBe("Completed");
    expect(JSON.stringify(detectedOnly)).not.toMatch(/accepted|generated|agent completed/i);
  });
});
