# Roadmap

The first local private-chat milestone, **v0.1.0**, is implemented. The preceding
increments remain recorded below so the architecture can be inspected in stages.

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

## v0.1.0 private-chat baseline

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

v0.1.0 consolidates these stages without adding another subsystem. The validated
private-chat flow covers:

- Character creation, independent queues, room switching, scoped cancellation,
  and drafts retained while switching, through Rust and Swift bridge checks.
- Both live protocols: settings save, two turns across restart, native replay,
  cancellation, and explicit retry through the actual adapters using isolated
  synthetic characters. The selected llama-server passed both protocols.
- Configuration repair, credential preservation, manual SOUL/MEMORY edits,
  interrupted active/queued calls, and retry without duplicate user messages.
- Stable compressed native bytes, schema migration, context-full failure, and
  explicit fresh contexts with preserved transcript.

The native settings and failure/retry screens have been rendered and reviewed.
Composer checks cover growth, scrolling, marked text, Enter to send, and the
Command/Shift–Enter newline shortcuts. See README for the runnable checks and
local data paths; provider, history, context, and settings docs own their details.

No automatic retry, transcript clearing, or history summarization occurs. Drafts
do not survive app restart. Retry is limited to the latest unsuccessful reply,
settings saves are atomic per file, and context estimates are not exact provider
token usage. These limits are explicit parts of the first local milestone.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## After v0.1.0

Group chats and participation classifiers, explicit mentions, session
compaction, automated memory, Codex delegation, attachments, proactive
messages, cold archives, and cross-device sync remain deferred. Do not create
empty tables or dormant services for them during the private-chat milestones.
