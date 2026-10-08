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

## v0.0.1 review

Open the app and inspect the visual direction, text input, scrolling, character
panel, and cancellation. The preview is scripted and temporary. The stage
demonstrates that Rust can own persistence and stream events into a thin native
frontend; it is not an implemented chat-history engine.

The project currently uses one starter persona, local ad-hoc signing, and
Apple Silicon. Wider distribution and minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
