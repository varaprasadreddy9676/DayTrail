param(
  [switch]$PrintProfileSnippet,
  [string]$CommandText = $(if ($env:DAYTRAIL_TERMINAL_COMMAND) { $env:DAYTRAIL_TERMINAL_COMMAND } else { $env:WORKTRACE_TERMINAL_COMMAND }),
  [string]$EventType = $(if ($env:DAYTRAIL_TERMINAL_EVENT) { $env:DAYTRAIL_TERMINAL_EVENT } elseif ($env:WORKTRACE_TERMINAL_EVENT) { $env:WORKTRACE_TERMINAL_EVENT } else { "terminal_context" }),
  [string]$OutFile = $(if ($env:DAYTRAIL_TERMINAL_BRIDGE) { $env:DAYTRAIL_TERMINAL_BRIDGE } elseif ($env:WORKTRACE_TERMINAL_BRIDGE) { $env:WORKTRACE_TERMINAL_BRIDGE } else { Join-Path $HOME ".daytrail\terminal-bridge.json" })
)

$ErrorActionPreference = "Stop"

if ($PrintProfileSnippet) {
  $scriptPath = $PSCommandPath
  @"
if (Get-Module -ListAvailable PSReadLine) {
  Set-PSReadLineOption -AddToHistoryHandler {
    param([string]`$line)
    `$env:DAYTRAIL_TERMINAL_EVENT = "command"
    `$env:DAYTRAIL_TERMINAL_COMMAND = `$line
    & "$scriptPath" | Out-Null
    return `$true
  }
}

if (Test-Path function:\prompt) {
  `$script:DayTrailOriginalPrompt = (Get-Command prompt).ScriptBlock
}
function global:prompt {
  `$env:DAYTRAIL_TERMINAL_EVENT = "prompt"
  Remove-Item Env:\DAYTRAIL_TERMINAL_COMMAND -ErrorAction SilentlyContinue
  & "$scriptPath" | Out-Null
  if (`$script:DayTrailOriginalPrompt) {
    & `$script:DayTrailOriginalPrompt
  } else {
    "PS `$(`$executionContext.SessionState.Path.CurrentLocation)> "
  }
}
"@
  exit 0
}

function Redact-DayTrailCommand {
  param([string]$Value)
  if ([string]::IsNullOrWhiteSpace($Value)) {
    return $null
  }

  $parts = $Value -split "\s+"
  $redacted = New-Object System.Collections.Generic.List[string]
  for ($index = 0; $index -lt $parts.Length; $index++) {
    $part = $parts[$index]
    if ([string]::IsNullOrWhiteSpace($part)) {
      continue
    }
    $previous = if ($index -gt 0) { $parts[$index - 1].ToLowerInvariant() } else { "" }
    if (@("-p", "--password", "--pass", "--token", "--api-key", "--apikey", "--secret") -contains $previous) {
      $redacted.Add("[redacted]")
    } elseif ($part -match "(?i)(password|passwd|token|api[_-]?key|secret|key)=") {
      $redacted.Add(($part -replace "=.*", "=[redacted]"))
    } else {
      $redacted.Add($part)
    }
  }
  return ($redacted -join " ")
}

function Normalize-DayTrailTerminal {
  param([string]$Value)

  $cleaned = "$Value".Trim()
  $normalized = $cleaned.ToLowerInvariant()
  if (-not $normalized -or @("dumb", "unknown", "ansi", "vt100", "xterm", "xterm-256color", "screen", "tmux") -contains $normalized) {
    return "Terminal"
  }
  if ($normalized -eq "vscode" -or $normalized.Contains("visual studio code")) {
    return "VS Code"
  }
  if ($normalized.Contains("warp")) {
    return "Warp"
  }
  if ($normalized.Contains("iterm")) {
    return "iTerm"
  }
  if ($normalized.Contains("terminal")) {
    return "Terminal"
  }
  return $cleaned
}

$parent = Split-Path -Parent $OutFile
if ($parent) {
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
}

$terminalValue = if ($env:WT_SESSION) { "Windows Terminal" } else { $env:TERM_PROGRAM }

$metadata = [ordered]@{
  cwd = (Get-Location).Path
  shell = "PowerShell"
  terminal = Normalize-DayTrailTerminal $terminalValue
  eventType = $EventType
  lastCommand = Redact-DayTrailCommand $CommandText
  updatedAt = (Get-Date).ToUniversalTime().ToString("o")
}

($metadata | ConvertTo-Json -Depth 5) | Set-Content -LiteralPath $OutFile -Encoding UTF8
Write-Output $OutFile
