use crate::Error;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

/// Freeze contact time separately from queue delay and background work.
pub(crate) fn context(
    db: &Connection,
    persona: &str,
    current: Option<&str>,
) -> Result<Value, Error> {
    let last_user:Option<String>=db.query_row("SELECT MAX(m.created_at) FROM persona_events e JOIN messages m ON m.id=e.message_id WHERE e.persona_id=?1 AND e.kind='received' AND m.sender_kind='user' AND (?2 IS NULL OR m.id<>?2)",params![persona,current],|r|r.get(0))?;
    let last_reply:Option<String>=db.query_row("SELECT MAX(COALESCE(c.finished_at,a.finished_at,m.created_at)) FROM messages m LEFT JOIN model_calls c ON c.reply_message_id=m.id LEFT JOIN activity_calls a ON a.published_message_id=m.id WHERE m.sender_kind='persona' AND m.sender_id=?1 AND m.status='complete' AND m.publication_sequence IS NOT NULL",[persona],|r|r.get(0))?;
    let receipt: Option<String> = match current {
        Some(id) => db
            .query_row("SELECT created_at FROM messages WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?,
        None => None,
    };
    let elapsed = |at: &Option<String>| -> Result<Option<i64>, Error> {
        Ok(db.query_row(
            "SELECT MAX(0,CAST((julianday('now')-julianday(?1))*86400 AS INTEGER))",
            [at],
            |r| r.get(0),
        )?)
    };
    let utc: String = db.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now')", [], |r| {
        r.get(0)
    })?;
    Ok(
        json!({"runtime":crate::codex::local_time(),"utc":utc,"last_user_contact_at":last_user,"seconds_since_last_user_contact":elapsed(&last_user)?,"last_published_persona_reply_at":last_reply,"seconds_since_last_persona_publication":elapsed(&last_reply)?,"current_user_receipt_at":receipt,"queue_delay_seconds":elapsed(&receipt)?,"instructions":"Frozen activation-time context. Prior user receipt and actual published replies measure contact. The current receipt's queue delay and private activity are never evidence of user silence or rejection."}),
    )
}
