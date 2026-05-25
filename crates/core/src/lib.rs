#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventKind {
    Window,
    Browser,
    Keyboard,
    Mouse,
    Idle,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvent {
    pub id: String,
    pub app: String,
    pub title: String,
    pub url: Option<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedEvent {
    pub id: String,
    pub app: String,
    pub title: String,
    pub url: Option<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub kind: EventKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizeError {
    EmptyId,
    EmptyApp,
    EmptyTitle,
    InvalidTimeRange,
}

impl NormalizedEvent {
    pub fn new(
        id: &str,
        app: &str,
        title: &str,
        url: Option<&str>,
        started_at_ms: u64,
        ended_at_ms: u64,
        kind: EventKind,
    ) -> Result<Self, NormalizeError> {
        normalize_event(RawEvent {
            id: id.to_string(),
            app: app.to_string(),
            title: title.to_string(),
            url: url.map(str::to_string),
            started_at_ms,
            ended_at_ms,
            kind,
        })
    }

    pub fn duration_ms(&self) -> u64 {
        self.ended_at_ms.saturating_sub(self.started_at_ms)
    }
}

pub fn normalize_event(raw: RawEvent) -> Result<NormalizedEvent, NormalizeError> {
    let id = collapse_whitespace(&raw.id);
    if id.is_empty() {
        return Err(NormalizeError::EmptyId);
    }

    let app = collapse_whitespace(&raw.app);
    if app.is_empty() {
        return Err(NormalizeError::EmptyApp);
    }

    let title = collapse_whitespace(&raw.title);
    if title.is_empty() {
        return Err(NormalizeError::EmptyTitle);
    }

    if raw.ended_at_ms < raw.started_at_ms {
        return Err(NormalizeError::InvalidTimeRange);
    }

    Ok(NormalizedEvent {
        id,
        app,
        title,
        url: raw.url.as_deref().and_then(normalize_url),
        started_at_ms: raw.started_at_ms,
        ended_at_ms: raw.ended_at_ms,
        kind: raw.kind,
    })
}

pub fn coalesce_events(mut events: Vec<NormalizedEvent>, max_gap_ms: u64) -> Vec<NormalizedEvent> {
    events.sort_by_key(|event| (event.started_at_ms, event.ended_at_ms));

    let mut merged: Vec<NormalizedEvent> = Vec::new();
    for event in events {
        if let Some(current) = merged.last_mut() {
            let gap = event.started_at_ms.saturating_sub(current.ended_at_ms);
            if gap <= max_gap_ms && can_coalesce(current, &event) {
                current.id = format!("{}+{}", current.id, event.id);
                current.started_at_ms = current.started_at_ms.min(event.started_at_ms);
                current.ended_at_ms = current.ended_at_ms.max(event.ended_at_ms);
                continue;
            }
        }
        merged.push(event);
    }

    merged
}

fn can_coalesce(left: &NormalizedEvent, right: &NormalizedEvent) -> bool {
    left.app == right.app
        && left.title == right.title
        && left.url == right.url
        && left.kind == right.kind
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_url(value: &str) -> Option<String> {
    let without_fragment = value.trim().split('#').next().unwrap_or_default().trim();
    if without_fragment.is_empty() {
        return None;
    }

    let Some((scheme, rest)) = without_fragment.split_once("://") else {
        return Some(without_fragment.to_string());
    };

    let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, suffix) = rest.split_at(authority_end);

    Some(format!(
        "{}://{}{}",
        scheme.to_ascii_lowercase(),
        authority.to_ascii_lowercase(),
        suffix
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_event_strings_and_urls_without_losing_time() {
        let raw = RawEvent {
            id: " event-1 ".to_string(),
            app: " Safari ".to_string(),
            title: "  Pull Request   ·   GitHub  ".to_string(),
            url: Some("HTTPS://Example.com/Path?x=1#section".to_string()),
            started_at_ms: 1_000,
            ended_at_ms: 2_250,
            kind: EventKind::Browser,
        };

        let event = normalize_event(raw).expect("valid event should normalize");

        assert_eq!(event.id, "event-1");
        assert_eq!(event.app, "Safari");
        assert_eq!(event.title, "Pull Request · GitHub");
        assert_eq!(event.url.as_deref(), Some("https://example.com/Path?x=1"));
        assert_eq!(event.duration_ms(), 1_250);
    }

    #[test]
    fn rejects_events_with_empty_identity_or_inverted_time() {
        let bad = RawEvent {
            id: " ".to_string(),
            app: "Code".to_string(),
            title: "main.rs".to_string(),
            url: None,
            started_at_ms: 10,
            ended_at_ms: 5,
            kind: EventKind::Window,
        };

        assert_eq!(normalize_event(bad), Err(NormalizeError::EmptyId));
    }

    #[test]
    fn coalesces_adjacent_matching_events_with_small_gaps() {
        let events = vec![
            NormalizedEvent::new("1", "Code", "main.rs", None, 0, 1_000, EventKind::Window)
                .unwrap(),
            NormalizedEvent::new(
                "2",
                "Code",
                "main.rs",
                None,
                1_150,
                2_000,
                EventKind::Window,
            )
            .unwrap(),
            NormalizedEvent::new("3", "Code", "lib.rs", None, 2_050, 3_000, EventKind::Window)
                .unwrap(),
        ];

        let merged = coalesce_events(events, 200);

        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "1+2");
        assert_eq!(merged[0].started_at_ms, 0);
        assert_eq!(merged[0].ended_at_ms, 2_000);
        assert_eq!(merged[1].title, "lib.rs");
    }
}
