mod live_support;
use live_support::{isolated, reopen, start, terminal};
use pebble_core::{ChatEventKind, Error};
use std::fs;

#[test]
fn fresh_context_keeps_transcript_snapshots_current_files_and_survives_restart() {
    let (root, core, id) = isolated();
    let (task, rx) = start(&core, &id, "earlier private input");
    terminal(&rx);
    let old = core.model_request(&task.id).unwrap();
    let saved = core.messages(&id).unwrap();
    let persona = core.workspace().unwrap().personas.remove(0);
    let directory = std::path::Path::new(&persona.directory);
    fs::write(directory.join("SOUL.md"), "fresh soul snapshot 🪨").unwrap();
    fs::write(directory.join("MEMORY.md"), "fresh memory snapshot").unwrap();
    let fresh = core.fresh_context(&id).unwrap();
    assert!(fresh.context_notice.contains("Fresh model context"));
    assert_eq!(core.messages(&id).unwrap(), saved);
    fs::write(
        directory.join("SOUL.md"),
        "later file edit must not rewrite snapshot",
    )
    .unwrap();
    drop(task);
    drop(core);
    let core = reopen(root.path());
    assert!(
        core.workspace().unwrap().personas[0]
            .context_notice
            .contains("Fresh model context")
    );
    let (task, rx) = start(&core, &id, "new private input");
    assert_eq!(terminal(&rx).last().unwrap().kind, ChatEventKind::Completed);
    let request = core.model_request(&task.id).unwrap();
    assert_ne!(request.session_id, old.session_id);
    assert_eq!(request.items.len(), 2);
    let prefix = String::from_utf8_lossy(&request.items[0].payload);
    assert!(prefix.contains("fresh soul snapshot"));
    assert!(prefix.contains("fresh memory snapshot"));
    assert!(!prefix.contains("later file edit"));
    assert!(
        !request
            .items
            .iter()
            .any(|i| String::from_utf8_lossy(&i.payload).contains("earlier private input"))
    );
    let replay = core.model_request(&old.call_id).unwrap();
    for (a, b) in old.items.iter().zip(replay.items) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.payload, b.payload);
    }
    assert_eq!(core.messages(&id).unwrap()[..saved.len()], saved);
}

#[test]
fn reset_refuses_active_or_queued_selected_persona_without_blocking_other_rooms() {
    let (_root, core, a) = isolated();
    let b = core.create_persona("Kai").unwrap();
    let (a1, rx1) = start(&core, &a, "A first");
    let (a2, rx2) = start(&core, &a, "A queued");
    assert!(matches!(core.fresh_context(&a), Err(Error::Busy)));
    core.fresh_context(&b.id).unwrap();
    let (b1, brx) = start(&core, &b.id, "B independent");
    a1.cancel();
    a2.cancel();
    terminal(&rx1);
    terminal(&rx2);
    core.fresh_context(&a).unwrap();
    assert_eq!(
        terminal(&brx).last().unwrap().kind,
        ChatEventKind::Completed
    );
    let request = core.model_request(&b1.id).unwrap();
    assert!(
        !request
            .items
            .iter()
            .any(|i| String::from_utf8_lossy(&i.payload).contains("A first"))
    );
    let (a3, rx3) = start(&core, &a, "A fresh input");
    terminal(&rx3);
    let request = core.model_request(&a3.id).unwrap();
    assert_eq!(request.items.len(), 2);
    assert!(!String::from_utf8_lossy(&request.items[1].payload).contains("A queued"));
}
