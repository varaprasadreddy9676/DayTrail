import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, vi } from "vitest";
import { check as checkUpdate } from "@tauri-apps/plugin-updater";

import App from "../src/App";

// Mock the Tauri updater plugin so UpdateChecker doesn't make real network calls
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

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

function installLocalStorageMock() {
  const values = new Map<string, string>();
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: {
      clear: () => values.clear(),
      getItem: (key: string) => values.get(key) ?? null,
      removeItem: (key: string) => values.delete(key),
      setItem: (key: string, value: string) => values.set(key, value),
    },
  });
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

  it("keeps Smart Breaks in settings instead of the sidebar", async () => {
    const user = userEvent.setup();
    const settings = {
      browserBridgeEnabled: true,
      excludedDomains: [],
      aiProvider: "Ollama Local",
      aiModel: "llama3.1",
      aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
      aiRedactSecrets: true,
      fullClipboardHistory: false,
      recoveryEnabled: false,
      recoveryThresholdMinutes: 30,
    };
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
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

      if (command === "update_settings") {
        return {
          ...settings,
          ...(args?.patch as Record<string, unknown>),
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
    expect(screen.queryByRole("heading", { name: /blink, posture, and break reminders/i })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^settings$/i }));
    await user.click(screen.getByRole("button", { name: /capture health/i }));
    expect(screen.getByRole("heading", { name: /blink, posture, and break reminders/i })).toBeInTheDocument();
    expect(screen.getByText(/blink, posture, break/i)).toBeInTheDocument();
    expect(screen.getByText(/quiet during calls/i)).toBeInTheDocument();

    await user.click(screen.getByRole("checkbox", { name: /enable smart breaks/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: expect.objectContaining({ recoveryEnabled: true }),
      }),
    );

    const breakReminderGroup = screen.getByRole("group", { name: /break reminder/i });
    expect(within(breakReminderGroup).getByRole("button", { name: /30m/i })).toHaveAttribute("aria-pressed", "true");
    await user.click(within(breakReminderGroup).getByRole("button", { name: /45m/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: expect.objectContaining({ recoveryThresholdMinutes: 45 }),
      }),
    );
    expect(screen.getByText(/smart breaks updated/i)).toBeInTheDocument();
  });

  it("monkey-clicks the primary navigation without extra onboarding burden", async () => {
    const user = userEvent.setup();

    render(<App />);

    expect(screen.queryByText(/view sample day/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/keep daytrail running while you work/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/^from$/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /app range custom range/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^my tasks$/i }));
    expect(screen.getByRole("heading", { level: 1, name: /^my tasks$/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^activity$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /activity/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^ai impact$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /ai impact/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^review queue$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /review queue/i })).toBeInTheDocument();
    expect(screen.getAllByText(/local record/i).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /^reports$/i }));
    expect(screen.getAllByRole("heading", { level: 2, name: /reports/i }).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /^settings$/i }));
    expect(screen.getByRole("heading", { level: 2, name: /settings/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^today$/i }));
    expect(screen.getAllByText(/today timeline/i).length).toBeGreaterThan(0);
  });

  it("opens bulk task import from My Tasks", async () => {
    const user = userEvent.setup();

    render(<App />);

    await user.click(screen.getByRole("button", { name: /^my tasks$/i }));
    await user.click(screen.getByRole("button", { name: /^import tasks$/i }));

    expect(screen.getByRole("heading", { name: /^import tasks$/i })).toBeInTheDocument();
    expect(screen.getByLabelText(/paste tasks/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /ai draft tasks/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /paste many/i })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("prompts for available updates from the automatic startup check", async () => {
    const user = userEvent.setup();
    installLocalStorageMock();

    const mockDownloadAndInstall = vi.fn().mockResolvedValue(undefined);
    vi.mocked(checkUpdate).mockResolvedValueOnce({
      available: true,
      version: "0.1.3",
      body: null,
      downloadAndInstall: mockDownloadAndInstall,
    } as never);

    window.__TAURI__ = {
      core: { invoke: vi.fn().mockResolvedValue(null) as never },
    };

    render(<App />);

    expect(
      await screen.findByRole("dialog", { name: /daytrail 0.1.3 is available/i }),
    ).toBeInTheDocument();
    expect(screen.getByText("v0.1.3")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /later/i })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /update now/i }));

    expect(mockDownloadAndInstall).toHaveBeenCalled();
  });

  it("checks for updates on startup even if a previous check happened recently", async () => {
    vi.mocked(checkUpdate).mockResolvedValueOnce({ available: false } as never);

    window.__TAURI__ = {
      core: { invoke: vi.fn().mockResolvedValue(null) as never },
    };

    render(<App />);

    expect(await screen.findByRole("heading", { name: /^today$/i })).toBeInTheDocument();
    expect(checkUpdate).toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("snoozes an available startup update for 8 hours", async () => {
    const user = userEvent.setup();
    installLocalStorageMock();
    const now = Date.now();

    vi.mocked(checkUpdate).mockResolvedValueOnce({
      available: true,
      version: "0.1.3",
      body: null,
      downloadAndInstall: vi.fn().mockResolvedValue(undefined),
    } as never);

    window.__TAURI__ = {
      core: { invoke: vi.fn().mockResolvedValue(null) as never },
    };

    render(<App />);

    expect(
      await screen.findByRole("dialog", { name: /daytrail 0.1.3 is available/i }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /later/i }));

    const snoozedUntil = Number(
      window.localStorage.getItem("daytrail:update:snoozedUntil:0.1.3"),
    );
    expect(snoozedUntil).toBeGreaterThanOrEqual(now + 8 * 60 * 60 * 1000 - 5_000);
    expect(
      screen.queryByRole("dialog", { name: /daytrail 0.1.3 is available/i }),
    ).not.toBeInTheDocument();
  });

  it("manages overall tasks and reminders from My Tasks", async () => {
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
      completedAt: null,
      createdAt: "2026-06-02T09:00:00Z",
      updatedAt: "2026-06-02T09:00:00Z",
    };
    const completedTask = {
      id: 44,
      title: "Ship release notes",
      status: "done",
      dueDate: "2026-06-01",
      dueAt: null,
      notes: "Published to changelog",
      priority: "medium",
      source: "manual",
      projectPath: null,
      clientLabel: "Product",
      projectLabel: "Release",
      reminderSentAt: null,
      completedAt: "2026-06-02T14:00:00Z",
      createdAt: "2026-06-01T09:00:00Z",
      updatedAt: "2026-06-02T14:00:00Z",
    };
    let mockedTasks = [openTask, completedTask];
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-06-02",
          tasks: mockedTasks,
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
        const input = args?.input as { title: string; dueDate?: string | null; dueAt?: number | null; priority?: string };
        const created = {
          ...openTask,
          id: 43 + mockedTasks.length,
          title: input.title,
          dueDate: input.dueDate ?? null,
          dueAt: input.dueAt ?? null,
          priority: input.priority ?? "medium",
        };
        mockedTasks = [created, ...mockedTasks];
        return created;
      }
      if (command === "update_task") {
        const input = args?.input as Partial<typeof openTask> & { title: string };
        const updated = { ...openTask, ...input, id: args?.id as number };
        mockedTasks = mockedTasks.map((task) => (task.id === args?.id ? updated : task));
        return updated;
      }
      if (command === "draft_tasks_from_text") {
        return [
          {
            title: "HER Health LIS Integration",
            dueDate: null,
            dueAt: null,
            notes: null,
            priority: "high",
            clientLabel: null,
            projectLabel: null,
          },
          {
            title: "NOVA Path kind LIS Integration",
            dueDate: null,
            dueAt: null,
            notes: null,
            priority: "high",
            clientLabel: null,
            projectLabel: null,
          },
        ];
      }
      if (command === "complete_task") {
        const current = mockedTasks.find((task) => task.id === args?.id) ?? openTask;
        const completed = {
          ...current,
          status: "done",
          completedAt: "2026-06-02T16:00:00Z",
          updatedAt: "2026-06-02T16:00:00Z",
        };
        mockedTasks = mockedTasks.map((task) => (task.id === args?.id ? completed : task));
        return completed;
      }
      if (command === "snooze_task") {
        const current = mockedTasks.find((task) => task.id === args?.id) ?? openTask;
        const snoozed = { ...current, status: "open", dueAt: args?.dueAt as number };
        mockedTasks = mockedTasks.map((task) => (task.id === args?.id ? snoozed : task));
        return snoozed;
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

    expect(await screen.findByRole("heading", { name: /^today$/i })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /tasks & reminders/i })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^my tasks$/i }));

    expect(await screen.findByRole("heading", { name: /^my tasks$/i })).toBeInTheDocument();
    expect(screen.getAllByText(/renew vendor contract/i).length).toBeGreaterThan(0);
    expect(screen.getByText(/confirm budget owner first/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/^completed recently$/i)).toHaveTextContent(/ship release notes/i);

    fireEvent.change(screen.getByLabelText(/task report from date/i), { target: { value: "2026-06-01" } });
    fireEvent.change(screen.getByLabelText(/task report to date/i), { target: { value: "2026-06-03" } });
    await user.click(screen.getByRole("button", { name: /generate report/i }));
    expect(screen.getByLabelText(/generated completed task report/i)).toHaveTextContent(/ship release notes/i);
    expect(screen.getByLabelText(/task report markdown preview/i)).toHaveTextContent(/ship release notes/i);
    expect(screen.getAllByRole("button", { name: /download md/i }).length).toBeGreaterThan(0);

    await user.type(screen.getByLabelText(/reminder title/i), "Call pharmacy back");
    await user.click(screen.getByRole("button", { name: /^10m$/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_task", {
        input: expect.objectContaining({
          title: "Call pharmacy back",
          priority: "medium",
          source: "quick-reminder",
          dueAt: expect.any(Number),
        }),
      }),
    );

    await user.click(screen.getByRole("button", { name: /^import tasks$/i }));
    await user.type(
      screen.getByLabelText(/paste tasks/i),
      "HER Health LIS Integration\nNOVA Path kind LIS Integration",
    );
    await user.click(screen.getByRole("button", { name: /draft tasks/i }));

    expect(await screen.findByText(/2 tasks ready/i)).toBeInTheDocument();
    expect(screen.getAllByText(/her health lis integration/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/nova path kind lis integration/i).length).toBeGreaterThan(0);

    await user.click(screen.getByRole("button", { name: /create 2 tasks/i }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("draft_tasks_from_text", {
        text: "HER Health LIS Integration\nNOVA Path kind LIS Integration",
        defaultPriority: "high",
      });
      expect(invoke).toHaveBeenCalledWith("create_task", {
        input: expect.objectContaining({
          title: "HER Health LIS Integration",
          priority: "high",
          source: "bulk-import",
        }),
      });
      expect(invoke).toHaveBeenCalledWith("create_task", {
        input: expect.objectContaining({
          title: "NOVA Path kind LIS Integration",
          priority: "high",
          source: "bulk-import",
        }),
      });
    });
    await waitFor(() =>
      expect(screen.queryByRole("heading", { name: /^import tasks$/i })).not.toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: /^add task$/i }));
    expect(screen.getByLabelText(/^due date$/i)).toHaveAttribute("type", "date");
    expect(screen.getByLabelText(/^due time$/i)).toHaveAttribute("type", "time");
    await user.type(screen.getByLabelText(/^title$/i), "Send invoice follow-up");
    await user.type(screen.getByLabelText(/^notes$/i), "Ask whether PO is approved");
    fireEvent.change(screen.getByLabelText(/^due date$/i), { target: { value: "2026-06-03" } });
    fireEvent.change(screen.getByLabelText(/^due time$/i), { target: { value: "13:30" } });
    await user.click(screen.getByRole("button", { name: /save task/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_task", {
        input: expect.objectContaining({
          title: "Send invoice follow-up",
          dueDate: "2026-06-03",
          dueAt: new Date(2026, 5, 3, 13, 30, 0, 0).getTime(),
          notes: "Ask whether PO is approved",
          priority: "medium",
          source: "manual",
        }),
      }),
    );

    const renewTaskRow = screen.getByText(/renew vendor contract/i).closest(".task-row");
    expect(renewTaskRow).not.toBeNull();
    await user.click(within(renewTaskRow as HTMLElement).getByRole("button", { name: /^edit$/i }));
    expect(screen.getByRole("heading", { name: /^edit task$/i })).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(/^title$/i), { target: { value: "Renew vendor agreement" } });
    fireEvent.change(screen.getByLabelText(/^due date$/i), { target: { value: "2026-06-18" } });
    fireEvent.change(screen.getByLabelText(/^due time$/i), { target: { value: "09:45" } });
    await user.click(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_task", {
        id: 42,
        input: expect.objectContaining({
          title: "Renew vendor agreement",
          dueDate: "2026-06-18",
          dueAt: new Date(2026, 5, 18, 9, 45, 0, 0).getTime(),
          priority: "high",
          source: "manual",
        }),
      }),
    );

    const taskRowAfterEdit = screen
      .getAllByText(/renew vendor agreement|renew vendor contract/i)
      .map((element) => element.closest(".task-row"))
      .find((row): row is HTMLElement => row instanceof HTMLElement && within(row).queryByLabelText(/snooze/i) !== null);
    expect(taskRowAfterEdit).toBeDefined();

    await user.selectOptions(within(taskRowAfterEdit as HTMLElement).getByLabelText(/snooze/i), "15");
    expect(invoke).toHaveBeenCalledWith("snooze_task", {
      id: 42,
      dueAt: expect.any(Number),
    });

    await user.click(within(taskRowAfterEdit as HTMLElement).getByRole("button", { name: /^complete$/i }));
    expect(invoke).toHaveBeenCalledWith("complete_task", { id: 42 });

    const completedRecentlySection = screen.getByLabelText(/^completed recently$/i);
    await waitFor(() => expect(completedRecentlySection).toHaveTextContent(/renew vendor agreement/i));
    const completedTaskRow = within(completedRecentlySection).getByText(/renew vendor agreement/i).closest(".task-row");
    expect(completedTaskRow).not.toBeNull();

    await user.click(within(completedTaskRow as HTMLElement).getByRole("button", { name: /^delete$/i }));
    expect(invoke).toHaveBeenCalledWith("delete_task", { id: 42 });
  });

  it("keeps the whole tasks page scrollable when many tasks exist", async () => {
    const user = userEvent.setup();
    const tasks = Array.from({ length: 12 }, (_, index) => ({
      id: index + 1,
      title: `Urgent backlog item ${index + 1}`,
      status: "open",
      dueDate: null,
      dueAt: null,
      notes: "bulk-import",
      priority: "high",
      source: "bulk-import",
      projectPath: null,
      clientLabel: null,
      projectLabel: null,
      reminderSentAt: null,
      completedAt: null,
      createdAt: "2026-06-02T09:00:00Z",
      updatedAt: "2026-06-02T09:00:00Z",
    }));
    const invoke = vi.fn(async (command: string) => {
      if (command === "today") {
        return {
          localDate: "2026-06-02",
          tasks,
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
          settings: { browserBridgeEnabled: true, excludedDomains: [] },
          projectContext: null,
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

    await user.click(await screen.findByRole("button", { name: /^my tasks$/i }));
    expect(await screen.findByText(/urgent backlog item 12/i)).toBeInTheDocument();

    const tasksContentPane = document.querySelector(".content-pane--task-page");
    expect(tasksContentPane).toBeInstanceOf(HTMLElement);

    const tasksPage = document.querySelector(".my-tasks-view");
    expect(tasksPage).toBeInstanceOf(HTMLElement);
    expect(tasksPage).toHaveAttribute("data-scrollable-page", "tasks");

    const openTasksSection = screen.getByLabelText(/^open tasks$/i);
    const openTasksList = openTasksSection.querySelector(".tasks-list");
    expect(openTasksList).toBeInstanceOf(HTMLElement);
    expect(openTasksList).not.toHaveAttribute("data-scrollable");
    expect(openTasksList).not.toHaveAttribute("tabindex");
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
              reason: "Smart Breaks are ready when sustained input continues",
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
    expect(screen.queryByRole("heading", { name: /blink, posture, and break reminders/i })).not.toBeInTheDocument();
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

  it("moves the visible mode checkmark when Pro Mode is selected", async () => {
    const user = userEvent.setup();
    const baseSettings = {
      browserBridgeEnabled: true,
      excludedDomains: [],
      experienceMode: "simple",
      showSystemApps: false,
      showRawEvents: false,
      showCaptureConfidence: false,
      showAiDetails: "summary",
      aiProvider: "Ollama Local",
      aiModel: "llama3.1",
      aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
      aiRedactSecrets: true,
      fullClipboardHistory: false,
    };
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "today") {
        return {
          localDate: "2026-06-02",
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
          settings: baseSettings,
          projectContext: null,
        };
      }

      if (command === "update_settings") {
        return {
          ...baseSettings,
          ...(args?.patch as Record<string, unknown>),
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

    await user.click(await screen.findByRole("button", { name: /^settings$/i }));

    const simpleModeButton = screen.getByRole("button", { name: /simple mode timeline/i });
    const proModeButton = screen.getByRole("button", { name: /pro mode detailed activity/i });

    expect(simpleModeButton).toHaveAttribute("aria-pressed", "true");
    expect(simpleModeButton.querySelector(".settings-selection-mark")?.getAttribute("data-state")).toBe("selected");
    expect(proModeButton.querySelector(".settings-selection-mark")?.getAttribute("data-state")).toBe("available");

    await user.click(proModeButton);

    await waitFor(() => expect(proModeButton).toHaveAttribute("aria-pressed", "true"));
    expect(simpleModeButton).toHaveAttribute("aria-pressed", "false");
    expect(simpleModeButton.querySelector(".settings-selection-mark")?.getAttribute("data-state")).toBe("available");
    expect(proModeButton.querySelector(".settings-selection-mark")?.getAttribute("data-state")).toBe("selected");
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
            source: "Meeting actions",
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
    expect(screen.getAllByText(/to review/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/ship backend wiring/i).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: /capture paused/i })).toBeInTheDocument();
    expect(screen.getAllByText(/ai detected/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/chatgpt/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/today timeline/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/google chrome/i).length).toBeGreaterThan(0);
    expect(
      Array.from(document.querySelectorAll("[data-tooltip]")).some((node) =>
        node.getAttribute("data-tooltip")?.match(/Google Chrome · 5m · AI: ChatGPT/i),
      ),
    ).toBe(true);

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
    expect(screen.getByRole("img", { name: /Google Chrome · 5m/i })).toBeInTheDocument();
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

    await user.click(screen.getByRole("button", { name: /^reports$/i }));
    await user.click(screen.getByRole("button", { name: /daily report/i }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("generate_daily_report", undefined));

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
          taskRetentionDays: 90,
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
            taskRetentionDays: 0,
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
        const patch = args?.patch as { dataRetentionDays?: number; taskRetentionDays?: number };
        if (patch.taskRetentionDays !== undefined) {
          return {
            browserBridgeEnabled: true,
            excludedDomains: [],
            aiProvider: "Ollama Local",
            aiModel: "llama3.1",
            aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
            aiRedactSecrets: true,
            fullClipboardHistory: false,
            dataRetentionDays: 30,
            taskRetentionDays: 90,
          };
        }
        return {
          browserBridgeEnabled: true,
          excludedDomains: [],
          aiProvider: "Ollama Local",
          aiModel: "llama3.1",
          aiEndpoint: "http://127.0.0.1:11434/v1/chat/completions",
          aiRedactSecrets: true,
          fullClipboardHistory: false,
          dataRetentionDays: 30,
          taskRetentionDays: 0,
        };
      }

      if (command === "prune_captured_data") {
        return { deletedRows: 3 };
      }

      if (command === "prune_completed_tasks") {
        return { deletedRows: 2 };
      }

      if (command === "purge_captured_data") {
        return { deletedRows: 12 };
      }

      if (command === "export_settings_config") {
        return exportedConfig;
      }

      if (command === "import_settings_config") {
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

    await user.click(screen.getByRole("button", { name: /^3 months$/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("update_settings", {
        patch: { taskRetentionDays: 90 },
      }),
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("prune_completed_tasks", { days: 90 }),
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
        "If DayTrail is missing from Accessibility Settings, click + and select /Applications/DayTrail.app.",
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

      if (command === "open_capture_permission_settings") {
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
    expect(screen.getAllByText(/click \+/i).length).toBeGreaterThan(0);
    expect(screen.getAllByRole("button", { name: /restart app/i }).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/browser tab context/i).length).toBeGreaterThan(0);
    expect(screen.queryByRole("heading", { name: /^today$/i })).not.toBeInTheDocument();

    await user.click(screen.getAllByRole("button", { name: /open accessibility settings/i })[0]);

    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("open_capture_permission_settings", {
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

type TestTask = {
  id: number;
  title: string;
  status: "open" | "done";
  priority?: string;
};

function snapshotWithTasks(tasks: TestTask[]) {
  return {
    localDate: "2026-06-02",
    tasks,
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
    recoverySummary: null,
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
    settings: { browserBridgeEnabled: true, excludedDomains: [] },
    projectContext: null,
    activeWorkContext: null,
  };
}

function installInvoke(invoke: ReturnType<typeof vi.fn>) {
  window.__TAURI__ = {
    core: {
      invoke: invoke as unknown as <T>(
        command: string,
        args?: Record<string, unknown>,
      ) => Promise<T>,
    },
  };
}

describe("activity ↔ task linking", () => {
  it("manages rules and links for a task from the My Tasks panel", async () => {
    const user = userEvent.setup();

    // Stateful backend mock so the panel's refresh-after-mutation reflects the
    // same lifecycle the real store enforces (create rule → apply → link).
    const rules: Array<Record<string, unknown>> = [];
    let linkedActivities: Array<Record<string, unknown>> = [];

    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      switch (command) {
        case "today":
          return snapshotWithTasks([
            { id: 1, title: "Project A work", status: "open", priority: "high" },
          ]);
        case "list_task_rules":
          return rules;
        case "list_task_activities":
          return linkedActivities;
        case "create_task_rule": {
          const input = args?.input as Record<string, unknown>;
          const rule = {
            id: 10,
            taskId: args?.taskId,
            ...input,
            createdAt: 1,
            updatedAt: 1,
          };
          rules.push(rule);
          return rule;
        }
        case "apply_task_rules": {
          // Simulate a single past activity now matching the new rule.
          linkedActivities = [
            {
              id: "evt-1",
              source: "window",
              eventType: "window",
              app: "Editor",
              title: "fix [PROJECT-A] crash",
              domain: null,
              urlRedacted: null,
              workspaceKey: null,
              startedAt: Date.parse("2026-06-02T10:00:00Z"),
              endedAt: Date.parse("2026-06-02T10:05:00Z"),
              durationMs: 300_000,
              sensitivity: "normal",
              metadataJson: null,
              createdAt: 1,
              linkId: 100,
              origin: "rule",
              ruleId: 10,
              linkedAt: 1,
            },
          ];
          return { linked: 1, scanned: 3, rules: 1 };
        }
        case "unlink_activity_from_task":
          linkedActivities = [];
          return { deletedRows: 1 };
        default:
          return null;
      }
    });

    installInvoke(invoke);
    render(<App />);

    await user.click(screen.getByRole("button", { name: /^my tasks$/i }));
    expect(
      await screen.findByRole("heading", { level: 1, name: /^my tasks$/i }),
    ).toBeInTheDocument();

    // Expand the per-task links & rules panel.
    await user.click(screen.getByRole("button", { name: /links & rules/i }));
    expect(await screen.findByText(/linked activities/i)).toBeInTheDocument();
    expect(screen.getByText(/no activities linked yet/i)).toBeInTheDocument();
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("list_task_rules", { taskId: 1 }),
    );
    expect(invoke).toHaveBeenCalledWith("list_task_activities", { taskId: 1 });

    // Add a rule. (fireEvent.change avoids userEvent treating "[" as a key tag.)
    fireEvent.change(screen.getByLabelText(/rule pattern/i), {
      target: { value: "[PROJECT-A]" },
    });
    await user.click(screen.getByRole("button", { name: /^add rule$/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("create_task_rule", {
        taskId: 1,
        input: {
          field: "any",
          matcher: "contains",
          pattern: "[PROJECT-A]",
          caseSensitive: false,
          enabled: true,
        },
      }),
    );
    expect(await screen.findByText(/rule added\./i)).toBeInTheDocument();
    expect(screen.getByText("[PROJECT-A]")).toBeInTheDocument();

    // Apply rules to history → one activity gets linked.
    await user.click(screen.getByRole("button", { name: /apply rules to history/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("apply_task_rules", { taskId: 1 }),
    );
    expect(await screen.findByText(/scanned 3 activities · linked 1 new\./i)).toBeInTheDocument();
    expect(await screen.findByText(/fix \[PROJECT-A\] crash/i)).toBeInTheDocument();
    expect(screen.getByText(/^Auto ·/)).toBeInTheDocument();

    // Unlink it again.
    await user.click(screen.getByRole("button", { name: /^unlink$/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("unlink_activity_from_task", {
        sourceEventId: "evt-1",
        taskId: 1,
      }),
    );
    expect(await screen.findByText(/activity unlinked\./i)).toBeInTheDocument();
    expect(screen.getByText(/no activities linked yet/i)).toBeInTheDocument();
  });

  it("links an existing activity to a task by hand via the picker", async () => {
    const user = userEvent.setup();

    let linked: Array<Record<string, unknown>> = [];
    const candidate = {
      id: "evt-9",
      source: "browser",
      eventType: "browser",
      app: "Chrome",
      title: "Acme dashboard",
      domain: "acme.test",
      urlRedacted: null,
      workspaceKey: null,
      startedAt: Date.parse("2026-06-02T09:00:00Z"),
      endedAt: Date.parse("2026-06-02T09:30:00Z"),
      durationMs: 1_800_000,
      sensitivity: "normal",
      metadataJson: null,
      createdAt: 1,
    };

    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
      switch (command) {
        case "today":
          return snapshotWithTasks([{ id: 7, title: "Acme", status: "open" }]);
        case "list_task_rules":
          return [];
        case "list_task_activities":
          return linked;
        case "search_recent_activities":
          return [candidate];
        case "link_activity_to_task":
          linked = [
            { ...candidate, linkId: 1, origin: "manual", ruleId: null, linkedAt: 1 },
          ];
          return { id: 1, sourceEventId: "evt-9", taskId: 7, origin: "manual", ruleId: null, createdAt: 1 };
        default:
          return null;
      }
    });

    installInvoke(invoke);
    render(<App />);

    await user.click(screen.getByRole("button", { name: /^my tasks$/i }));
    await user.click(await screen.findByRole("button", { name: /links & rules/i }));
    await user.click(await screen.findByRole("button", { name: /link an activity/i }));

    await user.click(screen.getByRole("button", { name: /^search$/i }));
    const result = await screen.findByText(/acme dashboard/i);
    expect(result).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^link$/i }));
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("link_activity_to_task", {
        sourceEventId: "evt-9",
        taskId: 7,
      }),
    );
    expect(await screen.findByText(/activity linked\./i)).toBeInTheDocument();
    // The newly linked activity now shows as a Manual link in the list.
    expect(await screen.findByText(/^Manual ·/)).toBeInTheDocument();
  });

  it("surfaces backend validation errors when a rule pattern is invalid", async () => {
    const user = userEvent.setup();

    const invoke = vi.fn(async (command: string) => {
      switch (command) {
        case "today":
          return snapshotWithTasks([{ id: 1, title: "Tickets", status: "open" }]);
        case "list_task_rules":
        case "list_task_activities":
          return [];
        case "create_task_rule":
          throw new Error("invalid regular expression: unclosed group");
        default:
          return null;
      }
    });

    installInvoke(invoke);
    render(<App />);

    await user.click(screen.getByRole("button", { name: /^my tasks$/i }));
    await user.click(await screen.findByRole("button", { name: /links & rules/i }));

    await user.selectOptions(screen.getByLabelText(/rule matcher/i), "regex");
    fireEvent.change(screen.getByLabelText(/rule pattern/i), {
      target: { value: "(unclosed" },
    });
    await user.click(screen.getByRole("button", { name: /^add rule$/i }));

    expect(
      await screen.findByText(/invalid regular expression/i),
    ).toBeInTheDocument();
  });
});
