# Architecture

## Boundaries

`pebble-core` is an ordinary Rust library. It owns TOML parsing and validation,
SOUL/MEMORY loading, stable persona/private-room IDs, SQLite, frozen model history, and the
demo worker's lifecycle. It can be tested without a UI or language bridge.

`pebble-ffi` wraps the core with UniFFI 0.32.2 records, typed errors, objects, and
a foreign-trait observer. It builds one static library. Bindings are generated
directly from that archive; no Rust dynamic library or local HTTP service is
needed. Generated Swift and C declarations always come from the current core.

SwiftUI owns navigation, text input, view state, and rendering. `CoreClient` is
an actor that keeps blocking configuration/database operations off MainActor.
The generated types conform to Sendable with the selected Swift 6 compiler.
No asynchronous Rust function is exported yet: short commands plus a callback
stream meet this stage's needs. A future HTTP implementation will own whatever
Rust async runtime it requires.

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
    → Rust accepts messages/events/request items in one transaction
    → Rust returns a cancellable handle and authoritative message IDs
    → local demo consumes the stored request outside the transaction
    → Rust checkpoints visible deltas, then notifies Swift
    → Rust atomically publishes output/event links and terminal state
    → Swift reloads the saved transcript
```

Each event carries call, persona, conversation, message, and event-sequence
IDs. Swift only renders matching output, reconciles authoritative snapshots,
and retains an unaccepted draft on error. MainActor never opens SQLite. The
actor bridge keeps blocking core calls off the UI thread, and listeners only
enqueue events. A bounded, disconnected subscription cancels generation.

Only one call runs at a time in v0.0.2; the UI disables character switching
during a reply. An early cancel request is retained until Rust returns the
handle. Closing the view cancels the worker and subscription. A process-level
data-root lock prevents another app instance from interrupting live work during
startup recovery. Basic recovery marks unfinished replies interrupted without
accepting their partial content into model history.

The demo protocol is distinct from Chat Completions and Responses. Provider
configuration has no effect on this stage's runtime. The fixed persona/memory
snapshot stays unchanged for the session, including after manual file reload.
Live inference and explicit fresh-context transitions remain later increments.
There is no background service, model network client, automatic memory writer,
or delegated work in this milestone.

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
