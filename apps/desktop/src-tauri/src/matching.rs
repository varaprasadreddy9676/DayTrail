//! Pattern matching engine for task rules.
//!
//! A [`TaskMatchRule`](crate::models::TaskMatchRule) describes how an activity
//! (a captured [`SourceEvent`](crate::models::SourceEvent)) should be matched so
//! it can be auto-linked to a task. Matching runs entirely on the already
//! redacted text stored on the event, so it stays local and privacy-safe.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::models::SourceEvent;

/// Which field of an activity a rule inspects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchField {
    Title,
    Url,
    App,
    Source,
    /// Match against title, url, or app (whichever is present).
    Any,
}

impl MatchField {
    pub fn as_db_value(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Url => "url",
            Self::App => "app",
            Self::Source => "source",
            Self::Any => "any",
        }
    }
}

impl TryFrom<&str> for MatchField {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "title" => Ok(Self::Title),
            "url" => Ok(Self::Url),
            "app" => Ok(Self::App),
            "source" => Ok(Self::Source),
            "any" => Ok(Self::Any),
            other => anyhow::bail!("unknown match field: {other}"),
        }
    }
}

/// How the pattern is interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatcherType {
    /// Plain substring match (default — easiest for non-technical users).
    Contains,
    /// Shell-style wildcard with `*` (any run) and `?` (single char).
    Wildcard,
    /// Full regular expression.
    Regex,
}

impl MatcherType {
    pub fn as_db_value(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Wildcard => "wildcard",
            Self::Regex => "regex",
        }
    }
}

impl TryFrom<&str> for MatcherType {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "contains" => Ok(Self::Contains),
            "wildcard" => Ok(Self::Wildcard),
            "regex" => Ok(Self::Regex),
            other => anyhow::bail!("unknown matcher type: {other}"),
        }
    }
}

/// A compiled, ready-to-evaluate rule. Construct once, reuse across events.
pub struct CompiledRule {
    field: MatchField,
    matcher: Matcher,
}

enum Matcher {
    Contains { needle: String, case_sensitive: bool },
    Regex(Regex),
}

impl CompiledRule {
    /// Compile a rule's matcher. Returns an error for an invalid regex or an
    /// empty pattern, so callers can reject a bad rule before it is saved.
    pub fn compile(
        field: MatchField,
        matcher: MatcherType,
        pattern: &str,
        case_sensitive: bool,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(!pattern.trim().is_empty(), "rule pattern cannot be empty");
        let matcher = match matcher {
            MatcherType::Contains => Matcher::Contains {
                needle: if case_sensitive {
                    pattern.to_string()
                } else {
                    pattern.to_lowercase()
                },
                case_sensitive,
            },
            MatcherType::Wildcard => {
                let regex = RegexBuilder::new(&wildcard_to_regex(pattern))
                    .case_insensitive(!case_sensitive)
                    .build()
                    .map_err(|err| anyhow::anyhow!("invalid wildcard pattern: {err}"))?;
                Matcher::Regex(regex)
            }
            MatcherType::Regex => {
                let regex = RegexBuilder::new(pattern)
                    .case_insensitive(!case_sensitive)
                    .size_limit(1 << 20)
                    .build()
                    .map_err(|err| anyhow::anyhow!("invalid regular expression: {err}"))?;
                Matcher::Regex(regex)
            }
        };
        Ok(Self { field, matcher })
    }

    /// True when the rule matches the given activity.
    pub fn matches(&self, event: &SourceEvent) -> bool {
        self.candidate_values(event)
            .into_iter()
            .flatten()
            .any(|value| self.matches_value(value))
    }

    fn candidate_values<'a>(&self, event: &'a SourceEvent) -> Vec<Option<&'a str>> {
        match self.field {
            MatchField::Title => vec![event.title.as_deref()],
            MatchField::Url => vec![event.url_redacted.as_deref(), event.domain.as_deref()],
            MatchField::App => vec![event.app.as_deref()],
            MatchField::Source => vec![Some(event.source.as_str())],
            MatchField::Any => vec![
                event.title.as_deref(),
                event.url_redacted.as_deref(),
                event.app.as_deref(),
            ],
        }
    }

    fn matches_value(&self, value: &str) -> bool {
        match &self.matcher {
            Matcher::Contains {
                needle,
                case_sensitive,
            } => {
                if *case_sensitive {
                    value.contains(needle)
                } else {
                    value.to_lowercase().contains(needle)
                }
            }
            Matcher::Regex(regex) => regex.is_match(value),
        }
    }
}

/// Translate a shell-style wildcard pattern into an anchored regex.
/// `*` matches any run of characters, `?` matches a single character, and every
/// other character is matched literally.
fn wildcard_to_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 2);
    out.push('^');
    for ch in pattern.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            other => out.push_str(&regex::escape(&other.to_string())),
        }
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: Option<&str>, url: Option<&str>, app: Option<&str>, source: &str) -> SourceEvent {
        SourceEvent {
            id: "e1".into(),
            source: source.into(),
            event_type: "window".into(),
            app: app.map(Into::into),
            title: title.map(Into::into),
            domain: None,
            url_redacted: url.map(Into::into),
            workspace_key: None,
            started_at: 0,
            ended_at: 0,
            duration_ms: 0,
            sensitivity: "normal".into(),
            metadata_json: None,
            created_at: 0,
        }
    }

    #[test]
    fn contains_is_case_insensitive_by_default() {
        let rule =
            CompiledRule::compile(MatchField::Title, MatcherType::Contains, "[PROJECT-A]", false)
                .unwrap();
        assert!(rule.matches(&event(Some("fix [project-a] bug"), None, None, "window")));
        assert!(!rule.matches(&event(Some("unrelated"), None, None, "window")));
    }

    #[test]
    fn contains_respects_case_sensitivity() {
        let rule =
            CompiledRule::compile(MatchField::Title, MatcherType::Contains, "PROJ", true).unwrap();
        assert!(rule.matches(&event(Some("PROJ work"), None, None, "window")));
        assert!(!rule.matches(&event(Some("proj work"), None, None, "window")));
    }

    #[test]
    fn wildcard_anchors_and_expands() {
        let rule =
            CompiledRule::compile(MatchField::Url, MatcherType::Wildcard, "*github.com*", false)
                .unwrap();
        assert!(rule.matches(&event(None, Some("https://github.com/x"), None, "browser")));
        assert!(!rule.matches(&event(None, Some("https://example.com"), None, "browser")));

        let single =
            CompiledRule::compile(MatchField::App, MatcherType::Wildcard, "Code?", false).unwrap();
        assert!(single.matches(&event(None, None, Some("Codes"), "window")));
        assert!(!single.matches(&event(None, None, Some("Code"), "window")));
    }

    #[test]
    fn regex_matches_and_rejects_invalid() {
        let rule = CompiledRule::compile(
            MatchField::Title,
            MatcherType::Regex,
            r"JIRA-\d+",
            false,
        )
        .unwrap();
        assert!(rule.matches(&event(Some("ticket JIRA-1234 done"), None, None, "window")));
        assert!(!rule.matches(&event(Some("no ticket"), None, None, "window")));

        assert!(CompiledRule::compile(MatchField::Title, MatcherType::Regex, "(", false).is_err());
    }

    #[test]
    fn any_field_checks_title_url_and_app() {
        let rule =
            CompiledRule::compile(MatchField::Any, MatcherType::Contains, "acme", false).unwrap();
        assert!(rule.matches(&event(Some("Acme planning"), None, None, "window")));
        assert!(rule.matches(&event(None, Some("https://acme.test"), None, "browser")));
        assert!(rule.matches(&event(None, None, Some("ACME.app"), "window")));
        assert!(!rule.matches(&event(Some("other"), None, None, "window")));
    }

    #[test]
    fn empty_pattern_is_rejected() {
        assert!(
            CompiledRule::compile(MatchField::Title, MatcherType::Contains, "   ", false).is_err()
        );
    }

    #[test]
    fn wildcard_special_chars_are_escaped() {
        let rule =
            CompiledRule::compile(MatchField::Title, MatcherType::Wildcard, "a.b*", false).unwrap();
        assert!(rule.matches(&event(Some("a.bc"), None, None, "window")));
        assert!(!rule.matches(&event(Some("axbc"), None, None, "window")));
    }
}
