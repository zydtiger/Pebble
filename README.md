# Pebble

A small space for familiar voices.

Pebble is a native Mac app for persistent characters, conversation, and
roleplay. Its name connects an everyday object with *pebbling*: sharing small
things as gestures of care.

**v0.3.0 adds durable memory and explicit context consolidation to private and group conversations.** Create
characters, choose group members, and let each character decide when to join,
or mention who should reply. Each character
keeps one native model history, state, and execution queue across all its rooms;
silent members observe public messages without generating. Native settings and
state/schema controls remain available through each character's private chat.
Validated replies, cancellation, and explicit retry preserve saved conversations.
Chat Completions/Responses retain native history with bounded context budgets
and lossless item compression.
Without a connection, the app uses its clearly labeled deterministic local demo.

See [completion evidence and current limits](docs/roadmap.md#group-boundaries-and-completion-scenarios)
for the verified end-to-end scenarios. Participation remains a model judgment;
manual mentions provide an explicit request, and whole-group fallback permits
only one brief acknowledgment/clarification.

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
   followup. **Stop** cancels this room's unfinished replies; a queued bubble has its
   own **Cancel** action.
3. Choose **New character**, enter a name, and create a separate private room.
   Switch between characters while both reply. Drafts and stream state stay in
   their own rooms.
4. Open the character information panel to read SOUL and MEMORY, or use
   **Character settings** to edit them and the character's connection. The
   **Character state and response schema** braces button opens separate controls
   for accepted state, initial defaults, and response schema.
5. After editing SOUL/MEMORY, choose **Start fresh context** when you want the
   model to use the new files. The same chat model remembers important outgoing knowledge in MEMORY before the new context starts; accepted state and visible messages stay saved. Advanced file
   editing and **Reload Character Files** remain available; character and room
   IDs stay stable.
6. Send a thought, then quit and reopen the app. The messages and their IDs
   survive. The next request reuses the saved prefix and completed native
   assistant output. The unconfigured demo quotes the preceding user message.

New contexts show **Typing…** until the whole structured response validates, then display only its `reply`. Accepted `state` persists per character in SQLite. Cancelled replies show a **Stopped** label without candidate text. Preserved legacy contexts retain their earlier partial-stream behavior; choose **Start fresh context** to activate structured replies. See [state and upgrade rules](docs/state.md). An abrupt
exit leaves an **Interrupted** reply after restart; incomplete outputs are not
accepted as assistant context. Empty drafts are not sent.

## Try a group chat

1. Choose **New group**, enter a name, and select one or more existing characters.
   Groups have stable IDs and appear beneath private chats in the sidebar.
2. Type `@Name` or use **Mention** to choose members. Selected chips are explicit
   recipients and remain selected while you edit the body; remove a chip to
   remove that selection. Duplicate names require choosing the member instead
   of an ambiguous typed token. Selected names are included in the saved message.
3. Send a public message. Unmentioned characters independently show **Deciding…**,
   then **Listening** or **Reply requested**. Listening creates no empty bubble.
   Mentioned characters bypass the participation model; others can still volunteer.
   Replies show their authors and run in room order. Requested speakers go first;
   optional speakers reconsider accepted predecessors. Waiting requests stay visible.
   **Participation settings** opens the separate small-model connection; a failed
   check remains visible with **Mention** and **Settings** recovery actions.
4. Switch rooms while replies run. Drafts and mention selections stay with each
   room; replies keep their original destination. The group header lets you
   rename the group, open a member's private chat/settings, or manage members.
   **Add**/**Rejoin** observes future public messages only. **Remove** stops that
   member's unfinished work here, preserving their knowledge and other rooms.
   A room can retain its transcript with no current members.
5. Stop cancels every unfinished round, decision and reply in this room; queued
   bubbles have individual Cancel. Retry uses the original room and speaker,
   and is available only for that character's latest unsuccessful call across
   all rooms while the character is idle and its original membership interval
   remains current. Rejoining never revives an old reply.

Only public user messages and accepted reply text fan out to current members.
Native reasoning, state, and private exchanges are never copied to other
characters. A character itself retains its private knowledge and may disclose
it in a public reply; prompt instructions cannot guarantee secrecy from the
same shared-context model. A round permits one initial reply per member and at
most two character followups. An explicit whole-group question with no volunteers
can receive one permitted acknowledgment; explicit silence is respected through
a separate permission check. A new user post interrupts old automatic replies
while preserving outstanding user mentions. Removing a member cancels their
unfinished work in the old interval; rejoining never revives it.
Participation reads do not consume chat observations or change character state.
An absent participation connection keeps manual mentions available. New roots
seed the selected Qwen connection independently of the demo/live chat model;
existing authored config is preserved. See [participation configuration and
limits](docs/participation.md). See [group rules](docs/groups.md)
and [the roadmap](docs/roadmap.md).


## Local data

On first launch, Rust seeds an empty data root with a character and editable
files. Existing files are never overwritten by the seed operation.

```text
~/Library/Application Support/Pebble/
├── config.toml
├── personas/<persona-uuid>/
│   ├── config.toml
│   ├── SOUL.md
│   ├── MEMORY.md
│   ├── .MEMORY-<operation-id>.displaced # retained displaced inode after memory publication
│   └── response_schema.json
└── data/
    ├── app.sqlite        # eighteen chat/history/state/memory/context tables
    └── app.lock          # exclusive app ownership; released automatically on exit
```

SQLite may also create its own WAL and shared-memory sidecars. Schema versions 1
through 10 are upgraded transactionally to 11, preserving character, room, message,
and history IDs. Only
one app instance can open a data root at a time; a second instance reports an
error without changing active calls. Keep this data
outside the source checkout. `PEBBLE_DATA_DIR` selects an isolated root for
development and tests; an existing, incomplete root reports an error instead
of silently reseeding it.

The app config optionally supplies `[defaults.chat]`. A persona can provide its
own `[chat]` table, replacing the whole connection. Connections require `model`,
`base_url`, `api` (`chat_completions` or `responses`), `context_len`, and
`max_output_tokens` together. `api_key = ""`
is the placeholder for an endpoint without authentication. Authenticated endpoints
can use a nonempty `api_key` in the local config, or an `api_key_env` reference;
do not configure both at once. Base URLs are API roots, normally ending in `/v1`
for either protocol. Set `api = "responses"` to use Responses. A configured connection calls that
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
context_len = 65536
max_output_tokens = 4096
```

Empty credentials omit Authorization. `api_key_env` is resolved when a request
starts; a missing variable fails the reply. Redirects are disabled and the
client honors environment proxy settings. Connection/header/idle waits are
bounded at 10/45/45 seconds, with a 180-second request timeout and a 4096-token
default output budget, configurable below. Reasoning-only, truncated, filtered, or budget-exhausted outputs
are failed replies; unvalidated candidate text is not shown or saved. Provider reasoning stays
in native model history and is never rendered as a message. Responses sends
`store = false` and requests encrypted reasoning state. Completed native output
items, including IDs and opaque fields, are stored and replayed in order with
the full input history; no `previous_response_id` is sent. New contexts support only the persona-scoped `update_memory` native tool. Tool calls/results and reasoning remain hidden from bubbles and replay in native order. Existing contexts keep their frozen prefix and an explicit capability notice until a successful fresh-context transition. See [memory and consolidation](docs/memory.md). In structured contexts, provider refusal is a terminal failed turn and leaves state/native history unchanged. Legacy contexts retain their earlier plaintext refusal behavior; see [upgrade rules](docs/state.md#upgrading-a-v010-root).

Changing the API root, model, or protocol starts a fresh model context. Earlier
messages stay visible, and a notice explains the boundary. Those earlier events
are not silently imported into the new model session. Editing character files
does not change an existing session's frozen SOUL/MEMORY prefix.

## Settings and explicit retry

Open **App defaults** with the sidebar gear, or **Character settings** with the
sliders button in the chat header. Edit model/API root/protocol and context
budgets. Character settings also edits name, subtitle, SOUL, and MEMORY. Turn off
**Override app connection** to inherit the app connection and both of its limits. An explicit connection override requires
its own context and output limits. Character edits require only that character to be
idle, including pending decisions and still-admittable group slots; app-default
edits require all affected work to finish or stop.

The Responses connection form offers **Standard · text.format** and the explicit
**llama.cpp · response_format** schema dialect. It appears only for Responses;
switching to Chat Completions removes the exclusive field. A character override
replaces the whole app connection, including this dialect.

**State & response** shows current accepted state separately from editable JSON
initial defaults and response schema. **Save initial defaults only** preserves
state and context. Schema changes require an explicit fresh context with either
compatible current state or reset to the displayed defaults. Reset asks for
confirmation, replaces state, and creates a new context together; earlier
messages, calls, contracts, and native history stay saved. Invalid or stale
candidates leave state/context unchanged and report how to recover. See
[state controls and failure recovery](docs/state.md#native-controls-and-publication).

Keys are never displayed. **Keep existing** retains credentials during unrelated
saves; **Replace inline key**, **Clear authentication**, and **Environment
variable** are explicit alternatives. Replacement uses a secure text field.
Settings stay in local TOML/Markdown files; advanced folder access remains
available. SOUL/MEMORY edits leave an existing frozen prefix intact. New memory-capable turns also receive the current MEMORY snapshot at the input tail so an automatic update cannot overwrite unseen earlier manual edits. Start fresh context to consolidate knowledge and snapshot the current files. Invalid manual TOML can require opening the files to repair.

A failed structured reply retains its error after restart; legacy attempts retain any earlier partial text. **Retry**
appears only on that character's latest failed, stopped, or interrupted reply,
when it has no outstanding work. Retry creates a new reply/call and reuses the
original user message; earlier attempts stay visible. In the same compatible
context it reuses the frozen request. After connection changes or explicit
fresh context, it admits only the selected original input into the new context,
without importing other old messages. Runtime key and budget repairs can be
used without rewriting the old call. Nothing is resent automatically.

Each settings file is staged and replaced atomically. A character save spans
SOUL.md, MEMORY.md, and config.toml (last); this is not a cross-file transaction.
All candidate settings and dependent inherited budgets are validated first.
Unrelated TOML fields/comments are preserved where practical; disabling a chat
override removes that entire table. If publication partly fails, the sheet
reports which files were saved and asks you to inspect/reopen before retrying.
See [settings and recovery](docs/settings.md) for the durable rules.

## Context budgets and fresh contexts

Each connection owns its total token window (`context_len`) and output cap
(`max_output_tokens`). Configure them inside `[defaults.chat]` and
`[defaults.classifier]`; persona `[chat]` and `[classifier]` overrides replace the
whole respective connection, including its limits. An omitted persona connection
inherits the whole app connection. Configured connections require both positive
limits, with output strictly smaller than the window; missing or obsolete context
tables report an actionable configuration error. The unconfigured demo uses an
internal 65536/4096 budget.

The output cap is sent as Chat Completions `max_tokens` or Responses
`max_output_tokens`; `context_len` is local and is never sent to the provider.
Budget edits preserve existing history, native prefix, session and accepted state.
Each accepted request retains its effective limits.

Pebble conservatively counts the final request JSON as UTF-8 bytes plus a
128-byte framing reserve, with one byte treated as one estimated input token.
This includes SOUL/MEMORY, native history, reasoning and opaque state, current
state, response schema and request metadata; structured repair feedback also
counts. The output cap must fit in the total window. This is **not exact provider
token usage**. Providers may still reject a request. An independent 8 MiB decoded
history bound remains in force. A context-full chat reply fails before HTTP and
keeps its input and failed bubble, so you can increase limits or start fresh.
Participation has an independent budget and can trim optional older excerpts;
mandatory trigger, rules and phase inputs must fit or the decision fails visibly
before HTTP without consuming chat observations or changing state.

Choose **Start fresh context** in the private-chat header. Rust reserves that
persona's queue, freezes its observation cutoff, and asks the same chat model
to consolidate outgoing native knowledge and eligible private/group observations
with current MEMORY. A successful review (including unchanged memory) publishes
a new SOUL/MEMORY prefix with the accepted state unchanged. Messages and native
archives remain saved. Other personas can keep chatting; later observations
remain available in the new context. A thin gray line and diamond mark the
successful transition in the private room and eligible current groups, where
the label identifies the affected character.

The UI shows progress, Cancel, and persisted recovery details. A cancelled or
failed transition keeps the old context active and adds no marker. A memory
write already committed survives a failed final reply or context publication;
explicit retry uses its recorded result without repeating the write. Corrupt,
unreadable or oversized outgoing knowledge fails actionably rather than being
discarded. Empty contexts need no consolidation; the local demo keeps MEMORY
unchanged and explains that no chat model is connected. New live contexts reserve
conservative maintenance headroom below the ordinary chat ceiling; configured
`context_len` itself is unchanged. See [memory and recovery](docs/memory.md) and
[exact budget arithmetic](docs/context.md#maintenance-headroom).

History items use raw JSON or zstd individually. Compression changes storage
bytes only; decoded native bytes and IDs remain stable. Existing raw items are
not rewritten. See [context and encoding](docs/context.md) for the loader bounds,
estimate limitations, and synthetic size measurements.

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
prefixes, multiple native output items and opaque replay, fragmented SSE, model failure states, event
links, concurrent persona queues, queued cancellation, transactional rollback,
and abrupt-process-exit recovery. The Swift executable
checks the real generated FFI and presentation adapter under complete Swift 6
concurrency checking. The renderer captures this app's own content without
capturing the desktop or other applications.

Live endpoint validation is opt-in; ordinary tests only use local deterministic
servers. The following test seeds synthetic character files in an isolated
temporary root, saves settings, completes two turns across restart, and checks early cancellation plus explicit retry:

```sh
PEBBLE_LIVE_API_ROOT=http://localhost:8080/v1 \
PEBBLE_LIVE_MODEL=your-model \
cargo test -p pebble-core --test live_endpoint opt_in_live_two_turns_restart_and_cancel -- --ignored --nocapture
```

For Responses, use the same environment variables and replace the test name
with `opt_in_live_responses_two_turns_restart_and_cancel`. For llama.cpp Responses, also set `PEBBLE_LIVE_RESPONSES_SCHEMA_FORMAT=response_format`; standard Responses uses `text.format` by default. See [provider contracts](docs/providers.md).

`PEBBLE_LIVE_API_KEY` optionally supplies a test credential. The test does not
read the app's actual character files or chat history and prints no reasoning
content or credentials.

Nothing is committed, published, or released automatically by these commands.
