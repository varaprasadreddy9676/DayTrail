use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    DailyReport,
    WeeklyPlan,
    ReturnMarker,
    SearchAnswer,
    NextBestAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptContext {
    pub kind: PromptKind,
    pub instruction: String,
    pub facts_markdown: String,
    pub max_input_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRequest {
    pub endpoint: String,
    pub model: String,
    pub messages: Vec<PromptMessage>,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptBuildError {
    InvalidProvider(ProviderConfigError),
    MissingEndpoint,
}

impl From<ProviderConfigError> for PromptBuildError {
    fn from(value: ProviderConfigError) -> Self {
        Self::InvalidProvider(value)
    }
}

pub fn build_openai_compatible_request(
    config: &ProviderConfig,
    context: &PromptContext,
) -> Result<PromptRequest, PromptBuildError> {
    config.validate()?;
    let base_url = match &config.provider {
        ProviderKind::OpenAi => config
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        ProviderKind::Anthropic | ProviderKind::Custom(_) => config
            .base_url
            .clone()
            .ok_or(PromptBuildError::MissingEndpoint)?,
    };
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let facts = redact_prompt_text(&context.facts_markdown);
    let facts = truncate_for_prompt(&facts, context.max_input_chars.max(1024));
    let messages = vec![
        PromptMessage {
            role: "system".to_string(),
            content: system_prompt_for(context.kind).to_string(),
        },
        PromptMessage {
            role: "user".to_string(),
            content: format!(
                "{}\n\nUse only the facts below. If facts are missing, say what is missing.\n\n{}",
                redact_prompt_text(&context.instruction),
                facts
            ),
        },
    ];
    let body = json!({
        "model": config.model,
        "messages": messages,
        "temperature": 0.2,
        "stream": false
    });

    Ok(PromptRequest {
        endpoint,
        model: config.model.clone(),
        messages,
        body,
    })
}

pub fn parse_openai_compatible_response(value: &Value) -> Option<String> {
    value
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
}

pub fn summarize_for_audit(value: &str, max_chars: usize) -> String {
    truncate_for_prompt(&redact_prompt_text(value), max_chars.max(32))
}

fn system_prompt_for(kind: PromptKind) -> &'static str {
    match kind {
        PromptKind::DailyReport => {
            "You are DayTrail AI. Produce a concise daily work execution report with closures, open loops, risks, AI-assisted outputs, and next actions. Do not invent facts."
        }
        PromptKind::WeeklyPlan => {
            "You are DayTrail AI. Produce a realistic weekly plan grouped by must close, should progress, can defer, waiting, and at risk. Do not invent facts."
        }
        PromptKind::ReturnMarker => {
            "You are DayTrail AI. Restore the user's mental state for returning to work. Include last clue, stopped point, related sources, and next likely step."
        }
        PromptKind::SearchAnswer => {
            "You are DayTrail AI. Answer from local work-memory search evidence only. Cite the evidence titles in plain text."
        }
        PromptKind::NextBestAction => {
            "You are DayTrail AI. Select the next best action from captured facts. Prefer reply debt, due promises, open AI outputs, and safety-net risks."
        }
    }
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut output = normalized.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

fn redact_prompt_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(redact_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("bearer ")
        || lower.contains("password=")
        || lower.contains("token=")
        || lower.contains("api_key=")
        || lower.contains("apikey=")
        || lower.contains("secret=")
        || token.starts_with("sk-")
        || token.starts_with("ghp_")
        || looks_like_jwt(token)
        || looks_like_secret_token(token)
    {
        "[redacted-secret]".to_string()
    } else {
        token.to_string()
    }
}

fn looks_like_jwt(value: &str) -> bool {
    let parts = value.split('.').collect::<Vec<_>>();
    parts.len() == 3 && parts[0].starts_with("eyJ") && parts.iter().all(|part| part.len() >= 8)
}

fn looks_like_secret_token(value: &str) -> bool {
    value.len() >= 32
        && value.chars().any(|ch| ch.is_ascii_alphabetic())
        && value.chars().any(|ch| ch.is_ascii_digit())
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '+'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiKeySource {
    Env(String),
    Inline(String),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    pub provider: ProviderKind,
    pub model: String,
    pub api_key: ApiKeySource,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderConfigError {
    EmptyModel,
    EmptyProviderName,
    EmptyApiKeyEnv,
    MissingBaseUrl,
    InvalidBaseUrl,
}

impl ProviderConfig {
    pub fn openai(model: &str, api_key: &str) -> Self {
        Self {
            provider: ProviderKind::OpenAi,
            model: model.trim().to_string(),
            api_key: parse_api_key(api_key),
            base_url: None,
        }
    }

    pub fn custom(name: &str, base_url: &str, model: &str, api_key: &str) -> Self {
        Self {
            provider: ProviderKind::Custom(name.trim().to_string()),
            model: model.trim().to_string(),
            api_key: parse_api_key(api_key),
            base_url: Some(base_url.trim().trim_end_matches('/').to_string()),
        }
    }

    pub fn validate(&self) -> Result<(), ProviderConfigError> {
        if self.model.trim().is_empty() {
            return Err(ProviderConfigError::EmptyModel);
        }

        if matches!(&self.provider, ProviderKind::Custom(name) if name.trim().is_empty()) {
            return Err(ProviderConfigError::EmptyProviderName);
        }

        if matches!(&self.api_key, ApiKeySource::Env(name) if name.trim().is_empty()) {
            return Err(ProviderConfigError::EmptyApiKeyEnv);
        }

        if matches!(self.provider, ProviderKind::Custom(_)) && self.base_url.is_none() {
            return Err(ProviderConfigError::MissingBaseUrl);
        }

        if let Some(base_url) = &self.base_url {
            if !(base_url.starts_with("https://") || base_url.starts_with("http://")) {
                return Err(ProviderConfigError::InvalidBaseUrl);
            }
        }

        Ok(())
    }
}

fn parse_api_key(value: &str) -> ApiKeySource {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.is_empty() {
        ApiKeySource::None
    } else if let Some(env_name) = trimmed.strip_prefix("env:") {
        ApiKeySource::Env(env_name.trim().to_string())
    } else {
        ApiKeySource::Inline(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_provider_config_shapes() {
        let openai = ProviderConfig::openai("gpt-4.1-mini", "env:OPENAI_API_KEY");
        assert_eq!(openai.provider, ProviderKind::OpenAi);
        assert!(openai.validate().is_ok());

        let local = ProviderConfig::custom("local", "http://localhost:11434/v1", "llama3", "none");
        assert_eq!(local.base_url.as_deref(), Some("http://localhost:11434/v1"));
        assert!(local.validate().is_ok());

        let bad = ProviderConfig {
            provider: ProviderKind::Custom("bad".to_string()),
            model: "".to_string(),
            api_key: ApiKeySource::Env("".to_string()),
            base_url: Some("ftp://example.com".to_string()),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn builds_redacted_openai_compatible_prompt_request() {
        let config =
            ProviderConfig::custom("ollama", "http://127.0.0.1:11434/v1/", "llama3.1", "none");
        let request = build_openai_compatible_request(
            &config,
            &PromptContext {
                kind: PromptKind::ReturnMarker,
                instruction: "Restore context".to_string(),
                facts_markdown: "Last clue token=secret abcdef1234567890abcdef1234567890"
                    .to_string(),
                max_input_chars: 4096,
            },
        )
        .expect("prompt request");

        assert_eq!(
            request.endpoint,
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(request.model, "llama3.1");
        assert_eq!(request.messages.len(), 2);
        let serialized = request.body.to_string();
        assert!(serialized.contains("llama3.1"));
        assert!(!serialized.contains("token=secret"));
        assert!(!serialized.contains("abcdef1234567890abcdef1234567890"));
    }

    #[test]
    fn parses_openai_compatible_response_content() {
        let response = json!({
            "choices": [
                {
                    "message": {
                        "content": "  # Daily Report\n- Closed one loop.  "
                    }
                }
            ]
        });

        assert_eq!(
            parse_openai_compatible_response(&response).as_deref(),
            Some("# Daily Report\n- Closed one loop.")
        );
    }
}
