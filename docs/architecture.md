# Architecture

v0.3.0 adds bounded persona memory tools and explicit memory consolidation to the reviewed private + group lifecycle. [Group scheduling](group-flow.md), [membership](groups.md),
[state lifetime](state.md) and [provider contracts](providers.md) define the
current boundaries. [Image generation](image-generation.md) adds background
backend execution, durable assets, single-source editing and original-room persona publication. The version-unassigned [Codex delegation](delegation.md) increment
adds Rust-owned local execution and durable reporting; the [roadmap](roadmap.md) maps completion scenarios to tests.

## Boundaries

`pebble-core` is an ordinary Rust library. It owns TOML parsing and validation,
SOUL/MEMORY loading, stable persona/private/group-room IDs, SQLite, frozen model history, and the
live/demo worker's lifecycle. It can be tested without a UI or language bridge.

`pebble-ffi` wraps the core with UniFFI 0.32.2 records, typed errors, objects, and
a foreign-trait observer. It builds one static library. Bindings are generated
directly from that archive; no Rust dynamic library or local HTTP service is
needed. Generated Swift and C declarations always come from the current core.

SwiftUI owns navigation, text input, view state, and rendering. `CoreClient` is
an actor that keeps blocking configuration/database operations off MainActor.
The generated types conform to Sendable with the selected Swift 6 compiler.
No asynchronous Rust function is exported yet: short commands plus a callback
stream meet this stage's needs. Each live worker owns a current-thread Tokio runtime for cancellable HTTP/SSE
I/O; no network wait blocks MainActor or a database transaction.

The host resolves the data directory and performs native actions such as
opening Finder. The core receives the path; it does not hard-code a Mac home
directory. This keeps product logic reusable with another frontend.

Swift owns local macOS reply notifications through UserNotifications. The first
send requests alert/sound authorization without blocking chat. Completed persona
events and task-snapshot room refreshes share session-only message-ID deduplication;
initial room histories establish the baseline, including unopened rooms. Only
nonempty accepted persona replies qualify. Chat-window focus (including attached
sheets) suppresses delivery, with another focus check before native presentation.
The notification delegate is installed before launch finishes, retains room
navigation until workspace loading completes, and restores the main window on
click. macOS preferences and Focus govern banners and sound; no background
service or remote push registration is involved.

The message composer embeds an AppKit `NSTextView` in an `NSScrollView`.
TextKit measures the actual wrapped text, including a trailing empty line,
to grow the editor from 33 to 160 points. Scrolling becomes available only
when the text exceeds that height, and clearing the draft restores the compact
size. The placeholder uses the same font and text-container origin as input.
Native marked text and selection stay with AppKit. Enter sends the draft;
Command–Enter and Shift–Enter insert new lines. While an input method has marked
text, Enter stays with AppKit to confirm composition without sending. The surrounding
SwiftUI layout keeps that button at the lower right and lets the introduction
scroll when an expanded composer reduces its available space.

Reply bubbles use Foundation's inline Markdown parser for emphasis and AppKit
TextKit for selectable, wrapping prose. SwiftMath 1.7.3 renders `\(...\)` inline
math as native text attachments with ascent/descent-aware baselines. The same
renderer uses display style for `\[...\]` equation blocks, which scroll
horizontally when wider than the bubble. Inline formulas wrap as whole units;
a formula wider than an entire line scales to fit that line. Copying selected
formulas returns their original delimited LaTeX source. Unsupported or unfinished
formulas remain literal source text. Math detection precedes Markdown parsing
and excludes fenced code and inline code spans.

Code blocks keep literal whitespace in a horizontally scrolling, unhighlighted
monospaced view. User messages and status/error text remain plain text. This is
presentation only: stored messages, model history and notification text retain
the original response string. SwiftMath and its math fonts ship inside the app;
rendering needs no WebView, network access or external TeX installation.

## Durable storage and generation

Rust owns chat/history/state/membership/decision/round/memory/context/image/task records and schema migration. Visible
messages and frozen model items are distinct records; the UI never reconstructs
model context from bubbles. See [durable history](history.md) for table ownership,
ordering, request reconstruction, atomic writes, and interrupted-call behavior.

```text
Swift sends room, text, immutable imported image IDs, explicit mentions, and observer
    → Rust accepts one user message, member receipts, group round/decisions/slots atomically
    → room coordinator admits only its ready speaker into the persona queue
    → persona queue activates the call and freezes its request atomically
    → Rust returns a cancellable handle and authoritative message IDs
    → Chat Completions, Responses, or unconfigured demo consumes the stored request
    → Rust buffers candidate JSON privately and validates complete reply/state
    → Rust retries only format/schema failures from the same frozen request (at most twice)
    → Rust atomically publishes reply/state/native output/event links and terminal state
    → Swift reloads the saved transcript
```

Each event carries call, persona, conversation, message, and event-sequence
IDs. Swift only renders matching output, reconciles authoritative snapshots,
and retains an unaccepted draft on error. MainActor never opens SQLite. The
actor bridge keeps blocking core calls off the UI thread, and listeners only
enqueue events. A bounded, disconnected subscription cancels generation.

Each persona has one ordered execution queue; different personas can generate
concurrently. The native send path uses `send_room_assigned_images` to steer:
accepting another user message atomically cancels unfinished ordinary replies and
rounds in that room, saves every user receipt, and starts replacement work with
the latest cutoff. Pending required group speakers and admitted speakers carry
forward. Rejected input cannot interrupt the earlier reply. Other rooms, background
task/image execution and their independent result-delivery calls retain their lifecycles.

Schema 15 records `steered_calls` links from obsolete calls to the newer user
receipt. Obsolete bubbles are excluded from transcript snapshots, including after
restart; their frozen requests and committed tool effects remain durable. Even
unactivated superseded inputs remain eligible for the replacement context. Old
workers are cancelled after the acceptance transaction and cannot publish late
results. Swift ignores obsolete stream updates and places unfinished replies
below all user messages, without a queued-message label or cancellation control.

The lower-level enqueue APIs still serve independent persona scheduling and
explicit retry. Activation freezes each request after the predecessor settles,
using its receipt cutoff and actual accepted native history. A group placeholder
is created only when its room slot is admitted. These internal reservations do
not create a separate reply for each rapid native send.

Swift keeps a presentation cache, draft, mention selection, and run handles per room. Room switching
changes the displayed cache without disconnecting other streams. Drafts, output,
and errors belong to that room. There is no conversation-wide Stop control. Closing the view
cancels every retained handle/subscription; dropping the core cancels its queues and participation workers.
Early cancellation is retained until handles arrive. A group observer adopts dynamically admitted speaker handles and remains open until every participation decision and selected call terminates; one character finishing cannot disconnect another. Startup marks both queued
and active calls interrupted, with no accepted partial output or hidden resubmit.

Rust character creation atomically publishes a UUID directory containing
config.toml, SOUL.md, MEMORY.md, and response_schema.json. It inherits the app's default connection,
creates a stable private room, and exposes a minimal name form in Swift. Native settings edits authoritative TOML/Markdown through Rust; deletion remains
outside this milestone.

An optional `avatar.png` beside those character files supplies the profile picture.
Swift decodes a thumbnail off MainActor from each Rust-provided persona directory
on workspace load/reload and keeps one presentation image per stable persona ID.
The native view crops it to a circle; missing/unreadable files use the existing
placeholder. Replacing/removing a picture is reflected by explicit file reload,
without altering TOML, SQLite, persona state or frozen model context.
The sidebar combines private and group rooms in one list, ordered by the newest
durable message creation timestamp. Empty rooms follow rooms with messages; ties
preserve workspace order. Swift derives this ordering from existing history and
message updates, including background reports, and rebuilds it on launch.
Opening a room or changing its settings does not move it to the top.
Group avatars compose up to seven current members’ circular portraits in a stable,
compact staggered layout, shared by the sidebar, header and empty conversation.
Missing portraits keep the character placeholder; empty groups use a group symbol.
Group chats omit the participation status panel and settings shortcuts; reply
progress remains beside the corresponding message. Participation decisions and
room scheduling are unchanged.
Header avatars and image attachments share an independent AppKit preview window.
Swift owns its lifetime, fit/zoom/pan presentation and temporary floating-window
pin state. NSScrollView handles trackpad magnification; opening a preview never
reserves a persona queue or blocks the chat window.

The demo protocol is distinct from Chat Completions and Responses. Configured
connections always run their real HTTP adapter.
An absent connection runs the local demo. Rust supplies mode/model labels and
fresh-context notices to the typed frontend records.

HTTP uses the configured API root plus `/chat/completions` or `/responses`, optional runtime
credentials, environment proxy settings, and no redirects. A cancellable future
covers connection, headers, and idle stream waits. Connection timeout is 10
seconds; headers and stream idle timeout are 45 seconds; the entire request has
a 180-second deadline. Requests default to a 4096-token output reserve, configurable with the context window. Credentials
never enter model history or call records, and provider bodies/URLs are not
copied into error diagnostics.

SSE is framed as bytes before decoding complete UTF-8 lines. Chat Completions
buffers `content` deltas privately and requires a successful `stop` finish plus
`[DONE]` and nonempty visible text. Native assistant content, `reasoning_content`,
and `refusal` are retained for replay.

Responses buffers output text and refusal deltas privately. A completed-status
`response.completed` event supplies authoritative native output in array order;
`[DONE]` is unnecessary. Whole accepted reasoning/message items, including IDs,
encrypted state, and unknown fields, are persisted once and replayed unchanged
semantically. Earlier added reasoning may be incomplete and is never used as
accepted output. Final item events and final visible text must agree with the
stream. Both indexed events and item-ID events with an unambiguous content-part
lifecycle are supported. See [provider contracts](providers.md).

Reasoning never appears in the UI. Unsupported tools, errors, incomplete terminal
states, truncation, text mismatch, refusal, and exhausted budgets fail without
publishing structured candidate text, state, or native output. Only JSON/schema
failures permit bounded automatic format repair. Preserved v0.1.0 sessions keep
their frozen plaintext behavior until an explicit context boundary. See [state
and upgrade rules](state.md).

Sessions bind protocol, model, credential-free API root and history-formatting
template parameters. The existing durable binding includes those template values;
sampling and the verified per-request `enable_thinking` switch are excluded. A changed binding
retires the previous context and starts after the current event frontier. Old
transcript and model items remain intact; character information explains the fresh
context. SOUL/MEMORY stays frozen inside an unchanged session after file reload.
Explicit fresh context is a cancellable Rust maintenance worker reserved in the
same persona queue. It freezes an observation cutoff, validates the complete
outgoing source, reviews it with current MEMORY using the same chat model, and
publishes a new prefix/unchanged state/marker only after durable tool completion.
Old transcript/native history stays intact. Other personas continue. Swift
presents confirmation, progress, cancellation and durable recovery status; it
never chooses paths, applies memory effects or reconstructs maintenance inputs.
See [memory](memory.md) for partial effects and restart reconciliation.

The request loader scans codec/size metadata before reading any payloads. Raw or
zstd items decode into the exact saved JSON bytes under a fixed 8 MiB context
bound; zstd window and output reads have independent bounds. A conservative
byte-based input estimate plus configured output reserve is checked before the
adapter starts. Budget edits alone leave the session and prefix unchanged.
See [context and encoding](context.md). There is no always-running background service or threshold-triggered rollover. Persona memory tools and explicit consolidation are defined in [memory](memory.md).

## Delegation boundary

Rust owns the stdio app-server child, capability discovery, scoped execution,
server requests, task IDs and Codex thread/turn mappings. Schema 13 stores task
snapshots, native submission identities/results, frozen group assignments and
historical runs. The native `delegate_task` transaction records the task and
replayable tool response before dispatch. A uniquely identified call with invalid
arguments receives up to two model corrections through matched native error
results. Balanced call/result pairs use existing model history; no task is
created before valid arguments. This shares the operation deadline and context
bounds while preserving the final response validation budget. Restart can safely give accepted,
never-submitted work its first dispatch with its saved identity and frozen scope.
Explicitly cancelled and terminal failed tasks are excluded. Already-submitted
or uncertain execution only reconciles saved outcomes and is never resent
automatically. App-level `[codex]` inherits local login/model/effort unless
explicitly overridden, independently of chat and classifier connections.
New tasks always use their own `data/codex-workspaces/<task-id>` directory
inside application data, with writes enabled by default. App settings expose no
external workspace selection. Frozen task snapshots retain their saved paths
and explicit read-only settings for restart and followups, independently of
editable app settings. Thread start/resume always select on-request approval with Codex's
`auto_review` reviewer (Approve for me). Codex owns eligible approval decisions;
review notifications keep execution active and save bounded denial/timeout
reasons in existing task evidence. Only an actual turn outcome ends execution.
Unsupported policy configuration fails visibly without a permission fallback.

Task execution runs outside the persona queue; one stored completion later
joins that original persona queue with current context/state. Rust verifies
origin visibility and the original membership interval before public publication.
Reporting and execution have separate durable statuses, so retrying an
explanation cannot rerun work. Group task ownership comes from the canonical
user message and explicit saved mention partitions, not model-generated keys.
Native questions are deferred without an invented answer; manual permission and
connected-tool requests reaching Pebble are declined. Rust interrupts the turn,
waits for its actual outcome, and includes the question or configuration limitation in the
durable persona report. A normal user reply can continue the same saved task
through `delegate_task(task_id)`. Uncertain work is never blindly resumed.

UniFFI exposes typed task snapshots, subscriptions and short commands. Swift
presents an app-wide circular task monitor fixed at the transcript’s top-right,
below the persona/group header, plus separate local settings. Hovering opens its
dropdown; a shared hover region spans the indicator, gap and dropdown, keeping it
open until the pointer leaves. Keyboard/accessibility activation can also open it,
and Escape dismisses it. The dropdown shows
brief task descriptions, owners and actual execution status only while tasks are
running or awaiting cancellation confirmation. The button and rows show progress;
queued, blocked and terminal tasks are hidden. Failures are explained through chat.
It has no task controls or
transcript cards. The first public Codex commentary supplies a bounded description
for each run, with the persona title retained until it arrives. Ordinary latest
failed-reply Retry can requeue a saved outcome explanation with tools disabled;
it cannot re-execute Codex. Swift never reconstructs task history, executes work
or polls the persona.
Closing Pebble stops its managed children; restart reconciles
saved threads rather than promising execution after quit. See
[delegation lifecycle, scope and measured limits](delegation.md).

## Image boundary

Rust owns immutable assets, content validation, normalization, metadata,
message ordering, visibility, frozen references, request materialization and
image/transport budgets. Swift owns the native picker, paste/drop conversion,
removable room drafts and image presentation; UniFFI carries typed records.
Schema v12 adds two tables without rewriting old native items. Schema v14 adds
independent image-generation operations and native submission records, with frozen
settings/remote identities and separate execution/publication status. Single-source
`edit_image` shares this lifecycle with `generate_image`, freezes a published
current-room original asset visible in the persona's frozen input, and publishes
one new immutable asset. Studio uploads that source once before edit submission;
ChatGPT stages it as typed localImage input. Recovery never reuploads or
redispatches edits; legacy operation records default to generation. A bounded
background worker releases the persona queue, then supplies actual generated
pixels to an original-room report in that queue. Validated text/state, attachment
and group events publish atomically. Lifecycle cancellation invalidates unpublished work and
requests backend cancellation. Startup reconciles saved remote identities with
read-only requests, never replacement generation or shared Studio queue resume. All chat,
classifier and maintenance adapters use the same 768-pixel model rendition.
Frozen user parts reference exact bytes; base64 exists only in a bounded HTTP
request. Required images fail visibly if unavailable. See [image input](images.md)
for limits, replay, attribution and verification.

## Settings and recovery

Rust owns concrete typed request-parameter validation, composition and final-body
budgeting for chat, classifier phases, repairs, native memory continuations and
consolidation. Accepted workers retain their connection snapshot; explicit Retry
uses current parameters with a compatible frozen input boundary. Native forms
edit one JSON object backed by the connection's TOML `request` table. No named
profile registry or duplicate durable parameter source exists. See
[request settings](settings.md#concrete-request-parameters).

Rust settings APIs expose connection identity, credential presence/environment
reference, connection-local context/output limits, and editable character files; they never return an
inline key. Credential writes use explicit Keep/Replace/Clear/Environment actions.
Before writing, candidate overlays validate the entire affected workspace,
including inherited budgets. Files stage beside their targets and publish by
atomic replacement, Markdown before TOML; partial publication is reported.
Queue acceptance is serialized against settings writes, with persona-local idle
checks and an all-idle rule for app defaults. Swift owns the labeled form,
secure input, and sheet-local errors, not configuration decisions.

The separate response-settings API reads current accepted state, initial defaults,
and schema with a session/state snapshot. Candidate schemas/defaults validate
before publication. A schema change requires explicit fresh context and keep or
reset; reset state and session commit together after all candidate files publish.
The snapshot guards stale saves, and failed publication/commit preserves the old
state and frozen active context with an explicit reload/repair path. Files remain
per-file atomic, without a cross-file journal. Swift presents JSON editors,
read-only state, and confirmations; Rust owns every compatibility/boundary rule.
See [state controls and recovery](state.md).

Only the latest terminal unsuccessful reply is retryable while that persona is
idle. Retry creates a reply/call with retry_of and reuses the original receipt,
never changing old bubbles or frozen requests. A current compatible session can
reuse the nearest frozen ancestor's cutoff, including across unactivated
cancelled retry attempts. Otherwise activation explicitly selects the original
user event into the new context and excludes other earlier events. Durable
failure detail and eligibility are derived for snapshots after restart. See
[settings and recovery](settings.md). There is no automatic resend of failed calls; bounded candidate format repair occurs within a structured call.

## Build decisions

- Native SwiftUI app, with AppKit for window and filesystem integration.
- Apple Silicon development target, macOS 14 deployment target, Swift 6 mode.
- Rust 1.98.1 pin; Cargo.lock records resolved dependencies.
- Bundled SQLite, built with the same deployment target as the app.
- Static Rust linkage through a generated module map and Swift wrapper.
- XcodeGen owns project generation; Xcode supplies local ad-hoc signing.
- A light palette and one starter persona are deliberate first-stage UI scope.

Xcode may place the debug app's code in its own preview/debug dylib. That is
Xcode's debug packaging, not dynamic linkage to a separate Rust core.

## Manual groups

Rooms belong to conversations; histories, state and execution queues belong to
personas. Group creation writes real membership intervals at publication boundary
zero. Add/rejoin records the current publication ordinal; remove closes the
original interval and cancels its unfinished decisions/slots/calls under the
same serialized publication gate. Late callbacks cannot publish, and rejoin
never revives old work. Existing accepted knowledge/state remains shared. A new
member observes later publications but is a response candidate only in later
user rounds. User acceptance resolves typed or picked mentions to stable current-member
IDs, saves one canonical message, assigns its publication order, and creates one
received event per member. Only selected personas receive calls/placeholders.
Unmentioned members may volunteer through independent classification. Bot text
never creates user mentions; accepted replies may prompt bounded optional followups.

Successful completion commits reply/state/native output, author sent event, and
other members' public received events together. A separate publication ordinal
is assigned then: preallocated display placeholders cannot define membership
visibility. Group observations remain consumable even if a mentioned reply is
cancelled before activation. Contiguous event coverage retains gaps. The receipt
cutoff bounds newly added observations, while accepted native history and current
state are preserved even when learned in another room after an older call queued.
First-ever sessions consume all observed events up to that cutoff; explicit
fresh/connection boundaries still exclude prior context.

Calls persist persona identity, persona-wide order, optional membership interval,
and immutable room/trigger. Retry/latest/busy checks use that persona order, not
room display order. Group members only receive attributed public text; the
author's own accepted output remains native replay. Existing frozen prefixes and
old request items are never rewritten to introduce group instructions. New
prefixes explain shared history and public disclosure boundaries. See
[group rules](groups.md) and [history](history.md).

## Selective participation and room flow

A group user-acceptance transaction records one round and initial decision/slot
per member. Explicit mentions become required slots. Unmentioned configured
members start independent cancellable classifier HTTP workers. Rust freezes
bounded persona-scoped inputs in memory; SQLite stores identities, cutoff,
revision, generation/phase, lifecycle, diagnostics and optional call link only.
Classification never reserves a persona queue, consumes events or writes native
history/state. False creates no bubble.

The room coordinator processes required slots first, then optional slots in
least-recently-accepted public speaker order with membership/ID tie breakers.
Only its ready head joins a persona queue; no future queue reservations or
cross-room wait cycle is introduced. Optional decisions recheck relevant accepted
predecessors/current persona state, including after an actual private predecessor
finishes while the admitted call was queued. Generation-scoped callbacks cannot
admit twice or revive cancelled work.

Each member gets at most one initial admission and the whole round at most two
bot followups, excluding the immediately previous speaker. Admission spends the
budget transactionally; failure/cancel does not refund it and format repair stays
inside one call. Native user posts steer unfinished work into the latest room
round, carrying unfinished requested speakers forward. Low-level enqueues retain
their original required-slot cutoffs. Room
cancellation and admission serialize through the persona acceptance gate; final reply,
state, native output and fanout publication checks the durable slot/round/interval
inside the same SQLite transaction. Earlier cancellation prevents late acceptance.

Actual accepted cross-room prefix/state remains current. New observations use
the root cutoff plus explicit accepted same-round earlier public replies; exact
links preserve skipped-event holes and the contiguous frontier. No call forks or
rebuilds history to simulate an earlier state. Native observers stay alive across
waiting required slots, initial decisions, followups and two-pass fallback; silent
checks remain separate from replayable chat history.

Fallback is at most one acknowledgment after valid all-false initial decisions,
using separate strict room-intent and per-persona permission passes. Completion
or failure ends the round. Restart interrupts unfinished rounds/slots/decisions/
calls with no implicit resumption. Explicit latest eligible Retry is independent
and never reopens a chain. Idle checks for state/context/connection publication
include still-admittable slots. See [room flow](group-flow.md) and
[participation](participation.md) for prompts, routing, limits and recovery.
