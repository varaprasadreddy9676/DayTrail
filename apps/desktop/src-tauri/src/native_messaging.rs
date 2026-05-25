use std::io::{self, Read, Write};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::{models::BrowserBridgeEvent, store::WorktraceStore};

const MAX_NATIVE_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_TITLE_BYTES: usize = 512;
const MAX_URL_BYTES: usize = 4096;
const MAX_BATCH_EVENTS: usize = 50;
const BROWSER_TAB_MESSAGE: &str = "worktrace.browser_tab";
const BROWSER_TAB_BATCH_MESSAGE: &str = "worktrace.browser_tab_batch";
const EDITOR_CONTEXT_MESSAGE: &str = "worktrace.editor_context";
const EDITOR_CONTEXT_BATCH_MESSAGE: &str = "worktrace.editor_context_batch";

pub fn run() -> i32 {
    match WorktraceStore::open_user_default()
        .and_then(|store| run_with_store_io(&store, io::stdin().lock(), io::stdout().lock()))
    {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("worktrace native messaging host failed: {error:#}");
            1
        }
    }
}

pub fn run_with_store_io<R: Read, W: Write>(
    store: &WorktraceStore,
    mut reader: R,
    mut writer: W,
) -> Result<()> {
    while let Some(message) = read_message(&mut reader)? {
        let response = handle_message(store, message);
        write_message(&mut writer, &response)?;
        writer.flush()?;
    }
    Ok(())
}

pub fn handle_message(store: &WorktraceStore, message: Value) -> Value {
    match ingest_message(store, message) {
        Ok(response) => response,
        Err(error) => json!({
            "ok": false,
            "error": error.to_string(),
        }),
    }
}

pub fn read_message<R: Read>(reader: &mut R) -> Result<Option<Value>> {
    let mut first = [0_u8; 1];
    if reader.read(&mut first)? == 0 {
        return Ok(None);
    }

    let mut header = [0_u8; 4];
    header[0] = first[0];
    reader
        .read_exact(&mut header[1..])
        .context("native message length header was truncated")?;
    let len = u32::from_le_bytes(header) as usize;
    if len == 0 {
        bail!("native message payload is empty");
    }
    if len > MAX_NATIVE_MESSAGE_BYTES {
        bail!("native message payload exceeds 1 MiB limit");
    }

    let mut payload = vec![0_u8; len];
    reader
        .read_exact(&mut payload)
        .context("native message payload was truncated")?;
    let value = serde_json::from_slice(&payload).context("native message payload is not JSON")?;
    Ok(Some(value))
}

pub fn write_message<W: Write>(writer: &mut W, value: &Value) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    if payload.len() > MAX_NATIVE_MESSAGE_BYTES {
        bail!("native response payload exceeds 1 MiB limit");
    }
    let len = u32::try_from(payload.len())
        .map_err(|_| anyhow!("native response payload is too large"))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&payload)?;
    Ok(())
}

fn ingest_message(store: &WorktraceStore, message: Value) -> Result<Value> {
    let message_type = message
        .get("type")
        .and_then(Value::as_str)
        .context("native message type is required")?;
    let schema_version = message
        .get("schemaVersion")
        .and_then(Value::as_i64)
        .context("native message schemaVersion is required")?;
    anyhow::ensure!(
        schema_version == 1,
        "unsupported native message schemaVersion: {schema_version}"
    );

    match message_type {
        BROWSER_TAB_MESSAGE => ingest_browser_tab_message(store, message),
        BROWSER_TAB_BATCH_MESSAGE => ingest_browser_tab_batch(store, message),
        EDITOR_CONTEXT_MESSAGE => ingest_editor_context_message(store, message),
        EDITOR_CONTEXT_BATCH_MESSAGE => ingest_editor_context_batch(store, message),
        _ => bail!("unsupported native message type: {message_type}"),
    }
}

fn ingest_browser_tab_batch(store: &WorktraceStore, message: Value) -> Result<Value> {
    let events = message
        .get("events")
        .and_then(Value::as_array)
        .context("native browser batch events are required")?;
    anyhow::ensure!(
        events.len() <= MAX_BATCH_EVENTS,
        "native browser batch exceeds {MAX_BATCH_EVENTS} events"
    );

    let mut stored = 0;
    let mut ignored = 0;
    for event in events {
        let mut event = event.clone();
        if let Some(object) = event.as_object_mut() {
            object.insert(
                "type".to_string(),
                Value::String(BROWSER_TAB_MESSAGE.to_string()),
            );
            object.insert("schemaVersion".to_string(), Value::from(1));
        }
        let response = ingest_browser_tab_message(store, event)?;
        if response
            .get("stored")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            stored += 1;
        } else {
            ignored += 1;
        }
    }

    Ok(json!({
        "ok": true,
        "stored": stored,
        "ignored": ignored,
        "type": "worktrace.browser_tab_batch.accepted",
    }))
}

fn ingest_editor_context_batch(store: &WorktraceStore, message: Value) -> Result<Value> {
    let events = message
        .get("events")
        .and_then(Value::as_array)
        .context("native editor batch events are required")?;
    anyhow::ensure!(
        events.len() <= MAX_BATCH_EVENTS,
        "native editor batch exceeds {MAX_BATCH_EVENTS} events"
    );

    let mut stored = 0;
    let mut ignored = 0;
    for event in events {
        let mut event = event.clone();
        if let Some(object) = event.as_object_mut() {
            object.insert(
                "type".to_string(),
                Value::String(EDITOR_CONTEXT_MESSAGE.to_string()),
            );
            object.insert("schemaVersion".to_string(), Value::from(1));
        }
        let response = ingest_editor_context_message(store, event)?;
        if response
            .get("stored")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            stored += 1;
        } else {
            ignored += 1;
        }
    }

    Ok(json!({
        "ok": true,
        "stored": stored,
        "ignored": ignored,
        "type": "worktrace.editor_context_batch.accepted",
    }))
}

fn ingest_editor_context_message(store: &WorktraceStore, message: Value) -> Result<Value> {
    let app = message
        .get("app")
        .and_then(Value::as_str)
        .unwrap_or("Visual Studio Code");
    anyhow::ensure!(
        app.len() <= MAX_TITLE_BYTES,
        "editor event app exceeds {MAX_TITLE_BYTES} bytes"
    );

    let document = message
        .get("document")
        .and_then(Value::as_object)
        .context("editor event document is required")?;
    let has_context = document
        .get("fileName")
        .or_else(|| document.get("filePath"))
        .or_else(|| document.get("uri"))
        .and_then(Value::as_str)
        .is_some()
        || message
            .get("workspace")
            .and_then(|workspace| workspace.get("name"))
            .and_then(Value::as_str)
            .is_some();
    anyhow::ensure!(
        has_context,
        "editor event requires document or workspace context"
    );

    let stored = store.ingest_editor_context_event(message)?;
    Ok(json!({
        "ok": true,
        "stored": stored,
        "type": "worktrace.editor_context.accepted",
    }))
}

fn ingest_browser_tab_message(store: &WorktraceStore, message: Value) -> Result<Value> {
    if let Some(title) = message.get("title").and_then(Value::as_str) {
        anyhow::ensure!(
            title.len() <= MAX_TITLE_BYTES,
            "browser event title exceeds {MAX_TITLE_BYTES} bytes"
        );
    }
    if let Some(url) = message.get("url").and_then(Value::as_str) {
        anyhow::ensure!(
            url.len() <= MAX_URL_BYTES,
            "browser event url exceeds {MAX_URL_BYTES} bytes"
        );
        if !is_allowed_browser_url(url) {
            return Ok(json!({
                "ok": true,
                "stored": false,
                "ignoredReason": "unsupported_url_scheme",
            }));
        }
    }

    let event: BrowserBridgeEvent =
        serde_json::from_value(message).context("invalid browser event payload")?;
    anyhow::ensure!(
        event.url.is_some() || event.title.is_some(),
        "browser event requires url or title"
    );

    let stored = store.ingest_browser_event(event)?;
    Ok(json!({
        "ok": true,
        "stored": stored,
        "type": "worktrace.browser_tab.accepted",
    }))
}

fn is_allowed_browser_url(value: &str) -> bool {
    url::Url::parse(value)
        .map(|parsed| matches!(parsed.scheme(), "http" | "https"))
        .unwrap_or(false)
}
