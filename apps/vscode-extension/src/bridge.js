"use strict";

const { spawn } = require("node:child_process");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");

const DEFAULT_SOURCE = "vscode-extension";
const MAX_NATIVE_MESSAGE_BYTES = 1024 * 1024;

function nowIso() {
  return new Date().toISOString();
}

function toBridgeMessage(events, options = {}) {
  return {
    type: "worktrace.editor_context_batch",
    schemaVersion: 1,
    source: options.source ?? DEFAULT_SOURCE,
    capturedAt: options.capturedAt ?? nowIso(),
    events: Array.isArray(events) ? events : [],
  };
}

function stableStringify(value) {
  return JSON.stringify(stableValue(value));
}

function stableValue(value) {
  if (Array.isArray(value)) {
    return value.map(stableValue);
  }
  if (!value || typeof value !== "object") {
    return value;
  }

  const output = {};
  for (const key of Object.keys(value).sort()) {
    output[key] = stableValue(value[key]);
  }
  return output;
}

function encodeNativeMessage(message) {
  const payload = Buffer.from(stableStringify(message), "utf8");
  if (payload.length > MAX_NATIVE_MESSAGE_BYTES) {
    throw new Error("native bridge message exceeds 1 MiB limit");
  }
  const header = Buffer.alloc(4);
  header.writeUInt32LE(payload.length, 0);
  return Buffer.concat([header, payload]);
}

function decodeNativeMessage(buffer) {
  if (buffer.length < 4) {
    return null;
  }
  const length = buffer.readUInt32LE(0);
  if (length === 0 || length > MAX_NATIVE_MESSAGE_BYTES || buffer.length < length + 4) {
    return null;
  }
  return JSON.parse(buffer.subarray(4, length + 4).toString("utf8"));
}

function sendNativeProcessMessage(message, options = {}) {
  const command = options.command;
  if (!command) {
    return Promise.resolve({ ok: false, error: "native bridge command is not configured" });
  }

  const args = Array.isArray(options.args) ? options.args : [];
  const timeoutMs = options.timeoutMs ?? 2500;

  return new Promise((resolve) => {
    let settled = false;
    const child = spawn(command, args, {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    const chunks = [];
    const errors = [];
    const timer = setTimeout(() => {
      if (!settled) {
        settled = true;
        child.kill();
        resolve({ ok: false, error: "native bridge timed out" });
      }
    }, timeoutMs);

    child.stdout.on("data", (chunk) => chunks.push(chunk));
    child.stderr.on("data", (chunk) => errors.push(chunk));
    child.on("error", (error) => {
      if (!settled) {
        settled = true;
        clearTimeout(timer);
        resolve({ ok: false, error: error.message });
      }
    });
    child.on("close", (code) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);

      const decoded = decodeNativeMessage(Buffer.concat(chunks));
      if (decoded) {
        resolve(decoded);
        return;
      }
      resolve({
        ok: code === 0,
        code,
        error: code === 0 ? null : Buffer.concat(errors).toString("utf8").trim() || "native bridge failed",
      });
    });

    child.stdin.end(encodeNativeMessage(message));
  });
}

async function appendBridgeFile(message, options = {}) {
  const filePath = expandHome(options.filePath ?? "~/.daytrail/editor-bridge.jsonl");
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.appendFile(filePath, `${stableStringify(message)}\n`, "utf8");
  return { ok: true, stored: Array.isArray(message.events) ? message.events.length : 0, transport: "file" };
}

function expandHome(value) {
  if (!value || typeof value !== "string") {
    return value;
  }
  if (value === "~") {
    return os.homedir();
  }
  if (value.startsWith("~/")) {
    return path.join(os.homedir(), value.slice(2));
  }
  return value;
}

function createBridgeSender(options = {}) {
  if (options.command) {
    return (message) => sendNativeProcessMessage(message, options);
  }
  return (message) => appendBridgeFile(message, options);
}

module.exports = {
  appendBridgeFile,
  createBridgeSender,
  decodeNativeMessage,
  encodeNativeMessage,
  sendNativeProcessMessage,
  stableStringify,
  toBridgeMessage,
};
