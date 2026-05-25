use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpStream},
    process::Command,
    time::Duration,
};

const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderRoute {
    OpenAiCompatible,
    Gemini,
    Anthropic,
}

pub fn generate_text(
    provider: &str,
    endpoint: &str,
    model: &str,
    api_key: Option<&str>,
    instruction: &str,
    context: &str,
) -> Result<String> {
    let route = provider_route(provider, endpoint);
    let endpoint = routed_endpoint(route, endpoint, model);
    let body = request_body(route, model, instruction, context);
    let headers = request_headers(route, api_key);
    let response = post_json_with_headers(&endpoint, &headers, &body.to_string())?;
    extract_text(route, &response)
}

pub fn post_json(endpoint: &str, api_key: Option<&str>, body: &str) -> Result<String> {
    post_json_with_headers(
        endpoint,
        &[(
            "Authorization".to_string(),
            format!("Bearer {}", api_key.unwrap_or_default().trim()),
        )],
        body,
    )
}

pub fn post_json_with_headers(
    endpoint: &str,
    headers: &[(String, String)],
    body: &str,
) -> Result<String> {
    let parsed = url::Url::parse(endpoint).context("invalid AI endpoint URL")?;
    match parsed.scheme() {
        "http" => post_json_http(&parsed, headers, body),
        "https" => post_json_https_with_curl(endpoint, headers, body),
        other => bail!("unsupported AI endpoint scheme: {other}"),
    }
}

fn post_json_http(parsed: &url::Url, headers: &[(String, String)], body: &str) -> Result<String> {
    let host = parsed.host_str().context("AI endpoint host is required")?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let address = format!("{host}:{port}");
    let path = if let Some(query) = parsed.query() {
        format!("{}?{query}", parsed.path())
    } else if parsed.path().is_empty() {
        "/".to_string()
    } else {
        parsed.path().to_string()
    };

    let mut stream = TcpStream::connect(address).context("failed to connect to AI endpoint")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .context("failed to set AI read timeout")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .context("failed to set AI write timeout")?;

    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        if value.trim().is_empty() || value.trim() == "Bearer" {
            continue;
        }
        request.push_str(&format!("{name}: {}\r\n", value.trim()));
    }
    request.push_str("\r\n");
    request.push_str(body);

    stream
        .write_all(request.as_bytes())
        .context("failed to send AI request")?;
    stream
        .shutdown(Shutdown::Write)
        .context("failed to finish AI request body")?;
    let mut response = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES)
        .read_to_end(&mut response)
        .context("failed to read AI response")?;

    let response = String::from_utf8(response).context("AI response was not UTF-8")?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow!("AI response was malformed"))?;
    let status_code = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("AI response status was malformed"))?;

    if !(200..300).contains(&status_code) {
        bail!("AI endpoint returned HTTP {status_code}");
    }

    Ok(body.to_string())
}

fn post_json_https_with_curl(
    endpoint: &str,
    headers: &[(String, String)],
    body: &str,
) -> Result<String> {
    let temp_dir = std::env::temp_dir();
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );
    let body_path = temp_dir.join(format!("worktrace-ai-body-{nonce}.json"));
    let config_path = temp_dir.join(format!("worktrace-ai-curl-{nonce}.conf"));

    fs::write(&body_path, body).context("failed to write AI request body")?;

    let mut config = String::new();
    config.push_str(&format!("url = \"{}\"\n", escape_curl_config(endpoint)));
    config.push_str("request = \"POST\"\n");
    config.push_str("silent\nshow-error\nfail-with-body\n");
    config.push_str("max-time = \"45\"\n");
    config.push_str("header = \"Content-Type: application/json\"\n");
    config.push_str("header = \"Accept: application/json\"\n");
    for (name, value) in headers {
        if value.trim().is_empty() || value.trim() == "Bearer" {
            continue;
        }
        config.push_str(&format!(
            "header = \"{}: {}\"\n",
            escape_curl_config(name),
            escape_curl_config(value.trim())
        ));
    }
    config.push_str(&format!(
        "data-binary = \"@{}\"\n",
        escape_curl_config(&body_path.display().to_string())
    ));
    fs::write(&config_path, config).context("failed to write AI curl config")?;

    let output = Command::new("curl")
        .arg("--config")
        .arg(&config_path)
        .output()
        .context("failed to invoke curl for HTTPS AI endpoint")?;

    let _ = fs::remove_file(&body_path);
    let _ = fs::remove_file(&config_path);

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!(
            "AI endpoint request failed{}",
            if error.is_empty() {
                String::new()
            } else {
                format!(": {error}")
            }
        );
    }

    String::from_utf8(output.stdout).context("AI response was not UTF-8")
}

fn escape_curl_config(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn provider_route(provider: &str, endpoint: &str) -> ProviderRoute {
    let haystack = format!("{provider} {endpoint}").to_ascii_lowercase();
    if haystack.contains("gemini") || haystack.contains("generativelanguage.googleapis.com") {
        ProviderRoute::Gemini
    } else if haystack.contains("anthropic") || haystack.contains("claude") {
        ProviderRoute::Anthropic
    } else {
        ProviderRoute::OpenAiCompatible
    }
}

fn routed_endpoint(route: ProviderRoute, endpoint: &str, model: &str) -> String {
    if route != ProviderRoute::Gemini {
        return endpoint.to_string();
    }

    let model = model.trim();
    if model.is_empty() {
        return endpoint.to_string();
    }

    if let Some((prefix, suffix)) = endpoint.split_once("/models/") {
        let suffix = suffix
            .split_once(':')
            .map(|(_, rest)| format!(":{rest}"))
            .unwrap_or_else(|| ":generateContent".to_string());
        return format!("{prefix}/models/{model}{suffix}");
    }

    let base = endpoint.trim_end_matches('/');
    if base.ends_with("/v1beta") || base.ends_with("/v1") {
        return format!("{base}/models/{model}:generateContent");
    }

    endpoint.to_string()
}

fn request_headers(route: ProviderRoute, api_key: Option<&str>) -> Vec<(String, String)> {
    let key = api_key.unwrap_or_default().trim();
    match route {
        ProviderRoute::Gemini => vec![("X-goog-api-key".into(), key.into())],
        ProviderRoute::Anthropic => vec![
            ("x-api-key".into(), key.into()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ],
        ProviderRoute::OpenAiCompatible => {
            vec![("Authorization".into(), format!("Bearer {key}"))]
        }
    }
}

fn request_body(route: ProviderRoute, model: &str, instruction: &str, context: &str) -> Value {
    let prompt = format!("{instruction}\n\nContext:\n{context}");
    match route {
        ProviderRoute::Gemini => json!({
            "contents": [
                {
                    "parts": [
                        { "text": prompt }
                    ]
                }
            ]
        }),
        ProviderRoute::Anthropic => json!({
            "model": model,
            "max_tokens": 1600,
            "messages": [
                { "role": "user", "content": prompt }
            ]
        }),
        ProviderRoute::OpenAiCompatible => json!({
            "model": model,
            "messages": [
                { "role": "system", "content": instruction },
                { "role": "user", "content": context }
            ],
            "temperature": 0.2
        }),
    }
}

fn extract_text(route: ProviderRoute, body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).context("AI response body was not JSON")?;
    let text = match route {
        ProviderRoute::Gemini => value
            .pointer("/candidates/0/content/parts/0/text")
            .and_then(Value::as_str),
        ProviderRoute::Anthropic => value.pointer("/content/0/text").and_then(Value::as_str),
        ProviderRoute::OpenAiCompatible => value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str),
    };

    text.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("AI response did not contain text output"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn posts_json_to_local_openai_compatible_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let address = listener.local_addr().expect("local addr");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0_u8; 4096];
            let size = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..size]);
            assert!(request.contains("POST /v1/chat/completions HTTP/1.1"));
            assert!(request.contains("Authorization: Bearer test-key"));
            assert!(request.contains("\"model\":\"llama3\""));
            let body = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });

        let body = post_json(
            &format!("http://{address}/v1/chat/completions"),
            Some("test-key"),
            r#"{"model":"llama3"}"#,
        )
        .expect("post json");
        server.join().expect("server join");
        assert!(body.contains("\"content\":\"ok\""));
    }

    #[test]
    fn routes_gemini_openai_compatible_and_anthropic_payloads() {
        assert_eq!(
            provider_route("Gemini", "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent"),
            ProviderRoute::Gemini
        );
        assert_eq!(
            provider_route(
                "OpenRouter",
                "https://openrouter.ai/api/v1/chat/completions"
            ),
            ProviderRoute::OpenAiCompatible
        );
        assert_eq!(
            provider_route("Groq", "https://api.groq.com/openai/v1/chat/completions"),
            ProviderRoute::OpenAiCompatible
        );
        assert_eq!(
            provider_route("Anthropic", "https://api.anthropic.com/v1/messages"),
            ProviderRoute::Anthropic
        );
        assert_eq!(
            routed_endpoint(
                ProviderRoute::Gemini,
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent",
                "gemini-2.5-flash"
            ),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        );

        let gemini = request_body(ProviderRoute::Gemini, "ignored", "Do it", "Facts");
        assert_eq!(
            gemini["contents"][0]["parts"][0]["text"],
            "Do it\n\nContext:\nFacts"
        );

        let openai = request_body(
            ProviderRoute::OpenAiCompatible,
            "gpt-4.1-mini",
            "Do it",
            "Facts",
        );
        assert_eq!(openai["model"], "gpt-4.1-mini");
        assert_eq!(openai["messages"][0]["role"], "system");

        let anthropic = request_body(
            ProviderRoute::Anthropic,
            "claude-3-5-sonnet",
            "Do it",
            "Facts",
        );
        assert_eq!(anthropic["model"], "claude-3-5-sonnet");
        assert_eq!(anthropic["messages"][0]["role"], "user");
    }

    #[test]
    fn extracts_text_from_provider_responses() {
        assert_eq!(
            extract_text(
                ProviderRoute::OpenAiCompatible,
                r#"{"choices":[{"message":{"content":"OpenAI text"}}]}"#
            )
            .expect("openai text"),
            "OpenAI text"
        );
        assert_eq!(
            extract_text(
                ProviderRoute::Gemini,
                r#"{"candidates":[{"content":{"parts":[{"text":"Gemini text"}]}}]}"#
            )
            .expect("gemini text"),
            "Gemini text"
        );
        assert_eq!(
            extract_text(
                ProviderRoute::Anthropic,
                r#"{"content":[{"type":"text","text":"Claude text"}]}"#
            )
            .expect("anthropic text"),
            "Claude text"
        );
    }
}
