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

## v0.0.4 review

v0.0.4 supports multiple independent characters and private rooms. Create a
character with separate editable files, switch rooms during simultaneous replies,
and keep each room's draft/stream/Stop state. Same-persona followups queue and
freeze only after earlier calls finish, preserving accepted native output and
excluding future queued inputs from earlier requests.

Queued cancellation is explicit and does not create model output or silently
consume cancelled input. Closing the app cancels retained work; abrupt restart
marks both queued and active calls interrupted. Schema 1–3 data upgrades to 4
while preserving IDs, frozen history, and the eight-table architecture.

The next increment is v0.0.5: Responses with native output and opaque replay
fidelity. Compression/context budgets and explicit context reset remain v0.0.6;
settings, retry, and richer recovery remain v0.0.7. No automatic retry occurs.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
