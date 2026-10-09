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

## v0.0.7 review

v0.0.7 provides native app-default and character settings, safe explicit
credential updates, and latest-reply retry/recovery. TOML and Markdown remain
authoritative; Rust validates candidates and inherited budgets before per-file
atomic replacement. SOUL/MEMORY changes preserve existing frozen prefixes.

Failed, cancelled, and interrupted attempts retain partial text and durable
errors. Explicit retry reuses the user/receipt and frozen input when compatible;
new contexts select only the requested input. Queued interruption and retry
chains are covered without duplicate users, native outputs, or altered old calls.
Schema 1–5 upgrades to 6 in the same eight tables. Both real protocol probes use
synthetic isolated roots and exercise settings save, restart/replay, cancel/retry.

The next increment is v0.1.0 consolidation of the completed private-chat stages.
No automatic retry, transcript clearing, or history summarization occurs.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
