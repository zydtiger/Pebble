use super::{Database, codec};
use crate::{Error, ModelItem, ModelRequest};
use rusqlite::params;

impl Database {
    pub(crate) fn request(&self, call_id: &str) -> Result<ModelRequest, Error> {
        let connection = self.connection.lock().expect("database lock");
        let (mut request, last): (ModelRequest, i64) = connection.query_row(
            "SELECT c.session_id, c.conversation_id, c.trigger_message_id, c.event_cutoff,
                    s.protocol, s.model, c.input_last_sequence, s.response_schema, c.state_before
             FROM model_calls c JOIN model_sessions s ON s.id = c.session_id WHERE c.id = ?1",
            [call_id],
            |row| {
                Ok((
                    ModelRequest {
                        call_id: call_id.into(),
                        session_id: row.get(0)?,
                        conversation_id: row.get(1)?,
                        trigger_message_id: row.get(2)?,
                        event_cutoff: row.get(3)?,
                        protocol: row.get(4)?,
                        model: row.get(5)?,
                        items: Vec::new(),
                        response_schema: None,
                        state_before: None,
                    },
                    row.get(6)?,
                ))
            },
        )?;
        let (schema, state): (Option<String>, Option<String>) = connection.query_row(
            "SELECT s.response_schema,c.state_before FROM model_calls c JOIN model_sessions s ON s.id=c.session_id WHERE c.id=?1", [call_id], |r| Ok((r.get(0)?,r.get(1)?)))?;
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
        Ok(request)
    }
}
