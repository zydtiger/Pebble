//! A frozen original asset, never a model rendition or a model-supplied path.
use crate::{Error, ModelRequest};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Source {
    pub asset_id: String,
    pub filename: String,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
}
impl Source {
    pub(crate) fn path(&self, root: &Path) -> Result<PathBuf, Error> {
        if uuid::Uuid::parse_str(&self.asset_id).is_err()
            || !matches!(
                (self.filename.as_str(), self.mime.as_str()),
                ("source.png", "image/png") | ("source.jpg", "image/jpeg")
            )
        {
            return Err(super::failure("Invalid frozen original image reference."));
        }
        Ok(root
            .join("data/assets")
            .join(&self.asset_id)
            .join(&self.filename))
    }
    pub(crate) fn read(&self, root: &Path) -> Result<Vec<u8>, Error> {
        let bytes =
            crate::images::read_bounded(&self.path(root)?, crate::images::MAX_SOURCE_BYTES)?;
        if bytes.len() as u64 != self.bytes {
            return Err(super::failure(
                "The saved original image is missing or changed.",
            ));
        }
        Ok(bytes)
    }
}
pub(crate) fn freeze(
    db: &Connection,
    request: &ModelRequest,
    persona: &str,
    id: &str,
) -> Result<Source, Error> {
    // Actual image parts prove that the source was exposed, not merely present in
    // state or a tool's untrusted text. Durable event links or frozen carry refs
    // constrain room access to this session boundary (including earlier speakers).
    let mut exposed = false;
    for item in request.items.iter().chain(request.recent_history.iter()) {
        let value: serde_json::Value = serde_json::from_slice(&item.payload)?;
        if value["role"] == "user"
            && value["content"].as_array().is_some_and(|parts| {
                parts
                    .iter()
                    .any(|p| p["type"] == "pebble_image_v1" && p["reference"]["asset_id"] == id)
            })
        {
            exposed = true;
        }
    }
    if !exposed {
        return Err(super::failure(
            "Edit source must be a visible image in this frozen request.",
        ));
    }
    let last = request.items.last().map_or(-1, |item| item.sequence);
    let source = db.query_row(
        "SELECT a.id,a.source_name,a.mime,a.width,a.height,a.source_bytes FROM image_assets a
         WHERE a.id=?1 AND EXISTS(SELECT 1 FROM message_images x JOIN messages m ON m.id=x.message_id
         JOIN persona_events e ON e.message_id=m.id AND e.persona_id=?2
         WHERE x.asset_id=a.id AND m.conversation_id=?3 AND m.status='complete' AND m.publication_sequence IS NOT NULL
         AND (EXISTS(SELECT 1 FROM model_item_events link JOIN model_items i ON i.id=link.item_id
                     WHERE link.event_id=e.id AND i.session_id=?4 AND i.sequence<=?5)
              OR EXISTS(SELECT 1 FROM session_recent_messages recent
                        WHERE recent.session_id=?4 AND recent.message_id=m.id)))",
        params![id,persona,request.conversation_id,request.session_id,last],
        |r| Ok(Source { asset_id:r.get(0)?, filename:r.get(1)?, mime:r.get(2)?, width:r.get(3)?, height:r.get(4)?, bytes:r.get::<_,i64>(5)? as u64 }),
    ).optional()?.ok_or_else(|| super::failure("Edit source must be published in the current room and visible to this persona within its frozen request."))?;
    source.read(&request.image_root)?;
    Ok(source)
}
