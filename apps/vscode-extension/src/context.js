"use strict";

const crypto = require("node:crypto");

const DEFAULT_SOURCE = "vscode-extension";
const SECRET_MARKER = "[redacted-secret]";
const OMITTED_MARKER = "[omitted]";

const SECRET_SEGMENT_PATTERN =
  /(^\.env(?:\..*)?$|^\.npmrc$|^\.pypirc$|^\.netrc$|^id_(?:rsa|dsa|ecdsa|ed25519)(?:\.pub)?$|secret|token|password|passwd|credential|api[-_]?key|private[-_]?key|session[-_]?id|auth|jwt)/i;
const SECRET_ASSIGNMENT_PATTERN =
  /\b(password|passwd|pwd|token|api[_-]?key|secret|session[_-]?id|client[_-]?secret|access[_-]?token|refresh[_-]?token)\s*[:=]\s*["']?[^"',\s;]+["']?/gi;
const AUTH_HEADER_PATTERN =
  /\b(authorization)\s*:\s*(bearer|basic)\s+[A-Za-z0-9._~+/-]+=*/gi;
const BEARER_PATTERN = /\bBearer\s+[A-Za-z0-9._~+/-]+=*/g;
const JWT_PATTERN = /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]+\b/g;
const WELL_KNOWN_TOKEN_PATTERN =
  /\b(?:sk|pk|rk|ghp|gho|ghu|ghs|github_pat|xox[baprs])[-_A-Za-z0-9]{16,}\b/g;
const LONG_SECRET_PATTERN =
  /\b(?=[A-Za-z0-9_+/-]*[A-Za-z])(?=[A-Za-z0-9_+/-]*\d)[A-Za-z0-9_+/-]{32,}={0,2}\b/g;
const OMITTED_METADATA_KEYS = new Set([
  "body",
  "clipboard",
  "content",
  "diff",
  "fullText",
  "prompt",
  "raw",
  "rawContent",
  "response",
  "sourceCode",
  "text",
]);

function nowIso() {
  return new Date().toISOString();
}

function isPlainObject(value) {
  return Boolean(value) && Object.prototype.toString.call(value) === "[object Object]";
}

function redactEditorString(value) {
  if (value == null) {
    return null;
  }
  const input = String(value);
  if (!input) {
    return input;
  }

  return input
    .replace(AUTH_HEADER_PATTERN, (_match, label, scheme) => `${label}: ${scheme} ${SECRET_MARKER}`)
    .replace(BEARER_PATTERN, `Bearer ${SECRET_MARKER}`)
    .replace(SECRET_ASSIGNMENT_PATTERN, (_match, label) => `${label}=${SECRET_MARKER}`)
    .replace(JWT_PATTERN, SECRET_MARKER)
    .replace(WELL_KNOWN_TOKEN_PATTERN, SECRET_MARKER)
    .replace(LONG_SECRET_PATTERN, SECRET_MARKER);
}

function redactPathSegment(segment) {
  if (!segment || segment === "." || segment === "..") {
    return segment;
  }
  if (SECRET_SEGMENT_PATTERN.test(segment)) {
    return SECRET_MARKER;
  }
  return redactEditorString(segment);
}

function redactFilePath(value) {
  if (!value || typeof value !== "string") {
    return null;
  }

  return value
    .trim()
    .split(/([/\\]+)/)
    .map((part) => (/^[/\\]+$/.test(part) ? part : redactPathSegment(part)))
    .join("");
}

function redactPathname(pathname) {
  if (!pathname || typeof pathname !== "string") {
    return pathname;
  }

  return pathname
    .split("/")
    .map((segment) => redactPathSegment(safeDecode(segment)))
    .map((segment) => encodeURIComponent(segment).replace(/%5B/g, "[").replace(/%5D/g, "]"))
    .join("/");
}

function safeDecode(value) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function redactUriString(value) {
  if (!value || typeof value !== "string") {
    return null;
  }

  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed = new URL(trimmed);
    parsed.search = "";
    parsed.hash = "";
    parsed.pathname = redactPathname(parsed.pathname);
    return redactEditorString(parsed.toString());
  } catch {
    const withoutQuery = trimmed.split("#")[0].split("?")[0];
    return redactEditorString(redactFilePath(withoutQuery) ?? withoutQuery);
  }
}

function uriToString(uri) {
  if (!uri) {
    return null;
  }
  if (typeof uri === "string") {
    return uri;
  }
  if (typeof uri.toString === "function" && !uri.fsPath) {
    return uri.toString();
  }
  if (uri.scheme === "file" && uri.fsPath) {
    const normalized = uri.fsPath.replace(/\\/g, "/");
    const prefix = normalized.startsWith("/") ? "file://" : "file:///";
    return `${prefix}${normalized}`;
  }
  if (uri.scheme && uri.path) {
    const authority = uri.authority ? `//${uri.authority}` : "";
    return `${uri.scheme}:${authority}${uri.path}`;
  }
  if (uri.fsPath) {
    return uri.fsPath;
  }
  return null;
}

function pathBaseName(value) {
  if (!value || typeof value !== "string") {
    return null;
  }
  const normalized = value.replace(/\\/g, "/");
  return normalized.slice(normalized.lastIndexOf("/") + 1) || normalized;
}

function normalizeNumber(value) {
  return Number.isFinite(value) ? value : null;
}

function normalizePosition(value) {
  if (!value) {
    return null;
  }
  return {
    line: normalizeNumber(value.line),
    character: normalizeNumber(value.character),
  };
}

function comparePositions(left, right) {
  if (!left || !right) {
    return 0;
  }
  if (left.line !== right.line) {
    return left.line < right.line ? -1 : 1;
  }
  if (left.character !== right.character) {
    return left.character < right.character ? -1 : 1;
  }
  return 0;
}

function normalizeSelection(selection, fallbackCursor) {
  if (!selection) {
    return {
      start: fallbackCursor,
      end: fallbackCursor,
    };
  }

  const active = normalizePosition(selection.active) ?? fallbackCursor;
  const anchor = normalizePosition(selection.anchor) ?? active;
  const directStart = normalizePosition(selection.start);
  const directEnd = normalizePosition(selection.end);
  if (directStart || directEnd) {
    return {
      start: directStart ?? directEnd,
      end: directEnd ?? directStart,
    };
  }

  return comparePositions(anchor, active) <= 0
    ? { start: anchor, end: active }
    : { start: active, end: anchor };
}

function sanitizeMetadataValue(value, key = "") {
  if (value == null) {
    return null;
  }
  if (OMITTED_METADATA_KEYS.has(key)) {
    return OMITTED_MARKER;
  }
  if (typeof value === "string") {
    return looksLikeUri(value) ? redactUriString(value) : redactEditorString(value);
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((item) => sanitizeMetadataValue(item));
  }
  if (isPlainObject(value)) {
    const output = {};
    for (const [childKey, childValue] of Object.entries(value)) {
      output[childKey] = sanitizeMetadataValue(childValue, childKey);
    }
    return output;
  }
  return redactEditorString(String(value));
}

function looksLikeUri(value) {
  return /^[a-z][a-z0-9+.-]*:/i.test(value) || value.includes("?") || value.includes("#");
}

function normalizeWorkspace(input = {}) {
  const folders = Array.isArray(input.folders)
    ? input.folders
    : Array.isArray(input.workspaceFolders)
      ? input.workspaceFolders
      : [];

  return {
    name: sanitizeMetadataValue(input.name ?? input.workspaceName ?? null),
    folders: folders
      .map((folder) => {
        if (typeof folder === "string") {
          return redactFilePath(folder);
        }
        return redactFilePath(folder?.uri?.fsPath ?? folder?.fsPath ?? folder?.name ?? null);
      })
      .filter(Boolean),
  };
}

function extractDocument(input = {}, options = {}) {
  const uri = input.uri ?? null;
  const uriString = redactUriString(uriToString(uri) ?? input.uriString ?? null);
  const rawFilePath = input.filePath ?? input.fileName ?? uri?.fsPath ?? null;
  const filePath = redactFilePath(rawFilePath);
  const rawFileName = input.shortFileName ?? pathBaseName(input.fileName ?? input.filePath ?? uri?.fsPath ?? null);
  const fileName = rawFileName ? redactPathSegment(rawFileName) : null;
  const cursor = normalizePosition(input.cursor ?? input.position ?? input.selection?.active) ?? {
    line: null,
    character: null,
  };
  const selection = normalizeSelection(input.selection, cursor);
  const contentHash = options.includeContentHash ? hashDocumentContent(input) : null;

  return {
    uri: uriString,
    scheme: sanitizeMetadataValue(uri?.scheme ?? input.scheme ?? null),
    filePath,
    fileName,
    languageId: sanitizeMetadataValue(input.languageId ?? null),
    isUntitled: Boolean(input.isUntitled),
    isDirty: Boolean(input.isDirty),
    lineCount: Number.isFinite(input.lineCount) ? input.lineCount : null,
    cursor,
    selection,
    contentCaptured: false,
    contentHash,
  };
}

function hashDocumentContent(document) {
  if (typeof document?.getText !== "function") {
    return null;
  }
  const text = document.getText();
  if (typeof text !== "string") {
    return null;
  }
  return crypto.createHash("sha256").update(text).digest("hex");
}

function isSensitiveContext(input, normalized) {
  const values = [
    input?.document?.fileName,
    input?.document?.filePath,
    input?.document?.uri?.fsPath,
    normalized?.document?.fileName,
    normalized?.document?.filePath,
    normalized?.document?.uri,
  ].filter(Boolean);

  if (values.some((value) => SECRET_SEGMENT_PATTERN.test(String(value)))) {
    return true;
  }
  return ["dotenv", "properties", "ssh_config"].includes(String(normalized?.document?.languageId ?? "").toLowerCase());
}

function normalizeEditorContext(input = {}, options = {}) {
  const document = extractDocument(input.document ?? {}, options);
  const workspace = normalizeWorkspace(input.workspace ?? input);
  const metadata = sanitizeMetadataValue(input.metadata ?? {});
  const event = {
    type: "worktrace.editor_context",
    schemaVersion: 1,
    source: sanitizeMetadataValue(input.source ?? DEFAULT_SOURCE),
    capturedAt: sanitizeMetadataValue(input.capturedAt ?? options.capturedAt ?? nowIso()),
    eventType: sanitizeMetadataValue(input.eventType ?? "active_editor_changed"),
    app: sanitizeMetadataValue(input.app ?? input.appName ?? "Visual Studio Code"),
    workspace,
    document,
    sensitivity: "normal",
    metadata,
  };
  event.sensitivity = isSensitiveContext(input, event) ? "sensitive" : "normal";
  return event;
}

function collectActiveEditorContext(input = {}, options = {}) {
  const editor = input.activeTextEditor ?? null;
  const document = editor?.document ?? {};
  return normalizeEditorContext(
    {
      source: input.source ?? DEFAULT_SOURCE,
      capturedAt: input.capturedAt ?? (typeof input.now === "function" ? input.now() : nowIso()),
      eventType: input.eventType ?? "active_editor_changed",
      app: input.appName ?? input.app ?? "Visual Studio Code",
      workspace: {
        name: input.workspaceName ?? input.workspace?.name ?? null,
        folders: input.workspaceFolders ?? input.workspace?.folders ?? [],
      },
      document: {
        uri: document.uri ?? null,
        filePath: document.uri?.fsPath ?? document.fileName ?? null,
        fileName: document.fileName ?? null,
        languageId: document.languageId ?? null,
        isUntitled: document.isUntitled ?? false,
        isDirty: document.isDirty ?? false,
        lineCount: document.lineCount ?? null,
        selection: editor?.selection ?? null,
        getText: document.getText?.bind(document),
      },
      metadata: {
        viewColumn: editor?.viewColumn ?? null,
      },
    },
    options,
  );
}

module.exports = {
  collectActiveEditorContext,
  normalizeEditorContext,
  redactEditorString,
  redactFilePath,
  redactUriString,
};
