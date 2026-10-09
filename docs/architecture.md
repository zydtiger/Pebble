# Architecture

## Boundaries

`pebble-core` is an ordinary Rust library. It owns TOML parsing and validation,
SOUL/MEMORY loading, stable persona/private-room IDs, SQLite, frozen model history, and the
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

Rust owns the eight private-chat/history tables and schema migration. Visible
messages and frozen model items are distinct records; the UI never reconstructs
model context from bubbles. See [durable history](history.md) for table ownership,
ordering, request reconstruction, atomic writes, and interrupted-call behavior.

```text
Swift sends text and observer
    → Rust accepts messages, receipt event, and queued call atomically
    → persona queue activates the call and freezes its request atomically
    → Rust returns a cancellable handle and authoritative message IDs
    → Chat Completions, Responses, or unconfigured demo consumes the stored request
    → Rust checkpoints visible deltas, then notifies Swift
    → Rust atomically publishes output/event links and terminal state
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

Swift keeps a presentation cache and run handles per private room. Room switching
changes the displayed cache without disconnecting other streams. Drafts, output,
errors, and Stop belong to that room. Queued bubbles expose individual cancel;
Stop targets the selected persona's oldest outstanding reply. Closing the view
cancels every retained handle/subscription; dropping the core cancels its queues.
Early cancellation is retained until a handle arrives. Startup marks both queued
and active calls interrupted, with no accepted partial output or hidden resubmit.

Rust character creation atomically publishes a UUID directory containing
config.toml, SOUL.md, and MEMORY.md. It inherits the app's default connection,
creates a stable private room, and exposes a minimal name form in Swift. General
editing/settings and deletion are outside this milestone.

The demo protocol is distinct from Chat Completions and Responses. Configured
connections always run their real HTTP adapter.
An absent connection runs the local demo. Rust supplies mode/model labels and
fresh-context notices to the typed frontend records.

HTTP uses the configured API root plus `/chat/completions` or `/responses`, optional runtime
credentials, environment proxy settings, and no redirects. A cancellable future
covers connection, headers, and idle stream waits. Connection timeout is 10
seconds; headers and stream idle timeout are 45 seconds; the entire request has
a 180-second deadline. Requests have a 4096-token completion budget. Credentials
never enter model history or call records, and provider bodies/URLs are not
copied into error diagnostics.

SSE is framed as bytes before decoding complete UTF-8 lines. Chat Completions
renders only `content` deltas and requires a successful `stop` finish plus
`[DONE]` and nonempty visible text. Native assistant content, `reasoning_content`,
and `refusal` are retained for replay.

Responses renders output text and refusal deltas. A completed-status
`response.completed` event supplies authoritative native output in array order;
`[DONE]` is unnecessary. Whole accepted reasoning/message items, including IDs,
encrypted state, and unknown fields, are persisted once and replayed unchanged
semantically. Earlier added reasoning may be incomplete and is never used as
accepted output. Final item events and final visible text must agree with the
stream. Both indexed events and item-ID events with an unambiguous content-part
lifecycle are supported. See [provider contracts](providers.md).

Reasoning never appears in the UI. Tool requests, errors, incomplete terminal
states, truncation, text mismatch, and exhausted budgets preserve visible partial
text as failed output and publish no native output. No retry is automatic.

Sessions bind protocol, model, and credential-free API root. A changed binding
retires the previous context and starts after the current event frontier. Old
transcript and model items remain intact; a persistent notice explains the fresh
context. SOUL/MEMORY stays frozen inside an unchanged session after file reload.
There is no background service, automatic memory writer, or delegated work.

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
