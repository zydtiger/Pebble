use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Complete,
    Streaming,
    Queued,
    Cancelled,
    Interrupted,
    Failed,
}

impl MessageStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Streaming | Self::Queued => "streaming",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub sequence: i64,
    pub sender_kind: String,
    pub sender_id: String,
    pub content: String,
    pub status: MessageStatus,
    pub created_at: String,
    pub failure_detail: String,
    pub can_retry: bool,
    pub images: Vec<crate::ImageAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelItem {
    pub id: String,
    pub sequence: i64,
    /// Frozen JSON bytes, preserved rather than re-rendered from visible messages.
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Runtime resolver only; never stored in frozen native history.
    #[serde(skip)]
    pub image_root: std::path::PathBuf,
    pub call_id: String,
    pub session_id: String,
    pub conversation_id: String,
    pub trigger_message_id: String,
    pub event_cutoff: i64,
    pub protocol: String,
    pub model: String,
    pub items: Vec<ModelItem>,
    /// None identifies a preserved v0.1.0 session, never reinterpreted as structured.
    pub response_schema: Option<serde_json::Value>,
    pub state_before: Option<serde_json::Value>,
    pub memory_tools: bool,
    pub memory_capable: bool,
    #[serde(default)]
    pub delegation_tools: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Turn {
    pub call_id: String,
    pub session_id: String,
    pub receipt_cutoff: i64,
    pub persona_id: String,
    pub conversation_id: String,
    pub user: Message,
    pub reply: Message,
    pub retry_of: Option<String>,
}
