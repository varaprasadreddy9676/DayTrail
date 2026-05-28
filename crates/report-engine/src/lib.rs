#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyReport {
    pub date: String,
    pub sessions: Vec<SessionSummary>,
    pub tasks: Vec<TaskSummary>,
    pub unclosed_loops: Vec<LoopSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub label: String,
    pub started_at: String,
    pub ended_at: String,
    pub active_minutes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    pub title: String,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopSummary {
    pub subject: String,
    pub from: String,
    pub age_hours: u64,
}

pub fn export_markdown(report: &DailyReport) -> String {
    let mut output = String::new();
    output.push_str(&format!("# DayTrail Report - {}\n\n", clean(&report.date)));

    output.push_str("## Sessions\n");
    if report.sessions.is_empty() {
        output.push_str("No sessions.\n");
    } else {
        for session in &report.sessions {
            output.push_str(&format!(
                "- {}-{} {} ({}m active)\n",
                clean(&session.started_at),
                clean(&session.ended_at),
                clean(&session.label),
                session.active_minutes
            ));
        }
    }

    output.push_str("\n## Tasks\n");
    if report.tasks.is_empty() {
        output.push_str("No tasks.\n");
    } else {
        for task in &report.tasks {
            let marker = if task.done { "x" } else { " " };
            output.push_str(&format!("- [{}] {}\n", marker, clean(&task.title)));
        }
    }

    output.push_str("\n## Unclosed Loops\n");
    if report.unclosed_loops.is_empty() {
        output.push_str("No unclosed loops.\n");
    } else {
        for loop_item in &report.unclosed_loops {
            output.push_str(&format!(
                "- {} from {} ({}h old)\n",
                clean(&loop_item.subject),
                clean(&loop_item.from),
                loop_item.age_hours
            ));
        }
    }

    output
}

fn clean(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_daily_report_markdown() {
        let report = DailyReport {
            date: "2026-05-23".to_string(),
            sessions: vec![SessionSummary {
                label: "Deep work".to_string(),
                started_at: "09:00".to_string(),
                ended_at: "10:30".to_string(),
                active_minutes: 90,
            }],
            tasks: vec![TaskSummary {
                title: "Fix checkout bug".to_string(),
                done: false,
            }],
            unclosed_loops: Vec::new(),
        };

        let markdown = export_markdown(&report);

        assert!(markdown.starts_with("# DayTrail Report - 2026-05-23"));
        assert!(markdown.contains("- 09:00-10:30 Deep work (90m active)"));
        assert!(markdown.contains("- [ ] Fix checkout bug"));
        assert!(markdown.contains("No unclosed loops."));
    }
}
