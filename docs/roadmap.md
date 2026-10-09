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

## v0.0.5 review

v0.0.5 supports Chat Completions and Responses with independent persona queues.
Responses stores the complete accepted reasoning/message item array, including
opaque encrypted state and IDs, and replays it with the stable frozen prefix
after restart. Streaming reasoning remains hidden. Failed, incomplete,
reasoning-only, inconsistent, truncated, or unsupported tool output is never
accepted as model continuation; visible partial text remains durable.

Protocol changes start a clearly marked fresh context while preserving earlier
room history. The eight-table schema remains at version 4. Mock tests exercise
native replay, queued followups, atomic multi-item publication, protocol
boundaries, fragmented streams, and cancellation. Opt-in synthetic live tests
cover both protocols across two turns and restart.

The next increment is v0.0.6: per-item raw/zstd encoding, context budgets, and
explicit context reset. Settings, retry, and richer recovery remain v0.0.7.
No automatic retry occurs.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
