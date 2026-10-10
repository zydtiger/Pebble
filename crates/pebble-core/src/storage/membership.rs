use super::{Database, rooms::room, turns::new_id};
use crate::{Error, Room, rooms::invalid};
use rusqlite::{OptionalExtension, params};

impl Database {
    pub(crate) fn add_group_member(&self, id: &str, persona: &str) -> Result<Room, Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let current = room(&tx, id)?;
        if current.kind != "group" {
            return Err(invalid("Manage members in a group chat."));
        }
        if !current.members.iter().any(|m| m.persona_id == persona) {
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM personas WHERE id=?1)",
                [persona],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::UnknownPersona);
            }
            tx.execute("INSERT INTO conversation_memberships(id,conversation_id,persona_id,ordinal,joined_publication) VALUES (?1,?2,?3,(SELECT COALESCE(MAX(ordinal),-1)+1 FROM conversation_memberships WHERE conversation_id=?2),(SELECT publication_sequence FROM conversations WHERE id=?2))",params![new_id(),id,persona])?;
        }
        let result = room(&tx, id)?;
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn remove_group_member(
        &self,
        id: &str,
        persona: &str,
    ) -> Result<(Room, Vec<String>), Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        if room(&tx, id)?.kind != "group" {
            return Err(invalid("Manage members in a group chat."));
        }
        let membership:Option<String> = tx.query_row("SELECT id FROM conversation_memberships WHERE conversation_id=?1 AND persona_id=?2 AND left_publication IS NULL",params![id,persona],|r|r.get(0)).optional()?;
        let calls = if let Some(membership) = membership {
            tx.execute("UPDATE conversation_memberships SET left_publication=(SELECT publication_sequence FROM conversations WHERE id=?2) WHERE id=?1",params![membership,id])?;
            super::rounds::cancel_membership(&tx, &membership)?
        } else {
            Vec::new()
        };
        let result = room(&tx, id)?;
        tx.commit()?;
        Ok((result, calls))
    }
}
