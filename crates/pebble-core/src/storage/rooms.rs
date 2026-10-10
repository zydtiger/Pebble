use super::{
    Database,
    turns::{insert_event_for, new_id},
};
use crate::{Error, Room, RoomMember, rooms::invalid};
use rusqlite::{Connection, OptionalExtension, params};

pub(super) fn room(db: &Connection, id: &str) -> Result<Room, Error> {
    let mut room = db.query_row("SELECT id,kind,CASE WHEN kind='direct' THEN (SELECT display_name FROM personas WHERE id=persona_id) ELSE title END,persona_id FROM conversations WHERE id=?1", [id], |r| Ok(Room {id:r.get(0)?,kind:r.get(1)?,title:r.get(2)?,persona_id:r.get(3)?,members:Vec::new(),former_member_ids:Vec::new()})).optional()?.ok_or_else(||invalid("This chat is no longer available."))?;
    let mut q=db.prepare("SELECT m.persona_id,m.id,p.display_name FROM conversation_memberships m JOIN personas p ON p.id=m.persona_id WHERE m.conversation_id=?1 AND m.left_publication IS NULL ORDER BY m.ordinal")?;
    room.members = q
        .query_map([id], |r| {
            Ok(RoomMember {
                persona_id: r.get(0)?,
                membership_id: r.get(1)?,
                display_name: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    room.former_member_ids = db.prepare("SELECT persona_id FROM conversation_memberships m WHERE conversation_id=?1 AND left_publication IS NOT NULL AND NOT EXISTS(SELECT 1 FROM conversation_memberships current WHERE current.conversation_id=m.conversation_id AND current.persona_id=m.persona_id AND current.left_publication IS NULL) GROUP BY persona_id ORDER BY MIN(ordinal)")?
        .query_map([id], |r|r.get(0))?.collect::<Result<_,_>>()?;
    Ok(room)
}

/// Publication order is assigned at acceptance, independently of display placeholders.
pub(super) fn publish(
    db: &Connection,
    conversation: &str,
    message: &str,
    author: Option<&str>,
) -> Result<(), Error> {
    db.execute(
        "UPDATE conversations SET publication_sequence=publication_sequence+1 WHERE id=?1",
        [conversation],
    )?;
    db.execute("UPDATE messages SET publication_sequence=(SELECT publication_sequence FROM conversations WHERE id=?2) WHERE id=?1",params![message,conversation])?;
    let mut q=db.prepare("SELECT m.persona_id FROM conversation_memberships m JOIN conversations r ON r.id=m.conversation_id WHERE m.conversation_id=?1 AND m.joined_publication<r.publication_sequence AND (m.left_publication IS NULL OR m.left_publication>=r.publication_sequence) AND (?2 IS NULL OR m.persona_id<>?2) ORDER BY m.ordinal")?;
    let members = q
        .query_map(params![conversation, author], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for persona in members {
        insert_event_for(db, &persona, conversation, message, "received")?;
    }
    Ok(())
}
impl Database {
    pub(crate) fn room(&self, id: &str) -> Result<Room, Error> {
        room(&self.connection.lock().expect("database lock"), id)
    }
    pub(crate) fn rooms(&self) -> Result<Vec<Room>, Error> {
        let db = self.connection.lock().expect("database lock");
        let mut q = db.prepare("SELECT id FROM conversations ORDER BY created_at,id")?;
        let ids = q
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.iter().map(|id| room(&db, id)).collect()
    }
    pub(crate) fn create_group(&self, title: &str, members: &[String]) -> Result<Room, Error> {
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid("Give the group a name."));
        }
        if members.is_empty() {
            return Err(invalid("Choose at least one character."));
        }
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let id = new_id();
        tx.execute(
            "INSERT INTO conversations(id,kind,title) VALUES (?1,'group',?2)",
            params![id, title],
        )?;
        let mut unique = Vec::new();
        for member in members {
            if unique.contains(member) {
                continue;
            }
            if !tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM personas WHERE id=?1)",
                [member],
                |r| r.get::<_, bool>(0),
            )? {
                return Err(Error::UnknownPersona);
            }
            tx.execute("INSERT INTO conversation_memberships(id,conversation_id,persona_id,ordinal,joined_publication) VALUES (?1,?2,?3,?4,0)",params![new_id(),id,member,unique.len() as i64])?;
            unique.push(member.clone());
        }
        let result = room(&tx, &id)?;
        tx.commit()?;
        Ok(result)
    }
    pub(crate) fn rename_group(&self, id: &str, title: &str) -> Result<Room, Error> {
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid("Give the group a name."));
        }
        let db = self.connection.lock().expect("database lock");
        if db.execute(
            "UPDATE conversations SET title=?2 WHERE id=?1 AND kind='group'",
            params![id, title],
        )? == 0
        {
            return Err(invalid("Only group chats can be renamed."));
        }
        room(&db, id)
    }
}
