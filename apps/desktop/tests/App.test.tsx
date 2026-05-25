import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, vi } from "vitest";

import App from "../src/App";

afterEach(() => {
  vi.restoreAllMocks();
  window.__TAURI__ = undefined;
  window.__TAURI_INTERNALS__ = undefined;
});

async function openCommandResult(
  user: ReturnType<typeof userEvent.setup>,
  name: RegExp,
) {
  await user.click(screen.getByRole("button", { name: /search work/i }));
  await user.click(screen.getByRole("button", { name }));
}

async function openAiSettings(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /^settings$/i }));
  await user.click(screen.getByRole("button", { name: /ai provider/i }));
}

describe("WorkTrace command center", () => {
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
    expect(screen.getByText(/no work captured yet/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^activity$/i }));

    expect(
      screen.getByRole("heading", { level: 2, name: /activity/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/no activity yet/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^activity$/i })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("adds a context-anchored note and toggles watcher status", async () => {
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

    await openCommandResult(user, /resume current context/i);

    const quickNote = screen.getByRole("textbox", { name: /quick bullet/i });

    await user.type(quickNote, "Follow up on Oval renewal");
    await user.click(screen.getByRole("button", { name: /^add note$/i }));

    expect(screen.getByText(/follow up on oval renewal/i)).toBeInTheDocument();
    expect(quickNote).toHaveValue("");

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
    expect(screen.getByText(/what daytrail captures/i)).toBeInTheDocument();
    expect(screen.queryByText(/^unknown$/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/default capture policy/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /ai provider/i }));

    await user.selectOptions(screen.getByLabelText(/provider/i), "Gemini");
    expect(screen.getByLabelText(/model/i)).toHaveValue("gemini-flash-latest");
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

  it("surfaces context restore clues and ritual execution controls", async () => {
    const user = userEvent.setup();

    render(<App />);

    await openCommandResult(user, /resume current context/i);

    expect(
      screen.getByRole("heading", {
        name: /no return marker yet/i,
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /resume context/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/no related clues/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^reports$/i }));

    expect(screen.getByRole("button", { name: /end-of-day summary/i })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    await user.click(screen.getAllByRole("button", { name: /^generate$/i })[0]);
    expect(screen.getByLabelText(/generated report markdown/i).textContent).toMatch(
      /daily work execution report/i,
    );
  });

  it("hydrates command-center data from the Tauri today snapshot", async () => {
    const user = userEvent.setup();
    const now = Date.UTC(2026, 4, 23, 10, 30);
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
            title: "App.tsx - WorkTrace",
            domain: null,
            urlRedacted: null,
            workspaceKey: "/Users/alice/work/worktrace",
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
            title: "ChatGPT - WorkTrace summary",
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
                  label: "worktrace",
                  durationMs: 300_000,
                  events: 1,
                  aiTools: [],
                  examples: ["App.tsx - WorkTrace"],
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
                  examples: ["ChatGPT - WorkTrace summary"],
                },
              ],
            },
          ],
        },
        automationCandidates: [
          {
            id: "automation-worktrace",
            title: "worktrace",
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
        projectContext: { path: "/tmp/worktrace", source: "git" },
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
    expect(screen.getByText(/ai-active today/i)).toBeInTheDocument();
    expect(screen.getAllByText(/chatgpt/i).length).toBeGreaterThan(0);
    expect(screen.getByText(/usage by app/i)).toBeInTheDocument();
    expect(screen.getAllByText(/google chrome/i).length).toBeGreaterThan(0);

    await user.click(screen.getAllByRole("button", { name: /sqlite capture block/i })[0]);

    expect(screen.getByText(/session details/i)).toBeInTheDocument();
    expect(screen.getAllByText(/app\.tsx - worktrace/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/https:\/\/chatgpt\.com\/c\/thread/i).length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: /close session details/i }));

    await openCommandResult(user, /resume current context/i);
    expect(screen.getByText(/backend note loaded from sqlite/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^activity$/i }));

    expect(
      screen.getByRole("heading", { level: 2, name: /activity/i }),
    ).toBeInTheDocument();
    expect(screen.getByText(/select project \/ workspace/i)).toBeInTheDocument();
    expect(screen.getByText(/activity details/i)).toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: /google chrome/i })[0]);
    expect(screen.getAllByText(/chatgpt\.com/i).length).toBeGreaterThan(0);
    expect(screen.getAllByText(/https:\/\/chatgpt\.com\/c\/thread/i).length).toBeGreaterThan(0);
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

  it("persists AI settings and generates weekly plans through Tauri", async () => {
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

      if (command === "generate_weekly_plan") {
        return {
          bodyMarkdown: "# Weekly Plan\n\n## Must close\n- Close Oval billing validation",
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
    await user.click(screen.getByRole("button", { name: /weekly review/i }));
    await user.click(screen.getAllByRole("button", { name: /^generate$/i })[0]);

    await waitFor(() =>
      expect(
        screen.getByLabelText(/generated report markdown/i).textContent,
      ).toMatch(/weekly plan/i),
    );
    expect(invoke).toHaveBeenCalledWith("generate_weekly_plan", undefined);
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
          },
          projectContext: null,
        };
      }

      if (command === "get_storage_locations") {
        return {
          databasePath: "/Users/alice/Library/Application Support/ai.daytrail.desktop/daytrail.sqlite3",
          backupDir: "/Users/alice/Library/Application Support/ai.daytrail.desktop/backups",
        };
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
          label: "Browser automation",
          required: false,
          status: "user_prompt",
          detail: "macOS asks once when DayTrail reads a supported browser's active tab URL.",
          settingsLabel: "Privacy & Security > Automation",
          settingsUrl:
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation",
          actionLabel: "Open Automation Settings",
        },
        {
          id: "screen-recording",
          label: "Screen Recording",
          required: false,
          status: "not_required",
          detail: "Not requested because screenshots are off by default.",
          settingsLabel: null,
          settingsUrl: null,
          actionLabel: null,
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
    expect(screen.getByText(/privacy & security > accessibility/i)).toBeInTheDocument();
    expect(screen.getAllByText(/\/applications\/daytrail\.app/i).length).toBeGreaterThan(0);
    expect(screen.getAllByRole("button", { name: /restart app/i }).length).toBeGreaterThan(0);
    expect(screen.getByText(/screenshots are off by default/i)).toBeInTheDocument();
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
