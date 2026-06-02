import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, vi } from "vitest";

import App from "../src/App";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.__TAURI__ = undefined;
  window.__TAURI_INTERNALS__ = undefined;
  if (typeof window.localStorage?.clear === "function") {
    window.localStorage.clear();
  }
  if (typeof window.sessionStorage?.clear === "function") {
    window.sessionStorage.clear();
  }
});

async function openAiSettings(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /^settings$/i }));
  await user.click(screen.getByRole("button", { name: /ai provider/i }));
}

describe("DayTrail command center", () => {
  it("renders the native today shell and switches to app usage", async () => {
    const user = userEvent.setup();

    render(<App />);

    expect(screen.getByRole("heading", { name: /^today$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^today$/i })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.getByLabelText(/today stats/i)).toBeInTheDocument();
    expect(screen.getByText(/desktop bridge not connected/i)).toBeInTheDocument();
    expect(screen.getByText(/no work app yet/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^activity$/i }));

    expect(
      screen.getByRole("heading", { level: 2, name: /activity/i }),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/no sessions yet/i).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /^activity$/i })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("renders Smart Recovery and records simple actions", async () => {
    const user = userEvent.setup();
    const settings = {
      browserBridgeEnabled: true,
      excludedDomains: [],
      aiProvider: "Ollama Local",
      aiModel: "llama3.1",
      aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
      aiRedactSecrets: true,
      fullClipboardHistory: false,
    };
    const invoke = vi.fn(async (command: string) => {
      if (command === "today") {
        return {
          localDate: "2026-06-02",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          calendarEvents: [],
          calendarReconciliation: {
            plannedEvents: 0,
            matchedEvents: 0,
            unmatchedEvents: 0,
            plannedDurationMs: 0,
            actualOverlapMs: 0,
            items: [],
          },
          focusSessions: [],
          recoverySummary: {
            score: 82,
            totalScreenMs: 3_600_000,
            longestUninterruptedMs: 42 * 60_000,
            currentStreakMs: 31 * 60_000,
            takenCount: 2,
            skippedCount: 1,
            snoozedCount: 1,
            promptedCount: 3,
            nextPrompt: {
              action: "due",
              reason: "Long uninterrupted screen run",
              streakMs: 31 * 60_000,
              suggestedMinutes: 3,
            },
            recentEvents: [],
          },
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          sourceEvents: [],
          workSessions: [],
          parallelStreams: [],
          aiUsageSummary: { totalDurationMs: 0, tools: [], contexts: [], outputCount: 0 },
          appUsageSummary: { totalDurationMs: 0, apps: [] },
          automationCandidates: [],
          unclosedLoopInbox: [],
          aiOutputLedger: [],
          loopRisks: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings,
          projectContext: null,
          activeWorkContext: null,
        };
      }

      if (["take_recovery_break", "snooze_recovery", "skip_recovery"].includes(command)) {
        return { id: `event-${command}` };
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByLabelText(/smart recovery/i)).toBeInTheDocument();
    expect(screen.getByText(/break due/i)).toBeInTheDocument();
    expect(screen.getByText(/31m current.*2 taken/i)).toBeInTheDocument();
    expect(screen.getByText(/longest 42m.*1 skipped/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /take break/i }));
    await user.click(screen.getByRole("button", { name: /snooze/i }));
    await user.click(screen.getByRole("button", { name: /skip/i }));

    expect(invoke).toHaveBeenCalledWith("take_recovery_break", { minutes: 3 });
    expect(invoke).toHaveBeenCalledWith("snooze_recovery", { minutes: 5 });
    expect(invoke).toHaveBeenCalledWith("skip_recovery", undefined);
  });

  it("monkey-clicks the primary navigation without extra onboarding burden", async () => {
    const user = userEvent.setup();

    render(<App />);

    expect(screen.queryByText(/view sample day/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/keep daytrail running while you work/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/^from$/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /app range custom range/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^activity$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /activity/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^ai impact$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /ai impact/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^needs review$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /needs review/i })).toBeInTheDocument();
    expect(screen.getAllByText(/source-backed/i).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /^reports$/i }));
    expect(screen.getAllByRole("heading", { level: 2, name: /reports/i }).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /^settings$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /settings/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^today$/i }));
    expect(screen.getAllByText(/today timeline/i).length).toBeGreaterThan(0);
  });

  it("manages overall tasks and reminders from Today", async () => {
    const user = userEvent.setup();
    const settings = { browserBridgeEnabled: true, excludedDomains: [] };
    const openTask = {
      id: 42,
      title: "Renew vendor contract",
      status: "open",
      dueDate: "2026-06-02",
      dueAt: new Date("2026-06-02T15:30:00").getTime(),
      notes: "Confirm budget owner first",
      priority: "high",
      source: "manual",
      projectPath: null,
      clientLabel: "Ops",
      projectLabel: "Vendors",
      reminderSentAt: null,
      createdAt: "2026-06-02T09:00:00Z",
      updatedAt: "2026-06-02T09:00:00Z",
    };
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-06-02",
          tasks: [openTask],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          sourceEvents: [],
          workSessions: [],
          parallelStreams: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings,
          projectContext: null,
        };
      }
      if (command === "create_task") {
        return { ...openTask, id: 43, title: (args?.input as { title: string }).title };
      }
      if (command === "complete_task") {
        return { ...openTask, status: "done" };
      }
      if (command === "snooze_task") {
        return { ...openTask, dueAt: args?.dueAt as number };
      }
      if (command === "delete_task") {
        return { deletedRows: 1 };
      }
      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByRole("heading", { name: /tasks & reminders/i })).toBeInTheDocument();
    expect(screen.getAllByText(/renew vendor contract/i).length).toBeGreaterThan(0);
    expect(screen.getByText(/confirm budget owner first/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /add task/i }));
    await user.type(screen.getByLabelText(/^title$/i), "Send invoice follow-up");
    await user.type(screen.getByLabelText(/^notes$/i), "Ask whether PO is approved");
    fireEvent.change(screen.getByLabelText(/due date/i), { target: { value: "2026-06-03" } });
    fireEvent.change(screen.getByLabelText(/due time/i), { target: { value: "10:15" } });
    await user.click(screen.getByRole("button", { name: /save task/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_task", {
        input: expect.objectContaining({
          title: "Send invoice follow-up",
          notes: "Ask whether PO is approved",
          priority: "medium",
          source: "manual",
        }),
      }),
    );

    await user.click(screen.getByRole("button", { name: /complete renew vendor contract/i }));
    expect(invoke).toHaveBeenCalledWith("complete_task", { id: 42 });

    await user.click(screen.getByRole("button", { name: /snooze renew vendor contract/i }));
    expect(invoke).toHaveBeenCalledWith("snooze_task", {
      id: 42,
      dueAt: expect.any(Number),
    });

    await user.click(screen.getByRole("button", { name: /delete renew vendor contract/i }));
    expect(invoke).toHaveBeenCalledWith("delete_task", { id: 42 });
  });

  it("shows the 24-hour timeline for a selected single-day range", async () => {
    const user = userEvent.setup();
    const settings = {
      browserBridgeEnabled: true,
      excludedDomains: [],
      aiProvider: "Ollama Local",
      aiModel: "llama3.1",
      aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
      aiRedactSecrets: true,
      fullClipboardHistory: false,
    };
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-05-28",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          sourceEvents: [],
          workSessions: [],
          parallelStreams: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings,
          projectContext: null,
        };
      }

      if (command === "export_data_range") {
        const range = args?.range as { fromDate: string; toDate: string };
        const startedAt = new Date(`${range.fromDate}T08:15:00`).getTime();
        const endedAt = startedAt + 15 * 60_000;
        return {
          generatedAt: "2026-05-28T09:00:00Z",
          fromDate: range.fromDate,
          toDate: range.toDate,
          timesheetRows: [],
          aiContributionRows: [],
          calendarEvents: [],
          calendarReconciliation: {
            plannedEvents: 0,
            matchedEvents: 0,
            unmatchedEvents: 0,
            plannedDurationMs: 0,
            actualOverlapMs: 0,
            items: [],
          },
          focusSessions: [],
          recoverySummary: {
            score: 76,
            totalScreenMs: endedAt - startedAt,
            longestUninterruptedMs: endedAt - startedAt,
            currentStreakMs: 0,
            takenCount: 1,
            skippedCount: 0,
            snoozedCount: 0,
            promptedCount: 1,
            nextPrompt: {
              action: "ready",
              reason: "Recovery rhythm is available",
              streakMs: 0,
              suggestedMinutes: 3,
            },
            recentEvents: [],
          },
          recoveryEvents: [],
          sourceEvents: [
            {
              id: "single-day-vscode",
              source: "active-window",
              eventType: "editor",
              app: "VS Code",
              title: "yesterday.ts - DayTrail",
              domain: null,
              urlRedacted: null,
              workspaceKey: "/repo/daytrail",
              startedAt,
              endedAt,
              durationMs: endedAt - startedAt,
              sensitivity: "normal",
              metadataJson: null,
              createdAt: endedAt,
            },
          ],
          workSessions: [],
          idleBlocks: [],
          aiUsage: [],
          appUsageSummary: {
            totalDurationMs: endedAt - startedAt,
            apps: [
              {
                app: "VS Code",
                category: "editor",
                durationMs: endedAt - startedAt,
                events: 1,
                projects: [],
                aiTools: [],
                files: [],
              },
            ],
          },
          aiUsageSummary: { totalDurationMs: 0, tools: [], contexts: [], outputCount: 0 },
          automationCandidates: [],
          unclosedLoopInbox: [],
          settings,
          pauseState: { paused: false },
          projectContext: null,
          activeWorkContext: null,
        };
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    await user.click(await screen.findByRole("button", { name: /app range yesterday/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith(
        "export_data_range",
        expect.objectContaining({
          range: expect.objectContaining({
            fromDate: expect.any(String),
            toDate: expect.any(String),
          }),
        }),
      ),
    );
    expect(await screen.findByRole("heading", { name: /what happened yesterday/i })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: /^yesterday$/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 2, name: /vs code/i })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /24-hour timeline/i })).toBeInTheDocument();
    expect(await screen.findByText(/showing 1 active hour/i)).toBeInTheDocument();
    expect(screen.queryByLabelText(/selected range summary/i)).not.toBeInTheDocument();
    expect(screen.getByLabelText(/smart recovery/i)).toBeInTheDocument();
    expect(screen.queryByText(/range summary/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /take break/i })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /app range last 7 days/i }));

    await waitFor(() =>
      expect(screen.getAllByRole("heading", { level: 2, name: /^last 7 days$/i }).length).toBeGreaterThan(0),
    );
    expect(screen.queryByRole("heading", { level: 2, name: /vs code/i })).not.toBeInTheDocument();
    expect(await screen.findByLabelText(/selected range summary/i)).toBeInTheDocument();
  });

  it("shows terminal bridge capability labels as Terminal", async () => {
    // Use the current local day so the event is treated as "today" — the app
    // filters the timeline to today's events, so a hard-coded past date makes
    // this test silently rot (it only passed on 2026-05-28).
    const base = new Date();
    base.setHours(9, 15, 0, 0);
    const now = base.getTime();
    const localDate = `${base.getFullYear()}-${String(base.getMonth() + 1).padStart(2, "0")}-${String(base.getDate()).padStart(2, "0")}`;
    const invoke = vi.fn(async (command: string) => {
      if (command !== "today") {
        return null;
      }

      return {
        localDate,
        tasks: [],
        quickNotes: [],
        commitments: [],
        pendingReplies: [],
        aiOutputs: [],
        meetings: [],
        fieldVisits: [],
        idleBlocks: [],
        sourceEvents: [
          {
            id: "terminal-dumb-env",
            source: "terminal-bridge",
            eventType: "command",
            app: "dumb",
            title: "printf daytrail qa --api-key [redacted]",
            domain: null,
            urlRedacted: null,
            workspaceKey: "/Users/alice/work/daytrail",
            startedAt: now - 60_000,
            endedAt: now,
            durationMs: 60_000,
            sensitivity: "normal",
            metadataJson: null,
            createdAt: now,
          },
        ],
        workSessions: [],
        parallelStreams: [],
        nextBestAction: null,
        pauseState: { paused: false },
        settings: { browserBridgeEnabled: true, excludedDomains: [] },
        projectContext: { path: "/Users/alice/work/daytrail", source: "terminal-bridge" },
      };
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect((await screen.findAllByText(/^terminal$/i)).length).toBeGreaterThan(0);
    expect(screen.queryByText(/^dumb$/i)).not.toBeInTheDocument();
  });

  it("toggles watcher status from the sidebar", async () => {
    const user = userEvent.setup();
    const invoke = vi.fn(async (command: string) => {
      if (command === "pause_tracking") {
        return { paused: true };
      }
      if (command === "resume_tracking") {
        return { paused: false };
      }
      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    await user.click(screen.getByRole("button", { name: /^capturing$/i }));

    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: /capture paused/i }),
      ).toBeInTheDocument(),
    );
  });

  it("updates AI settings and validates provider defaults", async () => {
    const user = userEvent.setup();

    render(<App />);

    await user.click(screen.getByRole("button", { name: /^settings$/i }));
    expect(
      screen.getByRole("heading", { level: 2, name: /^settings$/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/choose how much detail daytrail shows/i)).toBeInTheDocument();
    expect(screen.queryByText(/^unknown$/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/default capture policy/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /ai provider/i }));

    await user.selectOptions(screen.getByLabelText(/provider/i), "Gemini");
    expect(screen.getByLabelText(/model/i)).toHaveValue("gemini-flash-latest");
    await user.click(screen.getByText(/advanced endpoint/i));
    expect(screen.getByLabelText(/endpoint/i)).toHaveValue(
      "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent",
    );

    await user.selectOptions(
      screen.getByLabelText(/provider/i),
      "OpenAI Compatible",
    );
    await user.clear(screen.getByLabelText(/model/i));
    await user.type(screen.getByLabelText(/model/i), "gpt-4.1-mini");
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    expect(screen.getByText(/openai compatible ready/i)).toBeInTheDocument();
  });

  it("saves launch-at-login from capture settings", async () => {
    const user = userEvent.setup();
    const settings = {
      browserBridgeEnabled: true,
      excludedDomains: [],
      aiProvider: "Ollama Local",
      aiModel: "llama3.1",
      aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
      aiRedactSecrets: true,
      fullClipboardHistory: false,
      launchAtLogin: true,
    };
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-05-23",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          workSessions: [],
          parallelStreams: [],
          sourceEvents: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings,
          projectContext: null,
        };
      }

      if (command === "update_settings") {
        expect(args).toEqual({ patch: { launchAtLogin: false } });
        return { ...settings, launchAtLogin: false };
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByText(/waiting for captured activity/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^settings$/i }));
    await user.click(screen.getByRole("button", { name: /capture health/i }));
    expect(screen.getByText(/keeps tracking in tray/i)).toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: /launch at login/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: { launchAtLogin: false },
      }),
    );
    expect(await screen.findByText(/manual start/i)).toBeInTheDocument();
  });

  it("keeps reports generic and generates a daily report", async () => {
    const user = userEvent.setup();

    render(<App />);

    await user.click(screen.getByRole("button", { name: /^reports$/i }));

    expect(screen.getByText(/what will be summarized/i)).toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: /^generate$/i })[0]);
    expect(screen.getByLabelText(/generated report markdown/i).textContent).toMatch(
      /daily work report/i,
    );
    expect(screen.queryByRole("button", { name: /client update/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /weekly review/i })).not.toBeInTheDocument();
  });

  it("hydrates command-center data from the Tauri today snapshot", async () => {
    const user = userEvent.setup();
    const todayAtMidmorning = new Date();
    todayAtMidmorning.setHours(10, 30, 0, 0);
    const now = todayAtMidmorning.getTime();
    const invoke = vi.fn(async (command: string) => {
      if (command !== "today") {
        return null;
      }

      return {
        localDate: "2026-05-23",
        tasks: [],
        quickNotes: [
          {
            id: 17,
            body: "Backend note loaded from SQLite",
            source: "desktop-ui",
            projectPath: "SQLite workspace",
            createdAt: "2026-05-23T10:30:00+05:30",
          },
        ],
        commitments: [
          {
            id: "commitment-1",
            title: "Send sponsor update",
            source: "Meeting closure",
          },
        ],
        pendingReplies: [
          {
            id: "mail-1",
            subject: "Client billing answer",
            latestSender: "client@example.com",
          },
        ],
        aiOutputs: [
          {
            id: "output-1",
            title: "Review generated report",
            outputType: "report",
            status: "needs_review",
            aiAssisted: true,
          },
        ],
        meetings: [],
        fieldVisits: [],
        idleBlocks: [
          {
            id: "idle-1",
            durationMs: 1_800_000,
            classified: false,
          },
        ],
        workSessions: [
          {
            id: "session-1",
            title: "SQLite capture block",
            status: "Captured",
            startedAt: now - 900_000,
            endedAt: now,
            durationMs: 900_000,
            aiUsed: true,
            confidencePercent: 91,
            summary: "Loaded from local facts.",
            evidenceEventIds: ["event-vscode", "event-chatgpt"],
          },
        ],
        parallelStreams: [
          {
            id: "sqlite-stream",
            title: "SQLite workspace",
            status: "Active",
            startedAt: now - 900_000,
            endedAt: now,
            summary: "Loaded from the Tauri today command.",
            eventIds: ["event-1", "event-2"],
            nextAction: "Ship backend wiring",
          },
        ],
        sourceEvents: [
          {
            id: "event-vscode",
            source: "active-window",
            eventType: "active_window",
            app: "Code",
            title: "App.tsx - DayTrail",
            domain: null,
            urlRedacted: null,
            workspaceKey: "/Users/alice/work/daytrail",
            startedAt: now - 900_000,
            endedAt: now - 600_000,
            durationMs: 300_000,
            sensitivity: "normal",
            metadataJson: null,
            createdAt: now - 600_000,
          },
          {
            id: "event-chatgpt",
            source: "active-window",
            eventType: "active_window",
            app: "Google Chrome",
            title: "ChatGPT - DayTrail summary",
            domain: "chatgpt.com",
            urlRedacted: "https://chatgpt.com/c/thread",
            workspaceKey: "chatgpt.com",
            startedAt: now - 600_000,
            endedAt: now - 300_000,
            durationMs: 300_000,
            sensitivity: "normal",
            metadataJson: null,
            createdAt: now - 300_000,
          },
        ],
        aiUsageSummary: {
          totalDurationMs: 300_000,
          tools: [
            {
              tool: "ChatGPT",
              durationMs: 300_000,
              events: 1,
              contexts: ["chatgpt.com"],
            },
          ],
          contexts: [{ label: "chatgpt.com", durationMs: 300_000, events: 1 }],
          outputCount: 1,
        },
        appUsageSummary: {
          totalDurationMs: 600_000,
          apps: [
            {
              app: "VS Code",
              durationMs: 300_000,
              events: 1,
              aiTools: [],
              projects: [
                {
                  label: "daytrail",
                  durationMs: 300_000,
                  events: 1,
                  aiTools: [],
                  examples: ["App.tsx - DayTrail"],
                },
              ],
            },
            {
              app: "Google Chrome",
              durationMs: 300_000,
              events: 1,
              aiTools: [
                {
                  tool: "ChatGPT",
                  durationMs: 300_000,
                  events: 1,
                  contexts: ["chatgpt.com"],
                },
              ],
              projects: [
                {
                  label: "chatgpt.com",
                  durationMs: 300_000,
                  events: 1,
                  aiTools: [
                    {
                      tool: "ChatGPT",
                      durationMs: 300_000,
                      events: 1,
                      contexts: ["chatgpt.com"],
                    },
                  ],
                  examples: ["ChatGPT - DayTrail summary"],
                },
              ],
            },
          ],
        },
        automationCandidates: [
          {
            id: "automation-daytrail",
            title: "daytrail",
            signal: "Repeated app/project pattern",
            reason: "Repeated Today UI inspection",
            occurrences: 3,
            durationMs: 600_000,
            exampleSources: ["VS Code", "Google Chrome"],
          },
        ],
        nextBestAction: {
          title: "Ship backend wiring",
          reason: "The local store has a captured stream.",
          sourceType: "stream",
          sourceId: "sqlite-stream",
          priority: 1,
        },
        pauseState: { paused: true },
        settings: { browserBridgeEnabled: true, excludedDomains: [] },
        projectContext: { path: "/tmp/daytrail", source: "git" },
      };
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect((await screen.findAllByText(/sqlite capture block/i)).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/needs review/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/ship backend wiring/i).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /capture paused/i })).toBeInTheDocument();
    expect(screen.getAllByText(/ai detected/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/chatgpt/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/today timeline/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/google chrome/i).length).toBeGreaterThan(0);

    const dayTrackerRow = screen.getAllByText(/10 AM/i)
      .map((node) => node.closest("button"))
      .find((node): node is HTMLButtonElement => Boolean(node));
    if (!dayTrackerRow) {
      throw new Error("Expected active day tracker row");
    }
    fireEvent.contextMenu(dayTrackerRow);
    await user.click(screen.getByRole("button", { name: /mark selected time/i }));
    expect(screen.getByRole("heading", { level: 3, name: /mark time/i })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /cancel/i }));

    await user.click(screen.getAllByRole("button", { name: /sqlite capture block/i })[0]);

    expect(screen.getByText(/session details/i)).toBeInTheDocument();
    expect(screen.getAllByText(/app\.tsx - daytrail/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/https:\/\/chatgpt\.com\/c\/thread/i).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: /close session details/i }));

    await user.click(screen.getByRole("button", { name: /view full hour breakdown/i }));
    const chromeHourRow = screen.getAllByText(/google chrome/i)
      .map((node) => node.closest("button"))
      .find((node): node is HTMLButtonElement => Boolean(node?.getAttribute("aria-label")?.match(/breakdown/i)));
    if (!chromeHourRow) {
      throw new Error("Expected Google Chrome hour breakdown row");
    }
    await user.click(chromeHourRow);

    expect(screen.getByText(/app breakdown/i)).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 3, name: /google chrome/i })).toBeInTheDocument();
    expect(screen.getByText(/event timeline/i)).toBeInTheDocument();
    expect(screen.getAllByText(/chatgpt - daytrail summary/i).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /^activity$/i }));

    expect(
      screen.getByRole("heading", { level: 2, name: /activity/i }),
    ).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /^sessions$/i })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /open session/i }).length).toBeGreaterThan(0);
    await user.click(screen.getAllByRole("button", { name: /open session/i })[0]);
    expect(screen.getByText(/main apps/i)).toBeInTheDocument();
    expect(screen.getByText(/activity items/i)).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: /^apps$/i }));
    expect(screen.getByText(/top apps today/i)).toBeInTheDocument();
    expect(screen.getAllByText(/chatgpt\.com/i).length).toBeGreaterThan(0);
    expect(invoke).toHaveBeenCalledWith("today", undefined);
  });

  it("uses captured sessions as current work before app details are materialized", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command !== "today") {
        return null;
      }

      return {
        localDate: "2026-05-23",
        tasks: [],
        quickNotes: [],
        commitments: [],
        pendingReplies: [],
        aiOutputs: [],
        meetings: [],
        fieldVisits: [],
        idleBlocks: [],
        workSessions: [
          {
            id: "session-only",
            title: "Warp terminal in billing-api",
            status: "Captured",
            startedAt: Date.UTC(2026, 4, 23, 9, 0),
            endedAt: Date.UTC(2026, 4, 23, 9, 10),
            durationMs: 600_000,
            aiUsed: false,
            confidencePercent: 87,
            summary: "CLI folder context",
          },
        ],
        parallelStreams: [],
        nextBestAction: null,
        pauseState: { paused: false },
        settings: { browserBridgeEnabled: true, excludedDomains: [] },
        projectContext: { path: "/Users/alice/work/billing-api", source: "terminal-bridge" },
      };
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(
      (await screen.findAllByRole("heading", { name: /warp terminal in billing-api/i })).length,
    ).toBeGreaterThan(0);
    expect(screen.getAllByText(/\/users\/alice\/work\/billing-api/i).length).toBeGreaterThan(0);
  });

  it("persists AI settings and keeps reports generic", async () => {
    const user = userEvent.setup();
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-05-23",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          workSessions: [],
          parallelStreams: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings: {
            browserBridgeEnabled: true,
            excludedDomains: [],
            aiProvider: "Ollama Local",
            aiModel: "llama3.1",
            aiEndpoint: "http://127.0.0.1:11434/v1",
            aiRedactSecrets: true,
            fullClipboardHistory: false,
          },
          projectContext: null,
        };
      }

      if (command === "update_settings") {
        const patch = args?.patch as Record<string, unknown>;
        return {
          browserBridgeEnabled: true,
          excludedDomains: [],
          aiProvider: patch.aiProvider,
          aiModel: patch.aiModel,
          aiEndpoint: patch.aiEndpoint,
          aiRedactSecrets: patch.aiRedactSecrets,
          fullClipboardHistory: patch.fullClipboardHistory,
        };
      }

      if (command === "generate_daily_report") {
        return {
          bodyMarkdown: "# Daily Work Execution Report\n\n- Source-backed report",
        };
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByText(/waiting for captured activity/i)).toBeInTheDocument();

    await openAiSettings(user);
    await user.selectOptions(screen.getByLabelText(/provider/i), "OpenAI Compatible");
    await user.clear(screen.getByLabelText(/model/i));
    await user.type(screen.getByLabelText(/model/i), "gpt-4.1-mini");
    await user.click(screen.getByRole("button", { name: /save settings/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: expect.objectContaining({
          aiProvider: "OpenAI Compatible",
          aiModel: "gpt-4.1-mini",
        }),
      }),
    );
    expect(await screen.findByText(/openai compatible saved/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /daily report/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("generate_daily_report", undefined));

    await user.click(screen.getByRole("button", { name: /^reports$/i }));

    expect(screen.getByLabelText(/generated report markdown/i).textContent).toMatch(
      /daily work execution report/i,
    );
    expect(screen.queryByRole("button", { name: /weekly review/i })).not.toBeInTheDocument();
  });

  it("manages portable settings and database backup from storage settings", async () => {
    const user = userEvent.setup();
    vi.spyOn(window, "confirm").mockReturnValue(true);
    const exportedConfig = JSON.stringify(
      {
        schemaVersion: 1,
        exportedAt: "2026-05-23T08:00:00Z",
        secretsExported: false,
        settings: {
          idleTimeoutMinutes: 7,
          exportFormat: "json",
          launchAtLogin: false,
          browserBridgeEnabled: true,
          terminalBridgePath: "/Users/alice/.daytrail/terminal.json",
          excludedApps: ["slack"],
          excludedDomains: ["private.example.com"],
          excludedProjects: ["/users/alice/private"],
          aiProvider: "Ollama Local",
          aiModel: "llama3.1",
          aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
          aiRedactSecrets: true,
          fullClipboardHistory: false,
          dataRetentionDays: 30,
        },
      },
      null,
      2,
    );
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-05-23",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          workSessions: [],
          parallelStreams: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings: {
            browserBridgeEnabled: true,
            excludedDomains: [],
            aiProvider: "Ollama Local",
            aiModel: "llama3.1",
            aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
            aiRedactSecrets: true,
            fullClipboardHistory: false,
            dataRetentionDays: 0,
          },
          projectContext: null,
        };
      }

      if (command === "get_storage_locations") {
        return {
          databasePath: "/Users/alice/Library/Application Support/ai.daytrail.desktop/daytrail.sqlite3",
          backupDir: "/Users/alice/Library/Application Support/ai.daytrail.desktop/backups",
          databaseBytes: 4096,
          walBytes: 1024,
          shmBytes: 512,
          backupBytes: 8192,
          totalBytes: 13_824,
          retentionDays: 30,
        };
      }

      if (command === "update_settings") {
        expect(args).toEqual({ patch: { dataRetentionDays: 30 } });
        return {
          browserBridgeEnabled: true,
          excludedDomains: [],
          aiProvider: "Ollama Local",
          aiModel: "llama3.1",
          aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
          aiRedactSecrets: true,
          fullClipboardHistory: false,
          dataRetentionDays: 30,
        };
      }

      if (command === "prune_captured_data") {
        expect(args).toEqual({ days: 30 });
        return { deletedRows: 3 };
      }

      if (command === "purge_captured_data") {
        return { deletedRows: 12 };
      }

      if (command === "export_settings_config") {
        return exportedConfig;
      }

      if (command === "import_settings_config") {
        expect(args).toEqual({ configJson: exportedConfig });
        return {
          browserBridgeEnabled: true,
          excludedDomains: ["private.example.com"],
          aiProvider: "Ollama Local",
          aiModel: "llama3.1",
          aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
          aiRedactSecrets: true,
          fullClipboardHistory: false,
        };
      }

      if (command === "backup_database") {
        return {
          path: "/Users/alice/Library/Application Support/ai.daytrail.desktop/backups/daytrail-backup.sqlite3",
          bytes: 4096,
          generatedAt: "2026-05-23T08:05:00Z",
          preRestoreBackupPath: null,
        };
      }

      if (command === "restore_database") {
        expect(args).toEqual({ path: "/tmp/daytrail-import.sqlite3" });
        return {
          path: "/tmp/daytrail-import.sqlite3",
          bytes: 4096,
          generatedAt: "2026-05-23T08:06:00Z",
          preRestoreBackupPath:
            "/Users/alice/Library/Application Support/ai.daytrail.desktop/backups/daytrail-backup-before-restore.sqlite3",
        };
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByText(/waiting for captured activity/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /^settings$/i }));
    await user.click(screen.getByRole("button", { name: /data storage/i }));

    expect(await screen.findByText(/daytrail\.sqlite3/i)).toBeInTheDocument();
    expect(screen.getByText(/14 KB/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^30 days$/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: { dataRetentionDays: 30 },
      }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("prune_captured_data", { days: 30 }),
    );

    await user.click(screen.getByRole("button", { name: /apply now/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("prune_captured_data", { days: 30 }),
    );

    await user.click(screen.getByRole("button", { name: /export configuration/i }));
    expect(await screen.findByDisplayValue(/"schemaVersion": 1/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /import configuration/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("import_settings_config", {
        configJson: exportedConfig,
      }),
    );

    await user.click(screen.getByRole("button", { name: /backup database/i }));
    expect(await screen.findByText(/backup created/i)).toBeInTheDocument();

    await user.type(
      screen.getByLabelText(/database file path to restore/i),
      "/tmp/daytrail-import.sqlite3",
    );
    await user.click(screen.getByRole("button", { name: /restore database/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("restore_database", {
        path: "/tmp/daytrail-import.sqlite3",
      }),
    );
    expect(await screen.findByText(/database restored/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /clear captured data now/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("purge_captured_data", undefined));
    expect(await screen.findByText(/cleared 12 row/i)).toBeInTheDocument();
  });

  it("blocks first-run app entry until required capture permission is granted", async () => {
    const user = userEvent.setup();
    const missingPermissions = {
      platform: "macos",
      setupRequired: true,
      allRequiredGranted: false,
      appPath: "/Applications/DayTrail.app",
      executablePath: "/Applications/DayTrail.app/Contents/MacOS/daytrail",
      restartRecommended: true,
      diagnostics: [
        "Enable Accessibility for this exact app: /Applications/DayTrail.app",
        "If DayTrail is already enabled, quit and reopen the same app copy, then recheck.",
      ],
      checks: [
        {
          id: "accessibility",
          label: "Accessibility",
          required: true,
          status: "missing",
          detail: "Required for accurate app and window-title tracking.",
          settingsLabel: "Privacy & Security > Accessibility",
          settingsUrl:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
          actionLabel: "Open Accessibility Settings",
        },
        {
          id: "browser-automation",
          label: "Browser tab context",
          required: false,
          status: "user_prompt",
          detail: "Optional. Adds the active browser tab title and domain to your timeline. Skip this if app names are enough.",
          settingsLabel: "Privacy & Security > Automation",
          settingsUrl:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
          actionLabel: "Check access",
        },
      ],
    };
    const grantedPermissions = {
      ...missingPermissions,
      setupRequired: false,
      allRequiredGranted: true,
      restartRecommended: false,
      diagnostics: [],
      checks: missingPermissions.checks.map((check) =>
        check.id === "accessibility"
          ? {
              ...check,
              status: "granted",
              detail: "DayTrail can read the active app and focused window title.",
            }
          : check,
      ),
    };
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-05-23",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          workSessions: [],
          parallelStreams: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings: { browserBridgeEnabled: true, excludedDomains: [] },
          projectContext: null,
        };
      }

      if (command === "get_capture_permissions") {
        return missingPermissions;
      }

      if (command === "request_capture_permission") {
        expect(args).toEqual({ permissionId: "accessibility" });
        return grantedPermissions;
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByRole("heading", { name: /allow app and window tracking|still not detected/i })).toBeInTheDocument();
    expect(screen.getAllByText(/privacy & security > accessibility/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/\/applications\/daytrail\.app/i).length).toBeGreaterThan(0);
    expect(screen.getAllByRole("button", { name: /restart app/i }).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/browser tab context/i).length).toBeGreaterThan(0);
    expect(screen.queryByRole("heading", { name: /^today$/i })).not.toBeInTheDocument();

    await user.click(screen.getAllByRole("button", { name: /open accessibility settings/i })[0]);

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("request_capture_permission", {
        permissionId: "accessibility",
      }),
    );
    expect(await screen.findByRole("heading", { name: /^today$/i })).toBeInTheDocument();
  });

  it("does not block app entry for Windows permission summaries", async () => {
    const invoke = vi.fn(async (command: string) => {
      if (command === "today") {
        return {
          localDate: "2026-05-23",
          tasks: [],
          quickNotes: [],
          commitments: [],
          pendingReplies: [],
          aiOutputs: [],
          meetings: [],
          fieldVisits: [],
          idleBlocks: [],
          workSessions: [],
          parallelStreams: [],
          nextBestAction: null,
          pauseState: { paused: false },
          settings: { browserBridgeEnabled: true, excludedDomains: [] },
          projectContext: null,
        };
      }

      if (command === "get_capture_permissions") {
        return {
          platform: "windows",
          setupRequired: false,
          allRequiredGranted: true,
          appPath: null,
          executablePath: "C:\\Users\\alice\\AppData\\Local\\Programs\\DayTrail\\DayTrail.exe",
          restartRecommended: false,
          diagnostics: [
            "No Windows privacy permission is required for normal active-window tracking.",
          ],
          checks: [
            {
              id: "window-metadata",
              label: "Active app metadata",
              required: false,
              status: "granted",
              detail: "Windows allows normal active app and window-title tracking without a separate privacy grant.",
              settingsLabel: null,
              settingsUrl: null,
              actionLabel: null,
            },
          ],
        };
      }

      return null;
    });

    window.__TAURI__ = {
      core: {
        invoke: invoke as unknown as <T>(
          command: string,
          args?: Record<string, unknown>,
        ) => Promise<T>,
      },
    };

    render(<App />);

    expect(await screen.findByRole("heading", { name: /^today$/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /allow app and window tracking/i })).not.toBeInTheDocument();
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_capture_permissions", undefined));
  });
});
