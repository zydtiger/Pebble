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

## v0.1.1–v0.2.0 group-chat delivery

v0.1.1 through v0.1.6 are implemented. v0.2.0 remains planned; each leaves a runnable
increment and receives focused review before the next stage.

| Stage | User-visible result and exit boundary |
| --- | --- |
| **v0.1.1 — Structured replies and state** | Private replies validate `reply` + complete persona state, with typing until atomic acceptance, a frozen schema/semantic contract, bounded format repair, and explicit legacy-context upgrade. Both protocols preserve opaque native replay. |
| **v0.1.2 — State and schema controls** | Inspect current state, edit schema/initial values, and deliberately reset state. Schema changes require an explicit context boundary and compatible-state/reset decision; preserve old contracts. Initially affinity names the user as subject. |
| **v0.1.3 — Groups with manual responses** | Create/name groups of existing characters, switch rooms, and `@` stable member IDs to request replies. Membership intervals and durable public-message fan-out preserve private → group → private continuity; silent members observe. One session/state/queue belongs to each persona across rooms. Implemented with native creation/rename, typed/picked mentions, publication intervals, persona-wide retry, and populated private-data migration. |
| **v0.1.4 — Selective participation** | App-default/per-persona classifier connections decide separately on user messages. Supply persona-scoped room/state/memory context; `@` bypasses inference. Classifier reads change neither chat history coverage nor state. Failures remain visible and manual mentions usable. Implemented with independent native settings, strict two-protocol validation, durable single-admission decisions and dynamic speaker observers; bounded bot triggering follows in v0.1.5. |
| **v0.1.5 — Group conversation flow** | Order room replies alongside global persona queues. Each generation sees accepted predecessors/current state; reassess optional slots, protect mentions, bound automatic reply chains, and allow one fallback for an explicit whole-group question when nobody volunteers. Fan-out contains only visible `reply`. Implemented with durable ready-only slots, deterministic required/optional order, queued reassessment, N+2 budget, two-pass fallback, all-room Stop and selective predecessor coverage. |
| **v0.1.6 — Reliable daily group use** | Add/remove/rejoin members; room-scoped Stop/Cancel/Retry, per-speaker pending/errors, drafts, and restart recovery. No duplicate acceptance/fan-out/state change or silent chain resume. Retry eligibility spans the persona's whole cross-room history. Implemented with native member management, future publication intervals, old-interval cancellation/retry exclusion, edit-revision draft preservation and both-protocol crash recovery. |
| **v0.2.0 — Group-chat milestone** | Consolidate private + group chat, persistent personas, selective participation, state lifetime, membership visibility, scheduling limits, and supported provider behavior; no new subsystem at this final step. |

### Group boundaries and completion scenarios

Joining/rejoining observes future public messages only; leaving stops later
observations without erasing prior knowledge. Silent members receive eligible
public events without a model call or state mutation. Nonmembers receive no
copy, and one persona's private exchange is never fanned out to another.
A character may use its own private knowledge publicly; disclosure instructions
cannot guarantee that a shared-context model will never reveal private facts.
Mentions guarantee scheduling while still honoring validation, cancellation,
membership, and provider errors. Bot-to-bot triggers begin only in v0.1.5 and
remain finite and stoppable.

Completion must demonstrate:

1. Mira uses a private fact in her group context while Kai receives no copy of
   Mira's private exchange.
2. A silent Kai later sees the public messages he observed, without duplicating
   his prior native outputs.
3. One persona shared by private chat and two groups serializes calls/state,
   with every reply returning to its immutable originating room.
4. Mentions, selective silence, whole-group fallback, bounded character chains,
   and interruption by a new user message work together.
5. Membership changes, failure/cancel, and restart produce no backfill,
   duplicate acceptance, private-state fan-out, or silent automatic resend.

After v0.2.0: automatic memory extraction/retrieval/writing, session compaction,
Codex delegation, attachments, proactive conversations outside a user-started
group round, separate roleplay worlds, richer relationships, search/cold
archives/exports/deletion, sync, notarization, and broader platform validation
remain separately scoped. No classifier or group calls are part of v0.1.1.
