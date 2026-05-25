use std::collections::HashMap;

use worktrace_core::NormalizedEvent;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskExtractionOptions {
    pub min_event_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCandidate {
    pub title: String,
    pub canonical_title: String,
    pub evidence_event_ids: Vec<String>,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
    pub total_active_ms: u64,
}

pub fn extract_tasks(
    events: &[NormalizedEvent],
    options: TaskExtractionOptions,
) -> Vec<TaskCandidate> {
    let mut tasks = Vec::new();
    let mut indexes: HashMap<String, usize> = HashMap::new();

    for event in events {
        if event.duration_ms() < options.min_event_duration_ms {
            continue;
        }

        let Some(title) = extract_task_title(&event.title) else {
            continue;
        };

        let canonical = canonicalize(&title);
        if canonical.is_empty() {
            continue;
        }

        if let Some(index) = indexes.get(&canonical).copied() {
            let task: &mut TaskCandidate = &mut tasks[index];
            task.evidence_event_ids.push(event.id.clone());
            task.first_seen_ms = task.first_seen_ms.min(event.started_at_ms);
            task.last_seen_ms = task.last_seen_ms.max(event.ended_at_ms);
            task.total_active_ms += event.duration_ms();
        } else {
            indexes.insert(canonical.clone(), tasks.len());
            tasks.push(TaskCandidate {
                title,
                canonical_title: canonical,
                evidence_event_ids: vec![event.id.clone()],
                first_seen_ms: event.started_at_ms,
                last_seen_ms: event.ended_at_ms,
                total_active_ms: event.duration_ms(),
            });
        }
    }

    tasks
}

fn extract_task_title(title: &str) -> Option<String> {
    let base = trim_source_suffix(title);
    let lower = base.to_ascii_lowercase();
    let actionable = [
        "fix ",
        "review ",
        "write ",
        "implement ",
        "debug ",
        "ship ",
        "reply ",
        "follow up",
        "plan ",
        "draft ",
        "send ",
        "update ",
    ]
    .iter()
    .any(|prefix| lower.starts_with(prefix));

    actionable.then(|| sentence_case(base))
}

fn trim_source_suffix(title: &str) -> &str {
    [" - ", " | ", " · "]
        .iter()
        .filter_map(|separator| title.find(separator))
        .min()
        .map(|index| title[..index].trim())
        .unwrap_or_else(|| title.trim())
}

fn sentence_case(value: &str) -> String {
    let mut chars = value.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.as_str())
}

fn canonicalize(value: &str) -> String {
    let mut canonical = String::with_capacity(value.len());
    let mut previous_space = true;

    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || ch == '#' {
            canonical.push(ch);
            previous_space = false;
        } else if !previous_space {
            canonical.push(' ');
            previous_space = true;
        }
    }

    canonical.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktrace_core::{EventKind, NormalizedEvent};

    #[test]
    fn extracts_actionable_tasks_and_deduplicates_by_canonical_title() {
        let events = vec![
            NormalizedEvent::new(
                "1",
                "Browser",
                "Fix checkout bug - Jira",
                None,
                0,
                1_000,
                EventKind::Browser,
            )
            .unwrap(),
            NormalizedEvent::new(
                "2",
                "Code",
                "fix checkout bug",
                None,
                1_100,
                2_000,
                EventKind::Window,
            )
            .unwrap(),
            NormalizedEvent::new(
                "3",
                "Browser",
                "Review PR #42 · GitHub",
                None,
                3_000,
                4_000,
                EventKind::Browser,
            )
            .unwrap(),
            NormalizedEvent::new(
                "4",
                "Slack",
                "general",
                None,
                5_000,
                6_000,
                EventKind::Window,
            )
            .unwrap(),
        ];

        let tasks = extract_tasks(&events, TaskExtractionOptions::default());

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Fix checkout bug");
        assert_eq!(tasks[0].evidence_event_ids, vec!["1", "2"]);
        assert_eq!(tasks[1].title, "Review PR #42");
    }
}
