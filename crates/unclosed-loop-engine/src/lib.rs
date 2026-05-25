use worktrace_inbox_engine::{is_reply_to, Message};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopOptions {
    pub self_addresses: Vec<String>,
    pub now_ms: u64,
    pub stale_after_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnclosedLoop {
    pub message_id: String,
    pub from: String,
    pub subject: String,
    pub sent_at_ms: u64,
    pub age_ms: u64,
}

pub fn detect_unclosed_loops(messages: &[Message], options: LoopOptions) -> Vec<UnclosedLoop> {
    let self_addresses: Vec<String> = options
        .self_addresses
        .iter()
        .map(|address| address.trim().to_ascii_lowercase())
        .collect();

    let mut loops = Vec::new();

    for message in messages {
        if !is_inbound(message, &self_addresses) {
            continue;
        }

        let age_ms = options.now_ms.saturating_sub(message.sent_at_ms);
        if age_ms < options.stale_after_ms {
            continue;
        }

        let has_outbound_reply = messages.iter().any(|candidate| {
            is_outbound(candidate, &self_addresses) && is_reply_to(candidate, message)
        });

        if !has_outbound_reply {
            loops.push(UnclosedLoop {
                message_id: message.id.clone(),
                from: message.from.clone(),
                subject: message.subject.clone(),
                sent_at_ms: message.sent_at_ms,
                age_ms,
            });
        }
    }

    loops.sort_by_key(|item| item.sent_at_ms);
    loops
}

fn is_inbound(message: &Message, self_addresses: &[String]) -> bool {
    is_self(&message.to, self_addresses) && !is_self(&message.from, self_addresses)
}

fn is_outbound(message: &Message, self_addresses: &[String]) -> bool {
    is_self(&message.from, self_addresses)
}

fn is_self(address: &str, self_addresses: &[String]) -> bool {
    self_addresses
        .iter()
        .any(|self_address| self_address == &address.trim().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktrace_inbox_engine::Message;

    #[test]
    fn finds_stale_inbound_messages_without_outbound_replies() {
        let messages = vec![
            Message::new(
                "in-1",
                "lead@example.com",
                "me@example.com",
                "Can you send the deck?",
                0,
            ),
            Message::new(
                "in-2",
                "ops@example.com",
                "me@example.com",
                "Access request",
                100,
            ),
            Message::new(
                "out-2",
                "me@example.com",
                "ops@example.com",
                "Re: access request",
                200,
            ),
        ];

        let loops = detect_unclosed_loops(
            &messages,
            LoopOptions {
                self_addresses: vec!["me@example.com".to_string()],
                now_ms: 86_400_000 * 3,
                stale_after_ms: 86_400_000,
            },
        );

        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].message_id, "in-1");
        assert_eq!(loops[0].age_ms, 86_400_000 * 3);
    }
}
