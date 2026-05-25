param(
  [switch]$PrintProfileSnippet,
  [string]$CommandText = $env:WORKTRACE_TERMINAL_COMMAND,
  [string]$EventType = $(if ($env:WORKTRACE_TERMINAL_EVENT) { $env:WORKTRACE_TERMINAL_EVENT } else { "terminal_context" }),
  [string]$OutFile = $(if ($env:DAYTRAIL_TERMINAL_BRIDGE) { $env:DAYTRAIL_TERMINAL_BRIDGE } elseif ($env:WORKTRACE_TERMINAL_BRIDGE) { $env:WORKTRACE_TERMINAL_BRIDGE } else { Join-Path $HOME ".daytrail\terminal-bridge.json" })
)

$ErrorActionPreference = "Stop"

if ($PrintProfileSnippet) {
  $scriptPath = $PSCommandPath
  @"
if (Get-Module -ListAvailable PSReadLine) {
  Set-PSReadLineOption -AddToHistoryHandler {
    param([string]`$line)
    `$env:WORKTRACE_TERMINAL_EVENT = "command"
    `$env:WORKTRACE_TERMINAL_COMMAND = `$line
    & "$scriptPath" | Out-Null
    return `$true
  }
}

if (Test-Path function:\prompt) {
  `$script:WorkTraceOriginalPrompt = (Get-Command prompt).ScriptBlock
}
function global:prompt {
  `$env:WORKTRACE_TERMINAL_EVENT = "prompt"
  Remove-Item Env:\WORKTRACE_TERMINAL_COMMAND -ErrorAction SilentlyContinue
  & "$scriptPath" | Out-Null
  if (`$script:WorkTraceOriginalPrompt) {
    & `$script:WorkTraceOriginalPrompt
  } else {
    "PS `$(`$executionContext.SessionState.Path.CurrentLocation)> "
  }
}
"@
  exit 0
}

function Redact-WorkTraceCommand {
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

$parent = Split-Path -Parent $OutFile
if ($parent) {
  New-Item -ItemType Directory -Force -Path $parent | Out-Null
}

$metadata = [ordered]@{
  cwd = (Get-Location).Path
  shell = "PowerShell"
  terminal = $(if ($env:WT_SESSION) { "Windows Terminal" } else { $env:TERM_PROGRAM })
  eventType = $EventType
  lastCommand = Redact-WorkTraceCommand $CommandText
  updatedAt = (Get-Date).ToUniversalTime().ToString("o")
}

($metadata | ConvertTo-Json -Depth 5) | Set-Content -LiteralPath $OutFile -Encoding UTF8
Write-Output $OutFile
