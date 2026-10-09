# Architecture

## Boundaries

`pebble-core` is an ordinary Rust library. It owns TOML parsing and validation,
SOUL/MEMORY loading, stable persona/private-room IDs, SQLite, and the preview
worker's lifecycle. It can be tested without a UI or language bridge.

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

## First-stage storage

Each character's UUID comes from its folder name. The database has exactly two
application tables:

- `personas`: stable ID, display name, relative directory, creation time.
- `conversations`: stable room ID, `direct` kind, unique persona peer, creation
  time. One canonical private room belongs to each persona.

Foreign keys and WAL are enabled. The filesystem is authoritative for editable
persona content. Loading validates files before reconciling identity records
in one transaction. Editing a display name preserves both IDs. The database
schema version starts at 1; this stage has no legacy schema to migrate.

Preview bubbles are temporary presentation state. They are not a replacement
for the message/event/session model planned for v0.0.2.

## Preview lifecycle

```text
Swift creates AsyncStream and observer
    → Rust validates the character and starts a preview
    → Rust returns a cancellable handle
    → Rust emits started / textDelta / completed or cancelled
    → Swift consumes events in order on MainActor
```

Each event carries run, persona, conversation, and sequence IDs. The listener
only enqueues events; it never mutates SwiftUI from the Rust worker thread.
The UI rejects an event whose run or room does not match its active preview.

Rust emits small Unicode-safe chunks on a worker thread. A condition variable
wakes that thread promptly on cancellation. Only one preview runs at a time.
Dropping its handle also cancels it. The worker releases its listener on
completion, cancellation, or a disconnected consumer.

The Swift adapter remembers cancellation requested before the Rust handle has
returned. Closing the view explicitly cancels Rust work, cancels the consumer,
and finishes its stream. Cancelling a Swift Task alone is not used as a Rust
cancellation mechanism. The stream buffer is bounded; this fixed short preview
fits inside it, and a dropped update disconnects the listener rather than
silently presenting incomplete text as a complete reply.

The app process owns the core lifetime. There is no background service, network
client, model reasoning, automatic memory writer, or work-agent execution in
this milestone.

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
