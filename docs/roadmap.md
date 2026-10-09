# Roadmap

The first private-chat release is **v0.1.0**. Early milestones are intentionally
small enough to inspect and reconsider independently.

| Stage | Outcome |
| --- | --- |
| v0.0.1 | Native shell, static Swift–Rust FFI, editable persona files, two identity tables, simulated streaming/cancellation |
| v0.0.2 | Durable private-chat pipeline: messages, persona events, model sessions/items, event associations, and calls |
| v0.0.3 | One persona with a live Chat Completions connection, streaming, and stable request prefix |
| v0.0.4 | Multiple independent personas and sessions, per-persona queues and reliable room routing |
| v0.0.5 | Responses adapter and protocol-native replay fidelity |
| v0.0.6 | Per-item raw/zstd encoding, context budgets, and explicit fresh-context boundaries |
| v0.0.7 | Daily-use settings, retry/cancel, interruption recovery, and usability |
| v0.1.0 | Consolidated private-chat release |

## v0.0.2 review

v0.0.1 established the native shell and static bridge; v0.0.2 now runs the
private-chat pipeline with an explicitly labeled local demo. Send a message,
restart, and continue with the same visible message IDs and model session.
The demo references the preceding user message from stored request items.

Inspect the compact/growing composer, Enter to send, Command–Enter or
Shift–Enter for new lines, saved transcript, character panel, and cancellation.
The app still makes no model calls. Existing v0.0.1 identities migrate without
reseeding files. Cancelled/interrupted replies remain visible but are not
accepted as complete model outputs.

The next increment is v0.0.3: one real Chat Completions connection, streaming,
and provider-native request replay. Demo sessions use their own protocol and
must not be passed to a live provider unchanged. Per-persona concurrency,
Responses, compression/context limits, settings, and richer recovery retain
their later milestones. No automatic retry occurs in v0.0.2.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
