#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_schema_and_ordered_migrations() {
        assert!(SCHEMA_SQL.contains("CREATE TABLE IF NOT EXISTS events"));
        assert!(SCHEMA_SQL.contains("CREATE TABLE IF NOT EXISTS inbox_messages"));

        let migrations = migrations();
        assert_eq!(migrations[0].id, "0001_initial");
        assert!(migrations.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert!(migrations
            .iter()
            .all(|migration| migration.sql.trim_end().ends_with(';')));
    }
}
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS events (
  id TEXT PRIMARY KEY,
  app TEXT NOT NULL,
  title TEXT NOT NULL,
  url TEXT,
  kind TEXT NOT NULL,
  started_at_ms INTEGER NOT NULL,
  ended_at_ms INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  started_at_ms INTEGER NOT NULL,
  ended_at_ms INTEGER NOT NULL,
  active_ms INTEGER NOT NULL,
  primary_app TEXT
);

CREATE TABLE IF NOT EXISTS tasks (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  canonical_title TEXT NOT NULL UNIQUE,
  first_seen_ms INTEGER NOT NULL,
  last_seen_ms INTEGER NOT NULL,
  status TEXT NOT NULL DEFAULT 'open'
);

CREATE TABLE IF NOT EXISTS inbox_messages (
  id TEXT PRIMARY KEY,
  sender TEXT NOT NULL,
  recipient TEXT NOT NULL,
  subject TEXT NOT NULL,
  sent_at_ms INTEGER NOT NULL
);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub id: &'static str,
    pub sql: &'static str,
}

pub fn migrations() -> Vec<Migration> {
    vec![
        Migration {
            id: "0001_initial",
            sql: SCHEMA_SQL,
        },
        Migration {
            id: "0002_event_indexes",
            sql: "CREATE INDEX IF NOT EXISTS idx_events_time ON events(started_at_ms, ended_at_ms);",
        },
        Migration {
            id: "0003_inbox_indexes",
            sql: "CREATE INDEX IF NOT EXISTS idx_inbox_subject_time ON inbox_messages(subject, sent_at_ms);",
        },
    ]
}
