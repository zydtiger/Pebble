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

## v0.0.3 review

v0.0.3 streams one real OpenAI-compatible Chat Completions connection. Native
assistant output is saved once and replayed after restart with a frozen earlier
prefix. Only final content is visible; separate provider reasoning stays in
model history. Cancellation interrupts HTTP and slow streams. Failed, partial,
and interrupted replies remain explicit and never become accepted output.

Without configuration, the deterministic local demo remains available. A
configured connection calls the model or fails; Responses reports that it is
not implemented. Changing the connection starts a fresh context, preserving old
transcript and sessions with a visible notice. Schema 1/2 data upgrades to 3
without reseeding character files.

The next increment is v0.0.4: multiple independent personas, queues, and reliable
room routing. Responses remains v0.0.5; compression/context budgets and general
context reset remain v0.0.6; settings, retry, and richer recovery remain v0.0.7.
No automatic retry occurs in v0.0.3.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
