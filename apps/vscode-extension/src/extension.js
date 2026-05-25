"use strict";

const { createEventBatcher } = require("./batching");
const { createBridgeSender } = require("./bridge");
const { collectActiveEditorContext } = require("./context");

let controller = null;

function activate(context) {
  const vscode = require("vscode");
  controller = createExtensionController(vscode);
  controller.activate(context);
}

function deactivate() {
  if (controller) {
    return controller.flush();
  }
  return undefined;
}

function createExtensionController(vscode) {
  let batcher = createBatcher(vscode);
  let selectionTimer = null;

  function readConfig() {
    const config = vscode.workspace.getConfiguration("worktrace.editorTracking");
    return {
      enabled: config.get("enabled", true),
      includeContentHash: config.get("includeContentHash", false),
      batchDelayMs: config.get("batchDelayMs", 750),
      maxBatchSize: config.get("maxBatchSize", 10),
      maxQueueSize: config.get("maxQueueSize", 100),
      bridgeCommand: String(config.get("bridgeCommand", "") ?? "").trim(),
      bridgeArgs: config.get("bridgeArgs", []),
      bridgeFile: config.get("bridgeFile", "~/.worktrace/editor-bridge.jsonl"),
      bridgeTimeoutMs: config.get("bridgeTimeoutMs", 2500),
    };
  }

  function createBatcher() {
    const config = readConfig();
    const send = createBridgeSender(
      config.bridgeCommand
        ? {
            command: config.bridgeCommand,
            args: Array.isArray(config.bridgeArgs) ? config.bridgeArgs : [],
            timeoutMs: config.bridgeTimeoutMs,
          }
        : {
            filePath: config.bridgeFile,
          },
    );

    return createEventBatcher({
      delayMs: config.batchDelayMs,
      maxBatchSize: config.maxBatchSize,
      maxQueueSize: config.maxQueueSize,
      source: "vscode-extension",
      send,
    });
  }

  async function capture(eventType) {
    const config = readConfig();
    if (!config.enabled || !vscode.window.activeTextEditor) {
      return { ok: true, skipped: true };
    }

    const payload = collectActiveEditorContext(
      {
        source: "vscode-extension",
        eventType,
        appName: vscode.env.appName,
        workspaceName: vscode.workspace.name,
        workspaceFolders: vscode.workspace.workspaceFolders ?? [],
        activeTextEditor: vscode.window.activeTextEditor,
      },
      {
        includeContentHash: config.includeContentHash,
      },
    );
    return batcher.enqueue(payload);
  }

  function captureSelectionChanged() {
    if (selectionTimer) {
      clearTimeout(selectionTimer);
    }
    selectionTimer = setTimeout(() => {
      selectionTimer = null;
      capture("selection_changed").catch(reportError);
    }, 500);
  }

  function reportError(error) {
    console.warn("[WorkTrace] editor tracking failed:", error?.message ?? error);
  }

  function activateController(context) {
    context.subscriptions.push(
      vscode.window.onDidChangeActiveTextEditor(() => {
        capture("active_editor_changed").catch(reportError);
      }),
      vscode.window.onDidChangeTextEditorSelection((event) => {
        if (event.textEditor === vscode.window.activeTextEditor) {
          captureSelectionChanged();
        }
      }),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("worktrace.editorTracking")) {
          batcher.flush().catch(reportError);
          batcher = createBatcher();
        }
      }),
      vscode.commands.registerCommand("worktrace.captureActiveEditor", () => capture("manual_capture")),
      vscode.commands.registerCommand("worktrace.flushEditorContext", () => batcher.flush()),
    );

    capture("activation").catch(reportError);
  }

  return {
    activate: activateController,
    capture,
    flush: () => batcher.flush(),
  };
}

module.exports = {
  activate,
  createExtensionController,
  deactivate,
};
