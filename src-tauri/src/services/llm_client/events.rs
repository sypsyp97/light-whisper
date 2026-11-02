use serde_json::Value;
use tauri::{Emitter, Manager};

use crate::state::AppState;

/// Report recoverable feature loss without replacing the operation's result.
pub(crate) fn emit_api_warning(
    app_handle: &tauri::AppHandle,
    message: &str,
    session_id: Option<u64>,
) {
    let mut payload = serde_json::json!({ "message": message });
    if let Some(session_id) = session_id {
        if let Some(state) = app_handle.try_state::<AppState>() {
            state.ui.mark_api_warning(session_id);
        }
        payload["sessionId"] = serde_json::json!(session_id);
    }
    let _ = app_handle.emit("api-warning", payload);
}

pub(crate) fn build_stream_event_payload(
    session_id: Option<u64>,
    chunk: Option<&str>,
    tokens: usize,
) -> Value {
    let mut payload = serde_json::json!({ "status": "streaming" });
    if let Some(chunk) = chunk {
        payload["chunk"] = serde_json::json!(chunk);
    }
    if tokens > 0 {
        payload["tokens"] = serde_json::json!(tokens);
    }
    if let Some(session_id) = session_id {
        payload["sessionId"] = serde_json::json!(session_id);
    }
    payload
}

pub(crate) fn emit_stream_event(
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
    chunk: Option<&str>,
    tokens: usize,
) {
    if let (Some(app_handle), Some(event_name)) = (app_handle, event_name) {
        if event_name == "ai-polish-status" && (chunk.is_some() || tokens > 0) {
            if let Some(session_id) = session_id {
                app_handle
                    .state::<AppState>()
                    .mark_ai_polish_stream_started(session_id);
            }
        }
        let payload = build_stream_event_payload(session_id, chunk, tokens);
        let _ = app_handle.emit(event_name, payload);
    }
}

pub(crate) fn build_stream_error_payload(session_id: Option<u64>, message: &str) -> Value {
    let mut payload = serde_json::json!({
        "status": "error",
        "message": message,
    });
    if let Some(session_id) = session_id {
        payload["sessionId"] = serde_json::json!(session_id);
    }
    payload
}

pub(crate) fn emit_stream_error_event(
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
    message: &str,
) {
    if let (Some(app_handle), Some(event_name)) = (app_handle, event_name) {
        if event_name == "ai-polish-status" {
            if let (Some(state), Some(session_id)) =
                (app_handle.try_state::<AppState>(), session_id)
            {
                state.ui.mark_api_warning(session_id);
            }
        }
        let payload = build_stream_error_payload(session_id, message);
        let _ = app_handle.emit(event_name, payload);
    }
}

fn url_citation_payload(value: &Value) -> Option<Value> {
    if value["type"].as_str() != Some("url_citation") {
        return None;
    }
    let url = value["url"].as_str()?.trim();
    if url.is_empty() {
        return None;
    }
    let title = value["title"].as_str().unwrap_or(url).trim();
    Some(serde_json::json!({
        "title": if title.is_empty() { url } else { title },
        "url": url,
    }))
}

pub(crate) fn collect_url_citation_payloads(value: &Value, citations: &mut Vec<Value>) {
    if let Some(citation) = url_citation_payload(value) {
        if !citations
            .iter()
            .any(|existing| existing["url"] == citation["url"])
        {
            citations.push(citation);
        }
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect_url_citation_payloads(item, citations);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_url_citation_payloads(item, citations);
            }
        }
        _ => {}
    }
}

pub(crate) fn emit_stream_citations(
    app_handle: Option<&tauri::AppHandle>,
    event_name: Option<&str>,
    session_id: Option<u64>,
    value: &Value,
) {
    let (Some(app_handle), Some(event_name)) = (app_handle, event_name) else {
        return;
    };
    let mut citations = Vec::new();
    collect_url_citation_payloads(value, &mut citations);
    for source in citations {
        let mut payload = serde_json::json!({
            "status": "citation",
            "source": source,
        });
        if let Some(session_id) = session_id {
            payload["sessionId"] = serde_json::json!(session_id);
        }
        let _ = app_handle.emit(event_name, payload);
    }
}
