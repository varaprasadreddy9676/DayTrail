#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionPolicy {
    pub redact_emails: bool,
    pub redact_phone_numbers: bool,
    pub redact_tokens: bool,
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self {
            redact_emails: true,
            redact_phone_numbers: true,
            redact_tokens: true,
        }
    }
}

impl RedactionPolicy {
    pub fn redact_text(&self, input: &str) -> String {
        let mut output = input.to_string();
        if self.redact_tokens {
            output = redact_tokens(&output);
        }
        if self.redact_emails {
            output = redact_emails(&output);
        }
        if self.redact_phone_numbers {
            output = redact_phone_numbers(&output);
        }
        output
    }
}

fn redact_tokens(input: &str) -> String {
    replace_spans(input, token_spans(input), "[redacted-token]")
}

fn token_spans(input: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i + 3 <= bytes.len() {
        if bytes[i..].starts_with(b"sk-") || bytes[i..].starts_with(b"secret-token-") {
            let start = i;
            i += if bytes[start..].starts_with(b"secret-token-") {
                "secret-token-".len()
            } else {
                "sk-".len()
            };
            while i < bytes.len() && is_token_char(bytes[i]) {
                i += 1;
            }
            if i - start >= 12 {
                spans.push((start, i));
            }
        } else {
            i += 1;
        }
    }

    spans
}

fn redact_emails(input: &str) -> String {
    replace_spans(input, email_spans(input), "[redacted-email]")
}

fn email_spans(input: &str) -> Vec<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut spans = Vec::new();

    for at in 0..bytes.len() {
        if bytes[at] != b'@' {
            continue;
        }

        let mut start = at;
        while start > 0 && is_email_local(bytes[start - 1]) {
            start -= 1;
        }

        let mut end = at + 1;
        while end < bytes.len() && is_email_domain(bytes[end]) {
            end += 1;
        }

        let local = &input[start..at];
        let domain = &input[at + 1..end];
        if !local.is_empty()
            && domain.contains('.')
            && domain
                .rsplit('.')
                .next()
                .is_some_and(|suffix| suffix.len() >= 2)
        {
            spans.push((start, end));
        }
    }

    coalesce_spans(spans)
}

fn redact_phone_numbers(input: &str) -> String {
    replace_spans(input, phone_spans(input), "[redacted-phone]")
}

fn phone_spans(input: &str) -> Vec<(usize, usize)> {
    let bytes = input.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if !is_phone_start(bytes[i]) {
            i += 1;
            continue;
        }

        let start = i;
        let mut end = i;
        let mut digit_count = 0;
        let mut separator_count = 0;

        while end < bytes.len() && is_phone_body(bytes[end]) {
            if bytes[end].is_ascii_digit() {
                digit_count += 1;
            } else if !bytes[end].is_ascii_whitespace() {
                separator_count += 1;
            }
            end += 1;
        }

        while end > start && input.as_bytes()[end - 1].is_ascii_whitespace() {
            end -= 1;
        }

        if (10..=16).contains(&digit_count)
            && (separator_count > 0 || bytes[start] == b'+')
            && !is_embedded_in_word(input, start, end)
        {
            spans.push((start, end));
            i = end;
        } else {
            i = start + 1;
        }
    }

    coalesce_spans(spans)
}

fn replace_spans(input: &str, spans: Vec<(usize, usize)>, replacement: &str) -> String {
    if spans.is_empty() {
        return input.to_string();
    }

    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for (start, end) in spans {
        if start < cursor {
            continue;
        }
        output.push_str(&input[cursor..start]);
        output.push_str(replacement);
        cursor = end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn coalesce_spans(mut spans: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    spans.sort_unstable();
    let mut result: Vec<(usize, usize)> = Vec::new();
    for span in spans {
        if let Some(last) = result.last_mut() {
            if span.0 <= last.1 {
                last.1 = last.1.max(span.1);
                continue;
            }
        }
        result.push(span);
    }
    result
}

fn is_email_local(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
}

fn is_email_domain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-')
}

fn is_token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn is_phone_start(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'+' | b'(')
}

fn is_phone_body(byte: u8) -> bool {
    byte.is_ascii_digit()
        || byte.is_ascii_whitespace()
        || matches!(byte, b'+' | b'-' | b'.' | b'(' | b')')
}

fn is_embedded_in_word(input: &str, start: usize, end: usize) -> bool {
    input
        .as_bytes()
        .get(start.wrapping_sub(1))
        .is_some_and(u8::is_ascii_alphabetic)
        || input
            .as_bytes()
            .get(end)
            .is_some_and(u8::is_ascii_alphabetic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_common_sensitive_tokens() {
        let input =
            "Email alice@example.com or call +1 (415) 555-0134; key secret-token-1234567890abcdef.";

        let redacted = RedactionPolicy::default().redact_text(input);

        assert!(!redacted.contains("alice@example.com"));
        assert!(!redacted.contains("+1 (415) 555-0134"));
        assert!(!redacted.contains("secret-token-1234567890abcdef"));
        assert!(redacted.contains("[redacted-email]"));
        assert!(redacted.contains("[redacted-phone]"));
        assert!(redacted.contains("[redacted-token]"));
    }

    #[test]
    fn keeps_short_operational_numbers() {
        let input = "Review sprint 42 ticket WT-100 before 5pm.";

        let redacted = RedactionPolicy::default().redact_text(input);

        assert_eq!(redacted, input);
    }
}
