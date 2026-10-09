# Pebble

A small space for familiar voices.

Pebble is a native Mac app for persistent characters, conversation, and
roleplay. Its name connects an everyday object with *pebbling*: sharing small
things as gestures of care.

**v0.0.2 adds durable private chats.** It loads one starter character and streams
a clearly labeled local demo reply through the real Rust message/history pipeline.
Messages survive restarting the app. No model or network service is called.

## Run it

Requirements: Apple Silicon Mac, Xcode with Swift 6.3 or later and Icon Composer,
Rust 1.98.1 (including rustfmt and Clippy), XcodeGen 2.46 or later, and `just`. Validation also uses `prek`
0.4.14 or later. These tools are developer prerequisites, not app dependencies.
The app's deployment target is macOS 14; it is currently exercised on the
development host, not a matrix of older macOS releases.

```sh
just run
```

This builds and opens `build/Pebble.app`. It is ad-hoc signed for local use;
no Developer ID, App Store account, remote repository, or release is required.
You can reopen that app directly without rebuilding.

The build generates a local Xcode project at `macos/Pebble.xcodeproj`. Open it
after the first build if you want to iterate in Xcode. Generated bindings and
project files are ignored; their source of truth is Rust plus `macos/project.yml`.

## App icon

The editable [Pebble.icon](macos/Pebble/Resources/Pebble.icon) document contains
two vector pebbles with Liquid Glass materials. Run `just icon` to edit it in
Icon Composer, or `just preview-icon` to export appearance and small-size
previews into `build/icon-preview/`. `just build` compiles the icon into the app.
See [the icon design notes](docs/app-icon.md) for artwork and material settings.

## Try the durable-chat stage

1. Open Mira's private space and choose **Say hello**, or type a thought and
   press **Enter**. **Command–Enter** or **Shift–Enter** inserts a new line. The composer grows
   with your text, then scrolls at its maximum height; the send button stays at
   the lower right.
2. Stop a reply while it streams. This cancels the worker in Rust.
3. Open the character information panel to read SOUL and MEMORY.
4. Use **Open character folder**, edit the files, and choose **Reload Character
   Files** from the app menu or sidebar. Character and room IDs remain stable.
5. Send “I like rainy walks,” then quit and reopen the app. The messages and
   their IDs survive. Send another thought: the deterministic demo quotes the
   preceding user message from its saved request history.

Cancelled replies retain their partial text with a **Stopped** label. An abrupt
exit leaves an **Interrupted** reply after restart; incomplete outputs are not
accepted as assistant context. Empty drafts are not sent.

The interface is an early light-theme design with a starter character. Groups,
live inference, automated memory, and work delegation
belong to later milestones. See [the roadmap](docs/roadmap.md).

## Local data

On first launch, Rust seeds an empty data root with a character and editable
files. Existing files are never overwritten by the seed operation.

```text
~/Library/Application Support/Pebble/
├── config.toml
├── personas/<persona-uuid>/
│   ├── config.toml
│   ├── SOUL.md
│   └── MEMORY.md
└── data/
    ├── app.sqlite        # eight private-chat/history tables
    └── app.lock          # exclusive app ownership; released automatically on exit
```

SQLite may also create its own WAL and shared-memory sidecars. Schema version 1
from v0.0.1 is upgraded transactionally, preserving character and room IDs. Only
one app instance can open a data root at a time; a second instance reports an
error without changing active calls. Keep this data
outside the source checkout. `PEBBLE_DATA_DIR` selects an isolated root for
development and tests; an existing, incomplete root reports an error instead
of silently reseeding it.

The app config optionally supplies `[defaults.chat]`. A persona can provide its
own `[chat]` table, replacing the whole connection. Connections require `model`,
`base_url`, and `api` (`chat_completions` or `responses`) together. `api_key = ""`
is the placeholder for an endpoint without authentication. Authenticated endpoints
can use a nonempty `api_key` in the local config, or an `api_key_env` reference;
do not configure both at once. Base URLs are API roots, normally ending in `/v1`
for Chat Completions. v0.0.2 validates this configuration but still runs the local
demo; network inference and authentication arrive in v0.0.3. An omitted
connection means no model is configured. Runtime configuration and actual keys
stay outside the repository.

## Architecture

```text
SwiftUI / AppKit presentation
          │ typed records + event listener
          ▼
UniFFI Swift bindings → one Rust static library
          │
          ▼
pebble-core
  configuration · personas · messages · events · model history · demo lifecycle
```

Swift handles rendering, input, message snapshots, and platform paths.
Rust owns authoritative identities, configuration, messages, events, frozen
model history, database transactions, and the local demo stream. The core has no dependency on Swift or UniFFI.
Read [the architecture](docs/architecture.md) for the boundary and lifecycle.

## Validate

```sh
just check          # file checks, Rust lint/tests, Swift 6 FFI integration
just build          # native application compile and local signing
just preview        # render actual SwiftUI views into build/previews/
```

`just check-rust` and `just check-bridge` run narrower gates. The hook definitions
in `.pre-commit-config.yaml` own the exact validation commands. Install `prek`
on the development machine and run `prek install` in a clone to activate its
pre-commit, pre-push, and commit-message hooks. Hooks are not installed globally.

Tests use isolated data roots; UI-check data stays under the ignored `build/`
directory for inspection. The Rust tests cover restart
identity, schema scope, file edits, connection inheritance, invalid files,
Unicode event order, cancellation, callback lifetime, migration, frozen request
prefixes, event links, transactional rollback, and abrupt-process-exit recovery. The Swift executable
checks the real generated FFI and presentation adapter under complete Swift 6
concurrency checking. The renderer captures this app's own content without
capturing the desktop or other applications.

Nothing is committed, published, or released automatically by these commands.
