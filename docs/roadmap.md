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

## v0.0.6 review

v0.0.6 adds lossless per-item raw/zstd storage, an independent decoded context
memory bound, and inherited TOML context/output budgets. Requests retain exact
decoded native bytes and stable prefixes across restart. The documented input
estimate is conservative byte accounting, not exact provider token usage.
Oversized context fails before HTTP and preserves the visible accepted input.

An explicit fresh-context command preserves transcript and native history while
starting after a durable event boundary with current SOUL/MEMORY. It refuses
that persona's outstanding queued/active work without blocking other rooms.
Schema 1–4 migration to 5 preserves IDs and old raw bytes in the same eight tables.
Tests cover codecs, bounded corrupt-data handling, budget reserves, native replay,
manual boundaries, cancellation, and both protocols.

The next increment is v0.0.7: app/persona settings, safe credentials, retries,
interruption recovery, and daily usability. No automatic retry or history
summarization occurs.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
