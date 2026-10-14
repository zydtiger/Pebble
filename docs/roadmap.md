# Roadmap

The local private + group + memory/context milestone, **v0.3.0**, is implemented. Earlier
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
Command/Shift–Enter newline shortcuts. See [development](development.md#validate) for runnable checks and
[local data](local-data.md) for storage paths; provider, history, context, and
settings docs own their details.

No automatic retry or transcript clearing occurs. Automatic threshold review and explicit memory consolidation are described below. Drafts
do not survive app restart. Retry is limited to the latest unsuccessful reply,
settings saves are atomic per file, and context estimates are not exact provider
token usage. These limits are explicit parts of the first local milestone.

The local app uses ad-hoc signing and Apple Silicon. Wider distribution and
minimum-OS testing remain future work.

## v0.1.1–v0.2.0 group-chat delivery

v0.1.1 through v0.2.0 are implemented. Each increment leaves a runnable app
and receives focused review before the next stage.

| Stage | User-visible result and exit boundary |
| --- | --- |
| **v0.1.1 — Structured replies and state** | Private replies validate `reply` + complete persona state, with typing until atomic acceptance, a frozen schema/semantic contract, bounded format repair, and explicit legacy-context upgrade. Both protocols preserve opaque native replay. |
| **v0.1.2 — State and schema controls** | Inspect current state, edit schema/initial values, and deliberately reset state. Schema changes require an explicit context boundary and compatible-state/reset decision; preserve old contracts. Initially affinity names the user as subject. |
| **v0.1.3 — Groups with manual responses** | Create/name groups of existing characters, switch rooms, and `@` stable member IDs to request replies. Membership intervals and durable public-message fan-out preserve private → group → private continuity; silent members observe. One session/state/queue belongs to each persona across rooms. Implemented with native creation/rename, typed/picked mentions, publication intervals, persona-wide retry, and populated private-data migration. |
| **v0.1.4 — Selective participation** | App-default/per-persona classifier connections decide separately on user messages. Supply persona-scoped room/state/memory context; `@` bypasses inference. Classifier reads change neither chat history coverage nor state. Failures remain visible and manual mentions usable. Implemented with independent native settings, strict two-protocol validation, durable single-admission decisions and dynamic speaker observers; bounded bot triggering follows in v0.1.5. |
| **v0.1.5 — Group conversation flow** | Order room replies alongside global persona queues. Each generation sees accepted predecessors/current state; reassess optional slots, protect mentions, bound automatic reply chains, and allow one fallback for an explicit whole-group question when nobody volunteers. Fan-out contains only visible `reply`. Implemented with durable ready-only slots, deterministic required/optional order, queued reassessment, N+2 budget, two-pass fallback, all-room Stop and selective predecessor coverage. |
| **v0.1.6 — Reliable daily group use** | Add/remove/rejoin members; room-scoped Stop/Cancel/Retry, per-speaker pending/errors, drafts, and restart recovery. No duplicate acceptance/fan-out/state change or silent chain resume. Retry eligibility spans the persona's whole cross-room history. Implemented with native member management, future publication intervals, old-interval cancellation/retry exclusion, edit-revision draft preservation and both-protocol crash recovery. |
| **v0.2.0 — Group-chat milestone** | Consolidate private + group chat, persistent personas, selective participation, state lifetime, membership visibility, scheduling limits, and supported provider behavior. Implemented without a new subsystem, with the five completion scenarios below, frozen-label live integration accounting and current native renders. |

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

The completion checks map to maintained runnable fixtures. Ordinary tests use
isolated local HTTP servers; opt-in service checks use synthetic temporary roots.
The selected live integration and its limits are described in
[participation](participation.md#validation-and-measured-limits).

| Completion scenario | Passing behavior evidence |
| --- | --- |
| Private knowledge reaches its owner's group context, without private/native fanout | [group_history.rs](../crates/pebble-core/tests/group_history.rs) checks both protocols' unchanged native prefix/current state, public-only observations for the other member, opaque reasoning isolation and nonmember exclusion. [group_live_endpoint.rs](../crates/pebble-core/tests/group_live_endpoint.rs) separately exercises both real chat protocols. |
| Silent Kai is later `@Kai` in the group, without reinjecting own output | `silent_member_later_mentioned_in_group_keeps_own_native_outputs_without_reinjection_both_apis` in [group_history.rs](../crates/pebble-core/tests/group_history.rs) checks observed public messages once, exact earlier prefix, accepted state and native-only own replies. |
| One persona shares private + two groups, retaining state, immutable destinations and isolated cancellation | [group_acceptance.rs](../crates/pebble-core/tests/group_acceptance.rs), paired causal-prefix/coverage-hole cases in [group_rounds.rs](../crates/pebble-core/tests/group_rounds.rs), and both-protocol removal races in [group_membership.rs](../crates/pebble-core/tests/group_membership.rs) verify these boundaries. |
| Mentions, silence, bounded followups, fallback and newer input cooperate | [group_flow.rs](../crates/pebble-core/tests/group_flow.rs) checks deterministic required/optional ordering, N+2 admission, bot mentions, two-pass fallback and interruption. [group_fallback_request.rs](../crates/pebble-core/tests/group_fallback_request.rs) freezes acknowledgment purpose across repair/retry; [group_queued_cancel.rs](../crates/pebble-core/tests/group_queued_cancel.rs) rejects late reassessment callbacks. |
| Membership/failure/cancel/restart preserve accepted results without backfill or resend | [group_membership.rs](../crates/pebble-core/tests/group_membership.rs), [group_membership_recovery.rs](../crates/pebble-core/tests/group_membership_recovery.rs) and [group_recovery.rs](../crates/pebble-core/tests/group_recovery.rs) cover future publication boundaries, old interval rejection, both-protocol process interruption, immutable native bytes/state, explicit retry and transactional rollback. |

Native [bridge checks](../macos/Tests/BridgeChecks.swift) and
[render checks](../macos/Tests/RenderChecks.swift) exercise group creation,
member controls, typed/chosen mentions, speaker attribution, participation,
private navigation, recovery and per-room drafts. Composer checks retain native
marked text, Enter send and Command/Shift–Enter newline behavior.

After v0.3.0, proactive conversations outside a user-started
group round, separate roleplay worlds, richer relationships, search/cold
archives/exports/deletion, sync, notarization, and broader platform validation
remain separately scoped. The local app remains ad-hoc signed for Apple Silicon;
no remote release, distribution signing or notarization is provided.

## v0.2.1–v0.3.0 memory/context delivery

| Stage | Completed outcome |
| --- | --- |
| v0.2.1 | Persona-scoped native update_memory, current-memory input snapshots, durable effect journal/reconciliation, native replay and explicit partial-outcome recovery for both protocols |
| v0.2.2 | Same-chat-model asynchronous explicit consolidation, frozen cutoff, unchanged state, silent-group observation support, bounded headroom and saved-effect publication retry |
| v0.2.3 | Durable private/group persona boundary line/diamond, native progress/Cancel and persisted restart recovery |
| v0.3.0 | Schema 11 upgrade, core/FFI/Swift integration, synthetic durability/provider fixtures, live selected-service tool and maintenance integration, docs and local build version |

The targeted [memory/context fixtures](../crates/pebble-core/tests/memory_context.rs)
cover tool effect survival, frozen retry, manual conflicts, unknown/malformed
arguments, silent group knowledge, newer observations, membership removal,
publication failure/restart recovery and cancellation. Core journal tests cover
prepared/write/result restart stages, duplicate call IDs and unchanged-memory
no-op. Budget tests verify exact initial/maintenance boundaries. The local demo
explicitly preserves memory without claiming model-backed consolidation.
This historical memory milestone did not include a Codex adapter, delegated task
tables, work tools, retrieval or automatic rollover.

## Next development stages

Conversation request parameters, image input and the delegation implementation
are described below. The existing delegation version labels describe runnable
stage boundaries; no new release version is assigned until integrated validation
and independent review complete.

### Conversation improvements before delegation

1. **Direct model request parameters — implemented.** Concrete app/persona
   `request` tables serve chat and classification through one native JSON editor,
   preserving nested types, whole-connection inheritance and omission/false/zero.
   Rust rejects protected field collisions and known conflicts, composes before
   final budgeting, and freezes effective parameters through repair, tools and
   consolidation. Explicit Retry uses current settings with compatible input.
   Sampling and per-request `enable_thinking` preserve active contexts; other
   template changes participate in the durable connection boundary. Settings,
   protocol/dialect, classifier phase, repair and memory tests cover the contract;
   selected endpoint thinking behavior is measured separately in
   [provider contracts](providers.md#request-parameters-and-thinking-support).
2. **Image input and durable attachments — implemented, version unassigned.**
   Native pick/paste/drop, removable room drafts, image-only/private/group sends,
   transcript viewing and enlarged images use Rust-owned immutable JPEG/PNG
   assets. Typed frozen rendition references preserve real pixels through chat,
   every classifier phase, Retry/restart and explicit memory consolidation.
   Schema v12 is additive; old text/native output bytes remain intact. Public
   visibility and future-only membership follow existing delivered events;
   fresh contexts exclude old images after visual consolidation. Separate vision
   reserves, original/decode/model limits and bounded transport are documented
   with the selected-service evidence and test entry points in
   [image input](images.md). Image generation and general delegation are excluded.

These are useful independent increments while delegation details are discussed.
Neither requires exposing work tools to personas or launching Codex tasks.

### v0.3.1–v0.4.0 delegation implementation

| Stage | Runnable outcome | Completion boundary |
| --- | --- | --- |
| v0.3.1 — Local connection | Rust adapter for a local Codex app-server child process, capability checks and separate app settings | Verify installed protocol, authentication, available tools and one bounded read-only turn; missing CLI/login and disconnects are actionable. Desktop capabilities are not assumed. |
| v0.3.2 — Task manager | Durable task cards, progress, cancellation and startup reconciliation, initially launched explicitly for read-only work | Persist identity before dispatch; distinguish uncertain execution from failure, retain actual results, and never silently resubmit on restart. Chat remains usable. |
| v0.3.3 — Private delegation | Persona `delegate_task` returns promptly; Rust delivers a stored completion through the persona queue to the original private chat | Native tool/reply contracts remain valid. Chat and fresh contexts can continue during work; retries cannot repeat execution or completion delivery. |
| v0.3.4 — Interactive work | Task questions, required approvals and follow-ups resume the same task; add authorized file/code work in each managed task workspace | Preserve execution scope and actual effects on cancellation; validate a scoped code task and its artifact, including interrupted question/approval flows. |
| v0.3.5 — Group delegation | One reporting owner for each assigned shared task, with original-room delivery and current-membership checks | Avoid duplicate execution without collapsing deliberately distinct tasks; keep private source material private and late completion reports bounded. |
| v0.4.0 — Delegation milestone | Consolidate the private/group task experience and documentation | Exercise research and scoped code work, continued chat, artifacts, cancellation, restart and context changes; add no new subsystem here. |

Rust owns dispatch, event subscriptions, reconciliation and any necessary polling;
chat models only formulate requests and explain outcomes. Pebble owns task IDs
and origin bindings, while Codex thread/turn IDs identify execution. A result
reply uses the current persona context and its original destination. If that
destination is no longer eligible, retain the result in its card for explicit
routing. Retrying the explanation must not repeat completed work.

The implementation uses per-task directories in application data with writes
enabled by default and Approve for me automatic permission review. It supports
explicit read-only execution, defers native questions and declines any manual
approval requests reaching Pebble. It manages children while Pebble is open. Restart
reconciles actual outcomes without resending uncertain work. Shared ownership
is frozen per canonical user message; the Mention menu can explicitly partition
separate assignments among selected characters. See [delegation](delegation.md)
for the complete contract, deterministic tests, local evidence and the remaining
Calendar launch-context permission limitation. Independent audit and final
integration gates remain necessary before treating these stage labels as a release.

### Image generation and later work

**Persona image generation** is implemented as a version-unassigned increment.
G1–G6 provide strict native arguments and independent backend settings, durable
execution/publication, direct Studio HTTP, typed local Codex ChatGPT images,
actual-pixel persona delivery and original-room atomic publication. Generation
runs in the background; Stop cancels unpublished room work. Saved assets survive
reply failure, and saved remote identities only permit read-only reconciliation.
Studio sizes are strict; ChatGPT preserves actual output and explains size
mismatches. Targeted fixtures cover both protocols, queue release, failure/retry,
uncertain submission, group fanout, membership and cancellation. See
[image generation](image-generation.md) for the contract and verification entry
points. Release placement and signed distribution remain separate decisions.

### Automatic memory and native recall

The version-unassigned automatic-memory increment adds same-chat-model rollover
at soft/priority context thresholds, exact inherited event coverage, durable
unchanged-source attempt suppression, and quiet success without transcript dividers.
Automatic successors retain up to ten already-consumed visible messages through
frozen ID references, with whole-message budget reduction; explicit fresh contexts
keep their clean-prefix behavior.
Rust-owned search_history/read_history offer literal content/local-date filtering
and visible neighborhoods within a frozen persona receipt scope. Bounded native
search/read/memory/final flows replay on both protocols. Targeted tests cover queue
ownership, unseen private/group holes, restart/partial review recovery, visibility,
Unicode, local DST dates, bounds and native evidence. See [context](context.md),
[memory](memory.md), and [history tools](history-tools.md).

Richer retrieval, attachment deletion, sync and broader distribution remain
outside these increments.
