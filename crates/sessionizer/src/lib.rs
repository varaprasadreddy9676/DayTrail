use std::collections::HashMap;

use worktrace_core::NormalizedEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionOptions {
    pub idle_gap_ms: u64,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            idle_gap_ms: 5 * 60 * 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub active_ms: u64,
    pub primary_app: Option<String>,
    pub events: Vec<NormalizedEvent>,
}

pub fn group_sessions(mut events: Vec<NormalizedEvent>, options: SessionOptions) -> Vec<Session> {
    events.sort_by_key(|event| (event.started_at_ms, event.ended_at_ms));

    let mut sessions = Vec::new();
    let mut current: Vec<NormalizedEvent> = Vec::new();
    let mut last_end = None;

    for event in events {
        let starts_new = last_end
            .map(|ended_at| event.started_at_ms.saturating_sub(ended_at) > options.idle_gap_ms)
            .unwrap_or(false);

        if starts_new && !current.is_empty() {
            sessions.push(build_session(std::mem::take(&mut current)));
        }

        last_end = Some(event.ended_at_ms);
        current.push(event);
    }

    if !current.is_empty() {
        sessions.push(build_session(current));
    }

    sessions
}

fn build_session(events: Vec<NormalizedEvent>) -> Session {
    let started_at_ms = events.first().map(|event| event.started_at_ms).unwrap_or(0);
    let ended_at_ms = events
        .last()
        .map(|event| event.ended_at_ms)
        .unwrap_or(started_at_ms);
    let active_ms = events.iter().map(NormalizedEvent::duration_ms).sum();
    let primary_app = primary_app(&events);

    Session {
        started_at_ms,
        ended_at_ms,
        active_ms,
        primary_app,
        events,
    }
}

fn primary_app(events: &[NormalizedEvent]) -> Option<String> {
    let mut totals: HashMap<&str, u64> = HashMap::new();
    for event in events {
        *totals.entry(&event.app).or_default() += event.duration_ms();
    }

    totals
        .into_iter()
        .max_by_key(|(_, duration)| *duration)
        .map(|(app, _)| app.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktrace_core::{EventKind, NormalizedEvent};

    #[test]
    fn groups_events_into_sessions_by_idle_gap() {
        let events = vec![
            NormalizedEvent::new("1", "Code", "main.rs", None, 0, 1_000, EventKind::Window)
                .unwrap(),
            NormalizedEvent::new("2", "Code", "lib.rs", None, 1_200, 2_000, EventKind::Window)
                .unwrap(),
            NormalizedEvent::new("3", "Mail", "Inbox", None, 5_500, 6_000, EventKind::Window)
                .unwrap(),
        ];

        let sessions = group_sessions(events, SessionOptions { idle_gap_ms: 1_000 });

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].started_at_ms, 0);
        assert_eq!(sessions[0].ended_at_ms, 2_000);
        assert_eq!(sessions[0].active_ms, 1_800);
        assert_eq!(sessions[0].primary_app.as_deref(), Some("Code"));
        assert_eq!(sessions[1].events.len(), 1);
    }
}
