#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub sent_at_ms: u64,
}

impl Message {
    pub fn new(id: &str, from: &str, to: &str, subject: &str, sent_at_ms: u64) -> Self {
        Self {
            id: id.to_string(),
            from: normalize_address(from),
            to: normalize_address(to),
            subject: subject.trim().to_string(),
            sent_at_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyMatch {
    pub original_id: String,
    pub reply_id: String,
}

pub fn is_reply_to(reply: &Message, original: &Message) -> bool {
    reply.sent_at_ms > original.sent_at_ms
        && normalize_subject(&reply.subject) == normalize_subject(&original.subject)
        && reply.from == original.to
        && reply.to == original.from
}

pub fn detect_replies(messages: &[Message]) -> Vec<ReplyMatch> {
    let mut matches = Vec::new();

    for original in messages {
        for reply in messages {
            if is_reply_to(reply, original) {
                matches.push(ReplyMatch {
                    original_id: original.id.clone(),
                    reply_id: reply.id.clone(),
                });
                break;
            }
        }
    }

    matches.sort_by(|left, right| {
        left.original_id
            .cmp(&right.original_id)
            .then_with(|| left.reply_id.cmp(&right.reply_id))
    });
    matches
}

pub fn normalize_subject(subject: &str) -> String {
    let mut value = subject.trim();

    loop {
        let lower = value.to_ascii_lowercase();
        let Some(prefix) = ["re:", "fw:", "fwd:"]
            .iter()
            .find(|prefix| lower.starts_with(*prefix))
        else {
            break;
        };
        value = value[prefix.len()..].trim();
    }

    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_address(address: &str) -> String {
    address.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_replies_by_subject_participants_and_time() {
        let original = Message::new(
            "m1",
            "alice@example.com",
            "me@example.com",
            "Project status",
            1_000,
        );
        let reply = Message::new(
            "m2",
            "me@example.com",
            "alice@example.com",
            "Re: project status",
            2_000,
        );
        let unrelated = Message::new(
            "m3",
            "bob@example.com",
            "me@example.com",
            "Re: project status",
            2_500,
        );

        assert!(is_reply_to(&reply, &original));
        assert!(!is_reply_to(&unrelated, &original));

        let detected = detect_replies(&[original, reply, unrelated]);
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].original_id, "m1");
        assert_eq!(detected[0].reply_id, "m2");
    }
}
