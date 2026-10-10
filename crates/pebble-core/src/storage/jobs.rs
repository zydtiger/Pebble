use super::{
    Database,
    turns::{insert_message, message, new_id},
};
use crate::{Error, Message, MessageStatus, Persona, history::Turn};
use rusqlite::params;

impl Database {
    /// One canonical user message; all observations and selected calls accept atomically.
    pub(crate) fn enqueue_room(
        &self,
        room_id: &str,
        prompt: &str,
        picked: &[String],
        personas: &[Persona],
    ) -> Result<(Message, Vec<Turn>, Vec<crate::participation::DecisionWork>), Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        let room = super::rooms::room(&tx, room_id)?;
        let (text, selected) = if room.kind == "direct" {
            if !picked.is_empty() {
                return Err(crate::rooms::invalid("Use mentions in a group chat."));
            }
            (
                prompt.to_owned(),
                vec![room.persona_id.clone().ok_or(Error::UnknownPersona)?],
            )
        } else {
            crate::rooms::mentions(prompt, picked, &room.members, personas)?
        };
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let user = insert_message(
            &tx,
            room_id,
            "user",
            "owner",
            &text,
            MessageStatus::Complete,
        )?;
        super::rooms::publish(&tx, room_id, &user, None)?;
        if let Some(persona) = room.persona_id.as_ref() {
            super::turns::insert_event_for(&tx, persona, room_id, &user, "received")?;
        }
        let mut turns = Vec::new();
        for id in selected {
            let persona = personas
                .iter()
                .find(|p| p.id == id)
                .ok_or(Error::UnknownPersona)?;
            let membership = room
                .members
                .iter()
                .find(|m| m.persona_id == id)
                .map(|m| m.membership_id.as_str());
            if let Some(membership) = membership {
                tx.execute("INSERT INTO message_mentions(message_id,persona_id,membership_id) VALUES (?1,?2,?3)",params![user,id,membership])?;
            }
            let reply = insert_message(&tx, room_id, "persona", &id, "", MessageStatus::Streaming)?;
            let receipt_cutoff = tx.query_row(
                "SELECT MAX(sequence) FROM persona_events WHERE persona_id=?1",
                [&id],
                |r| r.get::<_, i64>(0),
            )?;
            let call_id = new_id();
            tx.execute("INSERT INTO model_calls (id,conversation_id,trigger_message_id,reply_message_id,event_cutoff,status,persona_id,persona_sequence,membership_id) VALUES (?1,?2,?3,?4,?5,'queued',?6,(SELECT COALESCE(MAX(persona_sequence),0)+1 FROM model_calls WHERE persona_id=?6),?7)",params![call_id,room_id,user,reply,receipt_cutoff,id,membership])?;
            let mut reply = message(&tx, &reply)?;
            reply.status = MessageStatus::Queued;
            turns.push(Turn {
                call_id,
                session_id: String::new(),
                receipt_cutoff,
                persona_id: persona.id.clone(),
                conversation_id: room_id.to_owned(),
                user: message(&tx, &user)?,
                reply,
                retry_of: None,
            });
        }
        let user = message(&tx, &user)?;
        let mut decisions = Vec::new();
        if room.kind == "group" {
            tx.execute("UPDATE participation_decisions SET status='cancelled',error='A newer group message replaced this decision.' WHERE conversation_id=?1 AND status='pending'", [room_id])?;
            for member in &room.members {
                let persona = personas
                    .iter()
                    .find(|p| p.id == member.persona_id)
                    .ok_or(Error::UnknownPersona)?;
                let call_id = turns
                    .iter()
                    .find(|t| t.persona_id == member.persona_id)
                    .map(|t| t.call_id.clone());
                let status = if call_id.is_some() {
                    "mentioned"
                } else if persona.classifier.is_some() {
                    "pending"
                } else {
                    "unconfigured"
                };
                let cutoff: i64 = tx.query_row("SELECT sequence FROM persona_events WHERE persona_id=?1 AND message_id=?2 AND kind='received'", params![member.persona_id,user.id], |r|r.get(0))?;
                let id = new_id();
                tx.execute("INSERT INTO participation_decisions(id,message_id,conversation_id,persona_id,membership_id,event_cutoff,revision,status,call_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![id,user.id,room_id,member.persona_id,member.membership_id,cutoff,user.sequence,status,call_id])?;
                let input = if status == "pending" {
                    crate::participation::compact_input(&tx, persona, room_id, cutoff)?
                } else {
                    String::new()
                };
                decisions.push(crate::participation::DecisionWork {
                    decision: super::participation::read(&tx, &id)?,
                    input,
                });
            }
        }
        tx.commit()?;
        Ok((user, turns, decisions))
    }
    pub(crate) fn enqueue_turn(&self, persona: &Persona, prompt: &str) -> Result<Turn, Error> {
        Ok(self
            .enqueue_room(
                &persona.conversation_id,
                prompt,
                &[],
                std::slice::from_ref(persona),
            )?
            .1
            .remove(0))
    }

    pub(crate) fn finish_pending(
        &self,
        pending: &Turn,
        status: MessageStatus,
        error: Option<&str>,
    ) -> Result<Message, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        tx.execute(
            "UPDATE messages SET status = ?2 WHERE id = ?1 AND status = 'streaming'",
            params![pending.reply.id, status.as_str()],
        )?;
        tx.execute("UPDATE model_calls SET status = ?2, error = ?3, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1 AND status = 'queued'", params![pending.call_id, status.as_str(), error])?;
        let reply = message(&tx, &pending.reply.id)?;
        tx.commit()?;
        Ok(reply)
    }
}
