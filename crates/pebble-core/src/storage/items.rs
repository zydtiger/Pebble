use super::{Database, codec};
use crate::{Error, ModelItem, ModelRequest};
use rusqlite::params;

impl Database {
    pub(crate) fn request(&self, call_id: &str) -> Result<ModelRequest, Error> {
        let connection = self.connection.lock().expect("database lock");
        request(&connection, &self.root, call_id, false)
    }
}

pub(super) fn request(
    connection: &rusqlite::Connection,
    root: &std::path::Path,
    call_id: &str,
    full: bool,
) -> Result<ModelRequest, Error> {
    let (mut request, last): (ModelRequest, i64) = connection.query_row(
        "SELECT c.session_id, c.conversation_id, c.trigger_message_id, c.event_cutoff,
                    s.protocol, s.model, c.input_last_sequence, s.response_schema, c.state_before
             FROM model_calls c JOIN model_sessions s ON s.id = c.session_id WHERE c.id = ?1",
        [call_id],
        |row| {
            Ok((
                ModelRequest {
                    image_root: root.to_path_buf(),
                    call_id: call_id.into(),
                    session_id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    trigger_message_id: row.get(2)?,
                    event_cutoff: row.get(3)?,
                    protocol: row.get(4)?,
                    model: row.get(5)?,
                    items: Vec::new(),
                    recent_history: None,
                    response_schema: None,
                    state_before: None,
                    history_tools: false,
                    history_cutoff: 0,
                    memory_tools: false,
                    memory_capable: false,
                    delegation_tools: false,
                    image_generation_tools: false,
                    image_generation_backends: Vec::new(),
                },
                row.get(6)?,
            ))
        },
    )?;
    let (schema, state): (Option<String>, Option<String>) = connection.query_row(
            "SELECT s.response_schema,c.state_before FROM model_calls c JOIN model_sessions s ON s.id=c.session_id WHERE c.id=?1", [call_id], |r| Ok((r.get(0)?,r.get(1)?)))?;
    request.memory_tools = connection.query_row(
        "SELECT memory_tools FROM model_sessions WHERE id=?1",
        [&request.session_id],
        |r| r.get(0),
    )?;
    request.memory_capable = request.memory_tools;
    request.history_tools = connection.query_row(
        "SELECT history_tools FROM model_sessions WHERE id=?1",
        [&request.session_id],
        |r| r.get(0),
    )?;
    request.history_cutoff = connection.query_row(
        "SELECT COALESCE(history_cutoff,event_cutoff) FROM model_calls WHERE id=?1",
        [call_id],
        |r| r.get(0),
    )?;
    let last = if full { i64::MAX } else { last };
    request.delegation_tools = connection.query_row(
        "SELECT delegation_tools FROM model_sessions WHERE id=?1",
        [&request.session_id],
        |r| r.get(0),
    )?;
    request.image_generation_tools = connection.query_row(
        "SELECT image_generation_tools FROM model_sessions WHERE id=?1",
        [&request.session_id],
        |r| r.get(0),
    )?;
    if super::image_generation::report_input(connection, call_id)?.is_some() {
        request.history_tools = false;
        request.image_generation_tools = false;
        request.delegation_tools = false;
        request.memory_tools = false;
        request.image_generation_tools = false;
    }
    if super::delegation::report_input(connection, call_id)?.is_some() {
        request.history_tools = false;
        request.delegation_tools = false;
        request.memory_tools = false;
        request.image_generation_tools = false;
    }
    request.response_schema = schema.map(|s| serde_json::from_str(&s)).transpose()?;
    request.state_before = state.map(|s| serde_json::from_str(&s)).transpose()?;
    // First scan metadata only: never materialize an oversized saved history.
    let mut query = connection.prepare("SELECT codec, decoded_length, length(payload) FROM model_items WHERE session_id=?1 AND sequence<=?2")?;
    let mut rows = query.query(params![request.session_id, last])?;
    let mut total = 0usize;
    while let Some(row) = rows.next()? {
        let length = codec::metadata(&row.get::<_, String>(0)?, row.get(1)?, row.get(2)?)?;
        total = total
            .checked_add(length)
            .and_then(|n| n.checked_add(codec::ITEM_OVERHEAD))
            .filter(|n| *n <= codec::DECODED_CAP)
            .ok_or_else(codec::cap_error)?;
    }
    let mut query = connection.prepare("SELECT id, sequence, codec, payload, decoded_length FROM model_items WHERE session_id=?1 AND sequence<=?2 ORDER BY sequence")?;
    let mut rows = query.query(params![request.session_id, last])?;
    let mut remaining = codec::DECODED_CAP;
    while let Some(row) = rows.next()? {
        remaining -= codec::ITEM_OVERHEAD;
        let payload = codec::decode(
            &row.get::<_, String>(2)?,
            row.get(3)?,
            row.get::<_, i64>(4)? as usize,
            remaining,
        )?;
        remaining -= payload.len();
        request.items.push(ModelItem {
            id: row.get(0)?,
            sequence: row.get(1)?,
            payload,
        });
    }
    request.recent_history =
        super::recent::hydrate(connection, &request.session_id, &request.protocol)?;
    Ok(request)
}
