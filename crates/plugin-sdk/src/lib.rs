#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiVersion {
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    EventSource,
    Command,
    ReportExport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCommand {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub api_version: ApiVersion,
    pub capabilities: Vec<Capability>,
    pub commands: Vec<PluginCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    InvalidId,
    EmptyName,
    EmptyCommandName,
}

impl PluginManifest {
    pub fn new(id: &str, name: &str, api_version: ApiVersion) -> Self {
        Self {
            id: id.trim().to_string(),
            name: name.trim().to_string(),
            api_version,
            capabilities: Vec::new(),
            commands: Vec::new(),
        }
    }

    pub fn with_capability(mut self, capability: Capability) -> Self {
        if !self.capabilities.contains(&capability) {
            self.capabilities.push(capability);
        }
        self
    }

    pub fn with_command(mut self, command: PluginCommand) -> Self {
        self.commands.push(command);
        self
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if !is_valid_plugin_id(&self.id) {
            return Err(ManifestError::InvalidId);
        }

        if self.name.trim().is_empty() {
            return Err(ManifestError::EmptyName);
        }

        if self
            .commands
            .iter()
            .any(|command| command.name.trim().is_empty())
        {
            return Err(ManifestError::EmptyCommandName);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEventKind {
    Activity,
    InboxMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEvent {
    pub id: String,
    pub source: String,
    pub title: String,
    pub kind: PluginEventKind,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
}

impl PluginEvent {
    pub fn activity(
        id: &str,
        source: &str,
        title: &str,
        started_at_ms: u64,
        ended_at_ms: u64,
    ) -> Self {
        Self {
            id: id.trim().to_string(),
            source: source.trim().to_string(),
            title: title.trim().to_string(),
            kind: PluginEventKind::Activity,
            started_at_ms,
            ended_at_ms,
        }
    }

    pub fn duration_ms(&self) -> u64 {
        self.ended_at_ms.saturating_sub(self.started_at_ms)
    }
}

fn is_valid_plugin_id(id: &str) -> bool {
    let mut has_dot = false;
    let mut previous_dot = true;

    for byte in id.bytes() {
        if byte == b'.' {
            if previous_dot {
                return false;
            }
            has_dot = true;
            previous_dot = true;
        } else if byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        {
            previous_dot = false;
        } else {
            return false;
        }
    }

    has_dot && !previous_dot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_plugin_manifest_and_activity_event_types() {
        let manifest = PluginManifest::new("acme.timer", "Acme Timer", ApiVersion::V1)
            .with_capability(Capability::EventSource)
            .with_command(PluginCommand {
                name: "sync".to_string(),
                description: "Sync recent activity".to_string(),
            });

        assert!(manifest.validate().is_ok());

        let event = PluginEvent::activity("event-1", "Timer", "Focus block", 0, 1_000);
        assert_eq!(event.id, "event-1");
        assert_eq!(event.kind, PluginEventKind::Activity);
        assert_eq!(event.duration_ms(), 1_000);
    }
}
