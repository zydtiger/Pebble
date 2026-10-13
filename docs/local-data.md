# Local data and connection files

On first launch, Rust seeds an empty data root with a character and editable
files. Existing files are never overwritten by the seed operation.

```text
~/Library/Application Support/Pebble/
├── config.toml
├── personas/<persona-uuid>/
│   ├── config.toml
│   ├── SOUL.md
│   ├── MEMORY.md
│   ├── avatar.png        # optional character profile picture
│   ├── .MEMORY-<operation-id>.displaced # retained displaced inode after memory publication
│   └── response_schema.json
└── data/
    ├── app.sqlite        # chat/history/state/memory/context/image/task records (schema v13)
    ├── app.lock          # exclusive app ownership; released automatically on exit
    ├── assets/<image-uuid>/ # original, fixed model rendition and thumbnail
    └── codex-workspaces/<task-uuid>/ # managed task working directory
```

SQLite may also create its own WAL and shared-memory sidecars. Schema versions 1
through 12 are upgraded transactionally to 13, preserving character, room, message,
and history IDs. Only
one app instance can open a data root at a time; a second instance reports an
error without changing active calls. Keep this data
outside the source checkout. `PEBBLE_DATA_DIR` selects an isolated root for
development and tests; an existing, incomplete root reports an error instead
of silently reseeding it.

To customize a character's profile picture, place `avatar.png` beside their
`config.toml` in `personas/<persona-uuid>/`, then click **Reload character files**
or reopen Pebble. Replacing or removing the file takes effect on the same reload.
The sidebar, chat header, character details and private/group replies use that
character's picture, cropped to a circle. Missing or unreadable images use the
default pebble avatar. Avatars are display-only and do not enter model requests.
Click a custom avatar in the private-chat header or an image attachment to open
the full picture in a separate, resizable window. Use the magnifier buttons or
pinch the trackpad to zoom, and scroll to explore a magnified image. The pin button
keeps that preview above normal windows until toggled off. **Escape**, **Command–W**
or the window's close button closes the preview; chat remains available throughout.

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
base_url = "https://api.example.com/v1"
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
the full input history; no `previous_response_id` is sent. New contexts support the persona-scoped `update_memory` native tool and `delegate_task` for work through local Codex when enabled. Tool calls/results and reasoning remain hidden from bubbles and replay in native order. Existing contexts keep their frozen prefix and a capability notice in character information until a successful fresh-context transition. See [memory and consolidation](memory.md). In structured contexts, provider refusal is a terminal failed turn and leaves state/native history unchanged. Legacy contexts retain their earlier plaintext refusal behavior; see [upgrade rules](state.md#upgrading-a-v010-root).

Changing the API root, model, or protocol starts a fresh model context. Earlier
messages stay visible, and character information explains the context boundary. Those earlier events
are not silently imported into the new model session. Editing character files
does not change an existing session's frozen SOUL/MEMORY prefix.

See [settings and recovery](settings.md) for native editing and safe writes,
[context budgets](context.md) for connection limits, and [durable history](history.md)
for storage and replay rules.
