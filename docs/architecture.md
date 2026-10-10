# Architecture

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

## Durable storage and generation

Rust owns the twelve chat/history/state/membership/decision tables and schema migration. Visible
messages and frozen model items are distinct records; the UI never reconstructs
model context from bubbles. See [durable history](history.md) for table ownership,
ordering, request reconstruction, atomic writes, and interrupted-call behavior.

```text
Swift sends room, text, explicit mentions, and observer
    → Rust accepts one user message, member receipts, selected replies/calls atomically
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
concurrently. Accepted user/reply messages and a received event are durable
before the call starts. A queued call has no session or frozen input yet.
Activation builds its request after the predecessor publishes accepted output,
while limiting incoming events to that call's receipt cutoff. Thus an earlier
call cannot consume later queued inputs, and a followup sees its predecessor's
native assistant output even when that output event was recorded later.

Swift keeps a presentation cache, draft, mention selection, and run handles per room. Room switching
changes the displayed cache without disconnecting other streams. Drafts, output,
errors, and Stop belong to that room. Queued bubbles expose individual cancel;
Stop cancels the selected room's oldest submission, including its remaining handles. Closing the view
cancels every retained handle/subscription; dropping the core cancels its queues and participation workers.
Early cancellation is retained until handles arrive. A group observer adopts dynamically admitted speaker handles and remains open until every participation decision and selected call terminates; one character finishing cannot disconnect another. Startup marks both queued
and active calls interrupted, with no accepted partial output or hidden resubmit.

Rust character creation atomically publishes a UUID directory containing
config.toml, SOUL.md, MEMORY.md, and response_schema.json. It inherits the app's default connection,
creates a stable private room, and exposes a minimal name form in Swift. Native settings edits authoritative TOML/Markdown through Rust; deletion remains
outside this milestone.

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

Reasoning never appears in the UI. Tool requests, errors, incomplete terminal
states, truncation, text mismatch, refusal, and exhausted budgets fail without
publishing structured candidate text, state, or native output. Only JSON/schema
failures permit bounded automatic format repair. Preserved v0.1.0 sessions keep
their frozen plaintext behavior until an explicit context boundary. See [state
and upgrade rules](state.md).

Sessions bind protocol, model, and credential-free API root. A changed binding
retires the previous context and starts after the current event frontier. Old
transcript and model items remain intact; a persistent notice explains the fresh
context. SOUL/MEMORY stays frozen inside an unchanged session after file reload.
Explicit fresh context is a Rust operation serialized against queue acceptance.
It refuses outstanding work for that persona, retires the active session, and
atomically saves a new prefix with the current files and event boundary. Earlier
transcript/native history stays intact and is excluded from new requests. Other
personas can continue. Swift presents confirmation and the persistent notice.

The request loader scans codec/size metadata before reading any payloads. Raw or
zstd items decode into the exact saved JSON bytes under a fixed 8 MiB context
bound; zstd window and output reads have independent bounds. A conservative
byte-based input estimate plus configured output reserve is checked before the
adapter starts. Budget edits alone leave the session and prefix unchanged.
See [context and encoding](context.md). There is no background service, automatic
memory writer, or delegated work.

## Settings and recovery

Rust settings APIs expose connection identity, credential presence/environment
reference, budget overrides, and editable character files; they never return an
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
zero. User acceptance resolves typed or picked mentions to stable current-member
IDs, saves one canonical message, assigns its publication order, and creates one
received event per member. Only selected personas receive calls/placeholders.
No mention means durable observations without generation. Bot text never creates
mentions or another call.

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

## Selective participation

A group user-acceptance transaction also records one participation decision per
member. Explicit mentions have their already accepted calls; unmentioned members
with a configured classifier start independent cancellable HTTP workers. Rust
freezes a bounded, persona-scoped read view in memory at acceptance. SQLite stores
only identity, cutoff, revision, lifecycle, bounded diagnostics, and optional call
link, without another copy of SOUL/MEMORY/transcript payloads.

A validated true result atomically selects the decision and creates one reply/call
in that persona's ordinary queue. Original room/root/interval/cutoff never change.
Actual generation freezes later from the current accepted native prefix and state;
classification does not reserve the queue, consume events, or write model history.
A false result becomes Listening without a reply. New user posts cancel older
pending decisions in that room; accepted calls continue. Callback ordering admits
the dynamic handle before Started/terminal events and before the final decision
settles the observer. No callback wait holds the cancellation mutex.

Restart interrupts pending decisions and never resends them. Failure and stopped
checks retain their public observations and expose manual mention recovery.
Classifier connections, credentials, protocol constraints, and time/input limits
are detailed in [participation](participation.md). Bot-triggered decisions and a
room scheduler remain later stages.
