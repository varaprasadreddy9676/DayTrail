#!/usr/bin/env bash
set -euo pipefail

OUT_FILE="${DAYTRAIL_TERMINAL_BRIDGE:-${WORKTRACE_TERMINAL_BRIDGE:-$HOME/.daytrail/terminal-bridge.json}}"
SCRIPT_PATH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

if [[ "${1:-}" == "--print-zsh-hook" ]]; then
  cat <<HOOK
worktrace_terminal_bridge_precmd() {
  WORKTRACE_TERMINAL_EVENT=prompt "$SCRIPT_PATH" >/dev/null 2>&1
}

worktrace_terminal_bridge_preexec() {
  WORKTRACE_TERMINAL_EVENT=command WORKTRACE_TERMINAL_COMMAND="\$1" "$SCRIPT_PATH" >/dev/null 2>&1
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd worktrace_terminal_bridge_precmd
add-zsh-hook preexec worktrace_terminal_bridge_preexec
HOOK
  exit 0
fi

if [[ "${1:-}" == "--print-bash-hook" ]]; then
  cat <<HOOK
worktrace_terminal_bridge_prompt() {
  WORKTRACE_TERMINAL_EVENT=prompt "$SCRIPT_PATH" >/dev/null 2>&1
}

worktrace_terminal_bridge_debug() {
  local command="\$BASH_COMMAND"
  case "\$command" in
    worktrace_terminal_bridge_*|"$SCRIPT_PATH"*) return ;;
  esac
  WORKTRACE_TERMINAL_EVENT=command WORKTRACE_TERMINAL_COMMAND="\$command" "$SCRIPT_PATH" >/dev/null 2>&1
}

PROMPT_COMMAND="worktrace_terminal_bridge_prompt\${PROMPT_COMMAND:+;\$PROMPT_COMMAND}"
trap worktrace_terminal_bridge_debug DEBUG
HOOK
  exit 0
fi

mkdir -p "$(dirname "$OUT_FILE")"

node -e '
const fs = require("node:fs");
const out = process.argv[1];
const redacted = (process.env.WORKTRACE_TERMINAL_COMMAND || "")
  .split(/\s+/)
  .reduce((parts, part, index, words) => {
    if (!part) return parts;
    const previous = (words[index - 1] || "").toLowerCase();
    const lower = part.toLowerCase();
    if (["-p", "--password", "--pass", "--token", "--api-key", "--apikey", "--secret"].includes(previous)) {
      parts.push("[redacted]");
    } else if (/(password|passwd|token|api[_-]?key|secret|key)=/i.test(part)) {
      parts.push(part.replace(/=.*/, "=[redacted]"));
    } else {
      parts.push(part);
    }
    return parts;
  }, [])
  .join(" ");
const normalizeTerminal = (value) => {
  const lower = String(value || "").toLowerCase();
  if (lower === "vscode" || lower.includes("visual studio code")) return "VS Code";
  if (lower.includes("warp")) return "Warp";
  if (lower.includes("iterm")) return "iTerm";
  if (lower.includes("terminal")) return "Terminal";
  return value || null;
};
const metadata = {
  cwd: process.cwd(),
  shell: process.env.SHELL || null,
  terminal: normalizeTerminal(process.env.TERM_PROGRAM || process.env.TERM),
  eventType: process.env.WORKTRACE_TERMINAL_EVENT || "terminal_context",
  lastCommand: redacted || null,
  updatedAt: new Date().toISOString()
};
fs.writeFileSync(out, `${JSON.stringify(metadata, null, 2)}\n`);
' "$OUT_FILE"

echo "$OUT_FILE"
