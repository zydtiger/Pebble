# Pebble

A small space for familiar voices.

Pebble is a native Mac app for persistent characters, conversation, and
roleplay. Its name connects an everyday object with *pebbling*: sharing small
things as gestures of care.

**v0.0.4 adds independent character conversations.** Create another character,
queue followups, and switch rooms while replies continue. Each character has its
own files, private room, model session, and serialized queue. Configure an
OpenAI-compatible API root and model for live Chat Completions; without a
connection, the app uses its clearly labeled deterministic local demo.

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

## Try independent private chats

1. Open Mira's private space and choose **Say hello**, or type a thought and
   press **Enter**. **Command–Enter** or **Shift–Enter** inserts a new line. The composer grows
   with your text, then scrolls at its maximum height; the send button stays at
   the lower right.
2. Type another message while a reply streams and press **Enter** to queue a
   followup. **Stop** cancels this room's current reply; a queued bubble has its
   own **Cancel** action.
3. Choose **New character**, enter a name, and create a separate private room.
   Switch between characters while both reply. Drafts and stream state stay in
   their own rooms.
4. Open the character information panel to read SOUL and MEMORY.
5. Use **Open character folder**, edit the files, and choose **Reload Character
   Files** from the app menu or sidebar. Character and room IDs remain stable.
6. Send a thought, then quit and reopen the app. The messages and their IDs
   survive. The next request reuses the saved prefix and completed native
   assistant output. The unconfigured demo quotes the preceding user message.

Cancelled replies retain their partial text with a **Stopped** label. An abrupt
exit leaves an **Interrupted** reply after restart; incomplete outputs are not
accepted as assistant context. Empty drafts are not sent.

The interface is an early light-theme design with a starter character. Responses, groups, automated memory, and work delegation belong to later
milestones. See [the roadmap](docs/roadmap.md).

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

SQLite may also create its own WAL and shared-memory sidecars. Schema versions 1
through 3 are upgraded transactionally to 4, preserving character, room, message,
and history IDs. Only
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
for Chat Completions. Responses is validated as a protocol choice but reports
“not implemented” when sent in this version. A configured connection calls that
endpoint or reports an error; it never falls back to the demo. An omitted
connection means no model is configured. New characters inherit the app default
connection until their own local config supplies a `[chat]` table. Runtime configuration and actual keys
stay outside the repository.

For example, edit the local app config (replace the sample root and model):

```toml
[defaults.chat]
model = "your-model"
base_url = "http://localhost:8080/v1"
api = "chat_completions"
api_key = ""
```

Empty credentials omit Authorization. `api_key_env` is resolved when a request
starts; a missing variable fails the reply. Redirects are disabled and the
client honors environment proxy settings. Connection/header/idle waits are
bounded at 10/45/45 seconds, with a 180-second request timeout and a 4096-token
output budget. Reasoning-only, truncated, filtered, or budget-exhausted outputs
are failed replies; visible partial text stays saved. Provider reasoning stays
in native model history and is never rendered as a message.

Changing the API root, model, or protocol starts a fresh model context. Earlier
messages stay visible, and a notice explains the boundary. Those earlier events
are not silently imported into the new model session. Editing character files
does not change an existing session's frozen SOUL/MEMORY prefix.

## Architecture

```text
SwiftUI / AppKit presentation
          │ typed records + event listener
          ▼
UniFFI Swift bindings → one Rust static library
          │
          ▼
pebble-core
  configuration · personas · messages · events · model history · HTTP/SSE lifecycle
```

Swift handles rendering, input, message snapshots, and platform paths.
Rust owns authoritative identities, configuration, messages, events, frozen
model history, database transactions, and live or demo streams. The core has no dependency on Swift or UniFFI.
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
prefixes, native assistant replay, fragmented SSE, model failure states, event
links, concurrent persona queues, queued cancellation, transactional rollback,
and abrupt-process-exit recovery. The Swift executable
checks the real generated FFI and presentation adapter under complete Swift 6
concurrency checking. The renderer captures this app's own content without
capturing the desktop or other applications.

Live endpoint validation is opt-in; ordinary tests only use local deterministic
servers. The following test seeds synthetic character files in an isolated
temporary root, completes two turns across restart, and checks early cancellation:

```sh
PEBBLE_LIVE_API_ROOT=http://localhost:8080/v1 \
PEBBLE_LIVE_MODEL=your-model \
cargo test -p pebble-core --test live_endpoint opt_in_live_two_turns_restart_and_cancel -- --ignored --nocapture
```

`PEBBLE_LIVE_API_KEY` optionally supplies a test credential. The test does not
read the app's actual character files or chat history and prints no reasoning
content or credentials.

Nothing is committed, published, or released automatically by these commands.
