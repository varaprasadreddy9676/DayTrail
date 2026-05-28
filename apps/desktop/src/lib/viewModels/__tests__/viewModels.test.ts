import { describe, expect, it } from "vitest";
import { buildAiImpactView } from "../aiImpactViewModel";
import { buildRangeSummaryView } from "../rangeSummaryViewModel";
import { buildReviewView } from "../reviewViewModel";

describe("buildAiImpactView", () => {
  it("uses evidence-based AI impact labels", () => {
    const view = buildAiImpactView({
      sourceEvents: [
        {
          id: "ai-event",
          app: "Google Chrome",
          title: "ChatGPT - draft reply",
          domain: "chatgpt.com",
          durationMs: 60_000,
        },
      ],
      workSessions: [
        {
          id: "session-1",
          title: "Client reply",
          aiUsed: true,
          evidenceEventIds: ["ai-event"],
        },
      ],
      aiUsageSummary: {
        totalDurationMs: 60_000,
        tools: [{ tool: "ChatGPT", durationMs: 60_000 }],
      },
      aiOutputLedger: [
        {
          tool: "ChatGPT",
          title: "Draft reply",
          status: "needs_review",
          durationMs: 60_000,
        },
      ],
    });

    expect(view.evidenceStatus).toBe("Needs review");
    expect(view.evidenceCounts.observed).toBe(1);
    expect(view.evidenceCounts.linkedToWork).toBe(1);
    expect(view.evidenceCounts.linkedOutputs).toBe(1);
    expect(view.toolSummaries[0].label).toBe("Needs review");
  });
});

describe("buildReviewView", () => {
  it("keeps source, reason, action, and evidence counts on review items", () => {
    const view = buildReviewView({
      unclosedLoopInbox: [
        {
          id: "loop-1",
          category: "pending_reply",
          title: "Reply to client",
          detail: "A client message is unanswered.",
          source: "Slack",
          risk: "high",
          status: "open",
          primaryAction: "Reply",
          evidenceIds: ["event-1", "event-2"],
        },
      ],
    });

    expect(view.items[0]).toMatchObject({
      id: "loop-1",
      source: "Slack",
      reason: "A client message is unanswered.",
      primaryAction: "Reply",
      evidenceCount: 2,
    });
  });
});

describe("buildRangeSummaryView", () => {
  it("summarizes captured work across a date range", () => {
    const view = buildRangeSummaryView({
      sourceEvents: [
        { id: "event-1", app: "VS Code", durationMs: 120_000 },
        { id: "event-2", app: "Slack", durationMs: 60_000 },
      ],
      timesheetRows: [
        { id: "row-1", app: "VS Code", durationMs: 120_000 },
      ],
      workSessions: [{ id: "session-1", durationMs: 120_000 }],
      aiContributionRows: [
        { id: "ai-1", tool: "ChatGPT", status: "completed", durationMs: 30_000 },
      ],
      unclosedLoopInbox: [{}],
    });

    expect(view.empty).toBe(false);
    expect(view.sessionCount).toBe(1);
    expect(view.aiOutputCount).toBe(1);
    expect(view.needsReviewCount).toBe(1);
    expect(view.topApps[0].label).toBe("VS Code");
    expect(view.topAiTools[0].label).toBe("ChatGPT");
  });
});
