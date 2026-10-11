use super::{
    Database,
    turns::{insert_item, new_id},
};
use crate::{
    Error,
    memory::{Call, invalid},
};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::path::Path;

pub(crate) const SAVED_BEFORE_STOP: &str = "Memory was saved before the reply stopped. Its tool result remains recorded; the write will not be repeated.";

// Room Stop/removal can terminalize a call before its worker finishes. Annotate
// only terminal metadata; never revive the call or accept late reply/state/items.
pub(in crate::storage) fn annotate_stopped_memory(
    db: &rusqlite::Connection,
    owner: &str,
    error: Option<&str>,
) -> Result<(), Error> {
    let (saved,pending):(bool,bool)=db.query_row("SELECT EXISTS(SELECT 1 FROM memory_operations WHERE owner_id=?1 AND previous_content!=content AND (status='committed' OR COALESCE(json_extract(result,'$.changed'),0))),EXISTS(SELECT 1 FROM memory_operations WHERE owner_id=?1 AND status='prepared')",[owner],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if !saved && (!pending || error.is_none()) {
        return Ok(());
    }
    let existing: Option<String> =
        db.query_row("SELECT error FROM model_calls WHERE id=?1", [owner], |r| {
            r.get(0)
        })?;
    let mut detail = error
        .map(str::to_owned)
        .or(existing)
        .unwrap_or_else(|| "Reply cancelled.".into());
    if saved && !detail.contains("Memory was saved") {
        detail = format!("{detail} {SAVED_BEFORE_STOP}");
    }
    db.execute("UPDATE model_calls SET error=?2 WHERE id=?1 AND status IN ('cancelled','failed','interrupted')",params![owner,detail])?;
    Ok(())
}

pub(crate) struct MemoryContext<'a> {
    pub owner: &'a str,
    pub persona: &'a crate::Persona,
    pub session: Option<&'a str>,
    pub protocol: &'a str,
    pub expected: &'a str,
}
pub(crate) fn read(path: &Path) -> Result<String, Error> {
    let file = std::fs::File::open(path).map_err(|e| crate::file_error(path, e))?;
    let mut bytes = Vec::new();
    file.take((super::codec::DECODED_CAP + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| crate::file_error(path, e))?;
    if bytes.len() > super::codec::DECODED_CAP {
        return Err(invalid(
            "Current MEMORY exceeds the 8 MiB readable history bound; shorten it manually before memory maintenance.",
        ));
    }
    String::from_utf8(bytes).map_err(|_| invalid("Current MEMORY is not valid UTF-8."))
}
fn effect_detail(error: Error) -> String {
    match error {
        Error::Generation(detail) => detail,
        other => other.to_string(),
    }
}
fn displaced(path: &Path, id: &str) -> std::path::PathBuf {
    path.with_file_name(format!(".MEMORY-{id}.displaced"))
}
fn conflict(path: &Path) -> Error {
    invalid(&format!(
        "Concurrent MEMORY edit detected during publication. MEMORY.md may already contain the proposed update; the displaced file is preserved at {}. Inspect both files and restore the desired content manually before retrying. No automatic rollback was attempted.",
        path.display()
    ))
}
fn exchanged(path: &Path, expected: &str, content: &str, id: &str) -> Result<bool, Error> {
    let actual = read(path)?;
    let previous = displaced(path, id);
    if previous.exists() {
        let before = read(&previous).map_err(|e| invalid(&format!("Memory publication evidence could not be read at {}: {e}. Inspect MEMORY.md and that file; no write will be repeated.", previous.display())))?;
        // The staged file can survive an interruption before exchange.
        if actual == expected && before == content && expected != content {
            return Ok(false);
        }
        if before != expected {
            if actual == content {
                return Err(conflict(&previous));
            }
            return Err(invalid(&format!(
                "Incomplete or conflicting memory publication evidence. Inspect MEMORY.md and the retained file at {}; no write will be repeated.",
                previous.display()
            )));
        }
    } else if expected != content {
        return Err(invalid(&format!(
            "Missing displaced memory publication evidence at {}. MEMORY.md was preserved; matching proposed bytes alone do not prove a completed exchange. Inspect the file before continuing; no write will be repeated.",
            previous.display()
        )));
    }
    Ok(actual == content)
}
#[cfg(unix)]
fn exchange(left: &Path, right: &Path) -> Result<(), Error> {
    use std::os::unix::ffi::OsStrExt;
    let left_c = std::ffi::CString::new(left.as_os_str().as_bytes())
        .map_err(|_| invalid("Invalid memory publication path."))?;
    let right_c = std::ffi::CString::new(right.as_os_str().as_bytes())
        .map_err(|_| invalid("Invalid memory publication path."))?;
    // Both names already exist in the same persona directory. Exchange preserves
    // the actual displaced inode, including writes through an older editor fd.
    #[cfg(target_os = "macos")]
    let status = unsafe { libc::renamex_np(left_c.as_ptr(), right_c.as_ptr(), libc::RENAME_SWAP) };
    #[cfg(target_os = "linux")]
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            left_c.as_ptr(),
            libc::AT_FDCWD,
            right_c.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let status = -1;
    if status != 0 {
        return Err(crate::file_error(right, std::io::Error::last_os_error()));
    }
    Ok(())
}
#[cfg(not(unix))]
fn exchange(_left: &Path, _right: &Path) -> Result<(), Error> {
    Err(invalid(
        "This filesystem platform does not support safe memory exchange.",
    ))
}
fn atomic(
    path: &Path,
    content: &str,
    expected: &str,
    id: &str,
    before_exchange: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    let previous = displaced(path, id);
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&previous)
        .map_err(|e| crate::file_error(&previous, e))?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| crate::file_error(&previous, e))?;
    before_exchange(&previous)?;
    exchange(&previous, path)?;
    // Never remove or swap back this inode: an arbitrary editor may still own it.
    std::fs::File::open(path.parent().expect("persona directory"))
        .and_then(|f| f.sync_all())
        .map_err(|e| crate::file_error(path, e))?;
    if read(&previous)? != expected {
        return Err(conflict(&previous));
    }
    Ok(())
}

impl Database {
    pub(crate) fn archive_tool_output(&self, owner: &str, output: &[Value]) -> Result<(), Error> {
        self.connection.lock().expect("database lock").execute(
            "UPDATE model_calls SET native_tool_output=?2 WHERE id=?1",
            params![owner, serde_json::to_string(output)?],
        )?;
        Ok(())
    }
    pub(crate) fn ensure_memory_resolved(&self, persona: &str) -> Result<(), Error> {
        if self.connection.lock().expect("database lock").query_row("SELECT EXISTS(SELECT 1 FROM memory_operations WHERE persona_id=?1 AND status='prepared')",[persona],|r|r.get::<_,bool>(0))? {return Err(invalid("A memory publication is awaiting durable recovery. Reopen Pebble before continuing this character; no write will be repeated."));}
        Ok(())
    }
    pub(crate) fn tool_used(&self, request: &crate::ModelRequest) -> Result<bool, Error> {
        let db = self.connection.lock().expect("database lock");
        Ok(db.query_row("WITH RECURSIVE attempts(id,retry_of) AS (SELECT id,retry_of FROM model_calls WHERE id=?1 UNION ALL SELECT c.id,c.retry_of FROM model_calls c JOIN attempts a ON c.id=a.retry_of) SELECT EXISTS(SELECT 1 FROM memory_operations o JOIN attempts a ON a.id=o.owner_id)",[&request.call_id],|r|r.get(0))?)
    }
    pub(crate) fn memory_snapshot(&self, owner: &str) -> Result<String, Error> {
        Ok(self.connection.lock().expect("database lock").query_row("WITH RECURSIVE attempts(id,retry_of,memory_snapshot,depth) AS (SELECT id,retry_of,memory_snapshot,0 FROM model_calls WHERE id=?1 UNION ALL SELECT c.id,c.retry_of,c.memory_snapshot,a.depth+1 FROM model_calls c JOIN attempts a ON c.id=a.retry_of) SELECT memory_snapshot FROM attempts WHERE memory_snapshot IS NOT NULL ORDER BY depth LIMIT 1",[owner],|r|r.get(0))?)
    }
    pub(crate) fn memory_reviewed(&self, owner: &str) -> Result<bool, Error> {
        Ok(self.connection.lock().expect("database lock").query_row("SELECT EXISTS(SELECT 1 FROM memory_operations WHERE owner_id=?1 AND status='committed')",[owner],|r|r.get(0))?)
    }
    pub(crate) fn memory_saved(&self, owner: &str) -> Result<bool, Error> {
        let records = {
            let db = self.connection.lock().expect("database lock");
            let mut query = db.prepare("SELECT persona_id,status,content,previous_content,id,COALESCE(json_extract(result,'$.changed'),0) FROM memory_operations WHERE owner_id=?1 AND previous_content!=content")?;
            query
                .query_map([owner], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, bool>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(records
            .into_iter()
            .any(|(persona, status, content, old, id, changed)| {
                status == "committed"
                    || changed
                    || (status == "prepared"
                        && exchanged(
                            &self.root.join("personas").join(persona).join("MEMORY.md"),
                            &old,
                            &content,
                            &id,
                        )
                        .unwrap_or(false))
            }))
    }

    pub(crate) fn execute_memory(
        &self,
        context: MemoryContext<'_>,
        native: &[Value],
        call: &Call,
        control: &crate::chat::Control,
    ) -> Result<Value, Error> {
        let MemoryContext {
            owner,
            persona,
            session,
            protocol,
            expected,
        } = context;
        // The persona queue/maintenance reservation owns this flow across rooms.
        let id = new_id();
        let path = Path::new(&persona.directory).join("MEMORY.md");
        {
            let mut db = self.connection.lock().expect("database lock");
            let tx = db.transaction()?;
            let prior: Option<(String, Option<String>)> = tx.query_row("SELECT arguments,result FROM memory_operations WHERE owner_id=?1 AND provider_call_id=?2",params![owner,call.id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((arguments, result)) = prior {
                if arguments != call.arguments {
                    return Err(invalid(
                        "Duplicate provider call ID has different arguments.",
                    ));
                }
                return result.map(|r|crate::memory::result(protocol,&call.id,&r)).ok_or_else(||invalid("This tool operation is pending recovery; reopen Pebble before retrying."));
            }
            if control.is_cancelled() {
                return Err(invalid("Cancelled before memory effects."));
            }
            if read(&path)? != expected {
                return Err(invalid(
                    "MEMORY changed since this operation began. Reload character files and retry; the manual edit was preserved.",
                ));
            }
            if let Some(session) = session {
                for item in native {
                    insert_item(&tx, session, Some(owner), "output", item)?;
                }
            }
            tx.execute("INSERT INTO memory_operations(id,owner_id,persona_id,session_id,provider_call_id,arguments,previous_content,content,protocol,status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'prepared')",params![id,owner,persona.id,session,call.id,call.arguments,expected,call.content,protocol])?;
            tx.commit()?;
        }
        let effect = control
            .while_active(|| {
                if read(&path)? != expected {
                    return Err(invalid(
                        "MEMORY changed before publication; the manual edit was preserved.",
                    ));
                }
                if call.content != expected {
                    atomic(&path, &call.content, expected, &id, |previous| {
                        #[cfg(test)]
                        if let Some(hook) = control
                            .memory_publication
                            .lock()
                            .expect("test publication hook")
                            .take()
                        {
                            hook(previous)?;
                        }
                        #[cfg(not(test))]
                        let _ = previous;
                        Ok(())
                    })?;
                }
                Ok(())
            })
            .unwrap_or_else(|| Err(invalid("Cancelled before memory effects.")));
        // A failed exchange/fsync can leave proposed bytes at the target. A
        // displaced conflict must never be reclassified as a successful review.
        let published = exchanged(&path, expected, &call.content, &id);
        let changed = published.as_ref().is_ok_and(|p| *p) && call.content != expected;
        let committed = effect.is_ok() && published.as_ref().is_ok_and(|p| *p);
        let detail = if committed {
            if changed {
                "Memory saved.".to_owned()
            } else {
                "Memory reviewed; content unchanged.".to_owned()
            }
        } else {
            let detail = match (effect.err(), published.err()) {
                (Some(effect), Some(evidence)) => {
                    format!("{} {}", effect_detail(effect), effect_detail(evidence))
                }
                (Some(error), None) | (None, Some(error)) => effect_detail(error),
                (None, None) => "Memory was not saved.".into(),
            };
            if changed {
                format!(
                    "{detail} Proposed memory is present, but publication did not complete successfully; inspect MEMORY.md and {} before continuing.",
                    displaced(&path, &id).display()
                )
            } else {
                detail
            }
        };
        let result = json!({"ok":committed,"changed":changed,"detail":detail}).to_string();
        if let Err(error) = self.finish_memory(&id, protocol, &result, committed) {
            return Err(invalid(&format!(
                "{} {detail} Its durable result could not be recorded: {error}. Reopen Pebble to reconcile; no write will be repeated.",
                if changed {
                    "Memory was saved."
                } else {
                    "Memory publication requires recovery."
                }
            )));
        }
        Ok(crate::memory::result(protocol, &call.id, &result))
    }
    fn finish_memory(
        &self,
        id: &str,
        protocol: &str,
        result: &str,
        committed: bool,
    ) -> Result<(), Error> {
        let mut db = self.connection.lock().expect("database lock");
        let tx = db.transaction()?;
        let (owner, session, call): (String, Option<String>, String) = tx.query_row(
            "SELECT owner_id,session_id,provider_call_id FROM memory_operations WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let item = if let Some(session) = session {
            let item = insert_item(
                &tx,
                &session,
                Some(&owner),
                "output",
                &crate::memory::result(protocol, &call, result),
            )?;
            tx.execute("UPDATE model_calls SET input_last_sequence=(SELECT MAX(sequence) FROM model_items WHERE session_id=?2) WHERE id=?1",params![owner,session])?;
            Some(item)
        } else {
            None
        };
        tx.execute(
            "UPDATE memory_operations SET status=?2,result=?3,result_item_id=?4 WHERE id=?1",
            params![
                id,
                if committed { "committed" } else { "failed" },
                result,
                item
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn recover_memory(&self, root: &Path) -> Result<(), Error> {
        let operations = {
            let db = self.connection.lock().expect("database lock");
            let mut q=db.prepare("SELECT id,persona_id,previous_content,content,protocol FROM memory_operations WHERE status='prepared'")?;
            q.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
        };
        for (id, persona, old, new, protocol) in operations {
            let path = root.join("personas").join(persona).join("MEMORY.md");
            let actual = read(&path);
            let publication = exchanged(&path, &old, &new, &id);
            let committed = publication.as_ref().is_ok_and(|p| *p);
            let detail = if let Err(error) = publication {
                effect_detail(error)
            } else if committed {
                "Recovered completed memory review; the final response was interrupted.".to_owned()
            } else if actual.as_ref().is_ok_and(|s| s == &old) {
                "Interrupted before memory publication; no write was repeated.".to_owned()
            } else {
                "Memory recovery found an unreadable or conflicting manual edit. Inspect MEMORY.md before continuing; no write was repeated.".to_owned()
            };
            let result =
                json!({"ok":committed,"changed":committed && old!=new,"detail":detail}).to_string();
            self.finish_memory(&id, &protocol, &result, committed)?;
            let notice = if committed && old != new {
                "Memory was saved before interruption; retry continues from its recorded tool result.".to_owned()
            } else if !committed {
                detail.clone()
            } else {
                String::new()
            };
            if !notice.is_empty() {
                self.connection.lock().expect("database lock").execute("UPDATE model_calls SET error=COALESCE(error,'') || ' ' || ?2 WHERE id=(SELECT owner_id FROM memory_operations WHERE id=?1)",params![id,notice])?;
            }
            let _ = std::fs::remove_file(path.with_file_name(format!(".MEMORY-{id}.tmp")));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_write_and_result_restart_stages_reconcile_without_repeating_effects() {
        for stage in ["prepared", "written", "result", "conflict"] {
            let root = tempfile::tempdir().unwrap();
            let core = crate::Core::open(root.path()).unwrap();
            let persona = core.workspace().unwrap().personas.remove(0);
            let pending = core
                .database
                .enqueue_turn(&persona, "synthetic durable memory")
                .unwrap();
            let turn = core.database.activate_turn(&persona, &pending).unwrap();
            let path = Path::new(&persona.directory).join("MEMORY.md");
            let old = read(&path).unwrap();
            let new = "# Memory\n\nSaved synthetic fact.";
            let operation = new_id();
            let call = Call {
                id: "duplicate-call".into(),
                arguments: json!({"content":new}).to_string(),
                content: new.into(),
            };
            {
                let db = core.database.connection.lock().unwrap();
                insert_item(&db,&turn.session_id,Some(&turn.call_id),"output",&json!({"role":"assistant","content":null,"tool_calls":[{"id":call.id,"type":"function","function":{"name":"update_memory","arguments":call.arguments}}]})).unwrap();
                db.execute("INSERT INTO memory_operations(id,owner_id,persona_id,session_id,provider_call_id,arguments,previous_content,content,protocol,status) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'prepared')",params![operation,turn.call_id,persona.id,turn.session_id,call.id,call.arguments,old,new,"chat_completions"]).unwrap();
            }
            if stage == "written" || stage == "result" {
                atomic(&path, new, &old, &operation, |_| Ok(())).unwrap();
            }
            if stage == "conflict" {
                std::fs::write(&path, "manual conflicting edit").unwrap();
            }
            if stage == "result" {
                core.database
                    .finish_memory(
                        &operation,
                        "chat_completions",
                        &json!({"ok":true,"changed":true}).to_string(),
                        true,
                    )
                    .unwrap();
            }
            let actual = std::fs::read(&path).unwrap();
            drop(core);
            let core = crate::Core::open(root.path()).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), actual);
            let (status, result): (String, String) = core
                .database
                .connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT status,result FROM memory_operations WHERE id=?1",
                    [&operation],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                status,
                if stage == "written" || stage == "result" {
                    "committed"
                } else {
                    "failed"
                }
            );
            let request = core.model_request(&turn.call_id).unwrap();
            let last: Value =
                serde_json::from_slice(&request.items.last().unwrap().payload).unwrap();
            assert_eq!(last["role"], "tool");
            assert_eq!(last["tool_call_id"], call.id);
            assert_eq!(last["content"], result);
            let before = request.items.len();
            let again = core
                .database
                .execute_memory(
                    crate::storage::memory::MemoryContext {
                        owner: &turn.call_id,
                        persona: &persona,
                        session: Some(&turn.session_id),
                        protocol: "chat_completions",
                        expected: &old,
                    },
                    &[],
                    &call,
                    &crate::chat::Control::default(),
                )
                .unwrap();
            assert_eq!(again, last);
            assert_eq!(
                core.model_request(&turn.call_id).unwrap().items.len(),
                before
            );
            let changed = Call {
                arguments: "different".into(),
                ..call
            };
            assert!(
                core.database
                    .execute_memory(
                        crate::storage::memory::MemoryContext {
                            owner: &turn.call_id,
                            persona: &persona,
                            session: Some(&turn.session_id),
                            protocol: "chat_completions",
                            expected: &old
                        },
                        &[],
                        &changed,
                        &crate::chat::Control::default()
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("Duplicate")
            );
        }
    }

    #[test]
    fn pre_exchange_manual_save_is_preserved_as_conflict_across_restart() {
        for maintenance in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let core = crate::Core::open(root.path()).unwrap();
            let persona = core.workspace().unwrap().personas.remove(0);
            let (owner, session) = if maintenance {
                (core.database.begin_maintenance(&persona).unwrap().id, None)
            } else {
                let pending = core.database.enqueue_turn(&persona, "synthetic").unwrap();
                let turn = core.database.activate_turn(&persona, &pending).unwrap();
                (turn.call_id, Some(turn.session_id))
            };
            let path = Path::new(&persona.directory).join("MEMORY.md");
            let old = read(&path).unwrap();
            let authored = path.clone();
            let control = crate::chat::Control::default();
            *control.memory_publication.lock().unwrap() = Some(Box::new(move |_| {
                // A complete editor atomic save after the final comparison and fsync.
                let save = authored.with_extension("editor-save");
                std::fs::write(&save, "concurrent manual save").unwrap();
                std::fs::rename(save, authored).unwrap();
                Ok(())
            }));
            // Keep the operation prepared to exercise recovery after a result failure.
            core.database.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_conflict_result BEFORE UPDATE ON memory_operations BEGIN SELECT RAISE(FAIL,'result interruption'); END;").unwrap();
            let call = Call {
                id: "race".into(),
                arguments: json!({"content":"proposed memory"}).to_string(),
                content: "proposed memory".into(),
            };
            let error = core
                .database
                .execute_memory(
                    MemoryContext {
                        owner: &owner,
                        persona: &persona,
                        session: session.as_deref(),
                        protocol: "chat_completions",
                        expected: &old,
                    },
                    &[],
                    &call,
                    &control,
                )
                .unwrap_err();
            assert!(error.to_string().contains("recovery"));
            let operation: String = core
                .database
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT id FROM memory_operations", [], |r| r.get(0))
                .unwrap();
            let previous = displaced(&path, &operation);
            assert_eq!(read(&path).unwrap(), "proposed memory");
            assert_eq!(read(&previous).unwrap(), "concurrent manual save");
            assert!(!core.database.memory_saved(&owner).unwrap());
            core.database
                .connection
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER reject_conflict_result")
                .unwrap();
            drop(core);
            let core = crate::Core::open(root.path()).unwrap();
            let (status, result): (String, String) = core
                .database
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT status,result FROM memory_operations", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .unwrap();
            assert_eq!(status, "failed");
            let result: Value = serde_json::from_str(&result).unwrap();
            assert_eq!(result["ok"], false);
            assert!(
                result["detail"]
                    .as_str()
                    .unwrap()
                    .contains(previous.to_str().unwrap())
            );
            assert!(!core.database.memory_reviewed(&owner).unwrap());
            assert_eq!(read(&path).unwrap(), "proposed memory");
            assert_eq!(read(&previous).unwrap(), "concurrent manual save");
            if maintenance {
                let status = core.maintenance_status(&persona.id).unwrap().unwrap();
                assert_eq!(status.status, "interrupted");
                assert!(status.detail.contains(previous.to_str().unwrap()));
                assert!(status.detail.contains("may already contain"));
                assert!(!status.memory_reviewed && !status.memory_saved);
                assert!(
                    core.context_markers(&persona.conversation_id)
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }

    #[test]
    fn changed_prepared_review_requires_displaced_evidence_even_when_manual_target_matches() {
        for maintenance in [false, true] {
            for evidence in ["missing", "unreadable"] {
                let root = tempfile::tempdir().unwrap();
                let core = crate::Core::open(root.path()).unwrap();
                let persona = core.workspace().unwrap().personas.remove(0);
                let pending = core
                    .database
                    .enqueue_turn(&persona, "synthetic source")
                    .unwrap();
                let turn = core.database.activate_turn(&persona, &pending).unwrap();
                let (owner, session) = if maintenance {
                    core.database
                        .finish_turn(
                            &turn,
                            "",
                            crate::MessageStatus::Failed,
                            Some("synthetic failed source"),
                            None,
                        )
                        .unwrap();
                    (core.database.begin_maintenance(&persona).unwrap().id, None)
                } else {
                    (turn.call_id.clone(), Some(turn.session_id.clone()))
                };
                let path = Path::new(&persona.directory).join("MEMORY.md");
                let old = read(&path).unwrap();
                let proposed = "same bytes saved by external editor";
                let operation = new_id();
                let arguments = json!({"content":proposed}).to_string();
                {
                    let db = core.database.connection.lock().unwrap();
                    if let Some(session) = &session {
                        insert_item(&db,session,Some(&owner),"output",&json!({"role":"assistant","content":null,"tool_calls":[{"id":"missing-evidence-call","type":"function","function":{"name":"update_memory","arguments":arguments}}]})).unwrap();
                    }
                    db.execute("INSERT INTO memory_operations(id,owner_id,persona_id,session_id,provider_call_id,arguments,previous_content,content,protocol,status) VALUES(?1,?2,?3,?4,'missing-evidence-call',?5,?6,?7,'chat_completions','prepared')",params![operation,owner,persona.id,session,arguments,old,proposed]).unwrap();
                }
                // The app stopped after journaling, without staging or exchange.
                // An external editor happens to save the proposed bytes meanwhile.
                std::fs::write(&path, proposed).unwrap();
                let previous = displaced(&path, &operation);
                if evidence == "unreadable" {
                    std::fs::write(&previous, [0xff]).unwrap();
                }
                assert!(!core.database.memory_saved(&owner).unwrap());
                drop(core);
                let core = crate::Core::open(root.path()).unwrap();
                let (status, result): (String, String) = core
                    .database
                    .connection
                    .lock()
                    .unwrap()
                    .query_row(
                        "SELECT status,result FROM memory_operations WHERE id=?1",
                        [&operation],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(status, "failed");
                let result: Value = serde_json::from_str(&result).unwrap();
                assert_eq!(result["ok"], false);
                assert_eq!(result["changed"], false);
                let detail = result["detail"].as_str().unwrap();
                assert!(detail.contains(previous.to_str().unwrap()));
                assert!(detail.contains(if evidence == "missing" {
                    "Missing displaced"
                } else {
                    "could not be read"
                }));
                assert_eq!(read(&path).unwrap(), proposed);
                assert!(!core.database.memory_reviewed(&owner).unwrap());
                assert!(!core.database.memory_saved(&owner).unwrap());
                let active: String = core
                    .database
                    .connection
                    .lock()
                    .unwrap()
                    .query_row(
                        "SELECT id FROM model_sessions WHERE persona_id=?1 AND status='active'",
                        [&persona.id],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(active, turn.session_id);
                assert!(
                    core.context_markers(&persona.conversation_id)
                        .unwrap()
                        .is_empty()
                );
                if maintenance {
                    let status = core.maintenance_status(&persona.id).unwrap().unwrap();
                    assert_eq!(status.status, "interrupted");
                    assert!(!status.memory_reviewed && !status.memory_saved);
                    assert!(status.detail.contains(previous.to_str().unwrap()));
                } else {
                    let message = core.messages(&persona.id).unwrap().pop().unwrap();
                    assert_eq!(message.status, crate::MessageStatus::Interrupted);
                    assert!(message.failure_detail.contains(previous.to_str().unwrap()));
                }
            }
        }
    }
    #[test]
    fn old_editor_descriptor_keeps_late_in_place_save_in_displaced_file() {
        use std::io::{Seek, SeekFrom};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("MEMORY.md");
        std::fs::write(&path, "expected").unwrap();
        let mut editor = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        atomic(&path, "proposed", "expected", "late-editor", |_| Ok(())).unwrap();
        editor.seek(SeekFrom::Start(0)).unwrap();
        editor.set_len(0).unwrap();
        editor.write_all(b"late authored edit").unwrap();
        editor.sync_all().unwrap();
        assert_eq!(read(&path).unwrap(), "proposed");
        assert_eq!(
            read(&displaced(&path, "late-editor")).unwrap(),
            "late authored edit"
        );
        assert!(exchanged(&path, "expected", "proposed", "late-editor").is_err());
    }
    #[test]
    fn unchanged_memory_is_successful_noop_and_read_failure_has_no_journal_effect() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::Core::open(root.path()).unwrap();
        let persona = core.workspace().unwrap().personas.remove(0);
        let pending = core.database.enqueue_turn(&persona, "synthetic").unwrap();
        let turn = core.database.activate_turn(&persona, &pending).unwrap();
        let path = Path::new(&persona.directory).join("MEMORY.md");
        let old = read(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let call = Call {
            id: "noop".into(),
            arguments: json!({"content":old}).to_string(),
            content: old.clone(),
        };
        let result = core
            .database
            .execute_memory(
                crate::storage::memory::MemoryContext {
                    owner: &turn.call_id,
                    persona: &persona,
                    session: Some(&turn.session_id),
                    protocol: "chat_completions",
                    expected: &old,
                },
                &[],
                &call,
                &crate::chat::Control::default(),
            )
            .unwrap();
        let output: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(output["ok"], true);
        assert_eq!(output["changed"], false);
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            modified
        );
        std::fs::remove_file(&path).unwrap();
        let call = Call {
            id: "read-fail".into(),
            ..call
        };
        assert!(
            core.database
                .execute_memory(
                    crate::storage::memory::MemoryContext {
                        owner: &turn.call_id,
                        persona: &persona,
                        session: Some(&turn.session_id),
                        protocol: "chat_completions",
                        expected: &old
                    },
                    &[],
                    &call,
                    &crate::chat::Control::default()
                )
                .is_err()
        );
        assert_eq!(
            core.database
                .connection
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn failed_atomic_prepare_preserves_authored_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("MEMORY.md");
        std::fs::write(&path, "authored memory").unwrap();
        std::fs::create_dir(root.path().join(".MEMORY-write-failure.displaced")).unwrap();
        assert!(
            atomic(
                &path,
                "replacement",
                "authored memory",
                "write-failure",
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(read(&path).unwrap(), "authored memory");
    }

    #[test]
    fn saved_file_with_failed_result_record_is_explicit_and_reconciled() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::Core::open(root.path()).unwrap();
        let persona = core.workspace().unwrap().personas.remove(0);
        let pending = core.database.enqueue_turn(&persona, "synthetic").unwrap();
        let turn = core.database.activate_turn(&persona, &pending).unwrap();
        let path = Path::new(&persona.directory).join("MEMORY.md");
        let old = read(&path).unwrap();
        core.database.connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_memory_result BEFORE UPDATE ON memory_operations BEGIN SELECT RAISE(FAIL,'synthetic result failure'); END;").unwrap();
        let call = Call {
            id: "result-failure".into(),
            arguments: json!({"content":"saved fact"}).to_string(),
            content: "saved fact".into(),
        };
        let error = core
            .database
            .execute_memory(
                MemoryContext {
                    owner: &turn.call_id,
                    persona: &persona,
                    session: Some(&turn.session_id),
                    protocol: "chat_completions",
                    expected: &old,
                },
                &[],
                &call,
                &crate::chat::Control::default(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("Memory was saved"));
        assert!(core.database.memory_saved(&turn.call_id).unwrap());
        assert!(core.database.ensure_memory_resolved(&persona.id).is_err());
        assert_eq!(read(&path).unwrap(), "saved fact");
        core.database
            .connection
            .lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_memory_result")
            .unwrap();
        drop(core);
        let core = crate::Core::open(root.path()).unwrap();
        assert!(core.database.memory_reviewed(&turn.call_id).unwrap());
        assert!(core.database.ensure_memory_resolved(&persona.id).is_ok());
        assert_eq!(read(&path).unwrap(), "saved fact");
        let count: i64 = core
            .database
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM memory_operations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}
