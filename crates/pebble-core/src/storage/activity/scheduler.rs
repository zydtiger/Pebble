use super::*;

pub(super) fn user_work_pending(db: &rusqlite::Connection, persona: &str) -> Result<bool, Error> {
    // A group member can already owe a reply before its model_call exists.
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_calls WHERE persona_id=?1 AND status IN ('queued','streaming')) OR EXISTS(SELECT 1 FROM group_reply_slots s JOIN participation_decisions d ON d.id=s.decision_id WHERE d.persona_id=?1 AND s.status IN ('pending','admitted'))",
        [persona],
        |r| r.get(0),
    )?)
}

impl Database {
    /// Records a scheduler wake without producing a message, notification, or persona event.
    /// The worker claims this durable step before it starts a private model call.
    pub(crate) fn claim_due_activity(&self, persona: &str) -> Result<Option<String>, Error> {
        let mut connection = self.connection.lock().expect("database lock");
        let tx = connection.transaction()?;
        Self::ensure_activity_settings(&tx, persona)?;
        let policy = policy::read(&self.root, persona)?;
        Self::reconcile_activity_policy(&tx, persona, &policy)?;
        if !policy.active() {
            tx.commit()?;
            return Ok(None);
        }
        let due: bool = tx.query_row("SELECT (next_wake_at IS NOT NULL AND next_wake_at<=strftime('%Y-%m-%dT%H:%M:%fZ','now')) OR next_reason IN ('Quiet hours','Daily private activity budget reached; resumes next UTC day') FROM persona_activity_runtime WHERE persona_id=?1", [persona], |r| r.get(0))?;
        if !due {
            tx.commit()?;
            return Ok(None);
        }
        let busy:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM activity_steps s JOIN persona_activities a ON a.id=s.activity_id WHERE a.persona_id=?1 AND s.status IN ('pending','running')) OR EXISTS(SELECT 1 FROM model_calls WHERE persona_id=?1 AND status IN ('queued','streaming')) OR EXISTS(SELECT 1 FROM context_maintenance WHERE persona_id=?1 AND status='running')",[persona],|r|r.get(0))?;
        if busy || user_work_pending(&tx, persona)? {
            tx.commit()?;
            return Ok(None);
        }
        let (start, end) = policy.quiet_minutes();
        if let (Some(start), Some(end)) = (start, end) {
            let start = i64::from(start);
            let end = i64::from(end);
            let now = local_minutes();
            let inside = if start < end {
                now >= start && now < end
            } else {
                now >= start || now < end
            };
            if inside {
                let minutes = (end - now).rem_euclid(1440).max(1);
                tx.execute("UPDATE persona_activity_runtime SET next_wake_at=strftime('%Y-%m-%dT%H:%M:%fZ','now',?2),next_reason='Quiet hours' WHERE persona_id=?1",params![persona,format!("+{minutes} minutes")])?;
                tx.commit()?;
                return Ok(None);
            }
        }
        let day: String = tx.query_row("SELECT strftime('%Y-%m-%d','now')", [], |r| r.get(0))?;
        let (saved_day, used): (String, i64) = tx.query_row(
            "SELECT budget_day,wakes_used FROM persona_activity_runtime WHERE persona_id=?1",
            [persona],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let used = if saved_day == day { used } else { 0 };
        if used >= i64::from(policy.daily_wake_budget) {
            // Budgets use UTC calendar days for durable, DST-independent
            // accounting. Quiet hours remain local-time admission controls.
            tx.execute("UPDATE persona_activity_runtime SET next_wake_at=strftime('%Y-%m-%dT00:00:00Z','now','+1 day'),next_reason='Daily private activity budget reached; resumes next UTC day',budget_day=?2,wakes_used=?3 WHERE persona_id=?1",params![persona,day,used])?;
            tx.commit()?;
            return Ok(None);
        }
        let activity: Option<String> = tx.query_row("SELECT id FROM persona_activities WHERE persona_id=?1 AND phase NOT IN ('stopped','failed') ORDER BY updated_at DESC LIMIT 1",[persona],|r|r.get(0)).optional()?;
        let Some(activity) = activity else {
            tx.execute("UPDATE persona_activity_runtime SET next_wake_at=NULL,next_reason='No active private objective' WHERE persona_id=?1",[persona])?;
            tx.commit()?;
            return Ok(None);
        };
        let step = Uuid::new_v4().to_string();
        tx.execute("INSERT INTO activity_steps(id,activity_id,wake_reason,status) VALUES(?1,?2,'timer','pending')",params![step,activity])?;
        tx.execute("INSERT INTO activity_events(id,activity_id,ordinal,kind,summary,provenance) VALUES(?1,?2,(SELECT COALESCE(MAX(ordinal),0)+1 FROM activity_events WHERE activity_id=?2),'wake','Private check admitted','scheduler')",params![Uuid::new_v4().to_string(),activity])?;
        tx.execute("UPDATE persona_activities SET phase='evaluating',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",[activity])?;
        tx.execute("UPDATE persona_activity_runtime SET budget_day=?2,wakes_used=?3,next_wake_at=NULL,next_reason='Private check in progress',updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE persona_id=?1",params![persona,day,used+1])?;
        tx.commit()?;
        Ok(Some(step))
    }
}
