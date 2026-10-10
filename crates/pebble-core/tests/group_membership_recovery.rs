mod classifier_support;
mod group_support;
mod live_support;
mod responses_support;
use classifier_support::{Reply, Server, answer, configure, submit};
use group_support::{count, db, task};
use pebble_core::{ChatEventKind, CredentialChange, MessageStatus, SettingsUpdate};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

fn connection(core: &pebble_core::Core, persona: &str, url: &str, api: &str) {
    let mut s = core.settings(Some(persona)).unwrap();
    s.chat.enabled = true;
    s.chat.model = "synthetic".into();
    s.chat.base_url = url.into();
    s.chat.api = api.into();
    core.save_settings(
        Some(persona),
        &SettingsUpdate {
            chat: s.chat,
            credential: CredentialChange::Clear,
            context_window_tokens: s.context_window_tokens,
            max_output_tokens: s.max_output_tokens,
            persona: s.persona,
        },
    )
    .unwrap();
}
fn generated(api: &str) -> Reply {
    let text = live_support::structured("Accepted once across restart");
    Reply::stream(if api == "responses" {
        responses_support::candidate(&text)
    } else {
        live_support::candidate(&text)
    })
}
fn native(root: &Path) -> Value {
    let db = db(root);
    let rows = db
        .prepare("SELECT id,session_id,codec,hex(payload),sequence FROM model_items ORDER BY id")
        .unwrap()
        .query_map([], |r| {
            Ok(json!([
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?
            ]))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    json!(rows)
}

#[test]
fn reopen_keeps_accepted_native_and_interrupts_classifier_queued_generating_both_apis() {
    for api in ["chat_completions", "responses"] {
        for phase in ["classifying", "queued", "generating"] {
            let root = tempfile::tempdir().unwrap();
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "membership_crash_fixture", "--nocapture"])
                .env("PEBBLE_MEMBERSHIP_CRASH_ROOT", root.path())
                .env("PEBBLE_MEMBERSHIP_CRASH_API", api)
                .env("PEBBLE_MEMBERSHIP_CRASH_PHASE", phase)
                .status()
                .unwrap();
            assert!(child.success());
            let before: Value =
                serde_json::from_slice(&std::fs::read(root.path().join("before.json")).unwrap())
                    .unwrap();
            let core = live_support::reopen(root.path());
            let workspace = core.workspace().unwrap();
            let persona = &workspace.personas[0].id;
            let room = workspace.rooms.iter().find(|r| r.kind == "group").unwrap();
            assert_eq!(room.id, before["room"].as_str().unwrap());
            assert_eq!(
                room.members[0].membership_id,
                before["membership"].as_str().unwrap()
            );
            assert_eq!(native(root.path()), before["native"]);
            assert_eq!(
                core.response_settings(persona).unwrap().current_state_json,
                before["state"].as_str().unwrap()
            );
            let db = db(root.path());
            assert_eq!(
                count(&db, "SELECT COUNT(*) FROM model_calls"),
                before["calls"].as_i64().unwrap()
            );
            assert_eq!(
                count(&db, "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"),
                1
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM group_rounds WHERE status='interrupted'"
                ),
                1
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM group_reply_slots WHERE status='interrupted'"
                ),
                1
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM model_calls WHERE status IN ('queued','streaming')"
                ),
                0
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM participation_decisions WHERE status='pending'"
                ),
                0
            );
            let messages = core.room_messages(&room.id).unwrap();
            assert_eq!(messages[0].id, before["user"].as_str().unwrap());
            let generation = Server::handling(move |_| generated(api));
            connection(&core, persona, &generation.url, api);
            if phase == "classifying" {
                assert_eq!(
                    core.participation(&room.id).unwrap()[0].status,
                    "interrupted"
                );
                assert_eq!(
                    messages.len(),
                    1,
                    "Classifier must not leave a blank assistant bubble"
                );
                let (post, rx) = submit(
                    &core,
                    &room.id,
                    "Explicit manual recovery",
                    std::slice::from_ref(persona),
                );
                classifier_support::finish(&post, &rx);
            } else {
                let interrupted = messages.last().unwrap();
                assert_eq!(interrupted.status, MessageStatus::Interrupted);
                assert!(interrupted.can_retry);
                let (_retry, rx) = live_support::retry(&core, persona, &interrupted.id);
                assert_eq!(
                    live_support::terminal(&rx).last().unwrap().kind,
                    ChatEventKind::Completed
                );
                assert_eq!(
                    core.room_messages(&room.id)
                        .unwrap()
                        .iter()
                        .filter(|m| m.sender_kind == "user")
                        .count(),
                    1
                );
            }
            assert_eq!(
                count(&db, "SELECT COUNT(*) FROM persona_events WHERE kind='sent'"),
                2
            );
            assert_eq!(
                count(
                    &db,
                    "SELECT COUNT(*) FROM group_rounds WHERE status IN ('active','superseded')"
                ),
                0
            );
            assert!(
                !db.prepare("PRAGMA foreign_key_check")
                    .unwrap()
                    .exists([])
                    .unwrap()
            );
        }
    }
}

#[test]
fn membership_crash_fixture() {
    let Some(root) = std::env::var_os("PEBBLE_MEMBERSHIP_CRASH_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let api = std::env::var("PEBBLE_MEMBERSHIP_CRASH_API").unwrap();
    let phase = std::env::var("PEBBLE_MEMBERSHIP_CRASH_PHASE").unwrap();
    let core = pebble_core::Core::open(root).unwrap();
    std::fs::write(
        root.join("config.toml"),
        "# Isolated recovery fixture; no default network classifier.\n",
    )
    .unwrap();
    let persona = core.workspace().unwrap().personas[0].id.clone();
    let protocol = api.clone();
    let requests = AtomicUsize::new(0);
    let generation = Server::handling(move |_| {
        generated(&protocol).delayed(if requests.fetch_add(1, Ordering::SeqCst) == 0 {
            0
        } else {
            2000
        })
    });
    connection(&core, &persona, &generation.url, &api);
    let (_accepted, rx) = live_support::start(&core, &persona, "Accepted before interruption");
    live_support::terminal(&rx);
    generation
        .requests
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    let room = core
        .create_group("Durable members", std::slice::from_ref(&persona))
        .unwrap();
    core.remove_group_member(&room.id, &persona).unwrap();
    let room = core.add_group_member(&room.id, &persona).unwrap();
    let protocol = api.clone();
    let classifier = Server::handling(move |_| {
        Reply::json(answer(&protocol, "{\"respond\":true}")).delayed(2000)
    });
    if phase == "classifying" {
        configure(&core, None, &classifier.url, &api, "");
    }
    let _private = if phase == "queued" {
        let pair = live_support::start(&core, &persona, "Unfinished private predecessor");
        generation
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Some(pair)
    } else {
        None
    };
    let picked = if phase == "classifying" {
        vec![]
    } else {
        vec![persona.clone()]
    };
    let (post, _rx) = submit(&core, &room.id, "Unfinished original user", &picked);
    if phase == "classifying" {
        classifier
            .requests
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
    } else {
        let _task = task(&post, 0);
        if phase == "generating" {
            generation
                .requests
                .recv_timeout(Duration::from_secs(3))
                .unwrap();
        }
    }
    std::fs::write(root.join("before.json"),json!({"room":room.id,"membership":room.members[0].membership_id,"user":post.message.id,"native":native(root),"state":core.response_settings(&persona).unwrap().current_state_json,"calls":count(&db(root),"SELECT COUNT(*) FROM model_calls")}).to_string()).unwrap();
    std::process::exit(0);
}
