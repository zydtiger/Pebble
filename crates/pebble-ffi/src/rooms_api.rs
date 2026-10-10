use crate::{
    BackendError, ChatHandle, ChatObserver, MessageRecord, ObserverAdapter, PebbleBackend,
};
use std::sync::Arc;

#[derive(Clone, uniffi::Record)]
pub struct RoomMemberRecord {
    pub persona_id: String,
    pub membership_id: String,
    pub display_name: String,
}
#[derive(Clone, uniffi::Record)]
pub struct RoomRecord {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub persona_id: Option<String>,
    pub members: Vec<RoomMemberRecord>,
}
impl From<pebble_core::Room> for RoomRecord {
    fn from(room: pebble_core::Room) -> Self {
        Self {
            id: room.id,
            kind: room.kind,
            title: room.title,
            persona_id: room.persona_id,
            members: room
                .members
                .into_iter()
                .map(|m| RoomMemberRecord {
                    persona_id: m.persona_id,
                    membership_id: m.membership_id,
                    display_name: m.display_name,
                })
                .collect(),
        }
    }
}
#[derive(uniffi::Record)]
pub struct ChatSubmissionRecord {
    pub message: MessageRecord,
    pub handles: Vec<Arc<ChatHandle>>,
}
#[uniffi::export]
impl PebbleBackend {
    pub fn create_group(
        &self,
        title: String,
        members: Vec<String>,
    ) -> Result<RoomRecord, BackendError> {
        Ok(self.core.create_group(&title, &members)?.into())
    }
    pub fn rename_group(&self, room_id: String, title: String) -> Result<RoomRecord, BackendError> {
        Ok(self.core.rename_group(&room_id, &title)?.into())
    }
    pub fn room_messages(&self, room_id: String) -> Result<Vec<MessageRecord>, BackendError> {
        Ok(self
            .core
            .room_messages(&room_id)?
            .into_iter()
            .map(Into::into)
            .collect())
    }
    pub fn send_room_message(
        &self,
        room_id: String,
        prompt: String,
        mentions: Vec<String>,
        observer: Arc<dyn ChatObserver>,
    ) -> Result<ChatSubmissionRecord, BackendError> {
        let submission = self.core.send_room_message(
            &room_id,
            &prompt,
            &mentions,
            Arc::new(ObserverAdapter(observer)),
        )?;
        Ok(ChatSubmissionRecord {
            message: submission.message.into(),
            handles: submission
                .tasks
                .into_iter()
                .map(|task| Arc::new(ChatHandle { task }))
                .collect(),
        })
    }
}
